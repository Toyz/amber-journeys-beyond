use crate::chunk::{Endian, Reader};
use crate::{Error, Result};

/// PCM audio decoded from a `snd ` cast member.
pub struct Sound {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved signed 16-bit samples, whatever the source depth was.
    pub samples: Vec<i16>,
}

/// Director embeds Mac `snd ` resources. Format 1 carries a list of synth
/// commands before the header; format 2 is the older HyperCard shape. In both
/// cases the payload we want sits behind a "sound header" that is either the
/// standard 8-bit form or the extended/compressed form.
pub fn decode(data: &[u8], endian: Endian) -> Result<Sound> {
    let mut r = Reader::new(data, endian);
    let format = r.u16()?;

    match format {
        1 => {
            let modifier_count = r.u16()?;
            // Each modifier record is 6 bytes; we only need to step over them.
            for _ in 0..modifier_count {
                r.bytes(6)?;
            }
        }
        2 => {
            let _reference_count = r.u16()?;
        }
        _ => return Err(Error::Unsupported(format!("snd format {format}"))),
    }

    let command_count = r.u16()?;
    // Commands are 8 bytes each; the last one points at the buffer header.
    let mut buffer_offset = None;
    for _ in 0..command_count {
        let cmd = r.u16()?;
        let _param1 = r.u16()?;
        let param2 = r.u32()?;
        // bufferCmd (0x8051) and soundCmd (0x8050) carry the header offset.
        if cmd & 0x7fff == 0x0051 || cmd & 0x7fff == 0x0050 {
            buffer_offset = Some(param2 as usize);
        }
    }
    let start = buffer_offset.unwrap_or(r.pos);

    let mut h = Reader::at(data, endian, start);
    let _data_pointer = h.u32()?;
    let length_or_channels = h.u32()?;
    let rate_fixed = h.u32()?;
    let _loop_start = h.u32()?;
    let _loop_end = h.u32()?;
    let encoding = h.u8()?;
    let _base_frequency = h.u8()?;

    // Rate is 16.16 fixed point.
    let sample_rate = (rate_fixed >> 16).max(1);

    match encoding {
        // Standard header: samples follow immediately, 8-bit unsigned mono.
        0x00 => {
            let count = length_or_channels as usize;
            let raw = h.bytes(count.min(h.remaining()))?;
            Ok(Sound {
                sample_rate,
                channels: 1,
                samples: raw.iter().map(|&b| u8_to_i16(b)).collect(),
            })
        }
        // Extended header. Its layout is fixed, and the offsets below were
        // confirmed against the data rather than assumed: the frame count at
        // +22 matches the decoded length exactly, the sample size at +48 reads
        // 8, and the samples themselves begin at +64.
        //
        // Reading them from +52, as a naive field-by-field walk suggests,
        // pulls twelve bytes of header in as audio. Those bytes are zero, and
        // zero in unsigned 8-bit is full-scale negative, so every sound opened
        // with a loud click. On a three-second ambient loop that is a thump
        // every three seconds, for as long as the room is occupied.
        0xff => {
            const FRAME_COUNT: usize = 22;
            const SAMPLE_SIZE: usize = 48;
            const SAMPLE_DATA: usize = 64;

            let channels = length_or_channels.max(1) as u16;
            let frames = {
                let mut f = Reader::at(data, endian, start + FRAME_COUNT);
                f.u32()? as usize
            };
            let bits = {
                let mut b = Reader::at(data, endian, start + SAMPLE_SIZE);
                b.u16()?
            };

            let mut s = Reader::at(data, endian, start + SAMPLE_DATA);
            let total = frames.saturating_mul(channels as usize);
            let mut samples = Vec::with_capacity(total.min(1 << 24));
            if bits == 16 {
                for _ in 0..total.min(s.remaining() / 2) {
                    samples.push(s.i16()?);
                }
            } else {
                for _ in 0..total.min(s.remaining()) {
                    samples.push(u8_to_i16(s.u8()?));
                }
            }
            Ok(Sound {
                sample_rate,
                channels,
                samples,
            })
        }
        other => Err(Error::Unsupported(format!("snd encoding {other:#x}"))),
    }
}

