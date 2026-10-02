//! The PaddleOCR-VL greedy decode loop, independent of the model runtime:
//! the caller supplies one decoder step as a closure.

/// New tokens generated per region unless the caller asks for fewer.
pub const DEFAULT_MAX_NEW_TOKENS: usize = 2048;

/// Hard ceiling on new tokens per region, whatever the caller asks for.
pub const MAX_NEW_TOKENS_CAP: usize = 8192;

/// Why decoding stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    /// The model emitted end-of-sequence.
    EndOfSequence,
    /// The token ceiling was reached first; the text may be cut short.
    TokenLimit,
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
#[allow(clippy::cast_precision_loss)] // token counts are below MAX_NEW_TOKENS_CAP
pub fn greedy<E>(
    eos: u32,
    max_new_tokens: usize,
    mut step: impl FnMut(Option<u32>) -> Result<Vec<f32>, E>,
) -> Result<Decoded, DecodeError<E>> {
    let limit = max_new_tokens.min(MAX_NEW_TOKENS_CAP);
    let mut tokens = Vec::new();
    let mut prob_sum = 0f32;
    let mut stop = StopReason::TokenLimit;
    let mut logits = step(None).map_err(DecodeError::Step)?;
    while tokens.len() < limit {
        let (tok, p) = pick(&logits).ok_or(DecodeError::NoLogits)?;
        if tok == eos {
            stop = StopReason::EndOfSequence;
            break;
        }
        tokens.push(tok);
        prob_sum += p;
        if tokens.len() == limit {
            break;
        }
        logits = step(Some(tok)).map_err(DecodeError::Step)?;
    }
    let mean_probability = (!tokens.is_empty()).then(|| prob_sum / tokens.len() as f32);
    Ok(Decoded {
        tokens,
        stop,
        mean_probability,
    })
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
