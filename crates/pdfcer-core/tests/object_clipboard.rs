//! `Pass 120.0`/`120.3` — the object clipboard: `copy_objects`,
//! `paste_objects`, `paste_preview`, `cut_objects`.
//!
//! ## The claim this Pass was filed against, and what checking it found
//!
//! `pdfcer-gui` asked for a clipboard on the reading that
//! `EditSession::import_object` already does the hard part — a recursive
//! object-graph copy with reference remapping, cycle handling and stream
//! re-staging — so the ask was *"expose the one you have at object
//! granularity"*. **The reading is correct, and it is the smaller half.**
//!
//! `import_object` copies *indirect objects*. A page's content objects are
//! **byte ranges inside a content stream**, and the operators in those bytes
//! name their resources **by page-local name**: `/F1 12 Tf`, `/Im1 Do`. On the
//! destination page `/F1` is a different font. Pasting the bytes verbatim
//! draws the right shapes in the wrong typeface, or draws nothing, **and
//! neither failure errors.**
//!
//! So the tests here are weighted accordingly: the object-graph copy gets one
//! test, and **resource-name rebinding gets four**, because that is where the
//! silent wrongness lives.
//!
//! ## What is pinned
//!
//! 1. **★ A paste into a page whose `/F1` is a DIFFERENT font renders the
//!    clip's font, not the destination's** — the whole point, and the one
//!    failure a shell could not detect for itself.
//! 2. **The clip owns its resources**, so copy → drop the source session →
//!    paste still works.
//! 3. Paste-in-place, paste-with-offset and paste-rotated are one verb taking
//!    a page-space matrix.
//! 4. The preview and the verb cannot disagree; the preview commits nothing.
//! 5. Cut is **one** undo entry, and refuses with nothing deleted if the copy
//!    half refuses.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{Matrix, Point};
use pdfcer_core::writer::SaveOptions;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A one-page PDF whose content is `content`, whose `/F1` is `base_font`, and
/// which carries an image XObject `/Im1`.
///
/// `base_font` is a parameter for exactly one test — the one that pastes
/// between two documents whose `/F1` means different things — and that test is
/// the reason this whole Pass is more than `import_object`.
fn pdf_with_font(content: &str, base_font: &str) -> Vec<u8> {
    let image = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} /Encoding /WinAnsiEncoding >>"
        ),
        image.to_owned(),
    ];
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

fn pdf(content: &str) -> Vec<u8> {
    pdf_with_font(content, "Helvetica")
}

const MIXED: &str =
    "0 0 10 10 re S\nBT /F1 12 Tf 20 20 Td (hi) Tj ET\nq 5 0 0 5 40 40 cm /Im1 Do Q";

