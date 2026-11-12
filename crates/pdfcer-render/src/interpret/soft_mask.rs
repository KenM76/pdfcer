//! The `gs /SMask` soft mask (ISO 32000-1 §11.6.5): the mask group run
//! into a page-sized buffer and reduced to per-pixel coverage, and on an
//! export recording also recorded as a [`MaskSource`].

use std::sync::Arc;

use pdfcer_core::content::ContentChunks;
use pdfcer_core::function::PdfFunction;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap};

use super::{Diagnostics, Interpreter, intersect_clip, matrix_entry64, rect_entry, run_nested};
use crate::canvas::Canvas;
use crate::compositor::BlendSpace;
use crate::display_list::Op;
use crate::gstate::{GraphicsState, Rgb};
use crate::mask_source::{MaskSource, transfer_table};

/// A built soft mask, and on an export recording what it was built from.
pub(super) type BuiltMask = (Mask, Option<Arc<MaskSource>>);

/// The mask group's stream, read once for both runs of it.
struct MaskGroup<'g> {
    dict: &'g Dict,
    raw: &'g [u8],
    resources: &'g Dict,
    space: BlendSpace,
}

impl Interpreter<'_> {
    /// Build the soft mask a `gs` `/SMask` dictionary describes, or
    /// [`None`] if it cannot be built.
    ///
    /// * **§11.5.3** — a `/Luminosity` mask composites its group `/G`
    ///   over a fully opaque backdrop of `/BC`, `C = (1 − α_g)·BC +
    ///   α_g·C_g`, then takes `0.30 R + 0.59 G + 0.11 B` of it (NOTE 3:
    ///   device weights, no gamma). The group itself is isolated (Table
    ///   147 makes its `/CS` Required), so `/BC` is the backdrop of the
    ///   group, never of the objects inside it.
    /// * **Table 144** — `/BC` defaults to black *as a colour*: all-zero
    ///   components are black in RGB and Gray but white in `DeviceCMYK`.
    ///   `/TR` is applied last, once, to the one scalar; a function that
    ///   is not one-in one-out is counted
    ///   ([`Diagnostics::soft_mask_transfer_ignored`]) and the mask used
    ///   untransformed.
    /// * **§11.6.5.2** — outside the `/BBox` the value is `TR(lum(BC))`
    ///   or `TR(0)`: the group paints nothing there, so the composite
    ///   gives it without a case of its own. The mask's space is
    ///   `/Matrix` × the CTM when `gs` runs, which building it here, in
    ///   the `gs` handler, makes true by construction.
    pub(super) fn build_soft_mask(
        &mut self,
        sm: &Dict,
        canvas: &mut Canvas<'_>,
    ) -> Option<BuiltMask> {
        let doc = self.doc;
        let luminosity = match doc.resolve(sm.get(b"S")?) {
            Object::Name(n) if n.as_bytes() == b"Luminosity" => true,
            Object::Name(n) if n.as_bytes() == b"Alpha" => false,
            _ => return None,
        };
        let transfer = self.mask_transfer(sm);
        let Object::Stream(stream) = doc.resolve(sm.get(b"G")?) else {
            return None;
        };
        let g_dict = stream.dict.clone();
        let raw = doc.slice(stream.data_span)?;
        let mut content = ContentChunks::from_stream(&g_dict, raw);
        content.prime().ok()?;
        let backdrop = luminosity.then(|| self.soft_mask_backdrop(&g_dict, sm));
        let resources = self.mask_resources(&g_dict);
        let group = MaskGroup {
            dict: &g_dict,
            raw,
            resources,
            space: self.mask_space(&g_dict, resources),
        };
        let mut buf = Pixmap::new(canvas.width(), canvas.height())?;
        // The `/BBox` clip is built on the buffer's own canvas: on a
        // recording canvas it would become a clip definition the buffer's
        // paint never consults.
        if let Some(inner) = self.mask_group_state(&g_dict, &mut Canvas::paint(&mut buf)) {
            let nested = self.run_mask_group(&group, content, inner, &mut Canvas::paint(&mut buf));
            self.diag.merge(nested);
        }
        composite_over_backdrop(&mut buf, backdrop);
        let table = transfer.as_deref().map(transfer_table);
        let mask = mask_from_buffer(&buf, luminosity, table.as_ref())?;
        let source = self.record_mask_group(&group, canvas).map(|ops| {
            Arc::new(MaskSource {
                ops,
                luminosity,
                backdrop: backdrop.map_or([0.0; 3], |c| [c.red(), c.green(), c.blue()]),
                transfer,
                subtractive: group.space == BlendSpace::Subtractive,
            })
        });
        Some((mask, source))
    }

    /// `/TR` as a one-in one-out function; `None` for `/Identity`, an
    /// absent entry, or (counted) anything else.
    fn mask_transfer(&mut self, sm: &Dict) -> Option<Arc<PdfFunction>> {
        let doc = self.doc;
        let obj = sm.get(b"TR")?;
        match doc.resolve(obj) {
            Object::Null => return None,
            Object::Name(n) if n.as_bytes() == b"Identity" => return None,
            _ => {}
        }
        let f = PdfFunction::load(doc, obj)
            .ok()
            .filter(|f| f.inputs() == 1 && f.outputs() == 1);
        if f.is_none() {
            self.diag.soft_mask_transfer_ignored += 1;
            self.diag
                .note(b"gs /SMask /TR: not a one-in one-out function; mask used untransformed");
        }
        f.map(Arc::new)
    }

    /// The group's own `/Resources`, else the resources the `gs` ran
    /// under.
    fn mask_resources<'g>(&self, g_dict: &'g Dict) -> &'g Dict
    where
        Self: 'g,
    {
        match g_dict
            .get(b"Resources")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)
            .filter(|d| !d.is_empty())
        {
            Some(own) => own,
            None => self.inherited_resources(),
        }
    }

    /// The group's blending space, from its own `/Group /CS` (Table 147:
    /// Required, the group being rootless); additive when absent.
    fn mask_space(&mut self, g_dict: &Dict, resources: &Dict) -> BlendSpace {
        let doc = self.doc;
        g_dict
            .get(b"Group")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|g| g.get(b"CS"))
            .and_then(|cs| {
                crate::color::resolve_object(
                    doc,
                    doc.resolve(cs),
                    resources,
                    0,
                    &mut self.diag.color,
                )
            })
            .map_or(BlendSpace::Additive, |sp| BlendSpace::of(&sp))
    }

    /// The group's initial state: §11.6.6's defaults (Normal, alpha 1,
    /// no mask, so the mask is not applied to its own construction), the
    /// CTM at `gs` time, then `/Matrix`, then the `/BBox` clip in that
    /// space (§8.10.2 (b)). `None` for a degenerate `/BBox`, which paints
    /// nothing.
    fn mask_group_state(
        &mut self,
        g_dict: &Dict,
        canvas: &mut Canvas<'_>,
    ) -> Option<GraphicsState> {
        let doc = self.doc;
        let mut inner = GraphicsState::default_with_ctm(self.gs.current.ctm);
        if let Some(m) = matrix_entry64(doc, g_dict) {
            inner.set_ctm64(m.post_concat(inner.ctm64));
        }
        if let Some(rect) = rect_entry(doc, g_dict, b"BBox") {
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                return None;
            }
            let path = PathBuilder::from_rect(rect);
            let form_ctm = inner.ctm;
            intersect_clip(
                &mut inner,
                &path,
                FillRule::Winding,
                form_ctm,
                canvas,
                &mut self.clip_cache,
                self.edge_aa,
            );
        }
        Some(inner)
    }

    /// Run the mask group's content onto `canvas` from `inner`.
    fn run_mask_group(
        &mut self,
        g: &MaskGroup<'_>,
        content: ContentChunks<'_>,
        inner: GraphicsState,
        canvas: &mut Canvas<'_>,
    ) -> Diagnostics {
        run_nested(
            self.doc,
            content.into(),
            g.resources,
            self.page_resources,
            self.fonts,
            inner,
            canvas,
            self.depth + 1,
            // A real copy of the recursion guard, so a form reached
            // through its own soft mask is still caught.
            self.active.clone(),
            self.cancel,
            self.budget,
            self.policy,
            false,
            g.space,
            None,
        )
    }

    /// On an export recording, the mask group recorded as ops beside the
    /// page, its diagnostics discarded (the paint run counted them).
    fn record_mask_group(&mut self, g: &MaskGroup<'_>, canvas: &mut Canvas<'_>) -> Option<Vec<Op>> {
        if !canvas.records_export() {
            return None;
        }
        let Some(inner) = self.mask_group_state(g.dict, canvas) else {
            return Some(Vec::new());
        };
        let mut content = ContentChunks::from_stream(g.dict, g.raw);
        content.prime().ok()?;
        canvas.record_aside(|c| {
            let _ = self.run_mask_group(g, content, inner, c);
        })
    }

    /// The opaque backdrop colour a `/Luminosity` mask composites over:
    /// `/BC` in the group's space, black when absent or unreadable.
    ///
    /// Read by component count rather than by loading `/CS`: four are
    /// subtractive, so `[0 0 0 1]` is black and `[0 0 0 0]` white; one and
    /// three are additive. CMYK goes through the same conversion the
    /// paint path uses, because outside the `/BBox` the backdrop's
    /// magnitude is the mask value (§11.6.5.2).
    fn soft_mask_backdrop(&mut self, g_dict: &Dict, sm: &Dict) -> tiny_skia::Color {
        let doc = self.doc;
        let black = tiny_skia::Color::BLACK;
        let Some(arr) = sm
            .get(b"BC")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_array)
        else {
            return black;
        };
        #[allow(clippy::cast_possible_truncation)] // Colour components.
        let comps: Vec<f32> = arr
            .iter()
            .filter_map(|o| doc.resolve(o).as_number())
            .map(|v| v as f32)
            .collect();
        if comps.is_empty() {
            return black;
        }
        let has_cs = g_dict
            .get(b"Group")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .is_some_and(|g| g.contains_key(b"CS"));
        if !has_cs {
            self.diag.tolerated += 1;
            self.diag
                .note(b"gs /SMask /BC without group /CS (Required, Table 147)");
        }
        let Rgb { r, g, b } = match *comps.as_slice() {
            [v] => Rgb::from_gray(v),
            [r, g, b] => Rgb::from_rgb(r, g, b),
            [c, m, y, k] => Rgb::from_cmyk(self.policy.cmyk_intent, c, m, y, k),
            _ => {
                self.diag.tolerated += 1;
                self.diag
                    .note(b"gs /SMask /BC with unhandled component count - assuming black");
                return black;
            }
        };
        tiny_skia::Color::from_rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), 1.0)
            .unwrap_or(black)
    }
}

