//! `Pass 179.0` — bold becomes automatic: a fallback ladder that binds a real
//! face when one exists and synthesises when one does not, with no operator
//! intervention (decision 106).
//!
//! Rungs, per axis: (1) a real face already on the page that claims the
//! style and passes the `set_font` coverage gate; (2) the standard-14
//! sibling of the run's OWN family, bound as a new resource with nothing
//! embedded; (4) synthesis — subject to the posture. Every binding goes
//! through `plan_font`, the same code `set_font` uses (`R221`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::settings::StylePolicy;
use pdfcer_core::text_edit::{
    FontSelector, FormatError, FormatOptions, FormatRequest, StyleRung, StyleSynthesis,
};
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).unwrap())
}

/// A one-page PDF from numbered objects, with a correct xref — so a test can
/// state a font situation the committed fixtures do not have.
fn pdf(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn simple_font(base_font: &str) -> String {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    )
}

/// `hello` set in `/F1`, with the given font resources on the page.
fn page_with(fonts: &[(&str, &str)]) -> Vec<u8> {
    let content = "BT /F1 12 Tf 72 700 Td (hello) Tj ET";
    let font_refs = fonts
        .iter()
        .enumerate()
        .map(|(i, (key, _))| format!("/{key} {} 0 R", 5 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << {font_refs} >> >> >>"
            ),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
    ];
    for (i, (_, base)) in fonts.iter().enumerate() {
        objects.push((5 + i as u32, simple_font(base)));
    }
    let refs: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    pdf(&refs)
}

fn bold() -> FormatRequest {
    FormatRequest::new(0, "hello").style(StyleSynthesis::Bold)
}

fn reopened(s: &EditSession) -> String {
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn rung_2_binds_the_standard_14_sibling_on_the_discriminating_fixture() {
    // Criterion 3: format_other.pdf — one resource, Helvetica. The shipped
    // binary synthesised here; the ladder binds Helvetica-Bold, nothing embedded.
    let mut s = session("textedit/format_other.pdf");
    let r = s.format_text(&bold(), &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().expect("the ladder ran");
    assert_eq!(l.rung, StyleRung::StandardFourteenSibling);
    assert_eq!(l.bound.as_deref(), Some("Helvetica-Bold"));
    assert!(l.synthesised.is_none());
    assert!(r.synthesis.is_none(), "nothing synthesised");
    assert_eq!(
        r.font_change,
        Some(("Helvetica".to_owned(), "Helvetica-Bold".to_owned()))
    );
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.starts_with("style: bold via rung 2: the standard-14 sibling")),
        "{:?}",
        r.disclosures
    );
    let out = reopened(&s);
    assert!(out.contains("/BaseFont /Helvetica-Bold"));
    assert!(!out.contains("/FontFile"), "nothing embedded");
    assert!(!out.contains("2 Tr"), "no synthetic stroke");
}

#[test]
fn rung_1_binds_a_real_face_on_the_page_through_the_coverage_gate() {
    // Criterion 2: format_twins.pdf carries Times-Bold twice; the /Differences
    // twin cannot show 'o' and is passed over BY NAME, the plain twin binds.
    let mut s = session("textedit/format_twins.pdf");
    let r = s.format_text(&bold(), &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::RealFaceOnPage);
    assert_eq!(l.bound.as_deref(), Some("Times-Bold"));
    assert_eq!(l.passed_over.len(), 1, "{:?}", l.passed_over);
    // ★ Asserted on the FIELDS since `Pass 295.0`. The old form matched the
    // prefix of a joined `"<face> (<reason>)"` string -- the very shape the
    // consuming shell had to parse, and which this Pass replaced with a
    // struct. A test that reads the prose keeps the prose load-bearing.
    assert_eq!(
        l.passed_over[0].base_font, "Times-Bold",
        "{:?}",
        l.passed_over
    );
    assert!(
        l.passed_over[0].reason.starts_with("R-INV-7"),
        "{:?}",
        l.passed_over
    );
    // And the structured refusal travels with it, so a caller can name the
    // character instead of re-reading the sentence.
    assert!(
        l.passed_over[0]
            .refusal
            .as_ref()
            .is_some_and(|r| r.character.is_some()),
        "a coverage refusal carries the character that had no glyph: {:?}",
        l.passed_over
    );
    assert!(r.synthesis.is_none());
    // Nothing new was added: the face was already there.
    assert!(!reopened(&s).contains("/pdfceF"), "no created resource");
}

