//! The OCR model add-on manifest, `pdfcer-ocr-model.txt` (decision 182).
//!
//! Pure text parsing, no I/O, so it compiles everywhere and is fuzzed
//! (`fuzz/fuzz_targets/ocr_addon_manifest.rs`).
//!
//! Format: UTF-8, optional BOM, one `key = value` per line; blank lines and
//! lines starting `#` or `;` are ignored; a value may be wrapped in double
//! quotes.
//!
//! | key | value | |
//! |---|---|---|
//! | `name` | `[A-Za-z0-9._-]`, 1–64, starts alphanumeric | required, unique id |
//! | `engine` | `[a-z0-9_-]`, 1–32 | required; any token parses |
//! | `label` | free text ≤ 200 chars | optional |
//! | `languages` | tokens split on `,` or whitespace | optional |
//! | `version` | free text ≤ 64 chars | optional |
//! | `licence` (alias `license`) | free text ≤ 64 chars, e.g. an SPDX id | optional |
//! | `sha256` | `<relative file> <64 hex digits>`; repeatable | optional |
//!
//! A repeated single-valued key, a bad value or a line without `=` is an
//! error naming the line. An unknown key is kept in
//! [`OcrModelManifest::unknown_keys`] rather than refused, so an add-on
//! written for a newer pdfcer still loads here and the shell can say what it
//! ignored.

use std::collections::HashSet;

/// The manifest's file name inside an add-on folder.
pub const MANIFEST_FILE: &str = "pdfcer-ocr-model.txt";

/// Largest manifest accepted, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;

const MAX_NAME: usize = 64;
const MAX_ENGINE: usize = 32;
const MAX_LABEL: usize = 200;
const MAX_SHORT: usize = 64;
const MAX_FILE_PATH: usize = 260;

/// A parsed `pdfcer-ocr-model.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrModelManifest {
    /// The add-on's unique id (`--ocr-model NAME`).
    pub name: String,
    /// The engine whose data this folder holds (`ocrs`, `ocrcer`, `paddle`,
    /// `tesseract`, or a token a future build may know).
    pub engine: String,
    /// Human-readable label.
    pub label: Option<String>,
    /// Language tags, as written.
    pub languages: Vec<String>,
    /// The add-on's own version string.
    pub version: Option<String>,
    /// Licence identifier, as written.
    pub licence: Option<String>,
    /// Files to verify before the model is used, in manifest order.
    pub files: Vec<FileDigest>,
    /// Keys this build does not know, in file order.
    pub unknown_keys: Vec<String>,
}

/// One `sha256 = FILE HEX` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDigest {
    /// Path relative to the add-on folder, `/`-separated; never absolute and
    /// never containing `..`.
    pub file: String,
    /// The expected SHA-256.
    pub sha256: [u8; 32],
}

/// Why a manifest did not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// The bytes are not UTF-8.
    #[error("the manifest is not UTF-8 text")]
    NotUtf8,
    /// Larger than [`MAX_MANIFEST_BYTES`].
    #[error("the manifest is {len} bytes, over the {MAX_MANIFEST_BYTES}-byte limit")]
    TooLarge {
        /// Its size.
        len: usize,
    },
    /// A line is malformed; `line` is 1-based.
    #[error("line {line}: {reason}")]
    Line {
        /// 1-based line number.
        line: usize,
        /// What is wrong with it.
        reason: String,
    },
    /// A required key is absent.
    #[error("the manifest has no `{0}` line")]
    Missing(&'static str),
}

/// Parse manifest bytes (size check, UTF-8, then [`parse_manifest`]).
///
/// # Errors
///
/// [`ManifestError`], naming the line where one is at fault.
pub fn parse_manifest_bytes(bytes: &[u8]) -> Result<OcrModelManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge { len: bytes.len() });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ManifestError::NotUtf8)?;
    parse_manifest(text)
}

