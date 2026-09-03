//! Recognition and native recomputation of well-known Acrobat form-field
//! JavaScript helpers — **decision 009 posture B**, without an interpreter.
//!
//! # What this module is for
//!
//! Ordinary interactive PDF forms do arithmetic and presentation with a
//! handful of Acrobat-generated helper calls: `AFSimple_Calculate` for
//! totals, `AF*_Format` for how a number, date or postcode is displayed.
//! pdfcer **never executes JavaScript** (standing rules R53–R57; decision 009
//! rejects a sandboxed engine outright), so before this module existed a
//! form's totals simply went stale the moment the operator changed an input:
//! posture A recognised that a field was script-driven, disclosed it, and
//! stopped there.
//!
//! Posture B closes that gap the only way that is compatible with
//! never running a script: **recognise the exact shape of Acrobat's own
//! generated call, and reimplement what it does, natively, in Rust.** A
//! recognised `AFSimple_Calculate("SUM", ["A","B"])` is not run — it is
//! *read*, and pdfcer adds two numbers.
//!
//! # The three-layer split, and why each layer is separate
//!
//! | Layer | Module | Answers |
//! |---|---|---|
//! | Shape | [`shape`] | "Is this one call with literal arguments?" |
//! | Identity | this module | "Is that call a whitelisted helper, and with what parameters?" |
//! | Effect | [`calc`], [`format`] | "What does that helper produce?" |
//!
//! They are separate because they fail differently and the failures must not
//! be confusable. A shape failure means *arbitrary code* — refuse. An
//! identity failure means *a function pdfcer does not know* — refuse. An
//! effect failure means *a known helper whose operands are unusable* —
//! which is a disclosable, per-field outcome, not a refusal to recognise.
//! Collapsing the layers would lose that distinction and, with it, the
//! ability to tell an operator which of the three happened.
//!
//! # The rule that governs every judgement call here
//!
//! **A false positive is far worse than a false negative** (argued at length
//! in [`shape`]). Failing to recognise a real helper costs a recompute pdfcer
//! could have offered, and the stale value is disclosed as stale. Wrongly
//! recognising author code writes a wrong number into a real document, and a
//! wrong total looks exactly like a right one. So every ambiguity resolves
//! to [`ScriptClass::Custom`].
//!
//! # What a recompute is, and what it is never
//!
//! Per decision 009 §5.1, and binding:
//!
//! - It is an **operator-invoked, undoable edit** — never a load-time or
//!   save-time side effect. Merely opening and saving a form must not change
//!   a computed `/V`, or pdfcer would be an editor that silently rewrites
//!   documents it was only asked to look at.
//! - **The source script is left in place.** If pdfcer's native recompute
//!   ever diverges from Acrobat's real semantics on an edge case, a
//!   downstream JavaScript-executing reader recomputes and *corrects* pdfcer's
//!   value. Stripping the script would freeze a possibly-divergent value as
//!   authoritative — the opposite of fail-safe.
//! - **Format helpers never touch `/V`.** They choose a display string; the
//!   stored value stays raw. The two paths do not merge, and a formatted
//!   string being baked into `/V` would be a data-loss bug, not a cosmetic
//!   one: `"$1,234.00"` does not parse back as `1234`.
//!
//! # Non-goals, permanently
//!
//! No interpreter, no expression evaluation, no DOM, no `event` object, no
//! trigger dispatch. Recognition is a lookup table over a fixed grammar, and
//! the correct response to an unrecognised-but-legitimate script is a
//! disclosed false negative — never a bigger parser.

pub mod calc;
pub mod datetime;
pub mod disclose;
pub mod format;
pub mod inventory;
pub mod recompute;
pub mod shape;

use shape::{Call, Literal};

