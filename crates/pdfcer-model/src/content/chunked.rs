//! Windowed reading of content: [`ContentChunks`].
//!
//! The renderer reads a page's `/Contents` (or a form, pattern or glyph
//! procedure) through this rather than [`ContentStream::from_page`], so a
//! content stream of any size is decoded and tokenized a window at a time
//! and never held whole.
//!
//! Each window ends just after an operator or inline image, so no
//! operation straddles two windows and a consumer that runs each window's
//! [`ContentStream::operations`] in turn sees exactly the operations a
//! whole parse would yield. Tokens ending within a few bytes of the bytes
//! read so far are held back to the next window, because a number, name or
//! `EI` cut by the window edge looks complete.

use super::{ContentError, ContentStream, ContentTokenKind, tokenize};
use crate::filters::{ReadStatus, StreamDecoder};
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::view::DocumentView;

/// Bytes aimed for per window.
const WINDOW: usize = 4 << 20;
/// Largest window tried before a parse error or operator-less run is final.
const MAX_WINDOW: usize = 256 << 20;
/// Tokens ending this close to the bytes read so far wait for more.
const MARGIN: usize = 16;

/// Why a [`ContentChunks`] stopped before the end of its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChunkTruncation {
    /// A stream decoded past its budget
    /// ([`crate::filters::MAX_DECODE_RATIO`] times its encoded size).
    Budget,
    /// No complete operator within the largest window.
    Overlong,
}

/// A content stream read a window at a time (module docs).
///
/// # Examples
///
/// ```
/// use pdfcer_model::content::ContentChunks;
/// use pdfcer_model::object::Dict;
///
/// let dict = Dict::new();
/// let mut chunks = ContentChunks::from_stream(&dict, b"0 0 m 10 10 l S");
/// let mut ops = 0;
/// while let Some(window) = chunks.next_chunk()? {
///     ops += window.operations().count();
/// }
/// assert_eq!(ops, 3);
/// # Ok::<(), pdfcer_model::content::ContentError>(())
/// ```
#[derive(Debug)]
pub struct ContentChunks<'a> {
    sources: Vec<(&'a Dict, &'a [u8])>,
    next: usize,
    cur: Option<StreamDecoder<'a>>,
    started: bool,
    buf: Vec<u8>,
    window: usize,
    ended: bool,
    pending: Option<ContentError>,
    truncation: Option<ChunkTruncation>,
    primed: Option<ContentStream>,
    emitted: usize,
    floor: usize,
}

impl<'a> ContentChunks<'a> {
    /// A page's `/Contents`, concatenated exactly as
    /// [`ContentStream::from_page`] concatenates them.
    ///
    /// # Errors
    ///
    /// [`ContentError::NotAStream`] when a `/Contents` entry does not
    /// resolve to a stream. Decoding has not started yet.
    pub fn from_page(view: &DocumentView<'a>, page: &Page) -> Result<Self, ContentError> {
        let sources = page
            .contents
            .iter()
            .map(|id| stream_of(view, *id))
            .collect::<Result<_, _>>()?;
        Ok(Self::new(sources))
    }

    /// A form XObject's own content, as [`ContentStream::from_form`] reads it.
    ///
    /// # Errors
    ///
    /// [`ContentError::NotAStream`] when `id` is absent or not a stream.
    pub fn from_form(view: &DocumentView<'a>, id: ObjId) -> Result<Self, ContentError> {
        Ok(Self::new(vec![stream_of(view, id)?]))
    }

    /// One stream given by its dictionary and encoded bytes.
    #[must_use]
    pub fn from_stream(dict: &'a Dict, raw: &'a [u8]) -> Self {
        Self::new(vec![(dict, raw)])
    }

    fn new(sources: Vec<(&'a Dict, &'a [u8])>) -> Self {
        Self {
            sources,
            next: 0,
            cur: None,
            started: false,
            buf: Vec::new(),
            window: WINDOW,
            ended: false,
            pending: None,
            truncation: None,
            primed: None,
            emitted: 0,
            floor: crate::filters::MAX_DECODED_LEN,
        }
    }

