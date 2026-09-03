//! Exact-shape recognition of a form-field script as **one call with
//! literal arguments** — the gate every posture-B recompute passes through.
//!
//! # Purpose
//!
//! Decision 009 posture B natively recomputes a whitelist of well-known
//! Acrobat helpers (`AFSimple_Calculate`, `AF*_Format`) **without executing
//! any JavaScript**. Before any helper-specific matching can happen,
//! something must decide whether a `/JS` string is one of Acrobat's own
//! generated calls at all, or arbitrary author code that merely mentions the
//! same name. That decision is this module's entire job, and it is where the
//! safety of the whole posture lives.
//!
//! # The asymmetry that dictates every rule below
//!
//! The two ways to be wrong are **not** equally bad, and the design is
//! deliberately lopsided because of it:
//!
//! - A **false negative** — a genuine `AFSimple_Calculate` classified as
//!   `Custom` — costs the operator a recompute pdfcer could have offered.
//!   The stored `/V` is shown as-last-saved and *disclosed as possibly
//!   stale* (decision 009 §7). Nothing is wrong on the page; a capability is
//!   merely unoffered.
//! - A **false positive** — author code mis-recognised as a built-in and
//!   recomputed — writes a **wrong number into a real document**. On an
//!   invoice, that is a wrong total that looks exactly like a right one.
//!
//! So: **when in any doubt, `Custom`.** Every ambiguity in this module
//! resolves toward refusing to recognise. There is no "best effort" parse
//! here and no recovery from a malformed one — a script this module cannot
//! read *in full*, with total confidence, is not recognised.
//!
//! # What is accepted
//!
//! Exactly one call expression, optionally terminated by `;`, surrounded by
//! nothing but whitespace and comments:
//!
//! ```text
//! AFNumber_Format(2, 0, 0, 0, "", true);
//! AFSimple_Calculate("SUM", new Array("Item.1", "Item.2"));
//! AFSimple_Calculate("SUM", ["Item.1", "Item.2"])
//! ```
//!
//! Every argument must be a **literal**: a string, a number, `true`/`false`,
//! `null`, or an array of literals. That single constraint is what makes
//! non-execution possible — a literal has one meaning, knowable without
//! evaluating anything. The moment an argument is an identifier, a property
//! access, or an expression, its value depends on the runtime pdfcer does not
//! have, and the script is `Custom` by construction rather than by policy.
//!
//! # What is rejected, and why each rejection is load-bearing
//!
//! | Input | Why it must not be recognised |
//! |---|---|
//! | `x = 1; AFSimple_Calculate(…)` | A second statement can do anything, including change what the call means. |
//! | `if (a) AFSimple_Calculate(…)` | Conditional: Acrobat might not run it at all. |
//! | `AFSimple_Calculate("SUM", flds)` | `flds` is a runtime value; pdfcer cannot know the operands. |
//! | `AFSimple_Calculate("SU" + "M", …)` | Concatenation is evaluation. |
//! | `event.value = AFSimple_Calculate(…)` | An assignment target changes the effect. |
//! | `myAFNumber_Format(…)` | A different function whose name merely ends the same way. |
//! | `AFNumber_Format(2, 0, 0, 0, "", true) // then more` after a `}` | Any brace/bracket structure beyond an array literal. |
//!
//! Note the third row especially: Acrobat's *own* Calculate tab can emit a
//! variable-carrying form for long field lists in some versions. Refusing it
//! is a false negative — accepted deliberately, per the asymmetry above.
//!
//! # Comments
//!
//! Line (`//`) and block (`/* */`) comments are skipped wherever whitespace
//! is allowed, because Acrobat's generated scripts have historically carried
//! a leading banner comment and an operator's own annotation of a generated
//! script should not silently disable the recompute. Skipping a comment
//! cannot change what the call does, so this is the one liberty taken here
//! that does not widen what gets *executed* — nothing is executed at all.
//!
//! An **unterminated** block comment is a parse failure, not an
//! end-of-input: a script that does not lex cleanly is not a script this
//! module claims to understand.
//!
//! # Non-goals
//!
//! This is not a JavaScript parser and must never grow into one. It has no
//! expression grammar, no operators, no precedence, no statements, no
//! scoping. Growing those would be the first step toward posture C (a
//! sandboxed engine), which decision 009 **rejects outright** and standing
//! rule R57 prohibits. The correct response to "this legitimate script is
//! not recognised" is a disclosed false negative, never a bigger parser.