/// Which `/AA` trigger a script hangs off (§12.6.3).
///
/// Carried alongside the classification because the *same* helper text means
/// different things on different triggers, and the difference decides
/// whether a value may be written at all. The pairing is checked rather than
/// assumed: a format helper found on the calculate trigger is a
/// contradiction, and pdfcer treats a contradiction as a reason not to act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// `/C` — calculate. The only trigger whose helper may change `/V`.
    Calculate,
    /// `/F` — format. Display only, never `/V`.
    Format,
    /// `/V` — validate. Advisory in pdfcer; never enforced by execution.
    Validate,
    /// `/K` — keystroke. Advisory; pdfcer's fills are operator-reviewed.
    Keystroke,
}

impl Trigger {
    /// The `/AA` dictionary key this trigger is stored under.
    #[must_use]
    pub const fn key(self) -> &'static [u8] {
        match self {
            Self::Calculate => b"C",
            Self::Format => b"F",
            Self::Validate => b"V",
            Self::Keystroke => b"K",
        }
    }

    /// A stable, locale-invariant token for CLI output and disclosure lines.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Calculate => "calculate",
            Self::Format => "format",
            Self::Validate => "validate",
            Self::Keystroke => "keystroke",
        }
    }
}

/// The five `AFSimple_Calculate` operations (decision 009 §6).
///
/// The wire spellings are Acrobat's, and matching is **case-sensitive and
/// exact**: `"Sum"` is not `"SUM"`. Accepting variants would be guessing at
/// what a non-Acrobat producer meant, and this whitelist exists precisely to
/// recognise Acrobat's own output rather than anything that resembles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimpleOp {
    /// `"SUM"` — the total of the operands.
    Sum,
    /// `"AVG"` — the arithmetic mean.
    Average,
    /// `"PRD"` — the product.
    Product,
    /// `"MIN"` — the least operand.
    Minimum,
    /// `"MAX"` — the greatest operand.
    Maximum,
}

impl SimpleOp {
    /// Match an operation code exactly.
    #[must_use]
    pub fn from_code(code: &[u8]) -> Option<Self> {
        match code {
            b"SUM" => Some(Self::Sum),
            b"AVG" => Some(Self::Average),
            b"PRD" => Some(Self::Product),
            b"MIN" => Some(Self::Minimum),
            b"MAX" => Some(Self::Maximum),
            _ => None,
        }
    }

    /// The wire code, for disclosure and round-tripping a description.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Average => "AVG",
            Self::Product => "PRD",
            Self::Minimum => "MIN",
            Self::Maximum => "MAX",
        }
    }

    /// The operation named in prose, for an operator-facing disclosure.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Average => "average",
            Self::Product => "product",
            Self::Minimum => "minimum",
            Self::Maximum => "maximum",
        }
    }
}

/// A recognised calculation helper — one that may change a field's `/V`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcHelper {
    /// `AFSimple_Calculate(op, [operands])`.
    ///
    /// Operand names are the **fully-qualified field names** the script
    /// passed, kept as raw bytes: a PDF field name is a text string with no
    /// UTF-8 guarantee, and re-encoding one would break the lookup it exists
    /// for.
    Simple {
        /// Which of the five operations.
        op: SimpleOp,
        /// The operand field names, in source order. Order is preserved even
        /// though none of the five operations is order-sensitive, because
        /// the disclosure quotes it back and an operator matching it against
        /// the Acrobat dialog they remember should see their own list.
        operands: Vec<Vec<u8>>,
    },
}