#[inline]
fn u8_to_i16(b: u8) -> i16 {
    ((b as i16) - 128) << 8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assembles a format-1 `snd ` whose bufferCmd points at an extended
    /// header, so the offsets below are exercised the way the real resources
    /// exercise them.
    fn extended_snd(rate: u32, frames: u32, bits: u16, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&1u16.to_be_bytes()); // format
        v.extend_from_slice(&0u16.to_be_bytes()); // modifier count
        v.extend_from_slice(&1u16.to_be_bytes()); // command count
        v.extend_from_slice(&0x8051u16.to_be_bytes()); // bufferCmd
        v.extend_from_slice(&0u16.to_be_bytes()); // param1
        let start = 14u32;
        v.extend_from_slice(&start.to_be_bytes()); // param2: header offset
        assert_eq!(v.len(), start as usize);

        let mut h = vec![0u8; 64];
        h[4..8].copy_from_slice(&1u32.to_be_bytes()); // channels
        h[8..12].copy_from_slice(&(rate << 16).to_be_bytes()); // 16.16 rate
        h[20] = 0xff; // extended encoding
        h[22..26].copy_from_slice(&frames.to_be_bytes());
        h[48..50].copy_from_slice(&bits.to_be_bytes());
        v.extend_from_slice(&h);
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn extended_samples_begin_at_sixty_four() {
        // Walking the header field by field suggests +52, which pulls twelve
        // bytes of header in as audio. Those bytes are zero, and zero in
        // unsigned 8-bit is full-scale negative, so every sound opened with a
        // loud click -- on an ambient loop, a thump every few seconds for as
        // long as the player stayed in the room.
        //
        // Every loop in the game reported a peak of exactly 32768 and I
        // explained that away once before believing it.
        let snd = extended_snd(22050, 4, 8, &[0x80, 0xff, 0x00, 0x80]);
        let out = decode(&snd, Endian::Big).expect("decodes");

        assert_eq!(out.sample_rate, 22050);
        assert_eq!(out.channels, 1);
        assert_eq!(out.samples, vec![0, 32512, -32768, 0]);
        assert_ne!(
            out.samples[0], -32768,
            "a full-scale negative first sample is the click"
        );
    }

    #[test]
    fn no_loop_peaks_at_exactly_full_scale_by_accident() {
        // The signature of the misread: a run of header zeroes decoding to a
        // string of identical full-scale samples at the head of the buffer.
        let snd = extended_snd(22050, 8, 8, &[0x80; 8]);
        let out = decode(&snd, Endian::Big).unwrap();
        assert!(
            out.samples.iter().all(|&s| s == 0),
            "silence in must be silence out, got {:?}",
            &out.samples[..4]
        );
    }

    #[test]
    fn sixteen_bit_payloads_are_read_as_pairs() {
        let payload: Vec<u8> = [1000i16, -1000, 32767, -32768]
            .iter()
            .flat_map(|s| s.to_be_bytes())
            .collect();
        let snd = extended_snd(44100, 4, 16, &payload);
        let out = decode(&snd, Endian::Big).unwrap();
        assert_eq!(out.samples, vec![1000, -1000, 32767, -32768]);
        assert_eq!(out.sample_rate, 44100);
    }

    #[test]
    fn a_truncated_payload_yields_what_is_there_rather_than_reading_past_it() {
        let snd = extended_snd(22050, 64, 8, &[0x80, 0x90]);
        let out = decode(&snd, Endian::Big).unwrap();
        assert_eq!(out.samples.len(), 2, "frame count is a claim, not a promise");
    }

    #[test]
    fn eight_bit_conversion_is_centred_on_128() {
        assert_eq!(u8_to_i16(128), 0);
        assert_eq!(u8_to_i16(0), -32768);
        assert_eq!(u8_to_i16(255), 32512);
    }

    #[test]
    fn an_unknown_format_is_refused_rather_than_guessed() {
        assert!(decode(&[0x00, 0x09, 0, 0], Endian::Big).is_err());
    }
}
