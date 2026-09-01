use std::path::Path;

use crate::atom::{self, u16_at, u32_at};
use crate::{Error, Result};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TrackKind {
    Video,
    Sound,
    /// Text, timecode and the other tracks the game does not present.
    Other,
}

/// One sample: a video frame, or a block of audio.
#[derive(Copy, Clone, Debug)]
pub struct Sample {
    pub offset: usize,
    pub len: usize,
    /// Start time in the track's own timescale.
    pub time: u64,
    pub duration: u32,
    /// True for a keyframe. Cinepak inter-frames depend on the previous frame,
    /// so seeking has to start from one of these.
    pub sync: bool,
}

pub struct Track {
    pub kind: TrackKind,
    /// Four-character codec identifier from `stsd`, e.g. `cvid` or `ima4`.
    pub codec: [u8; 4],
    pub timescale: u32,
    pub duration: u64,
    pub width: u16,
    pub height: u16,
    pub channels: u16,
    pub sample_rate: u32,
    /// Audio frames per compressed packet, and the bytes those occupy.
    /// IMA ADPCM packs 64 frames into 34 bytes per channel.
    pub samples_per_packet: u32,
    pub bytes_per_packet: u32,
    pub samples: Vec<Sample>,
}

impl Track {
    /// The sample covering `time`, given in the track's timescale.
    pub fn sample_at(&self, time: u64) -> Option<usize> {
        match self.samples.binary_search_by(|s| s.time.cmp(&time)) {
            Ok(i) => Some(i),
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }

    /// The most recent keyframe at or before `index`, which is where decoding
    /// of an inter-frame codec has to begin.
    pub fn sync_before(&self, index: usize) -> usize {
        (0..=index.min(self.samples.len().saturating_sub(1)))
            .rev()
            .find(|&i| self.samples[i].sync)
            .unwrap_or(0)
    }
}

pub struct Movie {
    data: Vec<u8>,
    pub tracks: Vec<Track>,
}

impl Movie {
    pub fn open(path: impl AsRef<Path>) -> Result<Movie> {
        Movie::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Movie> {
        let end = data.len();
        let moov = atom::find(&data, 0, end, b"moov").ok_or(Error::MissingAtom("moov"))?;

        let mut tracks = Vec::new();
        for trak in atom::children(&data, moov.body, moov.body + moov.body_len) {
            if !trak.is(b"trak") {
                continue;
            }
            if let Some(track) = parse_track(&data, trak.body, trak.body + trak.body_len) {
                tracks.push(track);
            }
        }
        if tracks.is_empty() {
            return Err(Error::NotQuickTime);
        }
        Ok(Movie { data, tracks })
    }

    pub fn track(&self, kind: TrackKind) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == kind)
    }

    /// The bytes of one sample.
    pub fn sample_data(&self, track: &Track, index: usize) -> Option<&[u8]> {
        let s = track.samples.get(index)?;
        self.data.get(s.offset..s.offset + s.len)
    }
}

