//! Playback as WAV decoded on the fly. WKWebView cannot seek long Ogg/Opus files (it maps time
//! to bytes by average bitrate; a 3-hour file was off by up to 56 minutes) but seeks WAV to the
//! byte. The tracks of a meeting are mixed into one 16 kHz 16-bit mono stream; a byte range
//! maps to a sample range, and only that range is decoded.

use std::path::PathBuf;

use super::{Reader, RecordingError};
use crate::SAMPLE_RATE;

pub const HEADER_LEN: u64 = 44;
const BYTES_PER_SAMPLE: u64 = 2;
/// Largest response: about 65 s of audio. Players ask again for the rest.
pub const MAX_CHUNK: u64 = 2 << 20;

pub struct MixedWav {
    /// Track files; a missing one plays as silence.
    pub tracks: Vec<PathBuf>,
    /// Length of the longest track in samples.
    pub samples: u64,
}

/// A `Range` header that cannot be served (HTTP 416).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsatisfiable;

/// The inclusive byte range `[start, end]` a `Range` header asks for, capped at `MAX_CHUNK`.
/// `None` for a missing header means the whole file.
pub fn parse_range(header: Option<&str>, len: u64) -> Result<(u64, u64), Unsatisfiable> {
    let Some(header) = header else {
        return if len == 0 {
            Err(Unsatisfiable)
        } else {
            Ok((0, (len - 1).min(MAX_CHUNK - 1)))
        };
    };
    let spec = header.trim().strip_prefix("bytes=").ok_or(Unsatisfiable)?;
    // Only the first range of a multi-range request is served.
    let spec = spec.split(',').next().ok_or(Unsatisfiable)?.trim();
    let (from, to) = spec.split_once('-').ok_or(Unsatisfiable)?;
    let (start, end) = match (from.trim(), to.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().map_err(|_| Unsatisfiable)?;
            (len.saturating_sub(n), len.saturating_sub(1))
        }
        (from, "") => (
            from.parse().map_err(|_| Unsatisfiable)?,
            len.saturating_sub(1),
        ),
        (from, to) => (
            from.parse().map_err(|_| Unsatisfiable)?,
            to.parse::<u64>()
                .map_err(|_| Unsatisfiable)?
                .min(len.saturating_sub(1)),
        ),
    };
    if len == 0 || start >= len || end < start {
        return Err(Unsatisfiable);
    }
    Ok((start, end.min(start + MAX_CHUNK - 1)))
}

impl MixedWav {
    pub fn len(&self) -> u64 {
        HEADER_LEN + self.samples * BYTES_PER_SAMPLE
    }

    pub fn is_empty(&self) -> bool {
        self.samples == 0
    }

    pub fn header(&self) -> [u8; HEADER_LEN as usize] {
        let data = (self.samples * BYTES_PER_SAMPLE) as u32;
        let mut header = [0u8; HEADER_LEN as usize];
        let mut put = |at: usize, bytes: &[u8]| header[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, b"RIFF");
        put(4, &(36 + data).to_le_bytes());
        put(8, b"WAVEfmt ");
        put(16, &16u32.to_le_bytes());
        put(20, &1u16.to_le_bytes()); // PCM
        put(22, &1u16.to_le_bytes()); // mono
        put(24, &SAMPLE_RATE.to_le_bytes());
        put(28, &(SAMPLE_RATE * BYTES_PER_SAMPLE as u32).to_le_bytes());
        put(32, &(BYTES_PER_SAMPLE as u16).to_le_bytes());
        put(34, &16u16.to_le_bytes());
        put(36, b"data");
        put(40, &data.to_le_bytes());
        header
    }

    /// Bytes `[start, end]` of the WAV file.
    pub fn read(&self, start: u64, end: u64) -> Result<Vec<u8>, RecordingError> {
        let end = end.min(self.len().saturating_sub(1));
        let mut out = Vec::with_capacity((end + 1).saturating_sub(start) as usize);
        if start < HEADER_LEN {
            let header = self.header();
            out.extend_from_slice(&header[start as usize..=(end.min(HEADER_LEN - 1)) as usize]);
        }
        if end < HEADER_LEN {
            return Ok(out);
        }
        let data_start = start.max(HEADER_LEN) - HEADER_LEN;
        let data_end = end - HEADER_LEN;
        let first = data_start / BYTES_PER_SAMPLE;
        let last = data_end / BYTES_PER_SAMPLE;
        let count = (last - first + 1) as usize;
        let mut mix = vec![0f32; count];
        let mut buffer = vec![0f32; count];
        for path in &self.tracks {
            if !path.is_file() {
                continue;
            }
            let mut reader = Reader::open_at(path, first)?;
            let mut filled = 0;
            while filled < count {
                let n = reader.read(&mut buffer[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            for (sum, sample) in mix.iter_mut().zip(&buffer[..filled]) {
                *sum += sample;
            }
        }
        let mut pcm = Vec::with_capacity(count * 2);
        for sample in mix {
            pcm.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes());
        }
        // The range may start or end in the middle of a sample.
        let skip = (data_start % BYTES_PER_SAMPLE) as usize;
        let take = (data_end - data_start + 1) as usize;
        out.extend_from_slice(&pcm[skip..skip + take]);
        Ok(out)
    }
}
