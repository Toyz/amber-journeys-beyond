//! Game state: the flag store the room conditions read, plus inventory.

use std::collections::BTreeMap;

use lingo::Value;

use crate::world::Cond;

/// The mutable half of a save file.
///
/// Amber keeps its progress in one property list on a Lingo object the scripts
/// call `oStoryteller`, addressed by symbol. Every flag holds a *list*, and the
/// current value is its first element -- the game's own accessors say so
/// exactly:
///
/// ```text
/// on getState me, stateVar
///   return getAt( getProp(me.states, stateVar), 1 )
///
/// on setState me, stateVar, suggestion
///   valueList = getProp(me.states, stateVar)
///   if count(valueList) > 1 then
///     oldPos = getPos(valueList, suggestion)
///     if oldPos then addAt(valueList, 1, suggestion)
///                    deleteAt(valueList, oldPos + 1)
/// ```
///
/// So a flag's list is at once its current value and the set of settings it may
/// legally take, and writing one moves it to the front rather than replacing
/// anything. That single shape covers what looked like three separate kinds of
/// flag: a scalar is a one-element list, an enumeration is a list whose head is
/// the current choice, and a pool is a list nothing reads the head of.
///
/// Modelling flags as scalars-or-lists instead, with the list ones guessed from
/// how they were used, got `#tunedIn` wrong: it is tested with `#includes` in
/// eleven rooms, so it looked like a pool, but a sprite indexes its art by it
/// and wanted the head.
#[derive(Clone, Default, Debug)]
pub struct State {
    /// Flags, lower-cased keys to match Lingo's case-insensitive symbols.
    /// Each holds its whole value list; element 0 is the current setting.
    props: BTreeMap<String, Vec<Value>>,
    /// Everything the player is carrying, in slot order.
    ///
    /// Kept in step with `slots`, since most callers only want the list.
    inventory: Vec<String>,
    /// The seven slots the inventory bar draws, `#None` where empty.
    ///
    /// `lsInventory` is a fixed seven-element list and an item keeps its
    /// place in it, which is what makes a click on the bar mean the same
    /// thing twice. Slot 4 is the middle of the bar and the three tools have
    /// homes around it: see `place`.
    slots: [Option<String>; 7],
    /// The item currently held over the scene, if any.
    item_in_use: Option<String>,
}

impl State {
    pub fn new() -> State {
        State::default()
    }

    pub fn get(&self, key: &str) -> Value {
        let key = key.to_ascii_lowercase();
        // The two inventory-flavoured keys are views over dedicated storage
        // rather than plain flags, because the actions manipulate them as lists.
        match key.as_str() {
            "iteminuse" => match &self.item_in_use {
                Some(i) => Value::Symbol(i.clone()),
                None => Value::Symbol("None".into()),
            },
            "inventory" => Value::List(
                self.inventory
                    .iter()
                    .map(|i| Value::Symbol(i.clone()))
                    .collect(),
            ),
            // `getState` is `getAt(list, 1)`: the head is the current value.
            _ => self
                .props
                .get(&key)
                .and_then(|v| v.first())
                .cloned()
                .unwrap_or(Value::Void),
        }
    }