/// A recognised formatting helper — display only, **never** `/V`.
///
/// Parameters are captured as read; their *meaning* is [`format`]'s
/// business. Separating capture from interpretation means an unsourced
/// parameter value can be recognised and disclosed ("this field is formatted
/// by AFNumber_Format") while still refusing to render it, which is strictly
/// more useful than classifying the whole script `Custom`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatHelper {
    /// `AFNumber_Format(nDec, sepStyle, negStyle, currStyle, strCurrency,
    /// bCurrencyPrepend)`.
    Number {
        /// Digits after the decimal separator.
        decimals: i64,
        /// Grouping/decimal separator style.
        separator_style: i64,
        /// How a negative value is shown.
        negative_style: i64,
        /// Currency-symbol placement style.
        currency_style: i64,
        /// The currency symbol's raw bytes (often empty).
        currency: Vec<u8>,
        /// Whether the symbol precedes the number.
        prepend_currency: bool,
    },
    /// `AFPercent_Format(nDec, sepStyle)`.
    Percent {
        /// Digits after the decimal separator.
        decimals: i64,
        /// Grouping/decimal separator style.
        separator_style: i64,
    },
    /// `AFDate_Format(pdfFormat)` — a predefined format by index.
    Date {
        /// The index into Acrobat's predefined date-format table.
        index: i64,
    },
    /// `AFDate_FormatEx(cFormat)` — an explicit format string.
    DateEx {
        /// The format string's raw bytes.
        format: Vec<u8>,
    },
    /// `AFTime_Format(pdfFormat)` — a predefined time format by index.
    Time {
        /// The index into Acrobat's predefined time-format table.
        index: i64,
    },
    /// `AFSpecial_Format(psf)` — zip, zip+4, phone, or social-security mask.
    Special {
        /// The predefined special-format selector.
        selector: i64,
    },
}

/// A helper pdfcer **recognises and discloses but deliberately does not
/// reimplement** in this cut (decision 009 §6).
///
/// These are not failures. Each names a real constraint an operator benefits
/// from seeing — "this field only accepts 1–100" is worth surfacing even
/// though pdfcer will not enforce it by execution. Classifying them as
/// `Custom` would throw that information away; reimplementing them would
/// enforce a rule pdfcer cannot guarantee it evaluates the way Acrobat does,
/// on a surface (rejecting operator input) where being wrong is loud and
/// obstructive rather than merely stale.
///
/// Not `Eq`: a range bound is an `f64` read verbatim from the script, and
/// `f64` is only `PartialEq`. Nothing compares two advisories for identity,
/// so there is no reason to force a total ordering onto a value whose whole
/// purpose is to be displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum AdvisoryHelper {
    /// An `AF*_Keystroke` input filter. Advisory: pdfcer's fills are
    /// operator-reviewed, so a keystroke filter is disclosed, not enforced.
    Keystroke {
        /// The helper's name as written, so disclosure can name it exactly.
        name: String,
    },
    /// `AFRange_Validate(bGreaterThan, nGreaterThan, bLessThan, nLessThan)`
    /// — a numeric range constraint, disclosed as a constraint.
    RangeValidate {
        /// The lower bound, if the call enabled one.
        lower: Option<f64>,
        /// The upper bound, if the call enabled one.
        upper: Option<f64>,
    },
}

/// What a `/JS` string is, as far as pdfcer is willing to commit.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptClass {
    /// A whitelisted calculation helper. May produce a new `/V`.
    Calculate(CalcHelper),
    /// A whitelisted formatting helper. Produces a display string only.
    Format(FormatHelper),
    /// Recognised, disclosed, never enforced by execution.
    Advisory(AdvisoryHelper),
    /// Everything else — including anything at all that is doubtful.
    ///
    /// This is not an error variant. It is the **safe default**, and the
    /// large majority of real-world scripts land here legitimately. A
    /// `Custom` field's stored `/V` is shown as-last-saved and disclosed as
    /// possibly stale (decision 009 §7); nothing is wrong, and nothing is
    /// silently computed.
    Custom,
}

impl ScriptClass {
    /// Whether pdfcer can natively produce this helper's effect.
    ///
    /// [`ScriptClass::Advisory`] is recognised but deliberately not
    /// reimplemented, so it is **not** reproducible — the distinction
    /// matters to a caller deciding whether to offer a recompute.
    #[must_use]
    pub const fn is_reproducible(&self) -> bool {
        matches!(self, Self::Calculate(_) | Self::Format(_))
    }

