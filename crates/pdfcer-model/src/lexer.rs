//! # PDF tokenizer (ISO 32000-1 §7.2 lexical conventions + §7.3 token syntax)
//!
//! Byte-classification, token scanning, and the lossy-decode-plus-span
//! model that underpins pdfcer's round-trip invariant. Spec sources are
//! the Pass 1 slice of the PDF-spec RAG (`D:\Dev\Rag-Specialized\PDF_Spec\`):
//! `iso32000__s__7.2.md` (byte classes, EOL, comments),
//! `iso32000__s__7.3.md` (numerics, keywords), `iso32000__s__7.3.4.md`
//! (strings), `iso32000__s__7.3.5.md` (names). Clause numbers below are
//! ISO 32000-1:2008.
//!
//! ## Scope and layering
//!
//! The lexer produces [`Token`]s — syntax-level units with **decoded
//! values where decoding is lossy** (strings, names) and a [`ByteSpan`]
//! recording exactly which source bytes each token came from. It does
//! **not**:
//!
//! - resolve object structure (`obj`/`endobj`/`R` are delivered as
//!   [`TokenKind::Keyword`]; the object parser interprets them);
//! - read stream *data* (the parser handles the `stream` keyword's
//!   `/Length`-governed data span itself — §7.3.8's framing rules need
//!   the resolved `/Length`, which can be an indirect reference, so data
//!   extraction is inherently a parser-with-xref concern);
//! - interpret string bytes as text (§7.9.2 is a later-Pass concern; the
//!   lexer delivers exact decoded bytes).
//!
//! Per §7.8.2 content streams are lexed with **these same rules** — this
//! one lexer serves both the file body and content streams (operators
//! arrive as `Keyword` tokens; the sole divergence, inline-image data
//! between `ID` and `EI`, is handled by the content-stream layer, see
//! `iso32000__s__8.9.7.md`).
//!
//! ## The decode + span model (round-trip, ARCHITECTURE.md §5)
//!
//! PDF token syntax is non-canonical: `/A#42` ≡ `/AB` (§7.3.5 NOTE 1), a
//! bare CRLF inside a literal string decodes to one 0Ah byte (§7.3.4.2 —
//! lossy *by specification*), and `4.` / `+17` / `0.40` carry formatting
//! a parsed value cannot reproduce (§7.3.3). Therefore every token
//! carries its span, tokens never re-emit themselves from decoded
//! values, and untouched entities re-emit their source bytes verbatim.
//! See `crate::span` for the full contract
//! (docs/decisions/001-oxidize-pdf-adopt-vs-build.md §6.1).
//!
//! ## Failure philosophy
//!
//! Fail-clean (ARCHITECTURE.md §10; decision 001 §6.1 item 4): malformed
//! syntax yields a structured [`LexError`] with the exact byte offset —
//! never a silent best-guess decode, never a panic (the crate-level
//! `deny` lints enforce the latter mechanically). Where the spec leaves
//! reader behaviour undefined (e.g. `#` in a name followed by non-hex,
//! §7.3.5 — see that RAG file's gotcha list), Pass 1 errors strictly;
//! documented tolerances for real-world deviations get added
//! deliberately, with corpus evidence filed in `C:\personal_rag\pdf\`,
//! not by default.
//!
//! ## Resource guards (ARCHITECTURE.md §10.1 — pdfcer policy, not spec)
//!
//! [`MAX_TOKEN_LEN`] bounds name/keyword/number tokens;
//! [`MAX_STRING_LEN`] bounds decoded string output. Annex C's limits
//! (name 127 bytes, content-stream string 32,767 bytes) are *writer*
//! guidance a reader should exceed gracefully (signature `/Contents`
//! strings routinely blow past 32,767 — `iso32000__s__7.3.4.md`), so the
//! ceilings here are deliberately far above spec limits: high enough for
//! any legitimate file, low enough that a hostile file can't force
//! unbounded allocation from the lexer.

use crate::span::ByteSpan;

// ---------------------------------------------------------------------------
// Byte classification (§7.2.2, Tables 1 and 2)
// ---------------------------------------------------------------------------

