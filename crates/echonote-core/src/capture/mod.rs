//! Audio capture: sources, the lock-free hand-off from their callbacks, and the per-track
//! timeline that turns them into 16 kHz mono on one clock.
//!
//! Every source implements [`AudioSource`]. Its OS callback (or feeder thread) pushes whole
//! buffers into a [`ring`] with a capture timestamp; the capture thread drains the ring every
//! 20 ms and hands samples and stamps to the track's [`Timeline`]. Sources are created on the
//! capture thread and never leave it, so they need not be `Send`.

mod fake;
#[cfg(target_os = "macos")]
pub mod macos_tap;
mod mic;
pub mod ring;
#[cfg(target_os = "macos")]
mod system_macos;
#[cfg(target_os = "windows")]
mod system_windows;
pub mod timeline;

pub use fake::{FakeSource, Pace, read_wav};
pub use mic::{DeviceInfo, MicSource, input_devices};
pub use ring::{Stamp, now_ns};
pub use timeline::{Timeline, TimelineConfig, TimelineStats};

/// Seconds of audio each source's ring holds; the capture thread drains it every 20 ms.
pub const RING_SECONDS: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceFormat {
    pub rate: u32,
    pub channels: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("{what} failed with OSStatus {status} ({})", fourcc(*status))]
    Os { what: &'static str, status: i32 },
    #[error("no {0} device")]
    NoDevice(&'static str),
    #[error("{0}")]
    Device(String),
    #[error("unsupported sample format {0}")]
    Format(String),
    #[error("microphone access is denied in system settings")]
    Denied,
    #[error(transparent)]
    Wav(#[from] hound::Error),
}

/// A running capture source. Dropping it stops capture.
pub trait AudioSource {
    fn format(&self) -> SourceFormat;
    fn name(&self) -> &str;
    /// Append everything captured since the last call (see [`ring::RingConsumer::drain`]).
    fn drain(&mut self, samples: &mut Vec<f32>, stamps: &mut Vec<Stamp>);
    /// The device went away, changed format, or is no longer the system default: replace
    /// the source.
    fn failed(&self) -> bool {
        false
    }
    /// A finite source (file, test signal) delivered everything and its ring is empty.
    fn finished(&self) -> bool {
        false
    }
    /// Frames lost because the ring was full.
    fn dropped(&self) -> u64;
}

/// The system-audio source of this platform (Core Audio process tap, WASAPI loopback).
pub fn open_system() -> Result<Box<dyn AudioSource>, CaptureError> {
    #[cfg(target_os = "macos")]
    return system_macos::SystemSource::start().map(|s| Box::new(s) as Box<dyn AudioSource>);
    #[cfg(target_os = "windows")]
    return system_windows::SystemSource::start().map(|s| Box::new(s) as Box<dyn AudioSource>);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err(CaptureError::NoDevice("system audio"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Granted,
    Denied,
    /// Not asked yet; opening the device shows the system prompt.
    Undetermined,
}

/// Microphone permission. On Windows a microphone behind the privacy switch opens fine and
/// records silence, so the switch is read from the registry instead (`Undetermined` never
/// occurs there).
pub fn mic_permission() -> Permission {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn echonote_mic_permission() -> i32;
        }
        // SAFETY: a plain query of AVCaptureDevice authorization status.
        match unsafe { echonote_mic_permission() } {
            3 => Permission::Granted,
            0 => Permission::Undetermined,
            _ => Permission::Denied,
        }
    }
    #[cfg(target_os = "windows")]
    return system_windows::mic_privacy();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Permission::Granted
}

/// System audio permission. macOS asks when the first tap is created and a denied tap records
/// silence; `Undetermined` also covers "could not tell". Windows needs no permission.
pub fn sys_permission() -> Permission {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn echonote_audio_capture_permission() -> i32;
        }
        // SAFETY: a TCC preflight query; returns 2 when the private symbol is unavailable.
        match unsafe { echonote_audio_capture_permission() } {
            0 => Permission::Granted,
            1 => Permission::Denied,
            _ => Permission::Undetermined,
        }
    }
    #[cfg(not(target_os = "macos"))]
    Permission::Granted
}

/// Ask for microphone access if not asked yet; blocks until the user answers. Call off the
/// main thread.
pub fn request_mic_permission() -> Permission {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn echonote_mic_request() -> i32;
        }
        if mic_permission() == Permission::Undetermined {
            // SAFETY: blocks on a semaphore until AVFoundation calls back.
            unsafe { echonote_mic_request() };
        }
    }
    mic_permission()
}

/// Keeps the machine from idle-sleeping (and macOS from napping the app) while alive.
pub struct KeepAwake {
    #[cfg(target_os = "macos")]
    token: *mut std::ffi::c_void,
    #[cfg(target_os = "windows")]
    stop: Option<std::sync::mpsc::Sender<()>>,
}

impl KeepAwake {
    pub fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn echonote_keep_awake_begin() -> *mut std::ffi::c_void;
            }
            // SAFETY: returns a retained activity token released in drop.
            Self {
                token: unsafe { echonote_keep_awake_begin() },
            }
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Power::{
                ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
            };
            // The execution state belongs to a thread, so hold it on one that lives as long
            // as this value.
            let (stop, stopped) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                // SAFETY: plain Win32 calls on this thread.
                unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                let _ = stopped.recv();
                unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            });
            Self { stop: Some(stop) }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        Self {}
    }
}

impl Default for KeepAwake {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn echonote_keep_awake_end(token: *mut std::ffi::c_void);
            }
            // SAFETY: `token` came from echonote_keep_awake_begin and is released once.
            unsafe { echonote_keep_awake_end(self.token) };
        }
        #[cfg(target_os = "windows")]
        drop(self.stop.take());
    }
}

/// Render an OSStatus as its four-character code when printable.
fn fourcc(status: i32) -> String {
    let bytes = status.to_be_bytes();
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        "-".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_queries_answer_without_prompting() {
        // Neither query may show a prompt or crash; macOS reads TCC through a private symbol
        // that may be missing, which reports `Undetermined`.
        let mic = mic_permission();
        let sys = sys_permission();
        eprintln!("mic {mic:?}, system audio {sys:?}");
        assert_eq!(sys_permission(), sys, "stable between calls");
    }
}
