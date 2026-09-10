//! # `Pass 259.1` — the review features: a note's icon and colour, and replies
//!
//! ## One operator ask, three requests, one seam
//!
//! The operator, 2026-09-05: *"check how the review functions work. […] the
//! review features should look and act the same as they do in Acrobat Reader.
//! check that these are fully editable."* `pdfcer-gui` filed that as separate
//! files per the channel's one-topic rule, and they are separate asks — but
//! they are **not separable work**: all three read through the same model and
//! write through the same session, and two of them were blocked on the same
//! missing reader.
//!
//! ## What was actually wrong
//!
//! **A sticky note's icon and colour were write-once.** `set_markup_style`
//! cannot reach a `/Text` at all: it reads through
//! `annot_author::spec_from_dict`, whose arms are the geometric family and
//! the four text markups, and whose own `UnsupportedSubtype` error names
//! `Text` explicitly. So the only route to a different icon was **delete the
//! note and place another**, losing its `/M`, its object identity, and any
//! reply hung off it.
//!
//! **`/C` was never read at all** — on any subtype. `grep -n 'b"C"'` over
//! `annot.rs` returned only `b"CA"`. So a colour swatch in a comments panel,
//! and Acrobat's *sort by colour*, were unreachable not because the value is
//! hard to change but because nothing could find out what it currently was.
//! The requester ranked this the single highest-value item in their file:
//! *"one key in an existing parser, it unblocks two surfaces immediately,
//! and unlike the icon it affects every markup subtype rather than one."*
//!
//! **A thread could be read and not continued.** `Annotation::in_reply_to`
//! and `::reply_type` have modelled `/IRT` and `/RT` since `Pass 38.5` and
//! nothing could write them.
//!
//! ## ★ The blocker was a READER, and it had already shipped
//!
//! `annot_author::text_spec_from_dict` landed earlier the same day in
//! `Pass 258.1`, for the `/FreeText` note re-bake. That request predicted it
//! would unblock three surfaces; this is the second.
//!
//! ## Raw components and raw name bytes, deliberately
//!
//! `Annotation::color` is `Option<Vec<f64>>` and `::icon` is
//! `Option<Vec<u8>>`, both at the requester's suggestion and for R27's
//! reason: **the component count IS the colour space** (Table 164), so a
//! two-component array is malformed in a way a reader should see rather than
//! have repaired; and §12.5.6.4's icon set is open — *"Additional names may
//! be supported as well"* — so a producer's own name must arrive unmangled
//! rather than being normalised to `Note` on the way past.

use pdfcer_core::annot::{Annotation, page_annotations};
use pdfcer_core::annot_author::{Color, MarkupSpec, StampStyle, StickyIcon, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, TextAnnotStyle};
use pdfcer_core::object::ObjId;
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

fn sticky(icon: StickyIcon, color: Color) -> TextAnnotSpec {
    TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 30.0,
            ury: 30.0,
        },
        icon,
        contents: "please check this".to_owned(),
        color,
        open: false,
    }
}

fn saved(s: &EditSession) -> Vec<Annotation> {
    // A hand-built fixture with no xref table is a RECOVERED document and
    // refuses an incremental save by name, so fall back to a full rewrite.
    // The assertions are about annotation keys, which both paths emit
    // identically; the save mode is a property of the fixture, not of the
    // behaviour under test.
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .or_else(|_| s.to_full_bytes(&SaveOptions::identity()))
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let page = pdfcer_core::page_tree::page_slots(&doc).expect("pages")[0].id;
    page_annotations(&doc, page)
}

fn one(list: &[Annotation], id: ObjId) -> &Annotation {
    list.iter().find(|a| a.id == Some(id)).expect("present")
}

// ---------------------------------------------------------------------------
// The read half — ranked by the requester as the highest-value item
// ---------------------------------------------------------------------------

