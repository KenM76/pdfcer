//! # COS object parser (ISO 32000-1 §7.3 object grammar)
//!
//! Recursive-descent parser from the token stream (`crate::lexer`) to
//! the object model (`crate::object`). Spec sources:
//! `iso32000__s__7.3.md` (grammar), `iso32000__s__7.3.10.md` (indirect
//! objects, the `N G R` lookahead), `iso32000__s__7.3.8.md` (stream
//! framing) in the PDF-spec RAG. Clause numbers are ISO 32000-1:2008.
//!
//! ## What this layer does and doesn't
//!
//! [`Parser`] parses **one syntactic region** — a direct object, or one
//! complete `N G obj … endobj` indirect-object definition at a known
//! offset. It does NOT walk cross-reference tables (that's
//! `crate::xref`), does not resolve references (document layer), and
//! does not decode stream data (filter layer). The one place it needs
//! outside help is a stream's `/Length`, which may itself be an
//! indirect reference (§7.3.10 EXAMPLE 3 sanctions this for single-pass
//! writers) — the caller supplies a resolver callback for that case,
//! keeping the parser xref-agnostic.
//!
//! ## The `N G R` lookahead (§7.3.10)
//!
//! An indirect reference is *three* tokens — `Integer Integer Keyword(R)`
//! — indistinguishable from two array elements `1 0` followed by more
//! content until the third token is seen. The parser therefore keeps a
//! two-token peek buffer: on reading an `Integer`, it peeks up to two
//! tokens ahead and commits to a reference only when it sees
//! `Integer` + `R`. (This ambiguity is exactly why §7.8.2 bans indirect
//! references inside content streams.)
//!
//! ## Stream framing rules enforced (§7.3.8.1)
//!
//! - The `stream` keyword shall be followed by CRLF or LF alone —
//!   **CR alone is a hard error** (the spec's own NOTE 2 explains the
//!   ambiguity it would create). The RAG calls this "the single
//!   highest-value byte rule in the whole file-structure area."
//! - Data is exactly `/Length` bytes from the byte after that EOL.
//! - After the data: optional EOL, then `endstream`, then `endobj`.
//!   Anything else is a `/Length` inconsistency (§7.3.8.2 "an error") —
//!   fail-clean under the default [`StreamLengthPolicy::Strict`].
//!
//! ## The one strictness knob: `/Length` vs `endstream`
//!
//! The corpus-driven recovery heuristic the point above used to defer is
//! now here, as an explicit, opt-in [`StreamLengthPolicy`] rather than a
//! hidden default. Under [`StreamLengthPolicy::RecoverFromEndstream`] an
//! unusable `/Length` causes the data extent to be re-derived by scanning
//! for the `endstream` keyword — §7.3.8.2's own definition of `/Length` is
//! written in terms of that keyword, so it is the other half of the same
//! normative statement, not a guess. Only [`crate::recover`] and the
//! recovered branch of [`crate::document::Document::from_bytes`] select it;
//! the clean load path is byte-for-byte unchanged, which is what keeps the
//! round-trip invariant (`ARCHITECTURE.md` §5) safe. Every repair is
//! counted ([`Parser::stream_lengths_recovered`]) and disclosed.
//!
//! ## Guards (ARCHITECTURE.md §10 — pdfcer policy, not spec)
//!
//! [`MAX_NESTING_DEPTH`] bounds recursion (the spec bounds nothing
//! here; a `[[[[…` bomb must not overflow the stack).

use crate::lexer::{LexError, Lexer, Token, TokenKind, is_regular};
use crate::object::{Dict, IndirectObject, Name, ObjId, Object, Provenance, Stream};
use crate::span::ByteSpan;

/// Maximum container (array/dictionary) nesting depth.
///
/// pdfcer policy (ARCHITECTURE.md §10): ISO 32000-1 does not bound
/// object nesting (Annex C bounds only `q`/`Q` nesting), so the guard
/// value is ours. 256 is far beyond any legitimate document structure
/// while keeping worst-case recursion shallow enough for any thread's
/// stack.
pub const MAX_NESTING_DEPTH: usize = 256;

/// A structural parse error: what went wrong and where.
///
/// C-GOOD-ERR via `thiserror`; offsets are absolute buffer offsets, the
/// same coordinate system as [`ByteSpan`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("parse error at byte {offset}: {kind}")]
pub struct ParseError {
    /// Byte offset where the problem was detected.
    pub offset: usize,
    /// What was wrong.
    pub kind: ParseErrorKind,
}

