//! # `Pass 119.0` — editing text inside a form XObject's own content stream
//!
//! The capability the operator escalated on 2026-08-20: *"I need that editing
//! capability as it is 99 % of the text I will want to edit."* On a
//! CAD-exported sheet the page's own `/Contents` holds the producer's
//! watermark and a **form XObject** holds every label, the title block and
//! every dimension callout (*pdf dimensions* — content the exporter drew, not
//! pdfcer-authored *ce dimensions*; project rule 15). Before this Pass the edit
//! surgery only ever rewrote the page's buffer, so "edit text" reached nothing
//! an operator would click.
//!
//! ## What these tests pin, and why each one is here
//!
//! 1. **The edit lands in the form's stream object, and nowhere else** — the
//!    page's `/Contents` must come out byte-identical. That is invariant 2
//!    (`ARCHITECTURE.md` §5): pdfcer rewrites what it logically changed and
//!    nothing more.
//! 2. **The form's dictionary survives.** `/Subtype`, `/BBox`, `/Matrix` and
//!    `/Resources` are the object's identity; a replacement stream built from
//!    scratch would render as nothing at all, and would do so *silently*.
//! 3. **★ Shared content is disclosed.** A form XObject may legally be painted
//!    from several pages (ISO 32000-1 §8.10.1) and **no clause binds one to a
//!    page** (`FX-N1`, a confirmed permanent negative result). So an in-place
//!    edit changes every sheet the form appears on, and the operator is told
//!    the count and the pages — off-canvas, in the report, per rule 4 as
//!    narrowed by decision 059.
//! 4. **A proxy form is refused by name** (`R-FX-2`): a `/Ref` reference
//!    XObject's visible content is a stand-in that a conforming reader may
//!    replace wholesale, so an edit there can appear to work and not reach
//!    what is printed.
//! 5. **Resource inheritance works** — §7.8.3's fourth bullet is a `shall` on
//!    the reader, not a tolerance, and PDF 2.0 *deleted* the sentence calling
//!    it obsolete. A form that omits `/Resources` and names a page font must
//!    be editable, with the inheritance disclosed.
//! 6. **An explicit target is honoured, including when it finds nothing** — a
//!    caller that asserts a fact about the document gets told when the
//!    assertion is wrong, rather than having the search quietly widen.

use pdfcer_core::document::Document;
use pdfcer_core::text_edit::{EditOptions, EditRequest, EditTarget, edit_text};
use pdfcer_core::text_extract::{self, ExtractOptions};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Assemble a classic-xref PDF from object bodies, numbered `1..=n`.
///
/// Hand-assembled rather than checked in: every fixture below differs from its
/// neighbour by one dictionary key, and the assertions are about what pdfcer
/// does with *that key*. A checked-in binary fixture would hide the difference
/// the test is about (and project rule 7 keeps the corpus to what is needed).
fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A content-stream object body carrying `content` verbatim.
fn stream_obj(extra_keys: &str, content: &str) -> String {
    format!(
        "<< {extra_keys} /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    )
}

const HELVETICA: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

/// One page. Its own `/Contents` says `PAGE`; the form XObject it invokes
/// (object 5) says `TITLE`. The shape of every CAD sheet in miniature.
fn page_and_form_pdf() -> Vec<u8> {
    let page_content = "BT /F1 12 Tf 50 700 Td (PAGE) Tj ET\nq 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET";
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        stream_obj("", page_content),
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Matrix [1 0 0 1 0 0] \
             /Resources << /Font << /F1 6 0 R >> >>",
            form_content,
        ),
        HELVETICA.to_owned(),
    ])
}

/// **Two** pages, both invoking the SAME form object 5. The shared-invocation
/// case the standard explicitly sanctions and provides no way to detect from
/// the form itself.
fn shared_form_pdf() -> Vec<u8> {
    let page_content = "q 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET";
    let page = |contents: u32| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {contents} 0 R \
             /Resources << /Font << /F1 7 0 R >> /XObject << /X1 5 0 R >> >> >>"
        )
    };
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>".to_owned(),
        page(4),
        stream_obj("", page_content),
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] \
             /Resources << /Font << /F1 7 0 R >> >>",
            form_content,
        ),
        page(8),
        HELVETICA.to_owned(),
        stream_obj("", page_content),
    ])
}