/// The saved bytes of a session, as text, for byte-level assertions.
fn saved(session: &EditSession) -> String {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---------------------------------------------------------------------------
// ★ THE ONE THAT MATTERS: resource-name rebinding
// ---------------------------------------------------------------------------

/// ★★ **A text run copied from a Helvetica document and pasted into a
/// Courier one must still be Helvetica.**
///
/// Both documents call their font `/F1`. Pasting the copied bytes verbatim
/// would bind to the *destination's* `/F1` and silently render the text in
/// Courier — the right glyphs, the wrong typeface, no error anywhere. This is
/// the failure `import_object` alone cannot prevent, because the name is not a
/// reference and there is nothing in the object graph to remap.
///
/// The assertion is on the saved bytes rather than on a render, because what
/// must be true is structural: the pasted operator names a resource that
/// resolves to a Helvetica font.
#[test]
fn a_pasted_font_is_the_clips_font_not_the_destinations() {
    let source = Document::from_bytes(pdf_with_font(MIXED, "Helvetica")).unwrap();
    let source_session = EditSession::new(source);
    let clip = source_session.copy_objects(0, &[1]).expect("copy the text");

    let destination = Document::from_bytes(pdf_with_font("0 0 5 5 re f", "Courier")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect("paste succeeds");
    assert_eq!(outcome.objects_pasted, 1);
    assert!(
        outcome.resources_added >= 1,
        "the font had to arrive with it: {outcome:?}"
    );

    let text = saved(&session);
    assert!(
        text.contains("/Helvetica"),
        "the clip's own font must have been imported: {text}"
    );
    // ★ AN ABSENCE ASSERTION OVER `to_incremental_bytes`, AND ITS SOUNDNESS
    // IS A PROPERTY OF THE FIXTURE RATHER THAN OF THE CODE.
    //
    // `saved()` here is an INCREMENTAL save, so `text` is every revision of
    // the file concatenated, not the state in force. A `!contains` over that
    // can only mean "these bytes were never written anywhere", which is a
    // stronger claim than the one being made and is true here for a reason
    // this test does not otherwise state:
    //
    //   - the destination fixture's content is `0 0 5 5 re f` -- no text
    //     operator at all, so `/F1 12 Tf` cannot appear from the base
    //     revision; and
    //   - the source carrying that string is a SEPARATE `Document` that is
    //     never saved into this one.
    //
    // Give the destination fixture any text and this assertion goes VACUOUS
    // with nothing turning red -- it would then be finding the destination's
    // own operator and passing for the wrong reason.
    //
    // Stated rather than hardened, deliberately: switching to a full rewrite
    // would change what the test exercises (the paste's incremental path is
    // the interesting one), and the honest fix for a fixture that grows text
    // is to assert on the CURRENT resource dictionary instead. See
    // `C:\personal_rag\pdf\lesson_20260813_absence_assertion_vacuous_under_incremental_save.md`,
    // whose worked examples include two other tests in this repo.
    assert!(
        !text.contains("/F1 12 Tf"),
        "the pasted operator must NOT still say /F1 -- that is the destination's Courier: {text}"
    );
}

/// The rebound name is the one actually written into the page's
/// `/Resources`, not merely *a* fresh name.
///
/// Detecting the name in the content and binding it in the resources are two
/// halves that could disagree — and if they did, the paste would name a
/// resource that is not there, which draws nothing.
#[test]
fn the_rewritten_name_is_the_one_bound_in_the_pages_resources() {
    let source = Document::from_bytes(pdf(MIXED)).unwrap();
    let session_a = EditSession::new(source);
    let clip = session_a.copy_objects(0, &[1]).unwrap();

    let destination = Document::from_bytes(pdf("0 0 5 5 re f")).unwrap();
    let mut session = EditSession::new(destination);
    session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();

    let text = saved(&session);
    // Find the name the pasted `Tf` uses, then assert the page binds it.
    let tf = text.find(" Tf").expect("a Tf survived the paste");
    let head = &text[..tf];
    let name_start = head.rfind('/').expect("the Tf names a font");
    let name: String = head[name_start + 1..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    assert!(
        name.starts_with("pdfceP"),
        "a pasted binding gets a fresh pdfcer-prefixed name, got {name:?}"
    );
    // ★ TWO occurrences, not one. `contains` alone was satisfied by the
    // CONTENT STREAM's own `/pdfcePF0 12 Tf` — so the first draft asserted
    // "the name I just found is present", which is a tautology, and passed
    // with the resource binding disabled. The second occurrence is the
    // `/Resources` entry, which is the half being claimed.
    assert!(
        text.matches(&format!("/{name}")).count() >= 2,
        "the page's /Resources must bind {name:?} as well as the content naming it: {text}"
    );
}

/// An `/XObject` invocation is rebound too — the same machinery, a different
/// category.
///
/// Pinned separately rather than assumed from the font case: the two go
/// through the same table, and a table with one wrong row fails for exactly
/// one category while every font test stays green.
#[test]
fn an_image_invocation_is_rebound_as_well() {
    let source = Document::from_bytes(pdf(MIXED)).unwrap();
    let session_a = EditSession::new(source);
    let clip = session_a.copy_objects(0, &[2]).expect("copy the image");
    assert_eq!(clip.kinds(), vec!["image"]);
    assert_eq!(
        clip.resource_count(),
        1,
        "the image object itself must travel on the clip"
    );

    let destination = Document::from_bytes(pdf("0 0 5 5 re f")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    assert_eq!(outcome.resources_added, 1);
    let text = saved(&session);
    assert!(
        text.contains("pdfcePX0 Do"),
        "the pasted Do must name the freshly-bound XObject: {text}"
    );
    // ★ AND the object it names must have ARRIVED. The first draft stopped at
    // the line above and passed with the whole resource import disabled --
    // name rewriting alone satisfied it, so it tested half the mechanism while
    // reading as if it tested both.
    let subtype_count = text.matches("/Subtype /Image").count();
    assert_eq!(
        subtype_count, 2,
        "the destination's own image plus the imported one: {text}"
    );
}

/// A path names no resource at all, so its clip carries none — the negative
/// case, which stops "everything gets a resource" from passing vacuously.
#[test]
fn a_plain_path_carries_no_resources() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).expect("copy the path");
    assert_eq!(clip.kinds(), vec!["path"]);
    assert_eq!(
        clip.resource_count(),
        0,
        "a stroked rectangle references nothing"
    );
}

// ---------------------------------------------------------------------------
// The clip owns what it needs
// ---------------------------------------------------------------------------

/// ★ **Copy, drop the source session, paste.**
///
/// The clip carries the transitive closure of its resources by value, with
/// stream payloads owned as bytes rather than as spans into a document that
/// may already be gone. That is what makes cross-document paste the same code
/// path as same-document paste — and what will make `Pass 120.1`'s `to_bytes`
/// a serialisation problem rather than a design problem.
#[test]
fn a_clip_outlives_the_document_it_came_from() {
    let clip = {
        let doc = Document::from_bytes(pdf_with_font(MIXED, "Times-BoldItalic")).unwrap();
        let session = EditSession::new(doc);
        session.copy_objects(0, &[1, 2]).unwrap()
        // `session` and its `Document` are dropped here.
    };
    assert_eq!(clip.len(), 2);

    let destination = Document::from_bytes(pdf("0 0 5 5 re f")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    assert_eq!(outcome.objects_pasted, 2);
    let text = saved(&session);
    // ★ `Times-BoldItalic`, not Helvetica. The first draft asserted the source
    // font was `/Helvetica` -- which the DESTINATION fixture also uses, so the
    // test passed with the entire resource import disabled. A distinctive font
    // is the difference between exercising the mechanism and covering it.
    assert!(
        text.contains("/Times-BoldItalic"),
        "the clip's own font must have arrived from a document that is gone: {text}"
    );
}

/// Paint order survives the round trip.
///
/// Pasting a selection back in a different order restacks it — a filled shape
/// that was behind text arriving in front of it — which is a visible change
/// nobody asked for and which no error reports.
#[test]
fn paint_order_is_preserved() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0, 1, 2]).unwrap();
    let kinds: Vec<&str> = clip.items.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, vec!["path", "text", "image"]);
}

