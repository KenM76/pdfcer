//! The tree walker: `usvg` nodes to content operators, forms and graphics
//! states, following resvg's order of effects (clip, then mask, then
//! opacity and blend, on an isolated layer).

use std::collections::HashMap;

use usvg::tiny_skia_path::{Path, PathBuilder};
use usvg::{
    BlendMode, ClipPath, FillRule, Group, Mask, MaskType, Node, Paint, PaintOrder, Transform,
};

use super::objects::{Canvas, GroupKind, ObjectTable, Res, SvgObject, finish_form, name, real};
use super::{ImportedSvg, SvgFeature, SvgImportError, SvgImportNotes};
use crate::image_import;
use crate::object::{Dict, Name, ObjId, Object};

/// Whether paths are painted normally, or as opaque coverage for a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Normal,
    Silhouette,
}

pub(super) struct Emitter {
    pub(super) table: ObjectTable,
    pub(super) notes: SvgImportNotes,
    alpha_gs: HashMap<u32, ObjId>,
    pub(super) pattern_depth: usize,
    blended: bool,
}

/// A hard clip: geometry already in canvas space, and its fill rule.
struct ClipGeom {
    path: Path,
    rule: FillRule,
}

/// Convert a parsed tree into the drawing's objects and root form;
/// `notes` carries what parsing already skipped.
pub(super) fn emit_document(
    tree: &usvg::Tree,
    notes: SvgImportNotes,
) -> Result<ImportedSvg, SvgImportError> {
    let size = tree.size();
    let (w, h) = (f64::from(size.width()), f64::from(size.height()));
    let mut em = Emitter {
        table: ObjectTable::default(),
        notes,
        alpha_gs: HashMap::new(),
        pattern_depth: 0,
        blended: false,
    };
    let base = Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, size.height());
    let mut canvas = Canvas::new(base, [0.0, 0.0, w, h]);
    em.group(
        tree.root(),
        Transform::identity(),
        &mut canvas,
        Mode::Normal,
    )?;
    // A blend mode composites against the SVG's own transparent canvas,
    // not the page underneath: isolate the whole drawing when one is used.
    let kind = if em.blended {
        GroupKind::Isolated
    } else {
        GroupKind::None
    };
    let root = finish_form(&mut em.table, canvas, kind)?;
    Ok(ImportedSvg {
        width: w,
        height: h,
        objects: em.table.objects,
        root,
        notes: em.notes,
    })
}

impl Emitter {
    /// Emit each child of `g` under `t`, in document order.
    pub(super) fn children(
        &mut self,
        g: &Group,
        t: Transform,
        canvas: &mut Canvas,
        mode: Mode,
    ) -> Result<(), SvgImportError> {
        for child in g.children() {
            match child {
                Node::Group(sub) => self.group(sub, t, canvas, mode)?,
                Node::Path(p) => self.path(p, t, canvas, mode)?,
                Node::Image(im) if mode == Mode::Normal => self.image(im, t, canvas)?,
                // Text is counted by the prescan; usvg only builds it with
                // its `text` feature, which pdfcer leaves off.
                Node::Image(_) | Node::Text(_) => {}
            }
        }
        Ok(())
    }

    fn group(
        &mut self,
        g: &Group,
        ctm: Transform,
        canvas: &mut Canvas,
        mode: Mode,
    ) -> Result<(), SvgImportError> {
        let t = ctm.pre_concat(g.transform());
        if mode == Mode::Normal {
            self.notes.skip(SvgFeature::Filter, g.filters().len());
        }
        let mut hard = Vec::new();
        let mut smasks = Vec::new();
        let mut clip = g.clip_path();
        while let Some(c) = clip {
            match hard_clip(c, t, canvas.base) {
                Some(geom) => hard.push(geom),
                None => smasks.push(self.clip_mask(c, t, canvas)?),
            }
            clip = c.clip_path();
        }
        let (opacity, blend) = if mode == Mode::Normal {
            let mut mask = g.mask();
            while let Some(m) = mask {
                if !m.root().has_children() {
                    return Ok(());
                }
                smasks.push(self.mask(m, t, canvas)?);
                mask = m.mask();
            }
            (g.opacity().get(), g.blend_mode())
        } else {
            (1.0, BlendMode::Normal)
        };
        let layer = opacity < 1.0
            || blend != BlendMode::Normal
            || !smasks.is_empty()
            || (mode == Mode::Normal && g.isolate());
        if !layer {
            if hard.is_empty() {
                return self.children(g, t, canvas, mode);
            }
            canvas.op("q");
            hard.iter().for_each(|c| clip_ops(canvas, c));
            self.children(g, t, canvas, mode)?;
            canvas.op("Q");
            return Ok(());
        }
        let mut sub = canvas.sibling();
        hard.iter().for_each(|c| clip_ops(&mut sub, c));
        self.children(g, t, &mut sub, mode)?;
        let mut form = finish_form(&mut self.table, sub, GroupKind::Isolated)?;
        // One soft mask per graphics state: extra ones wrap the layer.
        while smasks.len() > 1 {
            let sm = smasks.pop().unwrap_or(Object::Null);
            let gs = self.ext_gstate(1.0, BlendMode::Normal, Some(sm))?;
            let mut wrap = canvas.sibling();
            invoke(&mut wrap, gs, form);
            form = finish_form(&mut self.table, wrap, GroupKind::Isolated)?;
        }
        let gs = self.ext_gstate(opacity, blend, smasks.pop())?;
        invoke(canvas, gs, form);
        Ok(())
    }

