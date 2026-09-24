//! A minimal JSON reader, used only for the `.ocrw` `meta` block.
//!
//! # Contract
//!
//! Accepts RFC 8259 text. Object keys keep their file order and duplicates
//! are kept as written; [`Json::get`] returns the first match. Numbers are
//! parsed as `f64` — `meta` carries counts, codepoints and px/em sizes, all
//! of which are exact in `f64`. Depth is capped at [`MAX_DEPTH`] so a
//! malformed file cannot recurse the parser into a stack overflow.
//!
//! This exists rather than a dependency because `ocrcer-core` has none
//! (`CLAUDE.md` rule 3), and because `meta` is the only JSON the runtime
//! ever reads.

/// Nesting limit. A `meta` block is two levels deep; anything approaching
/// this is malformed or hostile.
pub const MAX_DEPTH: usize = 32;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// Why a `meta` block could not be parsed. `at` is a byte offset into the
/// input, so a malformed file can be pointed at rather than described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub at: usize,
    pub what: &'static str,
}

impl core::fmt::Display for JsonError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl Json {
    /// Parses a complete JSON document. Trailing whitespace is allowed;
    /// trailing non-whitespace is an error.
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut p = Parser { b: text.as_bytes(), at: 0, depth: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.at != p.b.len() {
            return Err(p.err("trailing data after value"));
        }
        Ok(v)
    }

    /// The value for `key` in an object, or `None` for a non-object or a
    /// missing key.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// The value as `u32`, or `None` if it is not a number or does not land
    /// exactly on a `u32`. A count that arrives as `12.5` is a corrupt file,
    /// not a rounding opportunity.
    pub fn as_u32(&self) -> Option<u32> {
        let n = self.as_f64()?;
        if n.fract() == 0.0 && n >= 0.0 && n <= f64::from(u32::MAX) {
            Some(n as u32)
        } else {
            None
        }
    }

    /// As [`Json::as_u32`], but signed; `meta` writes an absent case twin as
    /// `-1`.
    pub fn as_i64(&self) -> Option<i64> {
        let n = self.as_f64()?;
        if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
            Some(n as i64)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    /// The object's fields in file order, or `None` for a non-object. Used
    /// by callers that need to enumerate every key (`ocrcer-build`'s
    /// `safetensors` reader listing tensor names), not just look one up.
    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(v) => Some(v),
            _ => None,
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, what: &'static str) -> JsonError {
        JsonError { at: self.at, what }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn lit(&mut self, word: &[u8]) -> bool {
        if self.b[self.at..].starts_with(word) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.lit(b"true") => Ok(Json::Bool(true)),
            Some(b'f') if self.lit(b"false") => Ok(Json::Bool(false)),
            Some(b'n') if self.lit(b"null") => Ok(Json::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.at += 1; // '{'
        self.depth += 1;
        let mut fields = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a key"));
            }
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.at += 1;
            self.ws();
            fields.push((k, self.value()?));
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.at += 1; // '['
        self.depth += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1; // '"'
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            match c {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let e = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                c if c < 0x20 => return Err(self.err("raw control character in string")),
                _ => {
                    // Copy one whole UTF-8 sequence. The input came from
                    // `&str`, so the boundary arithmetic is sound.
                    let len = utf8_len(c);
                    let end = self.at + len;
                    if end > self.b.len() {
                        return Err(self.err("truncated UTF-8 sequence"));
                    }
                    match core::str::from_utf8(&self.b[self.at..end]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => return Err(self.err("invalid UTF-8 in string")),
                    }
                    self.at = end;
                }
            }
        }
    }

