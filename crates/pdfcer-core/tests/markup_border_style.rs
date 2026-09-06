//! # `Pass 258.0` — the border LINE STYLE: preserving, authoring and
//! clearing a dashed markup border, and refusing the properties a subtype
//! does not have
//!
//! Three `pdfcer-gui` requests of 2026-09-06 land in one Pass because they
//! are one seam. Each is tested below under its own heading.
//!
//! ## 1. The dash was being DESTROYED, not merely unauthorable
//!
//! The request that matters most did not ask for a feature. It reported a
//! loss of the operator's existing work:
//!
//! > ★ *"A dashed mark that already exists in the operator's file is
//! > silently converted to a solid one the first time he changes its
//! > colour."*
//!
//! The mechanism was not a bug in any one function. `set_markup_style`
//! regenerates `/AP` from `annot_author::spec_from_dict`; the spec had no
//! dash field; so the rebuilt appearance was solid. `/BS /D` survived in the
//! dictionary, R43 means pdfcer paints from `/AP`, and the file's two halves
//! then disagreed with the appearance winning.
//!
//! It was **disclosed** — `DroppedProperty::DashPattern` fired — and that is
//! the floor, not the ceiling. The requester put the distinction better than
//! a rule could: *"an operator who wanted a red dashed cloud and pressed the
//! colour swatch has been given a red solid one, and 'we told you' is a poor
//! second to 'we kept it'."*
//!
//! The fix follows `Pass 98.0`'s `/BE` precedent exactly — read the property
//! back on the way IN, so a regeneration re-authors it — rather than growing
//! `MarkupStyle` a field and calling the loss disclosed.
//!
//! ### ★ FOUR routes regenerate an appearance, not one
//!
//! `set_markup_style` is the one the request named. It is not the only one
//! that would have solidified a dash: `resize_annotation`,
//! `reshape_annotation` and (for authoring) `add_markup_inner` all bake
//! through the same generator. Fixing only the reported route is a failure
//! this project has hit repeatedly — the shell then discovers the asymmetry
//! by dragging a vertex instead of pressing a swatch. All four are covered
//! here.
//!
//! ## 2. `endings` could be SET but never REMOVED
//!
//! `MarkupStyle::endings` was a bare `Option<(LineEnding, LineEnding)>`
//! while its three siblings were `Option<StyleEdit<_>>`. So the nearest a
//! caller could get to removing `/LE` was writing `[/None /None]` — and
//! Table 176's default for both ends **is** `/None`, making that the least
//! informative key available: it says exactly what its own absence would
//! have said. A file could not be returned to the state it was opened in,
//! in a key neither UI shows.
//!
//! ## 3. A `width` on a text markup was silently swallowed
//!
//! `Ok`, no error, no `DroppedProperty`, and nothing changed:
//! `MarkupSpec::TextMarkup` has no border to widen. A shell trusting that
//! pair was entitled to tell the operator the width took. It is now refused
//! by name, and `MarkupStyleSupport` lets a shell ask in advance instead of
//! carrying its own copy of which subtypes have borders.

use pdfcer_core::annot_author::{BorderDash, Color, MarkupSpec, TextMarkupKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    DroppedProperty, EditError, EditSession, MarkupOptions, MarkupStyle, MarkupStyleSupport,
    StyleEdit, VertexEdit,
};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("annot/no-ap-circle.pdf")).expect("load fixture"))
}

fn square() -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 60.0,
            ury: 40.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
}

fn polygon() -> MarkupSpec {
    MarkupSpec::Polygon {
        vertices: vec![(10.0, 10.0), (60.0, 10.0), (60.0, 40.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    }
}

fn highlight() -> MarkupSpec {
    MarkupSpec::TextMarkup {
        kind: TextMarkupKind::Highlight,
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (10.0, 40.0),
            ur: (60.0, 40.0),
            ll: (10.0, 10.0),
            lr: (60.0, 10.0),
        }],
        color: Color::Rgb(1.0, 1.0, 0.0),
    }
}

fn dash(pattern: &[f64]) -> BorderDash {
    BorderDash::new(pattern.to_vec()).expect("a valid dash")
}