use std::fmt;

/// A literal argument value, the only kind this module accepts.
///
/// "Literal" is doing real work: every variant here has a value that is
/// fully determined by the source text. Nothing needs to be evaluated, so
/// non-execution is not a restriction pdfcer imposes on itself — it is simply
/// what reading these costs.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// A string literal's **decoded** bytes: quotes removed and escape
    /// sequences resolved. Kept as bytes rather than `String` because a
    /// field name in a PDF is not guaranteed to be UTF-8 and re-encoding it
    /// would break the lookup it exists for.
    Str(Vec<u8>),
    /// A numeric literal. JavaScript has one number type, so integers and
    /// reals both land here; a helper wanting an integer argument checks the
    /// value is integral itself.
    Num(f64),
    /// `true` or `false`.
    Bool(bool),
    /// `null`. Distinct from an absent argument: `AFNumber_Format(2, 0,
    /// null, …)` passed something, and a helper may treat that differently
    /// from a short argument list.
    Null,
    /// An array literal — either `[a, b]` or `new Array(a, b)`, which are
    /// the same thing here. Nested arrays are permitted by the grammar
    /// because refusing them would need a special case; no whitelisted
    /// helper takes one.
    Array(Vec<Literal>),
}

impl Literal {
    /// The literal as a string's bytes, or `None` for every other variant.
    ///
    /// Deliberately **not** a coercion. JavaScript would happily turn `2`
    /// into `"2"`; doing that here would let `AFSimple_Calculate(0, …)`
    /// match a helper expecting an operation name, which is exactly the
    /// loose matching this module exists to prevent.
    #[must_use]
    pub fn as_str(&self) -> Option<&[u8]> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The literal as a number, or `None` for every other variant.
    ///
    /// Also not a coercion, for the same reason: `"2"` is not `2` here even
    /// though it is in JavaScript. A helper argument that arrived as a
    /// string when the canonical generated call passes a number means the
    /// script was hand-edited, and a hand-edited script is `Custom`.
    #[must_use]
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// The literal as an integer, or `None` if it is not a number or not
    /// integral.
    ///
    /// `2.0` is accepted (JavaScript has no integer literal, so a generated
    /// call's `2` may lex as `2.0`); `2.5` is refused. An argument like a
    /// decimal count or a style enumerator has no meaning at a fractional
    /// value, and silently truncating one would invent a specification the
    /// helper does not have.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            // `fract() == 0` alone would accept values beyond i64's range,
            // where the cast is implementation-defined saturation rather
            // than the value the script named.
            Self::Num(n)
                if n.fract() == 0.0 && *n >= -(2.0_f64.powi(53)) && *n <= 2.0_f64.powi(53) =>
            {
                Some(*n as i64)
            }
            _ => None,
        }
    }

    /// The literal as a boolean, or `None` for every other variant.
    ///
    /// No truthiness. JavaScript's `1` is truthy; here it is simply not a
    /// boolean, because a generated call that passes `1` where the canonical
    /// form passes `true` was written by something other than Acrobat.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The literal's array elements, or `None` for every other variant.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Literal]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// One call expression with literal arguments — the whole of what a
/// recognisable script may be.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The callee's identifier, exactly as written. Not normalised: helper
    /// matching is **case-sensitive**, because JavaScript is, and
    /// `afnumber_format` is a different (nonexistent) function that would
    /// throw in Acrobat rather than format anything.
    pub name: String,
    /// The argument list, in source order.
    pub args: Vec<Literal>,
}