    /// A `\u` escape, joining a surrogate pair when one is present. An
    /// unpaired surrogate becomes U+FFFD rather than an error: it cannot
    /// appear in anything this crate reads, and refusing the whole model
    /// file over one is a worse trade than replacing it.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let hi = self.hex4()?;
        if (0xD800..0xDC00).contains(&hi) {
            if self.b[self.at..].starts_with(b"\\u") {
                let save = self.at;
                self.at += 2;
                let lo = self.hex4()?;
                if (0xDC00..0xE000).contains(&lo) {
                    let cp = 0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    return Ok(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                self.at = save;
            }
            return Ok('\u{FFFD}');
        }
        Ok(char::from_u32(hi).unwrap_or('\u{FFFD}'))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.at + 4 > self.b.len() {
            return Err(self.err("truncated \\u escape"));
        }
        let mut v = 0u32;
        for i in 0..4 {
            let d = self.b[self.at + i];
            let n = match d {
                b'0'..=b'9' => u32::from(d - b'0'),
                b'a'..=b'f' => u32::from(d - b'a') + 10,
                b'A'..=b'F' => u32::from(d - b'A') + 10,
                _ => return Err(self.err("bad hex digit in \\u escape")),
            };
            v = v * 16 + n;
        }
        self.at += 4;
        Ok(v)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if self.at == start {
            return Err(self.err("expected a value"));
        }
        let text = core::str::from_utf8(&self.b[start..self.at]).map_err(|_| self.err("bad number"))?;
        text.parse::<f64>().map(Json::Num).map_err(|_| JsonError { at: start, what: "bad number" })
    }
}

fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shape_a_meta_block_has() {
        let v = Json::parse(
            r#"{"feature_version":1,"sizes":[16,21.5,32],
                "faces":[{"family":"A \"B\"","distribution":"shippable"}],
                "charset":[{"index":0,"cp":48,"twin":-1}]}"#,
        )
        .unwrap();
        assert_eq!(v.get("feature_version").unwrap().as_u32(), Some(1));
        let sizes = v.get("sizes").unwrap().as_array().unwrap();
        assert_eq!(sizes.len(), 3);
        assert_eq!(sizes[1].as_f64(), Some(21.5));
        let face = &v.get("faces").unwrap().as_array().unwrap()[0];
        assert_eq!(face.get("family").unwrap().as_str(), Some("A \"B\""));
        let c = &v.get("charset").unwrap().as_array().unwrap()[0];
        assert_eq!(c.get("cp").unwrap().as_u32(), Some(48));
        assert_eq!(c.get("twin").unwrap().as_i64(), Some(-1));
    }

    #[test]
    fn a_count_that_is_not_an_integer_is_not_a_count() {
        let v = Json::parse("{\"n\":12.5}").unwrap();
        assert_eq!(v.get("n").unwrap().as_u32(), None);
        assert_eq!(v.get("n").unwrap().as_f64(), Some(12.5));
    }

    #[test]
    fn escapes_and_non_ascii_survive() {
        let v = Json::parse(r#"{"a":"é—\t","b":"😀"}"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_str(), Some("é—\t"));
        assert_eq!(v.get("b").unwrap().as_str(), Some("😀"));
        let raw = Json::parse("{\"a\":\"é—\"}").unwrap();
        assert_eq!(raw.get("a").unwrap().as_str(), Some("é—"));
    }

    #[test]
    fn malformed_input_is_refused_rather_than_guessed_at() {
        for bad in [
            "{", "{\"a\"}", "{\"a\":}", "[1,]", "{,}", "tru", "\"unterminated",
            "1 2", "{\"a\":1}x",
        ] {
            assert!(Json::parse(bad).is_err(), "should have refused {bad:?}");
        }
    }

    /// Depth is capped so a hostile `meta` cannot overflow the stack. The
    /// parser must refuse rather than crash.
    #[test]
    fn deep_nesting_is_refused_not_crashed_on() {
        let deep = "[".repeat(MAX_DEPTH + 5) + &"]".repeat(MAX_DEPTH + 5);
        assert!(Json::parse(&deep).is_err());
        let ok = "[".repeat(MAX_DEPTH - 1) + &"]".repeat(MAX_DEPTH - 1);
        assert!(Json::parse(&ok).is_ok());
    }
}