    /// A stable, locale-invariant token naming the classification, for CLI
    /// output and the recognition histogram.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::Calculate(CalcHelper::Simple { .. }) => "AFSimple_Calculate",
            Self::Format(FormatHelper::Number { .. }) => "AFNumber_Format",
            Self::Format(FormatHelper::Percent { .. }) => "AFPercent_Format",
            Self::Format(FormatHelper::Date { .. }) => "AFDate_Format",
            Self::Format(FormatHelper::DateEx { .. }) => "AFDate_FormatEx",
            Self::Format(FormatHelper::Time { .. }) => "AFTime_Format",
            Self::Format(FormatHelper::Special { .. }) => "AFSpecial_Format",
            Self::Advisory(AdvisoryHelper::Keystroke { .. }) => "AF*_Keystroke",
            Self::Advisory(AdvisoryHelper::RangeValidate { .. }) => "AFRange_Validate",
            Self::Custom => "custom",
        }
    }
}

/// Classify a `/JS` string found on a given `/AA` trigger.
///
/// Returns [`ScriptClass::Custom`] for anything not matched exactly,
/// including malformed input, unknown functions, and known functions called
/// with an argument list that does not fit.
///
/// # Why the trigger is a parameter
///
/// Because the trigger and the helper must **agree**, and a disagreement is
/// evidence the script is not what it looks like. Acrobat puts calculation
/// helpers on `/C` and formatting helpers on `/F`; a `AFNumber_Format` sitting
/// on `/C` was not generated by the Format tab, and whatever it is, pdfcer
/// should not act on it. Checking the pairing costs one comparison and closes
/// a whole class of mis-recognition — including the one that matters most,
/// a format helper reached through a code path that writes `/V`.
#[must_use]
pub fn classify(js: &[u8], trigger: Trigger) -> ScriptClass {
    let Some(call) = shape::parse_single_call(js) else {
        return ScriptClass::Custom;
    };
    match trigger {
        Trigger::Calculate => classify_calculate(&call),
        Trigger::Format => classify_format(&call),
        // A validate or keystroke script is never reproducible, so the only
        // question worth answering is whether it is a recognisable advisory
        // constraint worth disclosing.
        Trigger::Validate | Trigger::Keystroke => classify_advisory(&call, trigger),
    }
}

/// Match a calculate-trigger call against the calculation whitelist.
fn classify_calculate(call: &Call) -> ScriptClass {
    if call.name != "AFSimple_Calculate" {
        return ScriptClass::Custom;
    }
    // Exactly two arguments. A third would mean a signature pdfcer does not
    // know, and guessing that trailing arguments are ignorable is precisely
    // the kind of latitude that turns into a wrong total.
    if call.args.len() != 2 {
        return ScriptClass::Custom;
    }
    let Some(op) = call
        .arg(0)
        .and_then(Literal::as_str)
        .and_then(SimpleOp::from_code)
    else {
        return ScriptClass::Custom;
    };
    let Some(items) = call.arg(1).and_then(Literal::as_array) else {
        return ScriptClass::Custom;
    };
    // Every element must be a string. One non-string operand makes the whole
    // list untrustworthy — pdfcer would be summing a set it only partly
    // understood, and a total missing an operand is wrong, not approximate.
    let mut operands = Vec::with_capacity(items.len());
    for item in items {
        match item.as_str() {
            Some(name) => operands.push(name.to_vec()),
            None => return ScriptClass::Custom,
        }
    }
    ScriptClass::Calculate(CalcHelper::Simple { op, operands })
}

/// Match a format-trigger call against the formatting whitelist.
fn classify_format(call: &Call) -> ScriptClass {
    let helper = match call.name.as_str() {
        "AFNumber_Format" => number_format(call),
        "AFPercent_Format" => percent_format(call),
        "AFDate_Format" => single_int(call).map(|index| FormatHelper::Date { index }),
        "AFDate_FormatEx" => single_str(call).map(|format| FormatHelper::DateEx { format }),
        "AFTime_Format" => single_int(call).map(|index| FormatHelper::Time { index }),
        "AFSpecial_Format" => single_int(call).map(|selector| FormatHelper::Special { selector }),
        _ => None,
    };
    helper.map_or(ScriptClass::Custom, ScriptClass::Format)
}

