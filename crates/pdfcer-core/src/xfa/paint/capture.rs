//! A field's value drawn into its own appearance instead of the page
//! ([`FieldOutput::Fillable`](super::FieldOutput::Fillable)).

use crate::writer::content::ContentBuilder;
use crate::xfa::ContainerKind;

use super::{FieldOutput, LayoutItem, LayoutRect, Painter, XfaNode};

/// One field's value region as a widget appearance (ISO 32000-1 §12.5.5),
/// for the caller to write as a widget annotation.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PaintedWidget {
    /// The field's template path ([`LayoutItem::path`]).
    pub path: Vec<usize>,
    /// `[llx lly urx ury]` in the page's default user space: the widget's
    /// `/Rect` and its appearance's `/BBox` (identity `/Matrix`).
    pub rect: [f64; 4],
    /// The field's rotation, degrees counter-clockwise.
    pub rotate: u16,
    /// The appearance content, in page space: the value as the page would
    /// have drawn it, or a check button's on state.
    pub normal: Vec<u8>,
    /// A check button's off state.
    pub off: Option<Vec<u8>>,
    /// The value text's font resource name, size and RGB fill, for `/DA`;
    /// `None` for a check button.
    pub text: Option<(String, f64, [f64; 3])>,
    /// The field or an ancestor is not visible: nothing is drawn.
    pub hidden: bool,
}

/// Widgets a field's value can be written as (the rest stay on the page).
pub(super) fn writable(tag: &str) -> bool {
    matches!(
        tag,
        "textEdit" | "numericEdit" | "dateTimeEdit" | "passwordEdit" | "choiceList" | "signature"
    )
}

impl Painter<'_> {
    /// Whether `item`'s value goes into a widget.
    pub(super) fn captures(&self, item: &LayoutItem, node: &XfaNode) -> bool {
        self.opts.fields == FieldOutput::Fillable
            && item.kind == ContainerKind::Field
            && item.fragment.is_none()
            && {
                let tag = crate::xfa::widget::widget_tag(node);
                writable(tag) || tag == "checkButton"
            }
    }

    /// Run `draw` into a fresh appearance stream, under the rotation the
    /// page is drawing `item` with.
    pub(super) fn capture(&mut self, draw: impl FnOnce(&mut Self)) -> Vec<u8> {
        let page = std::mem::replace(&mut self.b, ContentBuilder::new());
        let dashed = std::mem::replace(&mut self.dashed, false);
        if let Some([a, b, c, d, e, f]) = self.turn {
            self.b.save_state();
            self.b.concat_matrix(a, b, c, d, e, f);
        }
        draw(self);
        if self.turn.is_some() {
            self.b.restore_state();
        }
        self.dashed = dashed;
        std::mem::replace(&mut self.b, page).into_bytes()
    }

    /// Record `item`'s widget over the value region `r`.
    pub(super) fn push_widget(
        &mut self,
        item: &LayoutItem,
        r: LayoutRect,
        normal: Vec<u8>,
        off: Option<Vec<u8>>,
        text: Option<(String, f64, [f64; 3])>,
    ) {
        let rect = self.page_rect(r);
        self.widgets.push(PaintedWidget {
            path: item.path.clone(),
            rect,
            rotate: item.rotate,
            normal,
            off,
            text,
            hidden: false,
        });
    }

    /// A field that is laid out but not drawn: a hidden widget keeps its value.
    pub(super) fn hidden_widget(&mut self, item: &LayoutItem) {
        if self.opts.fields == FieldOutput::Fillable
            && item.kind == ContainerKind::Field
            && item.fragment.is_none()
        {
            let rect = self.page_rect(item.rect);
            self.widgets.push(PaintedWidget {
                path: item.path.clone(),
                rect,
                rotate: item.rotate,
                hidden: true,
                ..PaintedWidget::default()
            });
        }
    }

    /// `r` (layout space) as a PDF rectangle, through the current rotation.
    fn page_rect(&self, r: LayoutRect) -> [f64; 4] {
        let (top, bottom) = (self.height - r.y, self.height - r.y - r.h);
        let corners = [
            (r.x, top),
            (r.x + r.w, top),
            (r.x, bottom),
            (r.x + r.w, bottom),
        ];
        let [a, b, c, d, e, f] = self.turn.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let pts = corners.map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for (x, y) in pts {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
        [lo.0, lo.1, hi.0, hi.1]
    }
}
