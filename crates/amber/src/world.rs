//! The world model: rooms, sprites, and hotspots, lifted out of the `.DAT` files.

use std::collections::HashMap;
use std::path::Path;

use lingo::{parse_dat, Rect, Value};

use crate::locations::LocationTable;

/// What a hotspot does when clicked. The verb picks the cursor the game shows and
/// which of the twelve interaction affordances the player is being offered.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Verb {
    /// The catch-all region; usually a commented-out action, so it mostly serves
    /// to swallow clicks that hit nothing more specific.
    Browse,
    Left,
    Right,
    Forward,
    Up,
    Down,
    Examine,
    /// A generic pointer target, used for machinery and puzzle widgets.
    Pointer,
    /// Fires only while the player is carrying something, to use it on the scene.
    ItemInUse,
    NextPage,
    RotateLeft,
    RotateRight,
}

impl Verb {
    fn parse(s: &str) -> Option<Verb> {
        Some(match s.to_ascii_lowercase().as_str() {
            "browse" => Verb::Browse,
            "left" => Verb::Left,
            "right" => Verb::Right,
            "forward" => Verb::Forward,
            "up" => Verb::Up,
            "down" => Verb::Down,
            "examine" => Verb::Examine,
            "pointer" => Verb::Pointer,
            "iteminuse" => Verb::ItemInUse,
            "nextpage" => Verb::NextPage,
            "rotateleft" => Verb::RotateLeft,
            "rotateright" => Verb::RotateRight,
            _ => return None,
        })
    }

}

/// Which Director channel a stage element occupies. Sprite channels stack back to
/// front; the two named channels are the audio and QuickTime layers.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Channel {
    Sprite(u8),
    Sound,
    Video,
    None,
}

impl Channel {
    fn parse(v: &Value) -> Channel {
        match v {
            Value::Int(n) => Channel::Sprite(*n as u8),
            Value::Symbol(s) => match s.to_ascii_lowercase().as_str() {
                "sound" => Channel::Sound,
                "video" => Channel::Video,
                _ => Channel::None,
            },
            _ => Channel::None,
        }
    }
}