impl Call {
    /// The argument at `index`, or `None` if the call passed fewer.
    ///
    /// A missing trailing argument is a real case — `AFNumber_Format` is
    /// documented with six parameters but shorter generated calls exist — so
    /// this returns `None` rather than treating a short list as a failure.
    /// Whether a short list is acceptable is the individual helper's
    /// judgement, not this module's.
    #[must_use]
    pub fn arg(&self, index: usize) -> Option<&Literal> {
        self.args.get(index)
    }
}

impl fmt::Display for Call {
    /// A stable, locale-invariant rendering for disclosure lines and
    /// `--json` output. Not a round-trip of the source: it prints what pdfcer
    /// *understood*, which is the thing worth showing an operator asked to
    /// trust a recompute.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write_literal(f, a)?;
        }
        f.write_str(")")
    }
}

/// Render one literal for [`Call`]'s `Display`.
fn write_literal(f: &mut fmt::Formatter<'_>, lit: &Literal) -> fmt::Result {
    match lit {
        Literal::Str(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
        // `{}` on an f64 gives `2` for 2.0, which is what the source said.
        Literal::Num(n) => write!(f, "{n}"),
        Literal::Bool(b) => write!(f, "{b}"),
        Literal::Null => f.write_str("null"),
        Literal::Array(items) => {
            f.write_str("[")?;
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_literal(f, it)?;
            }
            f.write_str("]")
        }
    }
}

/// Recognise a script as exactly one call with literal arguments.
///
/// Returns `None` for **everything else**, with no distinction between
/// "malformed" and "too complex" — both mean the same thing to every caller:
/// treat this script as [`super::ScriptClass::Custom`], disclose it, and do
/// not recompute. Reporting *why* recognition failed would invite a caller
/// to act on the reason, and there is no reason that makes recomputing safe.
///
/// # Bounds
///
/// Nesting is capped at [`MAX_DEPTH`] and the input at [`MAX_LEN`]. A `/JS`
/// string is attacker-controlled content from an untrusted file, so the
/// recogniser is bounded like every other parser in this crate rather than
/// trusting the document to be reasonable.
#[must_use]
pub fn parse_single_call(js: &[u8]) -> Option<Call> {
    if js.len() > MAX_LEN {
        return None;
    }
    let mut p = Parser { s: js, i: 0 };
    p.skip_trivia()?;
    let name = p.identifier()?;
    p.skip_trivia()?;
    p.expect(b'(')?;
    let args = p.argument_list()?;
    p.skip_trivia()?;
    // A single optional terminator, then nothing but trivia. A second `;`
    // implies a second (empty) statement, which is harmless in JavaScript —
    // but accepting it starts the slide toward accepting statements, and the
    // canonical generated form has at most one.
    if p.peek() == Some(b';') {
        p.i += 1;
        p.skip_trivia()?;
    }
    if p.i != p.s.len() {
        return None;
    }
    Some(Call { name, args })
}

/// Maximum accepted `/JS` length for recognition.
///
/// Generated helper calls are tens of bytes; a large field list might reach a
/// few thousand. Anything past this is author code, and refusing to even lex
/// it bounds the work an adversarial document can demand.
pub const MAX_LEN: usize = 64 * 1024;

/// Maximum literal nesting depth (arrays within arrays).
///
/// No whitelisted helper takes a nested array at all, so `8` is already far
/// beyond generous; it exists to make the recursive descent's stack use
/// provably finite rather than to enable anything.
pub const MAX_DEPTH: usize = 8;

