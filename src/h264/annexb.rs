use std::fmt;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum NalType {
    Unspecified,
    SliceNonIdr,
    SliceDataPartitionA,
    SliceDataPartitionB,
    SliceDataPartitionC,
    SliceIdr,
    Sei,
    Sps,
    Pps,
    Aud,
    EndOfSequence,
    EndOfStream,
    Filler,
    SpsExt,
    Prefix,
    SubsetSps,
    Reserved(u8),
    Unknown(u8),
}

impl NalType {
    pub fn from_header_byte(byte: u8) -> Self {
        match byte & 0x1f {
            0 => Self::Unspecified,
            1 => Self::SliceNonIdr,
            2 => Self::SliceDataPartitionA,
            3 => Self::SliceDataPartitionB,
            4 => Self::SliceDataPartitionC,
            5 => Self::SliceIdr,
            6 => Self::Sei,
            7 => Self::Sps,
            8 => Self::Pps,
            9 => Self::Aud,
            10 => Self::EndOfSequence,
            11 => Self::EndOfStream,
            12 => Self::Filler,
            13 => Self::SpsExt,
            14 => Self::Prefix,
            15 => Self::SubsetSps,
            v @ 16..=23 => Self::Reserved(v),
            v => Self::Unknown(v),
        }
    }

    pub fn is_key_video_unit(self) -> bool {
        matches!(self, Self::Sps | Self::Pps | Self::SliceIdr)
    }
}

#[derive(Debug, Clone)]
pub struct NalUnit {
    offset: usize,
    bytes: Vec<u8>,
}

impl NalUnit {
    pub fn new(offset: usize, bytes: Vec<u8>) -> Self {
        Self { offset, bytes }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn nal_type(&self) -> NalType {
        let Some((payload_start, _)) = start_code_len(&self.bytes, 0) else {
            return NalType::Unknown(255);
        };
        if payload_start >= self.bytes.len() {
            return NalType::Unknown(255);
        }
        NalType::from_header_byte(self.bytes[payload_start])
    }
}

#[derive(Debug, Default, Clone)]
pub struct NalStats {
    pub total: u64,
    pub sps: u64,
    pub pps: u64,
    pub idr: u64,
    pub non_idr: u64,
    pub sei: u64,
    pub aud: u64,
    pub other: u64,
}

impl NalStats {
    pub fn add(&mut self, ty: NalType) {
        self.total += 1;
        match ty {
            NalType::Sps => self.sps += 1,
            NalType::Pps => self.pps += 1,
            NalType::SliceIdr => self.idr += 1,
            NalType::SliceNonIdr => self.non_idr += 1,
            NalType::Sei => self.sei += 1,
            NalType::Aud => self.aud += 1,
            _ => self.other += 1,
        }
    }
}

impl fmt::Display for NalStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "total={} sps={} pps={} idr={} non_idr={} sei={} aud={} other={}",
            self.total, self.sps, self.pps, self.idr, self.non_idr, self.sei, self.aud, self.other
        )
    }
}

pub struct AnnexBStreamParser {
    buf: Vec<u8>,
    absolute_offset: usize,
    search_from: usize,
}

impl AnnexBStreamParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256 * 1024),
            absolute_offset: 0,
            search_from: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Returns the next complete NAL including its Annex-B start code.
    pub fn next_nal(&mut self) -> Option<NalUnit> {
        let first = find_start_code(&self.buf, self.search_from)?;
        let second = find_start_code(&self.buf, first.0 + first.1)?;

        if first.0 > 0 {
            self.absolute_offset += first.0;
            self.buf.drain(..first.0);
            let second_adjusted = second.0 - first.0;
            let bytes: Vec<u8> = self.buf.drain(..second_adjusted).collect();
            let offset = self.absolute_offset;
            self.absolute_offset += bytes.len();
            self.search_from = 0;
            return Some(NalUnit::new(offset, bytes));
        }

        let bytes: Vec<u8> = self.buf.drain(..second.0).collect();
        let offset = self.absolute_offset;
        self.absolute_offset += bytes.len();
        self.search_from = 0;
        Some(NalUnit::new(offset, bytes))
    }

    /// Flushes the final buffered NAL at end-of-stream.
    pub fn finish(&mut self) -> Option<NalUnit> {
        let first = find_start_code(&self.buf, 0)?;
        if first.0 > 0 {
            self.absolute_offset += first.0;
            self.buf.drain(..first.0);
        }
        if self.buf.is_empty() {
            return None;
        }
        let bytes = std::mem::take(&mut self.buf);
        let offset = self.absolute_offset;
        self.absolute_offset += bytes.len();
        Some(NalUnit::new(offset, bytes))
    }
}

fn start_code_len(data: &[u8], at: usize) -> Option<(usize, usize)> {
    if at + 4 <= data.len() && data[at..at + 4] == [0, 0, 0, 1] {
        return Some((at + 4, 4));
    }
    if at + 3 <= data.len() && data[at..at + 3] == [0, 0, 1] {
        return Some((at + 3, 3));
    }
    None
}

fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    if data.len() < 3 || from >= data.len() {
        return None;
    }
    let mut i = from;
    while i + 3 <= data.len() {
        if i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1] {
            return Some((i, 4));
        }
        if data[i..i + 3] == [0, 0, 1] {
            return Some((i, 3));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_nals() {
        let mut p = AnnexBStreamParser::new();
        p.push(&[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3]);
        let a = p.next_nal().unwrap();
        assert_eq!(a.nal_type(), NalType::Sps);
        assert_eq!(a.bytes(), &[0, 0, 0, 1, 0x67, 1, 2]);
        let b = p.finish().unwrap();
        assert_eq!(b.nal_type(), NalType::Pps);
    }
}
