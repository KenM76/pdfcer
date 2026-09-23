//! **The `/AA` scripts pdfcer classifies can now be written** (`Pass 308.6`,
//! request `G024`).
//!
//! ## The asymmetry this closes
//!
//! `form_script` had a **parser** for a closed set of Acrobat helper calls and
//! no **emitter** for the same set. `/AA` was a one-way door: a paste could
//! carry one, a delete could strip one, and nothing could author one — so the
//! Format, Validate and Calculate capabilities were readable, disclosable, and
//! unreachable.
//!
//! `pdfcer-gui` filed it as a boundary finding rather than a feature request:
//! *a crate that can classify a closed set of values and cannot construct one
//! is a boundary drawn on the read side only.*
//!
//! ## What keeps a script WRITER safe
//!
//! There is no `&str`-taking route into `/AA`. The input is a typed helper
//! from the same whitelist `classify` recognises, so **arbitrary JavaScript
//! stays unrepresentable** — an operator cannot ask pdfcer for a script pdfcer
//! cannot also read back and describe. That is the property, not a limitation
//! to work around later.
//!
//! ## Three things are written that nobody asked for by name
//!
//! Each is something a conforming producer owes, and each would otherwise have
//! been left to the caller to know:
//!
//! 1. **A format writes its keystroke twin** into `/AA` `/K`. Acrobat's Format
//!    tab emits two scripts carrying the same arguments; a file with one and
//!    not the other is not one Acrobat authored.
//! 2. **A calculation writes its `/CO` entry.** A calculate action absent from
//!    the AcroForm's calculation order is one Acrobat will not run — a field
//!    that looks calculated in every inspector and computes nothing. That is
//!    the shape of `Pass 308.4` and `Pass 308.5`, and shipping it a third time
//!    on purpose was not an option.
//! 3. **An emptied `/AA` is removed**, not left as an empty dictionary a field
//!    gained and lost. Same rule `/MK` follows since `Pass 308.1`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewCheckBox, NewChoiceField, NewTextField};
use pdfcer_core::form_script::{
    self, AdvisoryHelper, CalcHelper, FormatHelper, ScriptClass, SimpleOp, Trigger,
};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("dimension/plain-base.pdf")).unwrap())
}

fn rect(n: f64) -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0 + n * 30.0,
        urx: 200.0,
        ury: 120.0 + n * 30.0,
    }
}

/// A session with one text field called `Total`.
fn with_text_field() -> EditSession {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Total", rect(0.0)).declining_tooltip())
        .unwrap();
    s
}

fn field_named(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .expect("the field")
}

/// The field's `/AA` dictionary, or `None` when it carries none.
fn aa(s: &EditSession, name: &str) -> Option<Dict> {
    let g = s.graph();
    g.resolved(field_named(s, name).id)
        .as_dict()?
        .get(b"AA")
        .map(|o| g.resolve(o).clone())?
        .as_dict()
        .cloned()
}

/// The raw `/JS` bytes on one trigger.
fn js(s: &EditSession, name: &str, trigger: Trigger) -> Option<Vec<u8>> {
    let g = s.graph();
    let action = aa(s, name)?
        .get(trigger.key())
        .map(|o| g.resolve(o).clone())?
        .as_dict()
        .cloned()?;
    match action.get(b"JS").map(|o| g.resolve(o).clone()) {
        Some(Object::String(b)) => Some(b),
        _ => None,
    }
}

/// The AcroForm's `/CO` array as object numbers.
fn co(s: &EditSession) -> Option<Vec<u32>> {
    let g = s.graph();
    let catalog = g.resolved(g.catalog_id()?).as_dict().cloned()?;
    let af = catalog
        .get(b"AcroForm")
        .map(|o| g.resolve(o).clone())?
        .as_dict()
        .cloned()?;
    match af.get(b"CO").map(|o| g.resolve(o).clone()) {
        Some(Object::Array(a)) => Some(
            a.iter()
                .filter_map(|o| Some(o.as_reference()?.num))
                .collect(),
        ),
        _ => None,
    }
}