/// Parse manifest text.
///
/// # Errors
///
/// [`ManifestError`], naming the line where one is at fault.
///
/// # Examples
///
/// ```
/// use pdfcer_core::ocr::addon_manifest::parse_manifest;
/// let m = parse_manifest("name = demo-ja\nengine = paddle\nlanguages = ja, en\n")?;
/// assert_eq!(m.engine, "paddle");
/// assert_eq!(m.languages, ["ja", "en"]);
/// # Ok::<(), pdfcer_core::ocr::addon_manifest::ManifestError>(())
/// ```
pub fn parse_manifest(text: &str) -> Result<OcrModelManifest, ManifestError> {
    let mut b = Builder::default();
    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let at = |reason: String| ManifestError::Line {
            line: index + 1,
            reason,
        };
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| at(format!("{line:?} is not `key = value`")))?;
        let key = key.trim();
        let value = unquote(value.trim());
        if value.is_empty() {
            return Err(at(format!("{key} has no value")));
        }
        b.apply(key, value).map_err(at)?;
    }
    b.finish()
}

#[derive(Default)]
struct Builder {
    name: Option<String>,
    engine: Option<String>,
    label: Option<String>,
    languages: Option<Vec<String>>,
    version: Option<String>,
    licence: Option<String>,
    files: Vec<FileDigest>,
    file_names: HashSet<String>,
    unknown_keys: Vec<String>,
}

impl Builder {
    fn apply(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "name" => set(&mut self.name, key, checked_name(value)?),
            "engine" => set(&mut self.engine, key, checked_engine(value)?),
            "label" => set(&mut self.label, key, bounded(key, value, MAX_LABEL)?),
            "version" => set(&mut self.version, key, bounded(key, value, MAX_SHORT)?),
            "licence" | "license" => set(
                &mut self.licence,
                "licence",
                bounded(key, value, MAX_SHORT)?,
            ),
            "languages" => set(&mut self.languages, key, languages(value)?),
            "sha256" => self.add_digest(value),
            _ => {
                self.unknown_keys.push(key.to_owned());
                Ok(())
            }
        }
    }

    fn add_digest(&mut self, value: &str) -> Result<(), String> {
        let (file, hex) = value
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| format!("sha256 is {value:?}; use `FILE HEX`"))?;
        let file = checked_file(file.trim())?;
        let sha256 = parse_hex32(hex)
            .ok_or_else(|| format!("sha256 for {file:?}: {hex:?} is not 64 hex digits"))?;
        if !self.file_names.insert(file.clone()) {
            return Err(format!("sha256 for {file:?} is given twice"));
        }
        self.files.push(FileDigest { file, sha256 });
        Ok(())
    }

    fn finish(self) -> Result<OcrModelManifest, ManifestError> {
        Ok(OcrModelManifest {
            name: self.name.ok_or(ManifestError::Missing("name"))?,
            engine: self.engine.ok_or(ManifestError::Missing("engine"))?,
            label: self.label,
            languages: self.languages.unwrap_or_default(),
            version: self.version,
            licence: self.licence,
            files: self.files,
            unknown_keys: self.unknown_keys,
        })
    }
}

fn set<T>(slot: &mut Option<T>, key: &str, value: T) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{key} is set twice"));
    }
    *slot = Some(value);
    Ok(())
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

fn bounded(key: &str, value: &str, max: usize) -> Result<String, String> {
    if value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{key} must be at most {max} characters with no control characters"
        ));
    }
    Ok(value.to_owned())
}

/// Validate an add-on name: `[A-Za-z0-9._-]`, 1–64, starting alphanumeric.
///
/// # Errors
///
/// The reason, for display.
pub fn checked_name(value: &str) -> Result<String, String> {
    let ok = value.len() <= MAX_NAME
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "name is {value:?}; use 1-{MAX_NAME} letters, digits, `.`, `_` or `-`, starting with a letter or digit"
        ))
    }
}

fn checked_engine(value: &str) -> Result<String, String> {
    let ok = value.len() <= MAX_ENGINE
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'));
    if ok {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "engine is {value:?}; use 1-{MAX_ENGINE} lower-case letters, digits, `_` or `-`"
        ))
    }
}

fn languages(value: &str) -> Result<Vec<String>, String> {
    let tags: Vec<String> = value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect();
    for t in &tags {
        bounded("a language tag", t, MAX_SHORT)?;
    }
    Ok(tags)
}

