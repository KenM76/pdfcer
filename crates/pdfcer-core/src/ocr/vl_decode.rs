//! The PaddleOCR-VL greedy decode loop, independent of the model runtime:
//! the caller supplies one decoder step as a closure.

/// New tokens generated per region unless the caller asks for fewer.
pub const DEFAULT_MAX_NEW_TOKENS: usize = 2048;

/// Hard ceiling on new tokens per region, whatever the caller asks for.
pub const MAX_NEW_TOKENS_CAP: usize = 8192;

/// Repeated consecutive lines after which decoding stops as a loop: the
/// model, given a picture it cannot read (an unlabelled chart), can repeat
/// one line until the token ceiling, minutes of CPU for nothing. A line
/// counts as a repeat when it is identical, or when only its first
/// [`MAX_VARYING_PREFIX`] tokens differ over a shared tail of at least
/// [`MIN_REPEATED_TAIL`] tokens: the model also loops on a numbered list
/// (`36. Material: …`, `37. Material: …`), which an exact match never sees.
pub const MAX_REPEATED_LINES: usize = 12;

/// Leading tokens a repeated line may vary in (a list ordinal of up to
/// four digits and its punctuation, tokenised one digit per token).
pub const MAX_VARYING_PREFIX: usize = 4;

/// Shortest shared tail that makes two lines with different leading
/// tokens a repeat, so short distinct lines (`1. a`, `2. b`) never are.
pub const MIN_REPEATED_TAIL: usize = 4;

/// Why decoding stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    /// The model emitted end-of-sequence.
    EndOfSequence,
    /// The token ceiling was reached first; the text may be cut short.
    TokenLimit,
    /// The last [`MAX_REPEATED_LINES`] lines repeated (identical, or alike
    /// but for a leading ordinal), so the model was looping; the first of
    /// them is kept, the rest are dropped, and anything after is lost.
    Repetition,
}

/// The outcome of one greedy decode.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// Generated ids, end-of-sequence excluded.
    pub tokens: Vec<u32>,
    /// Why decoding stopped.
    pub stop: StopReason,
    /// Mean softmax probability of the chosen tokens, `None` when no token
    /// was generated.
    pub mean_probability: Option<f32>,
}

/// Why decoding failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError<E> {
    /// The step closure failed.
    #[error("decoder step failed: {0}")]
    Step(E),
    /// A step returned no finite logit.
    #[error("the decoder returned no finite logits")]
    NoLogits,
}

/// The highest finite logit's index and its softmax probability.
fn pick(logits: &[f32]) -> Option<(u32, f32)> {
    let (best, max) = logits
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .fold(None, |acc: Option<(usize, f32)>, (i, &v)| match acc {
            Some((_, m)) if m >= v => acc,
            _ => Some((i, v)),
        })?;
    let sum: f32 = logits
        .iter()
        .filter(|v| v.is_finite())
        .map(|v| (v - max).exp())
        .sum();
    Some((u32::try_from(best).ok()?, 1.0 / sum))
}

/// Greedy decoding. `step(None)` runs the prompt and `step(Some(t))` feeds
/// the token just chosen; each returns the logits for the next position.
/// Generates at most `max_new_tokens`, clamped to [`MAX_NEW_TOKENS_CAP`], so
/// `step` runs at most that many plus one times.
///
/// # Errors
///
/// [`DecodeError::Step`] passes a step failure through;
/// [`DecodeError::NoLogits`] when a step's logits are empty or all
/// non-finite.
pub fn greedy<E>(
    eos: u32,
    max_new_tokens: usize,
    step: impl FnMut(Option<u32>) -> Result<Vec<f32>, E>,
) -> Result<Decoded, DecodeError<E>> {
    greedy_guarded(eos, None, max_new_tokens, step)
}

