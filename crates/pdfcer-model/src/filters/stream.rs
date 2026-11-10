//! Windowed decoding of one stream: [`StreamDecoder`].
//!
//! A consumer that can work on a stream a piece at a time (the renderer's
//! content interpreter) reads it through this instead of
//! [`super::decode_stream`], so a legitimately huge stream is neither
//! refused by [`MAX_DECODED_LEN`] nor held whole in memory.
//!
//! Only the **last** stage of the `/Filter` chain streams. Earlier stages,
//! a last stage with a `/Predictor`, and the filters that cannot expand
//! (`ASCIIHexDecode`, `ASCII85Decode`) are decoded whole under the ordinary
//! ceiling and then served from memory. A real chain is one stage, so this
//! costs nothing in practice and keeps one decoder per filter.

use std::borrow::Cow;

use super::{FilterError, MAX_DECODED_LEN, Pull, brotli, flate, lzw, predictor, runlength};
use crate::object::Dict;

/// How far a streamed decode may expand its input: the budget is
/// `max(MAX_DECODED_LEN, MAX_DECODE_RATIO × encoded length)`.
///
/// 64 is above the expansion of any real content stream (text and vector
/// operators compress roughly 3–10×) and far below a decompression bomb's
/// (Flate reaches about 1000×). It is pdfcer policy; the standard sets no
/// such limit (ISO 32000-2 Annex C lists none for decoded stream size).
pub const MAX_DECODE_RATIO: usize = 64;

/// Outcome of one [`StreamDecoder::read`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadStatus {
    /// Bytes were appended and the stream has more.
    More,
    /// The stream ended; the call may still have appended bytes.
    End,
    /// The budget ([`StreamDecoder::limit`]) is spent: output stops at
    /// exactly the limit and the rest of the stream is never decoded.
    BudgetSpent,
}

enum Source<'a> {
    Bytes { data: Cow<'a, [u8]>, pos: usize },
    Pull(Box<dyn Pull + 'a>),
}

/// Decodes one stream a chunk at a time under a ratio budget.
///
/// # Examples
///
/// ```
/// use pdfcer_model::filters::{ReadStatus, StreamDecoder};
/// use pdfcer_model::object::Dict;
///
/// let dict = Dict::new(); // no /Filter: the raw bytes are the data
/// let mut dec = StreamDecoder::new(&dict, b"0 0 m 1 1 l S")?;
/// let mut out = Vec::new();
/// while dec.read(&mut out)? == ReadStatus::More {}
/// assert_eq!(out, b"0 0 m 1 1 l S");
/// # Ok::<(), pdfcer_model::filters::FilterError>(())
/// ```
pub struct StreamDecoder<'a> {
    src: Source<'a>,
    produced: usize,
    limit: usize,
    done: bool,
}

impl std::fmt::Debug for StreamDecoder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamDecoder")
            .field("produced", &self.produced)
            .field("limit", &self.limit)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl<'a> StreamDecoder<'a> {
    /// Prepare to decode `raw` (the stream's encoded bytes) per `dict`'s
    /// `/Filter` and `/DecodeParms`.
    ///
    /// # Errors
    ///
    /// [`FilterError`] from parsing the filter chain, or from a stage this
    /// decodes whole (module docs) — the same errors
    /// [`super::decode_stream`] would report for it.
    pub fn new(dict: &Dict, raw: &'a [u8]) -> Result<Self, FilterError> {
        Self::with_floor(dict, raw, MAX_DECODED_LEN)
    }

    /// [`Self::new`] with a smaller budget floor, so a test can reach the
    /// budget without decoding a quarter of a gigabyte.
    pub(crate) fn with_floor(
        dict: &Dict,
        raw: &'a [u8],
        floor: usize,
    ) -> Result<Self, FilterError> {
        let limit = floor.max(raw.len().saturating_mul(MAX_DECODE_RATIO));
        let src = source(dict, raw)?;
        Ok(Self {
            src,
            produced: 0,
            limit,
            done: false,
        })
    }

    /// The most bytes this decoder will ever produce.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Append the next chunk (about 64 KiB) of decoded bytes to `out`.
    ///
    /// # Errors
    ///
    /// [`FilterError::Corrupt`] or [`FilterError::Truncated`] when the
    /// stream is damaged part-way. Bytes decoded before the damage were
    /// returned by earlier calls; the decoder is finished after an error.
    pub fn read(&mut self, out: &mut Vec<u8>) -> Result<ReadStatus, FilterError> {
        if self.done {
            return Ok(ReadStatus::End);
        }
        if self.produced >= self.limit {
            return Ok(ReadStatus::BudgetSpent);
        }
        let before = out.len();
        let finished = match &mut self.src {
            Source::Bytes { data, pos } => {
                let rest = data.get(*pos..).unwrap_or(&[]);
                let take = rest.len().min(super::CHUNK);
                out.extend_from_slice(rest.get(..take).unwrap_or(&[]));
                *pos += take;
                *pos >= data.len()
            }
            Source::Pull(p) => p.pull(out).inspect_err(|_| self.done = true)?,
        };
        self.produced += out.len() - before;
        if self.produced > self.limit {
            out.truncate(out.len() - (self.produced - self.limit));
            self.produced = self.limit;
            self.done = !finished;
            return Ok(if finished {
                ReadStatus::End
            } else {
                ReadStatus::BudgetSpent
            });
        }
        self.done = finished;
        Ok(if finished {
            ReadStatus::End
        } else {
            ReadStatus::More
        })
    }
}

