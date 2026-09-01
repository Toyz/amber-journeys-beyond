use std::collections::BTreeMap;

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
/// Property lists keep insertion-independent ordering via `BTreeMap` because the
/// game only ever addresses them by key, and a sorted map makes diffs stable.
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
    /// A property list: `[#key: value, ...]`.
    Props(BTreeMap<String, Value>),
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

    /// Looks up a property, ignoring case as Lingo does.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Props(m) => m.get(&key.to_ascii_lowercase()),
            _ => None,
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