// -------------------------------------------------------------------------
// The acceptance criterion: what is written reads back as what was asked for
// -------------------------------------------------------------------------

#[test]
fn a_number_format_reads_back_as_the_same_helper() {
    let mut s = with_text_field();
    let helper = FormatHelper::Number {
        decimals: 2,
        separator_style: 0,
        negative_style: 1,
        currency_style: 0,
        currency: b"$".to_vec(),
        prepend_currency: true,
    };
    let out = s.set_field_format("Total", Some(helper.clone())).unwrap();
    assert_eq!(out.trigger, "format");
    assert_eq!(out.replaced, None, "nothing was there before");

    let written = js(&s, "Total", Trigger::Format).expect("a /F script");
    assert_eq!(
        form_script::classify(&written, Trigger::Format),
        ScriptClass::Format(helper),
        "the parser takes back exactly what the emitter wrote"
    );
}

#[test]
fn every_format_variant_survives_the_document_round_trip() {
    // The emitter's own round-trip test pins `classify(emit(x)) == x` in
    // memory. This one puts the bytes through a real field dictionary, a save
    // and a reopen — so a `/JS` written in a form the reader cannot take back
    // fails here even if the emitter is perfect.
    let variants = [
        FormatHelper::Number {
            decimals: 0,
            separator_style: 2,
            negative_style: 0,
            currency_style: 1,
            currency: Vec::new(),
            prepend_currency: false,
        },
        FormatHelper::Percent {
            decimals: 1,
            separator_style: 0,
        },
        FormatHelper::Date { index: 3 },
        FormatHelper::DateEx {
            format: b"yyyy-mm-dd".to_vec(),
        },
        FormatHelper::Time { index: 1 },
        FormatHelper::Special { selector: 0 },
    ];
    for helper in variants {
        let mut s = with_text_field();
        s.set_field_format("Total", Some(helper.clone())).unwrap();
        let (bytes, _) = s
            .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
            .unwrap();
        let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
        let written = js(&reopened, "Total", Trigger::Format).expect("a /F script");
        assert_eq!(
            form_script::classify(&written, Trigger::Format),
            ScriptClass::Format(helper.clone()),
            "{helper:?} did not survive"
        );
    }
}

#[test]
fn inventory_reports_the_script_that_was_written() {
    // The consuming check named in the request: write a format, reopen, and
    // the census sees one `/F` script classified as that helper.
    let mut s = with_text_field();
    s.set_field_format(
        "Total",
        Some(FormatHelper::Percent {
            decimals: 2,
            separator_style: 0,
        }),
    )
    .unwrap();
    let (bytes, _) = s
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
        .unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let inv = form_script::inventory::inventory(&doc.view());

    let formats: Vec<_> = inv
        .scripts
        .iter()
        .filter(|e| e.trigger == Trigger::Format)
        .collect();
    assert_eq!(formats.len(), 1, "one format script: {inv:?}");
    assert_eq!(
        formats[0].class,
        ScriptClass::Format(FormatHelper::Percent {
            decimals: 2,
            separator_style: 0,
        })
    );
}

// -------------------------------------------------------------------------
// The keystroke twin, written because a conforming producer owes it
// -------------------------------------------------------------------------

#[test]
fn a_format_writes_its_keystroke_twin_and_says_so() {
    let mut s = with_text_field();
    let out = s
        .set_field_format("Total", Some(FormatHelper::Date { index: 0 }))
        .unwrap();
    assert!(out.keystroke_paired, "the pair is disclosed, not silent");

    let twin = js(&s, "Total", Trigger::Keystroke).expect("a /K filter");
    assert!(
        String::from_utf8_lossy(&twin).starts_with("AFDate_Keystroke("),
        "{}",
        String::from_utf8_lossy(&twin)
    );
    // And pdfcer can read its own twin back — which it could NOT for
    // `AFDate_FormatEx` until this Pass widened the keystroke family matcher.
    assert!(matches!(
        form_script::classify(&twin, Trigger::Keystroke),
        ScriptClass::Advisory(AdvisoryHelper::Keystroke { .. })
    ));
}

