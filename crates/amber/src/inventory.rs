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
    /// `updateInventory` says exactly where the bar is, and it is not centred:
    ///
    /// ```text
    /// itemV = 410 : itemH = 110
    /// repeat with i = 1 to 7
    ///   ...
    ///   set the loc of sprite itemSprite = point( itemH, itemV ) + gOriginPoint
    ///   itemH = itemH + 70
    /// ```
    ///
    /// Seven fixed slots, left-aligned from 110 and 70 apart, all on one row
    /// whose centre line is 410. `the loc` is the registration point, and
    /// every icon is 67 by 67 registered at (33, 33), so a slot's top-left
    /// corner is 33 up and 33 left of its point.
    ///
    /// Centring the whole group at the very bottom of the stage instead --
    /// which is what this did -- put the bar 36 pixels too low and moved every
    /// icon whenever anything was picked up or put down.
    pub fn layout(
        &self,
        slots: impl Iterator<Item = (usize, String)>,
        _stage_w: i32,
        _stage_h: i32,
    ) -> Vec<(String, i32, i32)> {
        const FIRST: i32 = 110;
        const STEP: i32 = 70;
        const ROW: i32 = 410;
        slots
            .filter(|(_, item)| self.icons(item).is_some())
            .map(|(slot, item)| {
                (
                    item,
                    FIRST + (slot as i32 - 1) * STEP - ICON / 2,
                    ROW - ICON / 2,
                )
            })
            .collect()
    }

    /// The top of the inventory bar, which is what `gInventoryTopY` is.
    ///
    /// The cursor being below this is what turns the icons from outlines to
    /// full colour; the original compares `the mouseV` against it once a
    /// frame from `idle`. `birth` sets it to 380 on a PC, where
    /// `gOriginPoint` is the origin -- three pixels above the icons, not at
    /// the bottom of the stage.
    pub fn top_y(_stage_h: i32) -> i32 {
        380
    }

    /// The item whose icon covers a point, if any.
    pub fn hit(
        &self,
        slots: impl Iterator<Item = (usize, String)>,
        stage_w: i32,
        stage_h: i32,
        x: i32,
        y: i32,
    ) -> Option<String> {
        self.layout(slots, stage_w, stage_h)
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
    fn the_bar_top_is_where_birth_puts_it() {
        // `birth` sets `gInventoryTopY = 380` on a PC, and that is what the
        // cursor crossing is measured against -- not the bottom of the stage.
        assert_eq!(Inventory::top_y(480), 380);
    }

    #[test]
    fn an_item_with_no_icons_takes_no_room_in_the_bar() {
        let inv = table();
        let held = [(4, "ScanDevice".to_string()), (5, "Nonesuch".to_string())];
        let placed = inv.layout(held.into_iter(), 640, 480);
        assert_eq!(placed.len(), 1);
        // `updateInventory` puts the first slot's registration point at
        // (110, 410), and the icon is registered at its own centre.
        assert_eq!(placed[0].1, 110 + 3 * 70 - ICON / 2, "slot 4 is the middle");
        assert_eq!(placed[0].2, 410 - ICON / 2);
    }

    #[test]
    fn the_slots_are_fixed_and_seventy_apart() {
        // Left-aligned from 110, not centred: an item is in the same place
        // whatever else is being carried, which is what makes a recorded
        // click on the bar mean the same thing twice.
        let inv = table();
        let one = inv.layout([(4, "ScanDevice".to_string())].into_iter(), 640, 480);
        let two = inv.layout(
            [(4, "ScanDevice".to_string()), (5, "Crowbar".to_string())].into_iter(),
            640,
            480,
        );
        assert_eq!(one[0].1, two[0].1, "a slot does not move when another fills");
        assert_eq!(two[1].1 - two[0].1, 70);
    }

    #[test]
    fn the_bar_holds_seven() {
        // `repeat with i = 1 to 7`, and the eighth slot would run off the
        // right-hand edge: 110 + 7 * 70 is 600, and the icon is 67 wide.
        let inv = table();
        let held: Vec<(usize, String)> =
            (1..=7).map(|i| (i, "Crowbar".to_string())).collect();
        let placed = inv.layout(held.into_iter(), 640, 480);
        assert_eq!(placed.len(), 7);
        assert!(placed[0].1 >= 0);
        assert!(placed.last().unwrap().1 + ICON <= 640);
    }
}