/// A form that carries `/Ref` — a reference XObject (§8.10.4), whose visible
/// content is a proxy for content in another file.
fn reference_xobject_pdf() -> Vec<u8> {
    let page_content = "q 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET";
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        stream_obj("", page_content),
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] \
             /Resources << /Font << /F1 6 0 R >> >> \
             /Ref << /F << /Type /Filespec /F (other.pdf) >> /Page 0 >>",
            form_content,
        ),
        HELVETICA.to_owned(),
    ])
}

/// A form with **no `/Resources` at all**, naming `/F1` from the page's
/// dictionary — §7.8.3 bullet 4, which is a `shall` on the reader in both
/// editions and is *not* deprecated in PDF 2.0.
fn inheriting_form_pdf() -> Vec<u8> {
    let page_content = "q 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET";
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        stream_obj("", page_content),
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200]",
            form_content,
        ),
        HELVETICA.to_owned(),
    ])
}

/// Every text run the document extracts, concatenated.
fn all_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("edited document reloads");
    let text = text_extract::extract_document(&doc, &ExtractOptions::default())
        .expect("extraction runs on the edited document");
    text.pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .map(|r| r.text.as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// The capability
// ---------------------------------------------------------------------------

/// ★ **THE PASS, IN ONE TEST.** Text that lives inside a form XObject is found
/// and replaced, and the change survives a save-and-reload.
///
/// The reload is the half that matters: a splice that produced plausible bytes
/// but a wrong `/Length`, or that left a stale `/Filter` on unfiltered
/// content, would pass an in-memory assertion and fail here.
#[test]
fn text_inside_a_form_xobject_is_editable() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default()).expect("the edit succeeds");

    assert_eq!(
        outcome.report.form_object,
        Some(5),
        "the edit must report WHICH stream it rewrote, and it is the form's"
    );
    let text = all_text(&outcome.bytes);
    assert!(
        text.contains("SHEET"),
        "the replacement must be in the saved file: {text:?}"
    );
    assert!(
        !text.contains("TITLE"),
        "the original must be gone from the current revision: {text:?}"
    );
    assert!(
        text.contains("PAGE"),
        "the page's own text must be untouched: {text:?}"
    );
}

/// **The page's content stream is not touched.** A form edit rewrites exactly
/// one object, and the page's `/Contents` is not it.
///
/// Verified by byte-searching the appended revision rather than by trusting
/// the report: the report is pdfcer's claim about what it did, and this is the
/// file's evidence. An incremental save appends, so the ORIGINAL bytes are
/// necessarily still present — the assertion is about what the *update
/// section* re-emits, which is why it looks for the object header rather than
/// for the content.
#[test]
fn a_form_edit_rewrites_only_the_form_stream() {
    let base = page_and_form_pdf();
    let doc = Document::from_bytes(base.clone()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default()).expect("the edit succeeds");

    assert!(
        outcome.bytes.starts_with(&base),
        "an incremental save must leave the original file as a byte-prefix (R32/R46)"
    );
    let appended = &outcome.bytes[base.len()..];
    let appended = String::from_utf8_lossy(appended);
    assert!(
        appended.contains("5 0 obj"),
        "the form stream object must be re-emitted: {appended}"
    );
    assert!(
        !appended.contains("4 0 obj"),
        "the PAGE's content stream must NOT be re-emitted -- pdfcer did not logically change it: {appended}"
    );
}

/// **The form dictionary survives the edit.** `/Subtype /Form` and `/BBox` are
/// required entries; `/Matrix` and `/Resources` carry the placement and the
/// font binding. Rebuilding the dictionary — which is what the page-content
/// writer does, since a page content stream's dictionary holds nothing but
/// `/Length` — would produce a form that draws nothing, and would do it
/// without any error anywhere.
#[test]
fn the_form_dictionary_is_preserved_key_for_key() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default()).expect("the edit succeeds");
    let reloaded = Document::from_bytes(outcome.bytes).expect("the edited document reloads");

    let obj = reloaded
        .get(pdfcer_core::object::ObjId::new(5, 0))
        .map(|io| &io.value)
        .expect("object 5 is present after the edit");
    let pdfcer_core::object::Object::Stream(stream) = obj else {
        panic!("object 5 must still be a stream, got {obj:?}");
    };
    for key in [
        &b"Subtype"[..],
        &b"BBox"[..],
        &b"Matrix"[..],
        &b"Resources"[..],
    ] {
        assert!(
            stream.dict.contains_key(key),
            "the form dictionary lost /{}",
            String::from_utf8_lossy(key)
        );
    }
    assert!(
        !stream.dict.contains_key(b"Filter"),
        "the replacement content is emitted verbatim, so no /Filter may remain -- a stale one makes the file unreadable"
    );
}

