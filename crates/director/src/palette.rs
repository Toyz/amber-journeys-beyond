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
