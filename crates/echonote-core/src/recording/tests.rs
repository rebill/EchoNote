use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;
use std::time::Duration;

use super::ogg::{FLAG_EOS, Page, read_page};
use super::*;
use crate::test_util::TempDir;

const RATE: usize = crate::SAMPLE_RATE as usize;

/// Speech-like audio: 250 ms syllables of a gliding pulse train through two moving formants,
/// with a pause after each.
fn speech(samples: usize) -> Vec<f32> {
    let rate = RATE as f32;
    let tau = std::f32::consts::TAU;
    let (mut y1, mut y2, mut z1, mut z2, mut phase) = (0f32, 0f32, 0f32, 0f32, 0f32);
    let mut out: Vec<f32> = (0..samples)
        .map(|i| {
            let t = i as f32 / rate;
            let syllable = (t * 4.0).fract();
            let envelope = if syllable < 0.7 {
                (syllable / 0.7 * std::f32::consts::PI).sin()
            } else {
                0.0
            };
            phase += (120.0 + 40.0 * (t * 0.7 * tau).sin()) / rate;
            let pulse = if phase >= 1.0 {
                phase -= 1.0;
                envelope
            } else {
                0.0
            };
            let (f1, f2) = (
                500.0 + 300.0 * (t * 3.0).sin(),
                1_500.0 + 500.0 * (t * 2.3).sin(),
            );
            let (r1, r2) = (0.97f32, 0.95f32);
            let y = pulse + 2.0 * r1 * (tau * f1 / rate).cos() * y1 - r1 * r1 * y2;
            (y2, y1) = (y1, y);
            let z = y + 2.0 * r2 * (tau * f2 / rate).cos() * z1 - r2 * r2 * z2;
            (z2, z1) = (z1, z);
            z
        })
        .collect();
    let peak = out.iter().fold(0f32, |peak, x| peak.max(x.abs()));
    out.iter_mut().for_each(|x| *x *= 0.5 / peak);
    out
}

/// Record `audio` in irregular chunks, as capture delivers it.
fn record(path: &Path, audio: &[f32], config: WriterConfig) -> u64 {
    let mut writer = Writer::create(path, config).unwrap();
    push_all(&mut writer, audio);
    writer.finish().unwrap()
}

fn push_all(writer: &mut Writer, mut audio: &[f32]) {
    let mut size = 37;
    while !audio.is_empty() {
        let take = size.min(audio.len());
        writer.push(&audio[..take]).unwrap();
        audio = &audio[take..];
        size = size * 7 % 1013 + 1;
    }
}

/// Normalized correlation of `a[i]` with `b[i + lag]`.
fn correlation(a: &[f32], b: &[f32], lag: i64) -> f64 {
    let n = a.len().min(b.len()) as i64;
    let (mut xy, mut xx, mut yy) = (0f64, 0f64, 0f64);
    for i in 200..n - 200 {
        let (x, y) = (a[i as usize] as f64, b[(i + lag) as usize] as f64);
        xy += x * y;
        xx += x * x;
        yy += y * y;
    }
    xy / (xx * yy).sqrt()
}

/// The decoded audio lines up with the input. The search covers ±200 samples, so a missing
/// pre-skip (104 samples) or a lost frame (320) would show; SILK's parametric coding moves
/// the correlation peak by a sample or two (0.1 ms), which is allowed.
fn assert_aligned(input: &[f32], decoded: &[f32]) {
    let (lag, best) = (-200..=200)
        .map(|lag| (lag, correlation(input, decoded, lag)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(lag.abs() <= 2, "best lag {lag} (correlation {best:.3})");
    assert!(best > 0.8, "correlation {best:.3}");
}

/// The encoder delay in 16 kHz samples, from the OpusHead pre-skip.
fn delay(path: &Path) -> u64 {
    let head = pages(path).remove(0);
    let head = head.packets().next().unwrap();
    u64::from(u16::from_le_bytes([head[10], head[11]])) / 3
}

fn pages(path: &Path) -> Vec<Page> {
    let mut input = BufReader::new(File::open(path).unwrap());
    let mut pages = Vec::new();
    loop {
        let mut page = Page::default();
        if !read_page(&mut input, &mut page).unwrap() {
            return pages;
        }
        pages.push(page);
    }
}

#[test]
fn round_trip_keeps_length_and_alignment() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 3 + 123);
    assert_eq!(
        record(&path, &audio, WriterConfig::default()),
        audio.len() as u64
    );

    let probe = probe(&path).unwrap();
    assert_eq!(probe.samples, audio.len() as u64);
    assert_eq!(probe.duration_ms(), 3_007);
    assert!(probe.finished);
    assert_eq!(probe.valid_len, probe.file_len);

    let decoded = decode_range(&path, 0, u64::MAX).unwrap();
    assert_eq!(decoded.len(), audio.len());
    assert_aligned(&audio, &decoded);
}

