//! Walk a document's form fields and classify every script they carry.
//!
//! # What this produces, and why it is a list rather than a count
//!
//! Posture A already counts scripts ([`crate::forms::scan_javascript`]) — how
//! many fields calculate, how many format, how many custom. A count answers
//! "is this form script-driven?" and nothing else. Posture B has to answer a
//! different question: **which field, computed how, from what, and can pdfcer
//! reproduce it?** That is per-field, so this is per-field.
//!
//! The same distinction was learned the hard way elsewhere in this project
//! (standing rule R181): a count told the operator that *something* had lost
//! its formatting without telling them *which*, and on a batch surface that
//! is the only question worth answering.
//!
//! # Where the `/JS` bytes come from
//!
//! §12.6.4.16 lets a JavaScript action's `/JS` be **either** a text string
//! **or** a stream. Both are read here — a stream through the standard
//! filter pipeline, so a Flate-compressed script is classified exactly like
//! an uncompressed one. Producers do use the stream form for long scripts,
//! and a recogniser that silently saw nothing in a stream would report a
//! form as script-free when it is not, which is the most dangerous possible
//! direction for this particular error: it understates risk.
//!
//! A `/JS` that is neither, or a stream that will not decode, is recorded as
//! [`ScriptSource::Unreadable`] rather than dropped. "There is a script here
//! and pdfcer could not read it" is a fact an operator needs; silence would
//! read as "no script".
//!
//! # Traversal
//!
//! Field discovery reuses [`crate::forms::parse_acroform`], so the field
//! tree is walked once, by the code that already handles inheritance,
//! Shape A/B merging, `/T`-less kids and the cycle/depth bounds — rather
//! than a second, subtly different walk drifting from the first.

use crate::filters;
use crate::forms;
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::view::DocumentView;

use super::{ScriptClass, Trigger, classify};

/// How a script's bytes were carried in the file.
///
/// Recorded because it changes what a disclosure can honestly claim. A
/// script pdfcer could not read is not a script pdfcer found nothing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptSource {
    /// `/JS` was a text string (§7.9.2) — the common form.
    LiteralString,
    /// `/JS` was a stream, decoded through the filter pipeline.
    Stream,
    /// `/JS` was present but could not be read: a stream that failed to
    /// decode, or an object of an unexpected type.
    ///
    /// **Always classified [`ScriptClass::Custom`]**, because a script whose
    /// text pdfcer never saw cannot possibly be a recognised built-in. This
    /// is the safe direction by construction rather than by policy.
    Unreadable,
}

/// One script found on one field trigger.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldScript {
    /// The field's fully-qualified name (§12.7.3.2). May be empty for a
    /// field with no `/T` anywhere on its path — kept as found rather than
    /// synthesised, because an invented name would not match anything the
    /// operator can look up.
    pub field: String,
    /// The field dictionary's object id — the stable identity a recompute
    /// writes through, and the only unambiguous handle when two fields share
    /// a fully-qualified name.
    pub id: ObjId,
    /// Which `/AA` trigger carried it.
    pub trigger: Trigger,
    /// What pdfcer made of it.
    pub class: ScriptClass,
    /// How the bytes were carried.
    pub source: ScriptSource,
    /// The script's length in bytes, or 0 when unreadable.
    ///
    /// Present so a shell can show the size of something it is declining to
    /// interpret — "a 4 KB custom script" sets an expectation that "a custom
    /// script" does not.
    pub length: usize,
}

impl FieldScript {
    /// Whether pdfcer can natively reproduce this script's effect.
    #[must_use]
    pub const fn is_reproducible(&self) -> bool {
        self.class.is_reproducible()
    }
}

/// A whole document's classified form scripts, in field-tree order.
///
/// Ordering is the traversal's, not sorted: it matches
/// [`crate::forms::parse_acroform`]'s field order, so a shell showing
/// scripts beside fields shows them in the same sequence rather than in two
/// unrelated orders.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptInventory {
    /// Every script found, one entry per field-trigger pair.
    pub scripts: Vec<FieldScript>,
}

