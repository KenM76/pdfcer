//! The TOML subset a profile is written in: `key = value` lines, `[[rule]]`
//! and `[[object]]` table headers, `#` comments. Values are basic or literal
//! strings, integers, floats, booleans, and one-line arrays of those. Every
//! accepted file is valid TOML, so any TOML tool reads it.

/// A parsed value.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
}

/// Which table a run of keys belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Top,
    Rule,
    Object,
}

/// One table: its kind, header line, and `(key, value, line)` entries.
pub(super) struct Table {
    pub(super) kind: Kind,
    pub(super) line: usize,
    pub(super) entries: Vec<(String, Value, usize)>,
}

/// Split `text` into tables; the first is always [`Kind::Top`].
pub(super) fn tables(text: &str) -> Result<Vec<Table>, (usize, String)> {
    let mut out = vec![Table {
        kind: Kind::Top,
        line: 0,
        entries: Vec::new(),
    }];
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let s = raw.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if let Some(header) = s.strip_prefix("[[") {
            let name = header
                .split_once("]]")
                .filter(|(_, rest)| rest.trim().is_empty() || rest.trim().starts_with('#'))
                .map(|(n, _)| n.trim())
                .ok_or((line, "malformed table header".to_owned()))?;
            let kind = match name {
                "rule" => Kind::Rule,
                "object" => Kind::Object,
                _ => return Err((line, format!("unknown table [[{name}]]"))),
            };
            out.push(Table {
                kind,
                line,
                entries: Vec::new(),
            });
            continue;
        }
        let (key, rest) = s
            .split_once('=')
            .ok_or((line, "expected key = value".to_owned()))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err((line, format!("bad key {key:?}")));
        }
        let mut cur = Cursor {
            s: rest.trim_start(),
        };
        let value = cur.value(0).map_err(|m| (line, m))?;
        let tail = cur.s.trim_start();
        if !(tail.is_empty() || tail.starts_with('#')) {
            return Err((line, format!("unexpected text after value: {tail:?}")));
        }
        let table = out.last_mut().ok_or((line, "internal".to_owned()))?;
        if table.entries.iter().any(|(k, _, _)| k == key) {
            return Err((line, format!("duplicate key {key:?}")));
        }
        table.entries.push((key.to_owned(), value, line));
    }
    Ok(out)
}

struct Cursor<'a> {
    s: &'a str,
}

impl Cursor<'_> {
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        match self.s.as_bytes().first() {
            Some(b'"') => self.basic_string().map(Value::Str),
            Some(b'\'') => self.literal_string().map(Value::Str),
            Some(b'[') if depth == 0 => self.array(),
            Some(b'[') => Err("nested arrays are not supported".to_owned()),
            Some(_) => self.scalar(),
            None => Err("missing value".to_owned()),
        }
    }

    fn basic_string(&mut self) -> Result<String, String> {
        let mut out = String::new();
        let mut chars = self.s.char_indices().skip(1);
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.s = self.s.get(i + 1..).unwrap_or("");
                    return Ok(out);
                }
                '\\' => match chars.next().map(|(_, e)| e) {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let hex: String = (0..4)
                            .filter_map(|_| chars.next().map(|(_, h)| h))
                            .collect();
                        let c = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| format!("bad escape \\u{hex}"))?;
                        out.push(c);
                    }
                    other => return Err(format!("unsupported escape \\{}", other.unwrap_or(' '))),
                },
                _ => out.push(c),
            }
        }
        Err("unterminated string".to_owned())
    }

    fn literal_string(&mut self) -> Result<String, String> {
        let body = self.s.get(1..).unwrap_or("");
        let end = body.find('\'').ok_or("unterminated string")?;
        let out = body.get(..end).unwrap_or("").to_owned();
        self.s = body.get(end + 1..).unwrap_or("");
        Ok(out)
    }

    fn array(&mut self) -> Result<Value, String> {
        self.s = self.s.get(1..).unwrap_or("");
        let mut items = Vec::new();
        loop {
            self.s = self.s.trim_start();
            if let Some(rest) = self.s.strip_prefix(']') {
                self.s = rest;
                return Ok(Value::Array(items));
            }
            items.push(self.value(1)?);
            self.s = self.s.trim_start();
            if let Some(rest) = self.s.strip_prefix(',') {
                self.s = rest;
            } else if !self.s.starts_with(']') {
                return Err("expected , or ] in array".to_owned());
            }
        }
    }

    fn scalar(&mut self) -> Result<Value, String> {
        let end = self
            .s
            .find(|c: char| c.is_whitespace() || c == ',' || c == ']' || c == '#')
            .unwrap_or(self.s.len());
        let word = self.s.get(..end).unwrap_or("");
        self.s = self.s.get(end..).unwrap_or("");
        match word {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => {
                if let Ok(n) = word.parse::<i64>() {
                    Ok(Value::Int(n))
                } else if word.contains(['.', 'e', 'E'])
                    && let Ok(f) = word.parse::<f64>()
                    && f.is_finite()
                {
                    Ok(Value::Float(f))
                } else {
                    Err(format!("bad value {word:?}"))
                }
            }
        }
    }
}

/// `text` as a TOML basic string.
pub(super) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