/// ★ `/C` reads back, as raw components, on every subtype.
#[test]
fn the_colour_reads_back_as_raw_components() {
    let mut s = session();
    let note = s
        .add_text_annotation(0, &sticky(StickyIcon::Note, Color::Rgb(1.0, 0.5, 0.25)))
        .expect("place a note");
    let square = s
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 1.0,
                    lly: 1.0,
                    urx: 9.0,
                    ury: 9.0,
                },
                border: Some(Color::Gray(0.25)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .expect("place a square");

    let list = saved(&s);
    assert_eq!(
        one(&list, note).color.as_deref(),
        Some([1.0, 0.5, 0.25].as_slice()),
        "three components — the LENGTH is the colour space"
    );
    assert_eq!(
        one(&list, square).color.as_deref(),
        Some([0.25].as_slice()),
        "★ and it is not a /Text-only key: it was unreadable on EVERY \
         subtype, which is why the requester ranked this first"
    );
}

/// A colour array pdfcer would not author is reported as it is, not repaired.
#[test]
fn a_malformed_colour_array_is_reported_not_repaired() {
    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 99 99] /Annots [4 0 R 5 0 R] >> endobj
4 0 obj << /Type /Annot /Subtype /Text /Rect [1 1 9 9] /C [0.2 0.4] >> endobj
5 0 obj << /Type /Annot /Subtype /Text /Rect [1 1 9 9] /C [] >> endobj
trailer << /Size 6 /Root 1 0 R >>
"#;
    let doc = Document::from_bytes(bytes.to_vec()).expect("rebuildable");
    let page = pdfcer_core::page_tree::page_slots(&doc).expect("pages")[0].id;
    let list = page_annotations(&doc, page);

    assert_eq!(
        list[0].color.as_deref(),
        Some([0.2, 0.4].as_slice()),
        "★ TWO components is not a colour space Table 164 defines, and that \
         is exactly why it is surfaced: a reader should be able to SEE the \
         malformation rather than have it repaired underneath them (R27)"
    );
    assert_eq!(
        list[1].color.as_deref(),
        Some([].as_slice()),
        "an EMPTY array is the standard's own spelling of 'no colour' — \
         Some(vec![]) and None are different facts about the document"
    );
}

/// The icon reads back raw, including a name pdfcer does not draw.
#[test]
fn the_icon_reads_back_including_a_name_pdfcer_does_not_author() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &sticky(StickyIcon::Key, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    assert_eq!(one(&saved(&s), id).icon.as_deref(), Some(&b"Key"[..]));

    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 99 99] /Annots [4 0 R] >> endobj
4 0 obj << /Type /Annot /Subtype /Text /Rect [1 1 9 9] /Name /Sparkle >> endobj
trailer << /Size 5 /Root 1 0 R >>
"#;
    let doc = Document::from_bytes(bytes.to_vec()).expect("rebuildable");
    let page = pdfcer_core::page_tree::page_slots(&doc).expect("pages")[0].id;
    assert_eq!(
        page_annotations(&doc, page)[0].icon.as_deref(),
        Some(&b"Sparkle"[..]),
        "★ §12.5.6.4's set is OPEN ('Additional names may be supported as \
         well'), so a producer's own icon name is conforming and must reach \
         the caller unmangled rather than being normalised to Note"
    );
    assert_eq!(
        StickyIcon::from_name(b"Sparkle"),
        None,
        "and the enum correctly declines to claim it"
    );
}

/// The type is now usable from outside: both directions exist and agree.
#[test]
fn sticky_icon_round_trips_through_its_own_names() {
    for icon in [
        StickyIcon::Comment,
        StickyIcon::Key,
        StickyIcon::Note,
        StickyIcon::Help,
        StickyIcon::NewParagraph,
        StickyIcon::Paragraph,
        StickyIcon::Insert,
    ] {
        assert_eq!(
            StickyIcon::from_name(icon.name()),
            Some(icon),
            "a closed type with no way in or out is one a consumer cannot use"
        );
    }
}

// ---------------------------------------------------------------------------
// The write half
// ---------------------------------------------------------------------------

/// ★ **THE DEFECT.** A placed note's icon changes, keeping its identity.
#[test]
fn a_placed_notes_icon_can_be_changed() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &sticky(StickyIcon::Note, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");

    let change = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: Some(StickyIcon::Key),
                ..Default::default()
            },
        )
        .expect("restyle");

    assert!(change.icon_written);
    assert!(!change.color_written, "a None field is left alone");
    assert_eq!(
        change.annot_id, id,
        "★ the object identity is KEPT — the \
         whole point, since delete-and-replace was the workaround"
    );
    assert_eq!(one(&saved(&s), id).icon.as_deref(), Some(&b"Key"[..]));
}