/// A byte-cursor over the script, with no state beyond the position.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    /// The byte at the cursor, or `None` at end of input.
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// Consume one expected byte, or fail.
    fn expect(&mut self, b: u8) -> Option<()> {
        if self.peek() == Some(b) {
            self.i += 1;
            Some(())
        } else {
            None
        }
    }

    /// Skip whitespace and comments.
    ///
    /// Returns `None` only for an **unterminated block comment**, which is a
    /// lexical error rather than a run of skippable trivia. Treating it as
    /// end-of-input would let `AFNumber_Format(2,0,0,0,"",true) /* ` parse
    /// as a clean call, and a script that does not lex is not one this
    /// module understands.
    fn skip_trivia(&mut self) -> Option<()> {
        loop {
            match self.peek() {
                // JavaScript whitespace and line terminators. The vertical
                // tab and form feed are included because the spec counts
                // them; a generated script will only ever use the first two.
                Some(b' ' | b'\t' | b'\r' | b'\n' | 0x0B | 0x0C) => self.i += 1,
                Some(b'/') => match self.s.get(self.i + 1) {
                    Some(b'/') => {
                        self.i += 2;
                        while !matches!(self.peek(), None | Some(b'\n' | b'\r')) {
                            self.i += 1;
                        }
                    }
                    Some(b'*') => {
                        self.i += 2;
                        loop {
                            match self.peek() {
                                None => return None,
                                Some(b'*') if self.s.get(self.i + 1) == Some(&b'/') => {
                                    self.i += 2;
                                    break;
                                }
                                Some(_) => self.i += 1,
                            }
                        }
                    }
                    // A lone `/` starts a division or a regex — either way,
                    // an expression. Stop; the caller will fail on it.
                    _ => return Some(()),
                },
                _ => return Some(()),
            }
        }
    }

    /// Read an ASCII identifier (`[A-Za-z_$][A-Za-z0-9_$]*`).
    ///
    /// Restricted to ASCII on purpose. JavaScript identifiers may contain a
    /// great deal of Unicode, but no Acrobat helper name does, and a
    /// non-ASCII identifier in a form script is a signal the script is not
    /// generated — so narrowing here removes a class of confusable-character
    /// matches for free.
    fn identifier(&mut self) -> Option<String> {
        let start = self.i;
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => self.i += 1,
            _ => return None,
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                self.i += 1;
            } else {
                break;
            }
        }
        // ASCII by construction, so this cannot fail; `ok()?` rather than an
        // unwrap keeps the function total.
        // `get` rather than a slice index: `start` and `self.i` are both
        // positions this function advanced, so the range is valid by
        // construction — but a total accessor costs nothing and keeps the
        // parser panic-free by inspection rather than by argument.
        self.s
            .get(start..self.i)
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
    }

    /// Read the argument list, the opening `(` already consumed.
    fn argument_list(&mut self) -> Option<Vec<Literal>> {
        self.list(b')', 0)
    }

    /// Read a comma-separated literal list up to `close`.
    ///
    /// Shared by call arguments, `[...]` and `new Array(...)` so all three
    /// accept exactly the same element grammar — a divergence between them
    /// would be a place where one spelling of a field list is recognised and
    /// another is not, for no reason an operator could predict.
    ///
    /// A trailing comma is **refused**. It is legal in modern JavaScript
    /// array literals but never generated, and `["A", "B",]` differs from
    /// `["A", "B", ,]` (a hole) by one character — a distinction not worth
    /// carrying when neither form is canonical.
    fn list(&mut self, close: u8, depth: usize) -> Option<Vec<Literal>> {
        let mut out = Vec::new();
        self.skip_trivia()?;
        if self.peek() == Some(close) {
            self.i += 1;
            return Some(out);
        }
        loop {
            self.skip_trivia()?;
            out.push(self.literal(depth)?);
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(c) if c == close => {
                    self.i += 1;
                    return Some(out);
                }
                _ => return None,
            }
        }
    }

    /// Read one literal.
    fn literal(&mut self, depth: usize) -> Option<Literal> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek()? {
            b'"' | b'\'' => self.string(),
            b'[' => {
                self.i += 1;
                Some(Literal::Array(self.list(b']', depth + 1)?))
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => self.number(),
            c if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                let word = self.identifier()?;
                match word.as_str() {
                    "true" => Some(Literal::Bool(true)),
                    "false" => Some(Literal::Bool(false)),
                    "null" => Some(Literal::Null),
                    // `new Array(...)` is the older spelling of an array
                    // literal and appears in real generated scripts, so it
                    // is accepted as one — but ONLY for `Array`. `new
                    // Date(...)` and friends construct a runtime object
                    // whose value pdfcer cannot know.
                    "new" => {
                        self.skip_trivia()?;
                        if self.identifier()? != "Array" {
                            return None;
                        }
                        self.skip_trivia()?;
                        self.expect(b'(')?;
                        Some(Literal::Array(self.list(b')', depth + 1)?))
                    }
                    // Any other bare word is an identifier: a runtime value.
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Read a quoted string, resolving escapes.
    ///
    /// Both quote styles are accepted because both are generated. The escape
    /// set is JavaScript's simple one plus `\xNN` and `\uNNNN`; anything
    /// else — a line continuation, an octal escape, an unterminated string —
    /// fails the whole parse rather than being passed through, because a
    /// field name that pdfcer decoded differently from Acrobat would look up
    /// the wrong field and compute a total from the wrong operands.
    fn string(&mut self) -> Option<Literal> {
        let quote = self.peek()?;
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                c if c == quote => return Some(Literal::Str(out)),
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'v' => out.push(0x0B),
                        b'0' => out.push(0),
                        b'\\' | b'\'' | b'"' | b'/' => out.push(e),
                        b'x' => {
                            let v = self.hex(2)?;
                            // A `\xNN` escape names a code point, not a
                            // byte. Encoding it as UTF-8 keeps the decoded
                            // bytes self-consistent: everything in `out` is
                            // then UTF-8 for any script whose literal source
                            // bytes were.
                            push_char(&mut out, v)?;
                        }
                        b'u' => {
                            let v = self.hex(4)?;
                            push_char(&mut out, v)?;
                        }
                        // Including a raw newline: a line continuation
                        // inside a string is legal JavaScript and never
                        // generated.
                        _ => return None,
                    }
                }
                // A raw line terminator inside a string is a syntax error in
                // JavaScript, so a script containing one would not run in
                // Acrobat either.
                b'\n' | b'\r' => return None,
                _ => out.push(c),
            }
        }
    }

    /// Read exactly `n` hex digits as a value.
    fn hex(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let c = self.peek()?;
            let d = (c as char).to_digit(16)?;
            v = v * 16 + d;
            self.i += 1;
        }
        Some(v)
    }

    /// Read a numeric literal.
    ///
    /// Decimal only: no hex (`0x`), octal, binary, or exponent-less oddities
    /// beyond what a generated call uses. An exponent IS accepted because a
    /// hand-typed `1e3` in an otherwise canonical call is unambiguous.
    /// `Infinity` and `NaN` are identifiers, not literals, and fail above.
    fn number(&mut self) -> Option<Literal> {
        let start = self.i;
        if matches!(self.peek(), Some(b'-' | b'+')) {
            self.i += 1;
        }
        let digits_start = self.i;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        // At least one digit somewhere in the mantissa; `.` and `-` alone
        // are not numbers.
        if self
            .s
            .get(digits_start..self.i)
            .is_none_or(|d| d.iter().all(|c| !c.is_ascii_digit()))
        {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'-' | b'+')) {
                self.i += 1;
            }
            let exp_start = self.i;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == exp_start {
                return None;
            }
        }
        // A digit or identifier character immediately after the number means
        // something this grammar does not model (`0x10`, `1abc`).
        if matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'$') {
            return None;
        }
        std::str::from_utf8(self.s.get(start..self.i)?)
            .ok()?
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Literal::Num)
    }
}