#[test]
fn the_ex_twin_is_recognised_too() {
    // THE READ-SIDE HOLE THE EMITTER FOUND. `AFDate_FormatEx` pairs with
    // `AFDate_KeystrokeEx`, and the classifier matched only `_Keystroke` — so
    // a real Acrobat-authored explicit-date field lost its `/K` disclosure,
    // and pdfcer's own twin was one its reader would not take back.
    let mut s = with_text_field();
    s.set_field_format(
        "Total",
        Some(FormatHelper::DateEx {
            format: b"mmm d, yyyy".to_vec(),
        }),
    )
    .unwrap();
    let twin = js(&s, "Total", Trigger::Keystroke).expect("a /K filter");
    assert!(String::from_utf8_lossy(&twin).starts_with("AFDate_KeystrokeEx("));
    assert!(
        matches!(
            form_script::classify(&twin, Trigger::Keystroke),
            ScriptClass::Advisory(AdvisoryHelper::Keystroke { .. })
        ),
        "an inverse is a test of the original: {}",
        String::from_utf8_lossy(&twin)
    );
}

#[test]
fn clearing_a_format_takes_the_twin_with_it_and_removes_an_emptied_aa() {
    let mut s = with_text_field();
    s.set_field_format("Total", Some(FormatHelper::Date { index: 0 }))
        .unwrap();
    assert!(aa(&s, "Total").is_some());

    let out = s.set_field_format("Total", None).unwrap();
    assert_eq!(out.applied, None);
    assert!(out.keystroke_paired, "the twin went with it");
    assert!(
        aa(&s, "Total").is_none(),
        "an /AA with nothing left in it is removed, not written empty"
    );
}

// -------------------------------------------------------------------------
// `/CO`, which is part of the calculate verb and not a later request
// -------------------------------------------------------------------------

#[test]
fn a_calculation_registers_itself_in_the_calculation_order() {
    let mut s = with_text_field();
    s.add_text_field(&NewTextField::new(0, "A", rect(1.0)).declining_tooltip())
        .unwrap();
    assert!(co(&s).is_none(), "no /CO to begin with");

    let out = s
        .set_field_calculation(
            "Total",
            Some(CalcHelper::Simple {
                op: SimpleOp::Sum,
                operands: vec![b"A".to_vec()],
            }),
        )
        .unwrap();

    let order = out.calculation_order.expect("a /CO disclosure");
    assert!(
        order.array_created,
        "the document gains a calculation order"
    );
    assert!(order.appended_at_end);
    assert_eq!(order.entries, 1);
    assert_eq!(order.position, Some(0));

    let ids = co(&s).expect("a /CO array");
    assert_eq!(ids, vec![field_named(&s, "Total").id.num]);
}

#[test]
fn clearing_a_calculation_prunes_it_and_does_not_leave_an_empty_array() {
    let mut s = with_text_field();
    s.add_text_field(&NewTextField::new(0, "A", rect(1.0)).declining_tooltip())
        .unwrap();
    s.set_field_calculation(
        "Total",
        Some(CalcHelper::Simple {
            op: SimpleOp::Sum,
            operands: vec![b"A".to_vec()],
        }),
    )
    .unwrap();

    let out = s.set_field_calculation("Total", None).unwrap();
    let order = out.calculation_order.expect("a /CO disclosure");
    assert!(order.array_removed, "the array it gained, it loses");
    assert_eq!(order.entries, 0);
    assert_eq!(order.position, None);
    assert!(
        co(&s).is_none(),
        "a document that had no /CO does not keep an empty one"
    );
}