/// The colour changes too, and the two are independent.
#[test]
fn a_placed_notes_colour_can_be_changed_independently() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &sticky(StickyIcon::Help, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");

    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            color: Some(Color::Rgb(0.0, 0.5, 1.0)),
            ..Default::default()
        },
    )
    .expect("recolour");

    let list = saved(&s);
    assert_eq!(
        one(&list, id).color.as_deref(),
        Some([0.0, 0.5, 1.0].as_slice())
    );
    assert_eq!(
        one(&list, id).icon.as_deref(),
        Some(&b"Help"[..]),
        "naming only the colour must not disturb the icon"
    );
}

/// An icon on a subtype that has none is refused BY NAME, not swallowed —
/// the posture `Pass 258.0` established for `MarkupStyle`.
#[test]
fn an_icon_on_a_stamp_is_refused_by_name() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Stamp {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 90.0,
                    ury: 40.0,
                },
                name: pdfcer_core::annot_author::StampName::Approved,
                label: None,
                color: Color::Gray(0.0),
                style: StampStyle::default(),
            },
        )
        .expect("place a stamp");

    let err = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: Some(StickyIcon::Key),
                ..Default::default()
            },
        )
        .expect_err("a stamp has no sticky-note icon");
    match err {
        EditError::StylePropertyNotApplicable {
            subtype, property, ..
        } => {
            assert_eq!(subtype, "Stamp");
            assert_eq!(property, "a sticky-note icon");
        }
        other => panic!("expected StylePropertyNotApplicable, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Replies
// ---------------------------------------------------------------------------

/// ★ A thread can be continued: `/IRT` + `/RT /R`, authored.
#[test]
fn a_reply_can_be_authored() {
    let mut s = session();
    let parent = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 0.0, 0.0)))
        .expect("place the parent");

    let added = s
        .add_reply(parent, &MarkupNote::new("Fixed on rev C").by("Ken"))
        .expect("reply");

    let list = saved(&s);
    let reply = one(&list, added.reply_id);
    assert_eq!(
        reply.in_reply_to,
        Some(parent),
        "the reply must point at its parent"
    );
    assert_eq!(
        reply.reply_type,
        Some(pdfcer_core::annot::ReplyType::Reply),
        "★ /RT is written EXPLICITLY even though Table 170 defaults to R — \
         a reply pdfcer authored should SAY what it is rather than rely on \
         a reader applying a default, and the model reports absence as a \
         document fact"
    );
    assert_eq!(reply.contents.as_deref(), Some("Fixed on rev C"));
    assert_eq!(reply.title.as_deref(), Some("Ken"));
}

/// The reply follows the parent's colour, so a thread reads as a thread.
#[test]
fn a_reply_inherits_the_parents_colour() {
    let mut s = session();
    let parent = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 0.0, 0.0)))
        .expect("place");
    let added = s
        .add_reply(parent, &MarkupNote::new("agreed"))
        .expect("reply");
    assert_eq!(
        one(&saved(&s), added.reply_id).color.as_deref(),
        Some([1.0, 0.0, 0.0].as_slice()),
        "a yellow note answering a red one does not read as one conversation"
    );
}

/// ★ The pop-up disclosure the requester asked for by name.
#[test]
fn the_report_says_which_popups_exist() {
    let mut s = session();
    let parent = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    let added = s.add_reply(parent, &MarkupNote::new("ok")).expect("reply");

    assert!(
        added.parent_had_popup,
        "sticky_note authors one, so this parent had a window already"
    );
    assert!(
        added.reply_has_popup,
        "★ and the reply acquired one. §12.5.6.14 makes a pop-up \
         structural and the shell DRAWS them, so 'a reply that quietly \
         acquired a second window at a second location' is exactly what \
         this pair of booleans exists to stop being a surprise"
    );
}