// ---------------------------------------------------------------------------
// Shared invocation — the design question, made observable
// ---------------------------------------------------------------------------

/// ★ **THE DISCLOSURE THAT KEEPS A FORM EDIT HONEST.**
///
/// One form, two pages. There is no ownership rule anywhere in either ISO
/// edition (`FX-N1`), and §8.10.1 states multi-invocation as the *purpose* of
/// the feature — so a single stream holds glyphs that appear twice, and
/// editing it changes both. pdfcer cannot prevent that (there is exactly one
/// stream) and must not hide it.
#[test]
fn a_shared_form_reports_its_fan_out() {
    let doc = Document::from_bytes(shared_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default()).expect("the edit succeeds");

    assert_eq!(
        outcome.report.form_invocations, 2,
        "both invocations must be counted, document-wide"
    );
    assert_eq!(
        outcome.report.form_pages,
        vec![0, 1],
        "the operator needs to know WHICH pages changed, not just how many"
    );
    let disclosed = outcome.report.disclosures.join(" ");
    assert!(
        disclosed.contains("SHARED CONTENT"),
        "the fan-out must be disclosed in words, not only as a number a caller may drop: {disclosed}"
    );

    // And it genuinely did change both pages -- the disclosure is not a
    // pessimistic warning about something that might happen.
    let text = all_text(&outcome.bytes);
    assert_eq!(
        text.matches("SHEET").count(),
        2,
        "one stream, two invocations, two changed places: {text:?}"
    );
}

/// The ordinary case must not carry the shared-content disclosure. A warning
/// that fires every time is a warning nobody reads, and this one is meant to
/// be startling.
#[test]
fn an_unshared_form_says_nothing_about_sharing() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default()).expect("the edit succeeds");
    assert_eq!(outcome.report.form_invocations, 1);
    let disclosed = outcome.report.disclosures.join(" ");
    assert!(
        !disclosed.contains("SHARED CONTENT"),
        "a form painted once must not be described as shared: {disclosed}"
    );
}

// ---------------------------------------------------------------------------
// Refusals and targeting
// ---------------------------------------------------------------------------

/// `R-FX-2` — a reference XObject is refused **by name**, before any surgery.
///
/// Its visible content is a low-fidelity stand-in that a conforming reader may
/// substitute wholesale with content from another file. An edit here can
/// succeed on the bytes pdfcer can see and never reach what is actually
/// printed: an edit that *appears* to work and does not, which is the exact
/// class rule 4 exists to forbid.
#[test]
fn a_reference_xobject_is_refused_by_name() {
    let doc = Document::from_bytes(reference_xobject_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let err = edit_text(&doc, &req, &EditOptions::default())
        .expect_err("a /Ref proxy must be refused, not edited");
    let message = err.to_string();
    assert!(
        message.contains("/Ref"),
        "the refusal must name the trigger so the operator can act on it: {message}"
    );
}

/// §7.8.3 bullet 4: a form that omits `/Resources` inherits from **the page**.
/// The clause is a `shall` on the reader in both editions, and PDF 2.0 removed
/// the sentence that called the construct obsolete — so refusing this file
/// would be pdfcer declining to implement a rule the standard states plainly.
#[test]
fn a_form_that_inherits_the_pages_resources_is_editable_and_says_so() {
    let doc = Document::from_bytes(inheriting_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let outcome = edit_text(&doc, &req, &EditOptions::default())
        .expect("a form with inherited resources is editable");
    assert!(all_text(&outcome.bytes).contains("SHEET"));
    let disclosed = outcome.report.disclosures.join(" ");
    assert!(
        disclosed.contains("7.8.3"),
        "resolving through an inheritance rule is a fact about the file the operator may need: {disclosed}"
    );
}

/// `EditTarget::PageContents` keeps the pre-119.0 reach exactly. A batch caller
/// that means "the page's own text and nothing else" can still say so, and a
/// form match does not silently satisfy it.
#[test]
fn targeting_the_page_contents_does_not_reach_into_a_form() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req = EditRequest::find_replace(0, "TITLE", "SHEET").with_target(EditTarget::PageContents);
    let err = edit_text(&doc, &req, &EditOptions::default())
        .expect_err("the text is in the form, and the caller excluded forms");
    assert!(
        matches!(err, pdfcer_core::text_edit::EditError::NoMatch(_)),
        "not finding text where the caller said to look is a plain no-match: {err}"
    );
}

/// A named target that is not painted by this page is an **error**, not a
/// widened search. The caller asserted a fact about the document; if the
/// assertion is wrong they need to hear that, not get a different edit.
#[test]
fn a_named_form_target_that_is_not_on_the_page_is_refused() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req =
        EditRequest::find_replace(0, "TITLE", "SHEET").with_target(EditTarget::Form { object: 99 });
    let err = edit_text(&doc, &req, &EditOptions::default())
        .expect_err("form 99 is not painted by this page");
    assert!(
        err.to_string().contains("99"),
        "the refusal must name the object the caller asked for: {err}"
    );
}

