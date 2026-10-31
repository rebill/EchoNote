use std::fs::File;
use std::io::Write;
use std::path::Path;

use echonote_opus::Encoder;

use super::ogg::{FLAG_BOS, FLAG_EOS, PageBuilder};
use super::{FRAME, GRANULE_PER_SAMPLE, RecordingError};
use crate::SAMPLE_RATE;

/// Largest packet libopus produces for one 20 ms frame is 1275 bytes plus the TOC.
const MAX_PACKET: usize = 1_500;

#[derive(Clone, Copy, Debug)]
pub struct WriterConfig {
    pub bitrate: i32,
    /// libopus complexity, 0–10.
    pub complexity: i32,
    /// Frames per Ogg page: 50 is one second, the most audio an app crash can lose.
    pub frames_per_page: usize,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            bitrate: 24_000,
            complexity: 5,
            frames_per_page: 50,
        }
    }
}

/// Encodes one track into an Ogg/Opus file. Allocation happens in `create`; `push` only
/// encodes into reused buffers and writes a page about once a second.
pub struct Writer {
    file: File,
    encoder: Encoder,
    config: WriterConfig,
    /// Encoder delay in 16 kHz samples (the pre-skip is three times this).
    delay: u64,
    frame: Vec<f32>,
    packet: Vec<u8>,
    page: PageBuilder,
    frames_in_page: usize,
    /// Frames encoded so far, including the ones waiting in `page`.
    frames: u64,
    /// Frames in pages already written to the file.
    written: u64,
    /// Samples pushed so far.
    samples: u64,
}

impl Writer {
    /// Create `path` (it must not exist) and write the Opus headers.
    pub fn create(path: &Path, config: WriterConfig) -> Result<Self, RecordingError> {
        let mut encoder = Encoder::new(SAMPLE_RATE)?;
        encoder.set_bitrate(config.bitrate)?;
        encoder.set_complexity(config.complexity)?;
        encoder.set_vbr(true, false)?;
        let delay = u64::from(encoder.lookahead()?);
        let pre_skip = u16::try_from(delay * GRANULE_PER_SAMPLE)
            .map_err(|_| RecordingError::Format("encoder delay too long"))?;

        let file = File::options().write(true).create_new(true).open(path)?;
        let mut writer = Self {
            file,
            encoder,
            config,
            delay,
            frame: Vec::with_capacity(FRAME),
            packet: vec![0; MAX_PACKET],
            page: PageBuilder::new(serial()),
            frames_in_page: 0,
            frames: 0,
            written: 0,
            samples: 0,
        };

        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1); // version
        head.push(1); // channels
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // mapping family: mono/stereo
        writer.page.add(&head);
        writer.file.write_all(writer.page.finish(FLAG_BOS, 0))?;

        let vendor = echonote_opus::version().as_bytes();
        let mut tags = Vec::with_capacity(16 + vendor.len());
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes()); // no user comments
        writer.page.add(&tags);
        writer.file.write_all(writer.page.finish(0, 0))?;
        Ok(writer)
    }

    /// Samples pushed so far: the track duration once `finish` has run.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Samples a reader of the file can decode now: those whose frames, plus the encoder
    /// delay, are in written pages.
    pub fn readable(&self) -> u64 {
        (self.written * FRAME as u64)
            .saturating_sub(self.delay)
            .min(self.samples)
    }

    /// Write the page being filled now, so everything encoded so far is readable. Used before
    /// reading recent audio back; the up to 20 ms still in the frame buffer stays there.
    pub fn flush(&mut self) -> Result<(), RecordingError> {
        if self.page.is_empty() {
            return Ok(());
        }
        self.write_page(0)
    }

    /// A second handle to the file for the [`Syncer`](super::Syncer).
    pub fn sync_handle(&self) -> std::io::Result<File> {
        self.file.try_clone()
    }

    /// Append 16 kHz mono samples. After an error the file is valid up to the last whole
    /// page; stop recording this track rather than pushing more.
    pub fn push(&mut self, mut samples: &[f32]) -> Result<(), RecordingError> {
        self.samples += samples.len() as u64;
        while !samples.is_empty() {
            let take = (FRAME - self.frame.len()).min(samples.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == FRAME {
                self.encode_frame()?;
                if self.frames_in_page >= self.config.frames_per_page {
                    self.write_page(0)?;
                }
            }
        }
        Ok(())
    }

    /// Encode the buffered frame into the current page. The page only goes out early when
    /// its segment table is full, so the new packet is always left waiting in it.
    fn encode_frame(&mut self) -> Result<(), RecordingError> {
        let len = self.encoder.encode(&self.frame, &mut self.packet)?;
        self.frame.clear();
        if !self.page.fits(len) {
            self.write_page(0)?;
        }
        self.page.add(&self.packet[..len]);
        self.frames += 1;
        self.frames_in_page += 1;
        Ok(())
    }

    fn write_page(&mut self, flags: u8) -> Result<(), RecordingError> {
        let granule = self.frames * FRAME as u64 * GRANULE_PER_SAMPLE;
        self.write_page_at(flags, granule)
    }

    fn write_page_at(&mut self, flags: u8, granule: u64) -> Result<(), RecordingError> {
        self.frames_in_page = 0;
        self.written = self.frames;
        let page = self.page.finish(flags, granule);
        self.file.write_all(page)?;
        Ok(())
    }

    /// Encode silence until every pushed sample has left the encoder, then write the last
    /// page with the end-of-stream flag. Returns the number of samples recorded. The caller
    /// stops the [`Syncer`](super::Syncer) afterwards for the final flush to disk.
    pub fn finish(mut self) -> Result<u64, RecordingError> {
        while self.frames * (FRAME as u64) < self.samples + self.delay {
            let pad = FRAME - self.frame.len();
            self.frame.extend(std::iter::repeat_n(0.0, pad));
            self.encode_frame()?;
        }
        // The delay is never zero, so the loop encoded at least one frame and it is waiting
        // in this page; earlier pages end before `samples + delay`, so the granule grows.
        debug_assert!(!self.page.is_empty());
        let granule = (self.samples + self.delay) * GRANULE_PER_SAMPLE;
        self.write_page_at(FLAG_EOS, granule)?;
        Ok(self.samples)
    }
}

/// Stream serial number. It only has to differ between streams of one physical file; vary
/// it anyway, as encoders conventionally do.
fn serial() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() ^ elapsed.as_secs() as u32)
        .unwrap_or(0x4543_4e54)
}
