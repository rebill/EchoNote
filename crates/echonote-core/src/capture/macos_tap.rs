//! System audio capture through a Core Audio process tap (macOS 14.2+).
//!
//! The native side lives in `native/macos_tap.m`. The handler runs on Core Audio's real-time
//! I/O thread, so it must not block or allocate; production code pushes into a ring buffer.

use std::ffi::c_void;

use super::CaptureError;

/// Stream format reported by the tap (`kAudioTapPropertyFormat`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TapFormat {
    pub sample_rate: f64,
    pub channels: u32,
    pub format_id: u32,
    pub format_flags: u32,
    pub bits_per_channel: u32,
}

type NativeCallback =
    unsafe extern "C" fn(context: *mut c_void, samples: *const f32, count: u32, host_time_ns: u64);

unsafe extern "C" {
    fn echonote_tap_start(
        callback: NativeCallback,
        context: *mut c_void,
        status: *mut i32,
    ) -> *mut c_void;
    fn echonote_tap_get_format(tap: *const c_void) -> TapFormat;
    fn echonote_tap_stop(tap: *mut c_void);
}

type Handler = Box<dyn FnMut(&[f32], u64) + Send>;

unsafe extern "C" fn trampoline(
    context: *mut c_void,
    samples: *const f32,
    count: u32,
    host_time_ns: u64,
) {
    // SAFETY: `context` is the `Handler` leaked in `SystemTap::start` and outlives the tap.
    let handler = unsafe { &mut *context.cast::<Handler>() };
    let samples = if samples.is_null() || count == 0 {
        &[][..]
    } else {
        // SAFETY: Core Audio guarantees `count` valid floats for the duration of the callback.
        unsafe { std::slice::from_raw_parts(samples, count as usize) }
    };
    // A panic must never unwind into Core Audio.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handler(samples, host_time_ns)
    }));
}

/// A running system-audio tap. Dropping it stops capture.
pub struct SystemTap {
    raw: *mut c_void,
    handler: *mut Handler,
}

// SAFETY: the native handle is only used from the owning thread; Core Audio drives the callback.
unsafe impl Send for SystemTap {}

impl SystemTap {
    /// Start capturing every process except this one. `handler` receives interleaved samples
    /// in the tap format and the buffer's host time in nanoseconds.
    pub fn start(handler: impl FnMut(&[f32], u64) + Send + 'static) -> Result<Self, CaptureError> {
        let boxed: Handler = Box::new(handler);
        let handler: *mut Handler = Box::into_raw(Box::new(boxed));
        let mut status = 0i32;
        // SAFETY: `trampoline` matches the native callback type and `handler` stays valid until drop.
        let raw = unsafe { echonote_tap_start(trampoline, handler.cast(), &mut status) };
        if raw.is_null() {
            // SAFETY: the native side failed before retaining `handler`.
            drop(unsafe { Box::from_raw(handler) });
            return Err(CaptureError::Os {
                what: "starting the system audio tap",
                status,
            });
        }
        Ok(Self { raw, handler })
    }

    pub fn format(&self) -> TapFormat {
        // SAFETY: `raw` is a live tap until drop.
        unsafe { echonote_tap_get_format(self.raw) }
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: stopping destroys the IOProc first, so the handler is no longer called when freed.
        unsafe {
            echonote_tap_stop(self.raw);
            drop(Box::from_raw(self.handler));
        }
    }
}