#[test]
fn rung_4_synthesises_when_no_real_face_exists_and_says_which_rung() {
    // Criterion 4: an embedded subset with no sibling anywhere → the R90
    // emission, disclosed as rung 4.
    let mut s = session("text/subset-simple-embedded.pdf");
    let r = s
        .format_text(
            &FormatRequest::new(0, "ABC").style(StyleSynthesis::Bold),
            &FormatOptions::default(),
        )
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::Synthetic);
    assert_eq!(l.synthesised, StyleSynthesis::Bold);
    assert_eq!(r.synthesis, StyleSynthesis::Bold);
    assert!(r.synthetic_bold_width.is_some());
    assert!(reopened(&s).contains("2 Tr"), "the spec-native stroke");
}

#[test]
fn refuse_posture_stops_before_rung_4_and_the_override_still_works() {
    let mut s = session("text/subset-simple-embedded.pdf");
    let req = FormatRequest::new(0, "ABC").style(StyleSynthesis::Bold);
    let err = s
        .format_text(
            &req,
            &FormatOptions::default().with_style_policy(StylePolicy::Refuse),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            FormatError::SynthesisRefusedByPosture { style: "bold", .. }
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("--bold-synthetic"), "{err}");
    assert_eq!(s.undo_depth(), 0);
    // Criterion 7: the explicit verb survives as the override.
    let r = s
        .format_text(
            &FormatRequest::new(0, "ABC").synthetic(StyleSynthesis::Bold),
            &FormatOptions::default().with_style_policy(StylePolicy::Refuse),
        )
        .expect("the explicit request is the operator's say-so");
    assert_eq!(r.synthesis, StyleSynthesis::Bold);
    assert!(r.style_ladder.is_none(), "the ladder did not run");
}

#[test]
fn per_axis_a_real_bold_binds_while_italic_is_synthesised() {
    // Criterion 5 — the Acrobat exceed. `Verdana` has no standard-14 sibling
    // (rung 2 cannot fire) and the page carries only Verdana-Bold, so bold
    // binds at rung 1 and italic falls to rung 4, in ONE operation.
    let doc =
        Document::from_bytes(page_with(&[("F1", "Verdana"), ("F2", "Verdana-Bold")])).unwrap();
    let mut s = EditSession::new(doc);
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").style(StyleSynthesis::BoldItalic),
            &FormatOptions::default(),
        )
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::RealFaceOnPage);
    assert_eq!(l.bound.as_deref(), Some("Verdana-Bold"));
    assert_eq!(l.synthesised, StyleSynthesis::Italic);
    assert_eq!(r.synthesis, StyleSynthesis::Italic);
    assert!(r.synthetic_italic.is_some());
    assert!(r.synthetic_bold_width.is_none(), "bold is REAL");
    assert!(
        r.disclosures.iter().any(|d| d
            .contains("bound 'Verdana-Bold' for the bold axis; italic is synthesised (rung 4)")),
        "{:?}",
        r.disclosures
    );
}

#[test]
fn a_full_standard_14_sibling_beats_a_half_synthesised_page_face() {
    // Times-Roman run, Times-Bold on the page, bold+italic asked: rung 2's
    // Times-BoldItalic (both axes real, nothing embedded) wins over
    // rung 1's Times-Bold + synthetic italic.
    let mut s = session("textedit/format_twins.pdf");
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").style(StyleSynthesis::BoldItalic),
            &FormatOptions::default(),
        )
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::StandardFourteenSibling);
    assert_eq!(l.bound.as_deref(), Some("Times-BoldItalic"));
    assert!(r.synthesis.is_none());
}