/// §11.5.3's outer composite, `C = (1 − α_g)·BC + α_g·C_g`: the group's
/// buffer drawn `SourceOver` onto one filled with `/BC`. An alpha mask has
/// no backdrop colour, so `None` leaves the buffer alone.
fn composite_over_backdrop(buf: &mut Pixmap, backdrop: Option<tiny_skia::Color>) {
    let Some(colour) = backdrop else { return };
    let Some(mut base) = Pixmap::new(buf.width(), buf.height()) else {
        return;
    };
    base.fill(colour);
    base.draw_pixmap(
        0,
        0,
        buf.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    *buf = base;
}

/// A composited mask-group buffer as per-pixel coverage: luminosity of the
/// un-premultiplied colour, or alpha, then `/TR` through `table`.
fn mask_from_buffer(buf: &Pixmap, luminosity: bool, table: Option<&[u8; 256]>) -> Option<Mask> {
    let mut mask = Mask::new(buf.width(), buf.height())?;
    let data = mask.data_mut();
    for (i, px) in buf.pixels().iter().enumerate() {
        let a = f32::from(px.alpha()) / 255.0;
        let v = if !luminosity {
            a
        } else if a <= 0.0 {
            0.0
        } else {
            let c = |v: u8| f32::from(v) / 255.0 / a;
            0.30_f32.mul_add(
                c(px.red()),
                0.59_f32.mul_add(c(px.green()), 0.11 * c(px.blue())),
            )
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped first.
        let q = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        *data.get_mut(i)? = table.map_or(q, |t| t.get(usize::from(q)).copied().unwrap_or(q));
    }
    Some(mask)
}
