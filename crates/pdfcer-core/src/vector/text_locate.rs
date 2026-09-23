//! Map an extracted glyph to the surgery run an edit verb acts on.
//!
//! Text is described twice: [`crate::text_extract`] (caret, selection,
//! search) and [`super::decompose`] (every verb that rewrites bytes). They
//! share no index space. This module is the join, owned by the crate so a
//! change to either side fails a test here rather than editing the wrong run
//! in a shell.
//!
//! # The join
//!
//! - **Buffer.** [`ContentStreamRef::Page`] joins [`PageObjects::objects`];
//!   [`ContentStreamRef::Form`] joins the [`PageObjects::leaves`] whose
//!   directly-enclosing form ([`FormLeaf::containment`]'s last entry) is that
//!   object.
//! - **Operator.** Extraction records the show operator's keyword span;
//!   decomposition records operands through keyword. Both end on the
//!   keyword's last byte, so a run matches when its span *ends where the
//!   operator ends*. A `TJ` array is one operator
//!   on both sides (§9.4.3).
//! - **Placement.** A form drawn more than once yields one leaf per `Do`
//!   with the same bytes. The glyph's CTM (§8.3.4; `cm` is not permitted
//!   inside `BT`…`ET`, §9.4.1, so it equals the CTM at `BT`) picks the leaf.
//!   If no leaf's CTM agrees, the answer is `None` rather than a guess.
//!
//! # Preconditions
//!
//! The extraction must set [`crate::text_extract::ExtractOptions::capture_provenance`]
//! (off by default); without it glyphs carry no provenance to join on.
//!
//! `model` and the glyph must come from the **same revision** of the page —
//! both from one [`crate::document::DocumentView`], e.g. an
//! [`crate::edit::EditSession`]'s view. Spans from different revisions are
//! offsets into different buffers; nothing here can detect the mix.

use super::decompose::{FormLeaf, PageObjects, TextObject, VectorObject};
use super::geometry::Matrix;
use crate::text_extract::{ContentStreamRef, GlyphProvenance, TextRun as ExtractedRun};

/// Where an extracted glyph lives in the surgery model.
///
/// `Page` indexes [`PageObjects::objects`] and is what
/// [`crate::edit::EditSession::move_text_run`] and its siblings take; `Form`
/// indexes [`PageObjects::leaves`] and is what the `*_in_form` verbs take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextRunRef {
    /// A run in the page's own content stream.
    Page {
        /// Index into [`PageObjects::objects`] (a [`VectorObject::Text`]).
        object_index: usize,
        /// Index into that object's [`TextObject::runs`].
        run_index: usize,
    },
    /// A run inside a form XObject.
    Form {
        /// Index into [`PageObjects::leaves`].
        leaf_index: usize,
        /// Index into that leaf's [`TextObject::runs`].
        run_index: usize,
    },
}

/// Relative + absolute tolerance for comparing an `f32` glyph CTM against an
/// `f64` object CTM computed by a different walker.
const CTM_EPSILON: f64 = 1e-3;

/// Resolve one glyph's provenance to the surgery run that shows it.
///
/// `None` when the glyph came from a buffer `model` does not describe (a
/// Type 3 glyph procedure, a form the decomposition did not reach), when no
/// run's span ends at the glyph's operator, or when a repeated form's
/// placements cannot be told apart by CTM.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::page_tree::pages;
/// use pdfcer_core::text_extract::{extract_page, ExtractOptions};
/// use pdfcer_core::vector::{decompose_page, locate_text_run, Matrix};
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let page = &pages(doc)?[0];
/// let text = extract_page(doc, page, 0, &ExtractOptions::default().with_provenance(true))?;
/// let model = decompose_page(&doc.view(), page, Matrix::IDENTITY)?;
/// let glyph = &text.runs[0].glyphs[0];
/// if let Some(p) = &glyph.provenance {
///     println!("{:?}", locate_text_run(&model, p));
/// }
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn locate_text_run(model: &PageObjects, glyph: &GlyphProvenance) -> Option<TextRunRef> {
    let op = glyph.operator_span;
    match glyph.content_stream {
        ContentStreamRef::Page => model
            .objects
            .iter()
            .enumerate()
            .find_map(|(object_index, o)| {
                let VectorObject::Text(t) = o else {
                    return None;
                };
                run_ending_at(t, op).map(|run_index| TextRunRef::Page {
                    object_index,
                    run_index,
                })
            }),
        ContentStreamRef::Form { object } => {
            let candidates: Vec<(usize, &TextObject, usize)> = model
                .leaves
                .iter()
                .enumerate()
                .filter_map(|(i, leaf)| {
                    let t = leaf_text(leaf, object)?;
                    run_ending_at(t, op).map(|r| (i, t, r))
                })
                .collect();
            let pick = match candidates.as_slice() {
                [] => None,
                [one] => Some(*one),
                many => {
                    let want = glyph_ctm(glyph).post_concat(model.initial);
                    let mut agreeing = many.iter().filter(|(_, t, _)| ctm_eq(t.ctm, want));
                    // Two leaves at one placement show the same ink from the
                    // same bytes; the first is as correct as the second.
                    agreeing.next().copied()
                }
            };
            pick.map(|(leaf_index, _, run_index)| TextRunRef::Form {
                leaf_index,
                run_index,
            })
        }
    }
}

/// The distinct surgery runs behind every glyph of one extracted run, in
/// glyph order.
///
/// An extracted run can span several show operators (layout joins them on a
/// shared baseline), so this is a list. Glyphs that do not resolve are
/// skipped; compare the length against the operator count if that matters.
#[must_use]
pub fn locate_text_runs(model: &PageObjects, run: &ExtractedRun) -> Vec<TextRunRef> {
    let mut out: Vec<TextRunRef> = Vec::new();
    for g in &run.glyphs {
        let Some(p) = &g.provenance else { continue };
        if let Some(r) = locate_text_run(model, p)
            && !out.contains(&r)
        {
            out.push(r);
        }
    }
    out
}

/// The run of `t` whose span ends where `op` ends. Show operators never
/// share a keyword, so the end byte alone identifies the run.
fn run_ending_at(t: &TextObject, op: crate::span::ByteSpan) -> Option<usize> {
    if op.len == 0 {
        return None;
    }
    t.runs.iter().position(|r| r.bytes.end() == op.end())
}

/// The text object of `leaf` when it sits directly inside form `object`.
fn leaf_text(leaf: &FormLeaf, object: u32) -> Option<&TextObject> {
    let VectorObject::Text(t) = &leaf.object else {
        return None;
    };
    (leaf.containment.last()?.num == object).then_some(t)
}

/// The glyph's CTM as a [`Matrix`].
fn glyph_ctm(g: &GlyphProvenance) -> Matrix {
    let [a, b, c, d, e, f] = g.ctm.map(f64::from);
    Matrix::new(a, b, c, d, e, f)
}

/// Coefficient-wise equality within [`CTM_EPSILON`], scaled by magnitude.
fn ctm_eq(x: Matrix, y: Matrix) -> bool {
    [
        (x.a, y.a),
        (x.b, y.b),
        (x.c, y.c),
        (x.d, y.d),
        (x.e, y.e),
        (x.f, y.f),
    ]
    .iter()
    .all(|(p, q)| (p - q).abs() <= CTM_EPSILON * (1.0 + p.abs().max(q.abs())))
}
