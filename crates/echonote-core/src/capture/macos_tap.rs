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
    unsafe extern "C" fn(context: *mut c_void, samples: *const f32, count: u32, age_ns: u64);

unsafe extern "C" {
    fn echonote_tap_open(status: *mut i32) -> *mut c_void;
    fn echonote_tap_start(tap: *mut c_void, callback: NativeCallback, context: *mut c_void) -> i32;
    fn echonote_tap_get_format(tap: *const c_void) -> TapFormat;
    fn echonote_tap_changed(tap: *mut c_void) -> i32;
    fn echonote_tap_stop(tap: *mut c_void);
}

type Handler = Box<dyn FnMut(&[f32], u64) + Send>;

unsafe extern "C" fn trampoline(
    context: *mut c_void,
    samples: *const f32,
    count: u32,
    age_ns: u64,
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
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(samples, age_ns)));
}

/// A system-audio tap. Dropping it stops capture.
pub struct SystemTap {
    raw: *mut c_void,
    handler: Option<*mut Handler>,
}

// SAFETY: the native handle is only used from the owning thread; Core Audio drives the callback.
unsafe impl Send for SystemTap {}

impl SystemTap {
    /// Create a tap of every process except this one, without starting it.
    pub fn open() -> Result<Self, CaptureError> {
        let mut status = 0i32;
        // SAFETY: plain FFI call; NULL means failure with `status` set.
        let raw = unsafe { echonote_tap_open(&mut status) };
        if raw.is_null() {
            return Err(CaptureError::Os {
                what: "creating the system audio tap",
                status,
            });
        }
        Ok(Self { raw, handler: None })
    }

    /// Start delivering buffers. `handler` receives interleaved samples in the tap format and
    /// how many nanoseconds ago the buffer's first frame was captured.
    pub fn start(
        &mut self,
        handler: impl FnMut(&[f32], u64) + Send + 'static,
    ) -> Result<(), CaptureError> {
        assert!(self.handler.is_none(), "tap already started");
        let boxed: Handler = Box::new(handler);
        let handler: *mut Handler = Box::into_raw(Box::new(boxed));
        self.handler = Some(handler);
        // SAFETY: `trampoline` matches the native callback type and `handler` stays valid
        // until drop, which stops the IOProc before freeing it.
        let status = unsafe { echonote_tap_start(self.raw, trampoline, handler.cast()) };
        if status != 0 {
            return Err(CaptureError::Os {
                what: "starting the system audio tap",
                status,
            });
        }
        Ok(())
    }

    pub fn format(&self) -> TapFormat {
        // SAFETY: `raw` is a live tap until drop.
        unsafe { echonote_tap_get_format(self.raw) }
    }

    /// The default output device or the tap format changed since the tap was created.
    pub fn changed(&self) -> bool {
        // SAFETY: `raw` is a live tap until drop.
        unsafe { echonote_tap_changed(self.raw) != 0 }
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: stopping destroys the IOProc first, so the handler is no longer called when freed.
        unsafe {
            echonote_tap_stop(self.raw);
            if let Some(handler) = self.handler {
                drop(Box::from_raw(handler));
            }
        }
    }
}