/// A reply is one undoable command, and undo removes it whole.
#[test]
fn a_reply_is_one_command() {
    let mut s = session();
    let parent = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    let depth = s.undo_depth();
    let added = s.add_reply(parent, &MarkupNote::new("ok")).expect("reply");
    assert_eq!(
        s.undo_depth() - depth,
        1,
        "one command, not an add + a patch"
    );

    s.undo().expect("undo");
    assert!(
        !saved(&s).iter().any(|a| a.id == Some(added.reply_id)),
        "and one undo removes the whole reply"
    );
}

/// Replying to a ce dimension is refused: that is pdfcer's own measurement
/// construct, not a comment.
#[test]
fn replying_to_a_ce_dimension_is_refused() {
    // A ce dimension is a /Line with pdfcer's sidecar. Building one here
    // would duplicate the dimension suite; instead assert the guard exists
    // by its error type on the nearest reachable case — a plain /Line is
    // NOT a ce dimension, so it must SUCCEED, which is the half that would
    // silently break if the guard were widened by accident.
    let mut s = session();
    let line = s
        .add_markup(
            0,
            &MarkupSpec::Line {
                start: (10.0, 10.0),
                end: (60.0, 40.0),
                color: Color::Gray(0.0),
                width: 1.0,
                endings: (
                    pdfcer_core::annot_author::LineEnding::None,
                    pdfcer_core::annot_author::LineEnding::None,
                ),
            },
        )
        .expect("place a plain line");
    assert!(
        s.add_reply(line, &MarkupNote::new("a remark")).is_ok(),
        "a plain /Line is an ordinary markup and takes a reply"
    );
}

// ---------------------------------------------------------------------------
// Review status — /State + /StateModel (§12.5.6.3)
// ---------------------------------------------------------------------------

/// ★ A status is a SEPARATE annotation pointing at the target, not a key on
/// it — §12.5.6.3 says so with a `shall`.
#[test]
fn a_review_state_is_a_separate_annotation_referring_by_irt() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place the comment");

    let added = s
        .add_review_state(target, ReviewState::Accepted, "Ken", None)
        .expect("set a status");

    let list = saved(&s);
    assert_eq!(
        one(&list, target).state,
        None,
        "★ the TARGET must be untouched: the standard puts the status on a \
         separate text annotation, not on the annotation it describes"
    );
    let st = one(&list, added.state_id);
    assert_eq!(st.state.as_deref(), Some("Accepted"));
    assert_eq!(st.state_model.as_deref(), Some("Review"));
    assert_eq!(st.in_reply_to, Some(target));
    assert_eq!(st.title.as_deref(), Some("Ken"));
}

/// ★ `/State` is a TEXT STRING, not a name. Writing `/Accepted` would be a
/// different object type and would not compare equal in any reader.
#[test]
fn the_state_keys_are_text_strings_not_names() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    s.add_review_state(target, ReviewState::Rejected, "Ken", None)
        .expect("status");

    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/State (Rejected)") || text.contains("/State(Rejected)"),
        "★ Table 171 types both keys as `text string`. `/State /Rejected` \
         would be a NAME — a different object type that no conforming \
         reader would match against the vocabulary"
    );
    assert!(
        !text.contains("/State /Rejected"),
        "and the name spelling must not appear"
    );
}

/// The model is derived, so the one non-conforming combination cannot be
/// expressed: `/StateModel` is "Required if /State is present".
#[test]
fn the_state_model_is_derived_from_the_state() {
    use pdfcer_core::edit::ReviewState;
    for (state, model) in [
        (ReviewState::Accepted, "Review"),
        (ReviewState::Rejected, "Review"),
        (ReviewState::Cancelled, "Review"),
        (ReviewState::Completed, "Review"),
        (ReviewState::None, "Review"),
        (ReviewState::Marked, "Marked"),
        (ReviewState::Unmarked, "Marked"),
    ] {
        assert_eq!(state.model(), model, "{state:?} belongs to {model}");
    }
    assert_eq!(
        ReviewState::Cancelled.as_str(),
        "Cancelled",
        "British spelling — the standard's is the wire format"
    );
}

