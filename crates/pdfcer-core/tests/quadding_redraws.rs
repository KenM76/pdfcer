//! **`/Q` is written AND the appearance is rebuilt** (`Pass 308.4`, request
//! `G022`).
//!
//! ## The defect
//!
//! `FieldEdit::quadding` had been settable since `43192792`. `edit_field`
//! validated it, wrote `/Q` to the dictionary, and then left it out of
//! `layout_changed` — the gate that decides whether the baked `/AP` is
//! rebuilt. So a justification change landed in the dictionary and the
//! appearance kept drawing the old alignment until something unrelated forced
//! a rebuild.
//!
//! Justification is drawn **into** the stream (`vartext::align_x` places each
//! line by its AFM width) and pdfcer paints the baked `/AP` rather than
//! re-deriving alignment at view time. So writing the key alone changes a
//! number and no pixels.
//!
//! ## ★ Why every existing test missed it, which is the reusable part
//!
//! The capability was built and tested. `vartext`'s own
//! `quadding_places_lines_by_afm_width` drives all three arms **through the
//! builder**, and `edit_field`'s tests assert **the dictionary**. `/Q` is in
//! the dictionary, so the dictionary assertions passed; the builder works, so
//! the builder assertions passed. **Nothing tested the DISPATCH between
//! them**, and that is exactly where the defect lived.
//!
//! ⇒ *A property tested at both ends is not a property tested end to end.*
//! These tests drive `edit_field` and read the **stream**.
//!
//! ## ★★ And a relative assertion could not see the subtler half either
//!
//! `the_redraw_uses_the_quadding_being_written_not_the_snapshots` was first
//! written as *"the second edit sits further right than the first"*. It
//! **passed against a deliberately broken build**: with the stale snapshot
//! both edits were one step behind, and the ORDER survived intact. A relative
//! assertion cannot tell *"each redraw is one edit behind"* from *"each
//! redraw is correct"*.
//!
//! ⇒ *An off-by-one that shifts every sample preserves every comparison
//! between samples.* Where the arithmetic is available — a pinned font size
//! and a known box — assert the number. The sabotage run is what found this;
//! the test had looked entirely reasonable.
//!
//! ## ★★ The half a one-line fix gets wrong, twice over
//!
//! Adding `|| edit.quadding.is_some()` to the gate is not sufficient, and the
//! failure mode is that it looks fixed. `regen_field_appearance` reads
//! `field.quadding` from a snapshot taken **before** this command staged its
//! writes, so the redraw would re-bake the OLD `/Q` while the dictionary
//! carried the new one — a rebuild that changes no pixels and reports
//! `appearance_regenerated: true` while doing it. The requester found that;
//! it is the third instance of the pattern `/Rect` (`Pass 187.0`) and `/DA`
//! already carry ★ comments about.
//!
//! The second trap is one nobody flagged: **`/Q` is inheritable**
//! (§12.7.3.2, resolved own → ancestors → `/AcroForm` → 0), so CLEARING it
//! does not mean *left*, it means *inherit again*. Repairing the snapshot with
//! `Quadding::default()` would left-align a field sitting under a parent that
//! says centred — reintroducing the very defect on the branch that looks too
//! simple to get wrong. `inheriting_a_parents_quadding_is_not_left` is that
//! case.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, FieldEdit, NewTextField};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Name, Object};
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

/// A wide box, so left/centre/right land at visibly different offsets.
fn rect() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 220.0,
        ury: 124.0,
    }
}

fn field_named(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .expect("the field")
}

/// The widget's `/AP` `/N` stream as text.
fn ap_text(s: &EditSession, name: &str) -> String {
    let g = s.graph();
    let field = field_named(s, name);
    let dict = g
        .resolved(field.widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return String::new();
    };
    let Some(Object::Stream(st)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return String::new();
    };
    String::from_utf8_lossy(s.view().slice(st.data_span).unwrap_or_default()).into_owned()
}