/// Append a Unicode scalar value's UTF-8 bytes, failing on a surrogate.
///
/// A lone surrogate from a `\uD800`-style escape has no UTF-8 encoding.
/// Substituting U+FFFD would silently change a field name; failing routes the
/// script to `Custom`, which is the safe direction.
fn push_char(out: &mut Vec<u8>, v: u32) -> Option<()> {
    let c = char::from_u32(v)?;
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    Some(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// Recognise the canonical generated calculate call, in both array
    /// spellings, and read the operand names out of it.
    #[test]
    fn the_canonical_calculate_call_is_recognised_in_both_array_spellings() {
        for src in [
            br#"AFSimple_Calculate("SUM", new Array("Item.1","Item.2"));"#.as_slice(),
            br#"AFSimple_Calculate("SUM", ["Item.1","Item.2"]);"#.as_slice(),
        ] {
            let call = parse_single_call(src).expect("canonical generated form");
            assert_eq!(call.name, "AFSimple_Calculate");
            assert_eq!(
                call.arg(0).and_then(Literal::as_str),
                Some(b"SUM".as_slice())
            );
            let names = call.arg(1).and_then(Literal::as_array).expect("field list");
            assert_eq!(names.len(), 2);
            assert_eq!(names[0].as_str(), Some(b"Item.1".as_slice()));
        }
    }

    /// The canonical generated format call, including the boolean and the
    /// empty-string arguments that make its shape distinctive.
    #[test]
    fn the_canonical_format_call_is_recognised() {
        let call =
            parse_single_call(br#"AFNumber_Format(2, 0, 0, 0, "", true);"#).expect("canonical");
        assert_eq!(call.name, "AFNumber_Format");
        assert_eq!(call.args.len(), 6);
        assert_eq!(call.arg(0).and_then(Literal::as_int), Some(2));
        assert_eq!(call.arg(4).and_then(Literal::as_str), Some(b"".as_slice()));
        assert_eq!(call.arg(5).and_then(Literal::as_bool), Some(true));
    }

    /// ★ **Everything that is not exactly one literal call is refused.**
    ///
    /// This is the module's whole safety argument, so it is asserted as a
    /// table rather than scattered across cases: each entry is a script that
    /// a looser recogniser might accept, and each would produce a wrong
    /// recompute if it were.
    #[test]
    fn anything_beyond_one_literal_call_is_refused() {
        let refused: &[(&[u8], &str)] = &[
            (
                b"x = 1; AFSimple_Calculate(\"SUM\", [\"A\"]);",
                "a second statement",
            ),
            (
                b"if (a) AFSimple_Calculate(\"SUM\", [\"A\"]);",
                "conditional",
            ),
            (
                b"AFSimple_Calculate(\"SUM\", flds);",
                "an identifier operand",
            ),
            (
                b"AFSimple_Calculate(\"SU\" + \"M\", [\"A\"]);",
                "concatenation",
            ),
            (
                b"event.value = AFSimple_Calculate(\"SUM\", [\"A\"]);",
                "assignment",
            ),
            (
                b"AFSimple_Calculate(\"SUM\", [\"A\"]) + 1",
                "a trailing operator",
            ),
            (
                b"function f() { AFNumber_Format(2,0,0,0,\"\",true); }",
                "a wrapper",
            ),
            (
                b"AFSimple_Calculate(\"SUM\", new Date());",
                "a non-Array constructor",
            ),
            (
                b"AFNumber_Format(2, 0, 0, 0, \"\", true) /* unterminated",
                "bad lex",
            ),
            (
                b"AFSimple_Calculate(\"SUM\", [\"A\",]);",
                "a trailing comma",
            ),
            (
                b"AFSimple_Calculate(\"SUM\", [\"A\"]); AFNumber_Format(0,0,0,0,\"\",true);",
                "two calls",
            ),
            (b"AFSimple_Calculate", "no call at all"),
            (b"", "empty"),
        ];
        for (src, why) in refused {
            assert!(
                parse_single_call(src).is_none(),
                "{why} must not be recognised: {}",
                String::from_utf8_lossy(src)
            );
        }
    }

    /// A near-miss NAME is refused by the caller, not here — but the parse
    /// must still read it faithfully, so the classifier can disclose what it
    /// actually saw rather than a guess.
    #[test]
    fn a_lookalike_name_parses_as_itself_and_is_not_normalised() {
        let call = parse_single_call(b"myAFNumber_Format(2);").expect("parses");
        assert_eq!(call.name, "myAFNumber_Format", "no prefix stripping");
        let call = parse_single_call(b"afnumber_format(2);").expect("parses");
        assert_eq!(
            call.name, "afnumber_format",
            "case is preserved for the matcher"
        );
    }

    /// Comments and whitespace are skipped wherever they may appear, so an
    /// operator's annotation of a generated script does not silently turn
    /// off the recompute.
    #[test]
    fn comments_and_whitespace_do_not_defeat_recognition() {
        let src = br#"
            // Acrobat generated
            AFSimple_Calculate (
                "SUM" /* op */ ,
                [ "A" , "B" ]
            ) ;
            // trailing note
        "#;
        let call = parse_single_call(src).expect("trivia is not structure");
        assert_eq!(call.name, "AFSimple_Calculate");
        assert_eq!(
            call.arg(1).and_then(Literal::as_array).map(<[_]>::len),
            Some(2)
        );
    }

    /// String escapes are resolved, because a field name is looked up by the
    /// bytes the script meant, not the bytes it was written with.
    #[test]
    fn string_escapes_resolve_to_the_name_the_script_meant() {
        let call = parse_single_call(br#"F("a\tb", "c\u00e9d", 'single');"#).expect("parses");
        assert_eq!(
            call.arg(0).and_then(Literal::as_str),
            Some(b"a\tb".as_slice())
        );
        assert_eq!(
            call.arg(1).and_then(Literal::as_str),
            Some("céd".as_bytes()),
            "\\u escapes encode as UTF-8"
        );
        assert_eq!(
            call.arg(2).and_then(Literal::as_str),
            Some(b"single".as_slice())
        );
    }

    /// ★ **No coercion.** JavaScript would equate several of these; this
    /// module does not, because a type mismatch against the canonical call
    /// means the script was edited, and an edited script is `Custom`.
    #[test]
    fn literal_accessors_do_not_coerce() {
        let call = parse_single_call(br#"F("2", 2, 2.5, true, null);"#).expect("parses");
        assert_eq!(
            call.arg(0).and_then(Literal::as_num),
            None,
            "\"2\" is not 2"
        );
        assert_eq!(
            call.arg(1).and_then(Literal::as_str),
            None,
            "2 is not \"2\""
        );
        assert_eq!(
            call.arg(1).and_then(Literal::as_int),
            Some(2),
            "2.0 is integral"
        );
        assert_eq!(
            call.arg(2).and_then(Literal::as_int),
            None,
            "2.5 is not an int"
        );
        assert_eq!(call.arg(3).and_then(Literal::as_num), None, "true is not 1");
        assert_eq!(
            call.arg(4).and_then(Literal::as_bool),
            None,
            "null is not false"
        );
    }

    /// Numeric forms that a generated call never uses are refused rather
    /// than half-read.
    #[test]
    fn only_plain_decimal_numbers_are_read() {
        for src in [
            b"F(0x10);".as_slice(),
            b"F(1abc);",
            b"F(.);",
            b"F(-);",
            b"F(Infinity);",
            b"F(NaN);",
            b"F(1e);",
        ] {
            assert!(
                parse_single_call(src).is_none(),
                "{} must not parse",
                String::from_utf8_lossy(src)
            );
        }
        assert_eq!(
            parse_single_call(b"F(-1.5e2);").and_then(|c| c.arg(0).and_then(Literal::as_num)),
            Some(-150.0),
            "but a signed decimal with an exponent is unambiguous"
        );
    }

    /// Bounds hold: an oversized script and an over-nested literal are both
    /// refused without recursing away the stack.
    #[test]
    fn the_recogniser_is_bounded_against_a_hostile_document() {
        let big = vec![b' '; MAX_LEN + 1];
        assert!(parse_single_call(&big).is_none(), "length is capped");

        let mut deep = b"F(".to_vec();
        deep.extend(std::iter::repeat_n(b'[', MAX_DEPTH + 2));
        deep.extend(std::iter::repeat_n(b']', MAX_DEPTH + 2));
        deep.extend_from_slice(b");");
        assert!(parse_single_call(&deep).is_none(), "depth is capped");
    }

    /// The `Display` form shows what pdfcer understood, which is what a
    /// disclosure line must say.
    #[test]
    fn display_shows_the_understood_call() {
        let call = parse_single_call(br#"AFSimple_Calculate("SUM", new Array("A","B"))"#).unwrap();
        assert_eq!(call.to_string(), r#"AFSimple_Calculate("SUM", ["A", "B"])"#);
    }
}