/// [`greedy`], also stopping with [`StopReason::Repetition`] once the last
/// [`MAX_REPEATED_LINES`] lines, split on the `newline` token, are
/// non-empty repeats of one another (see [`MAX_REPEATED_LINES`]). The
/// returned tokens then end after the first of those lines: the repeats are
/// the model's invention, not the page's text.
///
/// # Errors
///
/// As [`greedy`].
#[allow(clippy::cast_precision_loss)] // token counts are below MAX_NEW_TOKENS_CAP
pub fn greedy_guarded<E>(
    eos: u32,
    newline: Option<u32>,
    max_new_tokens: usize,
    mut step: impl FnMut(Option<u32>) -> Result<Vec<f32>, E>,
) -> Result<Decoded, DecodeError<E>> {
    let limit = max_new_tokens.min(MAX_NEW_TOKENS_CAP);
    let mut tokens = Vec::new();
    let mut probs = Vec::new();
    let mut stop = StopReason::TokenLimit;
    let mut logits = step(None).map_err(DecodeError::Step)?;
    while tokens.len() < limit {
        let (tok, p) = pick(&logits).ok_or(DecodeError::NoLogits)?;
        if tok == eos {
            stop = StopReason::EndOfSequence;
            break;
        }
        tokens.push(tok);
        probs.push(p);
        if let Some(keep) = newline
            .filter(|&n| n == tok)
            .and_then(|n| repeating(&tokens, n))
        {
            tokens.truncate(keep);
            probs.truncate(keep);
            stop = StopReason::Repetition;
            break;
        }
        if tokens.len() == limit {
            break;
        }
        logits = step(Some(tok)).map_err(DecodeError::Step)?;
    }
    let mean_probability =
        (!probs.is_empty()).then(|| probs.iter().sum::<f32>() / probs.len() as f32);
    Ok(Decoded {
        tokens,
        stop,
        mean_probability,
    })
}

/// When `tokens`, ending in `newline`, end with [`MAX_REPEATED_LINES`]
/// non-empty lines that each repeat the last, the length that keeps only the
/// first of them (its newline included).
fn repeating(tokens: &[u32], newline: u32) -> Option<usize> {
    let body = tokens.strip_suffix(&[newline]).unwrap_or(tokens);
    let mut lines = body.rsplit(|&t| t == newline);
    let last = lines.next().filter(|l| !l.is_empty())?;
    let mut dropped = last.len() + 1;
    for l in lines.take(MAX_REPEATED_LINES - 2) {
        if !repeats(l, last) {
            return None;
        }
        dropped += l.len() + 1;
    }
    let first = body.len().checked_sub(dropped)?;
    let first_line = body.get(..first)?.rsplit(|&t| t == newline).next()?;
    repeats(first_line, last).then(|| tokens.len() - dropped)
}

