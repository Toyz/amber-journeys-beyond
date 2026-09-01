use crate::chunk::{Endian, Reader};

/// A 256-entry RGB palette from a `CLUT` chunk.
///
/// Each component is stored 16-bit, of which only the high byte is meaningful
/// at this colour depth. Entries are in palette order: entry 0 on disk is
/// colour index 0.
#[derive(Clone)]
pub struct Palette {
    pub colors: [[u8; 3]; 256],
}

impl Default for Palette {
    fn default() -> Self {
        // A neutral grey ramp, so a movie missing its CLUT still renders shapes
        // instead of a black rectangle.
        let mut colors = [[0u8; 3]; 256];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = [i as u8, i as u8, i as u8];
        }
        Palette { colors }
    }
}

impl Palette {
    pub fn decode(data: &[u8], endian: Endian) -> Palette {
        let mut pal = Palette::default();
        let count = (data.len() / 6).min(256);
        let mut r = Reader::new(data, endian);
        for entry in pal.colors.iter_mut().take(count) {
            let red = r.u16().unwrap_or(0);
            let green = r.u16().unwrap_or(0);
            let blue = r.u16().unwrap_or(0);
            *entry = [(red >> 8) as u8, (green >> 8) as u8, (blue >> 8) as u8];
        }
        pal
    }

    #[inline]
    pub fn color(&self, index: u8) -> [u8; 3] {
        self.colors[index as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a CLUT payload from 8-bit triples, widened to the 16-bit
    /// components the chunk actually stores.
    fn clut(entries: &[[u8; 3]]) -> Vec<u8> {
        let mut out = Vec::new();
        for e in entries {
            for c in e {
                out.extend_from_slice(&[*c, 0]); // high byte first, big-endian
            }
        }
        out
    }

    #[test]
    fn entry_zero_on_disk_is_colour_index_zero() {
        // I decoded this table in reverse and it survived a 307,200-pixel
        // cross-check, because both implementations shared the assumption and
        // the frame I checked was nearly grey. Only a table whose ends differ
        // can tell the two apart.
        let data = clut(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
        let p = Palette::decode(&data, Endian::Big);
        assert_eq!(p.color(0), [255, 0, 0], "index 0 is the first entry on disk");
        assert_eq!(p.color(1), [0, 255, 0]);
        assert_eq!(p.color(2), [0, 0, 255]);
    }

    #[test]
    fn components_take_the_high_byte_of_each_16_bit_field() {
        let data = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let p = Palette::decode(&data, Endian::Big);
        assert_eq!(p.color(0), [0x12, 0x56, 0x9a]);
    }

    #[test]
    fn endianness_selects_which_byte_is_high() {
        let data = vec![0x00, 0xff, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(Palette::decode(&data, Endian::Big).color(0)[0], 0x00);
        assert_eq!(Palette::decode(&data, Endian::Little).color(0)[0], 0xff);
    }

    #[test]
    fn a_short_table_leaves_the_rest_of_the_ramp_alone() {
        // A movie with a partial CLUT should still render shapes rather than
        // a black rectangle.
        let p = Palette::decode(&clut(&[[1, 2, 3]]), Endian::Big);
        assert_eq!(p.color(0), [1, 2, 3]);
        assert_eq!(p.color(255), [255, 255, 255], "default grey ramp survives");
    }

    #[test]
    fn an_oversized_table_is_clamped_to_256_entries() {
        let p = Palette::decode(&vec![0u8; 6 * 400], Endian::Big);
        assert_eq!(p.color(255), [0, 0, 0]);
    }
}