/// A condition guarding whether a sprite draws or a hotspot is live.
///
/// The data expresses these as nested one-key property lists, e.g.
/// `[#equals: [#always, 1]]` or `[#not: [#itemInUse, #None]]`. The operand pair
/// is always `[state-key, expected-value]`.
#[derive(Clone, Debug)]
pub enum Cond {
    Always,
    /// Never true. `#always` is 1, so `[#equals: [#always, 0]]` is how the
    /// authors switched a sprite off without deleting it -- four of them are
    /// left in the shipped data, including the panel graphic that otherwise
    /// covers the psionic bar's readouts, and the sprite for `MEewall.mov`.
    Never,
    /// State `key` equals `value`.
    Equals { key: String, value: Value },
    /// State `key` is numerically below `value`.
    Less { key: String, value: Value },
    /// State `key` is numerically above `value`.
    Greater { key: String, value: Value },
    /// State `key` is a list containing `value`.
    Includes { key: String, value: Value },
    /// State `key` is a list not containing `value`.
    Lacks { key: String, value: Value },
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

impl Cond {
    /// Builds a condition from the `[#op: [key, value]]` shape.
    ///
    /// A compound guard nests as `[#and: [#equals: [a, b], #equals: [c, d]]]`,
    /// where the operand is itself a property list carrying one entry per
    /// clause, and the same operator may appear more than once. Reading that
    /// operand as a linear list finds nothing and yields an empty `And`, which
    /// is vacuously true, so every compound guard in the game passed. Both
    /// clauses have to be read out of the entries.
    ///
    /// Unrecognised operators degrade to `Always` so a stray key cannot make a
    /// room unreachable.
    pub fn parse(v: &Value) -> Cond {
        let entries = v.entries();
        let Some((op, operand)) = entries.first() else {
            return Cond::Always;
        };

        // The operand of a comparison is a `[state key, expected value]` pair.
        let pair = |val: &Value| -> (String, Value) {
            match val.as_list() {
                Some([k, v]) => (
                    k.as_str().unwrap_or_default().to_ascii_lowercase(),
                    v.clone(),
                ),
                _ => (String::new(), Value::Void),
            }
        };

        let leaf = |op: &str, operand: &Value| -> Cond {
            let (key, value) = pair(operand);
            match op {
                // `#always` holds 1, so the value decides: comparing it to
                // 1 is the ordinary unconditional guard and comparing it to 0
                // is a sprite the authors turned off. Reading both as `Always`
                // drew four sprites that are meant never to appear.
                "equals" if key == "always" => {
                    if value.truthy() {
                        Cond::Always
                    } else {
                        Cond::Never
                    }
                }
                "equals" => Cond::Equals { key, value },
                "less" => Cond::Less { key, value },
                "greater" => Cond::Greater { key, value },
                "includes" => Cond::Includes { key, value },
                "lacks" => Cond::Lacks { key, value },
                // `#not` wraps a bare operand pair rather than a nested
                // condition, so rebuild it as a negated equality.
                "not" => Cond::Not(Box::new(Cond::Equals { key, value })),
                _ => Cond::Always,
            }
        };

        match op.as_str() {
            "and" | "or" => {
                let parts: Vec<Cond> = operand
                    .entries()
                    .iter()
                    .map(|(inner_op, inner)| match inner_op.as_str() {
                        "and" | "or" => Cond::parse(&Value::Props(vec![(
                            inner_op.clone(),
                            inner.clone(),
                        )])),
                        other => leaf(other, inner),
                    })
                    .collect();
                // An operator with no readable clauses would otherwise be
                // vacuously true, which is the failure this replaced.
                if parts.is_empty() {
                    return Cond::Always;
                }
                if op == "and" {
                    Cond::And(parts)
                } else {
                    Cond::Or(parts)
                }
            }
            other => leaf(other, operand),
        }
    }
}

/// One clickable region of a room.
#[derive(Clone, Debug)]
pub struct Hotspot {
    pub verb: Verb,
    pub bounds: Rect,
    /// Lingo source lines to run on click, in order. Lines opening with `--` are
    /// comments the authors left in place and are skipped at run time.
    pub actions: Vec<String>,
    pub condition: Cond,
}

/// A visual or audio element placed on the stage for a room.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub cast_name: Option<String>,
    pub cast_number: u32,
    /// `(state flag, lookup table)` when the sprite picks its art by state
    /// rather than naming a fixed cast number. See `casttable`.
    pub cast_lookup: Option<(String, String)>,
    pub channel: Channel,
    pub condition: Cond,
    /// Centre point on the 640x480 stage. Director positions by registration
    /// point, so this is the sprite's centre, not its top-left corner.
    pub center: Option<(i32, i32)>,
    pub ink: i32,
    /// Playback volume for sound and video elements, 0-255.
    pub volume: Option<i32>,
}

/// One navigable room.
#[derive(Clone, Debug, Default)]
pub struct Node {
    /// Index within its `.DAT` file, which is how the game addresses it.
    pub index: usize,
    /// The room's name from its chapter's location table, when it has one.
    pub name: Option<String>,
    /// The area of the house this room is in, from the same table. Handlers
    /// compare against areas rather than rooms when what matters is roughly
    /// where the player is standing.
    pub zone: Option<String>,
    /// Which character's chapter this room belongs to.
    pub domain: String,
    pub preload: Vec<u32>,
    pub sprites: Vec<Sprite>,
    pub hotspots: Vec<Hotspot>,
    /// `[cast library, first member, last member]` for the room's art.
    pub storage_cast: Option<(u32, u32, u32)>,
    /// Ambient mix levels keyed by source, e.g. `househum`, `phonevol`.
    pub ambience: HashMap<String, i32>,
}

impl Node {
    fn from_value(index: usize, domain: &str, v: &Value) -> Node {
        let preload = v
            .get_list("preLoad")
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_int)
            .map(|i| i as u32)
            .collect();

