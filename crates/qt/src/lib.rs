//! QuickTime reading and the codecs Amber ships.
//!
//! The game's movies are plain QuickTime files: a `moov` atom describing the
//! tracks and a `mdat` holding the samples. Video is Cinepak at 320x240, audio
//! is IMA ADPCM at 22 kHz. Both codecs are implemented here rather than pulled
//! in, so the engine has no native media dependency on any platform.

mod atom;
mod cinepak;
mod demux;
mod ima4;
pub mod pcm;
pub mod rle;

pub use cinepak::Cinepak;
pub use demux::{Movie, Sample, Track, TrackKind};
pub use ima4::{decode_ima4, Ima4Decoder};

use std::fmt;

#[derive(Debug)]
pub enum Error {
    NotQuickTime,
    MissingAtom(&'static str),
    Truncated,
    Unsupported(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotQuickTime => write!(f, "not a QuickTime movie"),
            Error::MissingAtom(a) => write!(f, "missing {a} atom"),
            Error::Truncated => write!(f, "truncated file"),
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
