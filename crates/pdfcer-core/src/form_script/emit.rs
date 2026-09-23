//! Author the exact `/JS` text of a whitelisted helper — the **inverse** of
//! [`classify`](super::classify), over the same closed set (`Pass 308.6`,
//! request `G024`).
//!
//! # Why an emitter is safe when an interpreter is not
//!
//! Nothing here widens what pdfcer will represent. There is no `&str`-taking
//! route into a `/AA` entry, because the input is a [`ScriptClass`] — a value
//! that can only be built out of the same whitelist
//! [`classify`](super::classify) recognises. **Arbitrary JavaScript remains
//! unrepresentable**, and that is the property, not a limitation to work
//! around: an operator cannot ask pdfcer for a script pdfcer cannot also read
//! back and describe.
//!
//! `pdfcer-gui` put it as a boundary finding rather than a request: *a crate
//! that can classify a closed set of values and cannot construct one is a
//! boundary drawn on the read side only.* This is the other side.
//!
//! # The acceptance criterion is a round trip, not a set of assertions
//!
//! For every variant of every helper, `classify(emit(x), trigger) == x`. That
//! pins the emitter against the parser — including the case-sensitivity
//! [`SimpleOp::from_code`](super::SimpleOp::from_code) deliberately enforces,
//! where `"Sum"` is not `"SUM"` — rather than against a second reading of
//! Acrobat's behaviour that could drift from the first.
//!
//! # TWO THINGS DO NOT ROUND-TRIP, AND BOTH ARE REFUSALS RATHER THAN GAPS
//!
//! [`ScriptClass::Custom`] is the obvious one: it holds no parameters, so
//! there is nothing to emit. It is the safe default for *everything pdfcer
//! declined to recognise*, and reconstructing a call from it would be
//! inventing one.
//!
//! The second is not obvious and the request did not anticipate it.
//! [`AdvisoryHelper::Keystroke`](super::AdvisoryHelper::Keystroke) **captures
//! only the helper's NAME** — the classifier matches the whole `AF*_Keystroke`
//! family by name shape rather than enumerating it, precisely because nothing
//! is computed from the match. Its arguments are never read, so they cannot be
//! written back. Emitting `AFNumber_Keystroke()` for something that was
//! `AFNumber_Keystroke(2, 0, 0, 0, "", true)` would **destroy the input filter
//! while reporting success** — the defect shape this project met twice in one
//! day as `G022` and `G023`.
//!
//! ⇒ *A type that captures a value for DISCLOSURE is not automatically a type
//! that can reconstruct it.* `Keystroke` was always a disclosure-only value;
//! it looked writable because it sits in an enum whose other variant is not.
//!
//! The keystroke twin a format helper needs is therefore **derived from the
//! format helper** ([`keystroke_twin`]), not round-tripped from an
//! `AdvisoryHelper` — which is also what Acrobat does: the two calls of a pair
//! carry the same arguments under two names.

use super::{AdvisoryHelper, CalcHelper, FormatHelper, ScriptClass};

/// Why a [`ScriptClass`] could not be turned back into script text.
///
/// A named enum rather than a bare `None` so a caller can say WHICH refusal
/// it met — the two are different sentences for an operator, and a control
/// that offers to replace a `Custom` script needs to know it is displacing
/// something rather than that emission merely failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotEmittable {
    /// [`ScriptClass::Custom`] — pdfcer never recognised this script, so it
    /// holds no parameters to write back.
    Custom,
    /// [`AdvisoryHelper::Keystroke`](super::AdvisoryHelper::Keystroke) — the
    /// classifier keeps the helper's name and discards its arguments, so
    /// re-emitting it would silently drop the filter. See the module header.
    KeystrokeArgumentsNotCaptured,
}

impl NotEmittable {
    /// A sentence an operator can act on.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Custom => {
                "pdfcer does not recognise this script, so it cannot write one like it -- \
                 only the helpers it can also read back are authorable"
            }
            Self::KeystrokeArgumentsNotCaptured => {
                "pdfcer records which keystroke filter a field has, not what it was called \
                 with, so it cannot rewrite one -- set the format instead, which writes the \
                 matching keystroke filter as a pair"
            }
        }
    }
}