impl ScriptInventory {
    /// The scripts pdfcer could natively reproduce.
    pub fn reproducible(&self) -> impl Iterator<Item = &FieldScript> {
        self.scripts.iter().filter(|s| s.is_reproducible())
    }

    /// The calculation scripts, in inventory order.
    ///
    /// The set a recompute considers. Filtering by trigger as well as by
    /// class is redundant — [`classify`] already refuses a calculation
    /// helper on any other trigger — and is done anyway, because the cost is
    /// one comparison and the property it protects is "nothing but a
    /// calculate trigger can ever reach the code that writes `/V`".
    pub fn calculations(&self) -> impl Iterator<Item = &FieldScript> {
        self.scripts.iter().filter(|s| {
            s.trigger == Trigger::Calculate && matches!(s.class, ScriptClass::Calculate(_))
        })
    }

    /// A histogram by classification token, for a one-line summary.
    ///
    /// Returns counts in a fixed order rather than a map, so CLI output is
    /// byte-stable across runs — a map's iteration order would make a
    /// diffable inventory undiffable.
    #[must_use]
    pub fn histogram(&self) -> Vec<(&'static str, usize)> {
        let mut out: Vec<(&'static str, usize)> = Vec::new();
        for s in &self.scripts {
            let token = s.class.token();
            if let Some(entry) = out.iter_mut().find(|(t, _)| *t == token) {
                entry.1 += 1;
            } else {
                out.push((token, 1));
            }
        }
        out
    }
}

/// The four triggers a form field's `/AA` may carry a script on, in the order
/// they are reported.
const TRIGGERS: [Trigger; 4] = [
    Trigger::Calculate,
    Trigger::Format,
    Trigger::Validate,
    Trigger::Keystroke,
];

/// Build the classified script inventory for a document.
///
/// Returns an empty inventory for a document with no `/AcroForm` — the
/// absence of a form is not an error, and a caller distinguishing "no form"
/// from "a form with no scripts" can ask [`crate::forms::parse_acroform`]
/// itself.
#[must_use]
pub fn inventory(view: &DocumentView<'_>) -> ScriptInventory {
    let mut out = ScriptInventory::default();
    let Some(form) = forms::parse_acroform(view) else {
        return out;
    };
    for field in &form.fields {
        // The read projection already tells us which fields have an /AA, so
        // the great majority of fields cost one boolean rather than a
        // dictionary lookup.
        if !field.has_additional_actions {
            continue;
        }
        let Some(aa) = view
            .resolved(field.id)
            .as_dict()
            .and_then(|d| d.get(b"AA"))
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
        else {
            continue;
        };
        for trigger in TRIGGERS {
            let Some(action) = aa.get(trigger.key()).map(|o| view.resolve(o)) else {
                continue;
            };
            // Only a /S /JavaScript action carries a script. A /SubmitForm
            // on /K is a real and common thing; it is posture A's hazard
            // census to report, not this module's to classify.
            let Some(dict) = action.as_dict() else {
                continue;
            };
            if dict
                .get(b"S")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes())
                != Some(b"JavaScript")
            {
                continue;
            }
            let (source, text) = read_js(view, dict.get(b"JS"));
            let class = match &text {
                Some(js) => classify(js, trigger),
                // Unreadable: cannot be a recognised built-in, because
                // nothing was read.
                None => ScriptClass::Custom,
            };
            out.scripts.push(FieldScript {
                field: field.fully_qualified_name.clone(),
                id: field.id,
                trigger,
                class,
                source,
                length: text.as_ref().map_or(0, Vec::len),
            });
        }
    }
    out
}

