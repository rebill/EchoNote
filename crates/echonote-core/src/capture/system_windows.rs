//! System audio on Windows through WASAPI loopback, on a thread of its own.
//!
//! - Windows 11 (build 20348+): process loopback of everything except EchoNote's own process
//!   tree, so playback inside the app is not recorded. It follows the output device by itself.
//! - Otherwise: loopback of the default render endpoint, plus a silent render stream on the
//!   same endpoint so loopback keeps delivering packets (on the device clock) while nothing
//!   plays. The default endpoint is checked about once a second; when it changes the source
//!   reports itself failed and the session opens a new one.
//!
//! `ECHONOTE_LOOPBACK=endpoint` forces the second mode.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{GUID, Interface, PWSTR, Ref, implement};

use super::ring::{RingConsumer, RingProducer, now_ns, ring};
use super::{AudioSource, CaptureError, RING_SECONDS, SourceFormat, Stamp};

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// 200 ms shared-mode buffers, in 100 ns units.
const BUFFER_HNS: i64 = 2_000_000;
const ENDPOINT_POLL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sample {
    F32,
    I16,
    I32,
}

pub struct SystemSource {
    consumer: RingConsumer,
    format: SourceFormat,
    name: String,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Opened {
    format: SourceFormat,
    name: String,
    consumer: RingConsumer,
}

impl SystemSource {
    pub fn start() -> Result<Self, CaptureError> {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (ready, opened) = mpsc::channel::<Result<Opened, CaptureError>>();
        let thread = {
            let (stop, failed) = (stop.clone(), failed.clone());
            std::thread::Builder::new()
                .name("wasapi-loopback".into())
                .spawn(move || {
                    // SAFETY: COM is initialized for this thread only and uninitialized after
                    // every COM object of the thread has been dropped inside `run`.
                    let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                    if init.is_ok() {
                        run(&stop, &failed, &ready);
                        unsafe { CoUninitialize() };
                    } else {
                        let _ = ready.send(Err(os("CoInitializeEx", init.0)));
                    }
                })
                .map_err(|error| CaptureError::Device(error.to_string()))?
        };
        match opened.recv() {
            Ok(Ok(opened)) => Ok(Self {
                consumer: opened.consumer,
                format: opened.format,
                name: opened.name,
                stop,
                failed,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::Device("loopback thread exited".into()))
            }
        }
    }
}

impl AudioSource for SystemSource {
    fn format(&self) -> SourceFormat {
        self.format
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn drain(&mut self, samples: &mut Vec<f32>, stamps: &mut Vec<Stamp>) {
        self.consumer.drain(samples, stamps);
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn dropped(&self) -> u64 {
        self.consumer.dropped()
    }
}

impl Drop for SystemSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn os(what: &'static str, status: i32) -> CaptureError {
    CaptureError::Os { what, status }
}

fn win(what: &'static str) -> impl FnOnce(windows::core::Error) -> CaptureError {
    move |error| os(what, error.code().0)
}

/// One open capture stream.
struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    sample: Sample,
    format: SourceFormat,
    name: String,
    event: Option<HANDLE>,
    /// Endpoint mode: the silent render stream and the endpoint id to watch.
    render: Option<(IAudioClient, IAudioRenderClient, u32)>,
    endpoint: Option<(IMMDeviceEnumerator, String)>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            if let Some((render, _, _)) = &self.render {
                let _ = render.Stop();
            }
            if let Some(event) = self.event {
                let _ = CloseHandle(event);
            }
        }
    }
}