    /// Every setting a flag holds, head first.
    ///
    /// The head is the current value; the rest are the other settings it may
    /// take, or -- for a pool like `#hauntsRemaining` -- what is left in it.
    pub fn get_all(&self, key: &str) -> &[Value] {
        self.props
            .get(&key.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Replaces a flag's whole value list, as a custom setter does.
    pub fn set_all(&mut self, key: &str, values: Vec<Value>) {
        trace!(crate::trace::Topic::State, "set {key} = {values:?}");
        self.props.insert(key.to_ascii_lowercase(), values);
    }

    /// Writes a flag without announcing it.
    ///
    /// Seeding a chapter writes several hundred flags at once, and a trace of
    /// that buries the handful of writes anyone is reading the log for.
    pub fn seed_flag(&mut self, key: &str, values: Vec<Value>) {
        self.props.insert(key.to_ascii_lowercase(), values);
    }

    /// Keeps the `playerHas<Item>` flag in step with what is carried.
    ///
    /// Rooms hide a taken object by drawing an "object gone" plate over the
    /// scene, gated on one of these flags; there are eight of them and 185
    /// references. The chapter schema seeds them all to zero, so they cannot
    /// be derived when read - a stored value always exists - and taking an
    /// item has to write the flag, which is what the compiled `addInventory`
    /// does alongside its slot bookkeeping.
    ///
    /// The one explicit assignment in the game, setting the headgear to
    /// `#usedUp` once consumed, still applies afterwards and is not disturbed.
    fn sync_possession(&mut self, item: &str, held: bool) {
        // `addInventory` ends with `setState( #playerHas<Item>, #carrying )`,
        // so this goes through the same write and keeps the flag's declared
        // settings behind the current one.
        //
        // Replacing the list with a single value instead lost two things.
        // The flag never held `#carrying`, so `setScanStatus`'s test for it
        // could not be true and the interrupted-scan message was unreachable;
        // and a one-entry list is this engine's signal that a `set<Flag>`
        // handler exists, so picking anything up quietly changed how writes
        // to that flag were dispatched.
        //
        // `deleteInventory` does not write zero. It writes `#usedUp`, and
        // makes two exceptions:
        //
        // ```text
        // if whichItem = #ScanDevice or whichItem = #Headgear then
        //   if whichItem = #ScanDevice then setState( #playerHasScanDevice, 0 )
        //   if whichItem = #Headgear   then setState( #playerHasHeadgear, #inUse )
        // else
        //   setState( value("#playerHas" & whichItem), #usedUp )
        // ```
        //
        // The scan device is put down and can be picked up again, so it goes
        // back to zero; the headgear is worn rather than spent. Everything
        // else is spent, and the difference is load-bearing: the door into
        // the 1940s bedroom opens on `#playerHasBedroomKey = #usedUp`, so
        // writing zero here turned the key in the lock and left the door
        // shut -- which is the door the whole second half of the game is
        // behind.
        let key = format!("playerHas{item}");
        let value = if held {
            Value::Symbol("carrying".into())
        } else if item.eq_ignore_ascii_case("ScanDevice") {
            Value::Int(0)
        } else if item.eq_ignore_ascii_case("Headgear") {
            Value::Symbol("inUse".into())
        } else {
            Value::Symbol("usedUp".into())
        };
        self.set(&key, value);
    }

    /// Whether the `playerHas<Item>` flag says the player has the item.
    ///
    /// The scripts always ask this as `getState( #playerHas<Item> ) = 0`, so
    /// anything that is not zero counts -- `#carrying`, `#inUse`, and
    /// `#usedUp` alike. That last one is deliberate on the game's part: an
    /// item that has been used up is still not zero, and the handlers that
    /// care about the difference test for `#usedUp` by name.
    pub fn carrying(&self, item: &str) -> bool {
        !self.get(&format!("playerHas{item}")).loosely_eq(&Value::Int(0))
    }

    pub fn set(&mut self, key: &str, value: Value) {
        let key = key.to_ascii_lowercase();
        if key == "iteminuse" {
            self.item_in_use = match &value {
                Value::Symbol(s) | Value::String(s) if !s.eq_ignore_ascii_case("none") => {
                    Some(s.clone())
                }
                _ => None,
            };
            return;
        }
        // `setState` moves the suggestion to the head of the list, keeping the
        // rest as the settings the flag may still take. A value the list does
        // not already hold is inserted rather than refused: the original answers
        // `#badValue` and leaves the flag alone, but a write this engine fails
        // to recognise would then freeze whatever it gates, and a room the
        // player cannot leave is a worse failure than a flag with one extra
        // legal setting.
        trace!(crate::trace::Topic::State, "set {key} = {value:?}");
        let slot = self.props.entry(key.clone()).or_default();
        match slot.iter().position(|v| v.loosely_eq(&value)) {
            Some(0) => {}
            Some(i) => {
                slot.remove(i);
                slot.insert(0, value);
            }
            None => slot.insert(0, value),
        }
    }

    /// Drops a flag entirely; a missing flag reads back as `Void` and so fails
    /// an equality test rather than matching zero.
    pub fn trim(&mut self, key: &str) {
        self.props.remove(&key.to_ascii_lowercase());
    }

    /// Adds an entry to the list a flag holds, if it is not already there.
    ///
    /// The counterpart of [`trim_item`](Self::trim_item). The control panel
    /// collects pressed buttons this way, so treating it as a plain write
    /// leaves the set holding only whichever button was pressed last and the
    /// puzzle can never be satisfied.
    pub fn add_item(&mut self, key: &str, item: Value) {
        let key = key.to_ascii_lowercase();
        trace!(crate::trace::Topic::State, "add {key} += {item:?}");
        let slot = self.props.entry(key).or_default();
        if !slot.iter().any(|v| v.loosely_eq(&item)) {
            slot.push(item);
        }
    }

    /// Removes one entry from the list a flag holds.
    ///
    /// This is what `trimState` does. Every call in the game passes a list and
    /// an item - `trimState( #hauntsRemaining, #gazebo2 )` - and each haunt
    /// trims itself once it has played, so the list is how the house runs out
    /// of things to do. Treating the second argument as the flag to delete
    /// leaves that list untouched and every haunt repeats for ever.
    pub fn trim_item(&mut self, key: &str, item: &Value) {
        let key = key.to_ascii_lowercase();
        trace!(crate::trace::Topic::State, "trim {key} -= {item:?}");
        if let Some(items) = self.props.get_mut(&key) {
            items.retain(|i| !i.loosely_eq(item));
        }
    }

    /// Every flag currently set, for inspection from the walkthrough.
    /// Replaces the whole store, for a save being loaded.
    ///
    /// Not `set_all` in a loop: a flag the save does not mention must not
    /// survive from the game that was running. And the inventory is put back
    /// by slot rather than re-added item by item, because a slot is a position
    /// on the bar and `place` would re-derive it -- which gives the same answer
    /// only if the items happen to be handed back in the order they were first
    /// picked up.
    pub fn restore(
        &mut self,
        props: Vec<(String, Vec<Value>)>,
        slots: [Option<String>; 7],
        in_hand: Option<String>,
    ) {
        self.props = props
            .into_iter()
            .map(|(key, values)| (key.to_ascii_lowercase(), values))
            .collect();
        self.slots = slots;
        self.inventory = self.slots.iter().flatten().cloned().collect();
        self.item_in_use = in_hand;
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &[Value])> {
        self.props.iter().map(|(k, v)| (k, v.as_slice()))
    }

    pub fn inventory(&self) -> &[String] {
        &self.inventory
    }

    pub fn add_inventory(&mut self, item: &str) {
        self.place(item);
        self.sync_possession(item, true);
    }

    /// Leading empty slots among 1..3, and trailing empty slots among 7..5.
    fn open_sides(&self) -> (usize, usize) {
        let left = self.slots[..3].iter().take_while(|s| s.is_none()).count();
        let right = self.slots[4..].iter().rev().take_while(|s| s.is_none()).count();
        (left, right)
    }

    fn slot_of(&self, item: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(item)))
            .map(|i| i + 1)
    }

