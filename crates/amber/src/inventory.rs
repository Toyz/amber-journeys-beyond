//! The inventory bar.
//!
//! Eight hundred hotspots in the game are `#itemInUse`, meaning they only fire
//! while the player is holding the right thing. Without a way to choose what is
//! held, all of them are unreachable, so this is the difference between walking
//! around the house and playing the game.
//!
//! Each item has a pair of 67x67 icons, listed in the movie's
//! `inventory.DATA` cast member as `#ScanDevice: [954, 955]`. The second icon
//! marks the item currently in hand.

use std::collections::HashMap;

use lingo::{parse_value, Value};

/// Icon size on the disc; every item's art is square and this size.
pub const ICON: i32 = 67;

/// Cast numbers for one item's icons.
///
/// Most items list two. The peek unit lists three, the extra
/// being a brighter glow it alternates with while alerting.
#[derive(Clone, Debug)]
pub struct Icons {
    /// Full colour, shown while the cursor is over the inventory bar --
    /// `updateInventory #hot` takes `getAt(itemData, 1)`.
    pub hot: u32,
    /// A glowing outline, shown while the cursor is anywhere else, from
    /// `getAt(itemData, 2)`.
    pub cool: u32,
    /// Every cast the item lists, in order, for handlers that index them.
    pub all: Vec<u32>,
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
                if let [hot, cool, ..] = nums[..] {
                    icons.insert(
                        name.clone(),
                        Icons {
                            hot,
                            cool,
                            all: nums.clone(),
                        },
                    );
                }
            }
        }
        Inventory { icons }
    }

    pub fn icons(&self, item: &str) -> Option<Icons> {
        self.icons.get(&item.to_ascii_lowercase()).cloned()
    }

    /// One of an item's icons by position, as `getAt` reads them.
    pub fn icon_at(&self, item: &str, index: usize) -> Option<u32> {
        let icons = self.icons.get(&item.to_ascii_lowercase())?;
        icons.all.get(index.checked_sub(1)?).copied()
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

    /// The top of the inventory bar, which is what `gInventoryTopY` is.
    ///
    /// The cursor being below this is what turns the icons from outlines to
    /// full colour; the original compares `the mouseV` against it once a
    /// frame from `idle`.
    pub fn top_y(stage_h: i32) -> i32 {
        stage_h - ICON
    }

    /// The item whose icon covers a point, if any.
    pub fn hit(&self, held: &[String], stage_w: i32, stage_h: i32, x: i32, y: i32) -> Option<String> {
        self.layout(held, stage_w, stage_h)
            .into_iter()
            .find(|(_, ix, iy)| x >= *ix && x < ix + ICON && y >= *iy && y < iy + ICON)
            .map(|(item, _, _)| item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Inventory {
        Inventory::from_texts(&["[#ScanDevice: [954, 955], #Crowbar: [960, 961]]".to_string()])
    }

    #[test]
    fn the_first_icon_is_the_lit_one_and_the_second_the_outline() {
        // `updateInventory` takes `getAt(itemData, 1)` for `#hot` and
        // `getAt(itemData, 2)` for `#cool`, and the hint book describes what
        // those look like: full colour under the cursor, a glowing outline
        // away from it.
        let inv = table();
        let icons = inv.icons("ScanDevice").expect("in the table");
        assert_eq!(icons.hot, 954);
        assert_eq!(icons.cool, 955);
    }

    #[test]
    fn the_bar_starts_one_icon_up_from_the_bottom() {
        // `gInventoryTopY`, which the cursor crossing is measured against.
        assert_eq!(Inventory::top_y(480), 480 - ICON);
    }

    #[test]
    fn an_item_with_no_icons_takes_no_room_in_the_bar() {
        let inv = table();
        let held = ["ScanDevice".to_string(), "Nonesuch".to_string()];
        let placed = inv.layout(&held, 640, 480);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].1, (640 - ICON) / 2, "the one icon is centred");
    }
}
