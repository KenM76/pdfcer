//! What pdfcer **says** about a script-driven field — decision 009 §7.
//!
//! # Why disclosure is its own module rather than a format string at the
//! call site
//!
//! Because the promise is uniform and the call sites are not. Decision 009
//! §7 requires every disclosure to name three things:
//!
//! 1. **what** computes the value,
//! 2. **whether pdfcer ran it** — always: no, and
//! 3. **whether the shown value may be stale.**
//!
//! Scattering that across the CLI, the GUI inspector and the recompute
//! report would let one of them drift, and the one that drifts is the one
//! that stops saying "may be stale" — the omission nobody notices, because a
//! stale number and a fresh number look identical. Centralising it makes the
//! three-part promise checkable in one place, and the tests below check it
//! as a property over *every* variant rather than case by case.
//!
//! # The rule this implements
//!
//! Project rule 4, *fuzzy, never sneaky*: anything pdfcer inferred is visible
//! before it becomes document state. A recomputed total is an inference —
//! pdfcer read a script it did not run and reproduced what it believes the
//! script means — so it is disclosed with its provenance attached, and the
//! operator commits it deliberately.
//!
//! Note what rule 4 does *not* demand, per its 2026-08-05 narrowing: a
//! confirm button anchored to the document. The disclosure here is text; how
//! a shell presents it, and where the commit control sits, is the shell's
//! decision.
//!
//! # Locale
//!
//! These strings are English and not localised, consistent with the rest of
//! pdfcer's operator-facing text. The **machine-readable** counterpart is
//! [`super::ScriptClass::token`] plus the structured fields on
//! [`Disclosure`], which is what a script should parse — never this prose.

use super::{AdvisoryHelper, CalcHelper, FormatHelper, ScriptClass};

/// Whether pdfcer is able to reproduce a field's computation, and therefore
/// what it can honestly say about the value on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reproducibility {
    /// A whitelisted helper pdfcer can recompute natively, and the operator
    /// has not asked it to. The stored value stands and **may be stale**.
    RecomputableNotRun,
    /// A whitelisted helper pdfcer recomputed at the operator's request. The
    /// value on screen is pdfcer's, and the source script remains in the file
    /// as the downstream authority.
    Recomputed,
    /// A script pdfcer recognises but deliberately does not reimplement
    /// ([`AdvisoryHelper`]), or does not recognise at all. The stored value
    /// stands and may be stale; pdfcer cannot offer to refresh it.
    NotReproducible,
}

/// One field's script disclosure: the structured facts, and the prose.
///
/// Structured **and** prose deliberately. A shell that wants to build its
/// own sentence has the parts; a shell that just needs a line has
/// [`Disclosure::message`]. Offering only prose would force every consumer to
/// parse English; offering only parts would let each consumer invent its own
/// wording, and the wording is the part decision 009 actually constrains.
#[derive(Debug, Clone, PartialEq)]
pub struct Disclosure {
    /// The fully-qualified field name.
    pub field: String,
    /// A stable token naming the recognised helper, or `custom`.
    pub helper: &'static str,
    /// Whether pdfcer can reproduce it, and whether it has.
    pub reproducibility: Reproducibility,
    /// The value as it currently stands in the document, rendered for
    /// display. For [`Reproducibility::Recomputed`] this is pdfcer's new
    /// value; otherwise it is the stored `/V` as last saved.
    pub value: String,
    /// The value that was there **before** a recompute, present only for
    /// [`Reproducibility::Recomputed`].
    ///
    /// Carried so a shell can show the change rather than only the result.
    /// A recompute that reports "Total = 148.50" without saying it used to
    /// read 132.00 has disclosed the value but not the *edit*, and the edit
    /// is the thing the operator is being asked to accept.
    pub previous: Option<String>,
    /// A human-readable account of what computes the value.
    pub computation: String,
}

