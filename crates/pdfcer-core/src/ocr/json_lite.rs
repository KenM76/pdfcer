//! A minimal JSON reader (RFC 8259) for model metadata files, so the
//! PaddleOCR-VL tokenizer needs no serde stack. Values only, no
//! serialisation. Bounded by [`MAX_DEPTH`]; the caller bounds the input size.

/// Deepest array/object nesting accepted.
pub const MAX_DEPTH: usize = 64;

/// A parsed JSON value. Objects keep their members in file order, duplicates
/// included; [`Json::get`] returns the first.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// Any number, as `f64`.
    Num(f64),
    /// A string, escapes resolved.
    Str(String),
    /// An array.
    Arr(Vec<Json>),
    /// An object's members in file order.
    Obj(Vec<(String, Json)>),
}

/// Why a JSON text was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("JSON error at byte {offset}: {reason}")]
pub struct JsonError {
    /// Byte offset of the failure.
    pub offset: usize,
    /// What was wrong.
    pub reason: &'static str,
}

impl Json {
    /// The first member named `key`, when `self` is an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, when `self` is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A non-negative integer that fits `u32`.
    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Num(n) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) =>
            {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Some(*n as u32)
            }
            _ => None,
        }
    }

    /// The boolean, when `self` is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Whether `self` is `null`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// Parse one JSON text; trailing non-whitespace is an error.
///
/// # Errors
///
/// [`JsonError`] naming the byte offset of the first malformation, or nesting
/// beyond [`MAX_DEPTH`].
pub fn parse(bytes: &[u8]) -> Result<Json, JsonError> {
    let mut p = Parser { s: bytes, i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != bytes.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, reason: &'static str) -> JsonError {
        JsonError {
            offset: self.i,
            reason,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        let ok = self.s.get(self.i..self.i + lit.len()) == Some(lit);
        if ok {
            self.i += lit.len();
        }
        ok
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        self.ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ if self.eat(b"null") => Ok(Json::Null),
            _ if self.eat(b"true") => Ok(Json::Bool(true)),
            _ if self.eat(b"false") => Ok(Json::Bool(false)),
            _ => Err(self.err("expected a value")),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.i += 1;
        let mut out = Vec::new();
        self.ws();
        if self.eat(b"]") {
            return Ok(Json::Arr(out));
        }
        loop {
            out.push(self.value(depth + 1)?);
            self.ws();
            if self.eat(b"]") {
                return Ok(Json::Arr(out));
            }
            if !self.eat(b",") {
                return Err(self.err("expected `,` or `]`"));
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.i += 1;
        let mut out = Vec::new();
        self.ws();
        if self.eat(b"}") {
            return Ok(Json::Obj(out));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a member name"));
            }
            let key = self.string()?;
            self.ws();
            if !self.eat(b":") {
                return Err(self.err("expected `:`"));
            }
            out.push((key, self.value(depth + 1)?));
            self.ws();
            if self.eat(b"}") {
                return Ok(Json::Obj(out));
            }
            if !self.eat(b",") {
                return Err(self.err("expected `,` or `}`"));
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        self.eat(b"-");
        let digits = |p: &mut Self| {
            let from = p.i;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.i += 1;
            }
            p.i > from
        };
        if !digits(self) {
            return Err(self.err("expected digits"));
        }
        if self.eat(b".") && !digits(self) {
            return Err(self.err("expected fraction digits"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !digits(self) {
                return Err(self.err("expected exponent digits"));
            }
        }
        let text = self
            .s
            .get(start..self.i)
            .and_then(|b| std::str::from_utf8(b).ok())
            .ok_or_else(|| self.err("bad number"))?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| self.err("bad number"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let h = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|t| u32::from_str_radix(t, 16).ok())
            .ok_or_else(|| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(h)
    }

    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let c = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
        self.i += 1;
        let ch = match c {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let hi = self.hex4()?;
                let code = if (0xD800..0xDC00).contains(&hi) && self.eat(b"\\u") {
                    let lo = self.hex4()?;
                    if !(0xDC00..0xE000).contains(&lo) {
                        return Err(self.err("unpaired surrogate"));
                    }
                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    hi
                };
                char::from_u32(code).ok_or_else(|| self.err("unpaired surrogate"))?
            }
            _ => return Err(self.err("unknown escape")),
        };
        out.push(ch);
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let run = self.i;
            while matches!(self.peek(), Some(b) if b != b'"' && b != b'\\' && b >= 0x20) {
                self.i += 1;
            }
            let chunk = self
                .s
                .get(run..self.i)
                .and_then(|b| std::str::from_utf8(b).ok())
                .ok_or_else(|| self.err("invalid UTF-8 in string"))?;
            out.push_str(chunk);
            match self.peek() {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.err("control character in string")),
                None => return Err(self.err("unterminated string")),
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn nested_values_parse_in_order() {
        let v = parse(br#" {"a": [1, -2.5e1, true, null], "b": {"c": "x"}, "a": 0} "#).unwrap();
        let Json::Arr(a) = v.get("a").unwrap() else {
            panic!("a is an array")
        };
        assert_eq!(a[0].as_u32(), Some(1));
        assert_eq!(a[1], Json::Num(-25.0));
        assert_eq!(a[2].as_bool(), Some(true));
        assert!(a[3].is_null());
        assert_eq!(
            v.get("b").and_then(|b| b.get("c")).and_then(Json::as_str),
            Some("x")
        );
    }

    #[test]
    fn escapes_and_surrogate_pairs_decode() {
        let v = parse(br#""a\n\u2581\ud83d\ude00\"""#).unwrap();
        assert_eq!(v.as_str(), Some("a\n\u{2581}\u{1F600}\""));
        assert!(parse(br#""\ud83d""#).is_err(), "a lone high surrogate");
        assert!(parse(br#""\ud83dx""#).is_err());
    }

    #[test]
    fn malformations_are_errors_not_panics() {
        for bad in [
            &b""[..],
            b"{",
            b"[1,]",
            b"{\"a\" 1}",
            b"01x",
            b"-",
            b"1.",
            b"1e",
            b"\"abc",
            b"\"\x01\"",
            b"nul",
            b"[1] 2",
            b"\"\\q\"",
            b"\"\xff\"",
        ] {
            assert!(parse(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert_eq!(
            parse(deep.as_bytes()).unwrap_err().reason,
            "nesting too deep"
        );
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(ok.as_bytes()).is_ok());
    }
}