#[test]
fn a_second_calculated_field_appends_and_the_order_is_reported() {
    let mut s = with_text_field();
    for (i, n) in [(1.0, "A"), (2.0, "B")] {
        s.add_text_field(&NewTextField::new(0, n, rect(i)).declining_tooltip())
            .unwrap();
    }
    let calc = |op| CalcHelper::Simple {
        op,
        operands: vec![b"A".to_vec()],
    };
    s.set_field_calculation("Total", Some(calc(SimpleOp::Sum)))
        .unwrap();
    let second = s
        .set_field_calculation("B", Some(calc(SimpleOp::Maximum)))
        .unwrap();

    let order = second.calculation_order.expect("a /CO disclosure");
    assert_eq!(order.position, Some(1), "appended at the end");
    assert_eq!(order.entries, 2);
    assert!(!order.array_created, "the array already existed");
    assert_eq!(
        co(&s).unwrap(),
        vec![field_named(&s, "Total").id.num, field_named(&s, "B").id.num]
    );
}

#[test]
fn setting_the_same_calculation_twice_does_not_double_the_co_entry() {
    let mut s = with_text_field();
    s.add_text_field(&NewTextField::new(0, "A", rect(1.0)).declining_tooltip())
        .unwrap();
    let calc = CalcHelper::Simple {
        op: SimpleOp::Sum,
        operands: vec![b"A".to_vec()],
    };
    s.set_field_calculation("Total", Some(calc.clone()))
        .unwrap();
    let again = s.set_field_calculation("Total", Some(calc)).unwrap();
    assert_eq!(again.calculation_order.unwrap().entries, 1);
    assert_eq!(co(&s).unwrap().len(), 1);
}

// -------------------------------------------------------------------------
// The refusals, which are what a shell renders
// -------------------------------------------------------------------------

#[test]
fn a_list_box_is_refused_and_a_combo_box_is_not() {
    // THE ONE WORTH READING TWICE. Both are `/Ch`; Acrobat offers the combo
    // box all three tabs and the list box none. The guess everyone makes is
    // that they behave the same.
    let options = vec![pdfcer_core::edit::ChoiceOption::plain("One")];
    let mut s = session();
    s.add_choice_field(
        &NewChoiceField::new(0, "ListBox", rect(0.0), options.clone()).declining_tooltip(),
    )
    .unwrap();
    s.add_choice_field(
        &NewChoiceField::new(0, "Combo", rect(1.0), options)
            .declining_tooltip()
            .as_combo(false),
    )
    .unwrap();

    let err = s
        .set_field_format("ListBox", Some(FormatHelper::Date { index: 0 }))
        .unwrap_err()
        .to_string();
    assert!(err.contains("list box"), "{err}");
    assert!(
        err.contains("combo"),
        "and it says what DOES carry one: {err}"
    );

    s.set_field_format("Combo", Some(FormatHelper::Date { index: 0 }))
        .expect("a combo box carries all three");
}

#[test]
fn a_check_box_is_refused_by_kind_for_all_three_verbs() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect(0.0)).declining_tooltip())
        .unwrap();

    let e1 = s
        .set_field_format("Agree", Some(FormatHelper::Date { index: 0 }))
        .unwrap_err()
        .to_string();
    let e2 = s
        .set_field_validation(
            "Agree",
            Some(AdvisoryHelper::RangeValidate {
                lower: Some(0.0),
                upper: None,
            }),
        )
        .unwrap_err()
        .to_string();
    let e3 = s
        .set_field_calculation(
            "Agree",
            Some(CalcHelper::Simple {
                op: SimpleOp::Sum,
                operands: Vec::new(),
            }),
        )
        .unwrap_err()
        .to_string();

    for (e, word) in [(&e1, "format"), (&e2, "validate"), (&e3, "calculate")] {
        assert!(e.contains("check box"), "{e}");
        assert!(e.contains(word), "the refusal names which tab: {e}");
    }
    assert!(aa(&s, "Agree").is_none(), "and nothing was written");
}