    /// Puts an item into the bar where `addInventory` puts it.
    ///
    /// ```text
    /// if getAt( inventoryList, 4 ) = #None then
    ///   setAt( inventoryList, 4, whichItem )
    /// else if whichItem = #PeekUnit then
    ///   ... take slot 4, pushing whatever is there aside ...
    ///   addAt( inventoryList, 4, whichItem )
    /// else if whichItem = #ScanDevice then
    ///   if leftSlotsOpen < 1 then deleteAt(list,7) : addAt(list, 8 - rightSlotsOpen, item)
    ///   else                      deleteAt(list,1) : addAt(list, leftSlotsOpen, item)
    /// else if whichItem = #Headgear then      -- the mirror of that
    /// else
    ///   whichSide = #right
    ///   if leftSlotsOpen > rightSlotsOpen then whichSide = #left
    ///   ... insert on that side, packed towards the middle ...
    /// ```
    ///
    /// So the bar is not a queue. The PeeK always ends up dead centre, which
    /// is why `peekAlert` can flash sprite 7 and know it is the PeeK; the scan
    /// device favours the left and the headgear the right; anything else fills
    /// inward from whichever side has more room. Packing items left to right
    /// in the order they were picked up, which is what this did, put the whole
    /// bar in the wrong place and moved every icon whenever anything changed.
    fn place(&mut self, item: &str) {
        if self.slot_of(item).is_some() {
            return;
        }
        if self.slots[3].is_none() {
            self.slots[3] = Some(item.to_owned());
            self.resync();
            return;
        }
        let (left, right) = self.open_sides();
        let is = |name: &str| item.eq_ignore_ascii_case(name);
        let centre_is =
            |name: &str| self.slots[3].as_deref().is_some_and(|s| s.eq_ignore_ascii_case(name));

        let (drop_at, insert_at) = if is("PeekUnit") {
            let drop_at = if centre_is("ScanDevice") {
                if left > 0 { 1 } else { 7 }
            } else if centre_is("Headgear") {
                if right > 0 { 7 } else { 1 }
            } else if left > right {
                1
            } else {
                7
            };
            (drop_at, 4)
        } else if is("ScanDevice") && left >= 1 {
            (1, left)
        } else if is("Headgear") && right >= 1 {
            (7, 8 - right)
        } else if is("ScanDevice") || is("Headgear") {
            // The favoured side is full, so the item takes the other one.
            if is("ScanDevice") { (7, 8 - right) } else { (1, left) }
        } else if left > right {
            (1, left)
        } else {
            (7, 8 - right)
        };

        // The bar is full when neither end is empty; dropping a real item to
        // make room would lose it, so refuse instead. Nothing in the game
        // hands the player an eighth thing, and losing one silently would be
        // far worse than a bar that is one short.
        if self.slots[drop_at - 1].is_some() {
            return;
        }
        let mut list: Vec<Option<String>> = self.slots.to_vec();
        list.remove(drop_at - 1);
        list.insert(insert_at.clamp(1, 7) - 1, Some(item.to_owned()));
        self.take_slots(list);
    }

