//! The PaddleOCR-VL tokenizer: a byte-fallback BPE read from a Hugging Face
//! `tokenizer.json`, implementing only the components that file uses.
//!
//! Supported, and anything else refused by name rather than approximated (a
//! mis-tokenised prompt reads as confident nonsense):
//! - model `BPE` (merges as `"a b"` or `["a", "b"]`), `byte_fallback`,
//!   `unk_token`, `fuse_unk`; no `dropout`, subword prefix or word suffix;
//! - normalizer `null`, `Replace` (string pattern) or a `Sequence` of them;
//! - pre-tokenizer `null`;
//! - decoder `Replace` (string pattern), `ByteFallback`, `Fuse`, or a
//!   `Sequence` of them;
//! - added tokens matched leftmost-longest on the raw text, without the
//!   `lstrip`/`rstrip`/`single_word` options.
//!
//! Pure; compiled everywhere and fuzzed (`fuzz/fuzz_targets/vl_tokenizer.rs`).

use std::collections::HashMap;

use super::json_lite::{self, Json};

/// Largest `tokenizer.json` accepted, in bytes.
pub const MAX_TOKENIZER_BYTES: usize = 64 * 1024 * 1024;

/// Largest vocabulary (model vocab plus added tokens) accepted.
pub const MAX_VOCAB: usize = 1 << 20;

/// Longest text [`VlTokenizer::encode`] accepts, in bytes: BPE merging is
/// quadratic in a segment's length and this tokenizer has no pre-tokenizer.
pub const MAX_ENCODE_BYTES: usize = 16 * 1024;

/// Why a tokenizer could not be built or used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TokenizerError {
    /// The file is larger than [`MAX_TOKENIZER_BYTES`].
    #[error("tokenizer file is {0} bytes; the limit is {MAX_TOKENIZER_BYTES}")]
    TooLarge(usize),
    /// The file is not valid JSON.
    #[error("tokenizer file is not JSON: {0}")]
    Json(#[from] json_lite::JsonError),
    /// A required member is missing or has the wrong type.
    #[error("tokenizer file: {0}")]
    Malformed(String),
    /// The file uses a component this reader does not implement.
    #[error("tokenizer file uses {0}, which pdfcer's tokenizer does not implement")]
    Unsupported(String),
    /// Text to encode is longer than [`MAX_ENCODE_BYTES`].
    #[error("text to encode is {0} bytes; the limit is {MAX_ENCODE_BYTES}")]
    TextTooLong(usize),
}

#[derive(Debug, Clone)]
enum DecodeStep {
    Replace(String, String),
    ByteFallback,
    Fuse,
}

/// A loaded tokenizer.
#[derive(Debug, Clone)]
pub struct VlTokenizer {
    vocab: HashMap<String, u32>,
    pieces: Vec<Option<String>>,
    special: Vec<bool>,
    merges: HashMap<(u32, u32), (u32, u32)>,
    added: HashMap<char, Vec<(String, u32)>>,
    normalize: Vec<(String, String)>,
    decoders: Vec<DecodeStep>,
    byte_fallback: bool,
    unk: Option<u32>,
    fuse_unk: bool,
}

fn malformed(what: &str) -> TokenizerError {
    TokenizerError::Malformed(what.to_owned())
}

fn str_member<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Json::as_str)
}

/// A `Replace` component's `(pattern, content)`; a regex pattern is refused.
fn replace_pair(v: &Json) -> Result<(String, String), TokenizerError> {
    let pat = v
        .get("pattern")
        .ok_or_else(|| malformed("Replace without pattern"))?;
    let Some(from) = str_member(pat, "String") else {
        return Err(TokenizerError::Unsupported(
            "a Replace with a non-string pattern".into(),
        ));
    };
    let to = str_member(v, "content").ok_or_else(|| malformed("Replace without content"))?;
    if from.is_empty() {
        return Err(malformed("Replace with an empty pattern"));
    }
    Ok((from.to_owned(), to.to_owned()))
}

/// Flatten a component that may be a `Sequence` of `key` entries.
fn flatten<'a>(v: &'a Json, key: &str) -> Vec<&'a Json> {
    match (str_member(v, "type"), v.get(key)) {
        (Some("Sequence"), Some(Json::Arr(items))) => items.iter().collect(),
        _ => vec![v],
    }
}

fn normalizer(root: &Json) -> Result<Vec<(String, String)>, TokenizerError> {
    let Some(n) = root.get("normalizer").filter(|n| !n.is_null()) else {
        return Ok(Vec::new());
    };
    flatten(n, "normalizers")
        .into_iter()
        .map(|c| match str_member(c, "type") {
            Some("Replace") => replace_pair(c),
            other => Err(TokenizerError::Unsupported(format!(
                "normalizer {}",
                other.unwrap_or("(untyped)")
            ))),
        })
        .collect()
}