fn parse_track(data: &[u8], start: usize, end: usize) -> Option<Track> {
    let mdia = atom::find(data, start, end, b"mdia")?;
    let (ms, me) = (mdia.body, mdia.body + mdia.body_len);

    let mdhd = atom::find(data, ms, me, b"mdhd")?;
    // Version 0 packs the times as 32-bit; version 1 uses 64-bit.
    let version = data.get(mdhd.body)?;
    let (timescale, duration) = if *version == 0 {
        (
            u32_at(data, mdhd.body + 12),
            u32_at(data, mdhd.body + 16) as u64,
        )
    } else {
        (
            u32_at(data, mdhd.body + 20),
            u32::from(u32_at(data, mdhd.body + 24)) as u64,
        )
    };

    let hdlr = atom::find(data, ms, me, b"hdlr")?;
    let kind = match &data[hdlr.body + 8..hdlr.body + 12] {
        b"vide" => TrackKind::Video,
        b"soun" => TrackKind::Sound,
        _ => TrackKind::Other,
    };

    let stbl = atom::path(data, ms, me, &[b"minf", b"stbl"])?;
    let (bs, be) = (stbl.body, stbl.body + stbl.body_len);

    let mut track = Track {
        kind,
        codec: *b"    ",
        timescale: timescale.max(1),
        duration,
        width: 0,
        height: 0,
        channels: 1,
        sample_rate: 0,
        samples_per_packet: 1,
        bytes_per_packet: 1,
        samples: Vec::new(),
    };

    // stsd: one description per format; the game only ever uses the first.
    if let Some(stsd) = atom::find(data, bs, be, b"stsd") {
        let entry = stsd.body + 8;
        track.codec = data.get(entry + 4..entry + 8)?.try_into().ok()?;
        match kind {
            TrackKind::Video => {
                track.width = u16_at(data, entry + 32);
                track.height = u16_at(data, entry + 34);
            }
            TrackKind::Sound => {
                track.channels = u16_at(data, entry + 24).max(1);
                // The rate is 16.16 fixed point.
                track.sample_rate = u32_at(data, entry + 32) >> 16;
                // A version 1 sound description declares its packet geometry;
                // version 0, which is what these files use, declares nothing and
                // the codec's own constants have to supply it.
                let sound_version = u16_at(data, entry + 16);
                if sound_version >= 1 {
                    track.samples_per_packet = u32_at(data, entry + 36).max(1);
                    track.bytes_per_packet = u32_at(data, entry + 40).max(1);
                } else if let Some((spp, bpp)) = codec_packet_geometry(&track.codec) {
                    track.samples_per_packet = spp;
                    track.bytes_per_packet = bpp;
                }
            }
            TrackKind::Other => {}
        }
    }

    let sizes = parse_stsz(data, bs, be);
    let (chunk_offsets, chunk_sizes) = parse_chunks(data, bs, be, &sizes);
    let times = parse_stts(data, bs, be);
    let syncs = parse_stss(data, bs, be);

    // Walk chunks in order, laying samples end to end inside each.
    //
    // For compressed audio a "sample" in these tables is a single decoded
    // audio frame, often declared as one byte, which is not independently
    // decodable: IMA ADPCM only makes sense in whole 34-byte packets. So sound
    // tracks are emitted at chunk granularity, which is the unit a player
    // actually consumes, while video stays one sample per frame.
    let coalesce = kind == TrackKind::Sound;
    // For compressed audio the sizes in `stsz` count decoded frames, not stored
    // bytes: these files declare one byte per frame, which is a placeholder
    // rather than a length. Convert through the packet geometry instead, or the
    // reader walks off the end of the track and into the next one's data.
    let frames_are_bytes = coalesce && track.samples_per_packet > 1;

    let mut samples = Vec::with_capacity(if coalesce { chunk_offsets.len() } else { sizes.len() });
    let mut index = 0usize;
    let mut time = 0u64;
    for (chunk, &base) in chunk_offsets.iter().enumerate() {
        let count = chunk_sizes.get(chunk).copied().unwrap_or(0);
        let mut offset = base as usize;
        let chunk_start = offset;
        let chunk_time = time;
        let mut chunk_len = 0usize;
        let mut chunk_duration = 0u32;
        let mut chunk_frames = 0usize;

        for _ in 0..count {
            let Some(&len) = sizes.get(index) else { break };
            let duration = times.get(index).copied().unwrap_or(0);
            if coalesce {
                chunk_len += len as usize;
                chunk_duration = chunk_duration.saturating_add(duration);
                chunk_frames += 1;
            } else {
                samples.push(Sample {
                    offset,
                    len: len as usize,
                    time,
                    duration,
                    // With no sync table every sample is a keyframe, which is
                    // what QuickTime means by omitting `stss`.
                    sync: syncs
                        .as_ref()
                        .map_or(true, |s| s.contains(&(index as u32 + 1))),
                });
            }
            offset += len as usize;
            time += duration as u64;
            index += 1;
        }

        if frames_are_bytes {
            let packets = chunk_frames / track.samples_per_packet as usize;
            chunk_len = packets * track.bytes_per_packet as usize * track.channels as usize;
            // Chunks from different tracks interleave in `mdat`, so a length
            // derived from frame counts must never be allowed to run past where
            // the next chunk of this track begins; reading into the neighbouring
            // track decodes as noise and saturates the ADPCM predictor.
            if let Some(&next) = chunk_offsets.get(chunk + 1) {
                if next as usize > chunk_start {
                    chunk_len = chunk_len.min(next as usize - chunk_start);
                }
            }
        }
        if coalesce && chunk_len > 0 {
            samples.push(Sample {
                offset: chunk_start,
                len: chunk_len,
                time: chunk_time,
                duration: chunk_duration,
                sync: true,
            });
        }
    }
    track.samples = samples;
    Some(track)
}