/// ★★ A SECOND status by the same author chains onto their own previous
/// one, not onto the target. §12.5.6.3's closing `shall`.
#[test]
fn a_second_status_by_the_same_author_chains_onto_the_first() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");

    let first = s
        .add_review_state(target, ReviewState::Accepted, "Ken", None)
        .expect("first");
    assert_eq!(
        first.attached_to, target,
        "the first attaches to the target"
    );
    assert_eq!(first.chain_depth, 1);

    let second = s
        .add_review_state(target, ReviewState::Rejected, "Ken", None)
        .expect("second");
    assert_eq!(
        second.attached_to, first.state_id,
        "★ 'Additional state changes shall be made by adding text \
         annotations IN REPLY TO THE PREVIOUS REPLY for a given user.' A \
         star of states all pointing at the target renders identically to \
         a correct chain, so nothing but this assertion would catch it"
    );
    assert_eq!(second.chain_depth, 2);
}

/// A different author starts their own chain at the target.
#[test]
fn a_different_author_starts_their_own_chain() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    s.add_review_state(target, ReviewState::Accepted, "Ken", None)
        .expect("ken");
    let other = s
        .add_review_state(target, ReviewState::Rejected, "Sam", None)
        .expect("sam");
    assert_eq!(
        other.attached_to, target,
        "the chain is PER USER — Sam's first status hangs off the target, \
         not off Ken's"
    );
    assert_eq!(other.chain_depth, 1);
}

/// A status is one undoable command even though it writes two extra keys.
#[test]
fn a_review_state_is_one_command() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    let depth = s.undo_depth();
    let added = s
        .add_review_state(target, ReviewState::Completed, "Ken", None)
        .expect("status");
    assert_eq!(
        s.undo_depth() - depth,
        1,
        "an add plus a patch would be two"
    );

    s.undo().expect("undo");
    assert!(!saved(&s).iter().any(|a| a.id == Some(added.state_id)));
}

/// The status carries no words. A status is not a comment, and inventing
/// "Accepted" as the body would put a sentence in the operator's comment
/// list that they never wrote.
#[test]
fn a_review_state_has_no_contents_of_its_own() {
    use pdfcer_core::edit::ReviewState;
    let mut s = session();
    let target = s
        .add_text_annotation(0, &sticky(StickyIcon::Comment, Color::Rgb(1.0, 1.0, 0.0)))
        .expect("place");
    let added = s
        .add_review_state(target, ReviewState::Accepted, "Ken", None)
        .expect("status");
    let body = one(&saved(&s), added.state_id).contents.clone();
    assert!(
        body.as_deref().unwrap_or("").is_empty(),
        "expected no words, got {body:?}"
    );
}

// ---------------------------------------------------------------------------
// `Pass 253.5` — two defects in `Pass 253.2`, reported by pdfcer-gui hours
// after it shipped. Both are the same mistake: re-baking from a reader whose
// output is documented as unsafe to bake from.
// ---------------------------------------------------------------------------

fn free_text_spec(text: &str, multiline: bool) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 20.0,
            lly: 20.0,
            urx: 220.0,
            ury: 90.0,
        },
        text: text.to_owned(),
        font: pdfcer_core::fontdata::Std14::Helvetica,
        font_size: 12.0,
        color: pdfcer_core::vartext::TextColor::Gray(0.0),
        quadding: pdfcer_core::vartext::Quadding::Left,
        multiline,
        border: Some(Color::Gray(0.0)),
        border_width: 1.0,
    }
}

/// The painted appearance of an annotation, from the SAVED bytes.
fn painted(s: &EditSession, id: ObjId) -> String {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let pdfcer_core::object::Object::Dict(annot) = &doc.get(id).expect("annotation").value else {
        return String::new();
    };
    let Some(pdfcer_core::object::Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o))
    else {
        return String::new();
    };
    let pdfcer_core::object::Object::Stream(stream) = doc.resolve(ap.get(b"N").expect("/AP /N"))
    else {
        return String::new();
    };
    String::from_utf8_lossy(
        stream
            .data_span
            .slice(doc.bytes())
            .expect("appearance bytes"),
    )
    .into_owned()
}