fn run(stop: &AtomicBool, failed: &AtomicBool, ready: &mpsc::Sender<Result<Opened, CaptureError>>) {
    let forced_endpoint = std::env::var("ECHONOTE_LOOPBACK").is_ok_and(|mode| mode == "endpoint");
    let opened = if forced_endpoint {
        open_endpoint()
    } else {
        open_process().or_else(|_| open_endpoint())
    };
    let stream = match opened {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let (mut producer, consumer) = ring(stream.format.rate, stream.format.channels, RING_SECONDS);
    let opened = Opened {
        format: stream.format,
        name: stream.name.clone(),
        consumer,
    };
    if ready.send(Ok(opened)).is_err() {
        return;
    }
    if let Err(error) = capture_loop(&stream, &mut producer, stop) {
        let _ = error;
        failed.store(true, Ordering::Release);
    }
}

fn capture_loop(
    stream: &Stream,
    producer: &mut RingProducer,
    stop: &AtomicBool,
) -> Result<(), CaptureError> {
    let clock = Qpc::new();
    let mut checked = Instant::now();
    unsafe {
        if let Some((render, client, frames)) = &stream.render {
            fill_silence(render, client, *frames)?;
            render
                .Start()
                .map_err(win("starting the silent render stream"))?;
        }
        stream
            .client
            .Start()
            .map_err(win("starting loopback capture"))?;
    }
    while !stop.load(Ordering::Relaxed) {
        match stream.event {
            Some(event) => {
                unsafe { WaitForSingleObject(event, 20) };
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
        unsafe {
            if let Some((render, client, frames)) = &stream.render {
                fill_silence(render, client, *frames)?;
            }
            read_packets(stream, producer, &clock)?;
        }
        if let Some((enumerator, id)) = &stream.endpoint {
            if checked.elapsed() >= ENDPOINT_POLL {
                checked = Instant::now();
                let current = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
                    .ok()
                    .and_then(|device| endpoint_id(&device));
                if current.as_deref() != Some(id.as_str()) {
                    return Err(CaptureError::Device("default output device changed".into()));
                }
            }
        }
    }
    Ok(())
}

/// Move every available capture packet into the ring.
unsafe fn read_packets(
    stream: &Stream,
    producer: &mut RingProducer,
    clock: &Qpc,
) -> Result<(), CaptureError> {
    let channels = stream.format.channels;
    loop {
        let next = unsafe { stream.capture.GetNextPacketSize() }
            .map_err(win("reading the loopback packet size"))?;
        if next == 0 {
            return Ok(());
        }
        let mut data = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        let mut qpc = 0u64;
        unsafe {
            stream
                .capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))
        }
        .map_err(win("reading loopback audio"))?;
        let now = clock.now_hns();
        let age_ns = if qpc > 0 && now > qpc {
            (now - qpc) * 100
        } else {
            0
        };
        let at_ns = now_ns().saturating_sub(age_ns);
        let len = frames as usize * channels;
        if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
            producer.push_iter(len, std::iter::repeat_n(0.0, len), at_ns);
        } else {
            // SAFETY: WASAPI guarantees `frames` frames of the stream format at `data` until
            // ReleaseBuffer.
            match stream.sample {
                Sample::F32 => {
                    let samples = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), len) };
                    producer.push(samples, at_ns);
                }
                Sample::I16 => {
                    let samples = unsafe { std::slice::from_raw_parts(data.cast::<i16>(), len) };
                    let scale = 1.0 / 32_768.0;
                    producer.push_iter(len, samples.iter().map(|&s| f32::from(s) * scale), at_ns);
                }
                Sample::I32 => {
                    let samples = unsafe { std::slice::from_raw_parts(data.cast::<i32>(), len) };
                    let scale = 1.0 / 2_147_483_648.0;
                    producer.push_iter(len, samples.iter().map(|&s| s as f32 * scale), at_ns);
                }
            }
        }
        unsafe { stream.capture.ReleaseBuffer(frames) }.map_err(win("releasing loopback audio"))?;
    }
}

/// Top the silent render stream up to its full buffer.
unsafe fn fill_silence(
    render: &IAudioClient,
    client: &IAudioRenderClient,
    buffer_frames: u32,
) -> Result<(), CaptureError> {
    let padding = unsafe { render.GetCurrentPadding() }.map_err(win("render padding"))?;
    let free = buffer_frames.saturating_sub(padding);
    if free > 0 {
        unsafe {
            client.GetBuffer(free).map_err(win("render buffer"))?;
            client
                .ReleaseBuffer(free, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)
                .map_err(win("render release"))?;
        }
    }
    Ok(())
}

/// QPC in 100 ns units, the clock of WASAPI's `pu64QPCPosition`.
struct Qpc {
    frequency: u64,
}

impl Qpc {
    fn new() -> Self {
        let mut frequency = 0i64;
        let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
        Self {
            frequency: frequency.max(1) as u64,
        }
    }

    fn now_hns(&self) -> u64 {
        let mut counter = 0i64;
        let _ = unsafe { QueryPerformanceCounter(&mut counter) };
        (counter.max(0) as u128 * 10_000_000 / u128::from(self.frequency)) as u64
    }
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let id: PWSTR = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        text
    }
}