fn decoder(root: &Json) -> Result<Vec<DecodeStep>, TokenizerError> {
    let Some(d) = root.get("decoder").filter(|d| !d.is_null()) else {
        return Ok(Vec::new());
    };
    flatten(d, "decoders")
        .into_iter()
        .map(|c| match str_member(c, "type") {
            Some("Replace") => replace_pair(c).map(|(a, b)| DecodeStep::Replace(a, b)),
            Some("ByteFallback") => Ok(DecodeStep::ByteFallback),
            Some("Fuse") => Ok(DecodeStep::Fuse),
            other => Err(TokenizerError::Unsupported(format!(
                "decoder {}",
                other.unwrap_or("(untyped)")
            ))),
        })
        .collect()
}

/// Refuse BPE options that change encoding and are not implemented.
fn check_model_options(model: &Json) -> Result<(), TokenizerError> {
    if str_member(model, "type") != Some("BPE") {
        return Err(TokenizerError::Unsupported(format!(
            "model type {}",
            str_member(model, "type").unwrap_or("(untyped)")
        )));
    }
    for key in ["continuing_subword_prefix", "end_of_word_suffix"] {
        if model.get(key).is_some_and(|v| !v.is_null()) {
            return Err(TokenizerError::Unsupported(format!("BPE {key}")));
        }
    }
    let dropout = model.get("dropout").filter(|d| !d.is_null());
    if dropout.is_some_and(|d| *d != Json::Num(0.0)) {
        return Err(TokenizerError::Unsupported("BPE dropout".into()));
    }
    if model.get("ignore_merges").and_then(Json::as_bool) == Some(true) {
        return Err(TokenizerError::Unsupported("BPE ignore_merges".into()));
    }
    Ok(())
}