/// The `/JS` text for a recognised helper, or why it cannot be written.
///
/// # Errors
///
/// [`NotEmittable`] for the two classes that hold no reconstructable
/// parameters — see the module header, which argues why both are refusals
/// rather than gaps to close later.
pub fn emit(class: &ScriptClass) -> Result<Vec<u8>, NotEmittable> {
    match class {
        ScriptClass::Calculate(c) => Ok(emit_calc(c)),
        ScriptClass::Format(f) => Ok(emit_format(f)),
        ScriptClass::Advisory(AdvisoryHelper::RangeValidate { lower, upper }) => {
            Ok(emit_range_validate(*lower, *upper))
        }
        ScriptClass::Advisory(AdvisoryHelper::Keystroke { .. }) => {
            Err(NotEmittable::KeystrokeArgumentsNotCaptured)
        }
        ScriptClass::Custom => Err(NotEmittable::Custom),
    }
}

/// `AFSimple_Calculate("OP", ["a", "b"])`.
fn emit_calc(helper: &CalcHelper) -> Vec<u8> {
    let CalcHelper::Simple { op, operands } = helper;
    let mut out = b"AFSimple_Calculate(".to_vec();
    push_str(&mut out, op.code().as_bytes());
    out.extend_from_slice(b", [");
    for (i, operand) in operands.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b", ");
        }
        push_str(&mut out, operand);
    }
    out.extend_from_slice(b"]);");
    out
}

/// The six `AF*_Format` calls, each with the argument list its classifier
/// requires — six for `AFNumber_Format`, two for `AFPercent_Format`, one for
/// the rest.
fn emit_format(helper: &FormatHelper) -> Vec<u8> {
    match helper {
        FormatHelper::Number {
            decimals,
            separator_style,
            negative_style,
            currency_style,
            currency,
            prepend_currency,
        } => {
            let mut out = b"AFNumber_Format(".to_vec();
            push_int(&mut out, *decimals);
            out.extend_from_slice(b", ");
            push_int(&mut out, *separator_style);
            out.extend_from_slice(b", ");
            push_int(&mut out, *negative_style);
            out.extend_from_slice(b", ");
            push_int(&mut out, *currency_style);
            out.extend_from_slice(b", ");
            push_str(&mut out, currency);
            out.extend_from_slice(if *prepend_currency {
                b", true);".as_slice()
            } else {
                b", false);".as_slice()
            });
            out
        }
        FormatHelper::Percent {
            decimals,
            separator_style,
        } => {
            let mut out = b"AFPercent_Format(".to_vec();
            push_int(&mut out, *decimals);
            out.extend_from_slice(b", ");
            push_int(&mut out, *separator_style);
            out.extend_from_slice(b");");
            out
        }
        FormatHelper::Date { index } => single_int_call(b"AFDate_Format", *index),
        FormatHelper::Time { index } => single_int_call(b"AFTime_Format", *index),
        FormatHelper::Special { selector } => single_int_call(b"AFSpecial_Format", *selector),
        FormatHelper::DateEx { format } => {
            let mut out = b"AFDate_FormatEx(".to_vec();
            push_str(&mut out, format);
            out.extend_from_slice(b");");
            out
        }
    }
}

/// `AFRange_Validate(bGreaterThan, nGreaterThan, bLessThan, nLessThan)`.
///
/// A disabled bound writes `false` beside a `0` placeholder, which is what
/// Acrobat writes and what the classifier's *"a disabled bound's number is
/// meaningless"* reading expects: the boolean is the only thing that decides
/// whether the number is read at all.
fn emit_range_validate(lower: Option<f64>, upper: Option<f64>) -> Vec<u8> {
    let mut out = b"AFRange_Validate(".to_vec();
    push_bound(&mut out, lower);
    out.extend_from_slice(b", ");
    push_bound(&mut out, upper);
    out.extend_from_slice(b");");
    out
}