/// Is `b` one of the six white-space characters (Table 1)?
///
/// NUL (00h) **is** white-space — per the spec RAG, "the single most
/// commonly mis-implemented entry in Table 1."
#[must_use]
pub const fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// Is `b` one of the ten delimiter characters (Table 2)?
///
/// `{` and `}` are delimiters even though no base-grammar construct uses
/// them (they belong to Type 4 PostScript-calculator function streams,
/// §7.10.5) — they still terminate adjacent tokens.
#[must_use]
pub const fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Is `b` a regular character (everything neither white-space nor
/// delimiter, §7.2.2)? Includes all bytes 80h–FFh.
#[must_use]
pub const fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

// ---------------------------------------------------------------------------
// Resource-guard policy constants (ARCHITECTURE.md §10.1)
// ---------------------------------------------------------------------------

/// Maximum length of a single name/keyword/number token, in source bytes.
///
/// pdfcer policy, not a spec value: Annex C's 127-byte name limit is
/// writer guidance ("readers should accept longer") — and PDF/A
/// §6.1.12 tests that conforming readers do NOT impose the old
/// Annex-C implementation limits at all. The 2026-07-30 corpus run
/// proved the point: the original 8 KiB ceiling rejected veraPDF's
/// `6-1-12-t02-pass-k.pdf`, a VALID file, the only pass-classified
/// corpus file pdfcer mishandled. Raised to 1 MiB: still a hard bound
/// against unbounded adversarial buffering, far above anything the
/// PDF/A limit tests exercise.
pub const MAX_TOKEN_LEN: usize = 1024 * 1024;

/// Maximum decoded length of a single string token, in bytes.
///
/// pdfcer policy, not a spec value: Annex C's 32,767-byte limit applies
/// only to content-stream strings, and legitimate signature `/Contents`
/// hex strings exceed it by design. 16 MiB accommodates any plausible
/// legitimate string (embedded-signature CMS containers included) while
/// bounding hostile allocation ([`LexErrorKind::StringTooLong`]).
pub const MAX_STRING_LEN: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// One lexical token: its classified/decoded content plus the exact
/// source-byte span it was scanned from.
///
/// The span is load-bearing (see module docs); [`Token::lexeme`]
/// recovers the raw source bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Classified/decoded content of the token.
    pub kind: TokenKind,
    /// Exact source bytes this token was scanned from.
    pub span: ByteSpan,
}

impl Token {
    /// The raw source bytes of this token within `buf` (the buffer it
    /// was lexed from). `None` indicates the span/buffer pairing is
    /// wrong — a caller logic error surfaced non-panicking per the
    /// crate policy.
    #[must_use]
    pub fn lexeme<'a>(&self, buf: &'a [u8]) -> Option<&'a [u8]> {
        self.span.slice(buf)
    }
}

/// Token classification.
///
/// Where decoding is lossy (strings, names) the decoded bytes are stored
/// here **and** the raw form remains reachable via [`Token::span`] —
/// both are needed (decoded for semantics, raw for verbatim re-emission).
/// Where the lexeme itself is the value (keywords) nothing is copied;
/// the parser reads the span.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TokenKind {
    /// Integer (§7.3.3): optional sign, decimal digits. Value parsed as
    /// `i64` — wider than Annex C's ±2³¹ *writer* range on purpose, so
    /// large-but-well-formed offsets in big files still lex.
    Integer(i64),
    /// Real (§7.3.3): optional sign, digits, one PERIOD — leading
    /// (`-.002`), trailing (`4.`), or embedded. The stored `f64` is the
    /// *semantic* value; the source formatting survives via the span.
    Real(f64),
    /// Literal (§7.3.4.2) or hexadecimal (§7.3.4.3) string, fully
    /// decoded to its byte content (escapes applied, EOLs normalized,
    /// hex pairs assembled). These are raw bytes, not text — §7.9.2
    /// interpretation happens in later layers.
    String(Vec<u8>),
    /// Name (§7.3.5), `#`-escapes decoded, WITHOUT the leading `/`.
    /// May be empty (the bare `/` is a valid name). Decoding before
    /// storage is required so `/Type` and `/Ty#70e` compare equal.
    Name(Vec<u8>),
    /// `[` (§7.3.6).
    ArrayOpen,
    /// `]` (§7.3.6).
    ArrayClose,
    /// `<<` (§7.3.7).
    DictOpen,
    /// `>>` (§7.3.7).
    DictClose,
    /// `{` — a delimiter with no role in the base object grammar; only
    /// Type 4 function streams (§7.10.5) use it. The lexer emits it so
    /// that layer can parse; the object parser treats it as a syntax
    /// error.
    BraceOpen,
    /// `}` — see [`TokenKind::BraceOpen`].
    BraceClose,
    /// Any other run of regular characters: `true`, `false`, `null`,
    /// `obj`, `endobj`, `stream`, `R`, `xref`, content-stream operators
    /// (`BT`, `Tj`, `re`, …), and also malformed numeric-looking runs
    /// (`1.2.3`) which the *parser* rejects in context. The bytes are
    /// the span — call [`Token::lexeme`].
    Keyword,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A lexical error: what went wrong and exactly where.