impl Disclosure {
    /// The full operator-facing sentence, satisfying all three parts of the
    /// decision 009 §7 contract.
    ///
    /// Every branch states that pdfcer did not run the script. That is
    /// repetitive on purpose: it is the single fact most likely to be
    /// assumed away by a reader who has used Acrobat, and the one whose
    /// absence would turn a careful disclosure into a false claim of
    /// authority.
    #[must_use]
    pub fn message(&self) -> String {
        match self.reproducibility {
            Reproducibility::Recomputed => {
                let was = self
                    .previous
                    .as_ref()
                    .map_or_else(String::new, |p| format!(" (was {p})"));
                format!(
                    "{}: {} = {}{was}. Computed natively by pdfcer from a recognised \
                     Acrobat built-in ({}); the script itself was NOT run. The source \
                     script is preserved, so a JavaScript-executing reader recomputes \
                     independently.",
                    self.field, self.computation, self.value, self.helper
                )
            }
            Reproducibility::RecomputableNotRun => format!(
                "{}: computed by a recognised Acrobat built-in ({}) that pdfcer does \
                 not execute — {}. Showing the stored value as last saved: {}. It may \
                 be stale if its inputs changed. pdfcer can recompute this natively on \
                 request.",
                self.field, self.helper, self.computation, self.value
            ),
            Reproducibility::NotReproducible => format!(
                "{}: computed by a document script pdfcer does not run — {}. Showing \
                 the stored value as last saved: {}. It may be stale if its inputs \
                 changed.",
                self.field, self.computation, self.value
            ),
        }
    }
}

/// Describe a classified script in prose, for [`Disclosure::computation`].
///
/// Names the operands explicitly for a calculation. A disclosure reading
/// "computed by AFSimple_Calculate" tells an operator that something
/// computes the field; "the sum of Item.1, Item.2, Item.3" tells them
/// **which inputs**, which is the only form of the statement they can check
/// against the document in front of them.
///
/// The operand list is capped at [`MAX_NAMED_OPERANDS`] with an explicit
/// "and N more" tail. Truncating silently would understate the computation;
/// printing ninety field names would bury the sentence.
#[must_use]
pub fn describe(class: &ScriptClass) -> String {
    match class {
        ScriptClass::Calculate(CalcHelper::Simple { op, operands }) => {
            format!("the {} of {}", op.describe(), name_list(operands))
        }
        ScriptClass::Format(f) => describe_format(f),
        ScriptClass::Advisory(AdvisoryHelper::RangeValidate { lower, upper }) => {
            match (lower, upper) {
                (Some(lo), Some(hi)) => {
                    format!("a validation constraining the value to between {lo} and {hi}")
                }
                (Some(lo), None) => format!("a validation requiring the value be at least {lo}"),
                (None, Some(hi)) => format!("a validation requiring the value be at most {hi}"),
                // Both bounds disabled: a call that constrains nothing. Said
                // plainly rather than dressed up, because "a validation" with
                // no bound would read as though pdfcer had lost the bounds.
                (None, None) => "a validation with no active bounds".to_owned(),
            }
        }
        ScriptClass::Advisory(AdvisoryHelper::Keystroke { name }) => {
            format!("an input filter ({name}) applied as you type in Acrobat")
        }
        ScriptClass::Custom => "a custom script".to_owned(),
    }
}

/// Prose for a formatting helper.
///
/// Every branch says **display**, never *value*. The distinction is the
/// load-bearing one in decision 009 §5.1 — a format helper changes what is
/// shown and never what is stored — and an operator who misreads a format
/// disclosure as a value disclosure would draw the wrong conclusion about
/// what a save is about to write.
fn describe_format(f: &FormatHelper) -> String {
    match f {
        FormatHelper::Number {
            decimals, currency, ..
        } => {
            let cur = if currency.is_empty() {
                String::new()
            } else {
                format!(" in {}", String::from_utf8_lossy(currency))
            };
            format!("a number display formatted to {decimals} decimal place(s){cur}")
        }
        FormatHelper::Percent { decimals, .. } => {
            format!("a percentage display formatted to {decimals} decimal place(s)")
        }
        FormatHelper::Date { index } => format!("a date display in predefined format {index}"),
        FormatHelper::DateEx { format } => {
            format!(
                "a date display formatted as {}",
                String::from_utf8_lossy(format)
            )
        }
        FormatHelper::Time { index } => format!("a time display in predefined format {index}"),
        FormatHelper::Special { selector } => {
            format!("a special display format (selector {selector})")
        }
    }
}

/// How many operand names a disclosure spells out before summarising.
pub const MAX_NAMED_OPERANDS: usize = 8;