// ---------------------------------------------------------------------------
// Placement — one verb, four gestures
// ---------------------------------------------------------------------------

/// Paste-in-place puts the content back where it was; paste-with-offset moves
/// it by exactly the offset.
///
/// Asserted on the reported `bbox`, which is what a shell draws its paste
/// outline from — so a wrong answer here is wrong on screen before it is wrong
/// in the file.
#[test]
fn paste_in_place_and_with_offset_land_where_they_say() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let source_bbox = clip.bbox();

    let in_place = session.paste_preview(0, &clip, Matrix::IDENTITY).unwrap();
    assert!(
        close(in_place.bbox.min.x, source_bbox.min.x)
            && close(in_place.bbox.max.y, source_bbox.max.y),
        "paste-in-place must not move it: {:?} vs {source_bbox:?}",
        in_place.bbox
    );

    let offset = session
        .paste_preview(0, &clip, Matrix::translate(100.0, 50.0))
        .unwrap();
    assert!(
        close(offset.bbox.min.x, source_bbox.min.x + 100.0)
            && close(offset.bbox.min.y, source_bbox.min.y + 50.0),
        "paste-with-offset must move by exactly the offset: {:?}",
        offset.bbox
    );
}

/// A rotated paste's reported bounds map **all four corners**.
///
/// The naive two-corner version is right for a translation and a scale and
/// wrong for a rotation — and a paste outline that is wrong only when rotated
/// is the kind of bug that ships.
#[test]
fn a_rotated_paste_reports_bounds_that_enclose_the_rotation() {
    let doc = Document::from_bytes(pdf("0 0 100 10 re S")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let before = clip.bbox();
    let quarter = Matrix::rotate(std::f64::consts::FRAC_PI_2).about(Point::new(0.0, 0.0));
    let rotated = session.paste_preview(0, &clip, quarter).unwrap().bbox;

    let wide = before.max.x - before.min.x;
    let tall = rotated.max.y - rotated.min.y;
    assert!(
        close(tall, wide),
        "a quarter-turn makes the wide box tall: {rotated:?} from {before:?}"
    );
}

// ---------------------------------------------------------------------------
// The preview
// ---------------------------------------------------------------------------

/// ★ The preview commits nothing and answers exactly what the paste would.
///
/// Three cases, including two refusals, because agreement on the happy path is
/// what a second implementation would also manage.
#[test]
fn the_preview_answers_exactly_what_the_paste_would() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let good = session.copy_objects(0, &[0, 1]).unwrap();

    let mut future = good.clone();
    future.version = 999;

    let mut dangling = good.clone();
    dangling.objects.clear();

    for (label, clip) in [
        ("good", &good),
        ("from the future", &future),
        ("dangling", &dangling),
    ] {
        let previewed = session.paste_preview(0, clip, Matrix::IDENTITY);
        assert_eq!(session.undo_depth(), 0, "preview committed for {label}");
        let applied = session.paste_objects(0, clip, Matrix::IDENTITY);
        assert_eq!(
            previewed.is_ok(),
            applied.is_ok(),
            "preview and paste disagree for {label}: {previewed:?} vs {applied:?}"
        );
        if applied.is_ok() {
            session.undo();
        }
    }
}

/// A payload from a newer build is refused **by name**, not partially
/// understood.
#[test]
fn a_clip_from_a_newer_build_is_refused_by_name() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let mut clip = session.copy_objects(0, &[0]).unwrap();
    clip.version = pdfcer_core::vector::CLIP_VERSION + 1;

    let err = session
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect_err("a newer format is refused");
    let message = err.to_string();
    assert!(
        message.contains("newer build"),
        "the refusal must say which way the mismatch runs: {message}"
    );
    assert_eq!(session.undo_depth(), 0);
}

// ---------------------------------------------------------------------------
// Cut — Pass 120.3
// ---------------------------------------------------------------------------

/// ★ **Cut is ONE undo entry.**
///
/// The requester's own framing: *"otherwise Ctrl+X then Ctrl+Z gives the
/// operator their objects back but leaves the clipboard changed, or takes two
/// presses."* The copy half is `&self` and commits nothing, so only the
/// deletion reaches the undo stack.
#[test]
fn cut_is_one_undo_entry_and_returns_the_clip() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let clip = session.cut_objects(0, &[1]).expect("cut the text run");
    assert_eq!(clip.len(), 1);
    assert_eq!(
        session.undo_depth(),
        1,
        "one gesture, one undo -- not one for the copy and one for the delete"
    );

    session.undo().expect("the cut undoes");
    let (_bytes, report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    assert_eq!(
        report.objects_written, 0,
        "cut then undo leaves no trace in the save: {report:?}"
    );
}