/// The `x` of the text matrix the generator emits — `1 0 0 1 <x> <y> Tm`.
///
/// Reading the OFFSET rather than looking for a fixed string is what makes
/// these assertions about justification rather than about one box size: the
/// three arms differ only in this number.
fn text_x(stream: &str) -> f64 {
    for line in stream.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 7 && parts[6] == "Tm" && parts[0] == "1" {
            return parts[4].parse().expect("a number");
        }
    }
    panic!("no `1 0 0 1 x y Tm` in:\n{stream}");
}

fn text_field(s: &mut EditSession, name: &str, value: &str) {
    s.add_text_field(
        &NewTextField::new(0, name, rect())
            .declining_tooltip()
            .with_value(value),
    )
    .unwrap();
}

/// A one-page form whose field tree is `Group` (`/Q 2`) → `Name` (`/Q 0`).
///
/// Hand-built rather than authored through `add_text_field`, because nothing
/// on `EditSession` writes `/Q` to a GROUPING node — `edit_field` resolves a
/// terminal — and the inheritance branch cannot be reached without one. The
/// kid carries its own `/Q 0`, so the test can watch what happens when that
/// is removed and the parent's `/Q 2` takes over.
///
/// No `/AP`: the field is unfilled and the first `edit_field` bakes one
/// through the same §12.7.3.3 regenerator a fill uses (R49), which is the
/// path under test.
fn inherited_q_pdf() -> Vec<u8> {
    let objects: Vec<(u32, String)> = vec![
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm 7 0 R >>".to_owned(),
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [6 0 R] >>".to_owned(),
        ),
        // The grouping node: a partial name, kids, and the `/Q` to inherit.
        (
            4,
            "<< /T (Group) /Q 2 /FT /Tx /Kids [5 0 R] >>".to_owned(),
        ),
        // The terminal field, stating its own `/Q 0` to begin with.
        (
            5,
            "<< /T (Name) /Parent 4 0 R /FT /Tx /Q 0 /V (AV) /Kids [6 0 R] >>".to_owned(),
        ),
        // Its widget.
        (
            6,
            "<< /Type /Annot /Subtype /Widget /Parent 5 0 R /Rect [20 100 220 124] /F 4 /P 3 0 R >>"
                .to_owned(),
        ),
        (
            7,
            "<< /Fields [4 0 R] /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 8 0 R >> >> >>"
                .to_owned(),
        ),
        (
            8,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (num, body) in &objects {
        offsets[*num as usize] = out.len();
        out.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let startxref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

// -------------------------------------------------------------------------
// The defect: a justification change reaches the pixels
// -------------------------------------------------------------------------

#[test]
fn setting_the_quadding_moves_the_text() {
    let mut s = session();
    text_field(&mut s, "Name", "AV");
    let left_x = text_x(&ap_text(&s, "Name"));

    let out = s
        .edit_field("Name", &FieldEdit::new().with_quadding(2))
        .unwrap();
    assert!(
        out.appearance_regenerated,
        "a /Q change must rebuild the baked /AP, not only write the key"
    );

    let right_x = text_x(&ap_text(&s, "Name"));
    assert!(
        right_x > left_x,
        "right-justified text starts further across the box than \
         left-justified: left={left_x}, right={right_x}"
    );

    // And the dictionary agrees, so this is not a redraw that forgot to record.
    assert_eq!(field_named(&s, "Name").quadding.code(), 2);
}

/// The generator's own left inset, `vartext::TEXT_PAD`.
///
/// The one number these tests hard-code, and it is safe to: it is a constant
/// of the generator, not a measurement of a string.
const TEXT_PAD: f64 = 2.0;

/// The x-offsets the three `/Q` values produce for one field, in order.
///
/// Each arm starts from a FRESH copy of the fixture, whose `/Q` is `0` — so
/// arms 1 and 2 also prove the staged value beat the snapshot, rather than
/// inheriting the previous edit's result.
fn offsets_for_each_quadding() -> [f64; 3] {
    let mut xs = [0.0; 3];
    for (i, q) in [0, 1, 2].into_iter().enumerate() {
        let mut s = EditSession::new(Document::from_bytes(inherited_q_pdf()).unwrap());
        s.edit_field("Group.Name", &FieldEdit::new().with_quadding(q))
            .unwrap();
        xs[i] = text_x(&ap_text(&s, "Group.Name"));
    }
    xs
}

#[test]
fn the_redraw_uses_the_quadding_being_written_not_the_snapshots() {
    // ★ THE TRAP THE ONE-LINE FIX FALLS INTO. `regen_field_appearance` reads
    // `field.quadding` from a snapshot taken before this command staged its
    // writes. Gating on `edit.quadding` without repairing that snapshot
    // re-bakes the OLD justification and reports success — a rebuild that
    // changes no pixels, with a green flag in front of it.
    //
    // The fixture states `/Q 0`, so a stale redraw lands at exactly
    // `TEXT_PAD`. Asserting against that constant is what makes this test
    // absolute rather than relative — see the module header for why the first
    // version of it, which only compared two edits, passed on a broken build.
    let mut s = EditSession::new(Document::from_bytes(inherited_q_pdf()).unwrap());
    s.edit_field("Group.Name", &FieldEdit::new().with_quadding(2))
        .unwrap();

    let x = text_x(&ap_text(&s, "Group.Name"));
    assert!(
        (x - TEXT_PAD).abs() > 1e-6,
        "the stream is still left-justified at TEXT_PAD ({TEXT_PAD}), which is \
         where a redraw from the STALE snapshot's /Q 0 puts it"
    );
}

#[test]
fn the_three_offsets_satisfy_the_geometry_and_not_merely_the_order() {
    // ★★ WIDTH-FREE AND STILL ABSOLUTE, which is the trick worth keeping.
    //
    // For a string of width `w` in a box of width `W`, the generator places
    // left at `PAD`, right at `W - PAD - w`, centre at `(W - w)/2`. Subtract
    // `PAD` from each and the centre offset is EXACTLY half the right one,
    // whatever `w` is. So the relation pins all three absolutely without the
    // test needing to know the string's width — which is the measurement it
    // has no business duplicating from the generator.
    //
    // A redraw one edit behind cannot satisfy it: from a `/Q 0` fixture every
    // arm lands at `PAD`, and the strict ordering below fails first.
    let xs = offsets_for_each_quadding();

    assert!(
        (xs[0] - TEXT_PAD).abs() < 1e-6,
        "left-justified sits at the generator's own pad: {xs:?}"
    );
    assert!(
        xs[0] < xs[1] && xs[1] < xs[2],
        "left < centre < right, strictly — all three equal is what a stale \
         snapshot produces: {xs:?}"
    );
    let centre = xs[1] - TEXT_PAD;
    let right = xs[2] - TEXT_PAD;
    assert!(
        (centre - right / 2.0).abs() < 1e-6,
        "centre must be exactly half of right, measured from the pad: \
         centre={centre}, right/2={}, from {xs:?}",
        right / 2.0
    );
}

// -------------------------------------------------------------------------
// ★ Clearing means INHERIT, which is not the same as left
// -------------------------------------------------------------------------

#[test]
fn clearing_the_quadding_with_nothing_above_it_gives_left() {
    let mut s = session();
    text_field(&mut s, "Name", "AV");
    let left_x = text_x(&ap_text(&s, "Name"));

    s.edit_field("Name", &FieldEdit::new().with_quadding(2))
        .unwrap();
    assert!(text_x(&ap_text(&s, "Name")) > left_x);

    let out = s
        .edit_field("Name", &FieldEdit::new().clearing_quadding())
        .unwrap();
    assert!(out.appearance_regenerated, "clearing redraws too");
    assert!(
        (text_x(&ap_text(&s, "Name")) - left_x).abs() < 1e-9,
        "with no ancestor stating one, Table 222's default of left applies"
    );
}

#[test]
fn inheriting_a_parents_quadding_is_not_left() {
    // ★★ THE SECOND TRAP, AND NOBODY FLAGGED IT. `/Q` is inheritable
    // (§12.7.3.2). A field under a parent carrying `/Q 2` that CLEARS its own
    // `/Q` must come back RIGHT-justified. Resolving a removal to
    // `Quadding::default()` would left-align it — the same "dictionary says
    // one thing, stream draws another" defect this Pass exists to close,
    // reintroduced on the branch that looks too simple to get wrong.
    let mut s = EditSession::new(Document::from_bytes(inherited_q_pdf()).unwrap());

    // The child states `/Q 0` explicitly, so it wins over the parent's 2.
    s.edit_field("Group.Name", &FieldEdit::new().with_quadding(0))
        .unwrap();
    let left_x = text_x(&ap_text(&s, "Group.Name"));

    // Now REMOVE the child's own key. It inherits the parent's `/Q 2`.
    let out = s
        .edit_field("Group.Name", &FieldEdit::new().clearing_quadding())
        .unwrap();
    assert!(out.appearance_regenerated, "clearing redraws");
    let inherited_x = text_x(&ap_text(&s, "Group.Name"));

    assert!(
        inherited_x > left_x,
        "clearing means INHERIT, not left: the parent says right-justified, \
         so the stream must be right-justified. left={left_x}, \
         inherited={inherited_x}"
    );
    assert_eq!(
        field_named(&s, "Group.Name").quadding.code(),
        2,
        "and the read model resolves the same inheritance, so the two agree"
    );
}

// -------------------------------------------------------------------------
// The half that must not change
// -------------------------------------------------------------------------

#[test]
fn an_edit_that_does_not_mention_quadding_leaves_the_stream_alone() {
    let mut s = session();
    text_field(&mut s, "Name", "AV");
    s.edit_field("Name", &FieldEdit::new().with_quadding(2))
        .unwrap();
    let before = ap_text(&s, "Name");

    // A pure tooltip change: nothing about the layout moved.
    let out = s
        .edit_field(
            "Name",
            &FieldEdit::new().with_tooltip(pdfcer_core::edit::TooltipChoice::Text(
                "Your name".to_owned(),
            )),
        )
        .unwrap();
    assert!(!out.appearance_regenerated);
    assert_eq!(ap_text(&s, "Name"), before);
}

#[test]
fn a_quadding_outside_the_three_is_refused_before_anything_is_written() {
    let mut s = session();
    text_field(&mut s, "Name", "AV");
    let before = ap_text(&s, "Name");

    let err = s
        .edit_field("Name", &FieldEdit::new().with_quadding(7))
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("12.7.3.3"), "the corrected citation: {msg}");
    assert!(
        msg.contains("Table 222"),
        "Table 233 is the signature-field /Lock table, not this one: {msg}"
    );
    assert_eq!(ap_text(&s, "Name"), before, "nothing was changed");
}

#[test]
fn a_dictionary_only_assertion_cannot_see_this_bug() {
    // Kept as an explicit note rather than a comment: `/Q` was ALWAYS written
    // correctly, which is why the dictionary tests stayed green through the
    // whole life of the defect. This asserts the dictionary — and passes on
    // the broken build. It is here so a future reader does not add one of
    // these and believe the property is covered.
    let mut s = session();
    text_field(&mut s, "Name", "AV");
    s.edit_field("Name", &FieldEdit::new().with_quadding(1))
        .unwrap();

    let g = s.graph();
    let id = field_named(&s, "Name").id;
    let q = g
        .resolved(id)
        .as_dict()
        .and_then(|d| d.get(b"Q"))
        .map(|o| g.resolve(o).clone());
    assert_eq!(q, Some(Object::Integer(1)));
    let _ = Name::from(b"Q");
}