/// Build the source for the last stage, decoding everything before it whole.
fn source<'a>(dict: &Dict, raw: &'a [u8]) -> Result<Source<'a>, FilterError> {
    let filters = super::filter_names(dict)?;
    let Some((last, _)) = filters.split_last() else {
        return Ok(Source::Bytes {
            data: Cow::Borrowed(raw),
            pos: 0,
        });
    };
    let n = filters.len();
    let mut notes = super::FilterNotes::default();
    let input: Cow<'a, [u8]> = if n > 1 {
        Cow::Owned(super::decode_prefix(dict, raw, n - 1, &mut notes)?)
    } else {
        Cow::Borrowed(raw)
    };
    let parms = super::decode_parms_for(dict, n - 1, n);
    let predicted = predictor::Params::from_dict(parms)?.is_some();
    let pull: Box<dyn Pull + 'a> = match last.as_slice() {
        _ if predicted => return whole(last, parms, input, &mut notes),
        b"FlateDecode" | b"Fl" => Box::new(flate::Inflate::new(input)),
        b"BrotliDecode" => Box::new(brotli::Unbrotli::new(input)),
        b"LZWDecode" | b"LZW" => Box::new(lzw::Lzw::new(input, lzw::early_change(parms))),
        b"RunLengthDecode" | b"RL" => Box::new(runlength::RunLength::new(input)),
        _ => return whole(last, parms, input, &mut notes),
    };
    Ok(Source::Pull(pull))
}

fn whole<'a>(
    name: &[u8],
    parms: Option<&Dict>,
    input: Cow<'_, [u8]>,
    notes: &mut super::FilterNotes,
) -> Result<Source<'a>, FilterError> {
    Ok(Source::Bytes {
        data: Cow::Owned(super::apply_one(name, parms, &input, notes)?),
        pos: 0,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::object::{Name, Object};

    fn read_all(dict: &Dict, raw: &[u8]) -> (Vec<u8>, ReadStatus) {
        let mut dec = StreamDecoder::new(dict, raw).unwrap();
        let mut out = Vec::new();
        loop {
            match dec.read(&mut out).unwrap() {
                ReadStatus::More => {}
                status => return (out, status),
            }
        }
    }

    fn with_filter(name: &str) -> Dict {
        let mut d = Dict::new();
        d.insert(
            Name::from(b"Filter".as_slice()),
            Object::Name(Name::from(name.as_bytes())),
        );
        d
    }

    fn sample() -> Vec<u8> {
        (0..300_000u32)
            .flat_map(|i| format!("{} {} m ", i % 97, i % 31).into_bytes())
            .collect()
    }

    #[test]
    fn every_streamed_filter_matches_the_whole_decode() {
        let plain = sample();
        let cases = [
            ("FlateDecode", flate::encode(&plain)),
            ("RunLengthDecode", runlength::encode(&plain)),
        ];
        for (name, enc) in cases {
            let dict = with_filter(name);
            let (got, status) = read_all(&dict, &enc);
            assert_eq!(status, ReadStatus::End, "{name}");
            assert_eq!(
                got,
                super::super::decode_stream(&dict, &enc).unwrap(),
                "{name}"
            );
            assert_eq!(got, plain, "{name}");
        }
    }

    #[test]
    fn a_bomb_stops_at_exactly_the_budget() {
        // 300 MiB of zeros deflates to ~300 KiB: about 1000x.
        let enc = flate::encode(&vec![0u8; 300 << 20]);
        let dict = with_filter("FlateDecode");
        let (got, status) = read_all(&dict, &enc);
        assert_eq!(status, ReadStatus::BudgetSpent);
        assert_eq!(got.len(), MAX_DECODED_LEN.max(enc.len() * MAX_DECODE_RATIO));
        assert!(got.iter().all(|&b| b == 0));
    }

    #[test]
    fn the_budget_is_the_ratio_once_that_exceeds_the_ceiling() {
        let raw = vec![b' '; 5 << 20];
        let dec = StreamDecoder::new(&Dict::new(), &raw).unwrap();
        assert_eq!(dec.limit(), raw.len() * MAX_DECODE_RATIO);
        assert!(dec.limit() > MAX_DECODED_LEN);
    }

    #[test]
    fn truncation_is_an_error_after_the_prefix() {
        let plain = sample();
        let enc = flate::encode(&plain);
        let cut = enc.get(..enc.len() / 2).unwrap();
        let mut dec = StreamDecoder::new(&with_filter("FlateDecode"), cut).unwrap();
        let mut out = Vec::new();
        let err = loop {
            match dec.read(&mut out) {
                Ok(ReadStatus::More) => {}
                Ok(s) => panic!("expected an error, got {s:?}"),
                Err(e) => break e,
            }
        };
        assert!(matches!(err, FilterError::Truncated { .. }), "{err:?}");
        assert!(!out.is_empty() && plain.starts_with(&out));
        assert_eq!(dec.read(&mut out).unwrap(), ReadStatus::End);
    }
}