/// The annotation's `/BS` sub-dictionary, resolved off the session's graph.
fn border_style(s: &EditSession, id: ObjId) -> Option<pdfcer_core::object::Dict> {
    let graph = s.graph();
    let Object::Dict(annot) = graph.value(id)? else {
        return None;
    };
    match graph.resolve(annot.get(b"BS")?) {
        Object::Dict(bs) => Some(bs.clone()),
        _ => None,
    }
}

/// `/BS /S` as a string, for readable assertions.
fn border_style_name(s: &EditSession, id: ObjId) -> Option<String> {
    let bs = border_style(s, id)?;
    match bs.get(b"S")? {
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

/// The `/BS /D` array, as numbers.
fn dash_array(s: &EditSession, id: ObjId) -> Option<Vec<f64>> {
    let bs = border_style(s, id)?;
    match bs.get(b"D")? {
        Object::Array(items) => Some(items.iter().filter_map(Object::as_number).collect()),
        _ => None,
    }
}

/// Whether the annotation's baked appearance stream sets a dash — the `d`
/// operator. This is the half that actually renders, and the half that was
/// being lost: the dictionary kept `/BS /D` all along.
fn appearance_has_dash(s: &EditSession, id: ObjId) -> bool {
    // Measured on the SAVED bytes, not on the session's overlay: the
    // question is what another program would draw (R159), and the defect
    // this Pass closed was exactly a file whose dictionary and appearance
    // disagreed about the same border.
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("re-parse the saved bytes");
    let Object::Dict(annot) = &doc.get(id).expect("annotation present").value else {
        return false;
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        return false;
    };
    let Object::Stream(stream) = doc.resolve(ap.get(b"N").expect("/AP /N")) else {
        return false;
    };
    let raw = stream
        .data_span
        .slice(doc.bytes())
        .expect("appearance bytes")
        .to_vec();
    // `d` is the dash operator (§8.4.3.6). A solid appearance never emits
    // one at all — see `annot_author::apply_dash` for why not even `[] 0 d`.
    let text = String::from_utf8_lossy(&raw);
    text.contains(" d\n") || text.contains(" d ")
}

// ---------------------------------------------------------------------------
// 1. Preservation — the reported defect
// ---------------------------------------------------------------------------

/// ★ **THE HEADLINE.** Author a dashed square, change ONLY its colour, and
/// the dash must still be there — in the dictionary *and* in the appearance.
///
/// Before this Pass the appearance came back solid while `/BS /D` stayed in
/// the dictionary, so the file described a dashed border and drew a solid
/// one, and under R43 the drawing wins.
#[test]
fn a_colour_change_does_not_solidify_a_dashed_border() {
    let mut s = session();
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                dash: Some(dash(&[4.0, 2.0])),
                ..Default::default()
            },
        )
        .expect("author a dashed square");

    assert_eq!(dash_array(&s, id).as_deref(), Some([4.0, 2.0].as_slice()));
    assert!(appearance_has_dash(&s, id), "authored appearance is dashed");

    // The gesture from the request: press the colour swatch, nothing else.
    let change = s
        .set_markup_style(
            id,
            &MarkupStyle {
                stroke: Some(StyleEdit::Set(Color::Rgb(1.0, 0.0, 0.0))),
                ..Default::default()
            },
        )
        .expect("recolour");

    assert_eq!(
        dash_array(&s, id).as_deref(),
        Some([4.0, 2.0].as_slice()),
        "the /BS /D array must survive a recolour"
    );
    assert_eq!(border_style_name(&s, id).as_deref(), Some("D"));
    assert!(
        appearance_has_dash(&s, id),
        "★ the APPEARANCE must still be dashed — this is the half that \
         renders, and the half that used to come back solid"
    );
    assert!(
        !change.dropped.contains(&DroppedProperty::DashPattern),
        "nothing was dropped, so disclosing a drop would be a FALSE \
         disclosure — the same narrowing Pass 98.0 made for /BE"
    );
    assert!(
        !change.dropped.contains(&DroppedProperty::BorderStyle),
        "/S /D is preserved too, so it is not a dropped border style"
    );
}

