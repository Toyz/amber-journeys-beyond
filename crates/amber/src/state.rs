//! Game state: the flag store the room conditions read, plus inventory.

use std::collections::BTreeMap;

use lingo::Value;

use crate::world::Cond;

/// The mutable half of a save file.
///
/// Amber keeps its progress in one flat property list on a Lingo object the
/// scripts call `oStoryteller`, addressed by symbol. Conditions read the same
/// store that actions write, so a single map is enough to model it faithfully.
#[derive(Clone, Default, Debug)]
pub struct State {
    /// Flags, lower-cased keys to match Lingo's case-insensitive symbols.
    props: BTreeMap<String, Value>,
    /// Everything the player is carrying.
    inventory: Vec<String>,
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
            _ => self
                .props
                .get(&key)
                .cloned()
                .unwrap_or(Value::Void),
        }
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
        let key = format!("playerhas{}", item.to_ascii_lowercase());
        self.props.insert(key, Value::Int(held as i32));
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
        self.props.insert(key, value);
    }

    /// Drops a flag entirely; a missing flag reads back as `Void` and so fails
    /// an equality test rather than matching zero.
    pub fn trim(&mut self, key: &str) {
        self.props.remove(&key.to_ascii_lowercase());
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
        if let Some(Value::List(items)) = self.props.get_mut(&key) {
            items.retain(|i| !i.loosely_eq(item));
        }
    }

    /// Every flag currently set, for inspection from the walkthrough.
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.props.iter()
    }

    pub fn inventory(&self) -> &[String] {
        &self.inventory
    }

    pub fn add_inventory(&mut self, item: &str) {
        if !self.inventory.iter().any(|i| i.eq_ignore_ascii_case(item)) {
            self.inventory.push(item.to_owned());
        }
        self.sync_possession(item, true);
    }

    pub fn delete_inventory(&mut self, item: &str) {
        self.inventory.retain(|i| !i.eq_ignore_ascii_case(item));
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
            Cond::Equals { key, value } => self.get(key).loosely_eq(value),
            Cond::Less { key, value } => {
                match (self.get(key).as_int(), value.as_int()) {
                    (Some(a), Some(b)) => a < b,
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

    fn list_has(&self, key: &str, value: &Value) -> bool {
        match self.get(key) {
            Value::List(items) => items.iter().any(|i| i.loosely_eq(value)),
            other => other.loosely_eq(value),
        }
    }
}