        let sprites = v
            .get_list("onStage")
            .unwrap_or_default()
            .iter()
            .map(|s| Sprite {
                cast_name: s
                    .get_str("castName")
                    .filter(|n| !n.eq_ignore_ascii_case("no assigned cast"))
                    .map(str::to_owned),
                cast_number: s.get_int("castNum").unwrap_or(0).max(0) as u32,
                // `#castNum: [#lock_A, #lock_A_digits]` names a state flag and
                // the table to index with it. Read as an integer this yields
                // nothing, so the sprite silently never drew.
                cast_lookup: match s.get("castNum").and_then(Value::as_list) {
                    Some([flag, table]) => match (flag.as_str(), table.as_str()) {
                        (Some(f), Some(t)) => Some((
                            f.trim_start_matches('#').to_ascii_lowercase(),
                            t.trim_start_matches('#').to_ascii_lowercase(),
                        )),
                        _ => None,
                    },
                    _ => None,
                },
                channel: s.get("channel").map(Channel::parse).unwrap_or(Channel::None),
                condition: s.get("showIF").map(Cond::parse).unwrap_or(Cond::Always),
                center: s.get("coords").and_then(Value::as_point),
                ink: s.get_int("ink").unwrap_or(0),
                volume: s.get_int("earShot"),
            })
            .collect();

        // A hotspot is a positional list, not a property list:
        // [verb, rect, [actions], condition].
        let hotspots = v
            .get_list("Hotspots")
            .unwrap_or_default()
            .iter()
            .filter_map(|h| {
                let f = h.as_list()?;
                let verb = Verb::parse(f.first()?.as_str()?)?;
                let bounds = f.get(1)?.as_rect()?;
                let actions = f
                    .get(2)
                    .and_then(Value::as_list)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|a| a.as_str())
                    .map(str::trim)
                    .filter(|a| !a.is_empty() && !a.starts_with("--"))
                    .map(str::to_owned)
                    .collect();
                let condition = f.get(3).map(Cond::parse).unwrap_or(Cond::Always);
                Some(Hotspot {
                    verb,
                    bounds,
                    actions,
                    condition,
                })
            })
            .collect();

        let storage_cast = v.get_list("storageCast").and_then(|s| match s {
            [a, b, c] => Some((
                a.as_int()? as u32,
                b.as_int()? as u32,
                c.as_int()? as u32,
            )),
            _ => None,
        });

        let mut ambience = HashMap::new();
        if let Some(Value::Props(m)) = v.get("earShot") {
            for (k, val) in m {
                if let Some(i) = val.as_int() {
                    ambience.insert(k.clone(), i);
                }
            }
        }

        Node {
            index,
            name: None,
            zone: None,
            domain: domain.to_owned(),
            preload,
            sprites,
            hotspots,
            storage_cast,
            ambience,
        }
    }

    /// Picks the hotspot a click at `(x, y)` should trigger.
    ///
    /// Regions overlap by design. Verb priority separates the affordances -
    /// a small `#examine` target has to beat the room-sized `#browse`
    /// rectangle underneath it - but two hotspots of the same verb are
    /// separated by the order the room lists them in, first match winning, as
    /// Director does.
    ///
    /// Order matters and area does not. The front porch offers two forward
    /// exits whose guards can both hold: the first leads into the darkened
    /// house, the second into the lit one, and the lit rectangle is the
    /// smaller of the two. Breaking the tie by size therefore sent the player
    /// into a lit house they had not turned the lights on in.
    /// `holding` says whether the player has an item in hand, which changes
    /// the order: an `itemInUse` region then outranks everything.
    ///
    /// Without that, most of them are unreachable. Four hundred and sixty-six
    /// of the game's eight hundred sit inside a navigation region -- the
    /// scanner's door knob is inside the rectangle that walks through the
    /// door -- so ranking them below a `#pointer` means the click walks the
    /// player away instead of using what they are carrying. Which is exactly
    /// what a cursor holding an object should not do.
    ///
    /// The condition is needed as well as the ordering: six hundred and
    /// eighty-nine of them are guarded on what is in hand and gate themselves,
    /// but eighty-one are guarded only on `#always` and would otherwise fire
    /// with empty hands.
    pub fn hit_test(
        &self,
        x: i32,
        y: i32,
        holding: bool,
        live: impl Fn(&Cond) -> bool,
    ) -> Option<&Hotspot> {
        self.hit_index(x, y, holding, live).map(|i| &self.hotspots[i])
    }

    /// The same test, answering with the hotspot's place in the room's list.
    ///
    /// The walkthrough needs to ask "does clicking here reach *this* row",
    /// which needs identity rather than the hotspot itself. Ranking this a
    /// second time somewhere else got the browse exception backwards and sent
    /// a route into a loop, so there is one copy of it and both callers use
    /// it.
    pub fn hit_index(
        &self,
        x: i32,
        y: i32,
        holding: bool,
        live: impl Fn(&Cond) -> bool,
    ) -> Option<usize> {
        self.hotspots
            .iter()
            .enumerate()
            .filter(|(_, h)| h.bounds.contains(x, y))
            .filter(|(_, h)| !h.actions.is_empty())
            // An `#itemInUse` region is there to catch something being used on
            // the scene, and does not exist with empty hands. Nearly every room
            // has one covering the whole stage, so leaving it in the running
            // put it above the browse region underneath -- which meant most of
            // most rooms showed the wrong cursor, and clicking the scenery
            // stowed nothing instead of walking.
            .filter(|(_, h)| holding || h.verb != Verb::ItemInUse)
            .filter(|(_, h)| live(&h.condition))
            // First in the room's list wins, with `#browse` alone ranked
            // below everything.
            //
            // This is what Director does -- it walks the regions in order and
            // takes the first one under the pointer -- and the data is written
            // for it: `#itemInUse` is listed first in 702 of the 1320 rooms
            // and averages third of a percent into the list, `#nextPage` and
            // the dials sit at six, and `#browse` is last in 1284 of them.
            //
            // `#browse` needs the exception because it is the catch-all and
            // blankets the frame. In the 36 rooms where it is not last there
            // are five places it would swallow a real affordance underneath
            // it, and the game clearly does not mean it to.
            //
            // A table of priorities by verb stood in for this and got the
            // books wrong. The BAR manual lists its two page-turn regions
            // first and a stage-sized `#pointer` after them to close the book;
            // ranking `#pointer` above `#nextPage` meant every attempt to turn
            // a page shut the manual instead. The manual carries two of the
            // three settings for the machine in the living room, so the game
            // was unfinishable by the book.
            .max_by_key(|(index, h)| {
                (h.verb != Verb::Browse, std::cmp::Reverse(*index))
            })
            .map(|(index, _)| index)
    }
}