/// A dash survives a RESIZE. Same defect, a route the request did not name.
#[test]
fn a_resize_does_not_solidify_a_dashed_border() {
    let mut s = session();
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                dash: Some(dash(&[4.0, 2.0])),
                ..Default::default()
            },
        )
        .expect("author");

    s.resize_annotation(id, (10.0, 10.0), 2.0, 2.0, &Default::default())
        .expect("resize");

    assert_eq!(dash_array(&s, id).as_deref(), Some([4.0, 2.0].as_slice()));
    assert!(
        appearance_has_dash(&s, id),
        "a resize regenerates the appearance and must carry the dash"
    );
}

/// A dash survives a VERTEX EDIT. The fourth route.
#[test]
fn a_reshape_does_not_solidify_a_dashed_border() {
    let mut s = session();
    let id = s
        .add_markup_with(
            0,
            &polygon(),
            &MarkupOptions {
                dash: Some(dash(&[5.0])),
                ..Default::default()
            },
        )
        .expect("author a dashed polygon");

    s.reshape_annotation(
        id,
        VertexEdit::Move {
            index: 0,
            dx: 2.0,
            dy: 4.0,
        },
        None,
    )
    .expect("move a vertex");

    assert_eq!(dash_array(&s, id).as_deref(), Some([5.0].as_slice()));
    assert!(
        appearance_has_dash(&s, id),
        "dragging a node must not solidify the border"
    );
}

// ---------------------------------------------------------------------------
// 1b. Authoring and clearing — the other two thirds of the same request
// ---------------------------------------------------------------------------

/// `Clear` makes a dashed border solid, and says so in both halves.
#[test]
fn clearing_the_dash_makes_it_solid_in_dictionary_and_appearance() {
    let mut s = session();
    let id = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                dash: Some(dash(&[4.0, 2.0])),
                ..Default::default()
            },
        )
        .expect("author");

    s.set_markup_style(
        id,
        &MarkupStyle {
            dash: Some(StyleEdit::Clear),
            ..Default::default()
        },
    )
    .expect("solidify");

    assert_eq!(border_style_name(&s, id).as_deref(), Some("S"));
    assert_eq!(dash_array(&s, id), None, "/D is removed, not zeroed");
    assert!(!appearance_has_dash(&s, id));
}

/// A solid mark can be MADE dashed — the control `RIBBON_IA.md` §5.8 names,
/// and the one deliberately built last because without preservation it would
/// destroy the property it sets.
#[test]
fn a_solid_mark_can_be_made_dashed() {
    let mut s = session();
    let id = s.add_markup(0, &square()).expect("author solid");
    assert_eq!(dash_array(&s, id), None);

    s.set_markup_style(
        id,
        &MarkupStyle {
            dash: Some(StyleEdit::Set(dash(&[6.0, 3.0]))),
            ..Default::default()
        },
    )
    .expect("make it dashed");

    assert_eq!(dash_array(&s, id).as_deref(), Some([6.0, 3.0].as_slice()));
    assert_eq!(border_style_name(&s, id).as_deref(), Some("D"));
    assert!(appearance_has_dash(&s, id));
}

/// §8.4.3.6's constraints are enforced at construction, so an unusable
/// pattern cannot reach a file at all.
#[test]
fn an_unusable_dash_pattern_is_refused_at_construction() {
    assert!(BorderDash::new(vec![3.0]).is_some());
    assert!(BorderDash::new(vec![4.0, 2.0]).is_some());
    assert!(
        BorderDash::new(Vec::new()).is_none(),
        "the empty array IS the solid line, not a dash"
    );
    assert!(BorderDash::new(vec![-1.0]).is_none(), "negative run length");
    assert!(
        BorderDash::new(vec![0.0, 0.0]).is_none(),
        "never on and never off is not a line"
    );
    assert!(BorderDash::new(vec![f64::NAN]).is_none());
    assert!(BorderDash::new(vec![f64::INFINITY]).is_none());
}