/// `AFNumber_Format(nDec, sepStyle, negStyle, currStyle, strCurrency,
/// bCurrencyPrepend)`.
///
/// All six arguments are required. Real generated calls always pass six, and
/// treating a short list as "defaults apply" would require inventing the
/// defaults — an invented default silently changes how a number is displayed,
/// which is the failure this whole module is arranged to avoid.
fn number_format(call: &Call) -> Option<FormatHelper> {
    if call.args.len() != 6 {
        return None;
    }
    Some(FormatHelper::Number {
        decimals: call.arg(0)?.as_int()?,
        separator_style: call.arg(1)?.as_int()?,
        negative_style: call.arg(2)?.as_int()?,
        currency_style: call.arg(3)?.as_int()?,
        currency: call.arg(4)?.as_str()?.to_vec(),
        prepend_currency: call.arg(5)?.as_bool()?,
    })
}

/// `AFPercent_Format(nDec, sepStyle)`.
fn percent_format(call: &Call) -> Option<FormatHelper> {
    if call.args.len() != 2 {
        return None;
    }
    Some(FormatHelper::Percent {
        decimals: call.arg(0)?.as_int()?,
        separator_style: call.arg(1)?.as_int()?,
    })
}

/// A one-integer-argument helper.
fn single_int(call: &Call) -> Option<i64> {
    (call.args.len() == 1)
        .then(|| call.arg(0)?.as_int())
        .flatten()
}

/// A one-string-argument helper.
fn single_str(call: &Call) -> Option<Vec<u8>> {
    (call.args.len() == 1)
        .then(|| call.arg(0)?.as_str().map(<[u8]>::to_vec))
        .flatten()
}

