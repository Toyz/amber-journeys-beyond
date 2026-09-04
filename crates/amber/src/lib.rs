//! *Amber: Journeys Beyond* (Hue Forest, 1996), as an engine.
//!
//! Everything here is the game: the rooms, the scripts, the state machine, the
//! compositor, the mixer. None of it names a platform. The three seams it
//! reaches the outside world through are traits --
//!
//!   - [`content::Content`], where the data comes from: a directory, a CD
//!     image, a bundle;
//!   - [`host::Host`], where a frame goes and where the pointer comes from;
//!   - [`audio::Sink`], where the mixed samples go.
//!
//! The desktop implementations of the last two are behind the `desktop`
//! feature, because they are the only things in the crate that pull in a
//! window and an audio device. Built without it -- which is how it is built
//! for the web -- the crate is the game and nothing else.

#[macro_use]
pub mod trace;
pub mod audio;
pub mod casttable;
pub mod clock;
pub mod content;
pub mod cursor;
pub mod game;
pub mod host;
pub mod inventory;
pub mod iso;
pub mod locations;
pub mod markers;
pub mod media;
pub mod natives;
pub mod player;
pub mod presentation;
pub mod record;
pub mod schema;
pub mod script;
pub mod sound;
pub mod state;
pub mod world;

#[cfg(feature = "desktop")]
pub mod audio_device;
#[cfg(feature = "desktop")]
pub mod host_desktop;
#[cfg(feature = "desktop")]
pub mod render;
#[cfg(feature = "desktop")]
pub mod walk;

/// Extra game directories to fall back on, from `AMBER_FALLBACK`.
///
/// The game had two releases and neither disc is complete on its own. The
/// Macintosh release carries five films the PC release references and does not
/// ship -- Margaret's opening, Roxy's east wall, and the three scan-unit films
/// -- and the PC release carries `tuner_bg.mov` and a pile of sounds the
/// Macintosh keeps inside its installer. Both are legitimate; they are just
/// different pressings, so a second root can be named rather than the files
/// being copied about.
///
/// Separator is `:`, as a PATH would be.
pub fn fallback_roots() -> Vec<std::path::PathBuf> {
    std::env::var("AMBER_FALLBACK")
        .ok()
        .into_iter()
        .flat_map(|v| {
            v.split(':')
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|p| p.is_dir())
        .collect()
}

/// Writes an RGBA buffer out as a PNG.
///
/// A dozen lines of deflate-stored blocks and a CRC, so a screenshot can be taken without a
/// dependency. `shot` and the walkthrough's `shot` step both use it.
pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, e) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
        c ^ 0xffff_ffff
    }

    /// Stored-mode deflate: no compression, but a valid zlib stream, which keeps
    /// the exporter dependency-free.
    fn deflate_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        for (i, block) in data.chunks(65535).enumerate() {
            let last = (i + 1) * 65535 >= data.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(block.len() as u16).to_le_bytes());
            out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
            out.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        out.extend_from_slice(&((b << 16) | a).to_be_bytes());
        out
    }

    let chunk = |tag: &[u8; 4], data: &[u8]| {
        let mut c = Vec::with_capacity(data.len() + 12);
        c.extend_from_slice(&(data.len() as u32).to_be_bytes());
        c.extend_from_slice(tag);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32(&[&tag[..], data].concat()).to_be_bytes());
        c
    };

    let mut hdr = Vec::new();
    hdr.extend_from_slice(&w.to_be_bytes());
    hdr.extend_from_slice(&h.to_be_bytes());
    hdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut raw = Vec::with_capacity((w * h * 4 + h) as usize);
    for y in 0..h as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }

    let mut f = std::fs::File::create(path)?;
    f.write_all(b"\x89PNG\r\n\x1a\n")?;
    f.write_all(&chunk(b"IHDR", &hdr))?;
    f.write_all(&chunk(b"IDAT", &deflate_stored(&raw)))?;
    f.write_all(&chunk(b"IEND", &[]))?;
    Ok(())
}