/// ★ **A cut whose COPY half refuses deletes nothing.**
///
/// The order matters and is not incidental: copy first, delete second. Reversed,
/// a selection that cannot be copied would be gone with nothing on the
/// clipboard — the one outcome from which the operator cannot recover by
/// pasting.
#[test]
fn a_cut_that_cannot_copy_deletes_nothing() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let err = session
        .cut_objects(0, &[0, 99])
        .expect_err("99 is not on this page");
    assert!(err.to_string().contains("99"), "{err}");
    assert_eq!(
        session.undo_depth(),
        0,
        "nothing was deleted -- the copy refused first"
    );
}

// ---------------------------------------------------------------------------
// Session hygiene
// ---------------------------------------------------------------------------

/// Paste is one undoable command however many objects arrive, and undo leaves
/// the file byte-identical.
#[test]
fn paste_is_one_command_and_undoes_completely() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0, 1, 2]).unwrap();
    let outcome = session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    assert_eq!(outcome.objects_pasted, 3);
    assert_eq!(session.undo_depth(), 1, "three objects, one command");

    session.undo().expect("the paste undoes");
    let (_bytes, report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    assert_eq!(
        report.objects_written, 0,
        "a pasted-then-undone page must appear in no update section: {report:?}"
    );
}

/// Copying commits nothing — it is `&self`, and the undo stack proves it.
#[test]
fn copying_commits_nothing() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let _clip = session.copy_objects(0, &[0, 1, 2]).unwrap();
    assert_eq!(session.undo_depth(), 0);
    assert!(!session.is_modified());
}

/// An empty selection produces an empty clip, and pasting one is a no-op
/// rather than an error — a caller need not special-case it.
#[test]
fn an_empty_clip_pastes_as_a_no_op() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[]).unwrap();
    assert!(clip.is_empty());
    let outcome = session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    assert_eq!(outcome.objects_pasted, 0);
    assert_eq!(session.undo_depth(), 0, "a no-op paste is not a command");
}

/// Pasting the same clip twice yields two independent copies, each with its
/// own resource binding — a paste must not alias the previous one's names.
#[test]
fn pasting_twice_binds_two_independent_sets() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[1]).unwrap();
    let first = session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    let second = session
        .paste_objects(0, &clip, Matrix::translate(10.0, 0.0))
        .unwrap();
    assert_eq!(first.resources_added, second.resources_added);
    assert_eq!(session.undo_depth(), 2, "two gestures, two undo entries");

    let text = saved(&session);
    assert!(
        text.contains("pdfcePF0") && text.contains("pdfcePF1"),
        "the second paste must not reuse the first's binding: {text}"
    );
}

// ---------------------------------------------------------------------------
// Serialisation — Pass 120.1
// ---------------------------------------------------------------------------

/// ★ **A serialised clip round-trips, and the round trip is what makes
/// cross-session paste free rather than a second feature.**
///
/// The strong form: serialise, parse, and paste the PARSED clip into a
/// document whose `/F1` is a different font. If anything is lost on the way
/// through — the bindings, the CTM, the font object's payload — the paste
/// either refuses or renders wrong, and both are caught here rather than by an
/// operator six months later.
#[test]
fn a_serialised_clip_round_trips_and_still_pastes_correctly() {
    let source = Document::from_bytes(pdf_with_font(MIXED, "Times-BoldItalic")).unwrap();
    let session_a = EditSession::new(source);
    let clip = session_a.copy_objects(0, &[1, 2]).unwrap();

    let bytes = clip.to_bytes();
    let parsed = pdfcer_core::vector::ObjectClip::from_bytes(&bytes).expect("it parses back");
    assert_eq!(
        parsed, clip,
        "the round trip must be exact, not approximate"
    );

    let destination = Document::from_bytes(pdf_with_font("0 0 5 5 re f", "Courier")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session
        .paste_objects(0, &parsed, Matrix::IDENTITY)
        .expect("the parsed clip pastes");
    assert_eq!(outcome.objects_pasted, 2);
    let text = saved(&session);
    assert!(
        text.contains("/Times-BoldItalic"),
        "the font must have survived serialisation: {text}"
    );
}

/// The matrix survives **bit-exactly**.
///
/// Decimal round-tripping would change a CTM in the last place on every
/// copy/paste cycle, and a shape that drifts a little every time is a bug that
/// takes months to attribute. Asserted on an awkward value rather than on a
/// round one, because `1.0` round-trips through anything.
#[test]
fn a_matrix_survives_serialisation_bit_exactly() {
    let doc =
        Document::from_bytes(pdf("q 0.13333 0 0 0.13333 7.77 3.331 cm 0 0 10 10 re S Q")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let parsed =
        pdfcer_core::vector::ObjectClip::from_bytes(&clip.to_bytes()).expect("it parses back");
    let bits = |m: Matrix| [m.a, m.b, m.c, m.d, m.e, m.f].map(f64::to_bits);
    assert_eq!(
        bits(parsed.items[0].ctm),
        bits(clip.items[0].ctm),
        "the CTM must be bit-identical after a round trip"
    );
}

/// A payload that is not a clip is refused **by name**, before any length
/// prefix is read.
#[test]
fn a_foreign_payload_is_refused_by_name() {
    let err = pdfcer_core::vector::ObjectClip::from_bytes(b"this is not a clip at all")
        .expect_err("a foreign payload is refused");
    assert!(
        err.to_string().contains("not a pdfcer clipboard payload"),
        "{err}"
    );
}

/// A truncated payload is refused, not read past.
///
/// Swept over **every** prefix length rather than one, because a length-prefix
/// format has as many truncation points as it has fields and "it survived the
/// one I tried" is not the claim being made.
#[test]
fn every_truncation_is_refused_rather_than_read_past() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let bytes = session.copy_objects(0, &[0, 1, 2]).unwrap().to_bytes();

    for cut in 0..bytes.len() {
        let result = pdfcer_core::vector::ObjectClip::from_bytes(&bytes[..cut]);
        assert!(
            result.is_err(),
            "a payload truncated to {cut} of {} bytes must be refused",
            bytes.len()
        );
    }
    // And the whole thing still parses, so the sweep above is not passing
    // because the parser refuses everything.
    assert!(pdfcer_core::vector::ObjectClip::from_bytes(&bytes).is_ok());
}

/// A serialised clip from a newer build is refused **before** its body is
/// read.
#[test]
fn a_serialised_clip_from_a_newer_build_is_refused() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let mut clip = session.copy_objects(0, &[0]).unwrap();
    clip.version = pdfcer_core::vector::CLIP_VERSION + 1;
    let err = pdfcer_core::vector::ObjectClip::from_bytes(&clip.to_bytes())
        .expect_err("a newer format is refused");
    assert!(err.to_string().contains("newer build"), "{err}");
}