impl VlTokenizer {
    /// Build from the bytes of a `tokenizer.json`.
    ///
    /// # Errors
    ///
    /// [`TokenizerError`]: too large, not JSON, a missing or mistyped member,
    /// a merge naming a token not in the vocabulary, or an unimplemented
    /// component.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, TokenizerError> {
        if bytes.len() > MAX_TOKENIZER_BYTES {
            return Err(TokenizerError::TooLarge(bytes.len()));
        }
        let root = json_lite::parse(bytes)?;
        if root.get("pre_tokenizer").is_some_and(|p| !p.is_null()) {
            return Err(TokenizerError::Unsupported("a pre_tokenizer".into()));
        }
        let model = root.get("model").ok_or_else(|| malformed("no `model`"))?;
        check_model_options(model)?;
        let mut tok = Self {
            vocab: HashMap::new(),
            pieces: Vec::new(),
            special: Vec::new(),
            merges: HashMap::new(),
            added: HashMap::new(),
            normalize: normalizer(&root)?,
            decoders: decoder(&root)?,
            byte_fallback: model.get("byte_fallback").and_then(Json::as_bool) == Some(true),
            unk: None,
            fuse_unk: model.get("fuse_unk").and_then(Json::as_bool) == Some(true),
        };
        tok.load_vocab(model)?;
        tok.load_added(&root)?;
        tok.load_merges(model)?;
        tok.unk = str_member(model, "unk_token").and_then(|u| tok.vocab.get(u).copied());
        Ok(tok)
    }

    fn set_piece(&mut self, id: u32, text: &str, special: bool) -> Result<(), TokenizerError> {
        let i = usize::try_from(id).map_err(|_| malformed("token id out of range"))?;
        if i >= MAX_VOCAB {
            return Err(malformed("token id beyond the vocabulary limit"));
        }
        if self.pieces.len() <= i {
            self.pieces.resize(i + 1, None);
            self.special.resize(i + 1, false);
        }
        if let (Some(p), Some(s)) = (self.pieces.get_mut(i), self.special.get_mut(i)) {
            *p = Some(text.to_owned());
            *s = special;
        }
        Ok(())
    }

    fn load_vocab(&mut self, model: &Json) -> Result<(), TokenizerError> {
        let Some(Json::Obj(vocab)) = model.get("vocab") else {
            return Err(malformed("`model.vocab` is not an object"));
        };
        if vocab.len() > MAX_VOCAB {
            return Err(malformed("vocabulary too large"));
        }
        for (text, id) in vocab {
            let id = id
                .as_u32()
                .ok_or_else(|| malformed("a vocab id is not an integer"))?;
            self.set_piece(id, text, false)?;
            self.vocab.insert(text.clone(), id);
        }
        Ok(())
    }

    fn load_added(&mut self, root: &Json) -> Result<(), TokenizerError> {
        let Some(Json::Arr(list)) = root.get("added_tokens") else {
            return Ok(());
        };
        if list.len() > MAX_VOCAB {
            return Err(malformed("too many added tokens"));
        }
        for t in list {
            for opt in ["lstrip", "rstrip", "single_word"] {
                if t.get(opt).and_then(Json::as_bool) == Some(true) {
                    return Err(TokenizerError::Unsupported(format!(
                        "an added token with {opt}"
                    )));
                }
            }
            let id = t.get("id").and_then(Json::as_u32);
            let content = str_member(t, "content").filter(|c| !c.is_empty());
            let (Some(id), Some(content)) = (id, content) else {
                return Err(malformed("an added token lacks an id or content"));
            };
            let special = t.get("special").and_then(Json::as_bool) == Some(true);
            self.set_piece(id, content, special)?;
            if let Some(first) = content.chars().next() {
                self.added
                    .entry(first)
                    .or_default()
                    .push((content.to_owned(), id));
            }
        }
        for v in self.added.values_mut() {
            v.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        }
        Ok(())
    }

    fn load_merges(&mut self, model: &Json) -> Result<(), TokenizerError> {
        let Some(Json::Arr(list)) = model.get("merges") else {
            return Err(malformed("`model.merges` is not an array"));
        };
        for (rank, m) in list.iter().enumerate() {
            let (a, b) = match m {
                Json::Str(s) => s
                    .split_once(' ')
                    .ok_or_else(|| malformed("a merge lacks a space"))?,
                Json::Arr(p) => match p.as_slice() {
                    [Json::Str(a), Json::Str(b)] => (a.as_str(), b.as_str()),
                    _ => return Err(malformed("a merge is not a pair of strings")),
                },
                _ => return Err(malformed("a merge is not a string or pair")),
            };
            let id = |t: &str| {
                self.vocab
                    .get(t)
                    .copied()
                    .ok_or_else(|| malformed("a merge names a token not in the vocabulary"))
            };
            let pair = (id(a)?, id(b)?);
            let merged = id(&format!("{a}{b}"))?;
            let rank = u32::try_from(rank).map_err(|_| malformed("too many merges"))?;
            self.merges.entry(pair).or_insert((rank, merged));
        }
        Ok(())
    }

    /// The id of a token's exact text (vocabulary or added token).
    #[must_use]
    pub fn token_id(&self, text: &str) -> Option<u32> {
        self.vocab.get(text).copied().or_else(|| {
            let first = text.chars().next()?;
            self.added
                .get(&first)?
                .iter()
                .find(|(c, _)| c == text)
                .map(|(_, id)| *id)
        })
    }

    /// One past the highest token id.
    #[must_use]
    pub fn vocab_len(&self) -> usize {
        self.pieces.len()
    }

    /// Encode `text` without adding any special tokens.
    ///
    /// # Errors
    ///
    /// [`TokenizerError::TextTooLong`] beyond [`MAX_ENCODE_BYTES`].
    pub fn encode(&self, text: &str) -> Result<Vec<u32>, TokenizerError> {
        if text.len() > MAX_ENCODE_BYTES {
            return Err(TokenizerError::TextTooLong(text.len()));
        }
        let mut out = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let (plain, added) = self.split_added(rest);
            self.encode_segment(plain, &mut out);
            match added {
                Some((len, id)) => {
                    out.push(id);
                    rest = rest.get(plain.len() + len..).unwrap_or("");
                }
                None => rest = "",
            }
        }
        Ok(out)
    }

    /// The text before the leftmost-longest added token, and that token's
    /// byte length and id.
    fn split_added<'t>(&self, text: &'t str) -> (&'t str, Option<(usize, u32)>) {
        for (pos, c) in text.char_indices() {
            let Some(cands) = self.added.get(&c) else {
                continue;
            };
            let tail = text.get(pos..).unwrap_or("");
            if let Some((t, id)) = cands.iter().find(|(t, _)| tail.starts_with(t.as_str())) {
                return (text.get(..pos).unwrap_or(""), Some((t.len(), *id)));
            }
        }
        (text, None)
    }

    fn encode_segment(&self, text: &str, out: &mut Vec<u32>) {
        if text.is_empty() {
            return;
        }
        let mut norm = text.to_owned();
        for (from, to) in &self.normalize {
            norm = norm.replace(from.as_str(), to);
        }
        let mut syms = self.initial_symbols(&norm);
        while let Some((at, merged)) = self.best_merge(&syms) {
            syms.splice(at..at + 2, [merged]);
        }
        out.extend(syms);
    }

    fn initial_symbols(&self, text: &str) -> Vec<u32> {
        let mut syms: Vec<u32> = Vec::new();
        let mut last_unk = false;
        let mut buf = [0u8; 4];
        for c in text.chars() {
            let s = c.encode_utf8(&mut buf);
            if let Some(&id) = self.vocab.get(&*s) {
                syms.push(id);
                last_unk = false;
                continue;
            }
            let bytes: Option<Vec<u32>> = self
                .byte_fallback
                .then(|| {
                    s.bytes()
                        .map(|b| self.vocab.get(&format!("<0x{b:02X}>")).copied())
                        .collect()
                })
                .flatten();
            if let Some(ids) = bytes {
                syms.extend(ids);
                last_unk = false;
            } else if let Some(unk) = self.unk {
                if !(self.fuse_unk && last_unk) {
                    syms.push(unk);
                }
                last_unk = true;
            }
        }
        syms
    }

    fn best_merge(&self, syms: &[u32]) -> Option<(usize, u32)> {
        let mut best: Option<(u32, usize, u32)> = None;
        for (i, w) in syms.windows(2).enumerate() {
            if let [a, b] = w
                && let Some(&(rank, merged)) = self.merges.get(&(*a, *b))
                && best.is_none_or(|(r, _, _)| rank < r)
            {
                best = Some((rank, i, merged));
            }
        }
        best.map(|(_, i, m)| (i, m))
    }

    /// Decode ids to text through the file's decoder chain. Unknown ids are
    /// skipped; `skip_special` drops special added tokens.
    #[must_use]
    pub fn decode(&self, ids: &[u32], skip_special: bool) -> String {
        let usize_id = |id: u32| usize::try_from(id).ok();
        let mut tokens: Vec<String> = ids
            .iter()
            .filter_map(|&id| {
                let i = usize_id(id)?;
                if skip_special && self.special.get(i).copied().unwrap_or(false) {
                    return None;
                }
                self.pieces.get(i)?.clone()
            })
            .collect();
        for step in &self.decoders {
            tokens = match step {
                DecodeStep::Replace(from, to) => tokens
                    .into_iter()
                    .map(|t| t.replace(from.as_str(), to))
                    .collect(),
                DecodeStep::ByteFallback => byte_fallback(tokens),
                DecodeStep::Fuse => vec![tokens.concat()],
            };
        }
        tokens.concat()
    }
}

