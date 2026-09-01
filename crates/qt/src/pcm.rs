//! Uncompressed QuickTime audio.
//!
//! Three of the four sound codecs on the disc carry samples directly rather
//! than compressing them, and each stores them differently:
//!
//! | codec  | layout                                        |
//! |--------|-----------------------------------------------|
//! | `raw ` | unsigned 8-bit, silence at 128                 |
//! | `twos` | signed 16-bit, big-endian                      |
//! | `sowt` | signed 16-bit, little-endian ("twos" reversed) |
//!
//! `raw ` is the one that matters: sixteen of the game's soundtracks use it,
//! and reading its silence byte of 0x80 as a signed value gives -32768, full
//! scale negative, on every sample that is not making a sound. A whole track
//! of that is not quiet audio -- it is the loudest possible noise.

/// Whether this codec is uncompressed audio this module can decode.
pub fn handles(codec: &[u8; 4]) -> bool {
    matches!(codec, b"raw " | b"twos" | b"sowt" | b"NONE")
}

/// Decodes one chunk of uncompressed audio to signed 16-bit.
pub fn decode(codec: &[u8; 4], bits: u16, data: &[u8]) -> Vec<i16> {
    match codec {
        // `NONE` is the same thing as `raw `, and its sample size decides
        // whether it is the 8-bit or the 16-bit form.
        b"raw " | b"NONE" if bits <= 8 => data.iter().map(|&b| unsigned8(b)).collect(),
        b"sowt" => data
            .as_chunks::<2>().0.iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect(),
        _ => data
            .as_chunks::<2>().0.iter()
            .map(|c| i16::from_be_bytes([c[0], c[1]]))
            .collect(),
    }
}

/// Centres an unsigned 8-bit sample and widens it.
#[inline]
fn unsigned8(b: u8) -> i16 {
    ((b as i16) - 128) << 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_eight_bit_is_centred_on_128() {
        // Silence is 0x80. Read as signed it is -32768, which is why a whole
        // track of it sounds like the speakers are failing.
        assert_eq!(decode(b"raw ", 8, &[128]), [0]);
        assert_eq!(decode(b"raw ", 8, &[0]), [-32768]);
        assert_eq!(decode(b"raw ", 8, &[255]), [32512]);
    }

    #[test]
    fn silence_decodes_to_silence() {
        let quiet = decode(b"raw ", 8, &[0x80; 64]);
        assert!(quiet.iter().all(|&s| s == 0));
    }

    #[test]
    fn twos_is_big_endian_and_sowt_is_little() {
        assert_eq!(decode(b"twos", 16, &[0x12, 0x34]), [0x1234]);
        assert_eq!(decode(b"sowt", 16, &[0x34, 0x12]), [0x1234]);
    }

    #[test]
    fn a_trailing_odd_byte_is_dropped_rather_than_read_past() {
        assert_eq!(decode(b"twos", 16, &[0x12, 0x34, 0x56]).len(), 1);
    }

    #[test]
    fn only_the_uncompressed_codecs_are_claimed() {
        assert!(handles(b"raw "));
        assert!(handles(b"twos"));
        assert!(handles(b"sowt"));
        assert!(!handles(b"ima4"), "ima4 is compressed and carries state");
        assert!(!handles(b"cvid"));
    }
}