#[test]
fn a_run_that_already_has_the_style_is_left_alone() {
    let doc = Document::from_bytes(page_with(&[("F1", "Helvetica-Bold")])).unwrap();
    let mut s = EditSession::new(doc);
    let r = s.format_text(&bold(), &FormatOptions::default());
    match r {
        Ok(r) => {
            let l = r.style_ladder.as_ref().unwrap();
            assert_eq!(l.rung, StyleRung::AlreadyStyled);
            assert!(r.synthesis.is_none());
            assert!(r.font_change.is_none());
        }
        // A no-op is also an honest answer, as long as it is named.
        Err(FormatError::NoOp) => {}
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn the_ladder_refuses_to_be_combined_with_a_named_face_or_an_overlapping_override() {
    let mut s = session("textedit/format_other.pdf");
    let err = s
        .format_text(
            &bold().font(FontSelector::new("Helvetica-Bold")),
            &FormatOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(err, FormatError::Unsupported(ref m) if m.contains("--set-font")),
        "{err:?}"
    );
    let err = s
        .format_text(
            &bold().synthetic(StyleSynthesis::Bold),
            &FormatOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(err, FormatError::Unsupported(ref m) if m.contains("same axis twice")),
        "{err:?}"
    );
    // Non-overlapping: ladder bold + explicit synthetic italic is allowed.
    let r = s
        .format_text(
            &bold().synthetic(StyleSynthesis::Italic),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(
        r.style_ladder.as_ref().unwrap().rung,
        StyleRung::StandardFourteenSibling
    );
    assert_eq!(r.synthesis, StyleSynthesis::Italic);
}

// ------------------------------------------------ `Pass 295.0` — the shell's
// ------------------------------------------------ four requests against 0.53.0

/// ★★★ The preview and the commit are two readings of ONE answer.
///
/// `preview_style_resolution` previews the R90 gate, which since `Pass 179.0`
/// answers a different question: it cannot see rung 2, because the standard-14
/// sibling needs no font file and is not on the page. The consuming shell's
/// tooltip therefore predicted synthesis while the status line afterwards
/// reported a real `Helvetica-Bold` — two instruments disagreeing by
/// construction, on the commonest page in the operator's working set.
///
/// This asserts the property that fixes it: what the preview says, the commit
/// does. Asserting the rung alone would pass on a preview that always answered
/// "rung 2"; the commit is run on the same session and compared.
#[test]
fn the_ladder_preview_answers_what_the_commit_then_does() {
    let mut s = session("textedit/format_other.pdf");
    let opts = FormatOptions::default();

    let preview = s
        .preview_style_ladder(0, "hello", None, StyleSynthesis::Bold, &opts)
        .expect("the preview runs");
    assert_eq!(preview.rung, StyleRung::StandardFourteenSibling);
    assert_eq!(preview.bound.as_deref(), Some("Helvetica-Bold"));

    let committed = s.format_text(&bold(), &opts).unwrap();
    let l = committed.style_ladder.as_ref().expect("the ladder ran");
    assert_eq!(preview.rung, l.rung, "the preview predicted the wrong rung");
    assert_eq!(preview.bound, l.bound, "...or the wrong face");
}

/// The preview is READ-ONLY: running it must not change what a later commit
/// does, nor leave anything staged.
#[test]
fn the_ladder_preview_stages_nothing() {
    let s = session("textedit/format_other.pdf");
    let before = reopened(&s);
    for _ in 0..3 {
        s.preview_style_ladder(
            0,
            "hello",
            None,
            StyleSynthesis::Bold,
            &FormatOptions::default(),
        )
        .expect("the preview runs");
    }
    assert_eq!(before, reopened(&s), "a preview staged something");
}

/// ★★ Under `Refuse`, the preview REFUSES — because that is what the commit
/// would do. A preview that promised rung 4 here would be predicting an event
/// that never happens.
#[test]
fn the_ladder_preview_refuses_where_the_commit_would_refuse() {
    let mut s = session("text/subset-simple-embedded.pdf");
    let opts = FormatOptions::default().with_style_policy(StylePolicy::Refuse);

    let req = FormatRequest::new(0, "ABC").style(StyleSynthesis::Bold);
    let previewed = s.preview_style_ladder(0, "ABC", None, StyleSynthesis::Bold, &opts);
    let committed = s.format_text(&req, &opts);

    assert!(
        matches!(
            previewed,
            Err(FormatError::SynthesisRefusedByPosture { .. })
        ),
        "{previewed:?}"
    );
    assert!(
        matches!(
            committed,
            Err(FormatError::SynthesisRefusedByPosture { .. })
        ),
        "the fixture must be one the commit also refuses, or this proves nothing: {committed:?}"
    );
}

/// ★★ `same_family` distinguishes the invisible change from the visible one.
///
/// `Helvetica` → `Helvetica-Bold` is invisible to a draughtsman;
/// `Helvetica` → another family's bold changes the look of a title block, and
/// that is the one he wants to be told about. The shell used to know because
/// it walked the page itself; adopting the ladder deleted the walk and took
/// the sentence with it.
#[test]
fn the_ladder_says_whether_the_bound_face_is_the_runs_own_family() {
    let mut s = session("textedit/format_other.pdf");
    let r = s.format_text(&bold(), &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().expect("the ladder ran");
    assert_eq!(l.bound.as_deref(), Some("Helvetica-Bold"));
    assert_eq!(
        l.same_family,
        Some(true),
        "Helvetica -> Helvetica-Bold is the same family"
    );
}

/// ★★★ THE HALF THE FIRST CUT OF THIS TEST COULD NOT FAIL.
///
/// The same-family assertion above passed a sabotage that hard-coded
/// `same_family: Some(true)`, because its fixture only ever binds
/// `Helvetica` → `Helvetica-Bold`. A fixture that cannot produce the other
/// answer cannot test for it — `R225`, caught here by sabotaging rather than
/// by reading.
///
/// This is the case the operator cares about: the run is `Helvetica`, the only
/// face on the page claiming bold is `Times-Bold`, and taking it CHANGES THE
/// LOOK of a title block. `Some(false)` is what makes that sentence sayable.
#[test]
fn same_family_is_false_when_the_bound_face_is_another_family() {
    let doc =
        Document::from_bytes(page_with(&[("F1", "Helvetica"), ("F2", "Times-Bold")])).unwrap();
    let mut s = EditSession::new(doc);
    let r = s.format_text(&bold(), &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().expect("the ladder ran");
    assert_eq!(l.rung, StyleRung::RealFaceOnPage, "{l:?}");
    assert_eq!(l.bound.as_deref(), Some("Times-Bold"));
    assert_eq!(
        l.same_family,
        Some(false),
        "Helvetica -> Times-Bold is a DIFFERENT family, and the operator is          entitled to be told: {l:?}"
    );
}

/// ...and when nothing is bound there is nothing to compare, which is `None`
/// and not `Some(false)`. Flattening the two would tell an operator that a
/// synthesised stroke came from a different family.
#[test]
fn same_family_is_none_when_no_face_was_bound() {
    let mut s = session("text/subset-simple-embedded.pdf");
    let req = FormatRequest::new(0, "ABC").style(StyleSynthesis::Bold);
    let r = s.format_text(&req, &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().expect("the ladder ran");
    if l.bound.is_none() {
        assert_eq!(l.same_family, None, "{l:?}");
    }
}