    /// An alpha soft mask drawing the clip path's coverage (§11.6.5.2).
    fn clip_mask(
        &mut self,
        c: &ClipPath,
        t: Transform,
        canvas: &Canvas,
    ) -> Result<Object, SvgImportError> {
        let mut sub = canvas.sibling();
        self.children(
            c.root(),
            t.pre_concat(c.transform()),
            &mut sub,
            Mode::Silhouette,
        )?;
        let form = finish_form(&mut self.table, sub, GroupKind::Isolated)?;
        Ok(smask(b"Alpha", form))
    }

    /// A soft mask for an SVG `mask`, clipped to the mask's region.
    fn mask(&mut self, m: &Mask, t: Transform, canvas: &Canvas) -> Result<Object, SvgImportError> {
        let mut sub = canvas.sibling();
        let region = PathBuilder::from_rect(m.rect().to_rect());
        if let Some(path) = region.transform(canvas.base.pre_concat(t)) {
            clip_ops(
                &mut sub,
                &ClipGeom {
                    path,
                    rule: FillRule::NonZero,
                },
            );
        }
        self.children(m.root(), t, &mut sub, Mode::Normal)?;
        let luminance = m.kind() == MaskType::Luminance;
        let (kind, subtype): (_, &[u8]) = if luminance {
            self.notes.approximate(SvgFeature::LuminanceMask);
            (GroupKind::Luminosity, b"Luminosity")
        } else {
            (GroupKind::Isolated, b"Alpha")
        };
        let form = finish_form(&mut self.table, sub, kind)?;
        Ok(smask(subtype, form))
    }

