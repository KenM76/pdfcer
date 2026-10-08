//! A form widget turned to any angle (pdfcer-gui request G160), and a
//! widget's constant opacity `/CA`.
//!
//! `/MK /R` can only state a multiple of 90 (ISO 32000-1 §12.5.6.19 Table
//! 189), so a free angle lives in the appearance stream's `/Matrix` (§8.10.1
//! Table 95) on top of the quarter turn, and `/Rect` becomes the upright bound
//! of the turned artwork (§12.5.5). The angle is read back from that
//! `/Matrix`, so no private key records it.

use super::{
    CommandKind, EditError, EditSession, ObjectWrite, WidgetEdit, read_matrix, read_rect_array,
    transformed_box_bound,
};
use crate::annot_author::CheckBoxStateAppearance;
use crate::forms;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree;
use crate::vector::geometry::{Matrix, Point};

/// Below this, two angles in degrees are the same angle.
const ANGLE_EPS: f64 = 1e-6;

/// What [`EditSession::turn_widget`] changed.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct WidgetTurn {
    /// The field's fully-qualified name, echoed.
    pub name: String,
    /// The widget index, echoed.
    pub index: usize,
    /// The free angle before, counterclockwise degrees in (-180, 180], on top
    /// of `/MK /R`. `0.0` for a widget never turned.
    pub was: f64,
    /// The free angle now, the same convention.
    pub now: f64,
    /// `/Rect` before.
    pub rect_before: Option<page_tree::Rect>,
    /// `/Rect` after: the upright bound of the turned appearance, centred
    /// where the old one was.
    pub rect_after: Option<page_tree::Rect>,
    /// `false` when the widget was already at the asked angle and nothing was
    /// written (no undo entry either).
    pub changed: bool,
    /// Off-canvas disclosures: what a regenerating viewer drops, an
    /// appearance that cannot be redrawn turned, a shared stream copied.
    pub disclosures: Vec<String>,
}

/// A widget's free turn as read from its normal appearance.
#[derive(Debug, Clone, Copy)]
pub(super) struct FreeTurn {
    /// Counterclockwise degrees on top of the quarter turn, in (-180, 180].
    pub(super) degrees: f64,
    /// The widget's logical size in the `/Rect` frame (before the quarter
    /// turn's width/height swap): what `/Rect` would be with no free angle.
    pub(super) w: f64,
    pub(super) h: f64,
}

