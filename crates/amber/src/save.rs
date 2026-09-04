//! Saving and loading, in the game's own notation.
//!
//! Amber has no save of its own -- the original hung one off Director's menu
//! bar, which this engine does not have -- so this is an addition rather than a
//! port, and it is written down as one.
//!
//! The format is a Lingo property list, which is not decoration. The game
//! already keeps every scrap of progress in exactly this shape, in a cast
//! member called `stateData`, and the engine already has a parser for it. So a
//! save is readable, diffable, and close enough to the original's own state
//! that it can be compared against one by eye:
//!
//! ```text
//! [#version: 1, #domain: "ROXY", #room: "HallLivingRmEntry",
//!  #inhand: VOID,
//!  #slots: [#None, #ScanDevice, #None, #PeekUnit, #None, #None, #None],
//!  #states: [#ambervision: [#on, #off], #moveCount: [63], ...]]
//! ```
//!
//! What is deliberately *not* saved: anything a room rebuilds on arrival. The
//! film that is playing, the effect queue, the puppet channels and the cue list
//! are all consequences of being somewhere, and loading walks into the room
//! afresh rather than trying to restore a moment mid-cutscene.

use lingo::Value;

use crate::game::Game;

/// Bumped when the shape changes in a way an older file cannot satisfy.
pub const VERSION: i32 = 1;

/// The seven slots the inventory bar draws.
const SLOTS: usize = 7;

fn prop<'a>(props: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    props
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Everything needed to put the player back where they were.
pub fn write(game: &Game) -> String {
    let node = &game.world.nodes[game.room];

    // The room is saved by *name*, not by index. Indices are positions in a
    // table built from the disc, and a save that outlives a change to how that
    // table is built would otherwise quietly land the player somewhere else.
    let mut top = vec![
        ("version".to_string(), Value::Int(VERSION)),
        ("domain".to_string(), Value::String(node.domain.clone())),
        (
            "room".to_string(),
            Value::String(node.name.clone().unwrap_or_default()),
        ),
        (
            "inhand".to_string(),
            match game.state.item_in_use() {
                Some(item) => Value::Symbol(item.to_string()),
                None => Value::Void,
            },
        ),
    ];

    // A fixed seven, `#None` where empty, because a slot is a position on the
    // bar and not a place in a queue: an item keeps its slot, which is what
    // makes a recorded click on the bar mean the same thing twice.
    let mut slots = vec![Value::Symbol("None".to_string()); SLOTS];
    for (n, item) in game.state.slots() {
        if let Some(cell) = slots.get_mut(n - 1) {
            *cell = Value::Symbol(item.to_string());
        }
    }
    top.push(("slots".to_string(), Value::List(slots)));

    // Every flag, whole. Not just its head: a flag's list is at once its
    // current value and the settings it may still take, and the pools --
    // `#hauntsRemaining`, `#utterancesRemaining` -- are the tail alone.
    let states = game
        .state
        .entries()
        .map(|(key, values)| (key.clone(), Value::List(values.to_vec())))
        .collect();
    top.push(("states".to_string(), Value::Props(states)));

    Value::Props(top).to_string()
}

/// Puts a saved game back, or says why it could not.
pub fn read(game: &mut Game, text: &str) -> Result<(), String> {
    let parsed = lingo::parse_value(text).map_err(|e| format!("not a save file: {e:?}"))?;
    let Value::Props(top) = parsed else {
        return Err("not a save file: expected a property list".into());
    };

    match prop(&top, "version") {
        Some(Value::Int(v)) if *v == VERSION => {}
        Some(Value::Int(v)) => return Err(format!("save is version {v}, this engine reads {VERSION}")),
        _ => return Err("save has no version".into()),
    }

    let domain = match prop(&top, "domain") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err("save names no chapter".into()),
    };
    let room = match prop(&top, "room") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err("save names no room".into()),
    };
    let Some(index) = game.world.resolve(&room, Some(&domain)) else {
        return Err(format!("{domain} has no room called {room}"));
    };

    let Some(Value::Props(states)) = prop(&top, "states") else {
        return Err("save has no state".into());
    };
    let props: Vec<(String, Vec<Value>)> = states
        .iter()
        .map(|(key, value)| {
            let values = match value {
                Value::List(items) => items.clone(),
                // A flag written as a bare value rather than a list is not
                // something this writes, but a hand-edited save may well have
                // one and meaning it as a single setting is the only reading.
                other => vec![other.clone()],
            };
            (key.clone(), values)
        })
        .collect();

    let mut slots: [Option<String>; SLOTS] = Default::default();
    if let Some(Value::List(items)) = prop(&top, "slots") {
        for (i, item) in items.iter().take(SLOTS).enumerate() {
            if let Value::Symbol(name) = item {
                if !name.eq_ignore_ascii_case("None") {
                    slots[i] = Some(name.clone());
                }
            }
        }
    }
    let in_hand = match prop(&top, "inhand") {
        Some(Value::Symbol(s)) if !s.eq_ignore_ascii_case("None") => Some(s.clone()),
        _ => None,
    };

    // Nothing is applied until everything has been read, so a save that turns
    // out to be malformed half way through leaves the game running rather than
    // half-loaded -- which would be a worse failure than refusing, because it
    // looks like it worked.
    game.state.restore(props, slots, in_hand);
    game.pending.clear();
    game.room = index;
    game.start_room_video();
    Ok(())
}