    /// An `ExtGState` (§8.4.5 Table 58) for the given opacity, blend mode
    /// and soft mask, or `None` when all three are the defaults.
    pub(super) fn ext_gstate(
        &mut self,
        alpha: f32,
        blend: BlendMode,
        smask: Option<Object>,
    ) -> Result<Option<ObjId>, SvgImportError> {
        let plain = blend == BlendMode::Normal && smask.is_none();
        if plain && alpha >= 1.0 {
            return Ok(None);
        }
        if plain && let Some(id) = self.alpha_gs.get(&alpha.to_bits()) {
            return Ok(Some(*id));
        }
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), name(b"ExtGState"));
        if alpha < 1.0 {
            // §11.6.4.4: CA strokes, ca everything else.
            d.insert(Name::from(b"CA"), real(f64::from(alpha)));
            d.insert(Name::from(b"ca"), real(f64::from(alpha)));
        }
        if blend != BlendMode::Normal {
            self.blended = true;
            d.insert(Name::from(b"BM"), name(blend_name(blend)));
        }
        if let Some(sm) = smask {
            d.insert(Name::from(b"SMask"), sm);
        }
        let id = self.table.push(SvgObject::Dict(d))?;
        if plain {
            self.alpha_gs.insert(alpha.to_bits(), id);
        }
        Ok(Some(id))
    }

    fn path(
        &mut self,
        p: &usvg::Path,
        ctm: Transform,
        canvas: &mut Canvas,
        mode: Mode,
    ) -> Result<(), SvgImportError> {
        if !p.is_visible() {
            return Ok(());
        }
        let before = canvas.out.len();
        // resvg does not fill a path with no area.
        let bounds = p.data().bounds();
        let fill = p
            .fill()
            .filter(|_| bounds.width() > 0.0 && bounds.height() > 0.0);
        let m = canvas.base.pre_concat(ctm);
        if mode == Mode::Silhouette {
            if let Some(f) = fill {
                canvas.op("q");
                canvas.cm(m);
                canvas.op("0 g");
                canvas.path(p.data());
                canvas.op(fill_op(f.rule()));
                canvas.op("Q");
            }
        } else {
            let fill_first = p.paint_order() == PaintOrder::FillAndStroke;
            if fill_first && let Some(f) = fill {
                self.paint_fill(p, f, ctm, canvas)?;
            }
            if let Some(s) = p.stroke() {
                self.paint_stroke(p, s, ctm, canvas)?;
            }
            if !fill_first && let Some(f) = fill {
                self.paint_fill(p, f, ctm, canvas)?;
            }
        }
        self.table.charge(canvas.out.len() - before)
    }

    fn paint_fill(
        &mut self,
        p: &usvg::Path,
        f: &usvg::Fill,
        ctm: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), SvgImportError> {
        let bbox = p.data().bounds();
        if !self.set_paint(f.paint(), f.opacity().get(), ctm, bbox, canvas, false)? {
            return Ok(());
        }
        canvas.path(p.data());
        canvas.op(fill_op(f.rule()));
        canvas.op("Q");
        Ok(())
    }

    fn paint_stroke(
        &mut self,
        p: &usvg::Path,
        s: &usvg::Stroke,
        ctm: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), SvgImportError> {
        let bbox = p.stroke_bounding_box();
        if !self.set_paint(s.paint(), s.opacity().get(), ctm, bbox, canvas, true)? {
            return Ok(());
        }
        canvas.num(f64::from(s.width().get()));
        canvas.op("w");
        let (cap, join) = stroke_style(s, &mut self.notes);
        canvas.op(cap);
        canvas.op(join);
        canvas.num(f64::from(s.miterlimit().get().max(1.0)));
        canvas.op("M");
        if let Some(dash) = s.dasharray() {
            canvas.out.push(b'[');
            for d in dash {
                canvas.num(f64::from(*d));
            }
            canvas.out.extend_from_slice(b"] ");
            canvas.num(f64::from(s.dashoffset()));
            canvas.op("d");
        }
        canvas.path(p.data());
        canvas.op("S");
        canvas.op("Q");
        Ok(())
    }

    /// Open `q`, set opacity, the transform and the colour for one paint.
    /// `Ok(false)` (nothing opened) when the paint draws nothing.
    fn set_paint(
        &mut self,
        paint: &Paint,
        opacity: f32,
        ctm: Transform,
        bbox: usvg::Rect,
        canvas: &mut Canvas,
        stroke: bool,
    ) -> Result<bool, SvgImportError> {
        let m = canvas.base.pre_concat(ctm);
        let (pattern, gs) = match paint {
            Paint::Color(_) => (None, self.ext_gstate(opacity, BlendMode::Normal, None)?),
            Paint::LinearGradient(_) | Paint::RadialGradient(_) => {
                let (pid, alpha, smask) = self.gradient(paint, opacity, m, bbox, canvas)?;
                (Some(pid), self.ext_gstate(alpha, BlendMode::Normal, smask)?)
            }
            Paint::Pattern(pat) => match self.tiling(pat, m)? {
                Some(pid) => (
                    Some(pid),
                    self.ext_gstate(opacity, BlendMode::Normal, None)?,
                ),
                None => return Ok(false),
            },
        };
        canvas.op("q");
        if let Some(gs) = gs {
            let n = canvas.name(Res::ExtGState, gs);
            canvas.name_op(&n, "gs");
        }
        canvas.cm(m);
        match (paint, pattern) {
            (Paint::Color(c), _) => {
                for v in [c.red, c.green, c.blue] {
                    canvas.num(f64::from(v) / 255.0);
                }
                canvas.op(if stroke { "RG" } else { "rg" });
            }
            (_, Some(pid)) => {
                let n = canvas.name(Res::Pattern, pid);
                canvas.op(if stroke { "/Pattern CS" } else { "/Pattern cs" });
                canvas.name_op(&n, if stroke { "SCN" } else { "scn" });
            }
            _ => {}
        }
        Ok(true)
    }

    fn image(
        &mut self,
        im: &usvg::Image,
        ctm: Transform,
        canvas: &mut Canvas,
    ) -> Result<(), SvgImportError> {
        if !im.is_visible() {
            return Ok(());
        }
        let (data, feature) = match im.kind() {
            usvg::ImageKind::SVG(tree) => {
                return self.group(tree.root(), ctm, canvas, Mode::Normal);
            }
            usvg::ImageKind::JPEG(d) | usvg::ImageKind::PNG(d) => (d, SvgFeature::UndecodableImage),
            usvg::ImageKind::GIF(d) => (d, SvgFeature::GifImage),
            usvg::ImageKind::WEBP(d) => (d, SvgFeature::WebpImage),
        };
        let Ok(img) = image_import::import(data) else {
            self.notes.skip(feature, 1);
            return Ok(());
        };
        let size = im.size();
        let [a, b, c, d, e, f] = img.orientation.unit_square_matrix().map(|v| v as f32);
        // Unit square, row 0 at the top, onto (0,0)–(w,h) in y-down space.
        let m = canvas
            .base
            .pre_concat(ctm)
            .pre_concat(Transform::from_row(
                size.width(),
                0.0,
                0.0,
                -size.height(),
                0.0,
                size.height(),
            ))
            .pre_concat(Transform::from_row(a, b, c, d, e, f));
        let id = self.table.push(SvgObject::Image(Box::new(img)))?;
        let n = canvas.name(Res::XObject, id);
        canvas.op("q");
        canvas.cm(m);
        canvas.name_op(&n, "Do");
        canvas.op("Q");
        Ok(())
    }
}