/// Read a `/JS` entry's bytes, from either carrier (§12.6.4.16).
///
/// Returns the carrier kind alongside the bytes so a caller can disclose
/// *why* a script was unreadable rather than only that it was.
fn read_js(view: &DocumentView<'_>, js: Option<&Object>) -> (ScriptSource, Option<Vec<u8>>) {
    match js.map(|o| view.resolve(o)) {
        Some(Object::String(s)) => (ScriptSource::LiteralString, Some(s.clone())),
        Some(Object::Stream(stream)) => {
            let decoded = view
                .slice(stream.data_span)
                .and_then(|raw| filters::decode_stream(&stream.dict, raw).ok());
            match decoded {
                Some(bytes) => (ScriptSource::Stream, Some(bytes)),
                // A stream that will not decode is reported as unreadable,
                // never as absent. An undecodable script is still a script.
                None => (ScriptSource::Unreadable, None),
            }
        }
        // Includes an absent /JS, which is a malformed JavaScript action:
        // /JS is Required (§12.6.4.16 Table 217). Reported as unreadable
        // rather than skipped, so the field still discloses that it carries
        // a JavaScript trigger.
        _ => (ScriptSource::Unreadable, None),
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
    use crate::document::Document;
    use crate::pageops::tests_support::build_pdf_bytes;

    /// A one-page document whose single text field `Total` carries the given
    /// `/AA` dictionary body, plus any extra numbered objects the `/AA`
    /// refers to (object numbers 5 and up are free).
    fn doc_with_aa(aa_body: &str, extra: &[(u32, &str)]) -> Vec<u8> {
        let field = format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Total) /V (0) \
             /Rect [0 0 100 20] /AA << {aa_body} >> >>"
        );
        let mut objects: Vec<(u32, &str)> = vec![
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            ),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
            ),
            (4, &field),
        ];
        objects.extend_from_slice(extra);
        build_pdf_bytes(&objects)
    }

    fn inventory_of(bytes: &[u8]) -> ScriptInventory {
        let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
        let view = DocumentView::new(&doc, doc.bytes(), doc.version());
        inventory(&view)
    }

    /// A `/JS` literal string carrying a calculate helper. The parentheses of
    /// the JavaScript call are escaped because a PDF literal string counts
    /// them (§7.3.4.2); everything else is verbatim.
    const CALC_JS: &str =
        r#"/C << /S /JavaScript /JS (AFSimple_Calculate\("SUM", ["A","B"]\);) >>"#;
    /// A `/JS` literal string carrying the canonical number-format helper.
    const FORMAT_JS: &str =
        r#"/F << /S /JavaScript /JS (AFNumber_Format\(2, 0, 0, 0, "", true\);) >>"#;
    /// A `/JS` literal string carrying a range validation.
    const VALIDATE_JS: &str =
        r"/V << /S /JavaScript /JS (AFRange_Validate\(true, 1, true, 9\);) >>";

    /// A recognised calculation on the calculate trigger is inventoried,
    /// named, and reported as reproducible.
    #[test]
    fn a_recognised_calculation_is_inventoried_and_reproducible() {
        let inv = inventory_of(&doc_with_aa(CALC_JS, &[]));
        assert_eq!(inv.scripts.len(), 1, "one trigger, one script");
        let s = &inv.scripts[0];
        assert_eq!(s.field, "Total");
        assert_eq!(s.trigger, Trigger::Calculate);
        assert_eq!(s.source, ScriptSource::LiteralString);
        assert!(s.is_reproducible());
        assert_eq!(inv.calculations().count(), 1);
        assert_eq!(inv.reproducible().count(), 1);
    }

    /// ★ **A `/JS` carried as a STREAM is classified, not silently missed.**
    ///
    /// The dangerous direction of this error is understating: a form whose
    /// scripts all live in streams would otherwise be reported script-free,
    /// and "no scripts" is exactly the answer an operator would act on.
    #[test]
    fn a_script_carried_as_a_stream_is_decoded_and_classified() {
        let js = r#"AFSimple_Calculate("SUM", ["A","B"]);"#;
        let stream = format!("<< /Length {} >>\nstream\n{js}\nendstream", js.len());
        let bytes = doc_with_aa("/C << /S /JavaScript /JS 5 0 R >>", &[(5, &stream)]);
        let inv = inventory_of(&bytes);
        assert_eq!(inv.scripts.len(), 1);
        assert_eq!(inv.scripts[0].source, ScriptSource::Stream);
        assert!(
            inv.scripts[0].is_reproducible(),
            "a streamed script classifies exactly like a literal one"
        );
    }

    /// A JavaScript action with no `/JS` at all is still reported — as
    /// unreadable and custom, never as absent.
    #[test]
    fn a_javascript_action_with_no_script_is_reported_as_unreadable() {
        let inv = inventory_of(&doc_with_aa("/C << /S /JavaScript >>", &[]));
        assert_eq!(inv.scripts.len(), 1, "the trigger is still disclosed");
        assert_eq!(inv.scripts[0].source, ScriptSource::Unreadable);
        assert_eq!(inv.scripts[0].class, ScriptClass::Custom);
        assert!(!inv.scripts[0].is_reproducible());
    }

    /// A non-JavaScript action on a trigger is not a script and is not
    /// inventoried here — posture A's hazard census owns it.
    #[test]
    fn a_non_javascript_action_is_not_a_script() {
        let aa = "/K << /S /SubmitForm /F (http://example.invalid) >>";
        assert!(inventory_of(&doc_with_aa(aa, &[])).scripts.is_empty());
    }

    /// Several triggers on one field each produce their own entry, in a fixed
    /// order, and each is classified against its own trigger.
    #[test]
    fn each_trigger_is_classified_separately_and_in_a_fixed_order() {
        let aa = format!("{CALC_JS} {FORMAT_JS} {VALIDATE_JS}");
        let inv = inventory_of(&doc_with_aa(&aa, &[]));
        assert_eq!(inv.scripts.len(), 3);
        assert_eq!(
            inv.scripts.iter().map(|s| s.trigger).collect::<Vec<_>>(),
            vec![Trigger::Calculate, Trigger::Format, Trigger::Validate],
            "reported in TRIGGERS order, not dictionary order"
        );
        assert_eq!(inv.scripts[0].class.token(), "AFSimple_Calculate");
        assert_eq!(inv.scripts[1].class.token(), "AFNumber_Format");
        assert_eq!(inv.scripts[2].class.token(), "AFRange_Validate");
        assert_eq!(
            inv.calculations().count(),
            1,
            "only the calculate trigger's helper is a calculation"
        );
    }

    /// Author code is inventoried as custom, with its length, so a shell can
    /// disclose the size of what it is declining to interpret.
    #[test]
    fn custom_code_is_inventoried_with_its_length() {
        let aa = r"/C << /S /JavaScript /JS (event.value = this.getField\(A\).value * 2;) >>";
        let inv = inventory_of(&doc_with_aa(aa, &[]));
        assert_eq!(inv.scripts[0].class, ScriptClass::Custom);
        assert!(inv.scripts[0].length > 20, "the size is recorded");
        assert_eq!(inv.calculations().count(), 0);
    }

    /// A document with no form yields an empty inventory rather than an
    /// error — the absence of a form is not a failure.
    #[test]
    fn a_document_with_no_form_yields_an_empty_inventory() {
        let bytes = build_pdf_bytes(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        ]);
        assert!(inventory_of(&bytes).scripts.is_empty());
    }

    /// The histogram is stable in order and totals to the script count.
    #[test]
    fn the_histogram_is_stable_and_totals_to_the_script_count() {
        let aa = format!("{CALC_JS} {FORMAT_JS}");
        let inv = inventory_of(&doc_with_aa(&aa, &[]));
        let hist = inv.histogram();
        assert_eq!(
            hist,
            vec![("AFSimple_Calculate", 1), ("AFNumber_Format", 1)]
        );
        assert_eq!(
            hist.iter().map(|(_, n)| n).sum::<usize>(),
            inv.scripts.len()
        );
    }
}