/// Classification of structural parse errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// The lexer failed underneath the parser.
    #[error("lexical error: {0}")]
    Lex(#[from] LexError),
    /// Input ended where an object (or more of one) was required.
    #[error("unexpected end of input")]
    UnexpectedEof,
    /// A token that cannot begin/continue the construct being parsed.
    /// The payload is a static description of what was expected.
    #[error("unexpected token; expected {0}")]
    Unexpected(&'static str),
    /// Dictionary key position held a non-name object (§7.3.7 "key
    /// shall be a name").
    #[error("dictionary key is not a name")]
    DictKeyNotName,
    /// The same key appeared twice in one dictionary (§7.3.7 "shall
    /// not"; reader behaviour undefined — Pass 1 is strict).
    #[error("duplicate dictionary key")]
    DuplicateDictKey,
    /// Container nesting exceeded [`MAX_NESTING_DEPTH`] (pdfcer guard).
    #[error("nesting exceeds MAX_NESTING_DEPTH ({MAX_NESTING_DEPTH})")]
    DepthExceeded,
    /// An indirect-object header wasn't `N G obj` with valid numbers
    /// (N positive, G in 0..=65535 — §7.3.10/§7.5.4 ranges).
    #[error("malformed indirect-object header")]
    BadObjectHeader,
    /// `endobj` missing after the object body.
    #[error("missing endobj")]
    MissingEndobj,
    /// `stream` keyword not directly preceded by a dictionary
    /// (§7.3.8.1 — a stream is a dictionary plus data).
    #[error("stream keyword without preceding dictionary")]
    StreamWithoutDict,
    /// The byte(s) after the `stream` keyword violate §7.3.8.1: must
    /// be CRLF or LF alone, never CR alone, never anything else.
    #[error("stream keyword not followed by CRLF or LF (CR alone is forbidden by \u{a7}7.3.8.1)")]
    BadStreamEol,
    /// `/Length` missing, non-integer, negative, or (when indirect)
    /// unresolvable via the caller's resolver.
    #[error("stream /Length missing, invalid, or unresolvable")]
    BadStreamLength,
    /// The `/Length`-delimited data region was not followed by
    /// (optional EOL +) `endstream` — the file's `/Length` is
    /// inconsistent (§7.3.8.2).
    #[error("endstream not found where /Length points (stream extent inconsistent)")]
    StreamExtentMismatch,
}

impl ParseError {
    const fn new(offset: usize, kind: ParseErrorKind) -> Self {
        Self { offset, kind }
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        Self {
            offset: e.offset,
            kind: ParseErrorKind::Lex(e),
        }
    }
}

/// Resolver for indirect `/Length` values (§7.3.10 EXAMPLE 3 pattern).
///
/// Given the referenced id, return the integer value of that object, or
/// `None` if it cannot be resolved / is not an integer. The document
/// layer implements this against the xref table; tests implement it
/// with a closure over a map. Kept as a trait alias-like type for
/// signature clarity.
pub type LengthResolver<'r> = &'r mut dyn FnMut(ObjId) -> Option<i64>;

/// How a stream whose `/Length` disagrees with the file should be handled.
///
/// This is the one deliberate strictness knob on the parser, and it exists
/// because §7.3.8.2's own definition of `/Length` is phrased in terms of
/// the `endstream` keyword: the value "shall be the number of bytes from
/// the beginning of the line following the keyword `stream` to the last
/// byte just before the keyword `endstream`". When the stored number and
/// the keyword disagree, the file contains **two** statements of the same
/// fact and one of them is wrong. Which one a reader believes is a policy
/// choice, not a spec choice — so pdfcer makes it an explicit parameter
/// rather than a hidden default.
///
/// **The default is and must stay [`StreamLengthPolicy::Strict`]**: on a
/// cleanly-loading file the stored `/Length` is authoritative, a
/// disagreement is real damage the operator should hear about (§7.3.8.2
/// calls it "an error"), and silently re-deriving extents would put a
/// guessed span into the writer's byte-identical re-emission path and
/// break the round-trip/minimal-diff invariant (`ARCHITECTURE.md` §5).
///
/// [`StreamLengthPolicy::RecoverFromEndstream`] is reachable **only** from
/// the rebuild-by-scan recovery path ([`crate::recover`] and the recovered
/// branch of [`crate::document::Document::from_bytes`]), where the file's
/// cross-reference machinery has already been proven unparseable, no
/// byte-identical re-emission is on the table (a recovered document
/// refuses incremental save and always normalizes), and refusing an object
/// costs the operator the whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StreamLengthPolicy {
    /// Believe `/Length`. A disagreement with `endstream` is
    /// [`ParseErrorKind::StreamExtentMismatch`]; a missing/invalid
    /// `/Length` is [`ParseErrorKind::BadStreamLength`]. The default, and
    /// the only policy the clean load path ever uses.
    #[default]
    Strict,
    /// Believe `endstream`. When (and only when) the stored `/Length`
    /// cannot be used — absent, non-integer, unresolvable, past the end of
    /// the buffer, or simply not landing on `endstream` — re-derive the
    /// data extent by scanning forward from the start of the data for the
    /// `endstream` keyword, then backing off the one end-of-line marker
    /// §7.3.8.1 says "should" precede it. Every such repair is counted in
    /// [`Parser::stream_lengths_recovered`] so it can be disclosed (R20,
    /// fuzzy-never-sneaky) rather than silently absorbed.
    RecoverFromEndstream,
}

/// What to do when an indirect object's `endobj` keyword is missing.
///
/// §7.3.10 requires a definition to be `N G obj … endobj`, so a body that
/// runs straight into the next `N G obj` header is malformed. But the
/// object *itself* may have parsed perfectly — the damage is a missing
/// four-byte keyword, not a corrupt value — and refusing it costs the
/// operator the whole object.
///
/// **The default is and must stay [`TerminatorPolicy::Strict`]**, for the
/// same reason [`StreamLengthPolicy::Strict`] is: on a cleanly-loading
/// file a missing `endobj` is real damage the operator should hear about,
/// and accepting it would put an inferred extent into the writer's
/// byte-identical re-emission path and break the round-trip/minimal-diff
/// invariant (`ARCHITECTURE.md` §5).
///
/// # Why the lenient policy exists
///
/// It was added 2026-08-07 after the veraPDF parse gate found pdfcer
/// writing a document whose catalog said `/Pages 2 0 R` while object 2
/// was **absent from the file**. The input (qpdf's `bad6.pdf`) omits
/// exactly one `endobj` — the one after the `/Pages` node — so
/// `confirm_candidates` dropped the object as unparseable and the writer
/// emitted a catalog pointing at nothing. veraPDF could recover the
/// original and could not recover pdfcer's rewrite of it, which is a
/// document made strictly worse by passing through pdfcer.
///
/// Like [`StreamLengthPolicy::RecoverFromEndstream`], this is reachable
/// **only** from the rebuild-by-scan recovery path, where the file's
/// cross-reference machinery has already been proven unparseable and no
/// byte-identical re-emission is on the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TerminatorPolicy {
    /// A missing `endobj` is [`ParseErrorKind::MissingEndobj`]. The
    /// default, and the only policy the clean load path ever uses.
    #[default]
    Strict,
    /// Accept a definition whose body parsed cleanly but whose `endobj` is
    /// missing, **provided the terminator looks like the start of the next
    /// object header** (an integer) or the buffer ended.
    ///
    /// The integer guard is what keeps this from swallowing arbitrary
    /// garbage: `2 0 obj << … >> 3 0 obj` recovers, while a body followed
    /// by an unexpected keyword still fails. Every such repair is counted
    /// in [`Parser::missing_endobj_recovered`] so it can be disclosed
    /// (R20, fuzzy-never-sneaky) rather than silently absorbed.
    RecoverAtNextHeader,
}