fn push_bound(out: &mut Vec<u8>, bound: Option<f64>) {
    match bound {
        Some(v) => {
            out.extend_from_slice(b"true, ");
            push_num(out, v);
        }
        None => out.extend_from_slice(b"false, 0"),
    }
}

fn single_int_call(name: &[u8], value: i64) -> Vec<u8> {
    let mut out = name.to_vec();
    out.push(b'(');
    push_int(&mut out, value);
    out.extend_from_slice(b");");
    out
}

fn push_int(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(v.to_string().as_bytes());
}

/// A number as the shortest text that reads back as the same `f64`.
///
/// `{}` on an `f64` is Rust's shortest round-trip form, so `1.0` prints as
/// `1` and `0.1` as `0.1` rather than as seventeen digits asserting a
/// precision the operator never gave — the same correction `Pass 308.0` made
/// to `/MK` colour components, for the same reason.
///
/// A non-finite bound cannot occur: it would have to have been READ from a
/// script, and [`shape`](super::shape) does not produce one.
fn push_num(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(v.to_string().as_bytes());
}

/// A JavaScript double-quoted string literal.
///
/// Backslash and double-quote are escaped; nothing else is. The classifier's
/// own literal reader accepts exactly this, and widening the escape set would
/// emit text one side of the round trip understands and the other does not.
///
/// **Bytes, not UTF-8.** A PDF field name is a text string with no UTF-8
/// guarantee, and re-encoding one would break the lookup an operand exists
/// for — the reason [`CalcHelper::Simple`](super::CalcHelper::Simple) keeps
/// operands as raw bytes in the first place.
fn push_str(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'"');
    for &b in bytes {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
}