/// `q [/G gs] /X Do Q`.
pub(super) fn invoke(canvas: &mut Canvas, gs: Option<ObjId>, form: ObjId) {
    canvas.op("q");
    if let Some(gs) = gs {
        let n = canvas.name(Res::ExtGState, gs);
        canvas.name_op(&n, "gs");
    }
    let x = canvas.name(Res::XObject, form);
    canvas.name_op(&x, "Do");
    canvas.op("Q");
}

/// A soft-mask dictionary (§11.6.5.2 Table 144).
fn smask(subtype: &[u8], group: ObjId) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), name(b"Mask"));
    d.insert(Name::from(b"S"), name(subtype));
    d.insert(Name::from(b"G"), Object::Reference(group));
    Object::Dict(d)
}

/// A clip path that is one filled path under plain single-child groups:
/// exactly a `W n` clip. Anything else becomes a soft mask.
fn hard_clip(c: &ClipPath, t: Transform, base: Transform) -> Option<ClipGeom> {
    let mut ts = t.pre_concat(c.transform());
    let mut g = c.root();
    loop {
        let [only] = g.children() else {
            return None;
        };
        match only {
            Node::Path(p) => {
                let fill = p.fill()?;
                let b = p.data().bounds();
                if !p.is_visible() || b.width() <= 0.0 || b.height() <= 0.0 {
                    return None;
                }
                let path = p.data().clone().transform(base.pre_concat(ts))?;
                return Some(ClipGeom {
                    path,
                    rule: fill.rule(),
                });
            }
            Node::Group(sub) if sub.clip_path().is_none() => {
                ts = ts.pre_concat(sub.transform());
                g = sub;
            }
            _ => return None,
        }
    }
}

fn clip_ops(canvas: &mut Canvas, c: &ClipGeom) {
    canvas.path(&c.path);
    canvas.op(match c.rule {
        FillRule::NonZero => "W n",
        FillRule::EvenOdd => "W* n",
    });
}

const fn fill_op(rule: FillRule) -> &'static str {
    match rule {
        FillRule::NonZero => "f",
        FillRule::EvenOdd => "f*",
    }
}

fn stroke_style(s: &usvg::Stroke, notes: &mut SvgImportNotes) -> (&'static str, &'static str) {
    let cap = match s.linecap() {
        usvg::LineCap::Butt => "0 J",
        usvg::LineCap::Round => "1 J",
        usvg::LineCap::Square => "2 J",
    };
    let join = match s.linejoin() {
        usvg::LineJoin::Miter => "0 j",
        usvg::LineJoin::MiterClip => {
            notes.approximate(SvgFeature::MiterClipJoin);
            "0 j"
        }
        usvg::LineJoin::Round => "1 j",
        usvg::LineJoin::Bevel => "2 j",
    };
    (cap, join)
}

/// §11.3.5 Tables 136–137 blend-mode names.
const fn blend_name(b: BlendMode) -> &'static [u8] {
    match b {
        BlendMode::Normal => b"Normal",
        BlendMode::Multiply => b"Multiply",
        BlendMode::Screen => b"Screen",
        BlendMode::Overlay => b"Overlay",
        BlendMode::Darken => b"Darken",
        BlendMode::Lighten => b"Lighten",
        BlendMode::ColorDodge => b"ColorDodge",
        BlendMode::ColorBurn => b"ColorBurn",
        BlendMode::HardLight => b"HardLight",
        BlendMode::SoftLight => b"SoftLight",
        BlendMode::Difference => b"Difference",
        BlendMode::Exclusion => b"Exclusion",
        BlendMode::Hue => b"Hue",
        BlendMode::Saturation => b"Saturation",
        BlendMode::Color => b"Color",
        BlendMode::Luminosity => b"Luminosity",
    }
}
