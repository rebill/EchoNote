//! Minimal Ogg pages (RFC 3533): one logical stream, packets never span pages.

use std::io::{self, Read};

use super::RecordingError;

pub const FLAG_CONTINUED: u8 = 0x01;
pub const FLAG_BOS: u8 = 0x02;
pub const FLAG_EOS: u8 = 0x04;

const HEADER: usize = 27;
const MAX_LACING: usize = 255;

/// CRC-32 with polynomial 0x04c11db7, no reflection, zero init and no final xor.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

fn crc(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc = (crc << 8) ^ CRC_TABLE[((crc >> 24) as u8 ^ byte) as usize];
    }
    crc
}

/// Collects whole packets for one page. Buffers are reused from page to page.
pub struct PageBuilder {
    serial: u32,
    sequence: u32,
    lacing: Vec<u8>,
    body: Vec<u8>,
    page: Vec<u8>,
}

impl PageBuilder {
    pub fn new(serial: u32) -> Self {
        Self {
            serial,
            sequence: 0,
            lacing: Vec::with_capacity(MAX_LACING),
            body: Vec::with_capacity(8 << 10),
            page: Vec::with_capacity(HEADER + MAX_LACING + (8 << 10)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lacing.is_empty()
    }

    /// Whether a packet of `len` bytes still fits in the segment table (it takes
    /// `len / 255 + 1` lacing values).
    pub fn fits(&self, len: usize) -> bool {
        self.lacing.len() + len / 255 < MAX_LACING
    }

    pub fn add(&mut self, packet: &[u8]) {
        debug_assert!(self.fits(packet.len()));
        self.lacing
            .extend(std::iter::repeat_n(255, packet.len() / 255));
        self.lacing.push((packet.len() % 255) as u8);
        self.body.extend_from_slice(packet);
    }

    /// Serialize the collected packets as the next page and clear them. The returned bytes go
    /// out in one write, so a crash leaves either the whole page or a tail that fails its CRC.
    pub fn finish(&mut self, flags: u8, granule: u64) -> &[u8] {
        let page = &mut self.page;
        page.clear();
        page.extend_from_slice(b"OggS");
        page.push(0);
        page.push(flags);
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&self.serial.to_le_bytes());
        page.extend_from_slice(&self.sequence.to_le_bytes());
        page.extend_from_slice(&[0; 4]);
        page.push(self.lacing.len() as u8);
        page.extend_from_slice(&self.lacing);
        page.extend_from_slice(&self.body);
        let checksum = crc(0, page);
        page[22..26].copy_from_slice(&checksum.to_le_bytes());
        self.sequence += 1;
        self.lacing.clear();
        self.body.clear();
        page
    }
}

/// One page read back from a file.
#[derive(Default)]
pub struct Page {
    pub flags: u8,
    pub granule: u64,
    pub serial: u32,
    /// Bytes the page takes in the file.
    pub len: u64,
    body: Vec<u8>,
    /// End offset of each packet in `body`.
    ends: Vec<usize>,
}

impl Page {
    pub fn packets(&self) -> impl Iterator<Item = &[u8]> {
        let starts = std::iter::once(0).chain(self.ends.iter().copied());
        starts
            .zip(&self.ends)
            .map(|(start, &end)| &self.body[start..end])
    }

    #[cfg(test)]
    pub fn packet_count(&self) -> usize {
        self.ends.len()
    }
}

/// Fill `buf` completely. `Ok(false)` when the input ends first.
fn read_full(input: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..]) {
            Ok(0) => return Ok(false),
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// Read the next page into `page`. `Ok(false)` at the end of the valid data: end of input, a
/// truncated page, or bytes that are not a page with a matching CRC (a torn write).
pub fn read_page(input: &mut impl Read, page: &mut Page) -> Result<bool, RecordingError> {
    let mut header = [0u8; HEADER];
    if !read_full(input, &mut header)? || &header[..4] != b"OggS" || header[4] != 0 {
        return Ok(false);
    }
    let mut lacing = [0u8; MAX_LACING];
    let lacing = &mut lacing[..header[26] as usize];
    if !read_full(input, lacing)? {
        return Ok(false);
    }
    let body_len: usize = lacing.iter().map(|&value| value as usize).sum();
    page.body.resize(body_len, 0);
    if !read_full(input, &mut page.body)? {
        return Ok(false);
    }
    let expected = u32::from_le_bytes(header[22..26].try_into().unwrap());
    header[22..26].fill(0);
    if crc(crc(crc(0, &header), lacing), &page.body) != expected {
        return Ok(false);
    }

    page.flags = header[5];
    page.granule = u64::from_le_bytes(header[6..14].try_into().unwrap());
    page.serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
    page.len = (HEADER + lacing.len() + body_len) as u64;
    page.ends.clear();
    let mut end = 0;
    for &value in lacing.iter() {
        end += value as usize;
        if value < 255 {
            page.ends.push(end);
        }
    }
    if page.flags & FLAG_CONTINUED != 0 || end != page.ends.last().copied().unwrap_or(0) {
        return Err(RecordingError::Format("packet spans pages"));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_ogg_reference_value() {
        // CRC-32/POSIX (check 0x765e7680) without its final xor.
        assert_eq!(crc(0, b"123456789"), 0x89a1_897f);
    }

    #[test]
    fn pages_round_trip_with_lacing_edge_cases() {
        let packets: Vec<Vec<u8>> = [0usize, 1, 254, 255, 256, 510, 1275]
            .iter()
            .map(|&len| (0..len).map(|i| i as u8).collect())
            .collect();
        let mut builder = PageBuilder::new(7);
        for packet in &packets {
            assert!(builder.fits(packet.len()));
            builder.add(packet);
        }
        let bytes = builder.finish(FLAG_BOS, 42).to_vec();
        assert!(builder.is_empty());

        let mut page = Page::default();
        let mut input = bytes.as_slice();
        assert!(read_page(&mut input, &mut page).unwrap());
        assert_eq!((page.flags, page.granule, page.serial), (FLAG_BOS, 42, 7));
        assert_eq!(page.len, bytes.len() as u64);
        let read: Vec<Vec<u8>> = page.packets().map(<[u8]>::to_vec).collect();
        assert_eq!(read, packets);
        assert!(!read_page(&mut input, &mut page).unwrap());

        let mut corrupt = bytes.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(!read_page(&mut corrupt.as_slice(), &mut page).unwrap());
        assert!(!read_page(&mut &bytes[..bytes.len() - 1], &mut page).unwrap());
    }

    #[test]
    fn the_segment_table_limit_is_respected() {
        let mut builder = PageBuilder::new(1);
        let packet = [0u8; 300]; // two lacing values each
        let mut added = 0;
        while builder.fits(packet.len()) {
            builder.add(&packet);
            added += 1;
        }
        assert_eq!(added, 127);
        let bytes = builder.finish(0, 0).to_vec();
        let mut page = Page::default();
        assert!(read_page(&mut bytes.as_slice(), &mut page).unwrap());
        assert_eq!(page.packet_count(), 127);
    }
}
