//! Loading the game's sound effects and ambient loops.
//!
//! Scripts name sounds by symbol (`soundEffect #crowbar`, `setLoop #houseHum`),
//! never by file. Each chapter movie carries a table resolving those symbols:
//!
//! ```text
//! [#crowbar: "bhcbar.wav", #clumsy: ["bump1.wav", "bump2.wav"],
//!  #computerLoop: 828, #houseHum: 854, #amberHum: 853]
//! ```
//!
//! A string names a file on the disc, an integer names a `snd ` cast member
//! inside the movie, and a list is a set of interchangeable takes to choose
//! between. A companion table gives per-sound gain.
//!
//! The files themselves are AIFF-C carrying IMA ADPCM, which is the same codec
//! the movies use, or WAV carrying unsigned 8-bit PCM.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lingo::{parse_value, Value};

/// Where a named sound comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// One or more interchangeable files; more than one means pick at random.
    Files(Vec<String>),
    /// A `snd ` cast member in the chapter's own movie.
    Cast(u32),
}

/// Decoded PCM, always 16-bit.
#[derive(Clone)]
pub struct Pcm {
    pub samples: Vec<i16>,
    pub rate: u32,
    pub channels: u16,
}

/// Symbol-to-source tables plus an index of the audio files on disc.
pub struct SoundBank {
    sources: HashMap<String, Source>,
    /// Sounds that belong to a named group rather than the top level, keyed by
    /// `(group, item)`. Radio and clock programmes are built this way: the
    /// group holds the takes and the schema holds the order to play them in,
    /// and item names repeat across groups, so `#tune1` means a different file
    /// in the bedroom than in the kitchen.
    groups: HashMap<(String, String), Source>,
    gains: HashMap<String, f32>,
    files: HashMap<String, PathBuf>,
}

impl SoundBank {
    pub fn new(root: &Path) -> SoundBank {
        let mut files = HashMap::new();
        index_files(root, 0, &mut files);
        SoundBank {
            sources: HashMap::new(),
            groups: HashMap::new(),
            gains: HashMap::new(),
            files,
        }
    }

    /// Merges a chapter's tables.
    ///
    /// Both live inside the chapter's `foreground.DATA` text member, which
    /// holds configuration rather than a room: `#soundBank` maps each symbol to
    /// its source and `#soundVolTweaks` gives per-sound gain. Matching on those
    /// key names rather than on the shape of the data matters, because every
    /// room record also mentions `houseHum` in its ambient mix and a shape
    /// match happily collects a hundred of those instead.
    pub fn add_tables(&mut self, texts: &[String]) {
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.starts_with("[#") && !trimmed.starts_with("[ #") {
                continue;
            }
            if !trimmed.contains("soundBank") {
                continue;
            }
            let Ok(config) = parse_value(trimmed) else {
                continue;
            };

            if let Some(Value::Props(bank)) = config.get("soundBank") {
                for (name, value) in bank {
                    match value {
                        Value::String(f) => {
                            self.sources.insert(name.clone(), Source::Files(vec![f.clone()]));
                        }
                        Value::Int(cast) if *cast > 0 => {
                            self.sources.insert(name.clone(), Source::Cast(*cast as u32));
                        }
                        Value::List(items) => {
                            let takes: Vec<String> = items
                                .iter()
                                .filter_map(|i| i.as_str())
                                .map(str::to_owned)
                                .collect();
                            if !takes.is_empty() {
                                self.sources.insert(name.clone(), Source::Files(takes));
                            }
                        }
                        // A nested property list is a group, not a sound.
                        Value::Props(items) => {
                            for (item, source) in items {
                                let parsed = match source {
                                    Value::String(f) => Source::Files(vec![f.clone()]),
                                    Value::Int(c) if *c > 0 => Source::Cast(*c as u32),
                                    Value::List(takes) => {
                                        let takes: Vec<String> = takes
                                            .iter()
                                            .filter_map(|t| t.as_str())
                                            .map(str::to_owned)
                                            .collect();
                                        if takes.is_empty() {
                                            continue;
                                        }
                                        Source::Files(takes)
                                    }
                                    _ => continue,
                                };
                                self.groups
                                    .insert((name.clone(), item.clone()), parsed);
                            }
                        }
                        _ => {}
                    }
                }
            }

            if let Some(Value::Props(tweaks)) = config.get("soundVolTweaks") {
                for (name, value) in tweaks {
                    let gain = match value {
                        Value::Float(g) => *g as f32,
                        Value::Int(g) => *g as f32,
                        _ => continue,
                    };
                    self.gains.insert(name.clone(), gain);
                }
            }
        }
    }

    pub fn source(&self, symbol: &str) -> Option<&Source> {
        self.sources
            .get(&symbol.trim_start_matches('#').to_ascii_lowercase())
    }

    /// A sound belonging to a named group, e.g. `#tune1` within `#BRradio`.
    pub fn source_in(&self, group: &str, item: &str) -> Option<&Source> {
        self.groups.get(&(
            group.trim_start_matches('#').to_ascii_lowercase(),
            item.trim_start_matches('#').to_ascii_lowercase(),
        ))
    }

    /// True when the name is a group of sounds rather than a single one.
    pub fn is_group(&self, name: &str) -> bool {
        let key = name.trim_start_matches('#').to_ascii_lowercase();
        self.groups.keys().any(|(g, _)| *g == key)
    }

    /// The item names inside a group.
    pub fn group_items(&self, group: &str) -> Vec<&str> {
        let key = group.trim_start_matches('#').to_ascii_lowercase();
        let mut items: Vec<&str> = self
            .groups
            .keys()
            .filter(|(g, _)| *g == key)
            .map(|(_, i)| i.as_str())
            .collect();
        items.sort_unstable();
        items
    }

    pub fn group_count(&self) -> usize {
        let mut names: Vec<&str> = self.groups.keys().map(|(g, _)| g.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names.len()
    }

    /// Playback gain for a sound, defaulting to unity when untabulated.
    pub fn gain(&self, symbol: &str) -> f32 {
        self.gains
            .get(&symbol.trim_start_matches('#').to_ascii_lowercase())
            .copied()
            .unwrap_or(1.0)
    }

    pub fn file(&self, name: &str) -> Option<&Path> {
        self.files
            .get(&name.trim().to_ascii_uppercase())
            .map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Names that resolve to a file which is not on the disc.
    pub fn missing(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|(_, s)| match s {
                Source::Files(f) => f.iter().all(|n| self.file(n).is_none()),
                Source::Cast(_) => false,
            })
            .map(|(n, _)| n.as_str())
            .collect()
    }
}