/// Recursive-descent parser over a byte buffer.
///
/// Create positioned at an offset ([`Parser::at`]) and call one of the
/// top-level entry points ([`Parser::parse_object`],
/// [`Parser::parse_indirect_object`]). The parser owns a lexer plus a
/// two-token peek buffer (see module docs on the `N G R` lookahead).
#[derive(Debug)]
pub struct Parser<'a> {
    buf: &'a [u8],
    lexer: Lexer<'a>,
    /// Peeked-but-unconsumed tokens, oldest first (max 2 in practice).
    peeked: Vec<Token>,
    /// Strictness for the `/Length`-vs-`endstream` disagreement. Strict
    /// unless a caller opted in via [`Parser::with_stream_length_policy`].
    stream_length_policy: StreamLengthPolicy,
    /// How many streams this parser re-derived an extent for. Only ever
    /// non-zero under [`StreamLengthPolicy::RecoverFromEndstream`].
    stream_lengths_recovered: usize,
    /// Strictness for a missing `endobj`. Strict unless a caller opted in
    /// via [`Parser::with_terminator_policy`].
    terminator_policy: TerminatorPolicy,
    /// How many definitions this parser accepted without an `endobj`. Only
    /// ever non-zero under [`TerminatorPolicy::RecoverAtNextHeader`].
    missing_endobj_recovered: usize,
}

impl<'a> Parser<'a> {
    /// Parser over `buf` starting at absolute offset `pos`.
    #[must_use]
    pub const fn at(buf: &'a [u8], pos: usize) -> Self {
        Self {
            buf,
            lexer: Lexer::at(buf, pos),
            peeked: Vec::new(),
            stream_length_policy: StreamLengthPolicy::Strict,
            stream_lengths_recovered: 0,
            terminator_policy: TerminatorPolicy::Strict,
            missing_endobj_recovered: 0,
        }
    }

    /// Set the `/Length`-disagreement policy (builder form).
    ///
    /// See [`StreamLengthPolicy`] for why the non-default value is
    /// restricted to the recovery path.
    #[must_use]
    pub const fn with_stream_length_policy(mut self, policy: StreamLengthPolicy) -> Self {
        self.stream_length_policy = policy;
        self
    }

    /// How many stream extents this parser re-derived from `endstream`
    /// because the stored `/Length` was unusable.
    ///
    /// Always `0` under [`StreamLengthPolicy::Strict`] (that policy errors
    /// instead of repairing), so a non-zero value is proof that the file
    /// disagreed with itself. Callers propagate it into the counted
    /// disclosure a front end shows the operator.
    #[must_use]
    pub const fn stream_lengths_recovered(&self) -> usize {
        self.stream_lengths_recovered
    }

    /// Set the missing-`endobj` policy (builder form).
    ///
    /// See [`TerminatorPolicy`] for why the non-default value is
    /// restricted to the recovery path.
    #[must_use]
    pub const fn with_terminator_policy(mut self, policy: TerminatorPolicy) -> Self {
        self.terminator_policy = policy;
        self
    }

    /// How many definitions this parser accepted with no `endobj`.
    ///
    /// Always `0` under [`TerminatorPolicy::Strict`], so a non-zero value
    /// is proof the file omitted a required keyword. Callers propagate it
    /// into the counted disclosure a front end shows the operator.
    #[must_use]
    pub const fn missing_endobj_recovered(&self) -> usize {
        self.missing_endobj_recovered
    }

    /// Current absolute offset for diagnostics: the start of the oldest
    /// unconsumed peeked token, or the lexer cursor.
    fn offset(&self) -> usize {
        self.peeked
            .first()
            .map_or_else(|| self.lexer.pos(), |t| t.span.start)
    }

    /// Next token, consuming.
    fn next(&mut self) -> Result<Option<Token>, ParseError> {
        if self.peeked.is_empty() {
            Ok(self.lexer.next_token()?)
        } else {
            Ok(Some(self.peeked.remove(0)))
        }
    }

    /// Peek the `n`-th upcoming token (0-based) without consuming.
    fn peek(&mut self, n: usize) -> Result<Option<&Token>, ParseError> {
        while self.peeked.len() <= n {
            match self.lexer.next_token()? {
                Some(t) => self.peeked.push(t),
                None => break,
            }
        }
        Ok(self.peeked.get(n))
    }

    /// Next token or `UnexpectedEof`.
    fn expect_any(&mut self) -> Result<Token, ParseError> {
        let off = self.offset();
        self.next()?
            .ok_or(ParseError::new(off, ParseErrorKind::UnexpectedEof))
    }

    /// Is `tok` the keyword whose lexeme is `word`? (Keywords carry no
    /// copied bytes — the span against the buffer is the value; see
    /// `lexer::TokenKind::Keyword`.)
    fn is_keyword(&self, tok: &Token, word: &[u8]) -> bool {
        matches!(tok.kind, TokenKind::Keyword) && tok.lexeme(self.buf) == Some(word)
    }

    // -----------------------------------------------------------------------
    // Direct objects
    // -----------------------------------------------------------------------

    /// Parse one direct object (any §7.3 value, including `N G R`
    /// references). This is the entry point for trailer dictionaries
    /// and other bare-value positions.
    ///
    /// # Errors
    ///
    /// [`ParseError`] on malformed syntax — see [`ParseErrorKind`].
    pub fn parse_object(&mut self) -> Result<Object, ParseError> {
        self.parse_value(0)
    }