/// Render an operand list, capped and with an explicit remainder.
fn name_list(operands: &[Vec<u8>]) -> String {
    if operands.is_empty() {
        // A recognised SUM over nothing. Worth saying out loud: it is a
        // real, if odd, script, and an operator seeing "the sum of no
        // fields" learns something a blank would hide.
        return "no fields".to_owned();
    }
    let shown: Vec<String> = operands
        .iter()
        .take(MAX_NAMED_OPERANDS)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    let mut s = shown.join(", ");
    if operands.len() > MAX_NAMED_OPERANDS {
        s.push_str(&format!(
            " and {} more",
            operands.len() - MAX_NAMED_OPERANDS
        ));
    }
    s
}

/// Which [`Reproducibility`] a classification implies when nothing has been
/// recomputed yet.
///
/// [`Reproducibility::Recomputed`] is never returned here — it is a fact
/// about an action that was taken, not a property of the script, and
/// deriving it from the classification would be exactly the conflation this
/// module exists to prevent.
#[must_use]
pub const fn reproducibility_of(class: &ScriptClass) -> Reproducibility {
    if class.is_reproducible() {
        Reproducibility::RecomputableNotRun
    } else {
        Reproducibility::NotReproducible
    }
}

/// Build the standing disclosure for a script-driven field that has not been
/// recomputed.
#[must_use]
pub fn for_field(field: &str, class: &ScriptClass, stored_value: &str) -> Disclosure {
    Disclosure {
        field: field.to_owned(),
        helper: class.token(),
        reproducibility: reproducibility_of(class),
        value: stored_value.to_owned(),
        previous: None,
        computation: describe(class),
    }
}