/// `<0xNN>` tokens become the UTF-8 text their bytes spell; an invalid run
/// becomes one U+FFFD per byte, as the reference implementation does.
fn byte_fallback(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut bytes: Vec<u8> = Vec::new();
    let flush = |bytes: &mut Vec<u8>, out: &mut Vec<String>| {
        if bytes.is_empty() {
            return;
        }
        match String::from_utf8(std::mem::take(bytes)) {
            Ok(s) => out.push(s),
            Err(e) => out.push("\u{FFFD}".repeat(e.as_bytes().len())),
        }
    };
    for t in tokens {
        let byte = t
            .strip_prefix("<0x")
            .and_then(|h| h.strip_suffix('>'))
            .filter(|h| h.len() == 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match byte {
            Some(b) => bytes.push(b),
            None => {
                flush(&mut bytes, &mut out);
                out.push(t);
            }
        }
    }
    flush(&mut bytes, &mut out);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A tiny tokenizer in the PaddleOCR-VL file's shape.
    pub(crate) const FIXTURE: &str = r#"{
      "added_tokens": [
        {"id": 0, "content": "<unk>", "special": true},
        {"id": 2, "content": "</s>", "special": true},
        {"id": 20, "content": "<|IMG|>", "special": true},
        {"id": 21, "content": "<|IMG_END|>", "special": true},
        {"id": 22, "content": "7", "special": false},
        {"id": 23, "content": "77", "special": false}
      ],
      "normalizer": {"type": "Sequence", "normalizers": [
        {"type": "Replace", "pattern": {"String": " "}, "content": "▁"}]},
      "pre_tokenizer": null,
      "decoder": {"type": "Sequence", "decoders": [
        {"type": "Replace", "pattern": {"String": "▁"}, "content": " "},
        {"type": "ByteFallback"}, {"type": "Fuse"}]},
      "model": {"type": "BPE", "dropout": null, "unk_token": "<unk>",
        "fuse_unk": true, "byte_fallback": true, "ignore_merges": false,
        "vocab": {"<unk>": 0, "</s>": 2, "a": 3, "b": 4, "▁": 5, "ab": 6,
          "▁ab": 7, "<0x0A>": 8, "<0xC3>": 9, "<0xA9>": 10, "c": 11, "bc": 12},
        "merges": ["a b", ["▁", "ab"], "b c"]}
    }"#;

    fn fixture() -> VlTokenizer {
        VlTokenizer::from_json_bytes(FIXTURE.as_bytes()).unwrap()
    }

    #[test]
    fn merges_apply_by_rank_after_the_space_normaliser() {
        let t = fixture();
        assert_eq!(t.encode("ab ab").unwrap(), [6, 7]);
        assert_eq!(t.encode("ba").unwrap(), [4, 3], "no merge for b a");
        assert_eq!(t.encode("abc").unwrap(), [6, 11], "a b outranks b c");
    }

    #[test]
    fn added_tokens_split_the_text_leftmost_longest() {
        let t = fixture();
        assert_eq!(
            t.encode("<|IMG|>ab<|IMG_END|>7c").unwrap(),
            [20, 6, 21, 22, 11]
        );
        assert_eq!(t.encode("777").unwrap(), [23, 22], "77 before 7");
        assert_eq!(t.token_id("<|IMG_END|>"), Some(21));
        assert_eq!(t.token_id("ab"), Some(6));
    }

    #[test]
    fn unknown_characters_fall_back_to_bytes_or_a_fused_unk() {
        let t = fixture();
        assert_eq!(t.encode("\n\u{e9}").unwrap(), [8, 9, 10]);
        assert_eq!(
            t.encode("a\u{4e00}\u{4e01}b").unwrap(),
            [3, 0, 4],
            "fused unk"
        );
    }

    #[test]
    fn decode_runs_the_decoder_chain_and_skips_specials() {
        let t = fixture();
        assert_eq!(t.decode(&[20, 7, 8, 9, 10, 22, 2], true), " ab\n\u{e9}7");
        assert_eq!(t.decode(&[9, 3], true), "\u{FFFD}a", "a lone lead byte");
        assert_eq!(
            t.decode(&[2, 999_999], false),
            "</s>",
            "an unknown id is skipped"
        );
    }

    #[test]
    fn round_trips_through_encode_and_decode() {
        let t = fixture();
        let text = "ab\nab \u{e9}7";
        assert_eq!(t.decode(&t.encode(text).unwrap(), true), text);
    }

    #[test]
    fn unimplemented_components_are_refused_by_name() {
        let swap = |from: &str, to: &str| FIXTURE.replacen(from, to, 1);
        for (from, to, needle) in [
            (
                "\"pre_tokenizer\": null",
                "\"pre_tokenizer\": {\"type\": \"Metaspace\"}",
                "pre_tokenizer",
            ),
            ("\"type\": \"BPE\"", "\"type\": \"Unigram\"", "Unigram"),
            ("{\"type\": \"Fuse\"}", "{\"type\": \"Strip\"}", "Strip"),
            ("\"dropout\": null", "\"dropout\": 0.1", "dropout"),
            (
                "\"special\": false",
                "\"special\": false, \"lstrip\": true",
                "lstrip",
            ),
            ("{\"String\": \" \"}", "{\"Regex\": \" \"}", "non-string"),
        ] {
            let err = VlTokenizer::from_json_bytes(swap(from, to).as_bytes()).unwrap_err();
            assert!(
                matches!(err, TokenizerError::Unsupported(_)),
                "{needle}: {err}"
            );
            assert!(err.to_string().contains(needle), "{err}");
        }
    }

    #[test]
    fn malformed_files_are_errors() {
        let bad_merge = FIXTURE.replacen("\"a b\"", "\"a zz\"", 1);
        assert!(matches!(
            VlTokenizer::from_json_bytes(bad_merge.as_bytes()),
            Err(TokenizerError::Malformed(_))
        ));
        let big_id = FIXTURE.replacen("\"c\": 11", "\"c\": 4294967295", 1);
        assert!(VlTokenizer::from_json_bytes(big_id.as_bytes()).is_err());
        assert!(VlTokenizer::from_json_bytes(b"{}").is_err());
        assert!(VlTokenizer::from_json_bytes(b"[").is_err());
    }

    #[test]
    fn encode_input_is_bounded() {
        let long = "a".repeat(MAX_ENCODE_BYTES + 1);
        assert_eq!(
            fixture().encode(&long),
            Err(TokenizerError::TextTooLong(MAX_ENCODE_BYTES + 1))
        );
    }
}