    /// Core value parser. `depth` counts container nesting for the
    /// [`MAX_NESTING_DEPTH`] guard.
    fn parse_value(&mut self, depth: usize) -> Result<Object, ParseError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(ParseError::new(
                self.offset(),
                ParseErrorKind::DepthExceeded,
            ));
        }
        let tok = self.expect_any()?;
        match tok.kind {
            TokenKind::Integer(v) => self.maybe_reference(v, &tok),
            TokenKind::Real(v) => Ok(Object::Real(v)),
            TokenKind::String(s) => Ok(Object::String(s)),
            TokenKind::Name(n) => Ok(Object::Name(Name(n))),
            TokenKind::ArrayOpen => self.parse_array_body(depth),
            TokenKind::DictOpen => Ok(Object::Dict(self.parse_dict_body(depth)?)),
            TokenKind::Keyword => {
                let lexeme = tok.lexeme(self.buf).unwrap_or(&[]);
                match lexeme {
                    b"true" => Ok(Object::Boolean(true)),
                    b"false" => Ok(Object::Boolean(false)),
                    b"null" => Ok(Object::Null),
                    _ => Err(ParseError::new(
                        tok.span.start,
                        ParseErrorKind::Unexpected("an object"),
                    )),
                }
            }
            // Closers/braces at value position are structural errors at
            // this level; the container parsers consume their own
            // closers before recursing.
            _ => Err(ParseError::new(
                tok.span.start,
                ParseErrorKind::Unexpected("an object"),
            )),
        }
    }

    /// After an `Integer`, decide reference vs plain integer via the
    /// two-token lookahead (module docs): `Integer Integer R` → a
    /// [`Object::Reference`], anything else → the integer stands alone.
    ///
    /// The reference's numbers must be in range (§7.3.10: object number
    /// positive; §7.5.4: generation ≤ 65,535) — out-of-range values
    /// mean the three tokens were NOT a reference after all, and since
    /// bare keyword `R` can't otherwise appear at value position, that
    /// is a structural error surfaced when the parser reaches it.
    fn maybe_reference(&mut self, value: i64, tok: &Token) -> Result<Object, ParseError> {
        // Copy out (kind-class, span) pairs so no peek borrow is held
        // while the buffer is sliced for the `R` check.
        let t1_is_int = self
            .peek(0)?
            .is_some_and(|t| matches!(t.kind, TokenKind::Integer(_)));
        let looks_like_ref = t1_is_int && {
            let t2 = self
                .peek(1)?
                .map(|t| (matches!(t.kind, TokenKind::Keyword), t.span));
            match t2 {
                Some((true, span)) => span.slice(self.buf) == Some(b"R"),
                _ => false,
            }
        };
        if !looks_like_ref {
            return Ok(Object::Integer(value));
        }
        // Commit: consume `gen` and `R`.
        let gen_tok = self.expect_any()?;
        let TokenKind::Integer(gen_value) = gen_tok.kind else {
            // Unreachable by construction of the lookahead; kept as a
            // structured error per the panic-free policy.
            return Err(ParseError::new(
                gen_tok.span.start,
                ParseErrorKind::Unexpected("generation number"),
            ));
        };
        self.expect_any()?; // the `R`, verified by the lookahead

        let (Ok(num), Ok(generation)) = (u32::try_from(value), u16::try_from(gen_value)) else {
            return Err(ParseError::new(
                tok.span.start,
                ParseErrorKind::Unexpected("reference numbers in range"),
            ));
        };
        if num == 0 {
            // Object number 0 is reserved for the free-list head
            // (§7.5.4); it never identifies a real object.
            return Err(ParseError::new(
                tok.span.start,
                ParseErrorKind::Unexpected("positive object number"),
            ));
        }
        Ok(Object::Reference(ObjId::new(num, generation)))
    }

    /// Parse array elements after `[`, through the matching `]`.
    fn parse_array_body(&mut self, depth: usize) -> Result<Object, ParseError> {
        let mut items = Vec::new();
        loop {
            match self.peek(0)? {
                None => {
                    return Err(ParseError::new(
                        self.offset(),
                        ParseErrorKind::UnexpectedEof,
                    ));
                }
                Some(t) if matches!(t.kind, TokenKind::ArrayClose) => {
                    self.next()?;
                    return Ok(Object::Array(items));
                }
                Some(_) => items.push(self.parse_value(depth + 1)?),
            }
        }
    }

    /// Parse dictionary entries after `<<`, through the matching `>>`.
    ///
    /// Enforces §7.3.7: keys are names; duplicate keys are malformed
    /// (spec: "shall not have the same key"; reader behaviour
    /// undefined → Pass 1 strict, real-world tolerance only with
    /// corpus evidence, per the module's failure philosophy).
    fn parse_dict_body(&mut self, depth: usize) -> Result<Dict, ParseError> {
        let mut dict = Dict::new();
        loop {
            let tok = self.expect_any()?;
            match tok.kind {
                TokenKind::DictClose => return Ok(dict),
                TokenKind::Name(key) => {
                    if dict.0.iter().any(|(k, _)| k.as_bytes() == key) {
                        return Err(ParseError::new(
                            tok.span.start,
                            ParseErrorKind::DuplicateDictKey,
                        ));
                    }
                    let value = self.parse_value(depth + 1)?;
                    dict.0.push((Name(key), value));
                }
                _ => {
                    return Err(ParseError::new(
                        tok.span.start,
                        ParseErrorKind::DictKeyNotName,
                    ));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Indirect objects (§7.3.10) + stream bodies (§7.3.8)
    // -----------------------------------------------------------------------

    /// Parse one complete indirect-object definition
    /// (`N G obj <value> endobj`, or the stream form) starting at this
    /// parser's position — normally an offset taken from the xref
    /// table.
    ///
    /// `resolve_length` is consulted only when a stream's `/Length` is
    /// an indirect reference (§7.3.10 EXAMPLE 3); pass a closure over
    /// the xref/document (or `&mut |_| None` where indirect lengths
    /// are illegal, e.g. xref-stream bootstrap — §7.5.8.2's directness
    /// rules).
    ///
    /// # Errors
    ///
    /// [`ParseError`] on malformed structure — see [`ParseErrorKind`].
    pub fn parse_indirect_object(
        &mut self,
        resolve_length: LengthResolver<'_>,
    ) -> Result<IndirectObject, ParseError> {
        // --- header: N G obj ---
        let num_tok = self.expect_any()?;
        let TokenKind::Integer(num) = num_tok.kind else {
            return Err(ParseError::new(
                num_tok.span.start,
                ParseErrorKind::BadObjectHeader,
            ));
        };
        let gen_tok = self.expect_any()?;
        let TokenKind::Integer(gen_value) = gen_tok.kind else {
            return Err(ParseError::new(
                gen_tok.span.start,
                ParseErrorKind::BadObjectHeader,
            ));
        };
        let obj_tok = self.expect_any()?;
        if !self.is_keyword(&obj_tok, b"obj") {
            return Err(ParseError::new(
                obj_tok.span.start,
                ParseErrorKind::BadObjectHeader,
            ));
        }
        let (Ok(num), Ok(generation)) = (u32::try_from(num), u16::try_from(gen_value)) else {
            return Err(ParseError::new(
                num_tok.span.start,
                ParseErrorKind::BadObjectHeader,
            ));
        };
        if num == 0 {
            return Err(ParseError::new(
                num_tok.span.start,
                ParseErrorKind::BadObjectHeader,
            ));
        }
        let id = ObjId::new(num, generation);

        // --- body ---
        let value = self.parse_value(0)?;

        // --- terminator: endobj, or stream … endstream endobj ---
        let term = self.expect_any()?;
        if self.is_keyword(&term, b"endobj") {
            return Ok(IndirectObject {
                id,
                value,
                provenance: Provenance::File(ByteSpan::from_range(
                    num_tok.span.start..term.span.end(),
                )),
            });
        }
        if self.is_keyword(&term, b"stream") {
            let Object::Dict(dict) = value else {
                return Err(ParseError::new(
                    term.span.start,
                    ParseErrorKind::StreamWithoutDict,
                ));
            };
            let before = self.stream_lengths_recovered;
            let stream = self.parse_stream_tail(dict, &term, resolve_length)?;
            let end_tok = self.expect_any()?;
            if !self.is_keyword(&end_tok, b"endobj") {
                return Err(ParseError::new(
                    end_tok.span.start,
                    ParseErrorKind::MissingEndobj,
                ));
            }
            let span = ByteSpan::from_range(num_tok.span.start..end_tok.span.end());
            // If this object's extent had to be re-derived, its source
            // bytes now contradict its parsed value: the bytes still carry
            // the old `/Length` while the value carries the recovered one.
            // Mark the provenance so the writer re-serializes rather than
            // copying the contradiction into a saved file (which would
            // produce a document pdfcer could not reload). See
            // `Provenance::RecoveredFile`.
            let provenance = if self.stream_lengths_recovered > before {
                Provenance::RecoveredFile(span)
            } else {
                Provenance::File(span)
            };
            return Ok(IndirectObject {
                id,
                value: Object::Stream(stream),
                provenance,
            });
        }
        // --- no terminator: policy decides -------------------------------
        //
        // Under RecoverAtNextHeader, a body that parsed cleanly and is
        // followed by what looks like the next object header is accepted.
        // The integer guard matters: `2 0 obj << … >> 3 0 obj` recovers,
        // while a body followed by an unexpected keyword still fails, so
        // this cannot swallow arbitrary trailing garbage.
        //
        // The provenance is RecoveredFile, NOT File, and that is
        // load-bearing rather than cosmetic. These source bytes do not
        // contain an `endobj`, so re-emitting them verbatim would copy the
        // malformation into the saved file and produce a document pdfcer
        // could not reload — the same trap the stream-length repair above
        // documents. RecoveredFile forces the writer to re-serialize.
        if self.terminator_policy == TerminatorPolicy::RecoverAtNextHeader
            && matches!(term.kind, TokenKind::Integer(_))
        {
            self.missing_endobj_recovered += 1;
            return Ok(IndirectObject {
                id,
                value,
                // Ends where the value ended — `term` is the NEXT object's
                // first token and must not be claimed by this one.
                provenance: Provenance::RecoveredFile(ByteSpan::from_range(
                    num_tok.span.start..term.span.start,
                )),
            });
        }
        Err(ParseError::new(
            term.span.start,
            ParseErrorKind::MissingEndobj,
        ))
    }

    /// Handle everything after a `stream` keyword: the §7.3.8.1 EOL
    /// rule, the `/Length`-delimited data span, and the `endstream`
    /// keyword. Returns with the parser positioned after `endstream`.
    fn parse_stream_tail(
        &mut self,
        dict: Dict,
        stream_kw: &Token,
        resolve_length: LengthResolver<'_>,
    ) -> Result<Stream, ParseError> {
        // The peek buffer must be empty here: `stream` was just
        // consumed and stream *data* must not be lexed. (It is empty by
        // construction — parse_value never leaves lookahead beyond the
        // value it returned, and the `stream` token itself was taken
        // with expect_any. debug_assert documents the reasoning.)
        debug_assert!(self.peeked.is_empty());

        // §7.3.8.1: `stream` followed by CRLF or LF alone; CR alone is
        // FORBIDDEN. Data begins at the byte after that EOL.
        let after_kw = stream_kw.span.end();
        let data_start = match (self.buf.get(after_kw), self.buf.get(after_kw + 1)) {
            (Some(b'\r'), Some(b'\n')) => after_kw + 2,
            (Some(b'\n'), _) => after_kw + 1,
            _ => {
                return Err(ParseError::new(after_kw, ParseErrorKind::BadStreamEol));
            }
        };

        // /Length: required, integer, non-negative; possibly indirect.
        // Under RecoverFromEndstream an unusable /Length is not fatal — it
        // simply means the keyword is the only surviving statement of the
        // extent, so fall straight through to the scan.
        let stored_length: Option<usize> = match dict.get(b"Length") {
            Some(Object::Integer(v)) => usize::try_from(*v).ok(),
            Some(Object::Reference(id)) => {
                resolve_length(*id).and_then(|v| usize::try_from(v).ok())
            }
            _ => None,
        };
        let length = match stored_length {
            Some(length) => length,
            None => {
                return self.recover_stream_extent(
                    dict,
                    data_start,
                    ParseError::new(stream_kw.span.start, ParseErrorKind::BadStreamLength),
                );
            }
        };

        let data_end = data_start.saturating_add(length);
        if data_end > self.buf.len() {
            return self.recover_stream_extent(
                dict,
                data_start,
                ParseError::new(data_start, ParseErrorKind::StreamExtentMismatch),
            );
        }

        // After the data: optional EOL ("should", not counted in
        // Length), then `endstream`. Re-enter token scanning there —
        // the lexer's whitespace skipping absorbs the optional EOL.
        self.lexer = Lexer::at(self.buf, data_end);
        let end_tok = self.expect_any().map_err(|e| match e.kind {
            // EOF right after the data reads better as an extent error.
            ParseErrorKind::UnexpectedEof => {
                ParseError::new(data_end, ParseErrorKind::StreamExtentMismatch)
            }
            _ => e,
        })?;
        if !self.is_keyword(&end_tok, b"endstream") {
            return self.recover_stream_extent(
                dict,
                data_start,
                ParseError::new(end_tok.span.start, ParseErrorKind::StreamExtentMismatch),
            );
        }

        Ok(Stream {
            dict,
            data_span: ByteSpan::from_range(data_start..data_end),
        })
    }

    /// The `/Length`-disagreement fork: under [`StreamLengthPolicy::Strict`]
    /// re-raise `strict_err` unchanged; under
    /// [`StreamLengthPolicy::RecoverFromEndstream`] re-derive the data
    /// extent from the `endstream` keyword.
    ///
    /// ## Why the keyword is a legitimate authority here
    ///
    /// §7.3.8.2 Table 5 defines `/Length` *as* "the number of bytes from
    /// the beginning of the line following the keyword `stream` to the last
    /// byte just before the keyword `endstream`". The keyword is therefore
    /// not a heuristic landmark invented by this function — it is the other
    /// half of the spec's own definition. When the two halves disagree, the
    /// keyword is the one that is still physically present in the bytes,
    /// which is why every mature reader (qpdf, pdfium, poppler, pdf.js)
    /// prefers it during damage recovery.
    ///
    /// ## Why the scan starts at `data_start`, not at the stored end
    ///
    /// The stored length can be wrong in either direction. Too short (the
    /// dominant real-world case: a file whose `/Length` values were
    /// computed for LF line endings and then converted to CRLF, so every
    /// stream grew by one byte per line) leaves `endstream` *after* the
    /// stored end; too long leaves it *before*. Scanning forward from the
    /// first data byte finds the first terminator in both directions, so a
    /// single rule covers both. The cost is that a stream whose binary data
    /// happens to contain the literal bytes `endstream` is truncated at
    /// that point — acceptable only because this path is unreachable except
    /// on a file whose cross-reference machinery has already failed, where
    /// the alternative outcome is not "correct data" but "no document".
    ///
    /// ## The EOL back-off
    ///
    /// §7.3.8.1 says an EOL marker "should" precede `endstream` and is not
    /// counted in `/Length`. One such marker (`\r\n`, `\n`, or a lone `\r`)
    /// is therefore removed from the end of the derived span. Exactly one —
    /// removing more would eat real data from a stream that legitimately
    /// ends in blank lines.
    ///
    /// ## `/Length` is rewritten to match
    ///
    /// The returned [`Stream`]'s dictionary carries the **derived** length,
    /// not the file's stale one, so the dictionary and the data span never
    /// disagree. See the inline commentary at the rewrite for why this is a
    /// correctness requirement rather than tidiness.
    ///
    /// # Errors
    ///
    /// `strict_err` verbatim under `Strict`; the same error under
    /// `RecoverFromEndstream` when no `endstream` keyword follows the data
    /// at all (nothing to recover *from*, so the refusal stands — recovery
    /// never invents an extent out of nothing).
    fn recover_stream_extent(
        &mut self,
        dict: Dict,
        data_start: usize,
        strict_err: ParseError,
    ) -> Result<Stream, ParseError> {
        if self.stream_length_policy != StreamLengthPolicy::RecoverFromEndstream {
            return Err(strict_err);
        }
        let Some(kw_at) = find_keyword(self.buf, data_start, b"endstream") else {
            return Err(strict_err);
        };
        // Back off exactly one EOL marker (§7.3.8.1: "should" be there, and
        // it is not part of the data).
        let mut data_end = kw_at;
        if data_end > data_start && self.buf.get(data_end - 1) == Some(&b'\n') {
            data_end -= 1;
        }
        if data_end > data_start && self.buf.get(data_end - 1) == Some(&b'\r') {
            data_end -= 1;
        }

        // Resume token scanning at the keyword so the caller's `endobj`
        // expectation is evaluated exactly as on the clean path.
        self.lexer = Lexer::at(self.buf, kw_at);
        self.peeked.clear();
        let end_tok = self.expect_any()?;
        if !self.is_keyword(&end_tok, b"endstream") {
            return Err(strict_err);
        }

        // Rewrite `/Length` to the extent actually found.
        //
        // This is not cosmetic — it is required for correctness, and the
        // round-trip harness is what proves it. `Stream` carries the
        // dictionary and the data span as two statements of one fact, and
        // the rest of the crate reads them independently: the filter layer
        // decodes `data_span`, while the writer re-emits `dict` verbatim.
        // Leaving the stale number in the dictionary next to a repaired
        // span would make pdfcer *emit* the very inconsistency it just
        // recovered from — a saved file whose `/Length` under-runs its own
        // data, i.e. a file pdfcer itself would then refuse to reload.
        //
        // §5.6's "never normalize" is not in tension here: it governs clean
        // passthrough, and this branch is unreachable except on a document
        // whose cross-reference machinery already failed, which always
        // saves as a full rewrite. Correcting a provably-wrong length is
        // the only way such a rewrite can be a valid PDF at all.
        //
        // If `/Length` was an indirect reference (§7.3.10 EXAMPLE 3), it is
        // replaced by a direct integer and the length object is simply left
        // unreferenced — harmless, and more honest than updating a separate
        // object the writer may not re-emit.
        let mut dict = dict;
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data_end - data_start).unwrap_or(0)),
        );

        self.stream_lengths_recovered += 1;
        Ok(Stream {
            dict,
            data_span: ByteSpan::from_range(data_start..data_end),
        })
    }
}