/// Build the disclosure for a field pdfcer has just recomputed.
#[must_use]
pub fn for_recompute(field: &str, class: &ScriptClass, previous: &str, now: &str) -> Disclosure {
    Disclosure {
        field: field.to_owned(),
        helper: class.token(),
        reproducibility: Reproducibility::Recomputed,
        value: now.to_owned(),
        previous: Some(previous.to_owned()),
        computation: describe(class),
    }
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
    use crate::form_script::{SimpleOp, Trigger, classify};

    fn sum_of(names: &[&str]) -> ScriptClass {
        let list = names
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(",");
        let js = format!("AFSimple_Calculate(\"SUM\", [{list}]);");
        classify(js.as_bytes(), Trigger::Calculate)
    }

    /// ★ **Every disclosure states that pdfcer did not run the script.**
    ///
    /// Asserted as a property over all three reproducibility states rather
    /// than per-case, because the failure this guards against is one branch
    /// quietly losing the clause — and the branch that loses it is the one
    /// that then reads as an authoritative pdfcer computation.
    #[test]
    fn every_disclosure_says_pdfcer_did_not_run_the_script() {
        let calc = sum_of(&["A", "B"]);
        let custom = classify(b"event.value = 1 + 1;", Trigger::Calculate);
        let cases = [
            for_field("Total", &calc, "132.00"),
            for_field("Total", &custom, "132.00"),
            for_recompute("Total", &calc, "132.00", "148.50"),
        ];
        for d in cases {
            let m = d.message();
            let disclaims = m.contains("does not run")
                || m.contains("does not execute")
                || m.contains("NOT run");
            assert!(disclaims, "no non-execution clause in: {m}");
        }
    }

    /// A value pdfcer did not compute is always marked as possibly stale.
    #[test]
    fn an_uncomputed_value_is_always_marked_possibly_stale() {
        for class in [
            sum_of(&["A"]),
            classify(b"event.value = 1;", Trigger::Calculate),
            classify(b"AFRange_Validate(true,1,true,9);", Trigger::Validate),
        ] {
            let m = for_field("F", &class, "7").message();
            assert!(m.contains("may be stale"), "no staleness clause in: {m}");
            assert!(
                m.contains("as last saved"),
                "and the value must be attributed to the file, not to pdfcer: {m}"
            );
        }
    }

    /// A recomputed value is NOT marked stale — it is pdfcer's own — and it
    /// says the source script is preserved, which is the fail-safe property
    /// decision 009 §5.1 requires be visible.
    #[test]
    fn a_recomputed_value_is_not_stale_and_says_the_script_is_preserved() {
        let m = for_recompute("Total", &sum_of(&["A", "B"]), "132.00", "148.50").message();
        assert!(!m.contains("may be stale"), "pdfcer computed it: {m}");
        assert!(m.contains("preserved"), "the script stays in the file: {m}");
        assert!(
            m.contains("was 132.00"),
            "the edit is shown, not just the result: {m}"
        );
        assert!(m.contains("148.50"));
    }

    /// The computation names its operands, so an operator can check the
    /// claim against the document rather than take it on trust.
    #[test]
    fn a_calculation_names_its_operands() {
        let d = for_field("Total", &sum_of(&["Item.1", "Item.2"]), "0");
        assert_eq!(d.computation, "the sum of Item.1, Item.2");
        assert!(d.message().contains("Item.2"));
    }

    /// A long operand list is capped with an explicit remainder rather than
    /// silently truncated.
    #[test]
    fn a_long_operand_list_is_capped_with_an_explicit_remainder() {
        let names: Vec<String> = (0..12).map(|i| format!("F{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let d = for_field("Total", &sum_of(&refs), "0");
        assert!(d.computation.contains("F0"));
        assert!(d.computation.contains("and 4 more"), "{}", d.computation);
        assert!(
            !d.computation.contains("F11"),
            "the tail is summarised, not listed: {}",
            d.computation
        );
    }

    /// ★ **A format disclosure says "display", never "value".**
    ///
    /// The whole format/value separation is invisible to an operator unless
    /// the words carry it.
    #[test]
    fn a_format_disclosure_talks_about_display_not_value() {
        let class = classify(
            br#"AFNumber_Format(2, 0, 0, 1, "$", true);"#,
            Trigger::Format,
        );
        let d = for_field("Price", &class, "1234");
        assert!(d.computation.contains("display"), "{}", d.computation);
        assert!(d.computation.contains('$'), "the currency is named");
        assert_eq!(
            d.value, "1234",
            "and the disclosed value is the RAW stored one"
        );
    }

    /// An advisory is described as the constraint it is, and is reported as
    /// not reproducible — pdfcer recognises it without offering to enforce it.
    #[test]
    fn an_advisory_is_described_as_a_constraint_and_is_not_reproducible() {
        let class = classify(b"AFRange_Validate(true, 1, true, 100);", Trigger::Validate);
        let d = for_field("Qty", &class, "5");
        assert_eq!(d.reproducibility, Reproducibility::NotReproducible);
        assert!(
            d.computation.contains("between 1 and 100"),
            "{}",
            d.computation
        );
    }

    /// A recognised-but-unrun helper advertises that pdfcer could refresh it;
    /// an unrecognised one does not, because offering a capability that does
    /// not exist is R83's exact failure.
    #[test]
    fn only_a_reproducible_script_advertises_the_recompute() {
        let can = for_field("T", &sum_of(&["A"]), "1").message();
        assert!(can.contains("can recompute"), "{can}");

        let cannot =
            for_field("T", &classify(b"event.value=1;", Trigger::Calculate), "1").message();
        assert!(
            !cannot.contains("can recompute"),
            "pdfcer must not offer what it cannot do: {cannot}"
        );
    }

    /// A recognised SUM over an empty list says so rather than rendering a
    /// blank where the operand list belongs.
    #[test]
    fn a_calculation_over_no_fields_says_so() {
        let class = classify(br#"AFSimple_Calculate("SUM", []);"#, Trigger::Calculate);
        assert_eq!(describe(&class), "the sum of no fields");
    }

    /// The token is stable and machine-readable, and is what a script should
    /// key on rather than the prose.
    #[test]
    fn the_helper_token_is_stable_and_machine_readable() {
        assert_eq!(
            for_field("T", &sum_of(&["A"]), "0").helper,
            "AFSimple_Calculate"
        );
        assert_eq!(
            for_field("T", &classify(b"x=1;", Trigger::Calculate), "0").helper,
            "custom"
        );
    }

    /// `SimpleOp::describe` covers all five, so no operation renders as a
    /// blank in a disclosure.
    #[test]
    fn every_operation_has_prose() {
        for op in [
            SimpleOp::Sum,
            SimpleOp::Average,
            SimpleOp::Product,
            SimpleOp::Minimum,
            SimpleOp::Maximum,
        ] {
            assert!(!op.describe().is_empty(), "{} has no prose", op.code());
        }
    }
}
