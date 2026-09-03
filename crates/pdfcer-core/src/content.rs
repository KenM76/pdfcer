//! # Content-stream token model (ISO 32000-1 §7.8.2, §8.9.7)
//!
//! The **lossless, byte-span-provenanced token stream** for content
//! streams, with the semantic operator view as a *projection* over it —
//! the architecture mandated by
//! `docs/decisions/001-oxidize-pdf-adopt-vs-build.md` §6.1 item 2.
//! Spec sources: `iso32000__s__7.8.md` (operand/operator rules,
//! resource model), `iso32000__s__8.9.7.md` (inline images — the one
//! lexing exception), `iso32000__s__7.7.3.md` (Contents-array
//! concatenation) in the PDF-spec RAG.
//!
//! ## Why a token stream and not an operator enum
//!
//! The audited prior art parses content into a semantic operator model
//! with **no serializer** — good for text extraction, structurally
//! wrong for editing (001 §5.3). pdfcer's model answers the question an
//! editor asks: *"change one operator and leave the other thousands
//! byte-identical."* Every [`ContentToken`] records its exact span in
//! the **decoded** content buffer; re-serialization emits verbatim
//! spans for untouched tokens and re-encodes only edited ones. (Spans
//! here index the decoded bytes, not the file: editing any token
//! re-encodes the whole stream *object*, but within the stream the
//! untouched majority re-emits exactly, which is what keeps diffs
//! reviewable and content edits minimal.)
//!
//! ## What this layer does and doesn't
//!
//! It tokenizes and structures; it does **not** interpret. Graphics
//! semantics (state machine, resource lookup, `BX`/`EX` tolerance,
//! unknown-operator policy) belong to the interpreter in
//! `pdfcer-render`. Per §7.8.2 the syntax is the standard §7.2 lexer —
//! [`crate::lexer`] is reused, never duplicated — with exactly one
//! divergence: raw image bytes between `ID` and `EI` (§8.9.7),
//! handled here because they must not reach the lexer at all.
//!
//! ## Operand rules enforced at this layer (§7.8.2)
//!
//! - Operands are **direct objects only** — the `N G R` reference
//!   syntax is banned in content streams outright, so this assembler
//!   has no reference lookahead; an `R` keyword simply becomes an
//!   (unknown) operator token for the interpreter to reject or skip.
//! - Dictionaries are legal operand syntax (used by `DP`/`BDC`);
//!   streams are impossible (no `stream` framing in content).
//!
//! ## Inline images (§8.9.7) — the `EI` hazard
//!
//! An inline image carries no `/Length`; finding the end of its data
//! is the most dangerous parse in the grammar. The strategy ladder
//! (from the RAG's four-case analysis):
//! 1. **Unfiltered data: compute, don't scan** — exactly
//!    `ceil(W × colors × BPC / 8) × H` bytes.
//! 2. **`AHx`/`A85`: scan for the filter's own EOD** (`>` / `~>`),
//!    which is ASCII-safe.
//! 3. **Other filters: scan for whitespace-delimited `EI`** — NOT
//!    spec-sourced, a documented heuristic (real-world divergence gets
//!    recorded in `C:\personal_rag\pdf\`).
//!
//! Abbreviated keys/values (Table 93/94) are normalized to full names
//! immediately for the semantic view; the raw span still re-emits the
//! original abbreviated bytes for untouched images (round-trip, §5).

use crate::filters::{self, FilterError};
// NOTE: `crate::graph::ObjectGraph` is not imported: `from_page` reaches the
// graph via `DocumentView::graph()`, whose `&dyn ObjectGraph` resolves trait
// methods without the trait being in scope.
use crate::lexer::{Lexer, Token, TokenKind};
/// How deep a chain of form XObjects invoking form XObjects may go before a
/// walker stops descending (ISO 32000-1 §8.10.1).
///
/// # ★★ 64, AND THE NUMBER IS CORPUS-CORRECTED RATHER THAN CHOSEN
///
/// Real documents nest two or three deep — a page invokes a template, which
/// invokes a logo — which is exactly what makes a small value *look* safe. But
/// veraPDF's PDF/A-1b §6.1.12 implementation-limits suite ships a
/// **conformant** file with a deliberate chain of **32** nested forms, and a
/// reader that refuses it is wrong. 64 is 2× the deepest conformant structure
/// anyone has measured, and Annex C sets no form-nesting limit at all.
///
/// # ★ It is a backstop, NOT the real defence
///
/// The attack it would have to stop is unbounded *recursion*, and that is
/// caught at any depth by a **cycle guard keyed on the form's object number**
/// — §8.10.1 does not forbid a form invoking itself, and the same stream is
/// reachable under different resource names, so a name-keyed guard misses the
/// cycle entirely. What this value actually bounds is the linear memory a
/// legitimate-but-absurd chain can pin.
///
/// # ★★★ WHY IT LIVES HERE, WHICH IS THE POINT OF THIS CONSTANT EXISTING
///
/// It was written down **twice** independently — `pdfcer-render`'s
/// `MAX_XOBJECT_DEPTH` and `text_extract`'s `ExtractOptions::max_form_depth`,
/// the second documented as *"matching `pdfcer-render`'s"* — and a third walker
/// (the vector decomposer) was about to add a fourth. Three hand-copied
/// constants that must agree is a disagreement waiting to happen, and the
/// disagreement would show as **the same file yielding different content
/// depending on which walker asked**.
///
/// `text_extract` and `vector` now both take it from here. `pdfcer-render`'s
/// copy is a `pub const` in another crate that its own callers name, so
/// retiring it is a separate, breaking change and is **owed rather than done**.
pub const MAX_FORM_DEPTH: usize = 64;