/// A relative, `/`-separated path with no `..`, no `.`, no empty component,
/// no drive or backslash. `\` would be a separator on Windows only, so it is
/// refused everywhere to keep one manifest meaning one file on every OS.
fn checked_file(file: &str) -> Result<String, String> {
    let bad = file.is_empty()
        || file.len() > MAX_FILE_PATH
        || file.starts_with('/')
        || file.contains(['\\', ':'])
        || file.chars().any(char::is_control)
        || file
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..");
    if bad {
        return Err(format!(
            "sha256 file {file:?} must be a relative path inside the add-on folder, `/`-separated"
        ));
    }
    Ok(file.to_owned())
}

fn parse_hex32(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let &[hi, lo] = pair else { return None };
        let hi = char::from(hi).to_digit(16)?;
        let lo = char::from(lo).to_digit(16)?;
        *slot = u8::try_from(hi * 16 + lo).ok()?;
    }
    Some(out)
}

/// Lower-case hex of a digest, for messages.
#[must_use]
pub fn to_hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const HEX: &str = "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9";

    #[test]
    fn every_key_parses() {
        let m = parse_manifest(&format!(
            "\u{feff}# c\n; c\n\nname = pp-ja.v5\nengine=paddle\nlabel = \"Japanese PP-OCRv5\"\n\
             languages = ja, en zh\nversion = 5.0\nlicense = Apache-2.0\n\
             sha256 = det.onnx {HEX}\nsha256 = sub dir/rec model.onnx {}\nfuture = x\n",
            HEX.to_uppercase()
        ))
        .unwrap();
        assert_eq!(m.name, "pp-ja.v5");
        assert_eq!(m.engine, "paddle");
        assert_eq!(m.label.as_deref(), Some("Japanese PP-OCRv5"));
        assert_eq!(m.languages, ["ja", "en", "zh"]);
        assert_eq!(m.version.as_deref(), Some("5.0"));
        assert_eq!(m.licence.as_deref(), Some("Apache-2.0"));
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.files[1].file, "sub dir/rec model.onnx");
        assert_eq!(to_hex(&m.files[0].sha256), HEX);
        assert_eq!(m.files[0].sha256, m.files[1].sha256);
        assert_eq!(m.unknown_keys, ["future"]);
    }

    #[test]
    fn name_and_engine_are_required() {
        assert_eq!(
            parse_manifest("engine = paddle"),
            Err(ManifestError::Missing("name"))
        );
        assert_eq!(
            parse_manifest("name = x"),
            Err(ManifestError::Missing("engine"))
        );
    }

    #[test]
    fn mistakes_name_the_line() {
        for (text, needle) in [
            ("name = a\nname = b", "line 2: name is set twice"),
            ("licence = a\nlicense = b", "line 2: licence is set twice"),
            ("name = -x", "line 1: name is"),
            ("name = a/b", "line 1: name is"),
            ("engine = Paddle", "line 1: engine is"),
            ("words", "is not `key = value`"),
            ("label =", "label has no value"),
            ("sha256 = det.onnx", "use `FILE HEX`"),
            ("sha256 = det.onnx abc", "not 64 hex digits"),
            (&format!("sha256 = ../x {HEX}"), "relative path"),
            (&format!("sha256 = /x {HEX}"), "relative path"),
            (&format!("sha256 = C:x {HEX}"), "relative path"),
            (&format!("sha256 = a\\b {HEX}"), "relative path"),
            (&format!("sha256 = a//b {HEX}"), "relative path"),
            (
                &format!("sha256 = x {HEX}\nsha256 = x {HEX}"),
                "line 2: sha256 for \"x\" is given twice",
            ),
        ] {
            let err = parse_manifest(text).unwrap_err().to_string();
            assert!(err.contains(needle), "{text:?} -> {err}");
        }
    }

    #[test]
    fn oversize_and_non_utf8_are_refused() {
        let big = vec![b'#'; MAX_MANIFEST_BYTES + 1];
        assert!(matches!(
            parse_manifest_bytes(&big),
            Err(ManifestError::TooLarge { .. })
        ));
        assert_eq!(
            parse_manifest_bytes(b"name = \xff"),
            Err(ManifestError::NotUtf8)
        );
    }
}