/// Targeting the form explicitly reaches the same text `Auto` would — the two
/// routes must not diverge, since a shell that has a
/// `GlyphProvenance` in hand will use the explicit one.
#[test]
fn an_explicit_form_target_edits_the_same_run_auto_would() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let auto = edit_text(
        &doc,
        &EditRequest::find_replace(0, "TITLE", "SHEET"),
        &EditOptions::default(),
    )
    .expect("auto finds it");
    let explicit = edit_text(
        &doc,
        &EditRequest::find_replace(0, "TITLE", "SHEET").with_target(EditTarget::Form { object: 5 }),
        &EditOptions::default(),
    )
    .expect("the explicit target finds it too");
    assert_eq!(
        auto.bytes, explicit.bytes,
        "the two routes must produce identical files -- one surgery, two ways of naming its target"
    );
}

/// Page text stays reachable, and reaches the PAGE's object. The regression
/// this guards is the obvious one: a search that now prefers forms would edit
/// the wrong buffer on every document that has both.
#[test]
fn page_text_still_edits_the_page_stream() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let outcome = edit_text(
        &doc,
        &EditRequest::find_replace(0, "PAGE", "LEAF"),
        &EditOptions::default(),
    )
    .expect("page text is still editable");
    assert_eq!(
        outcome.report.form_object, None,
        "an edit in the page's own content must not be reported as a form edit"
    );
    assert_eq!(outcome.report.content_object, 4);
    assert_eq!(
        outcome.report.form_invocations, 0,
        "the fan-out count is meaningless for a page edit and must read as absent"
    );
    let text = all_text(&outcome.bytes);
    assert!(text.contains("LEAF") && text.contains("TITLE"), "{text:?}");
}

// ---------------------------------------------------------------------------
// The session path — undo, accumulation, and the save-time diff
// ---------------------------------------------------------------------------

/// The interactive path reaches form text too, and the edit is **one undoable
/// command**.
///
/// The one-shot free function and the session are two entry points into the
/// same surgery, and they have diverged before: `Pass 19.1` found a condition
/// re-listed in the session path that had not learned about three new fields,
/// so a request that worked through the free function became a phantom no-op
/// through the GUI. This pins the pair together for form targets from the
/// first day they exist.
#[test]
fn the_session_edits_form_text_as_one_undoable_command() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let req = EditRequest::find_replace(0, "TITLE", "SHEET");
    let report = session
        .edit_text(&req, &EditOptions::default())
        .expect("the session edit succeeds");
    assert_eq!(report.form_object, Some(5));
    assert_eq!(session.undo_depth(), 1, "exactly one command, not two");

    let (bytes, _) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("the session saves");
    assert!(all_text(&bytes).contains("SHEET"));
}