///
/// C-GOOD-ERR: implements `std::error::Error` via `thiserror`, is
/// `Send + Sync + 'static`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("lex error at byte {offset}: {kind}")]
pub struct LexError {
    /// Byte offset (absolute, from buffer start) where the error was
    /// detected.
    pub offset: usize,
    /// What was wrong.
    pub kind: LexErrorKind,
}

/// Classification of lexical errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LexErrorKind {
    /// EOF inside a literal string before its parentheses balanced, or
    /// a REVERSE SOLIDUS as the final byte (§7.3.4.2 leaves the latter
    /// undefined; pdfcer errors).
    #[error("unterminated literal string")]
    UnterminatedString,
    /// EOF inside a hexadecimal string before the closing `>`.
    #[error("unterminated hexadecimal string")]
    UnterminatedHexString,
    /// A byte in a hexadecimal string that is neither a hex digit nor
    /// white-space (§7.3.4.3; treated as a syntax error per the RAG's
    /// reading).
    #[error("invalid byte 0x{0:02X} in hexadecimal string")]
    InvalidHexStringByte(u8),
    /// `#` in a name not followed by two hex digits. §7.3.5 defines no
    /// reader behaviour for this; Pass 1 is strict (see module docs'
    /// failure philosophy).
    #[error("malformed #-escape in name")]
    MalformedNameEscape,
    /// `#00` in a name — the name definition excludes NUL ("any
    /// characters except null", §7.3.5).
    #[error("#00 (NUL) escape in name")]
    NulInName,
    /// A delimiter that cannot begin a token: a lone `)` or a `>` not
    /// followed by another `>`.
    #[error("unexpected delimiter byte 0x{0:02X}")]
    UnexpectedByte(u8),
    /// A numeric-looking token whose integer value exceeds `i64` — far
    /// outside any legitimate PDF (Annex C writer range is ±2³¹);
    /// refused rather than silently saturated (fail-clean).
    #[error("integer token overflows i64")]
    IntegerOverflow,
    /// Regular-character run longer than [`MAX_TOKEN_LEN`] (resource
    /// guard, pdfcer policy).
    #[error("token exceeds MAX_TOKEN_LEN ({MAX_TOKEN_LEN} bytes)")]
    TokenTooLong,
    /// Decoded string output longer than [`MAX_STRING_LEN`] (resource
    /// guard, pdfcer policy).
    #[error("string exceeds MAX_STRING_LEN ({MAX_STRING_LEN} bytes)")]
    StringTooLong,
}

// ---------------------------------------------------------------------------
// The lexer
// ---------------------------------------------------------------------------