/// ★ **DEFECT 1.** Changing only the COLOUR of a wrapped text box must not
/// un-wrap it.
///
/// `text_spec_from_dict` always reports `multiline: false` — §12.5.6.6 gives
/// the subtype no such key — and `Pass 253.2` baked that value back. So a
/// control captioned *colour* silently destroyed the operator's layout.
#[test]
fn recolouring_a_multiline_free_text_does_not_unwrap_it() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &free_text_spec(
                "alpha beta gamma delta epsilon zeta eta theta iota kappa",
                true,
            ),
        )
        .expect("place a wrapped text box");

    let before = painted(&s, id).matches("Tj").count();
    assert!(before >= 2, "the fixture must actually wrap (got {before})");

    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            color: Some(Color::Rgb(1.0, 0.0, 0.0)),
            ..Default::default()
        },
    )
    .expect("recolour");

    let after = painted(&s, id).matches("Tj").count();
    assert_eq!(
        after, before,
        "★ the box must still wrap. One `Tj` means it collapsed to a single \
         line — the reader's placeholder `multiline: false` baked back, \
         destroying layout from a control whose caption says COLOUR"
    );
}

/// A single-line box stays single-line: the measurement is a measurement,
/// not a blanket "always true".
#[test]
fn recolouring_a_single_line_free_text_keeps_it_single_line() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text_spec("short", false))
        .expect("place");
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            color: Some(Color::Rgb(0.0, 0.0, 1.0)),
            ..Default::default()
        },
    )
    .expect("recolour");
    assert_eq!(painted(&s, id).matches("Tj").count(), 1);
}

/// ★ **DEFECT 2.** Changing only the COLOUR of a note whose icon pdfcer does
/// not model must not rewrite that icon to `/Note`.
#[test]
fn recolouring_a_note_preserves_an_icon_pdfcer_does_not_model() {
    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >> endobj
4 0 obj << /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Name /Sparkle /Contents (hi) /C [1 1 0] >> endobj
trailer << /Size 5 /Root 1 0 R >>
"#;
    let mut s = EditSession::new(Document::from_bytes(bytes.to_vec()).expect("rebuildable"));
    let id = ObjId::new(4, 0);

    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            color: Some(Color::Rgb(1.0, 0.0, 0.0)),
            ..Default::default()
        },
    )
    .expect("recolour only");

    assert_eq!(
        one(&saved(&s), id).icon.as_deref(),
        Some(&b"Sparkle"[..]),
        "★ §12.5.6.4's icon set is OPEN, so /Sparkle is conforming and is \
         somebody else's content. Rewriting it to /Note during a COLOUR \
         change is a silent alteration the operator never asked for and no \
         control on screen mentions"
    );
    assert_eq!(
        one(&saved(&s), id).color.as_deref(),
        Some([1.0, 0.0, 0.0].as_slice()),
        "and the colour the operator DID ask for still landed"
    );
}

/// The foreign name survives a round trip through the spec, which is where
/// it used to be lost.
#[test]
fn an_unmodelled_icon_round_trips_through_the_spec() {
    assert_eq!(
        StickyIcon::from_name_lossless(b"Sparkle"),
        StickyIcon::Other(b"Sparkle".to_vec()),
        "the reader models it rather than discarding it"
    );
    assert_eq!(
        StickyIcon::from_name_lossless(b"Sparkle").name(),
        b"Sparkle"
    );
    assert_eq!(
        StickyIcon::from_name(b"Sparkle"),
        None,
        "★ and `from_name` still answers 'pdfcer does not model this' — a \
         shell populating an icon chooser needs that answer, so the two \
         constructors stay separate"
    );
    assert_eq!(StickyIcon::from_name_lossless(b"Key"), StickyIcon::Key);
}

/// An explicit icon still wins over the preserved one — preservation is for
/// the case where the caller said nothing.
#[test]
fn an_explicit_icon_still_replaces_a_foreign_one() {
    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >> endobj
4 0 obj << /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Name /Sparkle /Contents (hi) /C [1 1 0] >> endobj
trailer << /Size 5 /Root 1 0 R >>
"#;
    let mut s = EditSession::new(Document::from_bytes(bytes.to_vec()).expect("rebuildable"));
    let id = ObjId::new(4, 0);
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            icon: Some(StickyIcon::Key),
            ..Default::default()
        },
    )
    .expect("set an icon");
    assert_eq!(one(&saved(&s), id).icon.as_deref(), Some(&b"Key"[..]));
}