/// ★ **Edit → undo → save must produce a byte-identical file.**
///
/// `ARCHITECTURE.md` §11.1: the dirty set is a *diff against the base at save
/// time*, never a log of what was touched. A form edit that registered the
/// stream object as permanently dirty would pass every functional test above
/// and still bloat every save with a reverted change — the exact defect §11.1
/// exists to forbid, and one that is invisible unless a test looks at the
/// bytes.
#[test]
fn a_form_edit_that_is_undone_leaves_no_trace_in_the_save() {
    let base = page_and_form_pdf();
    let doc = Document::from_bytes(base.clone()).expect("fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session
        .edit_text(
            &EditRequest::find_replace(0, "TITLE", "SHEET"),
            &EditOptions::default(),
        )
        .expect("the edit succeeds");
    session.undo().expect("there is a command to undo");

    let (bytes, report) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("the session saves");
    assert_eq!(
        report.objects_written, 0,
        "an edited-then-undone form must appear in no update section: {report:?}"
    );
    assert!(all_text(&bytes).contains("TITLE"));
}

/// Two sequential edits to the SAME form compose, rather than the second
/// re-splicing the base and discarding the first.
///
/// This is the property that makes an interactive caret usable at all, and it
/// depends on the session reading the form's *staged* content rather than the
/// file's. The page path has had it since Pass 14.3; the form path is new and
/// gets its own proof rather than inheriting the claim.
#[test]
fn sequential_edits_to_one_form_accumulate() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session
        .edit_text(
            &EditRequest::find_replace(0, "TITLE", "SHEETX"),
            &EditOptions::default(),
        )
        .expect("first edit");
    session
        .edit_text(
            &EditRequest::find_replace(0, "SHEETX", "PLATE"),
            &EditOptions::default(),
        )
        .expect("second edit composes on the first, not on the base");
    assert_eq!(session.undo_depth(), 2);

    let (bytes, _) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("the session saves");
    let text = all_text(&bytes);
    assert!(text.contains("PLATE"), "{text:?}");
    assert!(!text.contains("SHEETX"), "{text:?}");
    assert!(!text.contains("TITLE"), "{text:?}");
}

// ---------------------------------------------------------------------------
// Resource resolution — the shape real producers actually ship
// ---------------------------------------------------------------------------

/// A form whose `/Resources` carries a `/Font` dictionary that does **not**
/// contain the name its own text selects is **refused by name**, not edited.
///
/// ★ **The refusal is a deliberate choice, and the reason is agreement between
/// pdfcer's own components rather than a rule in the standard.** Nothing
/// sanctions filling a partially-declared form's `/Font` from the page, and
/// `pdfcer-render`'s interpreter does not do it: its `Do` handler takes the
/// form's own `/Resources` when present and the caller's only when absent.
/// Extraction gives the same answer from the other side — it reports no run at
/// all, which this test also pins.
///
/// So an edit that "worked" here would compute its advance from `/Widths` the
/// renderer never consults, and place text somewhere the operator can see is
/// wrong while every internal check reported success. A named refusal is the
/// honest answer until real producer output is measured
/// (`C:\personal_rag\pdf\`), and that measurement would change this test.
#[test]
fn a_form_with_a_partial_font_dictionary_is_refused_rather_than_guessed_at() {
    let page_content = "q 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET";
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R          /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        stream_obj("", page_content),
        // The form declares /F9 and uses /F1.
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200]              /Resources << /Font << /F9 6 0 R >> >>",
            form_content,
        ),
        HELVETICA.to_owned(),
    ]);
    assert_eq!(
        all_text(&bytes),
        "",
        "extraction reports no run for this shape -- the two halves must agree"
    );
    let doc = Document::from_bytes(bytes).expect("fixture loads");
    let err = edit_text(
        &doc,
        &EditRequest::find_replace(0, "TITLE", "SHEET"),
        &EditOptions::default(),
    )
    .expect_err("a font the form did not declare must not be guessed at");
    assert!(
        matches!(err, pdfcer_core::text_edit::EditError::NoMatch(_)),
        "with no resolvable font there is no editable run to find: {err}"
    );
}

/// A form that declares the font it uses gets **no** inheritance disclosure.
///
/// The disclosure has to be about the name this edit actually resolved, not
/// about the resource merge: a page that carries an `/XObject` dictionary and
/// a form that does not is the ordinary case, and reporting inheritance on
/// that basis would fire the warning on nearly every form in existence. A
/// warning that always fires is one nobody reads — and this one is meant to be
/// read, because it says the re-encoding used a font the form never declared.
#[test]
fn a_self_contained_form_reports_no_inheritance() {
    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let outcome = edit_text(
        &doc,
        &EditRequest::find_replace(0, "TITLE", "SHEET"),
        &EditOptions::default(),
    )
    .expect("the edit succeeds");
    let disclosed = outcome.report.disclosures.join(" ");
    assert!(
        !disclosed.contains("7.8.3"),
        "this form declares /F1 itself -- nothing was inherited: {disclosed}"
    );
}

// ---------------------------------------------------------------------------
// `Pass 119.2` — format_text reaches the same text edit_text does
// ---------------------------------------------------------------------------