// ---------------------------------------------------------------------------
// Interchange — Pass 120.2
// ---------------------------------------------------------------------------

/// ★ **A clip exports as a standalone one-page PDF that pdfcer itself can
/// reopen, and whose page IS the selection.**
///
/// Reopening it with pdfcer is the strongest available check that the file is
/// well-formed — a hand-emitted xref table with one wrong offset produces a
/// file that looks fine in a hex dump and opens in nothing.
#[test]
fn a_clip_exports_as_a_standalone_one_page_pdf() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0, 1, 2]).unwrap();
    let exported = clip.to_pdf();

    assert_eq!(exported.objects, 3);
    assert!(!exported.size_substituted, "the selection has real extent");

    let reopened = Document::from_bytes(exported.bytes).expect("pdfcer can reopen its own export");
    let pages = pdfcer_core::page_tree::pages(&reopened).expect("it has a page tree");
    assert_eq!(pages.len(), 1, "one selection, one page");

    // The page IS the selection: its MediaBox matches the clip's bounds, so a
    // consumer that places the file gets the objects and no whitespace.
    let media = pages[0].media_box;
    let source = clip.bbox();
    assert!(
        close(media.urx - media.llx, source.max.x - source.min.x)
            && close(media.ury - media.lly, source.max.y - source.min.y),
        "the page must be the selection's own size: {media:?} vs {source:?}"
    );
}

/// The exported PDF still carries the clip's own font — the export is not a
/// picture of the selection, it is the selection.
#[test]
fn an_exported_pdf_carries_the_clips_resources() {
    let doc = Document::from_bytes(pdf_with_font(MIXED, "Times-BoldItalic")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[1]).unwrap();
    let exported = clip.to_pdf();
    let text = String::from_utf8_lossy(&exported.bytes);
    assert!(
        text.contains("/Times-BoldItalic"),
        "the font must travel with the export: {text}"
    );
}

/// The exported text extracts, which is what an interchange consumer will
/// actually do with it.
#[test]
fn exported_text_is_still_text() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[1]).unwrap();
    let reopened = Document::from_bytes(clip.to_pdf().bytes).expect("it reopens");
    let text = pdfcer_core::text_extract::extract_document(
        &reopened,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .expect("extraction runs");
    let all: String = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .map(|r| r.text.as_str())
        .collect();
    assert!(
        all.contains("hi"),
        "the copied run must still be text: {all:?}"
    );
}

/// ★ **A degenerate selection gets a minimum page size, and SAYS SO.**
///
/// A zero-area `/MediaBox` is not merely ugly — a reader given one shows an
/// empty window or refuses the file, and an operator who copied a zero-height
/// rule and got back a document that will not open has no way to tell which
/// step failed. So the substitution happens and is disclosed.
#[test]
fn a_degenerate_selection_gets_a_minimum_page_and_discloses_it() {
    // A horizontal rule: real width, zero height.
    let doc = Document::from_bytes(pdf("10 10 m 110 10 l S")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let exported = clip.to_pdf();
    assert!(
        exported.size_substituted,
        "a zero-height selection must report the substitution: {exported:?}"
    );
    assert!(exported.size.1 > 0.0, "and the page must have real extent");
    assert!(
        Document::from_bytes(exported.bytes).is_ok(),
        "the substituted page must still open"
    );
}

/// Two exports of the same clip are **byte-identical** — no clock is
/// involved, so a shell can cache one.
#[test]
fn exporting_twice_is_byte_identical() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0, 1]).unwrap();
    assert_eq!(clip.to_pdf().bytes, clip.to_pdf().bytes);
}

