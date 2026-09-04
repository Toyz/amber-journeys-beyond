//! Reading the game straight off a CD image.
//!
//! The disc is a hybrid: an Apple partition map for the Macintosh half and an
//! ISO 9660 filesystem for the PC half, which is the one the Windows build
//! reads and the one this reads. Nothing here is a general ISO 9660
//! implementation -- no Joliet, no Rock Ridge, no multi-extent files -- it is
//! what a 1996 data disc uses and no more.
//!
//! The layout is simple enough to state in full. Sector 16 holds the primary
//! volume descriptor, which carries the logical block size and a thirty-four
//! byte directory record for the root. A directory is a run of records, each
//! one giving the extent it starts at, how long it is, whether it is itself a
//! directory, and its name. Walking that tree once gives the offset and length
//! of every file, which is all the `Content` trait wants.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::content::Content;

/// Somewhere an image's bytes can be read from at an offset.
///
/// A file on a desktop, and a buffer everywhere else -- a browser hands over
/// the image the player picked as one array, and the same reader walks it.
pub trait ReadAt: Send + Sync {
    fn read_at(&self, at: u64, len: u64) -> Option<Vec<u8>>;
}

impl ReadAt for Mutex<File> {
    fn read_at(&self, at: u64, len: u64) -> Option<Vec<u8>> {
        let mut out = vec![0u8; len as usize];
        let mut file = self.lock().ok()?;
        file.seek(SeekFrom::Start(at)).ok()?;
        file.read_exact(&mut out).ok()?;
        Some(out)
    }
}

impl ReadAt for Vec<u8> {
    fn read_at(&self, at: u64, len: u64) -> Option<Vec<u8>> {
        let (at, len) = (at as usize, len as usize);
        self.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
    }
}

/// One file's place in the image.
#[derive(Clone, Copy)]
struct Extent {
    at: u64,
    len: u64,
}

pub struct Iso {
    image: Box<dyn ReadAt>,
    files: HashMap<String, Extent>,
    paths: Vec<String>,
}

impl Iso {
    /// Opens an image file and reads its directory tree.
    pub fn open(path: &Path) -> std::io::Result<Iso> {
        Iso::over(Box::new(Mutex::new(File::open(path)?)))
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))
    }

    /// The same over an image already in memory, which is what a browser has.
    pub fn over(image: Box<dyn ReadAt>) -> std::io::Result<Iso> {
        // The primary volume descriptor. Sector 16 by definition, and the
        // sector size at this point is always 2048 -- the block size the
        // descriptor itself declares applies to everything after it.
        let pvd = image.read_at(16 * 2048, 2048).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "not a CD image")
        })?;
        if pvd.get(1..6) != Some(b"CD001") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "no ISO 9660 volume descriptor",
            ));
        }
        // Numbers are stored twice, little-endian then big; the little half is
        // first and is the one to read.
        let block = u16::from_le_bytes([pvd[128], pvd[129]]) as u64;
        let block = if block == 0 { 2048 } else { block };

        let root = &pvd[156..156 + 34];
        let at = u32::from_le_bytes([root[2], root[3], root[4], root[5]]) as u64;
        let len = u32::from_le_bytes([root[10], root[11], root[12], root[13]]) as u64;

        let mut iso = Iso {
            image,
            files: HashMap::new(),
            paths: Vec::new(),
        };
        iso.walk(block, at, len, "", 0)?;
        Ok(iso)
    }

    /// Reads one directory's records, recursing into the directories in it.
    fn walk(
        &mut self,
        block: u64,
        at: u64,
        len: u64,
        prefix: &str,
        depth: usize,
    ) -> std::io::Result<()> {
        // The disc is shallow. This only stops a malformed image looping.
        if depth > 8 {
            return Ok(());
        }
        let data = self.bytes(at * block, len)?;

        let mut i = 0usize;
        while i < data.len() {
            let record_len = data[i] as usize;
            // A zero length is padding to the end of the sector: the next
            // record starts at the next block boundary, if there is one.
            if record_len == 0 {
                let next = (i / block as usize + 1) * block as usize;
                if next >= data.len() {
                    break;
                }
                i = next;
                continue;
            }
            if i + record_len > data.len() || record_len < 33 {
                break;
            }
            let record = &data[i..i + record_len];
            let extent = u32::from_le_bytes([record[2], record[3], record[4], record[5]]) as u64;
            let size = u32::from_le_bytes([record[10], record[11], record[12], record[13]]) as u64;
            let flags = record[25];
            let name_len = record[32] as usize;
            let name = record.get(33..33 + name_len).unwrap_or(&[]);

            // The first two records of every directory are itself and its
            // parent, named with a single zero byte and a single one.
            let is_self_or_parent = name_len == 1 && (name[0] == 0 || name[0] == 1);
            if !is_self_or_parent {
                // File names carry a version, `README.TXT;1`, which nothing
                // outside the filesystem uses.
                let name = String::from_utf8_lossy(name);
                let name = name.split(';').next().unwrap_or(&name).to_string();
                let here = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                if flags & 0x02 != 0 {
                    self.walk(block, extent, size, &here, depth + 1)?;
                } else {
                    self.files.insert(
                        here.to_ascii_uppercase(),
                        Extent { at: extent * block, len: size },
                    );
                    self.paths.push(here);
                }
            }
            i += record_len;
        }
        Ok(())
    }

    fn bytes(&self, at: u64, len: u64) -> std::io::Result<Vec<u8>> {
        self.image
            .read_at(at, len)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short image"))
    }

    /// Whether a path is worth trying to open as an image.
    pub fn looks_like_one(path: &Path) -> bool {
        path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
    }

    pub fn count(&self) -> usize {
        self.paths.len()
    }
}

impl Content for Iso {
    fn list(&self) -> Vec<String> {
        self.paths.clone()
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let extent = *self.files.get(&path.to_ascii_uppercase())?;
        self.bytes(extent.at, extent.len).ok()
    }
}

/// Opens whichever kind of thing the path is.
///
/// A directory is read as a directory and a file as an image, so the same
/// command line takes an installed copy, a mounted disc or an `.iso` without
/// being told which.
pub fn open(path: &Path) -> std::io::Result<Box<dyn Content>> {
    if Iso::looks_like_one(path) {
        let iso = Iso::open(path)?;
        eprintln!("reading {} ({} files)", path.display(), iso.count());
        return Ok(Box::new(iso));
    }
    Ok(Box::new(crate::content::Files::new(&PathBuf::from(path))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the real disc when it is here, and silently skipped when it is
    /// not -- the repo carries no game data by design.
    fn disc() -> Option<Iso> {
        let path = Path::new("Amber - Journeys Beyond (1996)(Hue Forest).iso");
        path.is_file().then(|| Iso::open(path).ok()).flatten()
    }

    #[test]
    fn the_image_holds_the_game() {
        let Some(iso) = disc() else { return };
        // Every chapter's movie, read straight out of the image.
        for chapter in ["ROXY", "MARGARET", "BRICE", "EDWIN"] {
            let path = format!("{chapter}/{chapter}.DXR");
            let bytes = iso.read(&path).unwrap_or_else(|| panic!("no {path}"));
            assert_eq!(&bytes[..4], b"XFIR", "{path} is not a Director movie");
        }
    }

    #[test]
    fn a_name_is_matched_without_case() {
        let Some(iso) = disc() else { return };
        assert!(iso.read("roxy/roxy.dxr").is_some());
        assert!(iso.read("ROXY/ROXY.DXR").is_some());
        assert!(iso.read("ROXY/NOTHING.DXR").is_none());
    }
}
