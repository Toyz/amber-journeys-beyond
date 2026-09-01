use crate::{Error, Result};

/// One QuickTime atom: a size, a four-character type, and a body.
#[derive(Copy, Clone, Debug)]
pub struct Atom {
    pub kind: [u8; 4],
    /// Offset of the body, past the header.
    pub body: usize,
    pub body_len: usize,
    /// Offset just past the whole atom.
    pub end: usize,
}

impl Atom {
    pub fn is(&self, kind: &[u8; 4]) -> bool {
        &self.kind == kind
    }
}

/// Reads the atom starting at `off`.
///
/// A declared size of 0 means "to the end of the enclosing range", and 1 means
/// the real 64-bit size follows the type. Both appear in the wild.
pub fn read(data: &[u8], off: usize, end: usize) -> Result<Atom> {
    if off + 8 > end {
        return Err(Error::Truncated);
    }
    let size32 = u32::from_be_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    let kind: [u8; 4] = data[off + 4..off + 8].try_into().unwrap();

    let (size, header) = match size32 {
        0 => (end - off, 8),
        1 => {
            if off + 16 > end {
                return Err(Error::Truncated);
            }
            let big = u64::from_be_bytes(data[off + 8..off + 16].try_into().unwrap()) as usize;
            (big, 16)
        }
        n if n < 8 => return Err(Error::Truncated),
        n => (n, 8),
    };

    let atom_end = off.checked_add(size).ok_or(Error::Truncated)?.min(end);
    Ok(Atom {
        kind,
        body: off + header,
        body_len: atom_end.saturating_sub(off + header),
        end: atom_end,
    })
}

/// Iterates the atoms directly inside `[start, end)`.
pub fn children(data: &[u8], start: usize, end: usize) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut off = start;
    while off + 8 <= end {
        let Ok(atom) = read(data, off, end) else { break };
        if atom.end <= off {
            break;
        }
        off = atom.end;
        out.push(atom);
    }
    out
}

/// Finds the first direct child of the given type.
pub fn find(data: &[u8], start: usize, end: usize, kind: &[u8; 4]) -> Option<Atom> {
    children(data, start, end).into_iter().find(|a| a.is(kind))
}

/// Follows a path of nested atom types, e.g. `[b"moov", b"trak", b"tkhd"]`.
pub fn path(data: &[u8], start: usize, end: usize, path: &[&[u8; 4]]) -> Option<Atom> {
    let mut range = (start, end);
    let mut found = None;
    for kind in path {
        let atom = find(data, range.0, range.1, kind)?;
        range = (atom.body, atom.body + atom.body_len);
        found = Some(atom);
    }
    found
}

pub fn u16_at(data: &[u8], off: usize) -> u16 {
    data.get(off..off + 2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

pub fn u32_at(data: &[u8], off: usize) -> u32 {
    data.get(off..off + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}
