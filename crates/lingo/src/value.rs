
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
