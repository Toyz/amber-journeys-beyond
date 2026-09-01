use crate::{Error, Result};

/// Byte order of a Director movie, decided by the file magic.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Endian {
    /// `RIFX` — authored on a Mac.
    Big,
    /// `XFIR` — authored on Windows.
    Little,
}

impl Endian {
    fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            Endian::Big => u16::from_be_bytes(b),
            Endian::Little => u16::from_le_bytes(b),
        }
    }

    fn u32(self, b: [u8; 4]) -> u32 {
        match self {
            Endian::Big => u32::from_be_bytes(b),
            Endian::Little => u32::from_le_bytes(b),
        }
    }
}

/// A four-character chunk tag. Stored reversed in little-endian movies, so it is
/// normalised here to the conventional big-endian spelling (`BITD`, `CASt`, ...).
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    pub const fn new(s: &[u8; 4]) -> Self {
        FourCc(*s)
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl std::fmt::Debug for FourCc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&[u8; 4]> for FourCc {
    fn eq(&self, other: &&[u8; 4]) -> bool {
        &self.0 == *other
    }
}

/// One entry of the `mmap` resource table.
#[derive(Copy, Clone, Debug)]
pub struct Resource {
    pub tag: FourCc,
    pub offset: u32,
    pub size: u32,
}

/// Bounds-checked cursor over a movie's bytes, aware of the file's byte order.
#[derive(Copy, Clone)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub endian: Endian,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        Reader { data, pos: 0, endian }
    }

    pub fn at(data: &'a [u8], endian: Endian, pos: usize) -> Self {
        Reader { data, pos, endian }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated {
            need: n,
            have: self.remaining(),
        })?;
        let slice = self.data.get(self.pos..end).ok_or(Error::Truncated {
            need: n,
            have: self.remaining(),
        })?;
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(self.endian.u16([b[0], b[1]]))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(self.endian.u32([b[0], b[1], b[2], b[3]]))
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.take(n)
    }

    /// Reads a tag, un-reversing it when the movie is little-endian.
    pub fn fourcc(&mut self) -> Result<FourCc> {
        let b = self.take(4)?;
        let mut out = [b[0], b[1], b[2], b[3]];
        if self.endian == Endian::Little {
            out.reverse();
        }
        Ok(FourCc(out))
    }

    /// Reads a Pascal-style string: one length byte followed by that many bytes.
    pub fn pstring(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        let b = self.take(n)?;
        Ok(b.iter().map(|&c| c as char).collect())
    }
}
