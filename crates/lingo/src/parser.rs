use std::fmt;

use crate::value::{Rect, Value};

#[derive(Debug)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for ParseError {}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            offset: self.pos,
            message: msg.into(),
        })
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        if self.eat(c) {
            Ok(())
        } else {
            self.err(format!(
                "expected {:?}, found {:?}",
                c as char,
                self.peek().map(|b| b as char)
            ))
        }
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'-') | Some(b'+')) {
            self.pos += 1;
        }
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == b'.' && !is_float {
                // Only a digit after the dot makes it a decimal point; otherwise
                // the dot belongs to whatever follows.
                if self.src.get(self.pos + 1).is_some_and(|d| d.is_ascii_digit()) {
                    is_float = true;
                    self.pos += 1;
                } else {
                    break;
                }
            } else if (c == b'e' || c == b'E')
                && self
                    .src
                    .get(self.pos + 1)
                    .is_some_and(|d| d.is_ascii_digit() || *d == b'-' || *d == b'+')
            {
                is_float = true;
                self.pos += 2;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        if is_float {
            text.parse::<f64>()
                .map(Value::Float)
                .or_else(|_| self.err("bad float"))
        } else {
            text.parse::<i32>()
                .map(Value::Int)
                .or_else(|_| self.err("bad integer"))
        }
    }

    fn string(&mut self) -> Result<Value, ParseError> {
        self.expect(b'"')?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'"' {
                let s = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                self.pos += 1;
                return Ok(Value::String(s));
            }
            self.pos += 1;
        }
        self.err("unterminated string")
    }

    /// Parses the `name(a, b, ...)` forms the data uses: `rect`, `point`, and the
    /// occasional `string(...)`. Anything else is kept as a symbol so an unknown
    /// constructor does not abort the whole file.
    fn call(&mut self, name: &str) -> Result<Value, ParseError> {
        self.expect(b'(')?;
        let mut args = Vec::new();
        if !self.eat(b')') {
            loop {
                args.push(self.value()?);
                if self.eat(b',') {
                    continue;
                }
                self.expect(b')')?;
                break;
            }
        }
        let n = |i: usize| args.get(i).and_then(Value::as_int).unwrap_or(0);
        match name.to_ascii_lowercase().as_str() {
            "rect" => Ok(Value::Rect(Rect {
                left: n(0),
                top: n(1),
                right: n(2),
                bottom: n(3),
            })),
            "point" => Ok(Value::Point(n(0), n(1))),
            "string" => Ok(args.into_iter().next().unwrap_or(Value::Void)),
            _ => Ok(Value::List(args)),
        }
    }

    /// A bracketed run is either a property list or a linear list. Lingo spells an
    /// empty property list `[:]` and an empty linear list `[]`; otherwise the
    /// first element decides, by whether a colon follows its key.
    fn bracketed(&mut self) -> Result<Value, ParseError> {
        self.expect(b'[')?;
        if self.eat(b']') {
            return Ok(Value::List(Vec::new()));
        }
        if self.eat(b':') {
            self.expect(b']')?;
            return Ok(Value::Props(Vec::new()));
        }

        let mut list = Vec::new();
        // Kept as a list of pairs so a repeated key survives; Lingo allows it
        // and the game's compound guards depend on it.
        let mut props: Vec<(String, Value)> = Vec::new();
        let mut is_props = false;

        loop {
            self.skip_ws();
            // Look ahead: a `#key:` or `"key":` opener means this is a prop list.
            let save = self.pos;
            let key = if self.peek() == Some(b'#') {
                self.pos += 1;
                Some(self.ident())
            } else if self.peek() == Some(b'"') {
                match self.string()? {
                    Value::String(s) => Some(s),
                    _ => None,
                }
            } else {
                None
            };

            let followed_by_colon = key.is_some() && {
                self.skip_ws();
                self.peek() == Some(b':')
            };

            if followed_by_colon {
                is_props = true;
                self.pos += 1; // consume ':'
                let v = self.value()?;
                props.push((key.unwrap().to_ascii_lowercase(), v));
            } else {
                if is_props {
                    return self.err("mixed list and property entries");
                }
                self.pos = save;
                list.push(self.value()?);
            }

            if self.eat(b',') {
                continue;
            }
            self.expect(b']')?;
            break;
        }

        Ok(if is_props {
            Value::Props(props)
        } else {
            Value::List(list)
        })
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        self.skip_ws();
        match self.peek() {
            None => self.err("unexpected end of input"),
            Some(b'[') => self.bracketed(),
            Some(b'"') => self.string(),
            Some(b'#') => {
                self.pos += 1;
                let name = self.ident();
                if name.is_empty() {
                    return self.err("empty symbol");
                }
                Ok(Value::Symbol(name))
            }
            Some(c) if c.is_ascii_digit() || c == b'-' || c == b'+' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let name = self.ident();
                self.skip_ws();
                if self.peek() == Some(b'(') {
                    self.call(&name)
                } else if name.eq_ignore_ascii_case("void") || name.eq_ignore_ascii_case("empty") {
                    Ok(Value::Void)
                } else {
                    // A bare word in this data is always a symbol in practice.
                    Ok(Value::Symbol(name))
                }
            }
            Some(c) => self.err(format!("unexpected byte {:?}", c as char)),
        }
    }
}

/// Parses a single Lingo literal.
pub fn parse_value(src: &str) -> Result<Value, ParseError> {
    let mut p = Parser {
        src: src.as_bytes(),
        pos: 0,
    };
    let v = p.value()?;
    p.skip_ws();
    Ok(v)
}

/// Parses a whole `.DAT` file into its records.
///
/// Each file opens with a `* 10/4/96,4:22 PM *` banner padded out with spaces,
/// then holds one property list per room. Records are separated by a single
/// 0xBC byte rather than a newline, so the whole file reads as one physical
/// line to ordinary text tools. Records come back in file order, which is the
/// order the game indexes rooms by.
pub fn parse_dat(bytes: &[u8]) -> Result<Vec<Value>, ParseError> {
    /// Bytes that end a record. Director wrote 0xBC; a NUL is accepted too so
    /// files from other pressings of the disc still load.
    fn is_separator(c: u8) -> bool {
        c == 0xBC || c == 0x00
    }

    // Skip the banner by finding the closing `*` of the leading comment.
    let mut cursor = 0usize;
    if bytes.first() == Some(&b'*') {
        if let Some(end) = bytes.iter().skip(1).position(|&c| c == b'*') {
            cursor = end + 2;
        }
    }

    let mut out = Vec::new();
    while cursor < bytes.len() {
        let end = bytes[cursor..]
            .iter()
            .position(|&c| is_separator(c))
            .map(|i| cursor + i)
            .unwrap_or(bytes.len());

        // The bytes are Mac Roman, but every record is ASCII structure; a direct
        // widening keeps any high-byte text intact for later display.
        let text: String = bytes[cursor..end].iter().map(|&c| c as char).collect();
        if text.trim_start().starts_with('[') {
            let mut p = Parser {
                src: text.as_bytes(),
                pos: 0,
            };
            out.push(p.value()?);
        }
        cursor = end + 1;
    }
    Ok(out)
}