#[test]
fn audio_pages_hold_one_second_each() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 3 + 123);
    record(&path, &audio, WriterConfig::default());

    let pages = pages(&path);
    let (headers, audio_pages) = pages.split_at(2);
    assert!(headers.iter().all(|page| page.granule == 0));
    let (last, full) = audio_pages.split_last().unwrap();
    assert_eq!(full.len(), 3);
    for (index, page) in full.iter().enumerate() {
        assert_eq!(page.packet_count(), 50);
        assert_eq!(page.granule, (index as u64 + 1) * 50 * 960);
        assert_eq!(page.flags & FLAG_EOS, 0);
    }
    assert_eq!(last.flags & FLAG_EOS, FLAG_EOS);
    assert!(last.granule > full[2].granule);
}

#[test]
fn a_full_segment_table_starts_a_new_page() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 12);
    let config = WriterConfig {
        frames_per_page: 10_000,
        ..WriterConfig::default()
    };
    record(&path, &audio, config);
    let pages = pages(&path);
    assert!(pages.len() > 3, "{} pages", pages.len());
    assert!(pages.iter().all(|page| page.packet_count() <= 255));
    let decoded = decode_range(&path, 0, u64::MAX).unwrap();
    assert_eq!(decoded.len(), audio.len());
    assert_aligned(&audio, &decoded);
}

#[test]
fn a_crash_keeps_every_whole_page() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 5 / 2);
    let mut writer = Writer::create(&path, WriterConfig::default()).unwrap();
    push_all(&mut writer, &audio);
    drop(writer); // no finish: the process died

    // 125 frames were encoded; pages went out after 50 and 100. The encoder delay is still
    // in flight at the end of the second page.
    let probe = probe(&path).unwrap();
    assert!(!probe.finished);
    assert_eq!(probe.valid_len, probe.file_len);
    let delay = delay(&path);
    assert!(delay > 0 && delay < FRAME as u64, "delay {delay}");
    assert_eq!(probe.samples, 100 * FRAME as u64 - delay);

    let decoded = decode_range(&path, 0, u64::MAX).unwrap();
    assert_eq!(decoded.len() as u64, probe.samples);
    assert_aligned(&audio[..decoded.len()], &decoded);

    // Killed right after the headers.
    let empty = dir.path().join("sys.opus");
    drop(Writer::create(&empty, WriterConfig::default()).unwrap());
    let probe = super::probe(&empty).unwrap();
    assert_eq!((probe.samples, probe.finished), (0, false));
}

#[test]
fn a_torn_last_page_is_ignored_and_repaired() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 3 + 123);
    record(&path, &audio, WriterConfig::default());
    let whole = probe(&path).unwrap();
    let last_page = pages(&path).last().unwrap().len;
    let bytes = std::fs::read(&path).unwrap();

    // Cut inside the last page, and separately corrupt one of its bytes.
    let truncated = dir.path().join("truncated.opus");
    std::fs::write(&truncated, &bytes[..bytes.len() - 10]).unwrap();
    let corrupted = dir.path().join("corrupted.opus");
    let mut flipped = bytes.clone();
    let middle = bytes.len() - last_page as usize / 2;
    flipped[middle] ^= 0x40;
    std::fs::write(&corrupted, &flipped).unwrap();

    for torn in [&truncated, &corrupted] {
        let probe = probe(torn).unwrap();
        assert!(!probe.finished);
        assert_eq!(probe.valid_len, whole.file_len - last_page);
        // The lost page held frame 151; the third page ends at frame 150.
        assert_eq!(
            probe.samples,
            150 * FRAME as u64 - delay(&path),
            "{probe:?}"
        );

        let repaired = repair(torn).unwrap();
        assert_eq!(repaired.file_len, probe.valid_len);
        assert_eq!(std::fs::metadata(torn).unwrap().len(), probe.valid_len);
        assert_eq!(super::probe(torn).unwrap(), repaired);
        let decoded = decode_range(torn, 0, u64::MAX).unwrap();
        assert_eq!(decoded.len() as u64, probe.samples);
        assert_aligned(&audio[..decoded.len()], &decoded);
    }
}

