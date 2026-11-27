//! A `/FreeText` callout (ISO 32000-2 §12.5.6.6 Table 177; 32000-1 Table
//! 174): the `/CL` line from the annotated point to the text box, its `/LE`
//! ending at the start point (Table 179), and the `/RD` text box inside
//! `/Rect`.

use super::{
    Color, FreeTextFrame, LineEnding, emit_line_ending, line_ending_of, read_border_effect,
    read_name_value, read_numbers,
};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::page_tree::Rect;
use crate::writer::content::{ContentBuilder, LineCap, LineJoin, Paint};

/// A `/FreeText` callout line and the text box it leads to (§12.5.6.6,
/// `/IT /FreeTextCallout`). Coordinates are in default user space.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FreeTextCallout {
    /// `/CL`'s first point, where [`Self::ending`] is drawn: the point the
    /// callout annotates.
    pub start: (f64, f64),
    /// `/CL`'s knee, present in the six-number form.
    pub knee: Option<(f64, f64)>,
    /// `/CL`'s last point, at the text box.
    pub end: (f64, f64),
    /// `/LE`, the ending at [`Self::start`] (Table 179; default `None`).
    /// Styles outside [`LineEnding`] read as `None`.
    pub ending: LineEnding,
    /// `/RD`: how far the text box sits inside `/Rect` on the left, top,
    /// right and bottom (FT-R9; each ≥ 0). The box's fill, border and text
    /// are drawn in it; the callout line may cross the rest of `/Rect`.
    pub text_inset: [f64; 4],
}

impl FreeTextCallout {
    /// A callout from `start` (optionally through `knee`) to `end`, with
    /// `ending` at `start` and the text box `text_inset` inside `/Rect`.
    #[must_use]
    pub const fn new(
        start: (f64, f64),
        knee: Option<(f64, f64)>,
        end: (f64, f64),
        ending: LineEnding,
        text_inset: [f64; 4],
    ) -> Self {
        Self {
            start,
            knee,
            end,
            ending,
            text_inset,
        }
    }

    fn points(&self) -> impl Iterator<Item = (f64, f64)> {
        std::iter::once(self.start)
            .chain(self.knee)
            .chain(std::iter::once(self.end))
    }

    /// `/IT`, `/CL`, `/LE` (when not `None`) and `/RD` (when not all zero).
    pub(super) fn write_keys(&self, annot: &mut Dict) {
        annot.insert(
            Name::from(b"IT"),
            Object::Name(Name::from(b"FreeTextCallout")),
        );
        let cl = self
            .points()
            .flat_map(|(x, y)| [Object::Real(x), Object::Real(y)])
            .collect();
        annot.insert(Name::from(b"CL"), Object::Array(cl));
        if self.ending != LineEnding::None {
            annot.insert(
                Name::from(b"LE"),
                Object::Name(Name::from(self.ending.name())),
            );
        }
        if self.text_inset.iter().any(|v| *v != 0.0) {
            let rd = self.text_inset.iter().copied().map(Object::Real).collect();
            annot.insert(Name::from(b"RD"), Object::Array(rd));
        }
    }

    /// The line and its ending, stroked in `color` at `width`, in the
    /// appearance box whose origin is `/Rect`'s lower-left `origin`. The
    /// ending is stroked only: FreeText has no `/IC` (FT-R11).
    pub(super) fn paint(&self, origin: (f64, f64), color: Color, width: f64) -> Vec<u8> {
        let local = |(x, y): (f64, f64)| (x - origin.0, y - origin.1);
        let mut b = ContentBuilder::new();
        b.save_state();
        color.apply_stroke(&mut b);
        b.set_line_width(width);
        b.set_line_cap(LineCap::Round);
        b.set_line_join(LineJoin::Round);
        let mut points = self.points().map(local);
        if let Some((x, y)) = points.next() {
            b.move_to(x, y);
        }
        for (x, y) in points {
            b.line_to(x, y);
        }
        b.paint(Paint::Stroke);
        let shaft = local(self.knee.unwrap_or(self.end));
        emit_line_ending(&mut b, local(self.start), shaft, self.ending, width, false);
        b.restore_state();
        b.into_bytes()
    }
}

