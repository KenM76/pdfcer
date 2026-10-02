//! Interpreter side of `PatternType 1` tiling patterns (ISO 32000-1
//! §8.7.3): run the cell's content stream into a raster, then tile it
//! through the path × clip mask. Geometry and ceilings: `crate::tiling`.

use pdfcer_core::content::ContentStream;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object, Stream};
use tiny_skia::{FillRule, Mask, Path, PathStroker, Pixmap, Transform};

use super::{Interpreter, MAX_XOBJECT_DEPTH, clamp_region, run_nested};
use crate::canvas::Canvas;
use crate::device_clip::FitMask;
use crate::display_list::PoisonReason;
use crate::gstate::GraphicsState;
use crate::tiling::{self, CellPlan, NestGuard, PaintType, PlanRefusal, TilingPattern};

/// Device-space bounds `(left, top, right, bottom)`, clamped to the canvas.
type Region = (i32, i32, i32, i32);

/// What one pattern paint covers: the area in user space, how it reaches
/// the device, and which colour half it paints.
pub(super) struct PatternArea<'p> {
    pub path: &'p Path,
    pub rule: FillRule,
    pub ctm: Transform,
    pub stroking: bool,
}

impl Interpreter<'_> {
    /// Paint `area` with the tiling pattern `stream` (Table 75). `id` is the
    /// pattern's object number, for the cycle guard. Returns whether
    /// anything was painted; every refusal is counted in
    /// `patterns_unpainted` with a note.
    pub(super) fn paint_tiling_pattern(
        &mut self,
        id: Option<ObjId>,
        stream: &Stream,
        name: &[u8],
        area: &PatternArea<'_>,
        canvas: &mut Canvas<'_>,
    ) -> bool {
        let pattern = match TilingPattern::parse(self.doc, &stream.dict) {
            Ok(p) => p,
            Err(why) => return self.tiling_refused(name, why),
        };
        if self.oc_hidden() || crate::profile::skip_paint() {
            return false;
        }
        // §8.7.2: pattern space → the parent stream's default space
        // (`/Matrix`) → device (`base_ctm`), whatever the CTM is now.
        let to_device = self.base_ctm.pre_concat(pattern.matrix);
        let plan = match tiling::plan(&pattern, to_device) {
            Ok(p) => p,
            Err(PlanRefusal::Degenerate) => {
                return self.tiling_refused(name, "non-invertible pattern matrix");
            }
            Err(PlanRefusal::TooManyCopies(n)) => {
                let why = format!("{n} /BBox copies per step exceed the ceiling");
                return self.tiling_refused(name, &why);
            }
        };
        let Some((mask, region)) = self.pattern_area_mask(area, canvas) else {
            return false;
        };
        if region.0 >= region.2 || region.1 >= region.3 {
            return false;
        }
        let Some(cell) = self.render_tile_cell(id, stream, &pattern, &plan, area.stroking, name)
        else {
            return false;
        };
        self.composite_tiles(&cell, &plan, &mask, region, area.stroking, canvas)
    }

    fn tiling_refused(&mut self, name: &[u8], why: &str) -> bool {
        self.diag.color.patterns_unpainted += 1;
        self.diag.color.note(&format!(
            "scn /{}: tiling pattern not painted: {why}",
            String::from_utf8_lossy(name)
        ));
        false
    }

    /// Run the pattern's content stream into its `/BBox` raster and fold
    /// that into one period cell. The stream starts from a default graphics
    /// state whose CTM maps pattern space onto the raster (§8.7.3.1, PM5);
    /// an uncoloured pattern starts in the `scn` colour with its own colour
    /// operators ignored (Table 75 `PaintType 2`).
    fn render_tile_cell(
        &mut self,
        id: Option<ObjId>,
        stream: &Stream,
        pattern: &TilingPattern,
        plan: &CellPlan,
        stroking: bool,
        name: &[u8],
    ) -> Option<Pixmap> {
        // ARCHITECTURE.md §10.1: the shared depth budget, the object cycle
        // set, and the pattern-specific nesting ceiling.
        if self.depth >= MAX_XOBJECT_DEPTH {
            self.diag.xobject_depth_overflows += 1;
            self.tiling_refused(name, "nested past MAX_XOBJECT_DEPTH");
            return None;
        }
        if id.is_some_and(|id| self.active.contains(&id)) {
            self.tiling_refused(name, "the pattern paints itself");
            return None;
        }
        let Some(_nest) = NestGuard::enter() else {
            self.tiling_refused(name, "tiling patterns nested past the ceiling");
            return None;
        };
        let doc = self.doc;
        let content = doc
            .slice(stream.data_span)
            .and_then(|raw| pdfcer_core::filters::decode_stream(&stream.dict, raw).ok())
            .and_then(|bytes| ContentStream::parse(bytes).ok());
        let Some(content) = content else {
            self.tiling_refused(name, "content stream would not decode");
            return None;
        };
        // Table 75 requires `/Resources`; a pattern without one borrows the
        // resources it was named from rather than painting nothing.
        let resources = stream
            .dict
            .get(b"Resources")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .unwrap_or(self.resources);
        let mut initial = GraphicsState::default_with_ctm(plan.bbox_ctm);
        let colour_source = if pattern.paint_type == PaintType::Uncoloured {
            let colour = if stroking {
                self.gs.current.stroke_color
            } else {
                self.gs.current.fill_color
            };
            initial.fill_color = colour;
            initial.stroke_color = colour;
            Some(crate::type3::GlyphColorSource::ShapeOnly)
        } else {
            None
        };
        let Some(mut raster) = Pixmap::new(plan.bbox_px.0, plan.bbox_px.1) else {
            self.tiling_refused(name, "cell raster could not be allocated");
            return None;
        };
        let mut active = self.active.clone();
        active.extend(id);
        let nested = run_nested(
            doc,
            &content,
            resources,
            self.page_resources,
            self.fonts,
            initial,
            &mut Canvas::paint(&mut raster),
            self.depth + 1,
            active,
            self.cancel,
            self.policy,
            false,
            self.blend_space,
            colour_source,
        );
        self.diag.merge(nested);
        tiling::fold_cell(&raster, plan)
    }

    fn composite_tiles(
        &mut self,
        cell: &Pixmap,
        plan: &CellPlan,
        mask: &Mask,
        region: Region,
        stroking: bool,
        canvas: &mut Canvas<'_>,
    ) -> bool {
        // §11.6.7: the pattern composites as one group under the current
        // constant alpha.
        let alpha = if stroking {
            self.gs.current.stroke_alpha
        } else {
            self.gs.current.fill_alpha
        };
        let blend = self.gs.current.blend_mode;
        canvas.refuse(PoisonReason::TilingPattern);
        let painted = if let Some(buf) = canvas.cmyk_mut() {
            let (w, h) = (buf.width(), buf.height());
            match Pixmap::new(w, h) {
                Some(mut scratch) => {
                    let ok = tiling::paint_tiled(
                        &mut scratch,
                        cell,
                        plan,
                        mask,
                        region,
                        alpha,
                        tiny_skia::BlendMode::SourceOver,
                    );
                    if ok {
                        let r = clamp_region(region, w, h);
                        buf.composite_srgb(&scratch, r, 1.0, crate::compositor::Blend::Normal);
                    }
                    ok
                }
                None => false,
            }
        } else if let Some(scratch) = canvas.export_scratch(self.gs.current.clip_ref().id) {
            tiling::paint_tiled(scratch, cell, plan, mask, region, alpha, blend)
        } else if let Some(dest) = canvas.pixmap_mut() {
            tiling::paint_tiled(dest, cell, plan, mask, region, alpha, blend)
        } else {
            false
        };
        if painted {
            self.diag.color.tiling_patterns_painted += 1;
        } else {
            self.diag.color.patterns_unpainted += 1;
        }
        painted
    }

    /// The area a pattern paints: the path's coverage multiplied by the
    /// clip in force (§8.5.4 NOTE 2: a clip only shrinks, so a coverage
    /// multiply is exact), and the device bounds outside which it is zero.
    pub(super) fn pattern_area_mask(
        &self,
        area: &PatternArea<'_>,
        canvas: &Canvas<'_>,
    ) -> Option<(Mask, Region)> {
        let mut mask = Mask::new(canvas.width(), canvas.height())?;
        mask.fill_path_fit(area.path, area.rule, true, area.ctm);
        if let Some(old) = self.gs.current.clip.as_deref() {
            for (n, o) in mask.data_mut().iter_mut().zip(old.data()) {
                *n = ((u16::from(*n) * u16::from(*o)) / 255) as u8;
            }
        }
        let b = area.path.clone().transform(area.ctm)?.bounds();
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let region = (
            b.left().floor().max(0.0) as i32,
            b.top().floor().max(0.0) as i32,
            b.right().ceil().min(canvas.width() as f32) as i32,
            b.bottom().ceil().min(canvas.height() as f32) as i32,
        );
        Some((mask, region))
    }

    /// Stroke `path` with the stroking pattern: the stroke's outline
    /// (dashed first, when a dash is set) filled nonzero (§8.5.3.1: a
    /// stroke paints the area its outline encloses).
    pub(super) fn paint_pattern_stroke(
        &mut self,
        path: &Path,
        ctm: Transform,
        canvas: &mut Canvas<'_>,
    ) -> bool {
        if self.color.pattern(true).is_none() {
            return false;
        }
        let stroke = self.stroke_params();
        let res = PathStroker::compute_resolution_scale(&ctm);
        let dashed = stroke.dash.as_ref().and_then(|d| path.dash(d, res));
        let source = match (&stroke.dash, &dashed) {
            (Some(_), Some(d)) => d,
            (Some(_), None) => return false,
            (None, _) => path,
        };
        let Some(outline) = source.stroke(&stroke, res) else {
            return false;
        };
        self.paint_with_pattern(&outline, FillRule::Winding, ctm, true, canvas)
    }
}
