//! # A check box draws the glyph the operator picked, and keeps it
//!
//! ## Acrobat's six, and why pdfcer draws them instead of setting a font
//!
//! Acrobat exposes exactly six check styles — Check, Cross, Star, Circle,
//! Square, Diamond — and records the choice as a single ZapfDingbats
//! character in `/MK` `/CA` (Table 189/192). Until 2026-09-07 pdfcer drew one
//! hard-coded tick and offered no choice at all, which the operator named as
//! the checkbox gap when he asked what *"modern undeprecated"* features were
//! missing.
//!
//! The character codes come from Adobe's own `ZapfDingbats.afm` metrics and
//! the Adobe Glyph List, cross-checked by a second independent method before
//! being accepted — not from recall.
//!
//! ★★ **pdfcer writes the character AND draws the shape as vector artwork.**
//! Acrobat's own appearance selects a ZapfDingbats font and shows the glyph,
//! so the tick depends on resolving that font at display time — and Acrobat
//! and Reader have a long-standing, recurring bug failing exactly that,
//! leaving the box blank. Paths need no font, no `/Resources` entry and no
//! substitution.
//!
//! ## What is pinned
//!
//! 1. **Every style is reachable and draws DIFFERENT artwork.** The cheapest
//!    wrong implementation accepts the parameter, threads it through every
//!    signature and draws a tick regardless — and would pass any test that
//!    only checked "a check box was created". Comparing the six streams
//!    pairwise is what refutes it.
//! 2. **`/MK` `/CA` records the choice**, with Adobe's exact character.
//! 3. **The choice survives a RESIZE** — the appearance is rebuilt from
//!    geometry and the style lives only in `/MK` `/CA`, so the rebuild has to
//!    go back to that key or the operator's star silently becomes a tick
//!    while the file still says star.
//! 4. **The character round trip is exact**, and an unknown character reads
//!    as unknown rather than defaulting.
//! 5. **The default is unchanged**, so existing callers draw what they drew.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::CheckStyle;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewCheckBox, WidgetEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("minimal.pdf")).expect("load minimal.pdf"))
}

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 40.0,
    urx: 60.0,
    ury: 60.0,
};

const BIG: Rect = Rect {
    llx: 40.0,
    lly: 40.0,
    urx: 90.0,
    ury: 90.0,
};

/// The widget dictionary of the single field named `cb`.
fn widget_dict(s: &EditSession) -> pdfcer_core::object::Dict {
    let g = s.graph();
    let form = forms::parse_acroform(&g).expect("an AcroForm");
    let field = form.fields.first().expect("one field");
    g.resolved(field.widgets.first().expect("one widget").id)
        .as_dict()
        .cloned()
        .expect("widget dict")
}

