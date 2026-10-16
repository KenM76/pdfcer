//! The word diff: which old words are kept, given new text, or removed.

use crate::text_edit::BlockEditError;
use crate::text_edit::cause::UnsupportedCause;

use super::words::Word;

/// The most old × new word pairs the diff will tabulate.
const MAX_CELLS: usize = 16 * 1024 * 1024;

/// What becomes of one old word.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Fate {
    Keep,
    /// Set this text (one or more new words, space-joined) in its place.
    Replace(String),
    Delete,
}

/// One fate per old word. Old and new words are aligned by their longest
/// common subsequence; between two aligned words, old and new words pair in
/// order, extra old words are deleted, and extra new words join the hunk's
/// last paired word (or, in a hunk with no old word, the word before it, or
/// after it at the start).
///
/// # Errors
///
/// A block with no words, or more pairs than [`MAX_CELLS`].
pub(super) fn fates(old: &[Word], new: &[&str]) -> Result<Vec<Fate>, BlockEditError> {
    let (n, m) = (old.len(), new.len());
    if n == 0 || (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        return Err(BlockEditError::unsupported(
            UnsupportedCause::ShowOperatorsNotFound,
        ));
    }
    let table = lcs(old, new);
    let at = |i: usize, j: usize| table.get(i * (m + 1) + j).copied().unwrap_or(0);
    let mut out: Vec<Fate> = Vec::with_capacity(n);
    let mut hunk = Hunk::default();
    let mut prefix: Vec<String> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        let same = i < n && j < m && old.get(i).map(|w| w.text.as_str()) == new.get(j).copied();
        if same && at(i, j) == at(i + 1, j + 1) + 1 {
            hunk.flush(old, &mut out, &mut prefix);
            out.push(with_prefix(Fate::Keep, &mut prefix, old.get(i)));
            i += 1;
            j += 1;
        } else if j >= m || (i < n && at(i + 1, j) >= at(i, j + 1)) {
            hunk.old.push(i);
            i += 1;
        } else {
            hunk.new
                .push(new.get(j).copied().unwrap_or_default().to_owned());
            j += 1;
        }
    }
    hunk.flush(old, &mut out, &mut prefix);
    if !prefix.is_empty() {
        // Only reachable when every old word was deleted and new words remain,
        // which the pairing rule makes impossible; refuse rather than drop text.
        return Err(BlockEditError::unsupported(
            UnsupportedCause::ShowOperatorsNotFound,
        ));
    }
    Ok(out)
}

/// New words with no old word of their own.
pub(super) fn merged(fates: &[Fate], new: &[&str]) -> usize {
    let placed = fates.iter().filter(|f| !matches!(f, Fate::Delete)).count();
    new.len().saturating_sub(placed)
}

/// `dp[i][j]`: the LCS length of `old[i..]` and `new[j..]`, row-major.
fn lcs(old: &[Word], new: &[&str]) -> Vec<u32> {
    let (n, m) = (old.len(), new.len());
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let eq = old.get(i).map(|w| w.text.as_str()) == new.get(j).copied();
            let v = if eq {
                dp.get((i + 1) * (m + 1) + j + 1).copied().unwrap_or(0) + 1
            } else {
                let down = dp.get((i + 1) * (m + 1) + j).copied().unwrap_or(0);
                let right = dp.get(i * (m + 1) + j + 1).copied().unwrap_or(0);
                down.max(right)
            };
            if let Some(cell) = dp.get_mut(i * (m + 1) + j) {
                *cell = v;
            }
        }
    }
    dp
}

#[derive(Default)]
struct Hunk {
    old: Vec<usize>,
    new: Vec<String>,
}

impl Hunk {
    fn flush(&mut self, words: &[Word], out: &mut Vec<Fate>, prefix: &mut Vec<String>) {
        let old = std::mem::take(&mut self.old);
        let mut new = std::mem::take(&mut self.new).into_iter();
        if old.is_empty() {
            let extras: Vec<String> = new.collect();
            if extras.is_empty() {
                return;
            }
            let at = out.iter().rposition(|f| !matches!(f, Fate::Delete));
            match at.and_then(|k| Some((out.get_mut(k)?, words.get(k)?))) {
                Some((f, w)) => append(f, &extras, Some(&w.text)),
                None => prefix.extend(extras),
            }
            return;
        }
        for &k in &old {
            let own = words.get(k).map(|w| w.text.as_str());
            out.push(match new.next() {
                None => Fate::Delete,
                Some(t) if Some(t.as_str()) == own => Fate::Keep,
                Some(t) => Fate::Replace(t),
            });
        }
        let extras: Vec<String> = new.collect();
        if extras.is_empty() {
            return;
        }
        let first = out.len().saturating_sub(old.len());
        let at = out
            .iter()
            .rposition(|f| !matches!(f, Fate::Delete))
            .filter(|&k| k >= first);
        match at.and_then(|k| Some((out.get_mut(k)?, words.get(k)?))) {
            Some((f, w)) => append(f, &extras, Some(&w.text)),
            None => prefix.extend(extras),
        }
    }
}

/// `f` with `extras` appended; a kept word becomes a replacement of its own
/// text plus the extras (`own` is its text when known).
fn append(f: &mut Fate, extras: &[String], own: Option<&str>) {
    let base = match f {
        Fate::Replace(s) => std::mem::take(s),
        Fate::Keep => own.unwrap_or_default().to_owned(),
        Fate::Delete => String::new(),
    };
    let mut parts = vec![base];
    parts.extend(extras.iter().cloned());
    *f = Fate::Replace(parts.join(" "));
}

/// `fate` for the next placed word, taking any waiting prefix words.
fn with_prefix(fate: Fate, prefix: &mut Vec<String>, word: Option<&Word>) -> Fate {
    if prefix.is_empty() {
        return fate;
    }
    let own = match &fate {
        Fate::Replace(s) => s.clone(),
        _ => word.map(|w| w.text.clone()).unwrap_or_default(),
    };
    let mut parts = std::mem::take(prefix);
    parts.push(own);
    Fate::Replace(parts.join(" "))
}
