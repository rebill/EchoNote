//! Per-track recording files: Ogg/Opus, 16 kHz mono, 20 ms frames (RFC 7845).
//!
//! The capture thread pushes 16 kHz samples into a [`Writer`]. About once a second the
//! encoded packets go out as one Ogg page in a single unbuffered write, so an app crash loses
//! at most the page being filled. A write survives an app crash but not power loss; the
//! [`Syncer`] thread flushes the files to the disk every 10 s (`F_FULLFSYNC` on macOS) so the
//! capture thread never waits on the disk.
//!
//! Positions are samples on the track's 16 kHz timeline: sample `n` of the file is sample `n`
//! pushed into the writer. The encoder delay is stored as the Ogg pre-skip and removed when
//! decoding, and the last page's granule position trims the padding of the final frame.
//!
//! After a crash, [`probe`] reads how much audio the file holds (pages are CRC-checked and a
//! torn last page is ignored), [`repair`] cuts the torn tail off, and [`Reader`] decodes from
//! any position so the untranscribed tail can be recovered.

mod ogg;
mod reader;
mod sync;
#[cfg(test)]
mod tests;
mod writer;

pub use reader::{Probe, Reader, decode_range, probe, repair};
pub use sync::{Syncer, sync_dir};
pub use writer::{Writer, WriterConfig};

/// Samples per 20 ms frame at 16 kHz.
pub const FRAME: usize = 320;
/// Ogg/Opus granule positions count 48 kHz samples whatever the coded rate.
const GRANULE_PER_SAMPLE: u64 = 48_000 / crate::SAMPLE_RATE as u64;

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Opus(#[from] echonote_opus::Error),
    #[error("not an EchoNote Ogg/Opus recording: {0}")]
    Format(&'static str),
}
