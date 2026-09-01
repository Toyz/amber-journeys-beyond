//! The inventory bar.
//!
//! Eight hundred hotspots in the game are `#itemInUse`, meaning they only fire
//! while the player is holding the right thing. Without a way to choose what is
//! held, all of them are unreachable, so this is the difference between walking
//! around the house and playing the game.
//!
//! Each item has a pair of 67x67 icons, plain and lit, listed in the movie's
//! `inventory.DATA` cast member as `#ScanDevice: [954, 955]`. The lit icon
//! marks the item currently in hand.

use std::collections::HashMap;

use lingo::{parse_value, Value};

/// Icon size on the disc; every item's art is square and this size.
pub const ICON: i32 = 67;

/// Cast numbers for one item's icons.
#[derive(Copy, Clone, Debug)]
pub struct Icons {
    pub plain: u32,
    pub lit: u32,
}

pub struct Inventory {
    icons: HashMap<String, Icons>,
}

impl Inventory {
    /// Reads the icon table from a movie's text chunks.
    ///
    /// Recognised by its values: every entry maps a name to a short list of
    /// cast numbers, and the table names the items the game knows about.
    pub fn from_texts(texts: &[String]) -> Inventory {
        let mut icons = HashMap::new();
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.starts_with("[#") || !trimmed.contains("ScanDevice") {
                continue;
            }
            let Ok(value) = parse_value(trimmed) else { continue };
            for (name, casts) in value.entries() {
                let Value::List(items) = casts else { continue };
                let nums: Vec<u32> = items
                    .iter()
                    .filter_map(Value::as_int)
                    .filter(|n| *n > 0)
                    .map(|n| n as u32)
                    .collect();
                if let [plain, lit, ..] = nums[..] {
                    icons.insert(name.clone(), Icons { plain, lit });
                }
            }
        }
        Inventory { icons }
    }

    pub fn icons(&self, item: &str) -> Option<Icons> {
        self.icons.get(&item.to_ascii_lowercase()).copied()
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// Where each held item's icon sits, as `(item, x, y)`.
    ///
    /// The bar runs along the bottom of the stage, centred, which is clear of
    /// the room art: rooms are drawn at most 452 pixels tall on a 480 stage
    /// and centred, so the lowest band is free.
    pub fn layout(&self, held: &[String], stage_w: i32, stage_h: i32) -> Vec<(String, i32, i32)> {
        let shown: Vec<&String> = held.iter().filter(|i| self.icons(i).is_some()).collect();
        if shown.is_empty() {
            return Vec::new();
        }
        let total = shown.len() as i32 * ICON;
        let x0 = (stage_w - total) / 2;
        let y = stage_h - ICON;
        shown
            .iter()
            .enumerate()
            .map(|(i, item)| ((*item).clone(), x0 + i as i32 * ICON, y))
            .collect()
    }

    /// The item whose icon covers a point, if any.
    pub fn hit(&self, held: &[String], stage_w: i32, stage_h: i32, x: i32, y: i32) -> Option<String> {
        self.layout(held, stage_w, stage_h)
            .into_iter()
            .find(|(_, ix, iy)| x >= *ix && x < ix + ICON && y >= *iy && y < iy + ICON)
            .map(|(item, _, _)| item)
    }
}
