//! IMA ADPCM (`ima4`) audio decoder, in QuickTime's packet framing.
//!
//! Each packet is 34 bytes and yields 64 samples for one channel. The first two
//! bytes carry the initial predictor in the top 9 bits and the step index in the
//! bottom 7; the remaining 32 bytes hold 64 nibbles, low nibble first. Stereo
//! packets alternate between channels, one whole packet at a time.

const PACKET_BYTES: usize = 34;
const PACKET_SAMPLES: usize = 64;

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Streaming IMA ADPCM decoder.
///
/// The predictor has to survive across calls, not just across packets: the
/// header's quantised value only agrees with the encoder's true state to within
/// its low seven bits, so a decoder that restarts at every chunk boundary drifts
/// audibly. Holding the running state here is what takes reconstruction from
/// approximate to sample-exact.
#[derive(Default)]
pub struct Ima4Decoder {
    carried: Vec<i32>,
}

impl Ima4Decoder {
    pub fn new() -> Ima4Decoder {
        Ima4Decoder::default()
    }

    /// Decodes one chunk of packets, continuing from the previous call.
    pub fn decode(&mut self, data: &[u8], channels: u16) -> Vec<i16> {
        let channels = channels.max(1) as usize;
        self.carried.resize(channels, 0);

        let packets = data.len() / PACKET_BYTES;
        if packets == 0 {
            return Vec::new();
        }
        let frames = (packets / channels) * PACKET_SAMPLES;
        let mut out = vec![0i16; frames * channels];

        for (p, packet) in data.as_chunks::<PACKET_BYTES>().0.iter().enumerate() {
            let channel = p % channels;
            let frame_base = (p / channels) * PACKET_SAMPLES;
            if frame_base >= frames {
                break;
            }
            decode_packet(
                packet,
                &mut out,
                frame_base,
                channel,
                channels,
                &mut self.carried[channel],
            );
        }
        out
    }
}

/// Decodes a standalone run of packets, with no carried state.
///
/// Only correct for a whole track decoded in one call; use [`Ima4Decoder`] when
/// feeding chunk by chunk.
pub fn decode_ima4(data: &[u8], channels: u16) -> Vec<i16> {
    let channels = channels.max(1) as usize;
    let packets = data.len() / PACKET_BYTES;
    if packets == 0 {
        return Vec::new();
    }

    // Each channel decodes independently, then the streams are interleaved.
    let frames = (packets / channels) * PACKET_SAMPLES;
    let mut out = vec![0i16; frames * channels];

    // Running predictor per channel, carried between packets.
    let mut carried = vec![0i32; channels];

    for (p, packet) in data.as_chunks::<PACKET_BYTES>().0.iter().enumerate() {
        let channel = p % channels;
        let frame_base = (p / channels) * PACKET_SAMPLES;
        if frame_base >= frames {
            break;
        }
        decode_packet(
            packet,
            &mut out,
            frame_base,
            channel,
            channels,
            &mut carried[channel],
        );
    }
    out
}

fn decode_packet(
    packet: &[u8],
    out: &mut [i16],
    frame_base: usize,
    channel: usize,
    stride: usize,
    carried: &mut i32,
) {
    let header = u16::from_be_bytes([packet[0], packet[1]]);
    // The header holds the predictor in its top 9 bits and the step index in
    // the bottom 7. The predictor is therefore quantised to a multiple of 128,
    // which is a restart hint rather than an exact state: the encoder's true
    // predictor at this point still carries the low bits. Decoding from the
    // quantised value alone drifts from the reference by up to 127, so the
    // running predictor is kept whenever the header agrees with it to within
    // that quantisation.
    let header_predictor = (header & 0xff80) as i16 as i32;
    let mut predictor = if (*carried & !0x7f) == header_predictor {
        *carried
    } else {
        header_predictor
    };
    let mut index = ((header & 0x007f) as i32).min(88);

    for i in 0..PACKET_SAMPLES {
        let byte = packet[2 + i / 2];
        // Low nibble carries the earlier sample.
        let nibble = if i % 2 == 0 { byte & 0x0f } else { byte >> 4 } as i32;

        let step = STEP_TABLE[index as usize];
        // Reconstruct the magnitude as step * (nibble + 0.5) / 4, done in
        // integers the way the reference decoder does to stay bit-exact.
        let mut diff = step >> 3;
        if nibble & 4 != 0 {
            diff += step;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 1 != 0 {
            diff += step >> 2;
        }
        if nibble & 8 != 0 {
            predictor -= diff;
        } else {
            predictor += diff;
        }
        predictor = predictor.clamp(-32768, 32767);

        index = (index + INDEX_TABLE[nibble as usize]).clamp(0, 88);

        let slot = (frame_base + i) * stride + channel;
        if let Some(s) = out.get_mut(slot) {
            *s = predictor as i16;
        }
    }
    *carried = predictor;
}