/// The ON-state appearance stream's decoded bytes.
///
/// Read from the session's own view rather than from the builder's return
/// value: the question is what a viewer will find in the file, and a test
/// that asked the builder what it drew would agree with itself.
fn on_state_bytes(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let form = forms::parse_acroform(&g).expect("an AcroForm");
    let field = form.fields.first().expect("one field");
    let widget = field.widgets.first().expect("one widget");
    let on = widget.on_states.first().cloned().expect("an on-state name");
    let dict = g.resolved(widget.id).as_dict().cloned().expect("widget");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(Object::Dict(n)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(entry) = n.get(on.as_slice()) else {
        return Vec::new();
    };
    let Object::Stream(st) = g.resolve(entry).clone() else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

/// `/MK` `/CA` as raw bytes, if present.
fn mk_ca(s: &EditSession) -> Option<Vec<u8>> {
    let g = s.graph();
    let dict = widget_dict(s);
    let Some(Object::Dict(mk)) = dict.get(b"MK").map(|o| g.resolve(o).clone()) else {
        return None;
    };
    match mk.get(b"CA").map(|o| g.resolve(o).clone()) {
        Some(Object::String(b)) => Some(b),
        _ => None,
    }
}

/// Author one check box in `style`, at `rect`.
fn authored(style: CheckStyle, rect: Rect) -> EditSession {
    let mut s = session();
    // Tooltip declined explicitly: pdfcer refuses an undecided one by name
    // (TooltipDecisionRequired) rather than silently writing none, which is
    // rule 4 applied to authoring. Not what this file tests.
    let mut spec = NewCheckBox::new(0, "cb", rect).declining_tooltip();
    spec.style = style;
    s.add_check_box(&spec).expect("author the check box");
    s
}

/// ★★ All six styles draw DIFFERENT artwork.
#[test]
fn every_style_draws_different_artwork() {
    let mut seen: HashMap<Vec<u8>, CheckStyle> = HashMap::new();
    for style in CheckStyle::all() {
        let s = authored(style, BOX);
        let content = on_state_bytes(&s);
        assert!(
            !content.is_empty(),
            "{style:?} drew an EMPTY appearance — a blank box is the exact failure mode that \
             drawing paths instead of a ZapfDingbats glyph exists to prevent"
        );
        if let Some(prev) = seen.insert(content, style) {
            panic!(
                "{style:?} and {prev:?} produced byte-identical artwork — the style parameter is \
                 being accepted and ignored, which is what this test exists to catch"
            );
        }
    }
    assert_eq!(seen.len(), 6, "expected six distinct appearances");
}

/// `/MK` `/CA` records the choice with Adobe's own character.
///
/// The table is spelled out rather than derived from `mk_caption_char` —
/// deriving it would make the test agree with the code by construction and
/// prove nothing about the CODES being right, which is the part that came
/// from Adobe's font data.
#[test]
fn the_style_is_recorded_in_mk_ca_with_adobes_character() {
    for (style, want) in [
        (CheckStyle::Check, b'4'),
        (CheckStyle::Cross, b'8'),
        (CheckStyle::Star, b'H'),
        (CheckStyle::Circle, b'l'),
        (CheckStyle::Square, b'n'),
        (CheckStyle::Diamond, b'u'),
    ] {
        let s = authored(style, BOX);
        let ca = mk_ca(&s).unwrap_or_else(|| panic!("{style:?} wrote no /MK /CA"));
        assert_eq!(
            ca,
            vec![want],
            "{style:?} must record {:?} (ZapfDingbats)",
            want as char
        );
    }
}

/// ★★ The style SURVIVES A RESIZE.
///
/// The appearance is rebuilt from geometry on a resize and the style lives
/// only in `/MK` `/CA`, so the rebuild has to go back to that key. If it does
/// not, the operator's star silently becomes a tick the first time they drag
/// a handle — while `/MK` `/CA` still says star. That is the worst outcome
/// available: the file and the pixels disagree and neither is obviously
/// wrong.
#[test]
fn the_style_survives_a_resize() {
    let mut s = authored(CheckStyle::Star, BOX);
    s.edit_widget("cb", 0, &WidgetEdit::new().with_rect(BIG))
        .expect("resize the widget");
    let after = on_state_bytes(&s);

    let star_at_big = on_state_bytes(&authored(CheckStyle::Star, BIG));
    let check_at_big = on_state_bytes(&authored(CheckStyle::Check, BIG));

    assert_ne!(
        star_at_big, check_at_big,
        "the two references are identical, so this test could not tell them apart"
    );
    assert_eq!(
        after, star_at_big,
        "after a resize the box must still draw a STAR at the new size. If this equals the check \
         artwork, the rebuild ignored /MK /CA and the file now says star while the pixels say \
         check."
    );
}

/// The character round trip is exact, and an unknown character reads as
/// unknown rather than defaulting.
///
/// The `None` half is the one that matters: Table 189 places no constraint on
/// `/MK` `/CA`, so a producer may legitimately store a character outside
/// these six, and answering `Check` would tell a shell's picker the operator
/// chose a tick when they chose something pdfcer cannot name.
#[test]
fn the_caption_character_round_trips_and_an_unknown_one_is_not_defaulted() {
    for style in CheckStyle::all() {
        assert_eq!(
            CheckStyle::from_mk_caption_char(style.mk_caption_char()),
            Some(style),
            "{style:?} did not round-trip through its /MK /CA character"
        );
    }
    for c in *b"Z0 \x00" {
        assert_eq!(
            CheckStyle::from_mk_caption_char(c),
            None,
            "an unrecognised /MK /CA character ({c:?}) must read as None, not a default style"
        );
    }
    assert_eq!(CheckStyle::parse("nonsense"), None);
    assert_eq!(CheckStyle::parse("STAR"), Some(CheckStyle::Star));
}

/// The default is still `Check`, so every existing caller draws exactly what
/// it drew before this feature existed.
#[test]
fn the_default_style_is_unchanged() {
    assert_eq!(CheckStyle::default(), CheckStyle::Check);

    let explicit = on_state_bytes(&authored(CheckStyle::Check, BOX));

    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "cb", BOX).declining_tooltip())
        .expect("author with no style set");
    assert_eq!(
        on_state_bytes(&s),
        explicit,
        "an unspecified style must draw exactly what CheckStyle::Check draws"
    );
}