/// Every room in the game, grouped by the chapter it belongs to.
#[derive(Default)]
pub struct World {
    pub nodes: Vec<Node>,
    /// Chapter name -> the range of `nodes` it owns.
    pub domains: HashMap<String, (usize, usize)>,
    /// Room name -> every room carrying that name, built by joining each
    /// chapter's name table to its room records on the cast number.
    ///
    /// Names are not globally unique: all four chapters declare a
    /// `DEFAULT_LOCATION`, and several share room names outright, so a single
    /// index would silently resolve to whichever chapter loaded first.
    pub by_name: HashMap<String, Vec<usize>>,
}

/// Finds a file in `dir` whose name matches `name` ignoring case.
///
/// The PC pressing shouts its filenames -- `MARGARET.DXR`, `MOVIES_M` -- and
/// the Macintosh pressing does not: `MARGARET.dxr`, `movies_M`. HFS is
/// case-insensitive so both were the same name to the original; on a
/// case-sensitive filesystem they are not, and looking for the shouted form
/// found nothing at all on the Macintosh disc.
pub fn find_ci(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).find_map(|e| {
        e.file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            .then(|| e.path())
    })
}

impl World {
    /// Resolves a `goTo` destination to a room.
    ///
    /// Rooms are addressed within a chapter, so a match inside `from` always
    /// wins; the search widens to the other chapters only when the current one
    /// has no such room, which is how the few cross-chapter moves work.
    pub fn resolve(&self, name: &str, from: Option<&str>) -> Option<usize> {
        let candidates = self.by_name.get(&name.to_ascii_lowercase())?;
        if let Some(&(start, end)) = from.and_then(|d| self.domains.get(d)) {
            if let Some(&i) = candidates.iter().find(|&&i| i >= start && i < end) {
                return Some(i);
            }
        }
        candidates.first().copied()
    }

}