/// Default render endpoint loopback with a silent render stream on the same endpoint.
fn open_endpoint() -> Result<Stream, CaptureError> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(win("creating the device enumerator"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|_| CaptureError::NoDevice("output"))?;
        let id = endpoint_id(&device).unwrap_or_default();

        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(win("activating loopback capture"))?;
        let mix = client
            .GetMixFormat()
            .map_err(win("reading the mix format"))?;
        let parsed = parse_format(mix);
        let initialized = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            BUFFER_HNS,
            0,
            mix,
            None,
        );
        CoTaskMemFree(Some(mix as *const _));
        let (sample, format) = parsed?;
        initialized.map_err(win("initializing loopback capture"))?;
        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(win("loopback capture client"))?;

        let render: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(win("activating the silent render stream"))?;
        let render_mix = render
            .GetMixFormat()
            .map_err(win("reading the mix format"))?;
        let initialized =
            render.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER_HNS, 0, render_mix, None);
        CoTaskMemFree(Some(render_mix as *const _));
        initialized.map_err(win("initializing the silent render stream"))?;
        let frames = render.GetBufferSize().map_err(win("render buffer size"))?;
        let render_client: IAudioRenderClient =
            render.GetService().map_err(win("render client"))?;

        Ok(Stream {
            client,
            capture,
            sample,
            format,
            name: "WASAPI loopback (default output)".into(),
            event: None,
            render: Some((render, render_client, frames)),
            endpoint: Some((enumerator, id)),
        })
    }
}

/// Process loopback of everything except this process tree (Windows 11).
fn open_process() -> Result<Stream, CaptureError> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: unsafe { GetCurrentProcessId() },
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let blob = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: (&raw mut params).cast(),
                    },
                },
            }),
        },
    };
    let (done, finished) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated {
        done: std::sync::Mutex::new(Some(done)),
    }
    .into();
    unsafe {
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&blob),
            &handler,
        )
        .map_err(win("activating process loopback"))?;
        finished
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| CaptureError::Device("process loopback activation timed out".into()))?;
        let mut result = windows::core::HRESULT(0);
        let mut activated = None;
        operation
            .GetActivateResult(&mut result, &mut activated)
            .map_err(win("process loopback result"))?;
        result.ok().map_err(win("activating process loopback"))?;
        let client: IAudioClient = activated
            .ok_or(CaptureError::Device(
                "process loopback returned nothing".into(),
            ))?
            .cast()
            .map_err(win("process loopback client"))?;

        // Process loopback has no mix format; ask for 48 kHz stereo 16-bit PCM and let the
        // engine convert.
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM,
            nChannels: 2,
            nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 48_000 * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                BUFFER_HNS,
                0,
                &format,
                None,
            )
            .map_err(win("initializing process loopback"))?;
        let event = CreateEventW(None, false, false, None).map_err(win("creating an event"))?;
        if let Err(error) = client.SetEventHandle(event) {
            let _ = CloseHandle(event);
            return Err(win("setting the loopback event")(error));
        }
        let capture: IAudioCaptureClient = match client.GetService() {
            Ok(capture) => capture,
            Err(error) => {
                let _ = CloseHandle(event);
                return Err(win("process loopback capture client")(error));
            }
        };
        Ok(Stream {
            client,
            capture,
            sample: Sample::I16,
            format: SourceFormat {
                rate: 48_000,
                channels: 2,
            },
            name: "WASAPI process loopback".into(),
            event: Some(event),
            render: None,
            endpoint: None,
        })
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated {
    done: std::sync::Mutex<Option<mpsc::Sender<()>>>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        if let Some(done) = self.done.lock().ok().and_then(|mut done| done.take()) {
            let _ = done.send(());
        }
        Ok(())
    }
}

/// Sample type and format of a shared-mode mix format.
unsafe fn parse_format(
    format: *const WAVEFORMATEX,
) -> Result<(Sample, SourceFormat), CaptureError> {
    // SAFETY: `format` comes from GetMixFormat; WAVEFORMATEX is packed, so read unaligned.
    let base = unsafe { std::ptr::read_unaligned(format) };
    let tag = base.wFormatTag;
    let bits = base.wBitsPerSample;
    let sample = match tag {
        WAVE_FORMAT_IEEE_FLOAT if bits == 32 => Sample::F32,
        WAVE_FORMAT_PCM if bits == 16 => Sample::I16,
        WAVE_FORMAT_EXTENSIBLE => {
            let extensible =
                unsafe { std::ptr::read_unaligned(format.cast::<WAVEFORMATEXTENSIBLE>()) };
            let sub = extensible.SubFormat;
            match (sub, bits) {
                (sub, 32) if sub == SUBTYPE_IEEE_FLOAT => Sample::F32,
                (sub, 16) if sub == SUBTYPE_PCM => Sample::I16,
                (sub, 32) if sub == SUBTYPE_PCM => Sample::I32,
                _ => {
                    return Err(CaptureError::Format(format!(
                        "mix format {bits}-bit {sub:?}"
                    )));
                }
            }
        }
        _ => {
            return Err(CaptureError::Format(format!(
                "mix format tag {tag} {bits}-bit"
            )));
        }
    };
    let rate = base.nSamplesPerSec;
    let channels = usize::from(base.nChannels.max(1));
    Ok((sample, SourceFormat { rate, channels }))
}