/// Where a `/FreeText`'s box and text go in its `[0 0 w h]` appearance box:
/// all of it, or for a callout the `/RD` inner rectangle (FT-R9), unless
/// that would leave no area.
pub(super) fn text_box(w: f64, h: f64, frame: &FreeTextFrame) -> Rect {
    let full = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };
    let Some(c) = &frame.callout else {
        return full;
    };
    let [left, top, right, bottom] = c.text_inset;
    let inner = Rect {
        llx: left,
        lly: bottom,
        urx: w - right,
        ury: h - top,
    };
    if inner.urx > inner.llx && inner.ury > inner.lly {
        inner
    } else {
        full
    }
}

/// `body`, drawn for a `[0 0 W H]` box, moved to `text_box`'s corner.
pub(super) fn placed(text_box: Rect, body: Vec<u8>) -> Vec<u8> {
    if text_box.llx == 0.0 && text_box.lly == 0.0 {
        return body;
    }
    let mut b = ContentBuilder::new();
    b.save_state();
    b.concat_matrix(1.0, 0.0, 0.0, 1.0, text_box.llx, text_box.lly);
    b.append_raw(&body);
    b.restore_state();
    b.into_bytes()
}

fn is_callout<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> bool {
    read_name_value(graph, annot, b"IT").as_deref() == Some(b"FreeTextCallout".as_slice())
}

/// `/LE`: a name (Table 177); a one-element-or-longer array is read by its
/// first element, the ending at `/CL`'s start.
fn ending_object<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> Option<Object> {
    match annot.get(b"LE").map(|o| graph.resolve(o))? {
        Object::Array(items) => items.first().map(|o| graph.resolve(o).clone()),
        other => Some(other.clone()),
    }
}

/// The callout a `/FreeText` states: `/IT /FreeTextCallout` with a four- or
/// six-number `/CL` (FT-R6, FT-R7). `/CL` without that intent is not a
/// callout ("meaningful only if IT is FreeTextCallout").
pub(super) fn read_callout<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
) -> Option<FreeTextCallout> {
    if !is_callout(graph, annot) {
        return None;
    }
    let cl = read_numbers(graph, annot, b"CL");
    if cl.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let (start, knee, end) = match cl.as_slice() {
        [x1, y1, x2, y2] => ((*x1, *y1), None, (*x2, *y2)),
        [x1, y1, x2, y2, x3, y3] => ((*x1, *y1), Some((*x2, *y2)), (*x3, *y3)),
        _ => return None,
    };
    let ending = line_ending_of(ending_object(graph, annot).as_ref());
    let rd = read_numbers(graph, annot, b"RD");
    let text_inset = match rd.as_slice() {
        [l, t, r, b] if rd.iter().all(|v| v.is_finite() && *v >= 0.0) => [*l, *t, *r, *b],
        _ => [0.0; 4],
    };
    Some(FreeTextCallout::new(start, knee, end, ending, text_inset))
}

/// The keys of a `/FreeText` that a pdfcer redraw keeps in the dictionary
/// but does not depict: a callout `/LE` style outside [`LineEnding`], a
/// callout `/CL` that is not four or six numbers, and a cloudy `/BE`
/// (pdfcer draws a FreeText's border straight).
pub(crate) fn keys_not_drawn<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> Vec<String> {
    let mut keys = Vec::new();
    if is_callout(graph, annot) {
        if read_callout(graph, annot).is_none() && annot.contains_key(b"CL") {
            keys.push("CL".to_owned());
        }
        let ending = ending_object(graph, annot);
        let drawn = line_ending_of(ending.as_ref()) != LineEnding::None;
        if !drawn && !ending_is_none(graph, annot) {
            keys.push("LE".to_owned());
        }
    }
    if read_border_effect(graph, annot).is_some() {
        keys.push("BE".to_owned());
    }
    keys
}

/// `/LE` absent or `/None`: nothing to draw, so nothing is lost.
fn ending_is_none<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> bool {
    match ending_object(graph, annot) {
        None => true,
        Some(Object::Name(n)) => n.as_bytes() == b"None",
        Some(_) => false,
    }
}