impl World {
    /// Loads every `<DOMAIN>_<n>.DAT` under `root`, in numeric order.
    ///
    /// Amber splits its rooms across four chapters, one per haunting, each in its
    /// own directory with its own Director movie.
    pub fn load(
        content: &dyn crate::content::Content,
        catalogue: &crate::content::Catalogue,
    ) -> std::io::Result<World> {
        const DOMAINS: [(&str, &str); 4] = [
            ("ROXY", "ROXY"),
            ("MARGARET", "MARG"),
            ("EDWIN", "EDWIN"),
            ("BRICE", "BRICE"),
        ];

        let mut nodes = Vec::new();
        let mut domains = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();

        for (dir, prefix) in DOMAINS {
            let mut files: Vec<(u32, String)> = catalogue
                .dir(dir)
                .into_iter()
                .filter_map(|path| {
                    let name = path.rsplit('/').next()?.to_ascii_uppercase();
                    let stem = name.strip_suffix(".DAT")?;
                    let n = stem.strip_prefix(&format!("{prefix}_"))?;
                    Some((n.parse().ok()?, path.to_string()))
                })
                .collect();
            if files.is_empty() && catalogue.in_dir(dir, &format!("{dir}.DXR")).is_none() {
                continue;
            }
            files.sort_by_key(|(n, _)| *n);

            let start = nodes.len();

            // Rooms are stored twice on the disc: once as text cast members
            // inside the chapter movie, and again in the external `.DAT` files
            // the projector streams at run time. The two describe the same
            // rooms, so both are read and then deduplicated on the
            // `#storageCast` triple that identifies a room. The embedded copy
            // is preferred because the cast member's name is the room's name;
            // the `.DAT` copy carries no name of its own.
            // Keyed by cast number, which is element 0 of the `#storageCast`
            // triple. The other two elements are byte offsets into the chapter's
            // concatenated room text and so differ between the two copies; the
            // cast number is the same in all three places a room is referenced.
            let mut seen: HashMap<u32, usize> = HashMap::new();

            let movie = catalogue
                .in_dir(dir, &format!("{dir}.DXR"))
                .and_then(|p| content.read(p))
                .and_then(|b| director::Movie::from_bytes(b).ok());
            if let Some(movie) = &movie {
                for (number, member_name) in movie.members_named_with(".DATA") {
                    let Some(text) = movie.text(number) else { continue };
                    let Ok(value) = lingo::parse_value(text.trim()) else { continue };
                    // Some `.DATA` members hold configuration rather than a
                    // room; a room always declares hotspots or stage elements.
                    if value.get("Hotspots").is_none() && value.get("onStage").is_none() {
                        continue;
                    }
                    let room = member_name
                        .rsplit_once('.')
                        .map(|(base, _)| base)
                        .unwrap_or(&member_name)
                        .to_string();
                    let index = nodes.len();
                    let mut node = Node::from_value(index - start, dir, &value);
                    node.name = Some(room.clone());
                    // An embedded room does not repeat its own address, because
                    // it is the cast member that address points at.
                    node.storage_cast.get_or_insert((number, 0, text.len() as u32));
                    seen.insert(number, index);
                    by_name
                        .entry(room.to_ascii_lowercase())
                        .or_default()
                        .push(index);
                    nodes.push(node);
                }
            }

            for (_, file) in files {
                let Some(bytes) = content.read(&file) else { continue };
                let records = match parse_dat(&bytes) {
                    Ok(r) => r,
                    // One unparsable file should not sink the other chapters.
                    Err(e) => {
                        eprintln!("warning: {file}: {e}");
                        continue;
                    }
                };
                for rec in &records {
                    let index = nodes.len();
                    let node = Node::from_value(index - start, dir, rec);
                    // Skip a room the embedded copy already supplied.
                    if let Some((cast, _, _)) = node.storage_cast {
                        if seen.contains_key(&cast) {
                            continue;
                        }
                        seen.insert(cast, index);
                    }
                    nodes.push(node);
                }
            }

            // Join the chapter's name table to any rooms that still lack a
            // name, which are the ones only the `.DAT` files supplied.
            if let Some(movie) = &movie {
                let table = LocationTable::from_texts(&movie.texts());
                for name in table.all_names() {
                    if let Some((cast, _, _)) = table.triple(name) {
                        if let Some(&i) = seen.get(&cast) {
                            let slot = by_name.entry(name.to_ascii_lowercase()).or_default();
                            if !slot.contains(&i) {
                                slot.push(i);
                            }
                            if nodes[i].name.is_none() {
                                nodes[i].name = Some(name.to_string());
                            }
                            if nodes[i].zone.is_none() {
                                nodes[i].zone = table.zone(name).map(str::to_string);
                            }
                        }
                    }
                }
            }

            domains.insert(dir.to_string(), (start, nodes.len()));
        }

        Ok(World {
            nodes,
            domains,
            by_name,
        })
    }