/// Where a numbered slot lives.
///
/// Slots sit beside the base path rather than inside a directory of their own:
/// one file per slot, named for it, so a save can be copied about or read with
/// a text editor -- which is most of the point of the format being Lingo.
pub fn slot_path(base: &std::path::Path, slot: usize) -> std::path::PathBuf {
    let mut name = base.file_stem().unwrap_or_default().to_os_string();
    name.push(format!("{slot}."));
    name.push(base.extension().unwrap_or_default());
    base.with_file_name(name)
}

/// A one-line label for a slot, read out of the file itself.
///
/// Slots that say only "SLOT 2" are slots nobody can tell apart, so this reads
/// the chapter and room back and shows those. Cheap: the file is small and it
/// is only read when the menu is opened.
pub fn describe(text: &str) -> Option<String> {
    let Value::Props(top) = lingo::parse_value(text).ok()? else { return None };
    let domain = match prop(&top, "domain")? {
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let room = match prop(&top, "room")? {
        Value::String(s) => s.clone(),
        _ => return None,
    };
    // Upper case because the menu font has no lower case, and trimmed because
    // some room names are long enough to run off the panel.
    let label: String = format!("{domain} {room}")
        .to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    Some(label.chars().take(22).collect())
}

/// What each slot holds, for the menu to label them with.
pub fn slots(base: &std::path::Path) -> [Option<String>; 3] {
    let mut out: [Option<String>; 3] = Default::default();
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = std::fs::read_to_string(slot_path(base, i + 1))
            .ok()
            .and_then(|text| describe(&text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Option<Game> {
        // Anchored to the crate rather than the working directory: tests run
        // from `crates/amber`, so a bare "extract" resolves to nothing and the
        // test would take its skip path every single run -- which is exactly
        // how a test that could never fail got shipped twice before.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        root.is_dir().then(|| Game::new(&root).expect("extract/ is not a game"))
    }

    /// A save puts back what was there, and nothing else.
    ///
    /// The load is into a *separate* game that has been played differently, so
    /// this fails if `restore` merges rather than replaces -- which is the
    /// mistake a `set_all` in a loop would make, and it would look fine on a
    /// save taken and loaded in the same session.
    #[test]
    fn a_save_restores_progress_and_drops_what_came_before() {
        let Some(mut played) = game() else { return };

        // Somewhere with a pool part-consumed and something in the bag.
        played.enter_chapter("ROXY");
        played.state.set_all("hauntsremaining", vec![Value::Symbol("stairsGhost".into())]);
        played.state.add_inventory("PeekUnit");
        played.state.add_inventory("ScanDevice");
        played.state.set("ambervision", Value::Symbol("on".into()));
        let room = played.room;
        let slots: Vec<(usize, String)> =
            played.state.slots().map(|(n, i)| (n, i.to_string())).collect();

        let text = write(&played);

        // A different game, with a flag the save has never heard of.
        let Some(mut fresh) = game() else { return };
        fresh.enter_chapter("ROXY");
        fresh.state.set("aflagthesavenevermentions", Value::Int(7));
        read(&mut fresh, &text).expect("the save did not load");

        assert_eq!(fresh.room, room, "landed in the wrong room");
        assert_eq!(
            fresh.state.slots().map(|(n, i)| (n, i.to_string())).collect::<Vec<_>>(),
            slots,
            "the bar came back in different slots"
        );
        assert_eq!(
            fresh.state.get_all("hauntsremaining"),
            [Value::Symbol("stairsGhost".into())],
            "the haunt pool was not restored whole"
        );
        assert!(
            fresh.state.get_all("aflagthesavenevermentions").is_empty(),
            "a flag the save never mentioned survived the load"
        );
    }

    /// A pool is saved whole, not just its head.
    ///
    /// `getState` reads element zero, so a save that kept only the head would
    /// look right everywhere the head is what is read -- and would quietly
    /// empty every pool in the game, which is what `#hauntsRemaining` and
    /// `#utterancesRemaining` are.
    #[test]
    fn the_tail_of_a_flag_survives() {
        let Some(mut g) = game() else { return };
        g.enter_chapter("ROXY");
        let pool = vec![
            Value::Symbol("one".into()),
            Value::Symbol("two".into()),
            Value::Symbol("three".into()),
        ];
        g.state.set_all("apool", pool.clone());
        let text = write(&g);

        let Some(mut back) = game() else { return };
        read(&mut back, &text).expect("the save did not load");
        assert_eq!(back.state.get_all("apool"), pool.as_slice());
    }

    #[test]
    fn a_save_from_another_version_is_refused() {
        let Some(mut g) = game() else { return };
        let text = write(&g).replace("#version: 1", "#version: 99");
        let err = read(&mut g, &text).expect_err("a future save should not load");
        assert!(err.contains("version"), "unhelpful refusal: {err}");
    }
}