/// A pull tokenizer over a byte buffer.
///
/// Construct with [`Lexer::new`] (from the buffer start) or
/// [`Lexer::at`] (from an arbitrary offset — xref-driven parsing jumps
/// straight to object offsets), then call [`Lexer::next_token`]
/// repeatedly. White-space and comments are skipped between tokens
/// (§7.2.2/§7.2.3: a comment separates tokens exactly like white-space);
/// their bytes remain reconstructable from the buffer because every
/// token records its span, and untouched-object re-emission works on
/// whole-object spans anyway (see `crate::span`).
///
/// # Examples
///
/// ```
/// use pdfcer_model::lexer::{Lexer, TokenKind};
///
/// let mut lx = Lexer::new(b"/Type /Pages % a comment\n42");
/// assert!(matches!(lx.next_token().unwrap().unwrap().kind,
///                  TokenKind::Name(ref n) if n == b"Type"));
/// assert!(matches!(lx.next_token().unwrap().unwrap().kind,
///                  TokenKind::Name(ref n) if n == b"Pages"));
/// assert!(matches!(lx.next_token().unwrap().unwrap().kind,
///                  TokenKind::Integer(42)));
/// assert!(lx.next_token().unwrap().is_none()); // EOF
/// ```
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Lexer over `buf`, starting at byte 0.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Lexer over `buf`, starting at `pos`. If `pos` is past the end of
    /// `buf`, the lexer is immediately at EOF (a caller passing a bogus
    /// xref offset gets a clean "no token" rather than a panic).
    #[must_use]
    pub const fn at(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    /// Current byte offset (the next byte the lexer will examine).
    ///
    /// After a successful [`Lexer::next_token`] this sits one past the
    /// returned token — which is exactly where stream data begins after
    /// a `stream` keyword's EOL, so the parser uses this to take over
    /// for §7.3.8 data extraction.
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Scan and return the next token, `Ok(None)` at end of input.
    ///
    /// # Errors
    ///
    /// [`LexError`] with the offending byte offset — see
    /// [`LexErrorKind`] for the cases. After an error the lexer's
    /// position is unspecified; callers treat lex errors as fatal for
    /// the current parse unit (recovery strategies live in the parser,
    /// not here).
    pub fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_ws_and_comments();
        let start = self.pos;
        let Some(b) = self.peek() else {
            return Ok(None);
        };

        let kind = match b {
            b'[' => {
                self.bump();
                TokenKind::ArrayOpen
            }
            b']' => {
                self.bump();
                TokenKind::ArrayClose
            }
            b'{' => {
                self.bump();
                TokenKind::BraceOpen
            }
            b'}' => {
                self.bump();
                TokenKind::BraceClose
            }
            b'<' => {
                // `<<` opens a dictionary; `<…` begins a hex string.
                // One byte of lookahead disambiguates (§7.3.4.3 note in
                // the RAG).
                if self.peek_at(1) == Some(b'<') {
                    self.bump();
                    self.bump();
                    TokenKind::DictOpen
                } else {
                    self.bump();
                    TokenKind::String(self.scan_hex_string_body()?)
                }
            }
            b'>' => {
                if self.peek_at(1) == Some(b'>') {
                    self.bump();
                    self.bump();
                    TokenKind::DictClose
                } else {
                    // A single `>` begins nothing.
                    return Err(self.err_at(start, LexErrorKind::UnexpectedByte(b'>')));
                }
            }
            b'(' => {
                self.bump();
                TokenKind::String(self.scan_literal_string_body()?)
            }
            b')' => {
                // A `)` outside a literal string balances nothing.
                return Err(self.err_at(start, LexErrorKind::UnexpectedByte(b')')));
            }
            b'/' => {
                self.bump();
                TokenKind::Name(self.scan_name_body()?)
            }
            _ => {
                // A run of regular characters: number or keyword.
                debug_assert!(is_regular(b));
                self.scan_regular_run(start)?
            }
        };

        Ok(Some(Token {
            kind,
            span: ByteSpan::from_range(start..self.pos),
        }))
    }

    // -- low-level cursor helpers (all bounds-checked; crate policy) --------

    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.buf.get(self.pos.saturating_add(ahead)).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn err_here(&self, kind: LexErrorKind) -> LexError {
        LexError {
            offset: self.pos,
            kind,
        }
    }

    fn err_at(&self, offset: usize, kind: LexErrorKind) -> LexError {
        LexError { offset, kind }
    }

    // -- inter-token skipping (§7.2.2 white-space, §7.2.3 comments) ---------

    /// Skip white-space and comments. A comment runs from `%` to (not
    /// including) the next CR or LF, or to EOF (§7.2.3; the RAG's
    /// gotcha: a `%` as the file's last byte terminates at EOF). The
    /// comment's terminating EOL is consumed here too — as white-space,
    /// which it is.
    fn skip_ws_and_comments(&mut self) {
        while let Some(b) = self.peek() {
            if is_whitespace(b) {
                self.bump();
            } else if b == b'%' {
                self.bump();
                while let Some(c) = self.peek() {
                    if c == b'\r' || c == b'\n' {
                        break;
                    }
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    // -- literal strings (§7.3.4.2) -----------------------------------------

    /// Decode a literal string body. Entered with the opening `(`
    /// already consumed; consumes through the balancing `)`.
    ///
    /// Rules applied (each from `iso32000__s__7.3.4.md`):
    /// - balanced parens nest without escapes → depth tracking;
    /// - Table 3 escapes; unknown escape drops the backslash (`\q`→`q`);
    /// - `\ddd` octal, 1–3 digits, high-order overflow ignored (low 8
    ///   bits); `\8`/`\9` are NOT octal — they hit the unknown-escape
    ///   rule;
    /// - `\` + EOL = line continuation, both dropped (CRLF is one EOL);
    /// - bare EOL (CR, LF, or CRLF) decodes to a single 0Ah.
    fn scan_literal_string_body(&mut self) -> Result<Vec<u8>, LexError> {
        let mut out = Vec::new();
        let mut depth: usize = 1;
        loop {
            if out.len() > MAX_STRING_LEN {
                return Err(self.err_here(LexErrorKind::StringTooLong));
            }
            let Some(b) = self.peek() else {
                return Err(self.err_here(LexErrorKind::UnterminatedString));
            };
            self.bump();
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(b')');
                }
                b'\\' => {
                    let Some(e) = self.peek() else {
                        // `\` as the final byte: undefined by spec;
                        // error per the failure philosophy.
                        return Err(self.err_here(LexErrorKind::UnterminatedString));
                    };
                    match e {
                        b'n' => {
                            self.bump();
                            out.push(0x0A);
                        }
                        b'r' => {
                            self.bump();
                            out.push(0x0D);
                        }
                        b't' => {
                            self.bump();
                            out.push(0x09);
                        }
                        b'b' => {
                            self.bump();
                            out.push(0x08);
                        }
                        b'f' => {
                            self.bump();
                            out.push(0x0C);
                        }
                        b'(' | b')' | b'\\' => {
                            self.bump();
                            out.push(e);
                        }
                        b'0'..=b'7' => {
                            // 1–3 octal digits; overflow keeps low 8
                            // bits (§7.3.4.2: "high-order overflow shall
                            // be ignored").
                            let mut value: u32 = 0;
                            let mut digits = 0;
                            while digits < 3 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        self.bump();
                                        value = (value << 3) | u32::from(d - b'0');
                                        digits += 1;
                                    }
                                    _ => break,
                                }
                            }
                            #[allow(clippy::cast_possible_truncation)] // low 8 bits: spec rule
                            out.push((value & 0xFF) as u8);
                        }
                        b'\r' => {
                            // Line continuation; CRLF is ONE EOL marker
                            // (§7.2.2), so both bytes are disregarded.
                            self.bump();
                            if self.peek() == Some(b'\n') {
                                self.bump();
                            }
                        }
                        b'\n' => {
                            self.bump();
                        }
                        _ => {
                            // Unknown escape: backslash ignored, the
                            // character stands (`\q` → `q`; also `\8`,
                            // `\9`).
                            self.bump();
                            out.push(e);
                        }
                    }
                }
                b'\r' => {
                    // Bare EOL → single 0Ah, whether CR, LF, or CRLF.
                    if self.peek() == Some(b'\n') {
                        self.bump();
                    }
                    out.push(0x0A);
                }
                _ => out.push(b),
            }
        }
    }

    // -- hexadecimal strings (§7.3.4.3) -------------------------------------

    /// Decode a hex string body. Entered with the opening `<` consumed;
    /// consumes through the closing `>`. White-space between digits is
    /// ignored; an odd final digit is padded with `0` (`<901FA>` →
    /// `90 1F A0`); any other byte is a syntax error.
    fn scan_hex_string_body(&mut self) -> Result<Vec<u8>, LexError> {
        let mut out = Vec::new();
        let mut pending: Option<u8> = None;
        loop {
            if out.len() > MAX_STRING_LEN {
                return Err(self.err_here(LexErrorKind::StringTooLong));
            }
            let Some(b) = self.peek() else {
                return Err(self.err_here(LexErrorKind::UnterminatedHexString));
            };
            if b == b'>' {
                self.bump();
                if let Some(hi) = pending {
                    // Odd digit count: final digit assumed followed by 0.
                    out.push(hi << 4);
                }
                return Ok(out);
            }
            if is_whitespace(b) {
                self.bump();
                continue;
            }
            let Some(nibble) = hex_value(b) else {
                return Err(self.err_here(LexErrorKind::InvalidHexStringByte(b)));
            };
            self.bump();
            match pending.take() {
                Some(hi) => out.push((hi << 4) | nibble),
                None => pending = Some(nibble),
            }
        }
    }

    // -- names (§7.3.5) ------------------------------------------------------

    /// Decode a name body. Entered with the introducing `/` consumed
    /// (the `/` is not part of the name). Ends at the first
    /// non-regular byte, which is NOT consumed. The empty name (`/`
    /// alone) is valid and yields an empty vec.
    ///
    /// `#XX` escapes decode to the byte XX; `#` not followed by two hex
    /// digits errors (spec-undefined; Pass 1 strict), `#00` errors (NUL
    /// excluded from names by definition).
    fn scan_name_body(&mut self) -> Result<Vec<u8>, LexError> {
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            if out.len() > MAX_TOKEN_LEN {
                return Err(self.err_here(LexErrorKind::TokenTooLong));
            }
            self.bump();
            if b == b'#' {
                let hi = self.peek().and_then(hex_value);
                let lo = self.peek_at(1).and_then(hex_value);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        self.bump();
                        self.bump();
                        let decoded = (hi << 4) | lo;
                        if decoded == 0 {
                            return Err(self.err_here(LexErrorKind::NulInName));
                        }
                        out.push(decoded);
                    }
                    _ => return Err(self.err_here(LexErrorKind::MalformedNameEscape)),
                }
            } else {
                out.push(b);
            }
        }
        Ok(out)
    }

    // -- numbers and keywords (§7.3.3, §7.3.2, §7.8.2) -----------------------

    /// Scan a run of regular characters starting at `start` (first byte
    /// not yet consumed) and classify it: integer, real, or keyword.
    ///
    /// Classification (from §7.3.3's grammar): the run is numeric if it
    /// consists only of `+`/`-`/digits/`.`, with any sign leading only,
    /// at most one PERIOD, and at least one digit — `4.`, `-.002`,
    /// `+17` are all valid (spec EXAMPLEs). Anything else — `true`,
    /// `obj`, `T*`, but also malformed numeric-looking runs like
    /// `1.2.3` or a bare `.` — is a [`TokenKind::Keyword`], and the
    /// *parser* decides whether it is meaningful in context. This keeps
    /// the lexer total over content-stream operators without a
    /// hardcoded operator list.
    fn scan_regular_run(&mut self, start: usize) -> Result<TokenKind, LexError> {
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            if self.pos.saturating_sub(start) > MAX_TOKEN_LEN {
                return Err(self.err_here(LexErrorKind::TokenTooLong));
            }
            self.bump();
        }
        let run = self.buf.get(start..self.pos).unwrap_or(&[]);

        Ok(match classify_number(run) {
            NumberClass::Integer => TokenKind::Integer(parse_i64(run).ok_or(LexError {
                offset: start,
                kind: LexErrorKind::IntegerOverflow,
            })?),
            NumberClass::Real => {
                // The validated grammar (sign, digits, one period) is a
                // strict subset of what `f64: FromStr` accepts, so this
                // parse cannot fail; the fallback keeps the path
                // panic-free rather than trusting that reasoning.
                let text = String::from_utf8_lossy(run);
                text.parse::<f64>()
                    .map_or(TokenKind::Keyword, TokenKind::Real)
            }
            NumberClass::NotANumber => TokenKind::Keyword,
        })
    }
}