/// `degrees` reduced into (-180, 180].
fn normalise(degrees: f64) -> f64 {
    let r = degrees.rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

/// Whether `degrees` is a multiple of 90 (zero included).
fn is_quarter(degrees: f64) -> bool {
    (degrees / 90.0 - (degrees / 90.0).round()).abs() * 90.0 < ANGLE_EPS
}

/// Whether `m`'s linear part is a pure rotation (orthonormal, no mirror).
fn is_rotation(m: Matrix) -> bool {
    (m.a - m.d).abs() < 1e-6 && (m.b + m.c).abs() < 1e-6 && (m.a.hypot(m.b) - 1.0).abs() < 1e-6
}

/// `v` with float noise around 0 and ±1 removed, so a turn back to 0 writes
/// the quarter-turn matrix the redraw path writes.
fn snap(v: f64) -> f64 {
    for t in [0.0, 1.0, -1.0] {
        if (v - t).abs() < 1e-9 {
            return t;
        }
    }
    v
}

/// The linear part of a rotation by `degrees`, as a `/Matrix`.
fn rotation_matrix(degrees: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [snap(c), snap(s), snap(-s), snap(c), 0.0, 0.0]
}

fn matrix_object(m: [f64; 6]) -> Object {
    Object::Array(m.iter().map(|v| Object::Real(*v)).collect())
}

/// Section 12.5.5 step (b): the matrix fitting `bound` onto `rect`.
fn fit(bound: [f64; 4], rect: page_tree::Rect) -> Option<Matrix> {
    let sx = rect.width() / (bound[2] - bound[0]);
    let sy = rect.height() / (bound[3] - bound[1]);
    (sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0).then(|| {
        Matrix::new(
            sx,
            0.0,
            0.0,
            sy,
            rect.llx - sx * bound[0],
            rect.lly - sy * bound[1],
        )
    })
}

/// One appearance stream's planned rewrite.
struct Turned {
    id: ObjId,
    stream: Stream,
    /// The full placement after the turn: BBox through this lands on the
    /// page where the turned artwork belongs.
    placed: Matrix,
    bbox: [f64; 4],
}

impl WidgetEdit {
    /// Set the widget's constant opacity `/CA` (ISO 32000-2 Table 166),
    /// `0.0` transparent to `1.0` opaque. Refused outside that range.
    #[must_use]
    pub const fn with_opacity(mut self, opacity: f64) -> Self {
        self.opacity = Some(Some(opacity));
        self
    }

    /// Remove `/CA`, so the widget draws opaque (the default).
    #[must_use]
    pub const fn clearing_opacity(mut self) -> Self {
        self.opacity = Some(None);
        self
    }
}

impl EditSession {
    /// Write or remove `/CA` on a widget dictionary; the disclosure when the
    /// result is translucent.
    pub(super) fn write_widget_opacity(
        updated: &mut Dict,
        opacity: Option<Option<f64>>,
    ) -> Option<String> {
        match opacity? {
            None => {
                updated.remove(b"CA");
                None
            }
            Some(v) => {
                updated.insert(Name::from(b"CA"), Object::Real(v));
                (v < 1.0).then(|| {
                    "a strict PDF 2.0 reader may ignore a widget's /CA when it draws the appearance stream, so this field can look opaque there; pdfcer applies it"
                        .to_owned()
                })
            }
        }
    }

    /// The stream ids in a widget's `/AP` `/N`, `/R` and `/D`, normal first,
    /// each once.
    fn appearance_stream_ids(&self, widget_id: ObjId) -> Vec<ObjId> {
        let graph = self.graph();
        let Some(Object::Dict(dict)) = self.value(widget_id) else {
            return Vec::new();
        };
        let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| graph.resolve(o).clone()) else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        let mut take = |o: &Object| {
            if let Object::Reference(id) = o
                && self.resolves_to_stream(*id)
                && !ids.contains(id)
            {
                ids.push(*id);
            }
        };
        for key in [b"N", b"R", b"D"] {
            match ap.get(key) {
                Some(o @ Object::Reference(_)) if !matches!(graph.resolve(o), Object::Dict(_)) => {
                    take(o);
                }
                Some(o) => {
                    if let Object::Dict(states) = graph.resolve(o) {
                        states.iter().for_each(|(_, v)| take(v));
                    }
                }
                None => {}
            }
        }
        ids
    }

    /// The widget's free turn, or `None` when its normal appearance is
    /// upright in its quarter-turn frame (or is not a pure rotation, or has
    /// no `/BBox`). A free angle that is itself a multiple of 90 is not a
    /// free turn: that is a producer's quarter turn, and `/MK /R` governs it.
    pub(super) fn free_turn(&self, widget: &forms::Widget) -> Option<FreeTurn> {
        let id = *self.appearance_stream_ids(widget.id).first()?;
        let Some(Object::Stream(stream)) = self.value(id) else {
            return None;
        };
        let graph = self.graph();
        let m = read_matrix(&graph, &stream.dict);
        let bbox = read_rect_array(&graph, stream.dict.get(b"BBox")?)?;
        if !is_rotation(m) {
            return None;
        }
        let quarter = Self::quarter_of(widget.rotation);
        // Rounded so a 30 written comes back 30, not 29.999999999999996.
        let free = (normalise(m.b.atan2(m.a).to_degrees() - quarter as f64) * 1e9).round() / 1e9;
        if is_quarter(free) {
            return None;
        }
        let (bw, bh) = ((bbox[2] - bbox[0]).abs(), (bbox[3] - bbox[1]).abs());
        let (w, h) = if quarter == 90 || quarter == 270 {
            (bh, bw)
        } else {
            (bw, bh)
        };
        Some(FreeTurn {
            degrees: free,
            w,
            h,
        })
    }

    /// The `/Rect`-frame size a redraw of a turned widget draws at, instead
    /// of `/Rect`'s (which bounds the turned artwork).
    pub(super) fn turned_size(&self, widget: &forms::Widget) -> Option<(f64, f64)> {
        self.free_turn(widget).map(|t| (t.w, t.h))
    }

    /// The `/Matrix` a redrawn appearance carries: the quarter turn alone, or
    /// the quarter turn plus the widget's free angle. `None` for upright.
    pub(super) fn redraw_matrix(&self, widget: &forms::Widget, quarter: i64) -> Option<[f64; 6]> {
        match self.free_turn(widget) {
            Some(t) => Some(rotation_matrix(quarter as f64 + t.degrees)),
            None => Self::quarter_turn_matrix(quarter),
        }
    }

    /// Give freshly built button states a turned widget's `/Matrix`.
    pub(super) fn apply_free_turn(
        &self,
        widget: &forms::Widget,
        quarter: i64,
        states: &mut [CheckBoxStateAppearance],
    ) {
        if self.free_turn(widget).is_none() {
            return;
        }
        if let Some(m) = self.redraw_matrix(widget, quarter) {
            for state in states {
                state
                    .ap_dict
                    .insert(Name::from(b"Matrix"), matrix_object(m));
            }
        }
    }

    /// Refuse an operation that cannot keep a widget's free angle.
    pub(super) fn refuse_if_turned(
        &self,
        fqn: &str,
        index: usize,
        widget: &forms::Widget,
        operation: &'static str,
    ) -> Result<(), EditError> {
        match self.free_turn(widget) {
            Some(t) => Err(EditError::WidgetTurned {
                name: fqn.to_owned(),
                index,
                degrees: t.degrees,
                operation,
            }),
            None => Ok(()),
        }
    }

    /// Turn one widget to `degrees` counterclockwise on top of its `/MK /R`
    /// quarter turn (pdfcer-gui request G160).
    ///
    /// `degrees` is ABSOLUTE: `turn_widget(.., 30.0)` twice leaves the widget
    /// at 30, and `0.0` stands it back up. Reduced into (-180, 180].
    ///
    /// Every appearance stream (`/N`, `/R`, `/D`, each state) gets the turn
    /// composed into its `/Matrix`, and `/Rect` becomes the upright bound of
    /// the turned normal appearance, centred where the old box was
    /// (§12.5.5). Nothing is redrawn, so a foreign appearance turns too. A
    /// stream another annotation shares is copied first so the other one
    /// stays put. Later redraws (fill, restyle, a caption) keep the angle.
    ///
    /// # Disclosures
    ///
    /// A viewer that regenerates appearances from `/MK` keeps only the
    /// quarter turn; an appearance fitted to its box by a non-uniform scale
    /// is turned exactly but a later pdfcer redraw stands it upright; a
    /// shared stream was copied.
    ///
    /// # Errors
    ///
    /// - [`EditError::ResizeFactorInvalid`] — `degrees` not finite.
    /// - [`EditError::WidgetTurnIsQuarterTurn`] — a non-zero multiple of 90:
    ///   that is [`Self::rotate_widget`]'s `/MK /R`.
    /// - [`EditError::WidgetRectMissing`], [`EditError::WidgetTurnNeedsAppearance`].
    /// - [`EditError::WidgetTurnSharedAppearance`] — a shared stream in an
    ///   encrypted file, which cannot be copied under a new object number.
    /// - [`EditError::FieldNotFound`] / [`EditError::WidgetIndexOutOfRange`],
    ///   and the encryption and certification guards.
    ///
    /// ```
    /// # use pdfcer_core::edit::{EditError, EditSession};
    /// # fn turn(s: &mut EditSession) -> Result<(), EditError> {
    /// let turn = s.turn_widget("Name", 0, 30.0)?;
    /// for note in &turn.disclosures {
    ///     eprintln!("{note}");
    /// }
    /// # Ok(()) }
    /// ```
    pub fn turn_widget(
        &mut self,
        fqn: &str,
        index: usize,
        degrees: f64,
    ) -> Result<WidgetTurn, EditError> {
        if !degrees.is_finite() {
            return Err(EditError::ResizeFactorInvalid {
                axis: "degrees",
                value: degrees,
            });
        }
        let now = normalise(degrees);
        if is_quarter(now) && now.abs() > ANGLE_EPS {
            return Err(EditError::WidgetTurnIsQuarterTurn { degrees });
        }
        let now = if now.abs() <= ANGLE_EPS { 0.0 } else { now };
        let (field, ()) = self.deletion_preflight(fqn)?;
        let Some(widget) = field.widgets.get(index).cloned() else {
            return Err(EditError::WidgetIndexOutOfRange {
                name: fqn.to_owned(),
                index,
                widgets: field.widgets.len(),
            });
        };
        let Some(rect) = widget.rect else {
            return Err(EditError::WidgetRectMissing {
                name: fqn.to_owned(),
                index,
            });
        };
        let was = self.free_turn(&widget).map_or(0.0, |t| t.degrees);
        let mut out = WidgetTurn {
            name: fqn.to_owned(),
            index,
            was,
            now,
            rect_before: Some(rect),
            rect_after: Some(rect),
            changed: false,
            disclosures: Vec::new(),
        };
        let pivot = Point::new((rect.llx + rect.urx) / 2.0, (rect.lly + rect.ury) / 2.0);
        let turn = Matrix::rotate((now - was).to_radians()).about(pivot);
        let plan = self.plan_turn(&widget, rect, turn, fqn, index)?;
        if (now - was).abs() <= ANGLE_EPS {
            return Ok(out);
        }
        let Some(bound) = plan
            .first()
            .and_then(|t| transformed_box_bound(t.bbox, t.placed))
        else {
            return Err(EditError::WidgetTurnNeedsAppearance {
                name: fqn.to_owned(),
                index,
            });
        };
        let rect_after = page_tree::Rect::from_corners(bound[0], bound[1], bound[2], bound[3]);
        let objects = self.write_turn(&widget, plan, rect_after, &mut out.disclosures)?;
        self.commit(super::Command {
            kind: CommandKind::TurnWidget,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        if now != 0.0 {
            out.disclosures.push(
                "a viewer or tool that regenerates this field's appearance from /MK keeps only \
                 the quarter-turn /MK /R; the free angle is in the appearance /Matrix and such a \
                 regeneration drops it"
                    .to_owned(),
            );
        }
        out.rect_after = Some(rect_after);
        out.changed = true;
        Ok(out)
    }

    /// Each appearance stream with its turned placement. Refuses a widget
    /// with no normal appearance, and a shared stream in an encrypted file.
    fn plan_turn(
        &self,
        widget: &forms::Widget,
        rect: page_tree::Rect,
        turn: Matrix,
        fqn: &str,
        index: usize,
    ) -> Result<Vec<Turned>, EditError> {
        let needs = || EditError::WidgetTurnNeedsAppearance {
            name: fqn.to_owned(),
            index,
        };
        let graph = self.graph();
        let mut plan = Vec::new();
        for id in self.appearance_stream_ids(widget.id) {
            let Some(Object::Stream(stream)) = self.value(id) else {
                continue;
            };
            let Some(bbox) = stream
                .dict
                .get(b"BBox")
                .and_then(|o| read_rect_array(&graph, o))
            else {
                continue;
            };
            let m = read_matrix(&graph, &stream.dict);
            let Some(a) = transformed_box_bound(bbox, m).and_then(|b| fit(b, rect)) else {
                continue;
            };
            plan.push(Turned {
                id,
                stream: stream.clone(),
                placed: m.post_concat(a).post_concat(turn),
                bbox,
            });
        }
        if plan.is_empty() {
            return Err(needs());
        }
        let shared = self.shared_appearance_ids(widget.id);
        if self.base.encryption().is_some() && plan.iter().any(|t| shared.contains(&t.id)) {
            return Err(EditError::WidgetTurnSharedAppearance {
                name: fqn.to_owned(),
                index,
            });
        }
        Ok(plan)
    }

    /// Appearance stream ids this widget shares with another form widget.
    fn shared_appearance_ids(&self, widget_id: ObjId) -> Vec<ObjId> {
        let mine = self.appearance_stream_ids(widget_id);
        let Some(form) = forms::parse_acroform(&self.graph()) else {
            return Vec::new();
        };
        let mut shared = Vec::new();
        for other in form.fields.iter().flat_map(|f| &f.widgets) {
            if other.id == widget_id {
                continue;
            }
            for id in self.appearance_stream_ids(other.id) {
                if mine.contains(&id) && !shared.contains(&id) {
                    shared.push(id);
                }
            }
        }
        shared
    }

    /// The writes for a planned turn: each stream with its new `/Matrix`
    /// (copied when shared), and the widget's `/Rect` (and `/AP` when a copy
    /// was made).
    fn write_turn(
        &mut self,
        widget: &forms::Widget,
        plan: Vec<Turned>,
        rect_after: page_tree::Rect,
        disclosures: &mut Vec<String>,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let shared = self.shared_appearance_ids(widget.id);
        let mut objects = Vec::new();
        let mut renamed: Vec<(ObjId, ObjId)> = Vec::new();
        let mut skewed = false;
        for t in plan {
            let lin = Matrix::new(t.placed.a, t.placed.b, t.placed.c, t.placed.d, 0.0, 0.0);
            skewed |= !is_rotation(lin);
            let m = [snap(lin.a), snap(lin.b), snap(lin.c), snap(lin.d), 0.0, 0.0];
            let mut stream = t.stream;
            if m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
                stream.dict.remove(b"Matrix");
            } else {
                stream.dict.insert(Name::from(b"Matrix"), matrix_object(m));
            }
            let id = if shared.contains(&t.id) {
                let fresh = ObjId::new(self.alloc_number()?, 0);
                renamed.push((t.id, fresh));
                fresh
            } else {
                t.id
            };
            objects.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(Object::Stream(stream)),
            });
        }
        let Some(Object::Dict(dict)) = self.value(widget.id) else {
            return Err(EditError::NotADictionary {
                id: widget.id,
                key: "Rect",
            });
        };
        let mut updated = dict.clone();
        updated.insert(
            Name::from(b"Rect"),
            Object::Array(
                [
                    rect_after.llx,
                    rect_after.lly,
                    rect_after.urx,
                    rect_after.ury,
                ]
                .iter()
                .map(|v| Object::Real(*v))
                .collect(),
            ),
        );
        if !renamed.is_empty() {
            let ap = self.deref_dict(updated.get(b"AP")).unwrap_or_default();
            updated.insert(Name::from(b"AP"), Object::Dict(self.repoint(&ap, &renamed)));
            disclosures.push(format!(
                "{} appearance stream(s) were shared with another widget; this widget now has \
                 its own copy, so the other one does not turn",
                renamed.len()
            ));
        }
        if skewed {
            disclosures.push(
                "this appearance was fitted to its box by a non-uniform scale; it is turned \
                 exactly, but a later pdfcer redraw (a fill or a restyle) will draw it upright"
                    .to_owned(),
            );
        }
        objects.push(ObjectWrite {
            id: widget.id,
            before: self.state.get(&widget.id).cloned(),
            after: Some(Object::Dict(updated)),
        });
        Ok(objects)
    }

    /// `ap` with every reference in `renamed` replaced, one state level deep.
    fn repoint(&self, ap: &Dict, renamed: &[(ObjId, ObjId)]) -> Dict {
        let swap = |o: &Object| match o {
            Object::Reference(id) => renamed
                .iter()
                .find(|(old, _)| old == id)
                .map_or_else(|| o.clone(), |(_, new)| Object::Reference(*new)),
            _ => o.clone(),
        };
        let mut out = Dict::new();
        for (key, value) in ap.iter() {
            let next = match self.graph().resolve(value) {
                Object::Dict(states) => {
                    let mut d = Dict::new();
                    for (k, v) in states.iter() {
                        d.insert(k.clone(), swap(v));
                    }
                    Object::Dict(d)
                }
                _ => swap(value),
            };
            out.insert(key.clone(), next);
        }
        out
    }
}