    /// How many rooms the world holds.
    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether it holds none, which means the content source was wrong.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use lingo::parse_value;

    fn cond(src: &str) -> Cond {
        Cond::parse(&parse_value(src).expect("guard parses"))
    }

    fn spot(verb: Verb, bounds: Rect) -> Hotspot {
        Hotspot {
            verb,
            bounds,
            actions: vec!["goTo #somewhere".into()],
            condition: Cond::Always,
        }
    }

    fn room(hotspots: Vec<Hotspot>) -> Node {
        Node {
            index: 0,
            name: None,
            zone: None,
            domain: "test".into(),
            preload: Vec::new(),
            sprites: Vec::new(),
            hotspots,
            storage_cast: None,
            ambience: HashMap::new(),
        }
    }

    // --- guards ---------------------------------------------------------
    // A compound guard nests two clauses under repeated keys. Reading the
    // operand as a flat list found neither, leaving an empty `And` that was
    // vacuously true: every locked door in the game opened.

    #[test]
    fn compound_and_reads_both_clauses() {
        let c = cond("[#and: [#equals: [#hasKey, 1], #equals: [#doorUnlocked, 1]]]");
        let Cond::And(parts) = &c else {
            panic!("expected And, got {c:?}");
        };
        assert_eq!(parts.len(), 2, "an empty And is vacuously true");

        let mut s = State::new();
        s.set("hasKey", Value::Int(1));
        assert!(!s.test(&c), "one clause satisfied is not enough");
        s.set("doorUnlocked", Value::Int(1));
        assert!(s.test(&c));
    }

    #[test]
    fn compound_or_reads_both_clauses() {
        let c = cond("[#or: [#equals: [#a, 1], #equals: [#b, 1]]]");
        let Cond::Or(parts) = &c else { panic!("expected Or") };
        assert_eq!(parts.len(), 2);

        let mut s = State::new();
        assert!(!s.test(&c), "an empty Or must not be vacuously false either");
        s.set("b", Value::Int(1));
        assert!(s.test(&c));
    }

    #[test]
    fn nested_compounds_recurse() {
        let c = cond("[#and: [#equals: [#a, 1], #or: [#equals: [#b, 1], #equals: [#c, 1]]]]");
        let mut s = State::new();
        s.set("a", Value::Int(1));
        assert!(!s.test(&c));
        s.set("c", Value::Int(1));
        assert!(s.test(&c));
    }

    #[test]
    fn an_unreadable_guard_opens_rather_than_seals() {
        // A stray operator must not make a room permanently unreachable;
        // the failure mode has to be a passable door, not a dead end.
        assert!(matches!(cond("[#wobble: [#a, 1]]"), Cond::Always));
        assert!(matches!(cond("[]"), Cond::Always));
        assert!(matches!(cond("[#and: []]"), Cond::Always));
    }

