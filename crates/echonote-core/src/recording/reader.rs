use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use echonote_opus::Decoder;

use super::ogg::{FLAG_EOS, Page, read_page};
use super::{GRANULE_PER_SAMPLE, RecordingError};
use crate::SAMPLE_RATE;

/// Decoding from the middle of a stream starts this early so the decoder state converges
/// (RFC 7845 section 4.6 recommends at least 80 ms).
const PREROLL: u64 = SAMPLE_RATE as u64 * 80 / 1_000;
/// Room for the longest Opus packet: 120 ms.
const MAX_PACKET_SAMPLES: usize = SAMPLE_RATE as usize * 120 / 1_000;

/// What a recording file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// Samples that decode to the track timeline.
    pub samples: u64,
    /// Bytes up to the end of the last whole page; anything after it is a torn write.
    pub valid_len: u64,
    pub file_len: u64,
    /// The writer finished (the last page has the end-of-stream flag).
    pub finished: bool,
}

impl Probe {
    pub fn duration_ms(&self) -> u64 {
        crate::samples_to_ms(self.samples)
    }
}

struct Headers {
    serial: u32,
    /// Pre-skip in 16 kHz samples.
    delay: u64,
    len: u64,
}

/// Read and check the OpusHead and OpusTags pages.
fn read_headers(input: &mut impl Read, page: &mut Page) -> Result<Headers, RecordingError> {
    let format = RecordingError::Format;
    if !read_page(input, page)? {
        return Err(format("no header page"));
    }
    let head = page.packets().next().unwrap_or_default();
    if head.len() < 19 || &head[..8] != b"OpusHead" || head[8] != 1 {
        return Err(format("bad OpusHead"));
    }
    if head[9] != 1 || head[18] != 0 {
        return Err(format("not a mono recording"));
    }
    let pre_skip = u64::from(u16::from_le_bytes([head[10], head[11]]));
    let serial = page.serial;
    let mut len = page.len;
    if !read_page(input, page)? || page.serial != serial {
        return Err(format("no tags page"));
    }
    if !page
        .packets()
        .next()
        .unwrap_or_default()
        .starts_with(b"OpusTags")
    {
        return Err(format("bad OpusTags"));
    }
    len += page.len;
    Ok(Headers {
        serial,
        delay: pre_skip / GRANULE_PER_SAMPLE,
        len,
    })
}

/// Measure a recording after a crash without decoding it.
pub fn probe(path: &Path) -> Result<Probe, RecordingError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut input = BufReader::with_capacity(64 << 10, file);
    let mut page = Page::default();
    let headers = read_headers(&mut input, &mut page)?;
    let (mut valid_len, mut granule, mut finished) = (headers.len, 0, false);
    while !finished && read_page(&mut input, &mut page)? && page.serial == headers.serial {
        valid_len += page.len;
        granule = page.granule;
        finished = page.flags & FLAG_EOS != 0;
    }
    Ok(Probe {
        samples: (granule / GRANULE_PER_SAMPLE).saturating_sub(headers.delay),
        valid_len,
        file_len,
        finished,
    })
}

/// Cut a torn last page off so the file ends on a whole page, and return the new probe.
pub fn repair(path: &Path) -> Result<Probe, RecordingError> {
    let probe = probe(path)?;
    if probe.valid_len == probe.file_len {
        return Ok(probe);
    }
    let file = File::options().write(true).open(path)?;
    file.set_len(probe.valid_len)?;
    file.sync_all()?;
    Ok(Probe {
        file_len: probe.valid_len,
        ..probe
    })
}

/// Decodes a recording from any position, a page at a time.
pub struct Reader {
    input: BufReader<File>,
    page: Page,
    decoder: Decoder,
    serial: u32,
    /// Decoded samples (pre-skip included) up to the end of the last page read.
    decoded_end: u64,
    /// Decoded samples before this index are dropped: the pre-skip and the preroll. Decoded
    /// sample `i` is timeline sample `i - pre-skip`.
    skip_until: u64,
    pcm: Vec<f32>,
    cursor: usize,
    done: bool,
}

impl Reader {
    /// Open `path` positioned at timeline sample `start`.
    pub fn open_at(path: &Path, start: u64) -> Result<Self, RecordingError> {
        let mut input = BufReader::with_capacity(64 << 10, File::open(path)?);
        let mut page = Page::default();
        let headers = read_headers(&mut input, &mut page)?;
        let mut reader = Self {
            input,
            page,
            decoder: Decoder::new(SAMPLE_RATE)?,
            serial: headers.serial,
            decoded_end: 0,
            skip_until: start + headers.delay,
            pcm: Vec::new(),
            cursor: 0,
            done: false,
        };
        // Skip pages that end before the preroll; decode from the page that holds it.
        let warm_up = reader.skip_until.saturating_sub(PREROLL);
        while let Some(page_start) = reader.next_page()? {
            if reader.decoded_end > warm_up {
                reader.decode_page(page_start)?;
                break;
            }
        }
        Ok(reader)
    }

    /// Fill `out` with the next samples; returns how many were written (0 at the end).
    pub fn read(&mut self, out: &mut [f32]) -> Result<usize, RecordingError> {
        let mut written = 0;
        while written < out.len() {
            if self.cursor == self.pcm.len() {
                match self.next_page()? {
                    Some(page_start) => self.decode_page(page_start)?,
                    None => break,
                }
                continue;
            }
            let take = (self.pcm.len() - self.cursor).min(out.len() - written);
            out[written..written + take]
                .copy_from_slice(&self.pcm[self.cursor..self.cursor + take]);
            self.cursor += take;
            written += take;
        }
        Ok(written)
    }

    /// Read the next page of this stream; returns the decoded index it starts at.
    fn next_page(&mut self) -> Result<Option<u64>, RecordingError> {
        if self.done
            || !read_page(&mut self.input, &mut self.page)?
            || self.page.serial != self.serial
        {
            self.done = true;
            return Ok(None);
        }
        self.done = self.page.flags & FLAG_EOS != 0;
        let start = self.decoded_end;
        self.decoded_end = start.max(self.page.granule / GRANULE_PER_SAMPLE);
        Ok(Some(start))
    }

    /// Decode the current page into `pcm`, trimmed to its granule range and the skip point.
    fn decode_page(&mut self, page_start: u64) -> Result<(), RecordingError> {
        self.pcm.clear();
        let mut frame = [0f32; MAX_PACKET_SAMPLES];
        for packet in self.page.packets() {
            let n = self.decoder.decode(packet, &mut frame)?;
            self.pcm.extend_from_slice(&frame[..n]);
        }
        // The last page's granule trims the padding of the final frame.
        self.pcm.truncate((self.decoded_end - page_start) as usize);
        self.cursor = (self.skip_until.saturating_sub(page_start) as usize).min(self.pcm.len());
        Ok(())
    }
}

/// Decode timeline samples `start..end` (fewer if the file ends first).
pub fn decode_range(path: &Path, start: u64, end: u64) -> Result<Vec<f32>, RecordingError> {
    let mut reader = Reader::open_at(path, start)?;
    let wanted = end.saturating_sub(start) as usize;
    let mut out = Vec::new();
    while out.len() < wanted {
        let filled = out.len();
        out.resize((filled + (64 << 10)).min(wanted), 0.0);
        let n = reader.read(&mut out[filled..])?;
        out.truncate(filled + n);
        if n == 0 {
            break;
        }
    }
    Ok(out)
}