#[test]
fn an_operand_naming_no_field_is_refused_before_anything_is_written() {
    let mut s = with_text_field();
    // `A` exists, `NoSuchField` does not — so the refusal must name the
    // SECOND operand. Without the real one the test would pass on a build
    // that refused the first thing it looked at for any reason at all.
    s.add_text_field(&NewTextField::new(0, "A", rect(1.0)).declining_tooltip())
        .unwrap();
    let err = s
        .set_field_calculation(
            "Total",
            Some(CalcHelper::Simple {
                op: SimpleOp::Sum,
                operands: vec![b"A".to_vec(), b"NoSuchField".to_vec()],
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("NoSuchField"), "{err}");
    assert!(err.contains("not a field in this document"), "{err}");
    assert!(aa(&s, "Total").is_none(), "nothing partial was staged");
    assert!(co(&s).is_none(), "and no /CO was conjured");
}

#[test]
fn a_keystroke_helper_cannot_be_written_as_a_validation() {
    // The classifier keeps a keystroke helper's NAME and discards its
    // arguments, so re-emitting one would drop the filter while reporting
    // success — the defect shape this project met twice in one day.
    let mut s = with_text_field();
    let err = s
        .set_field_validation(
            "Total",
            Some(AdvisoryHelper::Keystroke {
                name: "AFNumber_Keystroke".to_owned(),
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("not what it was called with"), "{err}");
    assert!(
        err.contains("set the format instead"),
        "and it says what to do instead: {err}"
    );
}

// -------------------------------------------------------------------------
// Displacing somebody else's script is disclosed, never silent
// -------------------------------------------------------------------------

#[test]
fn replacing_a_recognised_script_names_what_it_displaced() {
    let mut s = with_text_field();
    s.set_field_format("Total", Some(FormatHelper::Date { index: 0 }))
        .unwrap();
    let out = s
        .set_field_format("Total", Some(FormatHelper::Time { index: 2 }))
        .unwrap();
    assert_eq!(
        out.replaced,
        Some(ScriptClass::Format(FormatHelper::Date { index: 0 })),
        "an operator changing a format is told what was there"
    );
}

// -------------------------------------------------------------------------
// ⚠ A format is display-only, and a writer is a new chance to break that
// -------------------------------------------------------------------------

#[test]
fn setting_a_format_does_not_touch_the_stored_value() {
    // `AFNumber_Format` makes `1234.56` read as `$1,234.56`, and the stored
    // value is the first one. `form_script::format` is display-only by design;
    // this verb writes `/AA` and nothing else.
    let mut s = session();
    s.add_text_field(
        &NewTextField::new(0, "Total", rect(0.0))
            .declining_tooltip()
            .with_value("1234.56"),
    )
    .unwrap();
    let before = field_named(&s, "Total").value.clone();

    s.set_field_format(
        "Total",
        Some(FormatHelper::Number {
            decimals: 2,
            separator_style: 0,
            negative_style: 0,
            currency_style: 0,
            currency: b"$".to_vec(),
            prepend_currency: true,
        }),
    )
    .unwrap();

    assert_eq!(
        field_named(&s, "Total").value,
        before,
        "the stored value is byte-identical across a format change"
    );
}

// -------------------------------------------------------------------------
// One undoable command, /CO included
// -------------------------------------------------------------------------

#[test]
fn a_calculation_and_its_co_entry_undo_together() {
    let mut s = with_text_field();
    s.add_text_field(&NewTextField::new(0, "A", rect(1.0)).declining_tooltip())
        .unwrap();
    s.set_field_calculation(
        "Total",
        Some(CalcHelper::Simple {
            op: SimpleOp::Sum,
            operands: vec![b"A".to_vec()],
        }),
    )
    .unwrap();
    assert!(co(&s).is_some());

    s.undo().unwrap();
    assert!(
        aa(&s, "Total").is_none() && co(&s).is_none(),
        "a half-undone calculation is a field that looks calculated and is \
         not in the order, which no second undo can repair"
    );
}

#[test]
fn a_validation_reads_back_as_the_range_it_was_given() {
    let mut s = with_text_field();
    let helper = AdvisoryHelper::RangeValidate {
        lower: Some(1.0),
        upper: Some(100.0),
    };
    s.set_field_validation("Total", Some(helper.clone()))
        .unwrap();
    let written = js(&s, "Total", Trigger::Validate).expect("a /V script");
    assert_eq!(
        form_script::classify(&written, Trigger::Validate),
        ScriptClass::Advisory(helper)
    );
}