/// Authoring solid must be **byte-identical** to what pdfcer authored before
/// this Pass existed, or every already-authored annotation in every existing
/// file would compare as foreign on its next restyle.
#[test]
fn authoring_solid_is_unchanged_by_this_pass() {
    use pdfcer_core::annot_author::{AppearanceOptions, build_appearance, build_appearance_opts};

    let spec = square();
    let old = build_appearance(&spec);
    let new = build_appearance_opts(&spec, &AppearanceOptions::default());
    assert_eq!(
        old.ap_content, new.ap_content,
        "a solid appearance must not gain an explicit `[] 0 d` reset — the \
         byte comparison that decides DroppedProperty::ForeignAppearance \
         depends on this"
    );
    assert_eq!(old.annot.get(b"BS"), new.annot.get(b"BS"));
}

// ---------------------------------------------------------------------------
// 2. `endings` can now be cleared
// ---------------------------------------------------------------------------

fn line_spec() -> MarkupSpec {
    MarkupSpec::Line {
        start: (10.0, 10.0),
        end: (60.0, 40.0),
        color: Color::Gray(0.0),
        width: 1.0,
        endings: (
            pdfcer_core::annot_author::LineEnding::OpenArrow,
            pdfcer_core::annot_author::LineEnding::None,
        ),
    }
}

fn has_le(s: &EditSession, id: ObjId) -> bool {
    let graph = s.graph();
    matches!(graph.value(id), Some(Object::Dict(d)) if d.contains_key(b"LE"))
}

/// ★ `Clear` **removes** `/LE`; it does not write `[/None /None]`.
///
/// The distinction is the request's whole point: an operator who turns an
/// arrow's heads off and changes their mind must get back the document they
/// opened, and *"is this byte-identical to what my client sent me"* is a
/// question that gets asked of a signed drawing.
#[test]
fn clearing_the_endings_removes_the_key() {
    let mut s = session();
    let id = s.add_markup(0, &line_spec()).expect("author an arrow");
    assert!(has_le(&s, id), "authored with /LE");

    s.set_markup_style(
        id,
        &MarkupStyle {
            endings: Some(StyleEdit::Clear),
            ..Default::default()
        },
    )
    .expect("clear the endings");

    assert!(
        !has_le(&s, id),
        "★ Clear must REMOVE /LE. Writing [/None /None] would be a key that \
         says exactly what its absence says (Table 176's default is /None \
         for both ends) — the least informative option available"
    );
}

/// `Set((None, None))` still writes the explicit array. Same picture,
/// different bytes, and this project does not treat those as one document.
#[test]
fn setting_none_none_still_writes_the_key() {
    use pdfcer_core::annot_author::LineEnding;
    let mut s = session();
    let id = s.add_markup(0, &line_spec()).expect("author");

    s.set_markup_style(
        id,
        &MarkupStyle {
            endings: Some(StyleEdit::Set((LineEnding::None, LineEnding::None))),
            ..Default::default()
        },
    )
    .expect("set both ends to None");

    assert!(
        has_le(&s, id),
        "an explicit Set writes the key even when both ends are /None — a \
         caller that wants the key present and empty-handed can say so"
    );
}

// ---------------------------------------------------------------------------
// 3. Properties a subtype does not have are refused BY NAME
// ---------------------------------------------------------------------------

/// ★ A `width` on a text markup is refused, not swallowed.
#[test]
fn a_width_on_a_text_markup_is_refused_by_name() {
    let mut s = session();
    let id = s.add_markup(0, &highlight()).expect("author a highlight");

    let err = s
        .set_markup_style(
            id,
            &MarkupStyle {
                width: Some(2.0),
                ..Default::default()
            },
        )
        .expect_err("a highlight has no border to widen");

    match err {
        EditError::StylePropertyNotApplicable {
            subtype, property, ..
        } => {
            assert_eq!(subtype, "Highlight");
            assert_eq!(property, "a border width");
        }
        other => panic!("expected StylePropertyNotApplicable, got {other:?}"),
    }
}