/// Value of an ASCII hex digit, or `None`.
const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// Outcome of numeric classification for a regular-character run.
enum NumberClass {
    Integer,
    Real,
    NotANumber,
}

/// Classify a regular-character run per §7.3.3's numeric grammar (see
/// [`Lexer::scan_regular_run`] for the rule statement).
fn classify_number(run: &[u8]) -> NumberClass {
    let mut digits = 0usize;
    let mut periods = 0usize;
    for (i, &b) in run.iter().enumerate() {
        match b {
            b'+' | b'-' if i == 0 => {}
            b'0'..=b'9' => digits += 1,
            b'.' => periods += 1,
            _ => return NumberClass::NotANumber,
        }
    }
    match (digits, periods) {
        (0, _) => NumberClass::NotANumber, // bare `.`, `+`, `-`
        (_, 0) => NumberClass::Integer,
        (_, 1) => NumberClass::Real,
        _ => NumberClass::NotANumber, // `1.2.3` — parser's problem
    }
}

/// Parse a (pre-validated) signed decimal integer run into `i64`,
/// `None` on overflow.
fn parse_i64(run: &[u8]) -> Option<i64> {
    let (neg, digits) = match run.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, run),
    };
    let mut value: i64 = 0;
    for &b in digits {
        let d = i64::from(b.checked_sub(b'0')?);
        if d > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(d)?;
    }
    Some(if neg { -value } else { value })
}
