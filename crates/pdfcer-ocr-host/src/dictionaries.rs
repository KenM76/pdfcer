//! Which word lists recognition may use: the engine's built-in ones, none,
//! and the operator's own word files.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest total size of the user word files, in bytes. A project
/// vocabulary is kilobytes; past this it is not a word list.
pub const MAX_USER_WORDS_BYTES: u64 = 16 * 1024 * 1024;

/// The word lists recognition may use. The default is the engine's
/// built-in lists and no user words, which is what every engine did before
/// this option existed.
///
/// Only Tesseract takes word lists. Its built-in lists are its system and
/// frequent-word dictionaries (`load_system_dawg`, `load_freq_dawg`); its
/// punctuation and number patterns stay on either way. User words go in
/// as `--user-words`. The in-process engines read characters with no word
/// list: they honour [`Dictionaries::none`] as they are, and refuse user
/// words; PaddleOCR-VL's language model cannot be turned off, so it refuses
/// [`Dictionaries::none`] as well.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Dictionaries {
    builtin: bool,
    user_words: Vec<PathBuf>,
}

impl Default for Dictionaries {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Dictionaries {
    /// The engine's built-in word lists, no user words.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            builtin: true,
            user_words: Vec::new(),
        }
    }

    /// No word lists: each word is read from its letters alone. Better for
    /// part numbers, grid labels and codes a dictionary would "correct".
    #[must_use]
    pub fn none() -> Self {
        Self {
            builtin: false,
            user_words: Vec::new(),
        }
    }

    /// Add a UTF-8 word file, one word per line, to whichever lists are on.
    #[must_use]
    pub fn with_user_words(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_words.push(path.into());
        self
    }

    /// Whether the engine's built-in lists are on.
    #[must_use]
    pub fn uses_builtin(&self) -> bool {
        self.builtin
    }

    /// The user word files, in the order given.
    #[must_use]
    pub fn user_words(&self) -> &[PathBuf] {
        &self.user_words
    }
}

impl std::fmt::Display for Dictionaries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.builtin {
            "built-in word lists"
        } else {
            "no built-in word lists"
        })?;
        if !self.user_words.is_empty() {
            let names: Vec<String> = self
                .user_words
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            write!(f, " + user words from {}", names.join(", "))?;
        }
        Ok(())
    }
}

/// The user word files merged into one temporary file, which Tesseract
/// reads on every page and which is deleted with the last engine holding
/// it. Merging lets several files through Tesseract's single
/// `--user-words`, and normalises line endings and blank lines.
#[derive(Debug)]
pub(crate) struct MergedWords {
    path: PathBuf,
    words: usize,
}

impl MergedWords {
    /// Read, check and merge `files`; `Ok(None)` when there are none.
    pub(crate) fn merge(files: &[PathBuf]) -> Result<Option<Arc<Self>>, String> {
        if files.is_empty() {
            return Ok(None);
        }
        let mut total = 0u64;
        let mut seen = std::collections::BTreeSet::new();
        let mut out = String::new();
        for file in files {
            let text = read_word_file(file, &mut total)?;
            for word in text.lines().map(str::trim).filter(|w| !w.is_empty()) {
                if seen.insert(word.to_owned()) {
                    out.push_str(word);
                    out.push('\n');
                }
            }
        }
        if seen.is_empty() {
            return Err(format!(
                "user words: {} hold no words",
                files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdfcer-user-words-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(Arc::new(Self {
            path,
            words: seen.len(),
        })))
    }

    /// The merged file.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// How many distinct words it holds.
    pub(crate) fn words(&self) -> usize {
        self.words
    }
}

impl Drop for MergedWords {
    fn drop(&mut self) {
        // Best effort: a leftover file in the temp folder is harmless.
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_word_file(file: &Path, total: &mut u64) -> Result<String, String> {
    let size = std::fs::metadata(file)
        .map_err(|e| format!("user words {}: {e}", file.display()))?
        .len();
    *total = total.saturating_add(size);
    if *total > MAX_USER_WORDS_BYTES {
        return Err(format!(
            "user words: the files exceed {MAX_USER_WORDS_BYTES} bytes together"
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(MAX_USER_WORDS_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("user words {}: {e}", file.display()))?;
    if bytes.len() as u64 > size {
        *total = total.saturating_add(bytes.len() as u64 - size);
        if *total > MAX_USER_WORDS_BYTES {
            return Err(format!(
                "user words: the files exceed {MAX_USER_WORDS_BYTES} bytes together"
            ));
        }
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("user words {}: not UTF-8 text", file.display()))?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}
