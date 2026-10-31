//! Vendored libopus (see `opus/VENDOR.txt`) with the small slice of its API EchoNote uses:
//! a mono floating-point encoder and decoder. Ogg framing lives in `echonote-core`.
//!
//! Both states are allocated once and then encode or decode without allocating, so they can
//! run on the capture thread.

use std::ffi::{CStr, c_char, c_int};
use std::fmt;
use std::ptr::NonNull;

#[repr(C)]
struct OpusEncoder {
    _private: [u8; 0],
}

#[repr(C)]
struct OpusDecoder {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn opus_encoder_create(
        fs: i32,
        channels: c_int,
        application: c_int,
        error: *mut c_int,
    ) -> *mut OpusEncoder;
    fn opus_encoder_destroy(st: *mut OpusEncoder);
    fn opus_encoder_ctl(st: *mut OpusEncoder, request: c_int, ...) -> c_int;
    fn opus_encode_float(
        st: *mut OpusEncoder,
        pcm: *const f32,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> i32;
    fn opus_decoder_create(fs: i32, channels: c_int, error: *mut c_int) -> *mut OpusDecoder;
    fn opus_decoder_destroy(st: *mut OpusDecoder);
    fn opus_decode_float(
        st: *mut OpusDecoder,
        data: *const u8,
        len: i32,
        pcm: *mut f32,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
    fn opus_strerror(error: c_int) -> *const c_char;
    fn opus_get_version_string() -> *const c_char;
}

const OPUS_OK: c_int = 0;
const OPUS_BAD_ARG: c_int = -1;
const OPUS_APPLICATION_VOIP: c_int = 2048;
const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
const OPUS_SET_VBR_REQUEST: c_int = 4006;
const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;
const OPUS_SET_VBR_CONSTRAINT_REQUEST: c_int = 4020;
const OPUS_GET_LOOKAHEAD_REQUEST: c_int = 4027;

/// A libopus error code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error(c_int);

impl Error {
    pub fn code(self) -> i32 {
        self.0
    }

    fn check(code: c_int) -> Result<c_int, Self> {
        if code < OPUS_OK {
            Err(Self(code))
        } else {
            Ok(code)
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: opus_strerror returns a static NUL-terminated string for any code.
        let text = unsafe { CStr::from_ptr(opus_strerror(self.0)) };
        write!(f, "opus: {} ({})", text.to_string_lossy(), self.0)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {}

/// `libopus 1.6.1`, as written into the Ogg vendor tag.
pub fn version() -> &'static str {
    // SAFETY: a static NUL-terminated ASCII string.
    unsafe { CStr::from_ptr(opus_get_version_string()) }
        .to_str()
        .unwrap_or("libopus")
}

/// Mono float encoder in the VoIP application mode (tuned for speech intelligibility).
pub struct Encoder {
    st: NonNull<OpusEncoder>,
}

// SAFETY: the state is plain memory owned by this value; every call takes `&mut self`.
unsafe impl Send for Encoder {}

impl Encoder {
    /// `sample_rate` must be one of 8000, 12000, 16000, 24000 or 48000.
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        let fs = i32::try_from(sample_rate).map_err(|_| Error(OPUS_BAD_ARG))?;
        let mut error = OPUS_OK;
        // SAFETY: `error` outlives the call; a null result comes with an error code.
        let st = unsafe { opus_encoder_create(fs, 1, OPUS_APPLICATION_VOIP, &mut error) };
        Error::check(error)?;
        NonNull::new(st).map(|st| Self { st }).ok_or(Error(error))
    }

    fn set(&mut self, request: c_int, value: i32) -> Result<(), Error> {
        // SAFETY: every SET request used here takes one opus_int32 argument.
        Error::check(unsafe { opus_encoder_ctl(self.st.as_ptr(), request, value) }).map(drop)
    }

    pub fn set_bitrate(&mut self, bits_per_second: i32) -> Result<(), Error> {
        self.set(OPUS_SET_BITRATE_REQUEST, bits_per_second)
    }

    /// 0 (fastest) to 10 (default, best quality).
    pub fn set_complexity(&mut self, complexity: i32) -> Result<(), Error> {
        self.set(OPUS_SET_COMPLEXITY_REQUEST, complexity)
    }

    /// Unconstrained VBR when `constrained` is false: the bitrate follows the signal, which
    /// suits storage (there is no channel to protect).
    pub fn set_vbr(&mut self, vbr: bool, constrained: bool) -> Result<(), Error> {
        self.set(OPUS_SET_VBR_REQUEST, vbr.into())?;
        self.set(OPUS_SET_VBR_CONSTRAINT_REQUEST, constrained.into())
    }

    /// Encoder delay in samples at the encoder's rate; the Ogg pre-skip is derived from it.
    pub fn lookahead(&mut self) -> Result<u32, Error> {
        let mut value: i32 = 0;
        // SAFETY: GET_LOOKAHEAD writes one opus_int32 through the pointer.
        Error::check(unsafe {
            opus_encoder_ctl(
                self.st.as_ptr(),
                OPUS_GET_LOOKAHEAD_REQUEST,
                &mut value as *mut i32,
            )
        })?;
        Ok(value as u32)
    }

    /// Encode one frame (`pcm.len()` must be a valid Opus frame size, e.g. 320 samples for
    /// 20 ms at 16 kHz) into `packet`; returns the packet length in bytes.
    pub fn encode(&mut self, pcm: &[f32], packet: &mut [u8]) -> Result<usize, Error> {
        let frame = c_int::try_from(pcm.len()).map_err(|_| Error(OPUS_BAD_ARG))?;
        let capacity = i32::try_from(packet.len()).unwrap_or(i32::MAX);
        // SAFETY: the buffers are valid for the lengths passed.
        let written = unsafe {
            opus_encode_float(
                self.st.as_ptr(),
                pcm.as_ptr(),
                frame,
                packet.as_mut_ptr(),
                capacity,
            )
        };
        Error::check(written).map(|n| n as usize)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_encoder_create and destroyed once.
        unsafe { opus_encoder_destroy(self.st.as_ptr()) }
    }
}

/// Mono float decoder.
pub struct Decoder {
    st: NonNull<OpusDecoder>,
}

// SAFETY: as for `Encoder`.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        let fs = i32::try_from(sample_rate).map_err(|_| Error(OPUS_BAD_ARG))?;
        let mut error = OPUS_OK;
        // SAFETY: as for `Encoder::new`.
        let st = unsafe { opus_decoder_create(fs, 1, &mut error) };
        Error::check(error)?;
        NonNull::new(st).map(|st| Self { st }).ok_or(Error(error))
    }

    /// Decode one packet into `pcm` (room for the largest frame: 120 ms); returns the number
    /// of samples written.
    pub fn decode(&mut self, packet: &[u8], pcm: &mut [f32]) -> Result<usize, Error> {
        let len = i32::try_from(packet.len()).map_err(|_| Error(OPUS_BAD_ARG))?;
        let frame = c_int::try_from(pcm.len()).unwrap_or(c_int::MAX);
        // SAFETY: the buffers are valid for the lengths passed.
        let decoded = unsafe {
            opus_decode_float(
                self.st.as_ptr(),
                packet.as_ptr(),
                len,
                pcm.as_mut_ptr(),
                frame,
                0,
            )
        };
        Error::check(decoded).map(|n| n as usize)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create and destroyed once.
        unsafe { opus_decoder_destroy(self.st.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;
    const FRAME: usize = 320;

    /// A voiced, speech-like test signal: 140 Hz pulse train through two formant resonators.
    fn voiced(samples: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(samples);
        let (mut y1, mut y2, mut z1, mut z2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let resonator = |hz: f32, r: f32| {
            let w = 2.0 * std::f32::consts::PI * hz / RATE as f32;
            (2.0 * r * w.cos(), -r * r)
        };
        let (a1, a2) = resonator(700.0, 0.97);
        let (b1, b2) = resonator(1_200.0, 0.95);
        for i in 0..samples {
            let pulse = if i % (RATE as usize / 140) == 0 {
                1.0
            } else {
                0.0
            };
            let y = pulse + a1 * y1 + a2 * y2;
            (y2, y1) = (y1, y);
            let z = y + b1 * z1 + b2 * z2;
            (z2, z1) = (z1, z);
            out.push(z * 0.02);
        }
        out
    }

    #[test]
    fn round_trip_keeps_the_waveform_after_the_encoder_delay() {
        let mut encoder = Encoder::new(RATE).unwrap();
        encoder.set_bitrate(24_000).unwrap();
        encoder.set_vbr(true, false).unwrap();
        let delay = encoder.lookahead().unwrap() as usize;
        assert!(delay > 0 && delay < FRAME, "lookahead {delay}");
        let mut decoder = Decoder::new(RATE).unwrap();

        let input = voiced(RATE as usize * 2);
        let (mut output, mut bytes) = (Vec::new(), 0);
        let mut packet = [0u8; 1500];
        let mut pcm = [0f32; 1920];
        for frame in input.chunks_exact(FRAME) {
            let len = encoder.encode(frame, &mut packet).unwrap();
            assert!(len > 0);
            bytes += len;
            let n = decoder.decode(&packet[..len], &mut pcm).unwrap();
            assert_eq!(n, FRAME);
            output.extend_from_slice(&pcm[..n]);
        }

        // Correlate input with the delayed output, skipping the first 200 ms of warm-up.
        let skip = RATE as usize / 5;
        let (mut xy, mut xx, mut yy) = (0f64, 0f64, 0f64);
        for (x, y) in input[skip..input.len() - delay]
            .iter()
            .zip(&output[skip + delay..])
        {
            let (x, y) = (*x as f64, *y as f64);
            xy += x * y;
            xx += x * x;
            yy += y * y;
        }
        let correlation = xy / (xx * yy).sqrt();
        assert!(correlation > 0.8, "correlation {correlation:.3}");
        let kbps = bytes as f64 * 8.0 / 2.0 / 1_000.0;
        assert!((4.0..40.0).contains(&kbps), "{kbps:.1} kbps");
    }

    #[test]
    fn invalid_frame_sizes_are_errors() {
        let mut encoder = Encoder::new(RATE).unwrap();
        let mut packet = [0u8; 1500];
        let error = encoder.encode(&[0.0; 100], &mut packet).unwrap_err();
        assert_eq!(error.code(), OPUS_BAD_ARG);
        assert!(error.to_string().contains("invalid argument"), "{error}");
        assert!(Encoder::new(44_100).is_err());
    }

    #[test]
    fn reports_the_vendored_version() {
        assert!(version().starts_with("libopus 1."), "{}", version());
    }
}