    #[test]
    fn always_is_spelled_as_an_equality() {
        assert!(matches!(cond("[#equals: [#always, 1]]"), Cond::Always));
        // `#always` holds 1, so comparing it to 0 is how a sprite is switched
        // off without being deleted. Four survive in the shipped data, and
        // reading them as unconditional drew the panel graphic that covers
        // the psionic bar's readouts.
        assert!(matches!(cond("[#equals: [#always, 0]]"), Cond::Never));
    }

    #[test]
    fn includes_and_lacks_read_pools() {
        let inc = cond("[#includes: [#hauntsRemaining, #gazebo2]]");
        let lacks = cond("[#lacks: [#hauntsRemaining, #gazebo2]]");
        let mut s = State::new();
        s.set_all("hauntsRemaining", vec![Value::Symbol("gazebo2".into())]);
        assert!(s.test(&inc) && !s.test(&lacks));
        s.trim_item("hauntsRemaining", &Value::Symbol("gazebo2".into()));
        assert!(!s.test(&inc) && s.test(&lacks));
    }

    // --- hotspot resolution ---------------------------------------------

    #[test]
    fn overlapping_exits_resolve_by_order_not_by_area() {
        // The front porch offers two forward exits whose guards can both
        // hold. Director takes the first. Taking the smaller sent the player
        // into a lit house they had never turned the lights on in.
        let dark = spot(Verb::Forward, Rect { left: 0, top: 0, right: 200, bottom: 200 });
        let lit = spot(Verb::Forward, Rect { left: 50, top: 50, right: 100, bottom: 100 });
        let n = room(vec![dark, lit]);
        let hit = n.hit_test(75, 75, false, |_| true).expect("inside both");
        assert_eq!(hit.bounds.right, 200, "first listed wins, though it is larger");
    }

    #[test]
    fn a_target_is_listed_before_the_region_it_sits_inside() {
        // An examine target drawn over a walk region is examinable because
        // the room lists it first, which is how the data is written: across
        // all 1320 rooms there is no case of a navigation region listed
        // before an examine or pointer target that sits inside it.
        let look = spot(Verb::Examine, Rect { left: 50, top: 50, right: 100, bottom: 100 });
        let walk = spot(Verb::Forward, Rect { left: 0, top: 0, right: 200, bottom: 200 });
        let n = room(vec![look, walk]);
        assert_eq!(n.hit_test(75, 75, false, |_| true).unwrap().verb, Verb::Examine);
        // and outside the examine box the walk region still answers
        assert_eq!(n.hit_test(150, 150, false, |_| true).unwrap().verb, Verb::Forward);
    }

    #[test]
    fn browse_loses_wherever_it_is_listed() {
        // The one exception to list order. `#browse` blankets the frame and
        // is the catch-all, and in the 36 rooms where it is not listed last
        // there are five places it would otherwise swallow a real affordance.
        let blanket = spot(Verb::Browse, Rect { left: 0, top: 0, right: 400, bottom: 400 });
        let real = spot(Verb::Pointer, Rect { left: 100, top: 100, right: 200, bottom: 200 });
        let n = room(vec![blanket, real]);
        assert_eq!(n.hit_test(150, 150, false, |_| true).unwrap().verb, Verb::Pointer);
        // Outside it, browse is still the answer.
        assert_eq!(n.hit_test(300, 300, false, |_| true).unwrap().verb, Verb::Browse);
    }

    #[test]
    fn the_page_regions_of_a_book_beat_the_one_that_closes_it() {
        // The BAR manual lists two page-turn regions and then a stage-sized
        // pointer to shut the book. Ranking pointer above nextPage -- which a
        // table of priorities by verb did -- meant every attempt to turn a
        // page closed the manual instead, and the manual holds two of the
        // three settings the machine in the living room needs.
        let next = spot(Verb::NextPage, Rect { left: 341, top: 58, right: 558, bottom: 363 });
        let prev = spot(Verb::NextPage, Rect { left: 71, top: 54, right: 287, bottom: 358 });
        let shut = spot(Verb::Pointer, Rect { left: -2, top: 32, right: 641, bottom: 386 });
        let n = room(vec![next, prev, shut]);
        let hit = n.hit_test(450, 200, false, |_| true).unwrap();
        assert_eq!(hit.verb, Verb::NextPage);
        assert_eq!(hit.bounds.left, 341, "the forward half, not the back one");
        // The margins outside both pages still shut it.
        assert_eq!(n.hit_test(320, 370, false, |_| true).unwrap().verb, Verb::Pointer);
    }

