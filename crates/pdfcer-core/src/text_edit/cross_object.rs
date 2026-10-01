//! Text edits whose match crosses text-object boundaries (`ET … BT`) on one
//! baseline.
//!
//! Word writes each fragment of a visual line as its own text object, in its
//! own `q … Q` and marked-content sequence, and ends the line with a
//! space-only object. A pinned spanning request
//! ([`EditRequest::spanning_from`](super::EditRequest::spanning_from)) may
//! continue into the next text object when its first show operator continues
//! the line ([`continues_line`]). An unpinned request never crosses: without
//! the operator saying where, two objects on one baseline may be unrelated
//! table cells.
//!
//! The replacement goes into the FIRST operator of the match, in place. The
//! later operators lose their matched glyphs, and every `BT`/`ET`, `q`/`Q`
//! and `BDC`/`EMC` stays where it was, so structure-tree MCIDs still
//! resolve. The last operator's unmatched tail is placed with a `TJ` number:
//! after the replacement (reflow) or where it was (pin). Later text objects
//! are positioned absolutely and are not moved.

use super::edit::{
    FOLLOWER_ORIGIN_EPSILON, FollowerDisposition, MatchRun, OpRec, Rec, SPAN_H_SCALE_TOLERANCE,
    ShowData, ShowOp, compensating_tj, emit_edited_operator, glyph_advance, line_x, round4,
    same_line,
};
use crate::text_extract::font::ExtractFont;
use crate::writer::content::emit_number;

/// The largest CTM component difference that still counts as "the same
/// CTM". Producers write the same `cm` operands for every fragment; this
/// absorbs only parse-and-multiply noise.
const CTM_TOLERANCE: f64 = 1e-6;

/// Whether `next`, the first show operator of a later text object, continues
/// the line `prev` is on, so a match may run from one into the other.
///
/// Everything that changes how the glyphs look must agree: font resource and
/// size, `Tc`, `Tw`, `Tz`, `Ts`, `Tr`, fill colour (and stroke colour when the
/// mode strokes), the CTM, and the text-space row. `next` must also not start
/// left of `prev`: text re-anchored backwards on the same row is different
/// text. The marked-content sequence may differ, because Word gives every
/// fragment its own MCID; [`disclosure`] says what that means.
pub(crate) fn continues_line(prev: &ShowData, next: &ShowData) -> bool {
    let (a, b) = (&prev.text_state, &next.text_state);
    let strokes = matches!(a.render_mode.value.round() as i64, 1 | 2 | 5 | 6);
    prev.matrix_known
        && next.matrix_known
        && matches!(next.op, ShowOp::Tj | ShowOp::TJ)
        && prev.font_name == next.font_name
        && prev.tf_size == next.tf_size
        && prev.tc() == next.tc()
        && prev.tw() == next.tw()
        && (prev.th() - next.th()).abs() <= SPAN_H_SCALE_TOLERANCE
        && a.rise.value == b.rise.value
        && a.render_mode.value == b.render_mode.value
        && prev.fill_color == next.fill_color
        && (!strokes || prev.stroke_color == next.stroke_color)
        && prev
            .ctm
            .iter()
            .zip(next.ctm.iter())
            .all(|(p, n)| (p - n).abs() <= CTM_TOLERANCE)
        && same_line(prev, &next.text_matrix)
        && line_x(&prev.text_matrix, &next.text_matrix) >= -FOLLOWER_ORIGIN_EPSILON
}

/// Whether the records `first..=last` include the end of a text object, i.e.
/// the span was joined across `ET`.
pub(crate) fn crosses(recs: &[OpRec], first: usize, last: usize) -> bool {
    recs.get(first..=last)
        .is_some_and(|rs| rs.iter().any(|r| matches!(r.rec, Rec::EndText)))
}

/// One later operator of a crossing match: its record index, the operator,
/// the matched part, and whether any of its text follows the match.
pub(crate) struct Later<'a> {
    pub(crate) index: usize,
    pub(crate) show: &'a ShowData,
    pub(crate) matched: &'a MatchRun,
    pub(crate) has_tail: bool,
}

