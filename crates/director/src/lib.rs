//! Reader for Macromedia Director 5 movie files (`.DXR` / `.DIR` / `.CST`).
//!
//! A Director movie is a RIFF-like container. The magic is `RIFX` when the file
//! was authored big-endian (Mac) and `XFIR` when little-endian (Windows); the
//! byte order applies to every integer and every four-character tag in the file.
//! Amber ships both flavours on the same hybrid disc, so endianness is resolved
//! at load time rather than assumed.
//!
//! Layout:
//!   - `imap` points at the resource map.
//!   - `mmap` is the resource table: one 20-byte entry per chunk (tag, size, offset).
//!   - `KEY*` associates an owning resource with its children (a `CASt` owns its `BITD`).
//!   - `CAS*` is a dense array mapping cast-member slots to `CASt` resource indices.

mod bitmap;
mod chunk;
mod movie;
mod palette;
mod sound;

pub use bitmap::Bitmap;
pub use chunk::{Endian, FourCc, Resource};
pub use movie::{CastKind, CastMember, Movie};
pub use palette::Palette;
pub use sound::Sound;

use std::fmt;

#[derive(Debug)]
pub enum Error {
    NotDirector,
    Truncated { need: usize, have: usize },
    MissingChunk(&'static str),
    BadCast(u32),
    Unsupported(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotDirector => write!(f, "not a Director movie (bad RIFX/XFIR magic)"),
            Error::Truncated { need, have } => {
                write!(f, "truncated: needed {need} bytes, file has {have}")
            }
            Error::MissingChunk(t) => write!(f, "movie has no {t} chunk"),
            Error::BadCast(n) => write!(f, "no such cast member: {n}"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
            Error::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