fn index_files(dir: &Path, depth: usize, out: &mut HashMap<String, PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            index_files(&path, depth + 1, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let upper = name.to_ascii_uppercase();
            if upper.ends_with(".AIF") || upper.ends_with(".WAV") || upper.ends_with(".AIFF") {
                out.entry(upper).or_insert(path);
            }
        }
    }
}

/// Reads an AIFF/AIFF-C or WAV file into PCM.
pub fn load(path: &Path) -> Option<Pcm> {
    let data = std::fs::read(path).ok()?;
    match data.get(..4)? {
        b"FORM" => load_aiff(&data),
        b"RIFF" => load_wav(&data),
        _ => None,
    }
}

fn be16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

fn be32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

fn le16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

fn le32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

/// Decodes the 80-bit IEEE extended float AIFF stores its sample rate in.
fn extended_rate(d: &[u8], o: usize) -> u32 {
    let exponent = (be16(d, o) & 0x7fff) as i32;
    let mantissa = d
        .get(o + 2..o + 10)
        .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0);
    if exponent == 0 && mantissa == 0 {
        return 0;
    }
    // value = mantissa * 2^(exponent - 16383 - 63)
    let shift = exponent - 16383 - 63;
    let value = if shift >= 0 {
        (mantissa as f64) * 2f64.powi(shift)
    } else {
        (mantissa as f64) / 2f64.powi(-shift)
    };
    value as u32
}

fn load_aiff(d: &[u8]) -> Option<Pcm> {
    let form = d.get(8..12)?;
    let compressed = form == b"AIFC";

    let mut channels = 1u16;
    let mut rate = 22050u32;
    let mut bits = 16u16;
    let mut compression = *b"NONE";
    let mut ssnd: Option<&[u8]> = None;

    let mut off = 12usize;
    while off + 8 <= d.len() {
        let kind = d.get(off..off + 4)?;
        let size = be32(d, off + 4) as usize;
        let body = off + 8;
        match kind {
            b"COMM" => {
                channels = be16(d, body).max(1);
                bits = be16(d, body + 6);
                rate = extended_rate(d, body + 8).max(1);
                if compressed && size >= 22 {
                    compression = d.get(body + 18..body + 22)?.try_into().ok()?;
                }
            }
            b"SSND" => {
                // The first eight bytes are an offset and a block size, both
                // zero in practice, and the samples follow.
                let start = body + 8 + be32(d, body) as usize;
                ssnd = d.get(start..(body + size).min(d.len()));
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        off = body + size + (size & 1);
    }

    let raw = ssnd?;
    let samples = match &compression {
        b"ima4" => qt::decode_ima4(raw, channels),
        b"NONE" | b"sowt" if bits == 8 => raw.iter().map(|&b| (b as i8 as i16) << 8).collect(),
        b"sowt" => raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
        _ => raw
            .chunks_exact(2)
            .map(|c| i16::from_be_bytes([c[0], c[1]]))
            .collect(),
    };

    Some(Pcm {
        samples,
        rate,
        channels,
    })
}

fn load_wav(d: &[u8]) -> Option<Pcm> {
    let mut channels = 1u16;
    let mut rate = 22050u32;
    let mut bits = 8u16;
    let mut pcm: Option<&[u8]> = None;

    let mut off = 12usize;
    while off + 8 <= d.len() {
        let kind = d.get(off..off + 4)?;
        let size = le32(d, off + 4) as usize;
        let body = off + 8;
        match kind {
            b"fmt " => {
                channels = le16(d, body + 2).max(1);
                rate = le32(d, body + 4).max(1);
                bits = le16(d, body + 14);
            }
            b"data" => pcm = d.get(body..(body + size).min(d.len())),
            _ => {}
        }
        off = body + size + (size & 1);
    }

    let raw = pcm?;
    // WAV stores 8-bit as unsigned and everything wider as signed.
    let samples = if bits == 8 {
        raw.iter().map(|&b| ((b as i16) - 128) << 8).collect()
    } else {
        raw.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    };

    Some(Pcm {
        samples,
        rate,
        channels,
    })
}