/// The byte rewrites for the later operators, and the tail's displacement.
pub(crate) struct Emptied {
    pub(crate) edits: Vec<(usize, usize, Vec<u8>)>,
    /// How far the last operator's unmatched tail moved along the line, in
    /// text-space units; zero when there is no tail or under pin.
    pub(crate) delta: f64,
    /// Whether the last operator keeps text after the match.
    pub(crate) tail: bool,
    /// Operators left showing nothing.
    pub(crate) emptied: u64,
    /// Text objects after the first that the match reached.
    pub(crate) objects: u64,
    /// Whether the later operators sit under other MCIDs than the first.
    pub(crate) mcid_moved: bool,
}

/// Remove the matched glyphs from every later operator of a crossing match.
///
/// `end_x` is where the replacement ends, along `anchor`'s line in
/// text-space units from `anchor`'s origin. The last operator's tail, if
/// any, is placed by a `TJ` number: at `end_x` under reflow, or where it was
/// under pin.
pub(crate) fn empty_later(
    recs: &[OpRec],
    font: &ExtractFont,
    (anchor_index, anchor): (usize, &ShowData),
    later: &[Later<'_>],
    end_x: f64,
    disposition: FollowerDisposition,
) -> Emptied {
    let mut out = Emptied {
        edits: Vec::with_capacity(later.len()),
        delta: 0.0,
        tail: later.last().is_some_and(|l| l.has_tail),
        emptied: 0,
        objects: 0,
        mcid_moved: later.iter().any(|l| l.show.mcid != anchor.mcid),
    };
    if let Some(last) = later.last() {
        out.objects = recs.get(anchor_index..=last.index).map_or(0, |rs| {
            rs.iter().filter(|r| matches!(r.rec, Rec::EndText)).count() as u64
        });
    }
    for (n, l) in later.iter().enumerate() {
        let is_last = n + 1 == later.len();
        let pin_num = if is_last && l.has_tail {
            let removed: f64 = l
                .matched
                .old_codes
                .iter()
                .map(|&c| glyph_advance(font, c, l.show))
                .sum::<f64>()
                + l.matched.kern_advance;
            let origin = line_x(&anchor.text_matrix, &l.show.text_matrix);
            let shift = match disposition {
                FollowerDisposition::Reflow => round4(end_x - origin),
                FollowerDisposition::Pin => round4(removed),
            };
            out.delta = round4(shift - removed);
            compensating_tj(-shift, l.show.tf_size, l.show.th()).filter(|n| *n != 0.0)
        } else {
            None
        };
        let bytes = lead_with(emit_edited_operator(l.show, l.matched, &[], None), pin_num);
        if !l.has_tail {
            out.emptied += 1;
        }
        if let Some(r) = recs.get(l.index) {
            out.edits.push((r.start, r.end, bytes));
        }
    }
    out
}

/// Put a `TJ` number BEFORE an emitted show operator's glyphs, so it moves
/// the tail that remains (a later operator's match starts at its first
/// glyph). `(s) Tj` becomes `[n (s)] TJ`; `[…] TJ` becomes `[n …] TJ`.
fn lead_with(op: Vec<u8>, num: Option<f64>) -> Vec<u8> {
    let Some(n) = num else {
        return op;
    };
    let mut out = vec![b'['];
    emit_number(&mut out, n);
    out.push(b' ');
    if let Some(body) = op.strip_suffix(b" Tj") {
        out.extend_from_slice(body);
        out.extend_from_slice(b"] TJ");
    } else {
        out.extend_from_slice(op.strip_prefix(b"[").unwrap_or(&op));
    }
    out
}

/// The disclosure for a crossing edit.
pub(crate) fn disclosure(e: &Emptied, disposition: FollowerDisposition) -> String {
    let tail = match (e.tail, disposition) {
        (false, _) => String::new(),
        (true, FollowerDisposition::Reflow) => format!(
            ", the rest of the last one was moved to follow the replacement ({:.3} text-space units)",
            e.delta
        ),
        (true, FollowerDisposition::Pin) => {
            ", the rest of the last one kept its position".to_owned()
        }
    };
    let mcid = if e.mcid_moved {
        " The edited text now lives in the first fragment's marked-content sequence; the later \
         fragments' sequences are empty, so a structure element pointing only at them now reads \
         as nothing."
    } else {
        ""
    };
    format!(
        "span: the text was written across {} text objects on one line and was edited as ONE run \
         — the replacement went into the first one at the match's start, the matched glyphs were \
         removed from the later ones ({} left showing nothing, every BT/ET, q/Q and BDC/EMC kept \
         in place){tail}, and text in later text objects kept its position.{mcid}",
        e.objects + 1,
        e.emptied,
    )
}