    /// Frees an item's slot, as `deleteInventory` does.
    ///
    /// The half the item was in closes up towards the middle and the empty
    /// arrives at that end; taking the middle one pulls a neighbour in from
    /// whichever side has more room.
    fn free(&mut self, item: &str) {
        let Some(pos) = self.slot_of(item) else { return };
        let mut list: Vec<Option<String>> = self.slots.to_vec();
        match pos.cmp(&4) {
            std::cmp::Ordering::Less => {
                list.remove(pos - 1);
                list.insert(0, None);
            }
            std::cmp::Ordering::Greater => {
                list.remove(pos - 1);
                list.push(None);
            }
            std::cmp::Ordering::Equal => {
                let (left, right) = self.open_sides();
                if left > right {
                    list.remove(3);
                    list.insert(0, None);
                } else {
                    list.insert(6, None);
                    list.remove(3);
                }
            }
        }
        self.take_slots(list);
    }

    fn take_slots(&mut self, list: Vec<Option<String>>) {
        for (slot, value) in self.slots.iter_mut().zip(list.into_iter().chain(std::iter::repeat(None))) {
            *slot = value;
        }
        self.resync();
    }

    fn resync(&mut self) {
        self.inventory = self.slots.iter().flatten().cloned().collect();
    }

    /// Where each carried item sits in the bar, as one-based slot numbers.
    pub fn slots(&self) -> impl Iterator<Item = (usize, &str)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_deref().map(|item| (i + 1, item)))
    }

    pub fn delete_inventory(&mut self, item: &str) {
        self.free(item);
        self.sync_possession(item, false);
        if self
            .item_in_use
            .as_deref()
            .is_some_and(|i| i.eq_ignore_ascii_case(item))
        {
            self.item_in_use = None;
        }
    }

    pub fn item_in_use(&self) -> Option<&str> {
        self.item_in_use.as_deref()
    }

    /// Puts the held item back in the bag without consuming it.
    pub fn stow(&mut self) {
        if let Some(item) = self.item_in_use.take() {
            self.add_inventory(&item);
        }
    }

    /// Evaluates a room's visibility or hotspot guard against current progress.
    pub fn test(&self, cond: &Cond) -> bool {
        match cond {
            Cond::Always => true,
            Cond::Never => false,
            Cond::Equals { key, value } => self.get(key).loosely_eq(value),
            Cond::Less { key, value } => {
                match (self.get(key).as_int(), value.as_int()) {
                    (Some(a), Some(b)) => a < b,
                    _ => false,
                }
            }
            Cond::Greater { key, value } => {
                match (self.get(key).as_int(), value.as_int()) {
                    (Some(a), Some(b)) => a > b,
                    _ => false,
                }
            }
            Cond::Includes { key, value } => self.list_has(key, value),
            Cond::Lacks { key, value } => !self.list_has(key, value),
            Cond::Not(inner) => !self.test(inner),
            Cond::And(parts) => parts.iter().all(|c| self.test(c)),
            Cond::Or(parts) => parts.iter().any(|c| self.test(c)),
        }
    }

    #[cfg(test)]
    fn list_has_pub(&self, key: &str, value: &Value) -> bool {
        self.list_has(key, value)
    }

    /// `inState` is `getPos(list, item) <> 0`, so membership is tested against
    /// the flag's whole list rather than only its head.
    fn list_has(&self, key: &str, value: &Value) -> bool {
        self.get_all(key).iter().any(|i| i.loosely_eq(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each of these is a bug that shipped. The list operations in particular
    // were written months apart and never checked against one another.

    #[test]
    fn trim_removes_an_item_rather_than_the_flag() {
        // `trimState( #hauntsRemaining, #gazebo2 )` takes an item out of a
        // list. Removing the flag named by the second argument instead left
        // the pool untouched, so every haunt repeated for ever.
        let mut s = State::new();
        s.set_all(
            "hauntsRemaining",
            vec![
                Value::Symbol("gazebo1".into()),
                Value::Symbol("gazebo2".into()),
            ],
        );
        s.trim_item("hauntsRemaining", &Value::Symbol("gazebo2".into()));
        let items = s.get_all("hauntsRemaining");
        assert_eq!(items.len(), 1);
        assert!(items[0].loosely_eq(&Value::Symbol("gazebo1".into())));
    }

    #[test]
    fn add_builds_a_set_rather_than_replacing_it() {
        // The control panel collects pressed buttons. Treating addState as a
        // plain write left only the last button, so the puzzle had no solution.
        let mut s = State::new();
        s.add_item("panelGuess", Value::Symbol("A1".into()));
        s.add_item("panelGuess", Value::Symbol("B2".into()));
        s.add_item("panelGuess", Value::Symbol("A1".into())); // already there
        assert_eq!(s.get_all("panelGuess").len(), 2, "no duplicates, both kept");
    }

    #[test]
    fn trim_and_add_are_inverses() {
        let mut s = State::new();
        s.add_item("k", Value::Symbol("x".into()));
        s.trim_item("k", &Value::Symbol("x".into()));
        assert!(s.get_all("k").is_empty());
    }

    #[test]
    fn a_flag_reads_back_as_the_head_of_its_list() {
        // `getState` is `getAt(list, 1)`. A flag's list is at once its current
        // setting and the settings it may take, which is why `#tunedIn` can be
        // tested for membership in eleven rooms and still index a sprite's art
        // by its head.
        let mut s = State::new();
        s.set_all(
            "tunedIn",
            vec![
                Value::Symbol("bedroom".into()),
                Value::Symbol("kitchen".into()),
                Value::Symbol("inBetween".into()),
            ],
        );
        assert!(s.get("tunedIn").loosely_eq(&Value::Symbol("bedroom".into())));
        assert_eq!(s.get_all("tunedIn").len(), 3);
    }

    #[test]
    fn writing_a_flag_moves_that_setting_to_the_head() {
        // `setState` does `addAt(list, 1, x)` then `deleteAt(list, oldPos + 1)`,
        // so the other settings survive the write and only the order changes.
        let mut s = State::new();
        s.set_all(
            "tunedIn",
            vec![
                Value::Symbol("bedroom".into()),
                Value::Symbol("kitchen".into()),
                Value::Symbol("inBetween".into()),
            ],
        );
        s.set("tunedIn", Value::Symbol("kitchen".into()));
        assert!(s.get("tunedIn").loosely_eq(&Value::Symbol("kitchen".into())));
        assert_eq!(s.get_all("tunedIn").len(), 3, "nothing is lost by a write");
        assert!(s.list_has_pub("tunedIn", &Value::Symbol("bedroom".into())));
    }

    #[test]
    fn a_pool_keeps_its_membership_after_the_head_is_written() {
        // The two operations have to coexist: `#hauntsRemaining` is trimmed as
        // haunts are used up while still answering `#includes`.
        let mut s = State::new();
        s.set_all(
            "hauntsRemaining",
            vec![
                Value::Symbol("lake".into()),
                Value::Symbol("gazebo".into()),
            ],
        );
        s.trim_item("hauntsRemaining", &Value::Symbol("lake".into()));
        assert!(!s.list_has_pub("hauntsRemaining", &Value::Symbol("lake".into())));
        assert!(s.list_has_pub("hauntsRemaining", &Value::Symbol("gazebo".into())));
    }

    #[test]
    fn taking_an_item_sets_its_possession_flag() {
        // Rooms hide a taken object with a plate gated on playerHas<Item>.
        // The flag is written when the item is taken; the schema seeds all of
        // them to zero, so it cannot be derived when read.
        let mut s = State::new();
        // The schema's own settings for one of these flags.
        s.set_all(
            "playerHasCrowbar",
            vec![
                Value::Int(0),
                Value::Symbol("carrying".into()),
                Value::Symbol("inUse".into()),
                Value::Symbol("usedUp".into()),
            ],
        );
        assert!(!s.carrying("Crowbar"));

        s.add_inventory("Crowbar");
        // `#carrying`, as `addInventory` writes it -- not 1.
        assert!(s.get("playerHasCrowbar").is_symbol("carrying"));
        assert!(s.carrying("Crowbar"));
        // And the settings it may still take are still behind it, because a
        // flag left with one value is a flag this engine treats differently.
        assert_eq!(s.get_all("playerHasCrowbar").len(), 4);

        // Spending it writes `#usedUp`, which is what `deleteInventory` does
        // for everything but the scan device and the headgear. It still reads
        // as "not zero", because that is the test the scripts make, and the
        // handlers that need the difference ask for `#usedUp` by name -- the
        // 1940s bedroom door is one of them.
        s.delete_inventory("crowbar"); // case-insensitive
        assert!(s.get("playerHasCrowbar").is_symbol("usedUp"));
        assert!(s.carrying("Crowbar"));

        // The scan device goes back in its box rather than being spent, and
        // the headgear is worn.
        s.set_all("playerHasScanDevice", vec![Value::Int(0), Value::Symbol("carrying".into())]);
        s.add_inventory("ScanDevice");
        s.delete_inventory("ScanDevice");
        assert_eq!(s.get("playerHasScanDevice").as_int(), Some(0));

        s.set_all(
            "playerHasHeadgear",
            vec![Value::Int(0), Value::Symbol("carrying".into()), Value::Symbol("inUse".into())],
        );
        s.add_inventory("Headgear");
        s.delete_inventory("Headgear");
        assert!(s.get("playerHasHeadgear").is_symbol("inUse"));
    }

    #[test]
    fn an_item_used_up_still_counts_as_had() {
        // The scripts ask `getState( #playerHas<Item> ) = 0`, so `#usedUp` is
        // not zero and answers yes. The handlers that need the difference
        // test for `#usedUp` by name.
        let mut s = State::new();
        s.set_all("playerHasWeedkiller", vec![Value::Int(0), Value::Symbol("usedUp".into())]);
        assert!(!s.carrying("Weedkiller"));
        s.set("playerHasWeedkiller", Value::Symbol("usedUp".into()));
        assert!(s.carrying("Weedkiller"));
    }

    #[test]
    fn item_in_use_reads_back_as_none_when_empty() {
        let mut s = State::new();
        assert!(s.get("itemInUse").loosely_eq(&Value::Symbol("None".into())));
        s.add_inventory("ScanDevice");
        s.set("itemInUse", Value::Symbol("ScanDevice".into()));
        assert_eq!(s.item_in_use(), Some("ScanDevice"));
        s.stow();
        assert!(s.get("itemInUse").loosely_eq(&Value::Symbol("None".into())));
    }
}