#[test]
fn decoding_from_the_middle_matches_a_full_decode() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    let audio = speech(RATE * 3 + 123);
    record(&path, &audio, WriterConfig::default());
    let full = decode_range(&path, 0, u64::MAX).unwrap();

    // Pages end every 16,000 decoded samples. 33,176 starts decoding at the third page with
    // exactly the 80 ms preroll; 40,000 and 47,000 with more.
    let shortest_preroll = 2 * 16_000 + 1_280 - delay(&path) as usize;
    for start in [0, 1, 319, 16_000, 16_123, shortest_preroll, 40_000, 47_000] {
        let part = decode_range(&path, start as u64, start as u64 + 5_000).unwrap();
        let expected = &full[start..(start + 5_000).min(full.len())];
        assert_eq!(part.len(), expected.len(), "start {start}");
        let error: f64 = part
            .iter()
            .zip(expected)
            .map(|(a, b)| ((a - b) as f64).powi(2))
            .sum();
        let energy: f64 = expected.iter().map(|b| (*b as f64).powi(2)).sum();
        eprintln!(
            "start {start}: error/energy {:.2e}",
            error / energy.max(1e-12)
        );
        assert!(
            error <= energy * 1e-3,
            "start {start}: {error:.3e} vs {energy:.3e}"
        );
    }
    let past_end = full.len() as u64 + 100;
    assert!(
        decode_range(&path, past_end, past_end + 100)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_empty_recording_is_valid() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    assert_eq!(record(&path, &[], WriterConfig::default()), 0);
    let probe = probe(&path).unwrap();
    assert_eq!((probe.samples, probe.finished), (0, true));
    assert!(decode_range(&path, 0, u64::MAX).unwrap().is_empty());
}

#[test]
fn other_files_are_rejected() {
    let dir = TempDir::new();
    let path = dir.path().join("mic.opus");
    for bytes in [&b""[..], b"not an ogg file at all, just some text"] {
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(probe(&path), Err(RecordingError::Format(_))));
    }
    let error = Writer::create(&path, WriterConfig::default())
        .err()
        .unwrap();
    assert!(
        matches!(&error, RecordingError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists),
        "{error}"
    );
}

#[test]
fn the_syncer_flushes_while_recording_and_on_stop() {
    let dir = TempDir::new();
    let mut writers: Vec<Writer> = ["mic.opus", "sys.opus"]
        .iter()
        .map(|name| Writer::create(&dir.path().join(name), WriterConfig::default()).unwrap())
        .collect();
    let handles = writers.iter().map(|w| w.sync_handle().unwrap()).collect();
    let syncer = Syncer::spawn(dir.path(), handles, Duration::from_millis(10)).unwrap();
    let audio = speech(RATE);
    for writer in &mut writers {
        push_all(writer, &audio);
    }
    std::thread::sleep(Duration::from_millis(30));
    for writer in writers {
        assert_eq!(writer.finish().unwrap(), RATE as u64);
    }
    syncer.stop().unwrap();
    std::fs::File::create(dir.path().join("marker"))
        .unwrap()
        .write_all(b"x")
        .unwrap();
    sync_dir(dir.path()).unwrap();
}

#[test]
fn readable_follows_written_pages_and_flush() {
    let dir = TempDir::new();
    let path = dir.path().join("live.opus");
    let mut writer = Writer::create(&path, WriterConfig::default()).unwrap();
    let audio = speech(RATE * 3 / 2);
    writer.push(&audio).unwrap();
    // One full page (1 s) is out; the encoder delay is still inside the encoder.
    let readable = writer.readable();
    assert!(
        readable < RATE as u64 && readable + 200 > RATE as u64,
        "{readable}"
    );
    assert_eq!(probe(&path).unwrap().samples, readable);
    writer.flush().unwrap();
    let readable = writer.readable();
    assert!(
        readable + FRAME as u64 * 2 > audio.len() as u64,
        "{readable}"
    );
    // What `readable` promises decodes while the writer keeps going.
    let decoded = decode_range(&path, 0, readable).unwrap();
    assert_eq!(decoded.len() as u64, readable);
    writer.push(&audio).unwrap();
    assert_eq!(writer.finish().unwrap(), 2 * audio.len() as u64);
    assert_eq!(probe(&path).unwrap().samples, 2 * audio.len() as u64);
}
