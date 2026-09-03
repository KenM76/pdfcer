//! Natively evaluate a recognised `AFSimple_Calculate` — decision 009
//! posture B's arithmetic, with no JavaScript engine anywhere near it.
//!
//! # Where these semantics come from, and how far to trust them
//!
//! ISO 32000-1 specifies **nothing** here. §12.6.4.16 defines the carrier
//! for a JavaScript action and no semantics at all — it delegates "the
//! contents and effects of JavaScript scripts" to two external documents,
//! and even they define a language, not `AFSimple_Calculate`. So there is no
//! standard to be conformant with: the only definition of what
//! `AFSimple_Calculate("AVG", …)` means is what Acrobat does.
//!
//! (This module previously called that a "hollow shall" and inferred that
//! non-execution was therefore free of any conformance cost. The inference
//! was wrong — see [`crate::forms::FormJavaScript`] for the corrected
//! argument. It does not affect anything below: the absence of *arithmetic
//! semantics* in the standard is separately true and is all this module
//! relies on.)
//!
//! Acrobat's behaviour was sourced through `pdfcer-acrobat-librarian` into
//! `Acrobat_Features/forms__calculation_validation_javascript.md`. That
//! source tags every fact by evidence tier, and the tiering matters enough
//! to repeat here, because the rules below are **not** all equally certain:
//!
//! - The five operation codes and their arithmetic: corroborated across
//!   independent sources.
//! - The empty/non-numeric and missing-field rules: sourced from Mozilla
//!   `pdf.js`'s independent reimplementation — a real, shipped, MPL-2.0
//!   interoperability effort, and **not** Adobe-primary. Behaviour only was
//!   taken; no code was copied, and pdfcer links nothing (`LEGAL.md` §6.1).
//! - Whether hidden or read-only fields participate: **unsourced.** No
//!   source checks a flag before including a field, so pdfcer does not
//!   either — recorded as an inference, not a finding.
//!
//! Where a rule is single-tier, it is marked in the item's own doc comment.
//! Saying "Acrobat does X" when what is known is "one careful
//! reimplementation does X" is the kind of quiet overstatement that later
//! reads as a verified fact.
//!
//! # The three operand rules, which are three rules and not one
//!
//! This is the part a naive reimplementation gets wrong, because the
//! obvious mental model — "skip anything that isn't a number" — is wrong in
//! a way that changes results:
//!
//! 1. **A name that resolves to no field**: pdfcer **refuses the whole
//!    calculation** (see [`Refusal::UnresolvedOperand`]). The two sources
//!    disagree about what Acrobat does — `pdf.js` skips silently, real-world
//!    reports describe Acrobat throwing — so pdfcer declines to pick, which
//!    is the only option that cannot be wrong in the direction that matters.
//! 2. **A field that resolves but holds blank or non-numeric text**: counts
//!    as **zero**, and participates. So a `PRD` with one blank operand is
//!    zero, not "the product of the filled ones", and an `AVG` divides by
//!    the count of *resolved* operands including the blanks.
//! 3. **A name matching several field representations**: each contributes
//!    its own entry, matching `getArray()`'s per-kid expansion.
//!
//! Rule 2 is the counter-intuitive one and the tests below pin all three of
//! its surprising consequences, because each is individually easy to
//! "simplify" back into a bug.
//!
//! # What this module never does
//!
//! It does not write anything. It computes a number and reports how it got
//! there; deciding whether that number becomes `/V` is the caller's, and the
//! operator's. That separation is decision 009 §5.1 — a recompute is an
//! operator-invoked, undoable edit, never a side effect of reading a file.

use std::collections::BTreeMap;

use crate::forms::{AcroForm, FieldValue};

use super::SimpleOp;

/// How to read a value containing a comma.
///
/// # Why this is a policy rather than a hard-coded rule
///
/// `1,234` is `1234` to an English-locale form and `1.234` to a
/// German-locale one, and a stored field value carries nothing that says
/// which. `pdf.js` resolves it by replacing the first comma with a decimal
/// point, which silently turns an en-locale `1,234` into `1.234` — a
/// thousand-fold error that produces a plausible-looking total. The RAG
/// flags that behaviour explicitly as one **not** to copy.
///
/// pdfcer will not guess. The default treats a comma-bearing value as
/// non-numeric — which, by rule 2 above, makes it a disclosed zero rather
/// than a silent misreading — and an operator who knows their document's
/// convention can say so.
///
/// This is the project's standing response to an ambiguity nobody can
/// resolve from the data: make it a setting with a safe default, rather than
/// hard-code one reading and hope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommaPolicy {
    /// A comma makes the value non-numeric. **The default.** The operand
    /// contributes zero and the ambiguity is counted, so a shell can say
    /// "3 operands were not numeric" rather than quietly producing a total
    /// that is off by a factor of a thousand.
    #[default]
    NotNumeric,
    /// A comma is a decimal separator: `1,5` is `1.5`. Grouping periods are
    /// stripped, so `1.234,56` reads as `1234.56`.
    DecimalSeparator,
    /// A comma is a grouping separator: `1,234.56` is `1234.56`. Commas are
    /// stripped and a period remains the decimal separator.
    GroupingSeparator,
}