/// The same for a dash — and this one matters because the flag exists now.
#[test]
fn a_dash_on_a_text_markup_is_refused_by_name() {
    let mut s = session();
    let id = s.add_markup(0, &highlight()).expect("author");
    let err = s
        .set_markup_style(
            id,
            &MarkupStyle {
                dash: Some(StyleEdit::Set(dash(&[3.0]))),
                ..Default::default()
            },
        )
        .expect_err("a highlight draws no /BS border");
    assert!(matches!(err, EditError::StylePropertyNotApplicable { .. }));
}

/// `/LE` is a `/Line` property; asking for it elsewhere is refused.
#[test]
fn endings_on_a_square_are_refused_by_name() {
    use pdfcer_core::annot_author::LineEnding;
    let mut s = session();
    let id = s.add_markup(0, &square()).expect("author");
    let err = s
        .set_markup_style(
            id,
            &MarkupStyle {
                endings: Some(StyleEdit::Set((LineEnding::OpenArrow, LineEnding::None))),
                ..Default::default()
            },
        )
        .expect_err("a square has no line endings");
    assert!(matches!(err, EditError::StylePropertyNotApplicable { .. }));
}

/// A refusal happens BEFORE anything is written, so the refused call leaves
/// the session exactly as it found it.
#[test]
fn a_refused_property_leaves_the_session_untouched() {
    let mut s = session();
    let id = s.add_markup(0, &highlight()).expect("author");
    let depth = s.undo_depth();

    let _ = s.set_markup_style(
        id,
        &MarkupStyle {
            width: Some(2.0),
            ..Default::default()
        },
    );

    assert_eq!(
        s.undo_depth(),
        depth,
        "a refusal must not leave a command on the undo stack"
    );
}

/// The predicate a shell asks instead of copying this crate's knowledge —
/// and the reason the workaround in `pdfcer-gui` can be deleted.
#[test]
fn the_support_matrix_answers_in_advance() {
    for subtype in [
        b"Highlight".as_slice(),
        b"Underline",
        b"StrikeOut",
        b"Squiggly",
    ] {
        let s = MarkupStyleSupport::for_subtype(subtype);
        assert!(!s.takes_border, "text markup draws no /BS border");
        assert!(!s.takes_interior);
        assert!(!s.takes_endings);
    }
    for subtype in [b"Square".as_slice(), b"Circle", b"Polygon"] {
        let s = MarkupStyleSupport::for_subtype(subtype);
        assert!(s.takes_border);
        assert!(s.takes_interior, "these three have an /IC");
        assert!(!s.takes_endings);
    }
    let line = MarkupStyleSupport::for_subtype(b"Line");
    assert!(line.takes_border);
    assert!(!line.takes_interior);
    assert!(line.takes_endings, "/LE is a /Line property");

    let ink = MarkupStyleSupport::for_subtype(b"Ink");
    assert!(ink.takes_border);
    assert!(!ink.takes_interior);

    // An unrecognised subtype answers no to everything — the conservative
    // direction, since the alternative offers a control for a shape pdfcer
    // cannot restyle at all.
    let unknown = MarkupStyleSupport::for_subtype(b"Screen");
    assert!(!unknown.takes_border);
    assert!(!unknown.takes_interior);
    assert!(!unknown.takes_endings);
}

/// ★ The predicate and the refusal must agree. If they ever drift, the
/// shell's "ask first" contract silently becomes wrong — so the agreement is
/// asserted rather than assumed.
#[test]
fn the_predicate_and_the_refusal_agree() {
    let mut s = session();
    let hi = s.add_markup(0, &highlight()).expect("highlight");
    let sq = s.add_markup(0, &square()).expect("square");

    for (id, subtype) in [(hi, b"Highlight".as_slice()), (sq, b"Square".as_slice())] {
        let support = MarkupStyleSupport::for_subtype(subtype);
        let result = s.set_markup_style(
            id,
            &MarkupStyle {
                width: Some(2.0),
                ..Default::default()
            },
        );
        assert_eq!(
            support.takes_border,
            result.is_ok(),
            "MarkupStyleSupport::takes_border must predict exactly whether \
             set_markup_style accepts a width, for /{}",
            String::from_utf8_lossy(subtype)
        );
    }
}