/// The `AF*_Keystroke` call that belongs beside a format helper, as Acrobat
/// writes the pair.
///
/// # Why a format helper owes a keystroke twin
///
/// Acrobat's Format tab emits **two** scripts: the display formatter into
/// `/AA` `/F` and an input filter of the same family into `/AA` `/K`, carrying
/// the same arguments. A file with one and not the other is not one Acrobat
/// authored, and the next person to open its Format dialog sees a format with
/// no input filter.
///
/// The pairing lives here rather than in the caller because it is a property
/// of *what a conforming producer writes*, not a choice a shell should have to
/// know about — the judgement `pdfcer-gui` explicitly left to this crate.
///
/// `AFDate_FormatEx` pairs with `AFDate_KeystrokeEx`; the other five simply
/// swap `_Format` for `_Keystroke`.
#[must_use]
pub fn keystroke_twin(helper: &FormatHelper) -> Vec<u8> {
    let formatted = emit_format(helper);
    let (from, to): (&[u8], &[u8]) = match helper {
        FormatHelper::DateEx { .. } => (b"AFDate_FormatEx(", b"AFDate_KeystrokeEx("),
        FormatHelper::Number { .. } => (b"AFNumber_Format(", b"AFNumber_Keystroke("),
        FormatHelper::Percent { .. } => (b"AFPercent_Format(", b"AFPercent_Keystroke("),
        FormatHelper::Date { .. } => (b"AFDate_Format(", b"AFDate_Keystroke("),
        FormatHelper::Time { .. } => (b"AFTime_Format(", b"AFTime_Keystroke("),
        FormatHelper::Special { .. } => (b"AFSpecial_Format(", b"AFSpecial_Keystroke("),
    };
    // `strip_prefix` rather than a slice: the prefix is guaranteed by the
    // match above, and a panic-free crate does not ask the reader to take that
    // on trust. An impossible `None` falls back to the format call itself,
    // which classifies as a FORMAT on `/K` and therefore as `Custom` -- a
    // visible wrong answer rather than a crash.
    let mut out = to.to_vec();
    out.extend_from_slice(formatted.strip_prefix(from).unwrap_or(&formatted));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::super::{SimpleOp, Trigger, classify};
    use super::*;

    /// Every emittable variant, so the round-trip test below is exhaustive by
    /// construction rather than by somebody remembering to extend it.
    fn every_helper() -> Vec<(ScriptClass, Trigger)> {
        vec![
            (
                ScriptClass::Calculate(CalcHelper::Simple {
                    op: SimpleOp::Sum,
                    operands: vec![b"Line1".to_vec(), b"Line2".to_vec()],
                }),
                Trigger::Calculate,
            ),
            (
                ScriptClass::Calculate(CalcHelper::Simple {
                    op: SimpleOp::Average,
                    operands: Vec::new(),
                }),
                Trigger::Calculate,
            ),
            (
                ScriptClass::Calculate(CalcHelper::Simple {
                    op: SimpleOp::Product,
                    operands: vec![b"A".to_vec()],
                }),
                Trigger::Calculate,
            ),
            (
                ScriptClass::Calculate(CalcHelper::Simple {
                    op: SimpleOp::Minimum,
                    operands: vec![b"A".to_vec(), b"B".to_vec(), b"C".to_vec()],
                }),
                Trigger::Calculate,
            ),
            (
                ScriptClass::Calculate(CalcHelper::Simple {
                    op: SimpleOp::Maximum,
                    operands: vec![b"A".to_vec()],
                }),
                Trigger::Calculate,
            ),
            (
                ScriptClass::Format(FormatHelper::Number {
                    decimals: 2,
                    separator_style: 0,
                    negative_style: 1,
                    currency_style: 0,
                    currency: b"$".to_vec(),
                    prepend_currency: true,
                }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::Number {
                    decimals: 0,
                    separator_style: 2,
                    negative_style: 0,
                    currency_style: 1,
                    currency: Vec::new(),
                    prepend_currency: false,
                }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::Percent {
                    decimals: 1,
                    separator_style: 0,
                }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::Date { index: 3 }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::DateEx {
                    format: b"yyyy-mm-dd".to_vec(),
                }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::Time { index: 1 }),
                Trigger::Format,
            ),
            (
                ScriptClass::Format(FormatHelper::Special { selector: 0 }),
                Trigger::Format,
            ),
            (
                ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                    lower: Some(1.0),
                    upper: Some(100.0),
                }),
                Trigger::Validate,
            ),
            (
                ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                    lower: Some(-2.5),
                    upper: None,
                }),
                Trigger::Validate,
            ),
            (
                ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                    lower: None,
                    upper: Some(0.1),
                }),
                Trigger::Validate,
            ),
            (
                ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                    lower: None,
                    upper: None,
                }),
                Trigger::Validate,
            ),
        ]
    }

    /// THE ACCEPTANCE CRITERION. `classify(emit(x)) == x`, for every
    /// emittable variant.
    ///
    /// This pins the emitter against the PARSER rather than against a second
    /// reading of Acrobat's behaviour, which is what makes it worth more than
    /// any number of hand-written expected strings: the parser already
    /// encodes the whitelist's exact semantics, including the
    /// case-sensitivity `SimpleOp::from_code` enforces.
    #[test]
    fn every_emittable_helper_round_trips_through_classify() {
        for (class, trigger) in every_helper() {
            let js = emit(&class).unwrap();
            let back = classify(&js, trigger);
            assert_eq!(
                back,
                class,
                "round trip failed for {}: emitted {:?}",
                class.token(),
                String::from_utf8_lossy(&js)
            );
        }
    }

    /// A keystroke twin classifies as the advisory it is, on the `/K` trigger.
    ///
    /// It does NOT round-trip to the format helper it came from — the twin is
    /// a different call with the same arguments, and the classifier keeps only
    /// its name. That asymmetry is the whole of why `keystroke_twin` derives
    /// from the format helper instead of from an `AdvisoryHelper`.
    #[test]
    fn a_keystroke_twin_is_recognised_as_a_keystroke_filter() {
        for (class, _) in every_helper() {
            let ScriptClass::Format(f) = &class else {
                continue;
            };
            let js = keystroke_twin(f);
            match classify(&js, Trigger::Keystroke) {
                ScriptClass::Advisory(AdvisoryHelper::Keystroke { name }) => {
                    assert!(
                        name.ends_with("_Keystroke") || name.ends_with("_KeystrokeEx"),
                        "{name}"
                    );
                }
                other => panic!(
                    "{:?} gave {:?}",
                    String::from_utf8_lossy(&js),
                    other.token()
                ),
            }
        }
    }

    /// The twin carries the FORMAT's arguments, byte for byte.
    ///
    /// Acrobat writes the pair that way, and a twin with different arguments
    /// would filter input the format cannot display.
    #[test]
    fn the_twin_differs_from_its_format_only_in_the_function_name() {
        let f = FormatHelper::Number {
            decimals: 2,
            separator_style: 0,
            negative_style: 1,
            currency_style: 0,
            currency: b"$".to_vec(),
            prepend_currency: true,
        };
        let format = emit_format(&f);
        let twin = keystroke_twin(&f);
        let args = |v: &[u8]| {
            v.iter()
                .position(|&b| b == b'(')
                .and_then(|i| v.get(i..))
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        };
        assert_eq!(args(&format), args(&twin));
        assert!(twin.starts_with(b"AFNumber_Keystroke("));
    }

    /// The two refusals, which are the safety property rather than a gap.
    #[test]
    fn custom_and_keystroke_are_refused_by_name() {
        assert_eq!(emit(&ScriptClass::Custom), Err(NotEmittable::Custom));
        assert_eq!(
            emit(&ScriptClass::Advisory(AdvisoryHelper::Keystroke {
                name: "AFNumber_Keystroke".to_owned(),
            })),
            Err(NotEmittable::KeystrokeArgumentsNotCaptured),
            "the classifier keeps the NAME and discards the arguments, so \
             re-emitting would drop the filter while reporting success"
        );
    }

    /// A field name containing a quote or a backslash survives the round trip.
    ///
    /// Legal in a PDF text string, and the one input that can break a
    /// hand-rolled emitter by closing its own literal early.
    #[test]
    fn an_operand_containing_a_quote_round_trips() {
        let class = ScriptClass::Calculate(CalcHelper::Simple {
            op: SimpleOp::Sum,
            operands: vec![br#"He said "hi""#.to_vec(), br"back\slash".to_vec()],
        });
        let js = emit(&class).unwrap();
        assert_eq!(classify(&js, Trigger::Calculate), class);
    }

    /// A non-UTF-8 field name survives, because operands are bytes.
    #[test]
    fn a_non_utf8_operand_round_trips() {
        let class = ScriptClass::Calculate(CalcHelper::Simple {
            op: SimpleOp::Sum,
            operands: vec![vec![0xFF, 0xFE, b'A']],
        });
        let js = emit(&class).unwrap();
        assert_eq!(classify(&js, Trigger::Calculate), class);
    }

    /// A bound reads back as the number given, not as seventeen digits.
    #[test]
    fn a_bound_is_written_at_the_precision_it_was_given() {
        let js = emit(&ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
            lower: Some(0.1),
            upper: Some(1.0),
        }))
        .unwrap();
        let text = String::from_utf8_lossy(&js).into_owned();
        assert!(text.contains("0.1"), "{text}");
        assert!(!text.contains("0.1000"), "{text}");
        assert!(text.contains("true, 1"), "{text}");
    }

    /// The trigger is part of the round trip, not decoration.
    ///
    /// A format helper read off the calculate trigger is a contradiction the
    /// classifier refuses — so an emitter that wrote one to the wrong key
    /// would produce a file pdfcer itself declines to act on.
    #[test]
    fn a_format_helper_on_the_calculate_trigger_does_not_classify_back() {
        let class = ScriptClass::Format(FormatHelper::Date { index: 0 });
        let js = emit(&class).unwrap();
        assert_eq!(classify(&js, Trigger::Calculate), ScriptClass::Custom);
    }
}