/// Packet geometry for codecs that do not declare their own, as
/// `(frames per packet, bytes per packet, per channel)`.
fn codec_packet_geometry(codec: &[u8; 4]) -> Option<(u32, u32)> {
    match codec {
        b"ima4" => Some((64, 34)),
        _ => None,
    }
}

/// Sample sizes, either a single shared size or one per sample.
fn parse_stsz(data: &[u8], start: usize, end: usize) -> Vec<u32> {
    let Some(stsz) = atom::find(data, start, end, b"stsz") else {
        return Vec::new();
    };
    let uniform = u32_at(data, stsz.body + 4);
    let count = u32_at(data, stsz.body + 8) as usize;
    if uniform != 0 {
        return vec![uniform; count];
    }
    (0..count)
        .map(|i| u32_at(data, stsz.body + 12 + i * 4))
        .collect()
}

/// Chunk file offsets and how many samples each chunk holds.
///
/// `stsc` is run-length encoded: an entry applies from its first chunk until
/// the next entry's first chunk, which is why this expands rather than indexes.
fn parse_chunks(data: &[u8], start: usize, end: usize, sizes: &[u32]) -> (Vec<u64>, Vec<u32>) {
    let offsets: Vec<u64> = if let Some(stco) = atom::find(data, start, end, b"stco") {
        let count = u32_at(data, stco.body + 4) as usize;
        (0..count)
            .map(|i| u32_at(data, stco.body + 8 + i * 4) as u64)
            .collect()
    } else if let Some(co64) = atom::find(data, start, end, b"co64") {
        let count = u32_at(data, co64.body + 4) as usize;
        (0..count)
            .map(|i| {
                let o = co64.body + 8 + i * 8;
                data.get(o..o + 8)
                    .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
                    .unwrap_or(0)
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut per_chunk = vec![0u32; offsets.len()];
    if let Some(stsc) = atom::find(data, start, end, b"stsc") {
        let count = u32_at(data, stsc.body + 4) as usize;
        let entries: Vec<(u32, u32)> = (0..count)
            .map(|i| {
                let o = stsc.body + 8 + i * 12;
                (u32_at(data, o), u32_at(data, o + 4))
            })
            .collect();
        for (i, &(first, samples)) in entries.iter().enumerate() {
            let last = entries
                .get(i + 1)
                .map(|(next, _)| *next as usize - 1)
                .unwrap_or(offsets.len());
            for c in (first as usize).saturating_sub(1)..last.min(offsets.len()) {
                per_chunk[c] = samples;
            }
        }
    } else if !offsets.is_empty() {
        // Without a sample-to-chunk table, assume one sample per chunk.
        let each = (sizes.len() / offsets.len().max(1)).max(1) as u32;
        per_chunk.fill(each);
    }

    (offsets, per_chunk)
}

/// Per-sample durations, expanded from the run-length `stts` table.
fn parse_stts(data: &[u8], start: usize, end: usize) -> Vec<u32> {
    let Some(stts) = atom::find(data, start, end, b"stts") else {
        return Vec::new();
    };
    let count = u32_at(data, stts.body + 4) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let o = stts.body + 8 + i * 8;
        let n = u32_at(data, o) as usize;
        let duration = u32_at(data, o + 4);
        // Guard against a corrupt count trying to allocate the world.
        out.extend(std::iter::repeat(duration).take(n.min(1 << 20)));
    }
    out
}

/// The keyframe list, absent when every sample is a keyframe.
fn parse_stss(data: &[u8], start: usize, end: usize) -> Option<std::collections::HashSet<u32>> {
    let stss = atom::find(data, start, end, b"stss")?;
    let count = u32_at(data, stss.body + 4) as usize;
    Some(
        (0..count)
            .map(|i| u32_at(data, stss.body + 8 + i * 4))
            .collect(),
    )
}