/// Whether `a` repeats `b`: identical, or differing only in their first
/// [`MAX_VARYING_PREFIX`] tokens over a tail of [`MIN_REPEATED_TAIL`] or more.
fn repeats(a: &[u32], b: &[u32]) -> bool {
    let tail = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    a == b || (tail >= MIN_REPEATED_TAIL && a.len().max(b.len()) - tail <= MAX_VARYING_PREFIX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Logits favouring `tok` among `n` classes.
    fn favour(tok: usize, n: usize) -> Vec<f32> {
        (0..n).map(|i| if i == tok { 5.0 } else { 0.0 }).collect()
    }

    #[test]
    fn stops_at_end_of_sequence_and_feeds_each_choice_back() {
        let script = [3usize, 1, 4, 0];
        let mut fed = Vec::new();
        let d = greedy::<()>(0, 100, |t| {
            fed.push(t);
            Ok(favour(script[fed.len() - 1], 6))
        })
        .unwrap();
        assert_eq!(d.tokens, [3, 1, 4]);
        assert_eq!(d.stop, StopReason::EndOfSequence);
        assert_eq!(fed, [None, Some(3), Some(1), Some(4)]);
        let p = 5f32.exp() / (5f32.exp() + 5.0);
        assert!((d.mean_probability.unwrap() - p).abs() < 1e-6);
    }

    #[test]
    fn the_token_ceiling_bounds_the_steps() {
        let mut steps = 0;
        let d = greedy::<()>(0, 5, |_| {
            steps += 1;
            Ok(favour(1, 3))
        })
        .unwrap();
        assert_eq!(
            (d.tokens.len(), d.stop, steps),
            (5, StopReason::TokenLimit, 5)
        );
        let mut steps = 0;
        let d = greedy::<()>(0, usize::MAX, |_| {
            steps += 1;
            if steps > MAX_NEW_TOKENS_CAP {
                return Err(());
            }
            Ok(favour(1, 3))
        })
        .unwrap();
        assert_eq!(
            d.tokens.len(),
            MAX_NEW_TOKENS_CAP,
            "the cap overrides the caller"
        );
        assert_eq!(steps, MAX_NEW_TOKENS_CAP);
    }

    #[test]
    fn a_looping_line_stops_decoding() {
        // "7 8\n" forever; newline is 9.
        let cycle = [7usize, 8, 9];
        let mut n = 0;
        let d = greedy_guarded::<()>(0, Some(9), 1000, |_| {
            n += 1;
            Ok(favour(cycle[(n - 1) % 3], 10))
        })
        .unwrap();
        assert_eq!(d.stop, StopReason::Repetition);
        assert_eq!(d.tokens, [7, 8, 9], "only the first repeat is kept");
        // Distinct lines, blank lines and no newline token never trip it.
        let mut n = 0;
        let d = greedy_guarded::<()>(0, Some(9), 200, |_| {
            n += 1;
            Ok(favour(if n % 2 == 0 { 9 } else { 1 + n % 7 }, 10))
        })
        .unwrap();
        assert_eq!(d.stop, StopReason::TokenLimit);
        let d = greedy_guarded::<()>(0, Some(9), 50, |_| Ok(favour(9, 10))).unwrap();
        assert_eq!(d.stop, StopReason::TokenLimit);
        let d = greedy_guarded::<()>(0, None, 60, |_| Ok(favour(9, 10))).unwrap();
        assert_eq!(d.stop, StopReason::TokenLimit);
    }

    /// Lines `<ordinal digits> 5 6 7 8 \n`, newline 9, ordinals counting up
    /// from `first` with one token per digit (digit d is token 10 + d).
    fn numbered(first: usize, body: &[usize]) -> impl FnMut(Option<u32>) -> Result<Vec<f32>, ()> {
        let mut script = Vec::new();
        for n in first..first + 40 {
            script.extend(n.to_string().bytes().map(|d| 10 + usize::from(d - b'0')));
            script.extend_from_slice(body);
            script.push(9);
        }
        let mut i = 0;
        move |_| {
            i += 1;
            Ok(favour(script[i - 1], 20))
        }
    }

    #[test]
    fn a_looping_numbered_list_stops_decoding() {
        // G196: "36. Material: …" counting up, across a digit-count change.
        let d = greedy_guarded(0, Some(9), 1000, numbered(95, &[5, 6, 7, 8])).unwrap();
        assert_eq!(d.stop, StopReason::Repetition);
        assert_eq!(
            d.tokens,
            [19, 15, 5, 6, 7, 8, 9],
            "line 95 kept, 96-106 dropped"
        );
        // A short shared tail is a real list, not a loop.
        let d = greedy_guarded(0, Some(9), 150, numbered(1, &[5, 6])).unwrap();
        assert_eq!(d.stop, StopReason::TokenLimit);
    }

    #[test]
    fn repeats_needs_a_long_tail_and_a_short_prefix() {
        assert!(repeats(&[1, 2], &[1, 2]));
        assert!(repeats(&[1, 5, 6, 7, 8], &[2, 3, 5, 6, 7, 8]));
        assert!(!repeats(&[1, 6, 7, 8], &[2, 6, 7, 8, 8]));
        assert!(!repeats(
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[9, 9, 9, 9, 9, 6, 7, 8, 9]
        ));
        assert!(!repeats(
            &[1, 2, 3, 4, 5, 5, 6, 7, 8],
            &[9, 9, 9, 9, 9, 5, 6, 7, 8]
        ));
    }

    #[test]
    fn an_immediate_end_yields_no_text() {
        let d = greedy::<()>(2, 10, |_| Ok(favour(2, 3))).unwrap();
        assert_eq!((d.tokens.len(), d.mean_probability), (0, None));
        assert_eq!(d.stop, StopReason::EndOfSequence);
        let d = greedy::<()>(2, 0, |_| Ok(favour(1, 3))).unwrap();
        assert_eq!((d.tokens.len(), d.stop), (0, StopReason::TokenLimit));
    }

    #[test]
    fn non_finite_logits_are_skipped_or_refused() {
        let mut first = true;
        let d = greedy::<()>(0, 10, |_| {
            let l = if first {
                vec![f32::NAN, 1.0, f32::INFINITY, 0.5]
            } else {
                favour(0, 2)
            };
            first = false;
            Ok(l)
        })
        .unwrap();
        assert_eq!(d.tokens, [1]);
        assert!(matches!(
            greedy::<()>(0, 10, |_| Ok(vec![f32::NAN])),
            Err(DecodeError::NoLogits)
        ));
        assert!(matches!(
            greedy::<()>(0, 10, |_| Ok(vec![])),
            Err(DecodeError::NoLogits)
        ));
    }

    #[test]
    fn a_step_failure_passes_through() {
        let r = greedy(0, 10, |t| {
            if t.is_some() {
                Err("boom")
            } else {
                Ok(favour(1, 2))
            }
        });
        assert!(matches!(r, Err(DecodeError::Step("boom"))));
    }
}