use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Page;
use crate::span::ByteSpan;
use crate::view::DocumentView;

/// One token of a content stream. `span` indexes the DECODED content
/// buffer the token was parsed from (see module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct ContentToken {
    /// Classification/content.
    pub kind: ContentTokenKind,
    /// Exact bytes in the decoded content buffer.
    pub span: ByteSpan,
}

/// Content-token classification.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ContentTokenKind {
    /// A complete operand: any direct object (scalar, array,
    /// dictionary), fully assembled. The span covers the whole
    /// composite (`[` through `]`).
    Operand(Object),
    /// An operator keyword (`q`, `cm`, `Tj`, … — also any
    /// unrecognized keyword; recognizing is the interpreter's job).
    /// The bytes are the span.
    Operator,
    /// A complete `BI … ID … EI` inline image (§8.9.7), as ONE token
    /// (they're a single indivisible graphics object; span covers
    /// `BI` through `EI` inclusive).
    InlineImage {
        /// Key–value pairs between `BI` and `ID`, keys AND filter/
        /// colour-space name values normalized from Table 93/94
        /// abbreviations to their full forms.
        params: Dict,
        /// Exact span of the raw (still-encoded) image data bytes.
        data: ByteSpan,
    },
}

/// A parsed content stream: the decoded bytes plus their lossless
/// token stream.
#[derive(Debug, Clone)]
pub struct ContentStream {
    /// The decoded (defiltered, concatenated) content bytes — the
    /// buffer every token span indexes.
    pub buf: Vec<u8>,
    /// The tokens, in stream order.
    pub tokens: Vec<ContentToken>,
}

/// One semantic operation: the projection's unit (module docs).
#[derive(Debug, Clone, Copy)]
pub struct Operation<'a> {
    /// The operand tokens preceding the operator, in order. Their
    /// `Operand` payloads are the arguments ("all of the operands
    /// needed by an operator shall immediately precede that
    /// operator", §7.8.2).
    pub operands: &'a [ContentToken],
    /// The operator token ([`ContentTokenKind::Operator`] — read its
    /// name via [`Operation::operator_name`] — or an
    /// [`ContentTokenKind::InlineImage`], which is its own complete
    /// operation).
    pub operator: &'a ContentToken,
}