/// One operand as pdfcer read it.
///
/// Kept per-operand, rather than collapsed into a single total, because the
/// disclosure has to be able to say *which* operand was blank. A recompute
/// that reports "148.50" without being able to explain that one of its
/// inputs was empty gives an operator no way to notice a half-filled form.
#[derive(Debug, Clone, PartialEq)]
pub struct Operand {
    /// The field name the script named.
    pub name: String,
    /// The raw stored value, rendered for display.
    pub raw: String,
    /// The number it contributed.
    pub value: f64,
    /// Whether [`Operand::value`] came from a real parse rather than from
    /// rule 2's blank-is-zero coercion.
    ///
    /// The flag a disclosure needs: a zero that was typed and a zero that
    /// stood in for an empty box are the same number and very different
    /// facts.
    pub numeric: bool,
}

/// Why pdfcer declined to compute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// An operand named a field that does not exist in this document.
    ///
    /// The sources conflict on Acrobat's behaviour here — one skips it,
    /// real-world reports describe an error — so pdfcer refuses rather than
    /// choosing. A refusal costs a recompute the operator can still do by
    /// hand; a wrong choice writes a total that silently omits an operand
    /// (if skipping is wrong) or invents one (if erroring is wrong).
    UnresolvedOperand(String),
    /// The script named no operands at all.
    ///
    /// `SUM` over nothing is arguably zero, but `MIN` over nothing is not a
    /// number in any reading, and treating the two differently would make
    /// the refusal depend on the operation rather than on the document. A
    /// calculation with no inputs is a broken script, and writing `0` into
    /// the field would present pdfcer's guess as the document's intent.
    NoOperands,
    /// The arithmetic produced a non-finite result — an overflow to
    /// infinity, or a NaN.
    ///
    /// Cannot arise from the five operations over finite inputs except by
    /// overflow, and is checked anyway: writing `inf` into a `/V` would
    /// produce a field no reader can parse back.
    NotFinite,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedOperand(name) => write!(
                f,
                "the script totals a field named {name:?} that this document \
                 does not contain, and Acrobat's behaviour in that case is not \
                 settled, so pdfcer will not guess at the total"
            ),
            Self::NoOperands => f.write_str("the script names no fields to compute from"),
            Self::NotFinite => {
                f.write_str("the computation overflowed to a value no reader could store")
            }
        }
    }
}

/// A completed native evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Computation {
    /// The operation performed.
    pub op: SimpleOp,
    /// Every operand, in the order the script named them (a name matching
    /// several field representations expands in place).
    pub operands: Vec<Operand>,
    /// The result.
    pub value: f64,
}

impl Computation {
    /// How many operands were blank or non-numeric and counted as zero.
    ///
    /// The number a disclosure quotes. Non-zero means the total is
    /// arithmetically correct and possibly not what the operator expects,
    /// which is precisely the case worth surfacing.
    #[must_use]
    pub fn coerced_operands(&self) -> usize {
        self.operands.iter().filter(|o| !o.numeric).count()
    }
}