/// Match a validate/keystroke call against the advisory whitelist.
fn classify_advisory(call: &Call, trigger: Trigger) -> ScriptClass {
    // `AFRange_Validate(bGreaterThan, nGreaterThan, bLessThan, nLessThan)`:
    // each bound is a boolean "is it enabled" paired with its value, so a
    // disabled bound's number is meaningless and must not be reported.
    if trigger == Trigger::Validate && call.name == "AFRange_Validate" && call.args.len() == 4 {
        let bounds = (|| {
            let has_lower = call.arg(0)?.as_bool()?;
            let lower = call.arg(1)?.as_num()?;
            let has_upper = call.arg(2)?.as_bool()?;
            let upper = call.arg(3)?.as_num()?;
            Some((has_lower.then_some(lower), has_upper.then_some(upper)))
        })();
        if let Some((lower, upper)) = bounds {
            return ScriptClass::Advisory(AdvisoryHelper::RangeValidate { lower, upper });
        }
    }
    // The keystroke family is matched by name shape rather than enumerated,
    // because its members share one purpose (filter input) and pdfcer's
    // response to all of them is identical: disclose, never enforce. Nothing
    // is computed from the match, so a loose match here cannot produce a
    // wrong value — only a slightly over-broad disclosure, which is the
    // harmless direction.
    if trigger == Trigger::Keystroke
        && call.name.starts_with("AF")
        && call.name.ends_with("_Keystroke")
    {
        return ScriptClass::Advisory(AdvisoryHelper::Keystroke {
            name: call.name.clone(),
        });
    }
    ScriptClass::Custom
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

    /// The canonical generated calculate call classifies, with its operands
    /// read out in source order.
    #[test]
    fn the_canonical_calculate_call_classifies_with_its_operands() {
        let js = br#"AFSimple_Calculate("SUM", new Array("Item.1","Item.2","Item.3"));"#;
        let ScriptClass::Calculate(CalcHelper::Simple { op, operands }) =
            classify(js, Trigger::Calculate)
        else {
            panic!("the canonical generated form must classify");
        };
        assert_eq!(op, SimpleOp::Sum);
        assert_eq!(operands.len(), 3);
        assert_eq!(operands[0], b"Item.1".to_vec());
        assert_eq!(operands[2], b"Item.3".to_vec());
    }

    /// All five operation codes match, and nothing else does.
    #[test]
    fn exactly_the_five_operation_codes_match() {
        for (code, want) in [
            ("SUM", SimpleOp::Sum),
            ("AVG", SimpleOp::Average),
            ("PRD", SimpleOp::Product),
            ("MIN", SimpleOp::Minimum),
            ("MAX", SimpleOp::Maximum),
        ] {
            let js = format!(r#"AFSimple_Calculate("{code}", ["A"]);"#);
            let ScriptClass::Calculate(CalcHelper::Simple { op, .. }) =
                classify(js.as_bytes(), Trigger::Calculate)
            else {
                panic!("{code} must classify");
            };
            assert_eq!(op, want);
        }
        for code in ["Sum", "sum", "TOTAL", "SUMX", ""] {
            let js = format!(r#"AFSimple_Calculate("{code}", ["A"]);"#);
            assert_eq!(
                classify(js.as_bytes(), Trigger::Calculate),
                ScriptClass::Custom,
                "{code:?} is not one of the five and must not be guessed at"
            );
        }
    }

    /// The canonical generated format call classifies with all six of its
    /// parameters captured.
    #[test]
    fn the_canonical_number_format_call_classifies_with_all_parameters() {
        let js = br#"AFNumber_Format(2, 0, 0, 1, "$", true);"#;
        let ScriptClass::Format(FormatHelper::Number {
            decimals,
            separator_style,
            negative_style,
            currency_style,
            currency,
            prepend_currency,
        }) = classify(js, Trigger::Format)
        else {
            panic!("the canonical generated form must classify");
        };
        assert_eq!(decimals, 2);
        assert_eq!(separator_style, 0);
        assert_eq!(negative_style, 0);
        assert_eq!(currency_style, 1);
        assert_eq!(currency, b"$".to_vec());
        assert!(prepend_currency);
    }

    /// Each single-argument format helper classifies to its own variant, so
    /// a date is never mistaken for a time.
    #[test]
    fn the_single_argument_format_helpers_stay_distinct() {
        assert_eq!(
            classify(b"AFDate_Format(1);", Trigger::Format),
            ScriptClass::Format(FormatHelper::Date { index: 1 })
        );
        assert_eq!(
            classify(b"AFTime_Format(1);", Trigger::Format),
            ScriptClass::Format(FormatHelper::Time { index: 1 })
        );
        assert_eq!(
            classify(b"AFSpecial_Format(0);", Trigger::Format),
            ScriptClass::Format(FormatHelper::Special { selector: 0 })
        );
        assert_eq!(
            classify(br#"AFDate_FormatEx("yyyy-mm-dd");"#, Trigger::Format),
            ScriptClass::Format(FormatHelper::DateEx {
                format: b"yyyy-mm-dd".to_vec()
            })
        );
    }

    /// ★ **A helper on the wrong trigger does not classify.**
    ///
    /// The pairing check exists for one case above all: a format helper
    /// must never be reachable through a path that writes `/V`. Acrobat
    /// generates calculation helpers on `/C` and format helpers on `/F`; a
    /// crossed pair was not generated by Acrobat, and pdfcer declines to
    /// guess what it was.
    #[test]
    fn a_helper_on_the_wrong_trigger_is_custom() {
        assert_eq!(
            classify(br#"AFNumber_Format(2,0,0,0,"",true);"#, Trigger::Calculate),
            ScriptClass::Custom,
            "a format helper on the calculate trigger must not be reachable \
             from a code path that writes /V"
        );
        assert_eq!(
            classify(br#"AFSimple_Calculate("SUM", ["A"]);"#, Trigger::Format),
            ScriptClass::Custom,
            "and a calculation helper on the format trigger is equally not \
             what it appears to be"
        );
    }

    /// A whitelisted name with an argument list that does not fit is
    /// `Custom` — pdfcer does not fill in a missing argument with a default
    /// it invented.
    #[test]
    fn a_known_name_with_an_unknown_signature_is_custom() {
        for js in [
            br#"AFNumber_Format(2, 0, 0, 0, "");"#.as_slice(),
            br#"AFNumber_Format(2, 0, 0, 0, "", true, 9);"#,
            br#"AFNumber_Format("2", 0, 0, 0, "", true);"#,
            br#"AFNumber_Format(2.5, 0, 0, 0, "", true);"#,
            br#"AFNumber_Format(2, 0, 0, 0, "", 1);"#,
        ] {
            assert_eq!(
                classify(js, Trigger::Format),
                ScriptClass::Custom,
                "{}",
                String::from_utf8_lossy(js)
            );
        }
        for js in [
            br#"AFSimple_Calculate("SUM");"#.as_slice(),
            br#"AFSimple_Calculate("SUM", ["A"], 1);"#,
            br#"AFSimple_Calculate("SUM", "A");"#,
            br#"AFSimple_Calculate("SUM", ["A", 2]);"#,
        ] {
            assert_eq!(
                classify(js, Trigger::Calculate),
                ScriptClass::Custom,
                "{}",
                String::from_utf8_lossy(js)
            );
        }
    }

    /// `AFRange_Validate` is disclosed as a constraint, and a **disabled**
    /// bound is reported as absent rather than as its meaningless number.
    #[test]
    fn range_validate_reports_only_the_bounds_that_are_enabled() {
        assert_eq!(
            classify(b"AFRange_Validate(true, 1, true, 100);", Trigger::Validate),
            ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                lower: Some(1.0),
                upper: Some(100.0)
            })
        );
        assert_eq!(
            classify(b"AFRange_Validate(true, 1, false, 0);", Trigger::Validate),
            ScriptClass::Advisory(AdvisoryHelper::RangeValidate {
                lower: Some(1.0),
                upper: None
            }),
            "a disabled bound's number is not a bound"
        );
    }

    /// The keystroke family is recognised by shape, and only on its own
    /// trigger.
    #[test]
    fn the_keystroke_family_is_disclosed_but_only_on_its_own_trigger() {
        let js = br#"AFNumber_Keystroke(2, 0, 0, 0, "", true);"#;
        assert_eq!(
            classify(js, Trigger::Keystroke),
            ScriptClass::Advisory(AdvisoryHelper::Keystroke {
                name: "AFNumber_Keystroke".to_owned()
            })
        );
        assert_eq!(
            classify(js, Trigger::Format),
            ScriptClass::Custom,
            "a keystroke filter on the format trigger is not a format helper"
        );
    }

    /// An advisory is recognised but **not** reproducible — the distinction
    /// a caller uses to decide whether to offer a recompute.
    #[test]
    fn advisories_are_recognised_but_not_reproducible() {
        let advisory = classify(b"AFRange_Validate(true, 1, true, 9);", Trigger::Validate);
        assert_ne!(advisory, ScriptClass::Custom, "it is recognised");
        assert!(!advisory.is_reproducible(), "and still not reproducible");

        let calc = classify(br#"AFSimple_Calculate("SUM", ["A"]);"#, Trigger::Calculate);
        assert!(calc.is_reproducible());
        assert!(!ScriptClass::Custom.is_reproducible());
    }

    /// Author code that merely mentions a helper name is `Custom`, which is
    /// the whole safety property restated at the classifier's own boundary.
    #[test]
    fn author_code_mentioning_a_helper_is_still_custom() {
        for js in [
            b"var t = 0; AFSimple_Calculate(\"SUM\", [\"A\"]);".as_slice(),
            b"if (this.getField(\"X\").value) AFSimple_Calculate(\"SUM\", [\"A\"]);",
            b"event.value = this.getField(\"A\").value * 2;",
            b"myAFSimple_Calculate(\"SUM\", [\"A\"]);",
        ] {
            assert_eq!(
                classify(js, Trigger::Calculate),
                ScriptClass::Custom,
                "{}",
                String::from_utf8_lossy(js)
            );
        }
    }
}