/// First occurrence of `needle` in `buf` at or after `from` that stands as
/// its own token — i.e. the byte after it is not a regular character, so
/// `endstreamx` never matches.
///
/// The byte *before* is deliberately NOT constrained: a stream whose
/// declared length was too long, or whose final EOL is missing, can put
/// `endstream` flush against the last data byte, and refusing that shape
/// would defeat the recovery this helper exists for.
fn find_keyword(buf: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let hay = buf.get(from..)?;
    let n = needle.len();
    let mut base = 0usize;
    while let Some(rel) = hay.get(base..)?.windows(n).position(|w| w == needle) {
        let at = base + rel;
        let after_ok = !hay.get(at + n).is_some_and(|&b| is_regular(b));
        if after_ok {
            return Some(from + at);
        }
        base = at + 1;
    }
    None
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

    fn parse(input: &[u8]) -> Object {
        Parser::at(input, 0).parse_object().unwrap()
    }

    fn parse_err(input: &[u8]) -> ParseErrorKind {
        Parser::at(input, 0).parse_object().unwrap_err().kind
    }

    fn no_lengths(_: ObjId) -> Option<i64> {
        None
    }

    // ---- scalars and containers ----

    #[test]
    fn scalars() {
        assert_eq!(parse(b"true"), Object::Boolean(true));
        assert_eq!(parse(b"false"), Object::Boolean(false));
        assert_eq!(parse(b"null"), Object::Null);
        assert_eq!(parse(b"42"), Object::Integer(42));
        assert_eq!(parse(b"4."), Object::Real(4.0));
        assert_eq!(parse(b"(hi)"), Object::String(b"hi".to_vec()));
        assert_eq!(parse(b"/Type"), Object::Name(Name::from(b"Type")));
    }

    #[test]
    fn heterogeneous_array_spec_example() {
        // §7.3.6 EXAMPLE.
        let Object::Array(a) = parse(b"[ 549 3.14 false (Ralph) /SomeName ]") else {
            panic!("not an array");
        };
        assert_eq!(a.len(), 5);
        assert_eq!(a[0], Object::Integer(549));
        assert_eq!(a[2], Object::Boolean(false));
        assert_eq!(a[4], Object::Name(Name::from(b"SomeName")));
    }

    #[test]
    fn nested_dict() {
        let obj = parse(b"<< /A << /B [1 2] >> /C null >>");
        let d = obj.as_dict().unwrap();
        let inner = d.get(b"A").unwrap().as_dict().unwrap();
        assert_eq!(inner.get(b"B").unwrap().as_array().unwrap().len(), 2);
        // §7.3.7: null value ≡ absent.
        assert!(d.get(b"C").is_none());
    }

    // ---- the N G R lookahead ----

    #[test]
    fn reference_lookahead_in_array() {
        // §7.3.10: `[1 0 R 2 0 R]` is two references, not six values.
        let Object::Array(a) = parse(b"[1 0 R 2 0 R]") else {
            panic!("not an array");
        };
        assert_eq!(
            a,
            vec![
                Object::Reference(ObjId::new(1, 0)),
                Object::Reference(ObjId::new(2, 0)),
            ]
        );
    }

    #[test]
    fn integers_that_are_not_references_stay_integers() {
        // Two integers followed by a non-R token: plain integers.
        let Object::Array(a) = parse(b"[1 0 3]") else {
            panic!("not an array");
        };
        assert_eq!(
            a,
            vec![Object::Integer(1), Object::Integer(0), Object::Integer(3)]
        );
        // Trailing pair at EOF (no third token): plain integers.
        let Object::Array(a) = parse(b"[1 0]") else {
            panic!("not an array");
        };
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn reference_as_dict_value() {
        let obj = parse(b"<< /Root 2 0 R >>");
        let d = obj.as_dict().unwrap();
        assert_eq!(
            d.get(b"Root").unwrap().as_reference(),
            Some(ObjId::new(2, 0))
        );
    }

    /// A missing `endobj` is an error on the STRICT path, and stays one.
    ///
    /// The recovery leniency added 2026-08-07 must not leak into clean
    /// loading. On a file whose xref parses, a missing terminator is real
    /// damage the operator should hear about, and accepting it would put an
    /// inferred extent into the writer's byte-identical re-emission path
    /// and break the round-trip invariant (`ARCHITECTURE.md` §5).
    #[test]
    fn missing_endobj_is_refused_under_the_default_policy() {
        let buf: &[u8] = b"2 0 obj
<< /Type /Pages >>
3 0 obj
<< >>
endobj
";
        let err = Parser::at(buf, 0)
            .parse_indirect_object(&mut |_| None)
            .expect_err("strict parsing must refuse a definition with no endobj");
        assert_eq!(err.kind, ParseErrorKind::MissingEndobj);
    }

    /// The same bytes are ACCEPTED under the recovery policy, and counted.
    ///
    /// Paired with the test above deliberately: together they prove the
    /// difference is caused by the POLICY and not by something incidental
    /// about the input. Either test alone would pass against a parser that
    /// ignored the policy entirely in one direction.
    #[test]
    fn missing_endobj_is_recovered_and_counted_under_the_lenient_policy() {
        let buf: &[u8] = b"2 0 obj
<< /Type /Pages >>
3 0 obj
<< >>
endobj
";
        let mut parser =
            Parser::at(buf, 0).with_terminator_policy(TerminatorPolicy::RecoverAtNextHeader);
        let io = parser
            .parse_indirect_object(&mut |_| None)
            .expect("the lenient policy accepts a body that parsed cleanly");
        assert_eq!(io.id, ObjId::new(2, 0));
        assert_eq!(parser.missing_endobj_recovered(), 1);
        // The extent must stop before `3 0 obj` — claiming the next
        // object's header would corrupt whatever re-parses this span.
        let Provenance::RecoveredFile(span) = io.provenance else {
            panic!(
                "a terminator-less definition must be RecoveredFile, so the writer re-serializes instead of copying bytes that lack an endobj"
            );
        };
        assert!(span.end() <= buf.windows(7).position(|w| w == b"3 0 obj").unwrap());
    }

    // ---- strictness ----

    #[test]
    fn duplicate_dict_key_is_error() {
        assert_eq!(
            parse_err(b"<< /A 1 /A 2 >>"),
            ParseErrorKind::DuplicateDictKey
        );
    }

    #[test]
    fn non_name_dict_key_is_error() {
        assert_eq!(parse_err(b"<< 1 2 >>"), ParseErrorKind::DictKeyNotName);
    }

    #[test]
    fn depth_guard_trips() {
        let mut bomb = vec![b'['; MAX_NESTING_DEPTH + 8];
        bomb.extend_from_slice(&vec![b']'; MAX_NESTING_DEPTH + 8]);
        assert_eq!(parse_err(&bomb), ParseErrorKind::DepthExceeded);
    }

    #[test]
    fn unclosed_array_is_eof_error() {
        assert_eq!(parse_err(b"[1 2"), ParseErrorKind::UnexpectedEof);
    }

    #[test]
    fn stray_keyword_at_value_position_is_error() {
        assert!(matches!(
            parse_err(b"frobnicate"),
            ParseErrorKind::Unexpected(_)
        ));
    }

    // ---- indirect objects ----

    #[test]
    fn indirect_object_spec_example_and_span() {
        // §7.3.10 EXAMPLE 1 — and the span covers the FULL definition
        // (the provenance contract, decision 001 §6.1 item 1).
        let buf: &[u8] = b"12 0 obj\n    (Brillig)\nendobj";
        let io = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap();
        assert_eq!(io.id, ObjId::new(12, 0));
        assert_eq!(io.value, Object::String(b"Brillig".to_vec()));
        assert_eq!(io.file_span().unwrap().slice(buf).unwrap(), buf);
    }

    #[test]
    fn stream_with_direct_length() {
        let buf: &[u8] = b"5 0 obj << /Length 9 >>\nstream\nsome data\nendstream endobj";
        let io = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"some data");
        assert_eq!(io.file_span().unwrap().slice(buf).unwrap(), buf);
    }

    #[test]
    fn stream_with_crlf_after_keyword() {
        let buf: &[u8] = b"5 0 obj << /Length 4 >>\nstream\r\nabcd\nendstream endobj";
        let io = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"abcd");
    }

    #[test]
    fn stream_cr_alone_after_keyword_is_error() {
        // §7.3.8.1: "and NOT by CR alone."
        let buf: &[u8] = b"5 0 obj << /Length 4 >>\nstream\rabcd\nendstream endobj";
        let e = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::BadStreamEol);
    }

    #[test]
    fn stream_with_indirect_length_resolves_via_callback() {
        // §7.3.10 EXAMPLE 3's single-pass-writer pattern.
        let buf: &[u8] = b"7 0 obj << /Length 8 0 R >>\nstream\n0123456\nendstream endobj";
        let mut resolver = |id: ObjId| (id == ObjId::new(8, 0)).then_some(7);
        let io = Parser::at(buf, 0)
            .parse_indirect_object(&mut resolver)
            .unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"0123456");
    }

    #[test]
    fn stream_wrong_length_is_extent_mismatch() {
        // §7.3.8.2: inconsistent extent "is an error" under the DEFAULT
        // Strict policy — no silent endstream scanning on the clean path.
        let buf: &[u8] = b"5 0 obj << /Length 3 >>\nstream\nsome data\nendstream endobj";
        let e = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::StreamExtentMismatch);
    }

    #[test]
    fn stream_missing_length_is_error() {
        let buf: &[u8] = b"5 0 obj << >>\nstream\nxx\nendstream endobj";
        let e = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::BadStreamLength);
    }

    // ---- StreamLengthPolicy::RecoverFromEndstream (recovery path only) ----

    /// A `/Length` that is too SHORT — the dominant real-world shape (a
    /// file whose lengths were computed for LF endings and then converted
    /// to CRLF) — re-derives from `endstream` and is counted.
    #[test]
    fn recovering_policy_repairs_a_short_length() {
        let buf: &[u8] = b"5 0 obj << /Length 3 >>\nstream\nsome data\nendstream endobj";
        let mut p =
            Parser::at(buf, 0).with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream);
        let io = p.parse_indirect_object(&mut no_lengths).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        // The EOL before `endstream` is backed off (§7.3.8.1), so the data
        // is exactly the payload — not `"some data\n"`.
        assert_eq!(s.data_span.slice(buf).unwrap(), b"some data");
        assert_eq!(p.stream_lengths_recovered(), 1);
        // The dictionary must agree with the span, or the writer would
        // re-emit the inconsistency it just recovered from.
        assert_eq!(s.dict.get(b"Length"), Some(&Object::Integer(9)));
    }

    /// The dictionary/span agreement holds for an INDIRECT `/Length` too:
    /// the reference is replaced by the derived direct integer.
    #[test]
    fn recovering_policy_replaces_an_indirect_length_with_the_derived_one() {
        let buf: &[u8] = b"5 0 obj << /Length 8 0 R >>\nstream\nsome data\nendstream endobj";
        // Object 8 resolves to a wrong (too short) value.
        let mut resolve = |id: ObjId| -> Option<i64> { (id.num == 8).then_some(3) };
        let mut p =
            Parser::at(buf, 0).with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream);
        let io = p.parse_indirect_object(&mut resolve).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"some data");
        assert_eq!(s.dict.get(b"Length"), Some(&Object::Integer(9)));
        assert_eq!(p.stream_lengths_recovered(), 1);
    }

    /// A `/Length` that is too LONG is repaired by the same rule: the scan
    /// starts at the first data byte, so it finds the terminator whichever
    /// side of the stored end it lies on.
    #[test]
    fn recovering_policy_repairs_a_long_length() {
        let buf: &[u8] = b"5 0 obj << /Length 400 >>\nstream\nabc\nendstream endobj";
        let mut p =
            Parser::at(buf, 0).with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream);
        let io = p.parse_indirect_object(&mut no_lengths).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"abc");
        assert_eq!(p.stream_lengths_recovered(), 1);
    }

    /// A missing `/Length` is also recoverable — `endstream` is then the
    /// only surviving statement of the extent.
    #[test]
    fn recovering_policy_repairs_a_missing_length() {
        let buf: &[u8] = b"5 0 obj << >>\nstream\nxx\nendstream endobj";
        let mut p =
            Parser::at(buf, 0).with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream);
        let io = p.parse_indirect_object(&mut no_lengths).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"xx");
        assert_eq!(p.stream_lengths_recovered(), 1);
    }

    /// A CORRECT `/Length` is left completely alone under the recovering
    /// policy: no scan, no repair, counter stays 0. This is what makes the
    /// policy safe to apply to a whole recovered file rather than only to
    /// the objects already known to be broken.
    #[test]
    fn recovering_policy_does_not_touch_a_correct_length() {
        let buf: &[u8] = b"5 0 obj << /Length 9 >>\nstream\nsome data\nendstream endobj";
        let mut p =
            Parser::at(buf, 0).with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream);
        let io = p.parse_indirect_object(&mut no_lengths).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(buf).unwrap(), b"some data");
        assert_eq!(
            p.stream_lengths_recovered(),
            0,
            "a file that agrees with itself is never 'repaired'"
        );
    }

    /// With NO `endstream` anywhere there is nothing to recover from, so
    /// the strict refusal stands — recovery never invents an extent.
    #[test]
    fn recovering_policy_still_refuses_when_there_is_no_endstream() {
        let buf: &[u8] = b"5 0 obj << /Length 3 >>\nstream\nsome data\nendobj";
        let e = Parser::at(buf, 0)
            .with_stream_length_policy(StreamLengthPolicy::RecoverFromEndstream)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::StreamExtentMismatch);
    }

    /// The default is Strict — the policy must be opted into explicitly,
    /// so no clean-path caller can acquire it by accident.
    #[test]
    fn default_policy_is_strict() {
        assert_eq!(StreamLengthPolicy::default(), StreamLengthPolicy::Strict);
    }

    #[test]
    fn object_number_zero_is_rejected() {
        // §7.5.4: object 0 is permanently the free-list head.
        let buf: &[u8] = b"0 0 obj null endobj";
        let e = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::BadObjectHeader);
    }

    #[test]
    fn missing_endobj_is_error() {
        let buf: &[u8] = b"3 0 obj 42 trailer";
        let e = Parser::at(buf, 0)
            .parse_indirect_object(&mut no_lengths)
            .unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::MissingEndobj);
    }
}