/// ★ The export and the private format are **not** interchangeable, and the
/// asymmetry is the reason both exist.
///
/// `from_bytes` refuses a PDF by name rather than half-parsing it. Pinned so
/// that a future "simplify these two into one" cannot pass quietly.
#[test]
fn the_pdf_export_is_not_a_clip_payload_and_is_refused_as_one() {
    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let err = pdfcer_core::vector::ObjectClip::from_bytes(&clip.to_pdf().bytes)
        .expect_err("a PDF is not a clip payload");
    assert!(
        err.to_string().contains("not a pdfcer clipboard payload"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// ★★ The prelude — inherited graphics state (Pass 120.2's real-file finding)
// ---------------------------------------------------------------------------

/// ★★ **A text object whose `Tf` is OUTSIDE its own `BT`…`ET` still pastes
/// with a font.**
///
/// Text state is graphics state (§8.4.1 Table 52), so a producer may set
/// `/F1 12 Tf` **once** and emit many `BT`…`ET` blocks that inherit it. A text
/// object's byte span is its `BT`…`ET`, so **the `Tf` is not in it** — and the
/// first cut of this Pass therefore recorded no font binding and pasted
/// content that showed text with no font selected at all.
///
/// **Found on the operator's real CAD drawing, not here.** pdfcer's own
/// extractor said it about the export: *"a show operator appeared with no font
/// selected (§9.4.1 requires Tf first)"*, `chars=0 codes=4 failed=4`. Nothing
/// errored at copy or at paste. The fixture below is that file in miniature.
#[test]
fn a_text_object_that_inherits_its_font_still_carries_one() {
    // `Tf` BEFORE `BT` — legal, common, and what a CAD exporter writes.
    let doc = Document::from_bytes(pdf_with_font(
        "/F1 12 Tf\nBT 20 20 Td (hi) Tj ET",
        "Times-BoldItalic",
    ))
    .unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).expect("copy the text");

    assert_eq!(
        clip.resource_count(),
        1,
        "the inherited font must travel with the clip: {clip:?}"
    );
    assert!(
        !clip.items[0].prelude.is_empty(),
        "the inherited state must be recorded as a prelude"
    );

    // The export is the strongest check available: pdfcer's own extractor is
    // the thing that diagnosed the original defect, so asking it again is
    // asking the same question that failed.
    let reopened = Document::from_bytes(clip.to_pdf().bytes).expect("the export reopens");
    let text = pdfcer_core::text_extract::extract_document(
        &reopened,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .expect("extraction runs");
    let all: String = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .map(|r| r.text.as_str())
        .collect();
    assert!(
        all.contains("hi"),
        "the exported text must be readable, not fontless: {all:?}"
    );
}

/// The same fix on the PASTE path, not only the export path.
///
/// Pinned separately because the two emit their content in different
/// functions, and "it works in the export" is exactly the reasoning that would
/// let one of a pair ship without it.
#[test]
fn an_inherited_font_survives_a_paste_too() {
    let source = Document::from_bytes(pdf_with_font(
        "/F1 12 Tf\nBT 20 20 Td (hi) Tj ET",
        "Times-BoldItalic",
    ))
    .unwrap();
    let session_a = EditSession::new(source);
    let clip = session_a.copy_objects(0, &[0]).unwrap();

    let destination = Document::from_bytes(pdf_with_font("0 0 5 5 re f", "Courier")).unwrap();
    let mut session = EditSession::new(destination);
    session.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    let text = saved(&session);
    assert!(
        text.contains("/Times-BoldItalic"),
        "the inherited font must arrive: {text}"
    );
    assert!(
        text.contains(" Tf"),
        "and a Tf must be emitted for it: {text}"
    );
}

/// A path stroked under an inherited colour and line width carries them too —
/// the same class as the font, one object kind over.
///
/// Without this a copied line pastes **black and hairline** regardless of what
/// the source drew, which is the failure mode that looks like a rendering bug
/// rather than like a clipboard bug.
#[test]
fn a_path_carries_the_colour_and_width_it_inherited() {
    let doc = Document::from_bytes(pdf("0.9 0.1 0.1 RG 4 w\n10 10 m 110 10 l S")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let prelude = String::from_utf8_lossy(&clip.items[0].prelude).into_owned();
    assert!(
        prelude.contains("RG"),
        "the stroke colour must be re-established: {prelude:?}"
    );
    assert!(
        prelude.contains(" w"),
        "and so must the line width: {prelude:?}"
    );
}

/// An object that sets its own state gets **no** prelude for it — the prelude
/// re-establishes what was inherited, never what the object already says.
///
/// Without this the paste would double-set, which is harmless but noisy, and
/// the noise is what a reader of the output would have to rule out first.
#[test]
fn an_object_that_sets_its_own_state_gets_no_prelude_for_it() {
    let doc = Document::from_bytes(pdf("BT /F1 12 Tf 20 20 Td (hi) Tj ET")).unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let prelude = String::from_utf8_lossy(&clip.items[0].prelude).into_owned();
    assert!(
        !prelude.contains("Tf"),
        "the object's own Tf is in its bytes; the prelude must not repeat it: {prelude:?}"
    );
}

/// The prelude survives serialisation — it is part of the payload, not a
/// derived value the parse side could recompute.
#[test]
fn the_prelude_round_trips_through_serialisation() {
    let doc = Document::from_bytes(pdf_with_font(
        "/F1 12 Tf\nBT 20 20 Td (hi) Tj ET",
        "Times-BoldItalic",
    ))
    .unwrap();
    let session = EditSession::new(doc);
    let clip = session.copy_objects(0, &[0]).unwrap();
    let parsed =
        pdfcer_core::vector::ObjectClip::from_bytes(&clip.to_bytes()).expect("it parses back");
    assert_eq!(parsed.items[0].prelude, clip.items[0].prelude);
    assert_eq!(parsed, clip);
}

// ---------------------------------------------------------------------------
// Annotations on the clipboard — Pass 120.4
// ---------------------------------------------------------------------------

/// ★ **A markup annotation copies and pastes, and it round-trips through
/// pdfcer's OWN MODEL rather than through the object graph.**
///
/// A raw dictionary copy would be structurally right and semantically wrong:
/// the appearance stream would arrive as-is rather than being re-baked for the
/// destination. Going through `MarkupSpec` means `add_markup` authors it in
/// the destination exactly as if the operator had drawn it there.
#[test]
fn a_markup_annotation_copies_and_pastes() {
    use pdfcer_core::annot_author::{Color, MarkupSpec};
    use pdfcer_core::page_tree::Rect;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut source = EditSession::new(doc);
    source
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 60.0,
                    ury: 40.0,
                },
                border: Some(Color::Rgb(1.0, 0.0, 0.0)),
                interior: None,
                border_width: 2.0,
                border_effect: None,
            },
        )
        .expect("author a square");

    let clip = source.copy_annotations(0, &[0]).expect("copy it");
    assert_eq!(clip.annotation_count(), 1);
    assert_eq!(clip.annotations[0].label(), "markup");

    let destination = Document::from_bytes(pdf("0 0 5 5 re f")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session
        .paste_objects(0, &clip, Matrix::translate(100.0, 0.0))
        .expect("paste it");
    assert_eq!(outcome.annotations_pasted, 1);

    // It arrived MOVED: the source rect started at x=10, so a 100-point
    // offset must put it at 110.
    let reopened = Document::from_bytes(saved(&session).into_bytes()).expect("reopens");
    let pages = pdfcer_core::page_tree::pages(&reopened).unwrap();
    let annots = pdfcer_core::annot::page_annotations(&reopened, pages[0].id);
    let square = annots
        .iter()
        .find(|a| a.subtype == b"Square")
        .expect("the pasted square is there");
    let rect = square.rect.expect("it has a /Rect");
    assert!(
        close(rect.llx, 110.0),
        "the paste offset must move the annotation: {rect:?}"
    );
}

/// ★★ **A ce dimension keeps its GROUP and its scale across a paste into
/// another document.**
///
/// This is the reason a raw graph copy was never going to be enough. A ce
/// dimension's measurement comes from its group's scale and unit, which live
/// in a `/PieceInfo` sidecar — not in the annotation. Copying the dictionary
/// alone would paste an outline whose printed number means nothing in the
/// destination.
///
/// The group is matched **by name**, because a `GroupId` means nothing in
/// another document.
#[test]
fn a_ce_dimension_carries_its_group_across_documents() {
    use pdfcer_core::dimension::{DimensionKind, Unit};
    use pdfcer_core::vector::AxisConstraint;
    use pdfcer_core::vector::Point as VPoint;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut source = EditSession::new(doc);
    let group = source
        .add_dimension_group("Site plan", Unit::Millimeter)
        .expect("a group");
    source
        .add_dimension(
            0,
            group,
            DimensionKind::Linear {
                a: VPoint::new(10.0, 10.0),
                b: VPoint::new(110.0, 10.0),
                constraint: AxisConstraint::Horizontal,
                offset: 12.0,
                text_along: 0.5,
            },
        )
        .expect("author a ce dimension");

    let copied = source.copy_annotations(0, &[0]).expect("copy it");
    assert_eq!(copied.annotation_count(), 1);
    assert_eq!(
        copied.annotations[0].label(),
        "ce dimension",
        "a ce dimension must NOT be classified as plain markup -- it is a /Line by subtype, which is exactly the R204 trap"
    );

    let destination = Document::from_bytes(pdf("0 0 5 5 re f")).unwrap();
    let mut session = EditSession::new(destination);
    let outcome = session
        .paste_objects(0, &copied, Matrix::IDENTITY)
        .expect("paste it");
    assert_eq!(outcome.annotations_pasted, 1);

    // The GROUP arrived, by name and unit -- which is what makes the pasted
    // dimension measure the same thing it measured at home.
    let model = session.dimension_model();
    let landed = model
        .groups()
        .iter()
        .find(|g| g.name == "Site plan")
        .expect("the group was recreated in the destination");
    assert_eq!(landed.unit(), Unit::Millimeter);
    assert_eq!(model.dimensions().len(), 1);
}

/// An unsupported annotation kind is **refused by name, with the reason**, and
/// is not counted as pasted.
///
/// ★ Note what changed for this to be possible at all: the original
/// acceptance criteria said "refuse loudly", written before `120.0` shipped —
/// and once copy addressed content objects by paint-order index, there was no
/// index by which those verbs could even *name* an annotation to refuse it.
/// This address space is what gives the refusal somewhere to live.
#[test]
fn an_unsupported_annotation_is_refused_by_name_and_not_counted() {
    use pdfcer_core::vector::ClipAnnotation;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let mut clip = session.copy_objects(0, &[]).unwrap();
    clip.annotations.push(ClipAnnotation::Unsupported {
        subtype: "Widget".to_owned(),
    });
    assert_eq!(clip.annotation_count(), 1);

    let outcome = session
        .paste_objects(0, &clip, Matrix::IDENTITY)
        .expect("the paste succeeds; the widget is skipped");
    assert_eq!(
        outcome.annotations_pasted, 0,
        "an unpasted annotation must not be counted as pasted"
    );
    let disclosed = outcome.disclosures.join(" ");
    assert!(
        disclosed.contains("Widget") && disclosed.contains("AcroForm"),
        "the refusal must name the kind AND the reason: {disclosed}"
    );
}

/// A rotated `Square` **encloses** rather than refusing, and says so.
///
/// `/Rect` is axis-aligned by definition (§12.5.2), so a rotated rectangle has
/// no spelling — the same shape as `re` in `Pass 113.0`. Enclosing is the only
/// thing the format admits, so this is not pdfcer choosing between two
/// renderings; it is pdfcer doing the one available and disclosing it.
#[test]
fn a_rotated_square_annotation_encloses_and_discloses() {
    use pdfcer_core::annot_author::{Color, MarkupSpec};
    use pdfcer_core::page_tree::Rect;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    session
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 0.0,
                    lly: 0.0,
                    urx: 100.0,
                    ury: 10.0,
                },
                border: Some(Color::Rgb(0.0, 0.0, 1.0)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .unwrap();
    let clip = session.copy_annotations(0, &[0]).unwrap();

    let quarter = Matrix::rotate(std::f64::consts::FRAC_PI_2).about(Point::new(0.0, 0.0));
    let outcome = session.paste_objects(0, &clip, quarter).expect("it pastes");
    assert_eq!(outcome.annotations_pasted, 1, "it is placed, not refused");
    let disclosed = outcome.disclosures.join(" ");
    assert!(
        disclosed.contains("cannot express a rotation"),
        "the compromise must be disclosed: {disclosed}"
    );
}

/// ★ **`to_bytes` CARRIES annotations as of `Pass 169.0`**, and the clip
/// still says so.
///
/// This test used to pin the opposite, and the old wording is worth keeping
/// legible rather than silently rewritten. It read:
///
/// > ★ **`to_bytes` does not carry annotations, and the clip SAYS SO**
/// > rather than letting a caller discover it from a count that silently
/// > drops. A shell writing a clip to disk can warn, or keep the in-process
/// > copy.
///
/// That was the right test for a format that dropped them, and the
/// consequence was larger than the wording admitted: `pdfcer` could never
/// paste an annotation of any kind, because the CLI only ever has the file.
///
/// The format carries them now (version 2), so
/// `annotations_survive_serialisation` answers `true` for every clip. The
/// method is kept rather than deleted — it is public, a shell may be
/// branching on it, and a caller that still checks simply always takes the
/// survives branch.
#[test]
fn a_clip_says_whether_serialisation_would_lose_anything() {
    use pdfcer_core::annot_author::{Color, MarkupSpec};
    use pdfcer_core::page_tree::Rect;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    let content_only = session.copy_objects(0, &[0]).unwrap();
    assert!(
        content_only.annotations_survive_serialisation(),
        "a content-only clip serialises completely"
    );

    let spec = MarkupSpec::Square {
        rect: Rect {
            llx: 1.0,
            lly: 1.0,
            urx: 2.0,
            ury: 2.0,
        },
        border: Some(Color::Rgb(0.0, 0.0, 0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    };
    session.add_markup(0, &spec).unwrap();
    let with_annots = session.copy_annotations(0, &[0]).unwrap();
    assert!(
        with_annots.annotations_survive_serialisation(),
        "and so does one holding annotations, since Pass 169.0"
    );
    let parsed =
        pdfcer_core::vector::ObjectClip::from_bytes(&with_annots.to_bytes()).expect("it parses");
    assert_eq!(
        parsed.annotation_count(),
        1,
        "the annotation came through the wire, not merely a promise that it would"
    );
    assert_eq!(
        parsed.annotations, with_annots.annotations,
        "and it came through UNCHANGED -- the clip carries each annotation as \
         the COS object pdfcer already has a codec for, so the round trip is \
         exact rather than approximate"
    );
}

/// `copy_selection` takes both address spaces in one call — the verb a shell
/// calls when a marquee caught content and a comment, which on a marked-up
/// drawing is the ordinary case.
#[test]
fn copy_selection_takes_both_address_spaces_at_once() {
    use pdfcer_core::annot_author::{Color, MarkupSpec};
    use pdfcer_core::page_tree::Rect;

    let doc = Document::from_bytes(pdf(MIXED)).unwrap();
    let mut session = EditSession::new(doc);
    session
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 1.0,
                    lly: 1.0,
                    urx: 2.0,
                    ury: 2.0,
                },
                border: Some(Color::Rgb(0.0, 0.0, 0.0)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .unwrap();

    let clip = session
        .copy_selection(0, &[0, 1], &[0])
        .expect("both at once");
    assert_eq!(clip.len(), 2, "two content objects");
    assert_eq!(clip.annotation_count(), 1, "and one annotation");
    // The bbox spans both address spaces, which is what a paste outline needs.
    assert!(clip.bbox().min.x <= 1.0);
}