    /// Read the first window now, so a caller can report an error in it
    /// before committing to the content; [`Self::next_chunk`] then returns
    /// that window first.
    ///
    /// # Errors
    ///
    /// As [`Self::next_chunk`] for its first window.
    pub fn prime(&mut self) -> Result<(), ContentError> {
        if self.primed.is_none() && self.emitted == 0 {
            self.primed = self.next_chunk()?;
        }
        Ok(())
    }

    /// Why reading stopped early, once it has.
    #[must_use]
    pub fn truncation(&self) -> Option<ChunkTruncation> {
        self.truncation
    }

    /// The next window, or `None` after the last.
    ///
    /// # Errors
    ///
    /// A decode or syntax error. In the first window every error is
    /// returned, as [`ContentStream::from_page`] would return it. After
    /// that, a decode error ends the content: the windows decoded before it
    /// are returned first, then the error once.
    pub fn next_chunk(&mut self) -> Result<Option<ContentStream>, ContentError> {
        if let Some(primed) = self.primed.take() {
            return Ok(Some(primed));
        }
        let mut window = self.window;
        loop {
            self.fill(window);
            if self.emitted == 0
                && let Some(e) = self.pending.take()
            {
                self.buf.clear();
                return Err(e);
            }
            if self.buf.is_empty() && self.ended {
                return self.pending.take().map_or(Ok(None), Err);
            }
            let complete = self.ended && self.pending.is_none() && self.truncation.is_none();
            let mut tokens = Vec::new();
            let parsed = tokenize(&self.buf, &mut tokens, 0);
            if complete {
                let buf = std::mem::take(&mut self.buf);
                parsed?;
                self.emitted += 1;
                return Ok(Some(ContentStream { buf, tokens }));
            }
            if let Some(chunk) = self.cut(tokens, window) {
                self.emitted += 1;
                return Ok(Some(chunk));
            }
            if self.ended {
                self.buf.clear();
            } else if window >= MAX_WINDOW {
                self.ended = true;
                self.buf.clear();
                parsed?;
                self.truncation = Some(ChunkTruncation::Overlong);
            } else {
                window = window.max(self.buf.len()).saturating_mul(2);
            }
        }
    }

    /// Split off the bytes and tokens up to the last operator that ends at
    /// or before `window` (or the first beyond it), outside the margin.
    fn cut(
        &mut self,
        mut tokens: Vec<super::ContentToken>,
        window: usize,
    ) -> Option<ContentStream> {
        let limit = self.buf.len().saturating_sub(MARGIN);
        let ends = tokens
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    ContentTokenKind::Operator | ContentTokenKind::InlineImage { .. }
                )
            })
            .map(|t| t.span.end())
            .take_while(|&end| end <= limit);
        let mut cut = None;
        for end in ends {
            if cut.is_some() && end > window {
                break;
            }
            cut = Some(end);
        }
        let cut = cut?;
        tokens.retain(|t| t.span.end() <= cut);
        let rest = self.buf.split_off(cut);
        let buf = std::mem::replace(&mut self.buf, rest);
        Some(ContentStream { buf, tokens })
    }

    /// Decode until `buf` holds `window` bytes or the content ends.
    fn fill(&mut self, window: usize) {
        while self.buf.len() < window && !self.ended {
            if self.cur.is_none() && !self.open_next() {
                break;
            }
            let Some(dec) = self.cur.as_mut() else { break };
            let before = self.buf.len();
            let status = dec.read(&mut self.buf);
            if self.buf.len() > before && !self.started {
                self.started = true;
                // ContentStream::from_page's separator, only where there
                // is something to separate.
                if self.next > 1 {
                    self.buf.insert(before, b'\n');
                }
            }
            match status {
                Ok(ReadStatus::BudgetSpent) => {
                    self.truncation = Some(ChunkTruncation::Budget);
                    self.ended = true;
                }
                Ok(ReadStatus::End) => self.cur = None,
                Ok(_) => {}
                Err(e) => {
                    self.pending = Some(e.into());
                    self.ended = true;
                }
            }
        }
    }

    /// Start the next source; `false` (and ended) when there is none.
    fn open_next(&mut self) -> bool {
        let Some(&(dict, raw)) = self.sources.get(self.next) else {
            self.ended = true;
            return false;
        };
        self.next += 1;
        self.started = false;
        match StreamDecoder::with_floor(dict, raw, self.floor) {
            Ok(dec) => {
                self.cur = Some(dec);
                true
            }
            Err(e) => {
                self.pending = Some(e.into());
                self.ended = true;
                false
            }
        }
    }
}