    #[test]
    fn a_failing_guard_hides_its_hotspot() {
        let mut locked = spot(Verb::Forward, Rect { left: 0, top: 0, right: 100, bottom: 100 });
        locked.condition = Cond::Equals { key: "open".into(), value: Value::Int(1) };
        let n = room(vec![locked]);
        let s = State::new();
        assert!(n.hit_test(50, 50, false, |c| s.test(c)).is_none());
    }

    #[test]
    fn a_hotspot_with_no_actions_is_not_a_target() {
        // Browse regions are usually a commented-out line. They must not
        // swallow a click that a live region underneath would handle.
        let mut inert = spot(Verb::Examine, Rect { left: 0, top: 0, right: 200, bottom: 200 });
        inert.actions.clear();
        let live = spot(Verb::Forward, Rect { left: 0, top: 0, right: 200, bottom: 200 });
        let n = room(vec![inert, live]);
        assert_eq!(n.hit_test(50, 50, false, |_| true).unwrap().verb, Verb::Forward);
    }

    #[test]
    fn a_greater_guard_compares_rather_than_passing() {
        // `#greater` is used twice in the game and was falling through to
        // `Always`, so both sprites showed unconditionally. One of them is the
        // telegram in the room where Margaret's chapter opens, which drew over
        // the scene it is supposed to appear after.
        let c = Cond::parse(&parse_value("[#greater: [#showMontage, 1]]").unwrap());
        assert!(matches!(c, Cond::Greater { .. }), "parsed as {c:?}");
        let mut s = State::new();
        s.set_all("showMontage", vec![Value::Int(1)]);
        assert!(!s.test(&c), "1 is not greater than 1");
        s.set_all("showMontage", vec![Value::Int(2)]);
        assert!(s.test(&c));
    }

    #[test]
    fn what_is_in_hand_outranks_walking_away() {
        // The scanner's door knob sits inside the region that walks through
        // the door. Ranked below it, a click carrying the scanner walks the
        // player away instead of using what they are holding; 466 of the
        // game's 800 item regions are covered like this.
        // Listed first, as it is in 702 of the 1320 rooms.
        let apply = spot(Verb::ItemInUse, Rect { left: 100, top: 100, right: 200, bottom: 200 });
        let walk = spot(Verb::Pointer, Rect { left: 0, top: 0, right: 400, bottom: 400 });
        let n = room(vec![apply, walk]);
        assert_eq!(
            n.hit_test(150, 150, true, |_| true).unwrap().verb,
            Verb::ItemInUse,
            "carrying something, the click applies it"
        );
        assert_eq!(
            n.hit_test(150, 150, false, |_| true).unwrap().verb,
            Verb::Pointer,
            "empty handed, the same click walks"
        );
    }

    #[test]
    fn an_item_region_still_yields_outside_its_bounds() {
        let walk = spot(Verb::Pointer, Rect { left: 0, top: 0, right: 400, bottom: 400 });
        let apply = spot(Verb::ItemInUse, Rect { left: 100, top: 100, right: 200, bottom: 200 });
        let n = room(vec![walk, apply]);
        assert_eq!(
            n.hit_test(300, 300, true, |_| true).unwrap().verb,
            Verb::Pointer
        );
    }

    #[test]
    fn a_click_outside_every_region_hits_nothing() {
        let n = room(vec![spot(Verb::Forward, Rect { left: 0, top: 0, right: 10, bottom: 10 })]);
        assert!(n.hit_test(500, 500, false, |_| true).is_none());
        // right and bottom edges are exclusive
        assert!(n.hit_test(10, 5, false, |_| true).is_none());
        assert!(n.hit_test(9, 9, false, |_| true).is_some());
    }
}