/// Read a stored field value as a number, per rule 2 and `policy`.
///
/// Returns `None` for anything not read as a number; rule 2 then makes that
/// a zero. Kept separate from the coercion so the *fact* of the coercion
/// survives into [`Operand::numeric`] — collapsing the two would compute the
/// same total and lose the only signal that the form is half-filled.
///
/// # What is accepted
///
/// A trimmed, optionally-signed decimal: `12`, `-3.5`, `+0.25`, `.5`.
/// Surrounding whitespace is ignored. Currency symbols, percent signs and
/// grouping characters are **not** stripped — a stored `/V` holds the raw
/// value, and a `/V` that looks formatted means something already went wrong
/// upstream, which is a thing to disclose rather than to paper over.
///
/// Exponent notation (`1e3`) is rejected: no form field holds one, and
/// accepting it would let a malformed value parse as a number wildly
/// different from what it looks like.
#[must_use]
pub fn parse_number(text: &str, policy: CommaPolicy) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let normalised = match policy {
        CommaPolicy::NotNumeric => {
            if t.contains(',') {
                return None;
            }
            t.to_owned()
        }
        CommaPolicy::DecimalSeparator => {
            // Periods group, the comma decides the fraction. Two commas
            // would be two decimal points, which is not a number.
            if t.matches(',').count() > 1 {
                return None;
            }
            t.replace('.', "").replace(',', ".")
        }
        CommaPolicy::GroupingSeparator => t.replace(',', ""),
    };
    // Reject anything but sign, digits and a single period BEFORE handing to
    // `f64::parse`, which would otherwise accept `inf`, `NaN`, `1e5` and a
    // trailing-whitespace-free `1_0` on some inputs.
    let mut seen_dot = false;
    let mut seen_digit = false;
    for (i, c) in normalised.char_indices() {
        match c {
            '+' | '-' if i == 0 => {}
            '.' if !seen_dot => seen_dot = true,
            c if c.is_ascii_digit() => seen_digit = true,
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }
    normalised.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Apply one of the five operations to already-resolved operand values.
///
/// Separate from operand resolution so the arithmetic is testable against
/// plain numbers, with no document in the way. Every surprising consequence
/// of rule 2 is a property of *this* function over a list that already
/// contains the coerced zeros.
///
/// Returns `None` for an empty list — see [`Refusal::NoOperands`].
#[must_use]
pub fn apply(op: SimpleOp, values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let result = match op {
        SimpleOp::Sum => values.iter().sum(),
        // Divides by the count of RESOLVED operands, blanks included — rule
        // 2. `values.len()` is that count by construction, because a blank
        // operand is present here as a zero rather than absent.
        #[allow(clippy::cast_precision_loss)]
        SimpleOp::Average => values.iter().sum::<f64>() / values.len() as f64,
        SimpleOp::Product => values.iter().product(),
        // `f64::min`/`max` rather than a `partial_cmp` fold: the inputs are
        // finite by construction (`parse_number` filters), so the NaN
        // asymmetry the two disagree about cannot arise.
        SimpleOp::Minimum => values.iter().copied().fold(f64::INFINITY, f64::min),
        SimpleOp::Maximum => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    };
    result.is_finite().then_some(result)
}

/// Resolve a recognised calculation's operands against a form and compute it.
///
/// # Resolution
///
/// A name matches **every terminal field whose fully-qualified name equals
/// it**, and each match contributes its own operand. That is pdfcer's model
/// of the same expansion `getArray()` performs: a field with several
/// same-named representations contributes one value per representation.
///
/// Matching is on the fully-qualified name exactly, with no partial-name
/// fallback. A script naming `Item.1` means the field whose FQN is `Item.1`,
/// and resolving a bare `1` against it would let a script total a field it
/// never named.
///
/// # Errors
///
/// Returns [`Refusal`] rather than a wrong number. Every refusal is
/// disclosable and none of them writes anything.
pub fn compute(
    form: &AcroForm,
    op: SimpleOp,
    operand_names: &[Vec<u8>],
    policy: CommaPolicy,
) -> Result<Computation, Refusal> {
    compute_with_overrides(form, op, operand_names, policy, &BTreeMap::new())
}

/// As [`compute`], but reading `overrides` in preference to the stored value.
///
/// # Why an overlay rather than mutating the form first
///
/// A calculated field can be another calculation's operand — a line total
/// feeding a subtotal feeding a grand total — so a whole-form recompute must
/// evaluate in dependency order and let each result be visible to the fields
/// downstream of it. The alternative, writing each result into the document
/// as it is computed, would make the *plan* mutate the document before the
/// operator has seen it, which is exactly what decision 009 §5.1 forbids.
///
/// The overlay keeps the whole cascade hypothetical until the operator
/// accepts it. Keys are fully-qualified field names; a name absent from the
/// map reads its stored value as usual.
pub fn compute_with_overrides(
    form: &AcroForm,
    op: SimpleOp,
    operand_names: &[Vec<u8>],
    policy: CommaPolicy,
    overrides: &BTreeMap<String, String>,
) -> Result<Computation, Refusal> {
    if operand_names.is_empty() {
        return Err(Refusal::NoOperands);
    }
    let mut operands = Vec::new();
    for raw_name in operand_names {
        let name = String::from_utf8_lossy(raw_name).into_owned();
        let matches: Vec<&crate::forms::Field> = form
            .fields
            .iter()
            .filter(|f| f.fully_qualified_name == name)
            .collect();
        if matches.is_empty() {
            return Err(Refusal::UnresolvedOperand(name));
        }
        for field in matches {
            let raw = overrides
                .get(&name)
                .cloned()
                .unwrap_or_else(|| display_of(&field.value));
            let parsed = parse_number(&raw, policy);
            operands.push(Operand {
                name: name.clone(),
                raw,
                // Rule 2: resolved but unreadable is a participating zero.
                value: parsed.unwrap_or(0.0),
                numeric: parsed.is_some(),
            });
        }
    }
    let values: Vec<f64> = operands.iter().map(|o| o.value).collect();
    let value = apply(op, &values).ok_or(Refusal::NotFinite)?;
    Ok(Computation {
        op,
        operands,
        value,
    })
}

/// A field value as the text a calculation reads.
///
/// Reads the **stored** value, never a formatted display string — the
/// distinction decision 009 §5.1 turns on, and one the sources confirm from
/// two directions (the clone reads `.value`, and calculate fires ahead of
/// format in the trigger pipeline, so no formatted string exists yet).
///
/// A signature field yields an empty string: it holds a dictionary, not a
/// value, and rule 2 makes that a disclosed zero rather than a parse
/// failure of something that was never text.
fn display_of(value: &FieldValue) -> String {
    match value {
        FieldValue::Signature => String::new(),
        other => other.display_text(),
    }
}

/// Render a computed value for storage in `/V`.
///
/// # Why this is not `{value}` and not `{value:.2}`
///
/// A stored form value is raw text, and the two obvious renderings are both
/// wrong. Rust's default prints `1e21` for large magnitudes, which no reader
/// parses back as a number. A fixed two decimals invents a precision the
/// calculation did not have and turns an exact `3` into `3.00`, changing the
/// stored value of every integer total in the document.
///
/// So: plain positional notation, with trailing zeros and a trailing point
/// removed. `3.0` stores as `3`, `1.5` as `1.5`, `0.1 + 0.2` as
/// `0.30000000000000004` — the last one deliberately, because rounding it
/// here would make the stored value disagree with the arithmetic that
/// produced it, and the display formatter ([`super::format`]) is the layer
/// that decides how many decimals an operator sees.
#[must_use]
pub fn render_value(value: f64) -> String {
    // `{:?}` on f64 gives the shortest representation that round-trips.
    let s = format!("{value:?}");
    if !s.contains('e') {
        return s.strip_suffix(".0").map_or(s.clone(), str::to_owned);
    }
    // Far outside any plausible form-field range, `{:?}` switches to
    // exponent notation — which §7.3.3 does not permit in a PDF numeric
    // object and which no reader parses back out of a text value either.
    // Expand it positionally and trim. A total this large is pathological
    // regardless; the point is that it stays *readable* rather than becoming
    // a value that silently fails to load.
    let expanded = format!("{value:.10}");
    let trimmed = expanded.trim_end_matches('0').trim_end_matches('.');
    // `-0` and `0.0000000000` both trim to something empty-ish; normalise.
    if trimmed.is_empty() || trimmed == "-" {
        return "0".to_owned();
    }
    trimmed.to_owned()
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
    use crate::document::Document;
    use crate::forms::parse_acroform;
    use crate::pageops::tests_support::build_pdf_bytes;

    /// A form whose fields are `(name, value)` pairs, all text fields.
    fn form_with(fields: &[(&str, &str)]) -> AcroForm {
        let mut objects: Vec<(u32, String)> = Vec::new();
        let refs = (0..fields.len())
            .map(|i| format!("{} 0 R", i + 4))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push((
            1,
            format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [{refs}] >> >>"),
        ));
        objects.push((2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()));
        objects.push((
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        ));
        for (i, (name, value)) in fields.iter().enumerate() {
            objects.push((
                u32::try_from(i + 4).expect("small fixture"),
                format!("<< /FT /Tx /T ({name}) /V ({value}) >>"),
            ));
        }
        let borrowed: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
        let doc = Document::from_bytes(build_pdf_bytes(&borrowed)).expect("fixture parses");
        parse_acroform(&doc).expect("fixture has a form")
    }

    fn names(list: &[&str]) -> Vec<Vec<u8>> {
        list.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    /// The five operations over ordinary filled fields.
    #[test]
    fn the_five_operations_compute_what_they_say() {
        let form = form_with(&[("A", "2"), ("B", "4"), ("C", "6")]);
        let all = names(&["A", "B", "C"]);
        let p = CommaPolicy::default();
        for (op, want) in [
            (SimpleOp::Sum, 12.0),
            (SimpleOp::Average, 4.0),
            (SimpleOp::Product, 48.0),
            (SimpleOp::Minimum, 2.0),
            (SimpleOp::Maximum, 6.0),
        ] {
            let got = compute(&form, op, &all, p).expect("computes").value;
            assert!(
                (got - want).abs() < 1e-9,
                "{} gave {got}, expected {want}",
                op.code()
            );
        }
    }

    /// ★ **A blank operand is a participating ZERO, not an ignored one.**
    ///
    /// Rule 2, and the single most important behaviour in this module,
    /// because the intuitive reimplementation ("skip what isn't a number")
    /// disagrees with all three of these and produces plausible wrong
    /// answers rather than obvious ones.
    #[test]
    fn a_blank_operand_participates_as_zero() {
        let form = form_with(&[("A", "10"), ("B", ""), ("C", "5")]);
        let all = names(&["A", "B", "C"]);
        let p = CommaPolicy::default();

        let sum = compute(&form, SimpleOp::Sum, &all, p).expect("computes");
        assert!((sum.value - 15.0).abs() < 1e-9, "the blank adds nothing");

        let avg = compute(&form, SimpleOp::Average, &all, p).expect("computes");
        assert!(
            (avg.value - 5.0).abs() < 1e-9,
            "but it DOES divide: 15/3, not 15/2 — got {}",
            avg.value
        );

        let prd = compute(&form, SimpleOp::Product, &all, p).expect("computes");
        assert!(
            (prd.value - 0.0).abs() < 1e-9,
            "and it zeroes a product entirely — got {}",
            prd.value
        );

        let min = compute(&form, SimpleOp::Minimum, &all, p).expect("computes");
        assert!(
            (min.value - 0.0).abs() < 1e-9,
            "and pulls a minimum down to 0 — got {}",
            min.value
        );

        assert_eq!(sum.coerced_operands(), 1, "and the coercion is countable");
        assert!(!sum.operands[1].numeric, "by operand, not just in total");
        assert!(sum.operands[0].numeric);
    }

    /// Non-numeric text is the same case as blank: a participating zero,
    /// counted.
    #[test]
    fn non_numeric_text_is_also_a_counted_zero() {
        let form = form_with(&[("A", "10"), ("B", "N/A")]);
        let c = compute(
            &form,
            SimpleOp::Sum,
            &names(&["A", "B"]),
            CommaPolicy::default(),
        )
        .expect("computes");
        assert!((c.value - 10.0).abs() < 1e-9);
        assert_eq!(c.coerced_operands(), 1);
        assert_eq!(
            c.operands[1].raw, "N/A",
            "the raw text is kept for disclosure"
        );
    }

    /// ★ **An operand naming a field that does not exist REFUSES.**
    ///
    /// The sources disagree about Acrobat here, so pdfcer declines to pick.
    /// A refusal is visible; either wrong choice would produce a total that
    /// silently omits or invents an operand.
    #[test]
    fn an_unresolved_operand_refuses_rather_than_guessing() {
        let form = form_with(&[("A", "10")]);
        let err = compute(
            &form,
            SimpleOp::Sum,
            &names(&["A", "Missing"]),
            CommaPolicy::default(),
        )
        .expect_err("must refuse");
        assert_eq!(err, Refusal::UnresolvedOperand("Missing".to_owned()));
        assert!(
            err.to_string().contains("Missing"),
            "and names the field, so the operator can fix the script or the form"
        );
    }

    /// A calculation naming no fields refuses rather than storing zero.
    #[test]
    fn a_calculation_with_no_operands_refuses() {
        let form = form_with(&[("A", "1")]);
        assert_eq!(
            compute(&form, SimpleOp::Sum, &[], CommaPolicy::default()),
            Err(Refusal::NoOperands)
        );
        assert_eq!(apply(SimpleOp::Minimum, &[]), None, "and MIN is not -inf");
        assert_eq!(apply(SimpleOp::Maximum, &[]), None);
    }

    /// ★ **A comma is ambiguous, so by default it is not a number.**
    ///
    /// The alternative — `pdf.js`'s first-comma-to-decimal rewrite — turns
    /// an English-locale `1,234` into `1.234`, a thousand-fold error that
    /// looks entirely plausible in a total.
    #[test]
    fn a_comma_is_ambiguous_and_defaults_to_not_a_number() {
        assert_eq!(parse_number("1,234", CommaPolicy::NotNumeric), None);
        assert_eq!(
            parse_number("1,234", CommaPolicy::GroupingSeparator),
            Some(1234.0)
        );
        assert_eq!(
            parse_number("1,5", CommaPolicy::DecimalSeparator),
            Some(1.5)
        );
        assert_eq!(
            parse_number("1.234,56", CommaPolicy::DecimalSeparator),
            Some(1234.56),
            "periods group when the comma is the decimal separator"
        );
        assert_eq!(
            parse_number("1,2,3", CommaPolicy::DecimalSeparator),
            None,
            "two decimal points is not a number"
        );

        // And the policy reaches a real computation.
        let form = form_with(&[("A", "1,234")]);
        let default = compute(
            &form,
            SimpleOp::Sum,
            &names(&["A"]),
            CommaPolicy::NotNumeric,
        )
        .expect("computes");
        assert!((default.value - 0.0).abs() < 1e-9, "a disclosed zero");
        assert_eq!(default.coerced_operands(), 1, "and it is disclosed");

        let grouped = compute(
            &form,
            SimpleOp::Sum,
            &names(&["A"]),
            CommaPolicy::GroupingSeparator,
        )
        .expect("computes");
        assert!((grouped.value - 1234.0).abs() < 1e-9);
        assert_eq!(grouped.coerced_operands(), 0);
    }

    /// Values that look numeric to `f64::parse` but are not form values are
    /// refused, so a stored `inf` cannot become a total.
    #[test]
    fn only_plain_decimal_values_parse() {
        let p = CommaPolicy::NotNumeric;
        for text in [
            "inf", "NaN", "1e5", "0x10", "1 2", "$5", "5%", "", "   ", "-", ".",
        ] {
            assert_eq!(parse_number(text, p), None, "{text:?} must not parse");
        }
        for (text, want) in [("12", 12.0), (" -3.5 ", -3.5), ("+0.25", 0.25), (".5", 0.5)] {
            assert_eq!(parse_number(text, p), Some(want), "{text:?}");
        }
    }

    /// A name matching several field representations expands to one operand
    /// each, in place.
    #[test]
    fn a_name_matching_several_representations_expands_in_place() {
        // Two same-named terminal fields — the shape a repeated widget or a
        // multi-representation field takes in pdfcer's flattened model.
        let form = form_with(&[("Dup", "3"), ("Other", "10"), ("Dup", "4")]);
        let c = compute(
            &form,
            SimpleOp::Sum,
            &names(&["Dup", "Other"]),
            CommaPolicy::default(),
        )
        .expect("computes");
        assert_eq!(c.operands.len(), 3, "Dup contributed twice");
        assert!((c.value - 17.0).abs() < 1e-9);
        assert_eq!(
            c.operands
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Dup", "Dup", "Other"],
            "expanded in place, not appended"
        );
    }

    /// The stored rendering is plain positional notation with no invented
    /// precision.
    #[test]
    fn a_value_is_stored_without_invented_precision_or_exponents() {
        assert_eq!(render_value(3.0), "3", "not 3.00 and not 3.0");
        assert_eq!(render_value(1.5), "1.5");
        assert_eq!(render_value(-0.25), "-0.25");
        assert_eq!(render_value(0.0), "0");
        assert!(
            !render_value(1e21).contains('e'),
            "a stored value no reader could parse is not acceptable: {}",
            render_value(1e21)
        );
    }

    /// Full-name matching only: a script naming one field never totals
    /// another whose name merely ends the same way.
    #[test]
    fn operand_matching_is_on_the_full_name_only() {
        let form = form_with(&[("Group.Item", "5")]);
        assert_eq!(
            compute(
                &form,
                SimpleOp::Sum,
                &names(&["Item"]),
                CommaPolicy::default()
            ),
            Err(Refusal::UnresolvedOperand("Item".to_owned())),
            "a partial name resolves to nothing rather than to the wrong field"
        );
        assert!(
            compute(
                &form,
                SimpleOp::Sum,
                &names(&["Group.Item"]),
                CommaPolicy::default()
            )
            .is_ok()
        );
    }
}