fn stream_of<'a>(view: &DocumentView<'a>, id: ObjId) -> Result<(&'a Dict, &'a [u8]), ContentError> {
    let Some(Object::Stream(stream)) = view.graph().value(id) else {
        return Err(ContentError::NotAStream);
    };
    let raw = view
        .slice(stream.data_span)
        .ok_or(ContentError::NotAStream)?;
    Ok((&stream.dict, raw))
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
    use crate::filters::flate;
    use crate::object::Name;

    /// Every operation's meaning (operand values, operator bytes, inline
    /// image parameters and data), which a window split must not change.
    fn ops_of(windows: &[ContentStream]) -> Vec<Vec<String>> {
        windows
            .iter()
            .flat_map(|w| {
                w.operations().map(move |op| {
                    op.operands
                        .iter()
                        .chain(std::iter::once(op.operator))
                        .map(|t| match &t.kind {
                            ContentTokenKind::Operand(o) => format!("{o:?}"),
                            ContentTokenKind::Operator => {
                                format!("{:?}", t.span.slice(&w.buf).unwrap())
                            }
                            ContentTokenKind::InlineImage { params, data } => {
                                format!("{params:?} {:?}", data.slice(&w.buf).unwrap())
                            }
                        })
                        .collect()
                })
            })
            .collect()
    }

    fn drain(mut c: ContentChunks<'_>) -> Vec<ContentStream> {
        let mut out = Vec::new();
        while let Some(w) = c.next_chunk().unwrap() {
            out.push(w);
        }
        out
    }

    fn sample() -> Vec<u8> {
        let mut s = Vec::new();
        for i in 0..300u32 {
            s.extend_from_slice(
                format!("q {i} 0 0 1 0 0 cm /F1 12 Tf ({i}) Tj [<0041> -{i} (b)] TJ Q\n")
                    .as_bytes(),
            );
            if i % 97 == 0 {
                s.extend_from_slice(b"BI /W 2 /H 1 /CS /G /BPC 8 ID \x00\xFF EI\n");
            }
        }
        s
    }

    /// Whether `at` falls inside a `BI … EI` line of [`sample`], where a
    /// stream split would not be on a token boundary.
    fn in_inline_image(s: &[u8], at: usize) -> bool {
        let line = s[..at]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        s[line..].starts_with(b"BI") && !s[line..at].ends_with(b"EI\n")
    }

    fn flate_dict() -> Dict {
        let mut d = Dict::new();
        d.insert(
            Name::from(b"Filter".as_slice()),
            Object::Name(Name::from(b"FlateDecode".as_slice())),
        );
        d
    }

    #[test]
    fn windows_are_invisible_at_every_size() {
        let plain = sample();
        let whole = ContentStream::parse(plain.clone()).unwrap();
        let expected = ops_of(std::slice::from_ref(&whole));
        let enc = flate::encode(&plain);
        let dict = flate_dict();
        for window in [1, 2, 7, 4096] {
            let mut c = ContentChunks::from_stream(&dict, &enc);
            c.window = window;
            let windows = drain(c);
            assert!(windows.len() > 1 || window > plain.len(), "window {window}");
            assert_eq!(ops_of(&windows), expected, "window {window}");
        }
    }

    #[test]
    fn an_operator_cut_by_the_read_edge_waits_for_its_tail() {
        // Unfiltered bytes arrive a decoder chunk at a time, so the first
        // read ends exactly between the `B` and `T` of `BT`.
        let mut plain = vec![b' '; crate::filters::CHUNK - 1];
        plain.extend_from_slice(
            b"BT /F1 12 Tf ET
",
        );
        let expected = ops_of(&[ContentStream::parse(plain.clone()).unwrap()]);
        let dict = Dict::new();
        let mut c = ContentChunks::from_stream(&dict, &plain);
        c.window = 4096;
        assert_eq!(ops_of(&drain(c)), expected);
    }

    #[test]
    fn streams_are_separated_even_without_trailing_whitespace() {
        let dict = Dict::new();
        let sources: Vec<(&Dict, &[u8])> =
            vec![(&dict, b"0 0 m"), (&dict, b""), (&dict, b"1 1 l S")];
        let expected = ops_of(&[ContentStream::parse(
            b"0 0 m
1 1 l S"
                .to_vec(),
        )
        .unwrap()]);
        assert_eq!(ops_of(&drain(ContentChunks::new(sources))), expected);
    }

    #[test]
    fn split_contents_streams_concatenate_like_from_page() {
        let plain = sample();
        let expected = ops_of(&[ContentStream::parse(plain.clone()).unwrap()]);
        let empty = Dict::new();
        for part in [1usize, 2, 7, 4096] {
            // Split on whitespace boundaries only (§7.7.3.3 guarantees token
            // boundaries; a split inside a token is not a valid page).
            let mut sources: Vec<(&Dict, &[u8])> = Vec::new();
            let mut start = 0;
            while start < plain.len() {
                let mut end = (start + part).min(plain.len());
                while end < plain.len()
                    && (!crate::lexer::is_whitespace(plain[end - 1])
                        || in_inline_image(&plain, end))
                {
                    end += 1;
                }
                sources.push((&empty, &plain[start..end]));
                sources.push((&empty, b""));
                start = end;
            }
            let windows = drain(ContentChunks::new(sources));
            assert_eq!(ops_of(&windows), expected, "part {part}");
        }
    }

    #[test]
    fn a_bomb_draws_its_prefix_and_reports_truncation() {
        let line = b"0 0 m 1 1 l S\n";
        let plain = line.repeat((8 << 20) / line.len());
        let enc = flate::encode(&plain);
        let floor = 1 << 20;
        let budget = floor.max(enc.len() * crate::filters::MAX_DECODE_RATIO);
        assert!(budget < plain.len(), "the fixture must be a bomb");
        let dict = flate_dict();
        let mut c = ContentChunks::from_stream(&dict, &enc);
        c.floor = floor;
        let mut ops = 0usize;
        while let Some(w) = c.next_chunk().unwrap() {
            ops += w.operations().count();
        }
        assert_eq!(c.truncation(), Some(ChunkTruncation::Budget));
        // Every whole line inside the budget ran; nothing past it did.
        let lines = budget / line.len();
        assert!(
            ops >= (lines - 2) * 3 && ops <= lines * 3,
            "{ops} vs {lines} lines"
        );
    }

    #[test]
    fn a_later_decode_error_follows_the_windows_before_it() {
        // Large enough that the damage lies past the first decoded chunk.
        let plain = sample().repeat(16);
        let enc = flate::encode(&plain);
        let cut = &enc[..enc.len() * 3 / 4];
        let dict = flate_dict();
        // A whole-stream read reports the damage up front, as today.
        assert!(ContentChunks::from_stream(&dict, cut).next_chunk().is_err());
        let mut c = ContentChunks::from_stream(&dict, cut);
        c.window = 4096;
        let mut ops = 0;
        let err = loop {
            match c.next_chunk() {
                Ok(Some(w)) => ops += w.operations().count(),
                Ok(None) => panic!("expected the decode error"),
                Err(e) => break e,
            }
        };
        assert!(matches!(err, ContentError::Decode(_)), "{err:?}");
        assert!(ops > 0);
    }

    #[test]
    fn a_syntax_error_in_complete_content_is_still_an_error() {
        let dict = Dict::new();
        let mut c = ContentChunks::from_stream(&dict, b"0 0 m (unterminated");
        assert!(c.next_chunk().is_err());
    }
}
