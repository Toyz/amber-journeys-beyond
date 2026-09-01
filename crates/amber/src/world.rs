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

    /// Hotspots overlap heavily and the data relies on specific-beats-general
    /// resolution: `#browse` blankets the whole frame, so it must lose to any
    /// real affordance sharing the same pixels.
    pub fn priority(self) -> u8 {
        match self {
            Verb::Browse => 0,
            Verb::ItemInUse => 1,
            Verb::Left | Verb::Right | Verb::Forward | Verb::Up | Verb::Down => 2,
            Verb::NextPage | Verb::RotateLeft | Verb::RotateRight => 3,
            Verb::Examine | Verb::Pointer => 4,
        }
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
    /// State `key` equals `value`.
    Equals { key: String, value: Value },
    /// State `key` is numerically below `value`.
    Less { key: String, value: Value },
    /// State `key` is a list containing `value`.
    Includes { key: String, value: Value },
    /// State `key` is a list not containing `value`.
    Lacks { key: String, value: Value },
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

impl Cond {
    /// Builds a condition from the `[#op: [key, value]]` shape. Unrecognised
    /// operators degrade to `Always` so a stray key cannot make a room
    /// unreachable.
    pub fn parse(v: &Value) -> Cond {
        let Value::Props(map) = v else {
            return Cond::Always;
        };
        let Some((op, operand)) = map.iter().next() else {
            return Cond::Always;
        };

        let pair = || -> (String, Value) {
            match operand.as_list() {
                Some([k, val]) => (
                    k.as_str().unwrap_or_default().to_ascii_lowercase(),
                    val.clone(),
                ),
                _ => (String::new(), Value::Void),
            }
        };

        match op.as_str() {
            "equals" => {
                let (key, value) = pair();
                if key == "always" {
                    Cond::Always
                } else {
                    Cond::Equals { key, value }
                }
            }
            "less" => {
                let (key, value) = pair();
                Cond::Less { key, value }
            }
            "includes" => {
                let (key, value) = pair();
                Cond::Includes { key, value }
            }
            "lacks" => {
                let (key, value) = pair();
                Cond::Lacks { key, value }
            }
            // `#not` wraps a bare operand pair rather than a nested condition,
            // so rebuild it as a negated equality.
            "not" => {
                let (key, value) = pair();
                Cond::Not(Box::new(Cond::Equals { key, value }))
            }
            "and" | "or" => {
                let parts: Vec<Cond> = operand
                    .as_list()
                    .unwrap_or_default()
                    .iter()
                    .map(Cond::parse)
                    .collect();
                if op == "and" {
                    Cond::And(parts)
                } else {
                    Cond::Or(parts)
                }
            }
            _ => Cond::Always,
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
#[derive(Clone, Debug)]
pub struct Node {
    /// Index within its `.DAT` file, which is how the game addresses it.
    pub index: usize,
    /// The room's name from its chapter's location table, when it has one.
    pub name: Option<String>,
    /// Which character's chapter this room belongs to.
    pub domain: String,
    pub preload: Vec<u32>,
    pub sprites: Vec<Sprite>,
    pub hotspots: Vec<Hotspot>,
    pub custom_palette: Option<String>,
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
            domain: domain.to_owned(),
            preload,
            sprites,
            hotspots,
            custom_palette: v
                .get_str("CustomPalette")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            storage_cast,
            ambience,
        }
    }

    /// Picks the hotspot a click at `(x, y)` should trigger.
    ///
    /// Regions overlap by design, so this resolves by verb priority first and
    /// then by smallest area, which is what makes a small `#examine` target win
    /// over the room-sized `#browse` rectangle underneath it.
    pub fn hit_test(&self, x: i32, y: i32, live: impl Fn(&Cond) -> bool) -> Option<&Hotspot> {
        self.hotspots
            .iter()
            .filter(|h| h.bounds.contains(x, y))
            .filter(|h| !h.actions.is_empty())
            .filter(|h| live(&h.condition))
            .max_by(|a, b| {
                a.verb
                    .priority()
                    .cmp(&b.verb.priority())
                    .then(b.bounds.area().cmp(&a.bounds.area()))
            })
    }
}

/// Every room in the game, grouped by the chapter it belongs to.
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

    /// Every room sharing a name, for diagnostics.
    pub fn resolve_all(&self, name: &str) -> &[usize] {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }
}

impl World {
    /// Loads every `<DOMAIN>_<n>.DAT` under `root`, in numeric order.
    ///
    /// Amber splits its rooms across four chapters, one per haunting, each in its
    /// own directory with its own Director movie.
    pub fn load(root: &Path) -> std::io::Result<World> {
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
            let path = root.join(dir);
            if !path.is_dir() {
                continue;
            }
            let mut files: Vec<(u32, std::path::PathBuf)> = std::fs::read_dir(&path)?
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_ascii_uppercase();
                    let stem = name.strip_suffix(".DAT")?;
                    let n = stem.strip_prefix(&format!("{prefix}_"))?;
                    Some((n.parse().ok()?, e.path()))
                })
                .collect();
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

            let movie = director::Movie::open(path.join(format!("{dir}.DXR"))).ok();
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
                        .or_insert_with(Vec::new)
                        .push(index);
                    nodes.push(node);
                }
            }

            for (_, file) in files {
                let bytes = std::fs::read(&file)?;
                let records = match parse_dat(&bytes) {
                    Ok(r) => r,
                    // One unparsable file should not sink the other chapters.
                    Err(e) => {
                        eprintln!("warning: {}: {e}", file.display());
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

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