/// ★ **The asymmetry `Pass 119.0` shipped, closed.**
///
/// `edit_text` reached form content and `format_text` did not, which an
/// operator meets as *"I can change the words but not the size"* — the same
/// class of half-working the whole Pass exists to remove, one verb over. The
/// asymmetry was named as a known non-goal rather than left to be discovered,
/// and this is it being paid off.
#[test]
fn format_text_reaches_form_xobject_text() {
    use pdfcer_core::text_edit::{FormatOptions, FormatRequest, set_format};

    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let req = FormatRequest::new(0, "TITLE").size(24.0);
    let outcome =
        set_format(&doc, &req, &FormatOptions::default()).expect("the format edit succeeds");

    assert_eq!(
        outcome.report.form_object,
        Some(5),
        "the restyle must land in the form's stream, and say so"
    );
    assert_eq!(
        outcome.report.size_change,
        Some((12.0, 24.0)),
        "the size actually changed"
    );
    // The text survives the restyle -- a format edit re-emits the show
    // operator, so a broken splice would lose the run rather than resize it.
    assert!(all_text(&outcome.bytes).contains("TITLE"));
}

/// A shared form reports its fan-out on the FORMAT path too.
///
/// Pinned separately rather than assumed from `edit_text`'s test: the two verbs
/// build their reports in different functions, and "it works for the sibling"
/// is exactly the reasoning that lets one of a pair ship without the
/// disclosure. (Same lesson as `Pass 106.2`, where three computed outcome
/// fields reached no shell at all.)
#[test]
fn a_shared_form_reports_its_fan_out_on_the_format_path() {
    use pdfcer_core::text_edit::{FormatOptions, FormatRequest, set_format};

    let doc = Document::from_bytes(shared_form_pdf()).expect("fixture loads");
    let outcome = set_format(
        &doc,
        &FormatRequest::new(0, "TITLE").size(24.0),
        &FormatOptions::default(),
    )
    .expect("the format edit succeeds");

    assert_eq!(outcome.report.form_invocations, 2);
    assert_eq!(outcome.report.form_pages, vec![0, 1]);
    assert!(
        outcome
            .report
            .disclosures
            .join(" ")
            .contains("SHARED CONTENT"),
        "the fan-out must be disclosed on this path too: {:?}",
        outcome.report.disclosures
    );
}

/// The session path formats form text as one undoable command, and an
/// edit-then-undo save is byte-identical.
#[test]
fn the_session_formats_form_text_as_one_undoable_command() {
    use pdfcer_core::text_edit::{FormatOptions, FormatRequest};

    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let report = session
        .format_text(
            &FormatRequest::new(0, "TITLE").size(24.0),
            &FormatOptions::default(),
        )
        .expect("the session format succeeds");
    assert_eq!(report.form_object, Some(5));
    assert_eq!(session.undo_depth(), 1);

    session.undo().expect("there is a command to undo");
    let (_bytes, save) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("the session saves");
    assert_eq!(
        save.objects_written, 0,
        "a formatted-then-undone form must appear in no update section: {save:?}"
    );
}

/// `edit_text` and `format_text` must agree about WHICH stream a run is in.
///
/// Two verbs, two search implementations (different plan and error types), one
/// answer required. They are kept textually parallel rather than merged, and
/// parallel code drifts — this is the assertion that notices.
#[test]
fn both_verbs_target_the_same_stream_for_the_same_run() {
    use pdfcer_core::text_edit::{FormatOptions, FormatRequest, set_format};

    let doc = Document::from_bytes(page_and_form_pdf()).expect("fixture loads");
    let edited = edit_text(
        &doc,
        &EditRequest::find_replace(0, "TITLE", "SHEET"),
        &EditOptions::default(),
    )
    .expect("edit finds it");
    let formatted = set_format(
        &doc,
        &FormatRequest::new(0, "TITLE").size(24.0),
        &FormatOptions::default(),
    )
    .expect("format finds it");
    assert_eq!(edited.report.form_object, formatted.report.form_object);
    assert_eq!(
        edited.report.content_object,
        formatted.report.content_object
    );

    // And on page text, both must still answer "the page".
    let page_edit = edit_text(
        &doc,
        &EditRequest::find_replace(0, "PAGE", "LEAF"),
        &EditOptions::default(),
    )
    .expect("edit finds the page run");
    let page_format = set_format(
        &doc,
        &FormatRequest::new(0, "PAGE").size(24.0),
        &FormatOptions::default(),
    )
    .expect("format finds the page run");
    assert_eq!(page_edit.report.form_object, None);
    assert_eq!(page_format.report.form_object, None);
}