impl<'a> Operation<'a> {
    /// The operator's keyword bytes (`b"cm"`, `b"Tj"`, …), or `None`
    /// for an inline image.
    #[must_use]
    pub fn operator_name(&self, buf: &'a [u8]) -> Option<&'a [u8]> {
        match self.operator.kind {
            ContentTokenKind::Operator => self.operator.span.slice(buf),
            _ => None,
        }
    }
}

/// Content-stream tokenization errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ContentError {
    /// Lexical failure in the decoded bytes.
    #[error(transparent)]
    Lex(#[from] crate::lexer::LexError),
    /// A container (array/dict) operand was malformed or unterminated.
    #[error("malformed operand at byte {0} of decoded content")]
    BadOperand(usize),
    /// Operand nesting exceeded the parser guard.
    #[error("operand nesting too deep at byte {0}")]
    TooDeep(usize),
    /// `BI` without a well-formed key/value section or no `ID`.
    #[error("malformed inline image parameters at byte {0}")]
    BadInlineParams(usize),
    /// The inline image's data end (`EI`) could not be located.
    #[error("unterminated inline image starting at byte {0}")]
    UnterminatedInlineImage(usize),
    /// Decoding a `Contents` stream failed.
    #[error("content stream decode failed: {0}")]
    Decode(#[from] FilterError),
    /// A page `Contents` id didn't resolve to a stream (page-tree
    /// validation normally prevents this; defensive).
    #[error("page Contents object is not a stream")]
    NotAStream,
}

/// Operand container nesting bound (pdfcer policy, ARCHITECTURE.md
/// §10 — same rationale as [`crate::parser::MAX_NESTING_DEPTH`]).
const MAX_OPERAND_DEPTH: usize = 64;

impl ContentStream {
    /// Concatenate, decode, and tokenize a page's `Contents` streams.
    ///
    /// Multiple streams concatenate in order with a single LF between
    /// parts (§7.7.3.3: the split is guaranteed to fall on a token
    /// boundary, but the boundary itself may carry no whitespace — a
    /// separator can never merge tokens and its absence could fail to
    /// separate them).
    ///
    /// ## Which document does `view` mean? (decision 018)
    ///
    /// This used to take `&Document` — always the file as loaded. It now
    /// takes a [`DocumentView`], because the answer genuinely differs by
    /// caller and the type should make the caller say which they meant:
    ///
    /// - `&doc.view()` — **the base revision**. What a one-shot
    ///   `pdfcer` operation, a text/redaction planner, or a save-time
    ///   walk wants: the bytes on disk, no session overlay.
    /// - `&session.view()` — **the edited state**. What the rasterizer and
    ///   the vector object model want, so the operator sees the page they
    ///   are actually editing.
    ///
    /// Getting this wrong is not a crash, it is the Pass 17.0 defect: the
    /// content parses fine and shows the wrong document. Every caller in
    /// the tree was audited when this signature changed, and the ones whose
    /// base-vs-session intent is not obvious from context carry a comment
    /// saying which they are and why.
    ///
    /// # Errors
    ///
    /// [`ContentError`] — decode failures or malformed content syntax.
    pub fn from_page(view: &DocumentView<'_>, page: &Page) -> Result<Self, ContentError> {
        let mut buf: Vec<u8> = Vec::new();
        for (i, id) in page.contents.iter().enumerate() {
            // `graph().value` rather than `doc.get(id).map(|io| &io.value)`:
            // the graph is the one abstraction both a `Document` and an
            // `EditSession` overlay answer, and an `IndirectObject`'s
            // provenance is of no interest to a reader (decision 018 §3).
            let obj = view.graph().value(*id).ok_or(ContentError::NotAStream)?;
            let Object::Stream(stream) = obj else {
                return Err(ContentError::NotAStream);
            };
            // `view.slice` rather than `span.slice(doc.bytes())`: a session
            // view has TWO buffers (base + R45 staging) and a content stream
            // rewritten this session — by `edit_text`, `format_text`,
            // `apply_reflow` or a 9c-min vector edit — has its payload in the
            // staging half. Indexing the base alone is exactly how those
            // edits stayed invisible.
            let raw = view
                .slice(stream.data_span)
                .ok_or(ContentError::NotAStream)?;
            let decoded = filters::decode_stream(&stream.dict, raw)?;
            // A separator only where there is something to separate. An
            // EMPTY payload is routine here: `EditSession::text_edit_command`
            // folds a multi-stream page into its first object and empties the
            // rest IN PLACE (object identity is preserved for minimal-diff
            // reasons), so a page edited this session concatenates as
            // "content" + "" + "" ... An unconditional separator would append
            // one whitespace byte per emptied stream on EVERY re-read, and a
            // session re-reads its own content on every subsequent edit -- the
            // staging buffer would grow by a byte per extra stream per edit,
            // forever. Skipping it also makes a re-read of an already-folded
            // page byte-identical to what was staged.
            if i > 0 && !decoded.is_empty() {
                buf.push(b'\n');
            }
            buf.extend_from_slice(&decoded);
        }
        Self::parse(buf)
    }

    /// One **form XObject's** own decoded content (`Pass 119.0`).
    ///
    /// A form XObject is a content stream in its own right (§8.10.1) — a
    /// separate object with separate bytes, its own `/Resources` and its own
    /// coordinate space. It is emphatically **not** part of the page's
    /// concatenated `/Contents`, which is why [`Self::from_page`] cannot reach
    /// the text inside one and why the edit surgery needed this door opened.
    ///
    /// The `view` argument carries the same base-versus-session meaning as
    /// [`Self::from_page`], and it is *more* load-bearing here: once a session
    /// can edit a form's content, a second edit to the same form must compose
    /// on top of the first, and only a session view resolves the staged value.
    ///
    /// # Errors
    ///
    /// [`ContentError::NotAStream`] when the object is absent or is not a
    /// stream; a decode or syntax error otherwise. The `/Subtype` is **not**
    /// checked here: this decodes whatever stream it is pointed at, and
    /// deciding what counts as an editable form belongs to the caller that
    /// found it (`crate::text_edit::forms`).
    pub fn from_form(view: &DocumentView<'_>, id: ObjId) -> Result<Self, ContentError> {
        let Some(Object::Stream(stream)) = view.graph().value(id) else {
            return Err(ContentError::NotAStream);
        };
        let raw = view
            .slice(stream.data_span)
            .ok_or(ContentError::NotAStream)?;
        Self::parse(filters::decode_stream(&stream.dict, raw)?)
    }

    /// Tokenize decoded content bytes into the lossless token stream.
    ///
    /// # Errors
    ///
    /// [`ContentError`] — malformed syntax; offsets are into `buf`.
    pub fn parse(buf: Vec<u8>) -> Result<Self, ContentError> {
        let mut tokens = Vec::new();
        let mut lexer = Lexer::new(&buf);
        while let Some(tok) = lexer.next_token()? {
            match tok.kind {
                TokenKind::Keyword => {
                    if tok.lexeme(&buf) == Some(b"BI") {
                        let (token, resume_at) = parse_inline_image(&buf, &tok, &mut lexer)?;
                        tokens.push(token);
                        lexer = Lexer::at(&buf, resume_at);
                    } else {
                        // true/false/null are OPERANDS even though the
                        // lexer classes them as keywords; everything
                        // else is an operator for the interpreter.
                        match tok.lexeme(&buf) {
                            Some(b"true") => tokens.push(ContentToken {
                                kind: ContentTokenKind::Operand(Object::Boolean(true)),
                                span: tok.span,
                            }),
                            Some(b"false") => tokens.push(ContentToken {
                                kind: ContentTokenKind::Operand(Object::Boolean(false)),
                                span: tok.span,
                            }),
                            Some(b"null") => tokens.push(ContentToken {
                                kind: ContentTokenKind::Operand(Object::Null),
                                span: tok.span,
                            }),
                            _ => tokens.push(ContentToken {
                                kind: ContentTokenKind::Operator,
                                span: tok.span,
                            }),
                        }
                    }
                }
                _ => {
                    let (object, span) = assemble_operand(&buf, tok, &mut lexer, 0)?;
                    tokens.push(ContentToken {
                        kind: ContentTokenKind::Operand(object),
                        span,
                    });
                }
            }
        }
        Ok(Self { buf, tokens })
    }

    /// The semantic projection: iterate operations (operand run +
    /// operator, or a standalone inline image). Purely a VIEW over
    /// the token stream — never a second representation (001 §6.1.2).
    ///
    /// Trailing operands with no operator (malformed per §7.8.2
    /// "operands shall not be left over") are simply not yielded —
    /// the tolerance every real viewer applies; the tokens remain in
    /// `self.tokens` for lossless re-emission.
    pub fn operations(&self) -> impl Iterator<Item = Operation<'_>> {
        let mut run_start = 0usize;
        self.tokens
            .iter()
            .enumerate()
            .filter_map(move |(i, tok)| match tok.kind {
                ContentTokenKind::Operand(_) => None,
                _ => {
                    let operands = self.tokens.get(run_start..i).unwrap_or(&[]);
                    run_start = i + 1;
                    Some(Operation {
                        operands,
                        operator: tok,
                    })
                }
            })
    }
}

/// Assemble one complete operand object starting from `first`,
/// consuming container contents from the lexer. Returns the object and
/// its full composite span.
fn assemble_operand(
    buf: &[u8],
    first: Token,
    lexer: &mut Lexer<'_>,
    depth: usize,
) -> Result<(Object, ByteSpan), ContentError> {
    if depth > MAX_OPERAND_DEPTH {
        return Err(ContentError::TooDeep(first.span.start));
    }
    let start = first.span.start;
    match first.kind {
        TokenKind::Integer(v) => Ok((Object::Integer(v), first.span)),
        TokenKind::Real(v) => Ok((Object::Real(v), first.span)),
        TokenKind::String(s) => Ok((Object::String(s), first.span)),
        TokenKind::Name(n) => Ok((Object::Name(Name(n)), first.span)),
        TokenKind::ArrayOpen => {
            let mut items = Vec::new();
            loop {
                let Some(tok) = lexer.next_token()? else {
                    return Err(ContentError::BadOperand(start));
                };
                match tok.kind {
                    TokenKind::ArrayClose => {
                        return Ok((
                            Object::Array(items),
                            ByteSpan::from_range(start..tok.span.end()),
                        ));
                    }
                    TokenKind::Keyword => match tok.lexeme(buf) {
                        Some(b"true") => items.push(Object::Boolean(true)),
                        Some(b"false") => items.push(Object::Boolean(false)),
                        Some(b"null") => items.push(Object::Null),
                        // An operator keyword inside an array operand
                        // is malformed (§7.8.2 grammar).
                        _ => return Err(ContentError::BadOperand(tok.span.start)),
                    },
                    _ => {
                        let (obj, _) = assemble_operand(buf, tok, lexer, depth + 1)?;
                        items.push(obj);
                    }
                }
            }
        }
        TokenKind::DictOpen => {
            let mut dict = Dict::new();
            loop {
                let Some(tok) = lexer.next_token()? else {
                    return Err(ContentError::BadOperand(start));
                };
                match tok.kind {
                    TokenKind::DictClose => {
                        return Ok((
                            Object::Dict(dict),
                            ByteSpan::from_range(start..tok.span.end()),
                        ));
                    }
                    TokenKind::Name(key) => {
                        let Some(value_tok) = lexer.next_token()? else {
                            return Err(ContentError::BadOperand(start));
                        };
                        let value = match value_tok.kind {
                            TokenKind::Keyword => match value_tok.lexeme(buf) {
                                Some(b"true") => Object::Boolean(true),
                                Some(b"false") => Object::Boolean(false),
                                Some(b"null") => Object::Null,
                                _ => return Err(ContentError::BadOperand(value_tok.span.start)),
                            },
                            _ => assemble_operand(buf, value_tok, lexer, depth + 1)?.0,
                        };
                        dict.insert(Name(key), value);
                    }
                    _ => return Err(ContentError::BadOperand(tok.span.start)),
                }
            }
        }
        // Braces (Type 4 function syntax) and stray closers are
        // malformed at operand position in a content stream.
        _ => Err(ContentError::BadOperand(start)),
    }
}

// ---------------------------------------------------------------------------
// Inline images (§8.9.7)
// ---------------------------------------------------------------------------

/// Parse a complete inline image. `bi` is the already-consumed `BI`
/// operator token. Returns the token and the buffer offset to resume
/// lexing at (just past `EI`).
fn parse_inline_image(
    buf: &[u8],
    bi: &Token,
    lexer: &mut Lexer<'_>,
) -> Result<(ContentToken, usize), ContentError> {
    // --- key/value pairs until ID (bare pairs, not a dictionary) ---
    let mut params = Dict::new();
    let id_end;
    loop {
        let Some(tok) = lexer.next_token()? else {
            return Err(ContentError::BadInlineParams(bi.span.start));
        };
        match tok.kind {
            TokenKind::Keyword if tok.lexeme(buf) == Some(b"ID") => {
                id_end = tok.span.end();
                break;
            }
            TokenKind::Name(key) => {
                let Some(value_tok) = lexer.next_token()? else {
                    return Err(ContentError::BadInlineParams(bi.span.start));
                };
                let value = match value_tok.kind {
                    TokenKind::Keyword => match value_tok.lexeme(buf) {
                        Some(b"true") => Object::Boolean(true),
                        Some(b"false") => Object::Boolean(false),
                        Some(b"null") => Object::Null,
                        _ => return Err(ContentError::BadInlineParams(value_tok.span.start)),
                    },
                    _ => {
                        assemble_operand(buf, value_tok, lexer, 0)
                            .map_err(|_| ContentError::BadInlineParams(bi.span.start))?
                            .0
                    }
                };
                let key = normalize_key(&key);
                let value = normalize_value(&key, value);
                params.insert(key, value);
            }
            _ => return Err(ContentError::BadInlineParams(tok.span.start)),
        }
    }

    // --- data start: ID + ONE whitespace character (§8.9.7's byte
    // rule; for AHx/A85 the spec relaxes this, but a single skipped
    // whitespace is still correct there since those filters ignore
    // whitespace anyway) ---
    //
    // "ONE whitespace CHARACTER" is not the same as "one whitespace
    // BYTE", and the difference is load-bearing. §7.2.2: "CR (0Dh) and
    // LF (0Ah) are both EOL markers. CR immediately followed by LF =
    // ONE EOL marker." So a producer that writes `ID\r\n` has written
    // `ID` plus a single white-space character, exactly as §8.9.7
    // requires — and it is the common habit, because it is the same
    // framing §7.3.8.1 mandates after the `stream` keyword.
    //
    // Consuming only the CR leaves a stray LF at the head of the image
    // data, which every non-ASCII filter then chokes on: a JPEG that
    // does not start with `FF D8`, a zlib stream with a garbage header,
    // an LZW stream whose first code is shifted. Measured on the
    // veraPDF corpus (2026-07-30): four inline DCT images failed with
    // "codestream does not begin with SOI" for exactly this reason.
    // CR alone and LF alone are still one character each.
    let data_start = match (buf.get(id_end).copied(), buf.get(id_end + 1).copied()) {
        (Some(b'\r'), Some(b'\n')) => id_end + 2,
        (Some(b), _) if crate::lexer::is_whitespace(b) => id_end + 1,
        _ => id_end,
    };

    // --- data end: the strategy ladder (module docs) ---
    let data_end = locate_inline_data_end(buf, data_start, &params)
        .ok_or(ContentError::UnterminatedInlineImage(bi.span.start))?;

    // --- EI ---
    let mut tail = Lexer::at(buf, data_end);
    let Ok(Some(ei)) = tail.next_token() else {
        return Err(ContentError::UnterminatedInlineImage(bi.span.start));
    };
    if !(matches!(ei.kind, TokenKind::Keyword) && ei.lexeme(buf) == Some(b"EI")) {
        return Err(ContentError::UnterminatedInlineImage(bi.span.start));
    }

    Ok((
        ContentToken {
            kind: ContentTokenKind::InlineImage {
                params,
                data: ByteSpan::from_range(data_start..data_end),
            },
            span: ByteSpan::from_range(bi.span.start..ei.span.end()),
        },
        ei.span.end(),
    ))
}

/// Find the end of inline image data per the strategy ladder.
fn locate_inline_data_end(buf: &[u8], start: usize, params: &Dict) -> Option<usize> {
    let filters = inline_filters(params);

    if filters.is_empty() {
        // Case 1: unfiltered — compute, don't scan.
        if let Some(len) = unfiltered_data_len(params) {
            let end = start.checked_add(len)?;
            return (end <= buf.len()).then_some(end);
        }
        // Missing W/H/BPC: malformed, but fall through to the scan so
        // the error becomes "unterminated" only if EI truly absent.
    } else if let Some(first) = filters.first() {
        // Case 2: ASCII filters self-terminate.
        let region = buf.get(start..)?;
        match first.as_slice() {
            b"ASCIIHexDecode" => {
                let eod = region.iter().position(|&b| b == b'>')?;
                return Some(start + eod + 1);
            }
            b"ASCII85Decode" => {
                let eod = region.windows(2).position(|w| w == b"~>")?;
                return Some(start + eod + 2);
            }
            _ => {}
        }
    }

    // Case 3 fallback (NOT spec-sourced — module docs): scan for `EI`
    // preceded by whitespace and followed by whitespace/delimiter/EOF.
    let mut i = start;
    while i + 1 < buf.len() {
        if buf.get(i) == Some(&b'E')
            && buf.get(i + 1) == Some(&b'I')
            && i > start
            && buf
                .get(i - 1)
                .copied()
                .is_some_and(crate::lexer::is_whitespace)
        {
            let after = buf.get(i + 2).copied();
            let terminated = match after {
                None => true,
                Some(b) => crate::lexer::is_whitespace(b) || crate::lexer::is_delimiter(b),
            };
            if terminated {
                // Return the offset of the whitespace before EI so the
                // caller's lexer re-lexes EI itself.
                return Some(i - 1);
            }
        }
        i += 1;
    }
    None
}

/// Byte length of unfiltered inline data:
/// `ceil(W × colors × BPC / 8) × H` (§8.9.7 case 1; row-padded per
/// §7.4.4.4 rule 2's shared convention for sampled data).
fn unfiltered_data_len(params: &Dict) -> Option<usize> {
    let get_u64 = |key: &[u8]| -> Option<u64> {
        params
            .get(key)
            .and_then(Object::as_int)
            .and_then(|v| u64::try_from(v).ok())
    };
    let w = get_u64(b"Width")?;
    let h = get_u64(b"Height")?;
    let image_mask = matches!(params.get(b"ImageMask"), Some(Object::Boolean(true)));
    let bpc = if image_mask {
        1
    } else {
        get_u64(b"BitsPerComponent")?
    };
    let colors = if image_mask {
        1
    } else {
        match params.get(b"ColorSpace") {
            Some(Object::Name(n)) => match n.as_bytes() {
                b"DeviceRGB" => 3,
                b"DeviceCMYK" => 4,
                // DeviceGray, Indexed (1 index component), and named
                // resource spaces (component count unknown → treat as
                // 1 and let the scan fallback correct a mismatch).
                _ => 1,
            },
            Some(Object::Array(_)) => 1, // Indexed [ /I base hival lookup ]
            _ => 1,
        }
    };
    let row_bytes = (w.checked_mul(colors)?.checked_mul(bpc)?).div_ceil(8);
    usize::try_from(row_bytes.checked_mul(h)?).ok()
}

/// The inline image's filter chain as full (normalized) names.
fn inline_filters(params: &Dict) -> Vec<Vec<u8>> {
    match params.get(b"Filter") {
        Some(Object::Name(n)) => vec![n.as_bytes().to_vec()],
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(|o| o.as_name().map(|n| n.as_bytes().to_vec()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Normalize a Table 93 abbreviated KEY to its full name.
fn normalize_key(key: &[u8]) -> Name {
    Name(
        match key {
            b"BPC" => &b"BitsPerComponent"[..],
            b"CS" => b"ColorSpace",
            b"D" => b"Decode",
            b"DP" => b"DecodeParms",
            b"F" => b"Filter",
            b"H" => b"Height",
            b"IM" => b"ImageMask",
            b"I" => b"Interpolate", // key position ⇒ Interpolate, not Indexed
            b"W" => b"Width",
            other => other,
        }
        .to_vec(),
    )
}

/// Normalize Table 94 abbreviated VALUES — only where they can occur
/// (`Filter` and `ColorSpace` values, including inside arrays). `I`
/// here means `Indexed` (value position), disambiguated from the
/// `Interpolate` KEY by context exactly as the RAG's gotcha requires.
fn normalize_value(key: &Name, value: Object) -> Object {
    fn map_name(bytes: &[u8]) -> Option<&'static [u8]> {
        Some(match bytes {
            b"G" => b"DeviceGray",
            b"RGB" => b"DeviceRGB",
            b"CMYK" => b"DeviceCMYK",
            b"I" => b"Indexed",
            b"AHx" => b"ASCIIHexDecode",
            b"A85" => b"ASCII85Decode",
            b"LZW" => b"LZWDecode",
            b"Fl" => b"FlateDecode",
            b"RL" => b"RunLengthDecode",
            b"CCF" => b"CCITTFaxDecode",
            b"DCT" => b"DCTDecode",
            _ => return None,
        })
    }
    if !matches!(key.as_bytes(), b"Filter" | b"ColorSpace") {
        return value;
    }
    match value {
        Object::Name(n) => match map_name(n.as_bytes()) {
            Some(full) => Object::Name(Name(full.to_vec())),
            None => Object::Name(n),
        },
        Object::Array(items) => Object::Array(
            items
                .into_iter()
                .map(|o| match o {
                    Object::Name(n) => match map_name(n.as_bytes()) {
                        Some(full) => Object::Name(Name(full.to_vec())),
                        None => Object::Name(n),
                    },
                    other => other,
                })
                .collect(),
        ),
        other => other,
    }
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

    fn parse(input: &[u8]) -> ContentStream {
        ContentStream::parse(input.to_vec()).unwrap()
    }

    /// Collect (operator name, operand objects) pairs.
    fn ops(cs: &ContentStream) -> Vec<(Vec<u8>, Vec<Object>)> {
        cs.operations()
            .map(|op| {
                let name = op.operator_name(&cs.buf).unwrap_or(b"<inline>").to_vec();
                let operands = op
                    .operands
                    .iter()
                    .filter_map(|t| match &t.kind {
                        ContentTokenKind::Operand(o) => Some(o.clone()),
                        _ => None,
                    })
                    .collect();
                (name, operands)
            })
            .collect()
    }

    #[test]
    fn basic_operations_projection() {
        let cs = parse(b"q 1 0 0 1 72 712 cm BT /F1 12 Tf (Hello) Tj ET Q");
        let ops = ops(&cs);
        let names: Vec<&[u8]> = ops.iter().map(|(n, _)| n.as_slice()).collect();
        assert_eq!(
            names,
            vec![&b"q"[..], b"cm", b"BT", b"Tf", b"Tj", b"ET", b"Q"]
        );
        // cm has six numeric operands.
        assert_eq!(ops[1].1.len(), 6);
        // Tf: name + number.
        assert_eq!(ops[3].1[0], Object::Name(Name::from(b"F1")));
        // Tj: the string.
        assert_eq!(ops[4].1[0], Object::String(b"Hello".to_vec()));
    }

    #[test]
    fn tj_array_is_one_operand_with_full_span() {
        let buf: &[u8] = b"[(He)-20(llo)] TJ";
        let cs = parse(buf);
        let ops = ops(&cs);
        assert_eq!(ops[0].0, b"TJ");
        assert_eq!(ops[0].1.len(), 1, "the TJ array is ONE operand");
        // Lossless span: the operand token covers the whole array.
        let ContentTokenKind::Operand(_) = &cs.tokens[0].kind else {
            panic!("expected operand");
        };
        assert_eq!(cs.tokens[0].span.slice(buf).unwrap(), b"[(He)-20(llo)]");
    }

    #[test]
    fn dict_operand_for_marked_content() {
        let cs = parse(b"/OC << /Type /OCMD >> BDC EMC");
        let ops = ops(&cs);
        assert_eq!(ops[0].0, b"BDC");
        assert_eq!(ops[0].1.len(), 2);
        assert!(matches!(ops[0].1[1], Object::Dict(_)));
    }

    #[test]
    fn keywords_true_false_null_are_operands_not_operators() {
        let cs = parse(b"true false null gs");
        let ops = ops(&cs);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, b"gs");
        assert_eq!(ops[0].1.len(), 3);
    }

    #[test]
    fn trailing_operands_without_operator_are_not_yielded() {
        // Malformed per §7.8.2; tolerated per module docs — tokens
        // stay recorded, projection just doesn't yield an operation.
        let cs = parse(b"q Q 1 2 3");
        assert_eq!(ops(&cs).len(), 2);
        assert_eq!(cs.tokens.len(), 5, "operand tokens preserved");
    }

    #[test]
    fn unknown_operator_is_a_token_not_an_error() {
        // Recognition is the interpreter's job (BX/EX, log-and-skip).
        let cs = parse(b"1 2 frobnicate");
        assert_eq!(ops(&cs)[0].0, b"frobnicate");
    }

    #[test]
    fn inline_image_unfiltered_computed_length() {
        // 2×2 grayscale 8bpc ⇒ exactly 4 data bytes — which happen to
        // spell "EI x" garbage-free; the COMPUTED path must not scan.
        // Data bytes deliberately contain "EI" to prove no scanning.
        let buf: &[u8] = b"BI /W 2 /H 2 /CS /G /BPC 8 ID \x45\x49\x00\xFF EI Q";
        let cs = parse(buf);
        let ContentTokenKind::InlineImage { params, data } = &cs.tokens[0].kind else {
            panic!("expected inline image, got {:?}", cs.tokens[0].kind);
        };
        assert_eq!(data.slice(buf).unwrap(), b"\x45\x49\x00\xFF");
        // Keys and CS value normalized to full names.
        assert!(params.contains_key(b"Width"));
        assert_eq!(
            params
                .get(b"ColorSpace")
                .unwrap()
                .as_name()
                .unwrap()
                .as_bytes(),
            b"DeviceGray"
        );
        // The Q after EI still lexes.
        assert_eq!(ops(&cs).last().unwrap().0, b"Q");
        // The token's own span covers BI..EI for verbatim re-emission.
        assert!(cs.tokens[0].span.slice(buf).unwrap().starts_with(b"BI"));
        assert!(cs.tokens[0].span.slice(buf).unwrap().ends_with(b"EI"));
    }

    #[test]
    fn id_followed_by_crlf_consumes_both_bytes() {
        // §8.9.7 requires ONE white-space CHARACTER after `ID`, and
        // §7.2.2 makes "CR immediately followed by LF" exactly one EOL
        // marker — the same reading §7.3.8.1 gives the `stream`
        // keyword, and the framing real producers actually emit.
        //
        // Consuming only the CR leaves a stray LF at the head of the
        // image data. Measured on the veraPDF corpus (2026-07-30): four
        // inline DCT images failed with "codestream does not begin with
        // SOI" for precisely this reason, so this is a regression test
        // for a real corpus finding, not a hypothetical.
        let buf: &[u8] = b"BI /W 2 /H 2 /CS /G /BPC 8 ID\r\n\x01\x02\x03\x04 EI Q";
        let cs = parse(buf);
        let ContentTokenKind::InlineImage { data, .. } = &cs.tokens[0].kind else {
            panic!("expected inline image, got {:?}", cs.tokens[0].kind);
        };
        assert_eq!(data.slice(buf).unwrap(), b"\x01\x02\x03\x04");
    }

    #[test]
    fn id_followed_by_a_single_whitespace_consumes_exactly_one_byte() {
        // The other half of the rule: only a CR *immediately followed
        // by* LF is one marker. Every other single white-space
        // character consumes exactly one byte, so a first data byte
        // that happens to be 0x0A must not be swallowed.
        //
        // The lone-CR case is deliberately tested with a non-LF first
        // data byte, because `ID\r` + a 0x0A data byte is genuinely
        // ambiguous in the format itself — §7.2.2 resolves it in favour
        // of CRLF being one marker, which is exactly why §7.3.8.1
        // forbids CR alone after the `stream` keyword. There is no
        // reading that recovers that data byte, and pretending
        // otherwise would be a test asserting a fiction.
        for (eol, first) in [
            (&b"\r"[..], 0x01u8),
            (b"\n", 0x0A),
            (b" ", 0x0A),
            (b"\t", 0x0A),
        ] {
            let mut buf = b"BI /W 2 /H 2 /CS /G /BPC 8 ID".to_vec();
            buf.extend_from_slice(eol);
            buf.extend_from_slice(&[first, 0x02, 0x03, 0x04]);
            buf.extend_from_slice(b" EI Q");
            let cs = parse(&buf);
            let ContentTokenKind::InlineImage { data, .. } = &cs.tokens[0].kind else {
                panic!("expected inline image");
            };
            assert_eq!(
                data.slice(&buf).unwrap(),
                &[first, 0x02, 0x03, 0x04],
                "eol {eol:?} must consume exactly one byte"
            );
        }
    }

    #[test]
    fn inline_image_ascii_hex_self_terminates() {
        let buf: &[u8] = b"BI /W 1 /H 1 /CS /G /BPC 8 /F /AHx ID FF> EI";
        let cs = parse(buf);
        let ContentTokenKind::InlineImage { params, data } = &cs.tokens[0].kind else {
            panic!("expected inline image");
        };
        assert_eq!(data.slice(buf).unwrap(), b"FF>");
        assert_eq!(
            params.get(b"Filter").unwrap().as_name().unwrap().as_bytes(),
            b"ASCIIHexDecode"
        );
    }

    #[test]
    fn inline_image_filtered_falls_back_to_ei_scan() {
        // Fl-filtered data of unknown length: whitespace-delimited EI
        // scan (the documented non-spec heuristic).
        let buf: &[u8] = b"BI /W 1 /H 1 /CS /G /BPC 8 /F /Fl ID \x78\x9c\x63\x00\x00 EI";
        let cs = parse(buf);
        let ContentTokenKind::InlineImage { data, .. } = &cs.tokens[0].kind else {
            panic!("expected inline image");
        };
        assert_eq!(data.slice(buf).unwrap(), b"\x78\x9c\x63\x00\x00");
    }

    #[test]
    fn inline_image_interpolate_vs_indexed_disambiguation() {
        // /I in KEY position = Interpolate; /I in ColorSpace VALUE
        // position (inside the Indexed array) = Indexed.
        let buf: &[u8] = b"BI /W 1 /H 1 /BPC 8 /I true /CS [/I /RGB 0 <000000>] ID \x00 EI";
        let cs = parse(buf);
        let ContentTokenKind::InlineImage { params, .. } = &cs.tokens[0].kind else {
            panic!("expected inline image");
        };
        assert_eq!(params.get(b"Interpolate").unwrap(), &Object::Boolean(true));
        let Object::Array(cs_arr) = params.get(b"ColorSpace").unwrap() else {
            panic!("Indexed array expected");
        };
        assert_eq!(cs_arr[0].as_name().unwrap().as_bytes(), b"Indexed");
        assert_eq!(cs_arr[1].as_name().unwrap().as_bytes(), b"DeviceRGB");
    }

    #[test]
    fn unterminated_inline_image_is_error() {
        let e =
            ContentStream::parse(b"BI /W 9 /H 9 /CS /G /BPC 8 ID \x00\x01".to_vec()).unwrap_err();
        assert!(matches!(e, ContentError::UnterminatedInlineImage(_)));
    }

    #[test]
    fn reference_syntax_in_content_is_just_an_unknown_operator() {
        // §7.8.2 bans indirect references; `1 0 R` lexes as two
        // operands + operator R, which the interpreter will reject.
        let cs = parse(b"1 0 R");
        let ops = ops(&cs);
        assert_eq!(ops[0].0, b"R");
        assert_eq!(ops[0].1.len(), 2);
    }

    #[test]
    fn spans_recover_exact_source_for_every_token() {
        let buf: &[u8] = b"q 0.5 0 0 0.5 0 0 cm /Im1 Do Q";
        let cs = parse(buf);
        // Reconstructing the stream from token spans + inter-token
        // whitespace is the round-trip foundation; here just verify
        // each token's span slices cleanly and in order.
        let mut last_end = 0;
        for t in &cs.tokens {
            assert!(t.span.start >= last_end);
            assert!(t.span.slice(buf).is_some());
            last_end = t.span.end();
        }
    }
}
