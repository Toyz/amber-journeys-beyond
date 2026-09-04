
/// An integer rectangle, as written by Lingo's `rect(l, t, r, b)`.
///
/// Note the argument order: Lingo takes left, top, right, bottom, which is not
/// the top, left, bottom, right order Director uses inside binary chunks.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn area(&self) -> i64 {
        self.width().max(0) as i64 * self.height().max(0) as i64
    }
}

/// A parsed Lingo literal.
///
/// Property lists are association lists, not maps: Lingo permits the same key
/// more than once and the game relies on it. A compound guard is written as
/// `[#and: [#equals: [a, b], #equals: [c, d]]]`, two entries under one key, and
/// 247 of the 381 compound guards in this game take that form. Storing them in
/// a map drops one clause of each and silently weakens the condition, which is
/// how a locked door comes to open. Entries are therefore kept in order, with
/// duplicates preserved, and lookup returns the first match as Lingo does.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Void,
    Int(i32),
    Float(f64),
    /// A `#symbol`. Stored without the hash and compared case-insensitively,
    /// matching Lingo, so `#Forward` and `#forward` are the same symbol.
    Symbol(String),
    String(String),
    Point(i32, i32),
    Rect(Rect),
    /// A linear list: `[a, b, c]`.
    List(Vec<Value>),
    /// A property list: `[#key: value, ...]`, in source order, duplicates kept.
    Props(Vec<(String, Value)>),
}

impl Value {
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i32),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Symbol(s) => Some(s),
            _ => None,
        }
    }

    /// Whether this is the named symbol, ignoring case and a leading `#`.
    ///
    /// Lingo compares symbols without regard to case, and the two pressings of
    /// the disc disagree about it: the PC location table says `bedrm_fadeIn`
    /// and the Macintosh one says `bedrm_fadein`. An exact comparison works on
    /// one disc and silently does nothing on the other, which is how
    /// Margaret's opening ran on the PC data and not on the Mac's.
    pub fn is_symbol(&self, name: &str) -> bool {
        self.as_symbol().is_some_and(|s| {
            s.trim_start_matches('#')
                .eq_ignore_ascii_case(name.trim_start_matches('#'))
        })
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_rect(&self) -> Option<Rect> {
        match self {
            Value::Rect(r) => Some(*r),
            _ => None,
        }
    }

    pub fn as_point(&self) -> Option<(i32, i32)> {
        match self {
            Value::Point(x, y) => Some((*x, *y)),
            _ => None,
        }
    }

    /// Looks up a property, ignoring case as Lingo does. With a repeated key
    /// the first entry wins, which is Lingo's own behaviour.
    pub fn get(&self, key: &str) -> Option<&Value> {
        let key = key.to_ascii_lowercase();
        match self {
            Value::Props(entries) => entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Every entry under a key, in order. Compound guards need all of them.
    pub fn get_all(&self, key: &str) -> Vec<&Value> {
        let key = key.to_ascii_lowercase();
        match self {
            Value::Props(entries) => entries
                .iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// All entries of a property list, in source order.
    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Props(entries) => entries,
            _ => &[],
        }
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get(key)?.as_int()
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_list(&self, key: &str) -> Option<&[Value]> {
        self.get(key)?.as_list()
    }

    /// True for the values Lingo treats as truthy in the game's conditions.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Void => false,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Symbol(s) => !s.eq_ignore_ascii_case("none"),
            Value::List(v) => !v.is_empty(),
            Value::Props(m) => !m.is_empty(),
            _ => true,
        }
    }

    /// Lingo equality: symbols and strings compare case-insensitively, and an
    /// integer compares equal to a numerically equal float.
    pub fn loosely_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Symbol(a) | Value::String(a), Value::Symbol(b) | Value::String(b)) => {
                a.eq_ignore_ascii_case(b)
            }
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
                (*a as f64 - *b).abs() < f64::EPSILON
            }
            (Value::Float(a), Value::Float(b)) => (a - b).abs() < f64::EPSILON,
            (Value::Void, Value::Void) => true,
            _ => self == other,
        }
    }
}

/// Writes a value back as Lingo source.
///
/// The inverse of [`crate::parse_value`], and deliberately so: the save file
/// this feeds is a Lingo property list of the same shape the game keeps its own
/// state in, so a save can be read -- and pasted back into the original's
/// `stateData` -- rather than being an opaque blob of somebody's serialiser.
///
/// Round-tripping is a test rather than a hope: `parse_value(v.to_string())`
/// gives `v` back for every variant.
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Void => write!(f, "VOID"),
            Value::Int(i) => write!(f, "{i}"),
            // `1` and `1.0` parse to different variants, so a float always
            // carries its point or it comes back an integer.
            Value::Float(x) if x.fract() == 0.0 => write!(f, "{x:.1}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Symbol(s) => write!(f, "#{s}"),
            Value::String(s) => write!(f, "\"{}\"", s.replace('"', "'")),
            Value::Point(x, y) => write!(f, "point({x}, {y})"),
            Value::Rect(r) => {
                write!(f, "rect({}, {}, {}, {})", r.left, r.top, r.right, r.bottom)
            }
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            // An empty property list is `[:]`; an empty linear list is `[]`.
            // Writing the wrong one turns a table into a list on the way back.
            Value::Props(pairs) if pairs.is_empty() => write!(f, "[:]"),
            Value::Props(pairs) => {
                write!(f, "[")?;
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "#{key}: {value}")?;
                }
                write!(f, "]")
            }
        }
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    /// Every variant survives being written and read back.
    ///
    /// This is the property the save file rests on, so it is asserted over the
    /// awkward cases rather than a happy one: an empty list is not an empty
    /// property list, and a whole-number float is not an integer.
    #[test]
    fn every_value_round_trips() {
        let cases = vec![
            Value::Void,
            Value::Int(-7),
            Value::Float(1.0),
            Value::Float(2.5),
            Value::Symbol("carrying".into()),
            Value::String("Gbhs_B_S".into()),
            Value::Point(320, 240),
            Value::Rect(Rect { left: 1, top: 2, right: 3, bottom: 4 }),
            Value::List(vec![]),
            Value::List(vec![Value::Int(1), Value::Symbol("None".into())]),
            Value::Props(vec![]),
            // Keys are lower case because the parser lower-cases them:
            // Lingo's symbols are case-insensitive, so `#tunedIn` and
            // `#tunedin` are one key and only one of them can come back. A
            // fixture in mixed case would be asserting my spelling rather than
            // the format.
            Value::Props(vec![
                ("tunedin".into(), Value::List(vec![Value::Symbol("bedroom".into())])),
                ("nested".into(), Value::Props(vec![("a".into(), Value::Int(0))])),
            ]),
        ];
        for value in cases {
            let written = value.to_string();
            let back = crate::parse_value(&written)
                .unwrap_or_else(|e| panic!("{written} did not parse back: {e:?}"));
            assert_eq!(back, value, "round trip changed {written}");
        }
    }
}
