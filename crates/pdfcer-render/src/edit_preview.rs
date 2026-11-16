//! Outlines for a typing preview: [`pdfcer_core::text_edit::TextEditPreview`]
//! turned into page-space glyph paths through the same font loading the
//! page renderer uses, so a shell can draw typed text in the run's own font
//! before the edit is committed.

use pdfcer_core::text_edit::TextEditPreview;
use pdfcer_core::view::DocumentView;
use tiny_skia::{Path, Transform};

use crate::font::program::FontProgram;
use crate::font::{FontEnvironment, GlyphSource};
use crate::text::{self, UnsupportedFont};

/// Why a preview carries no outlines.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutlineSkip {
    /// The font's machinery is outside what the renderer draws; the page
    /// renderer skips the same text.
    Unsupported(UnsupportedFont),
    /// A Type 3 font (§9.6.5): its glyphs are content streams, not
    /// outlines. The shell falls back to its own drawing of `preview.bbox`.
    Type3,
    /// The font program (embedded or substitute) could not be parsed.
    ProgramUnreadable,
}

/// Page-space outlines for each glyph of a [`TextEditPreview`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PreviewOutlines {
    /// One entry per `preview.glyphs`, same order. `None` for a glyph with
    /// no outline (a space) or a GID the font cannot resolve. Coordinates
    /// are PDF page user space, like the page renderer's before its
    /// page-to-device transform.
    pub glyphs: Vec<Option<Path>>,
    /// Whose shapes these are (decision 012): the document's own program,
    /// a bundled substitute or an operator-supplied face. A substitute is
    /// a disclosure the shell owes the operator, off-canvas (rule 4).
    pub source: Option<GlyphSource>,
    /// Set when there are no outlines at all, and why.
    pub skipped: Option<OutlineSkip>,
}

/// Outlines for `preview`, loaded through [`crate::text::load`] with `env`
/// (the same environment the page is rendered with, so substitutes match).
///
/// Each glyph path is the font program's outline scaled by `1/upem` and
/// then by the glyph's `matrix` (§9.4.4 `Trm`, already including `Tfs`,
/// `Th`, `Trise`, `Tm` and the CTM). Fill with `preview.fill` and stroke
/// with `preview.stroke` per `preview.render_mode` (§9.3.6).
///
/// A glyph with [`PreviewGlyph::fallback`](pdfcer_core::text_edit::PreviewGlyph::fallback)
/// set is drawn from `preview.fallback`'s face; a fallback face that does
/// not load leaves those glyphs `None` and the rest drawn. `source` and
/// `skipped` describe the run's own font.
///
/// Loads and parses the fonts on every call; the cost is the fonts', not
/// the page's.
#[must_use]
pub fn preview_outlines(
    doc: &DocumentView<'_>,
    preview: &TextEditPreview,
    env: &FontEnvironment,
) -> PreviewOutlines {
    let none = |skip| PreviewOutlines {
        glyphs: vec![None; preview.glyphs.len()],
        source: None,
        skipped: Some(skip),
    };
    let program = (preview.font_program.as_ref(), preview.cid_to_gid.as_ref());
    let own = match load_face(doc, &preview.font, program, env) {
        Ok(loaded) => loaded,
        Err(skip) => return none(skip),
    };
    let source = Some(own.source);
    let own = match Face::parse(&own) {
        Ok(face) => face,
        Err(skip) => {
            return PreviewOutlines {
                source,
                ..none(skip)
            };
        }
    };
    let fallback = preview
        .fallback
        .as_ref()
        .and_then(|f| load_face(doc, &f.font, (f.font_program.as_ref(), None), env).ok());
    let fallback = fallback.as_ref().and_then(|l| Face::parse(l).ok());
    let glyphs = preview
        .glyphs
        .iter()
        .map(|g| {
            let face = if g.fallback { fallback.as_ref()? } else { &own };
            face.outline(g.code, g.matrix)
        })
        .collect();
    PreviewOutlines {
        glyphs,
        source,
        skipped: None,
    }
}

/// A preview face, from `program` when the edit has not embedded it yet.
fn load_face(
    doc: &DocumentView<'_>,
    font: &pdfcer_core::object::Dict,
    (program, map): (Option<&Vec<u8>>, Option<&Vec<u8>>),
    env: &FontEnvironment,
) -> Result<text::LoadedFont, OutlineSkip> {
    match program {
        Some(p) => text::load_with_program(doc, font, env, p.clone(), map.cloned()),
        None => text::load(doc, font, env),
    }
    .map_err(OutlineSkip::Unsupported)
}

/// One loaded, parsed face of a preview.
struct Face<'a> {
    loaded: &'a text::LoadedFont,
    program: FontProgram<'a>,
    upem: f32,
}

impl<'a> Face<'a> {
    fn parse(loaded: &'a text::LoadedFont) -> Result<Self, OutlineSkip> {
        if loaded.is_type3() {
            return Err(OutlineSkip::Type3);
        }
        let program =
            FontProgram::parse(loaded.data.bytes()).map_err(|_| OutlineSkip::ProgramUnreadable)?;
        let upem = match program.upem() {
            u if u > 0.0 => u,
            _ => 1000.0,
        };
        Ok(Self {
            loaded,
            program,
            upem,
        })
    }

    fn outline(&self, code: u32, matrix: [f64; 6]) -> Option<Path> {
        let gid = self.loaded.gid(code, Some(&self.program))?;
        let path = self.program.outline(gid).ok()??;
        path.transform(to_transform(matrix).pre_scale(1.0 / self.upem, 1.0 / self.upem))
    }
}

/// PDF `[a b c d e f]` (x' = ax + cy + e) as a tiny-skia transform.
#[allow(clippy::cast_possible_truncation)] // tiny-skia is f32 throughout.
fn to_transform(m: [f64; 6]) -> Transform {
    Transform::from_row(
        m[0] as f32,
        m[1] as f32,
        m[2] as f32,
        m[3] as f32,
        m[4] as f32,
        m[5] as f32,
    )
}

/// Paint a ce dimension drag preview
/// ([`pdfcer_core::dimension::DimensionPreview`]) onto `pixmap`.
///
/// `page_to_device` maps PDF page user space to `pixmap` pixels, as for
/// the page render. The appearance is run through the same interpreter,
/// with its own `/Resources`, that paints the committed `/AP` (whose
/// `/BBox` equals `/Rect` under an identity `/Matrix`, so §12.5.5
/// placement is the identity), so the preview's pixels are the commit's.
/// Paints over whatever `pixmap` holds; a shell compositing it over the
/// page passes a transparent pixmap.
///
/// Returns the interpreter's diagnostics. A content stream the baker
/// wrote and the parser refuses paints nothing and is reported as one
/// `sample_ops` entry.
#[must_use]
pub fn paint_dimension_preview(
    doc: &DocumentView<'_>,
    preview: &pdfcer_core::dimension::DimensionPreview,
    options: &crate::RenderOptions,
    page_to_device: Transform,
    pixmap: &mut tiny_skia::Pixmap,
) -> crate::Diagnostics {
    let ap = &preview.appearance;
    paint_appearance(
        doc,
        (&ap.ap_dict, &ap.ap_content),
        "ce dimension preview",
        options,
        page_to_device,
        pixmap,
    )
}

/// Paint a text annotation that is not in the document yet, or a text box
/// being retyped, onto `pixmap`: the `authored` field of a
/// [`pdfcer_core::annot_author::FreeTextPreview`], or any
/// [`build_text_annotation`](pdfcer_core::annot_author::build_text_annotation)
/// result whose appearance box is the size of its rectangle.
///
/// `page_to_device` maps PDF page user space to `pixmap` pixels, as for the
/// page render, so it carries the page rotation and the scale. The
/// appearance is placed at `authored.rect` (its `/BBox` is the rectangle's
/// size under an identity `/Matrix`, so §12.5.5 placement is a translation)
/// and run through the interpreter that paints the committed `/AP`, with its
/// own `/Resources`, so the preview's pixels are the commit's. Paints over
/// whatever `pixmap` holds.
///
/// Returns the interpreter's diagnostics, as [`paint_dimension_preview`]
/// does.
#[must_use]
pub fn paint_text_annotation_preview(
    doc: &DocumentView<'_>,
    authored: &pdfcer_core::annot_author::AuthoredTextAnnot,
    options: &crate::RenderOptions,
    page_to_device: Transform,
    pixmap: &mut tiny_skia::Pixmap,
) -> crate::Diagnostics {
    let (bx, by) = bbox_origin(&authored.ap_dict);
    #[allow(clippy::cast_possible_truncation)] // tiny-skia is f32 throughout.
    let placed = page_to_device.pre_translate(
        (authored.rect.llx - bx) as f32,
        (authored.rect.lly - by) as f32,
    );
    paint_appearance(
        doc,
        (&authored.ap_dict, &authored.ap_content),
        "text annotation preview",
        options,
        placed,
        pixmap,
    )
}

/// The lower-left corner of a form's `/BBox`; the origin when it has none.
fn bbox_origin(ap_dict: &pdfcer_core::object::Dict) -> (f64, f64) {
    use pdfcer_core::object::Object;
    let num = |o: Option<&Object>| match o {
        #[allow(clippy::cast_precision_loss)] // a coordinate, far below 2^53.
        Some(Object::Integer(i)) => *i as f64,
        Some(Object::Real(r)) => *r,
        _ => 0.0,
    };
    let bbox = ap_dict
        .get(b"BBox")
        .and_then(Object::as_array)
        .unwrap_or(&[]);
    let (x0, y0, x1, y1) = (
        num(bbox.first()),
        num(bbox.get(1)),
        num(bbox.get(2)),
        num(bbox.get(3)),
    );
    (x0.min(x1), y0.min(y1))
}

/// Run a baked appearance (`/AP` form dictionary, content bytes) through the
/// page interpreter under `transform`. `what` names it in the diagnostic for
/// content the parser refuses.
fn paint_appearance(
    doc: &DocumentView<'_>,
    (ap_dict, ap_content): (&pdfcer_core::object::Dict, &[u8]),
    what: &str,
    options: &crate::RenderOptions,
    transform: Transform,
    pixmap: &mut tiny_skia::Pixmap,
) -> crate::Diagnostics {
    let Ok(content) = pdfcer_core::content::ContentStream::parse(ap_content.to_vec()) else {
        let mut diag = crate::Diagnostics::default();
        diag.sample_ops
            .push(format!("{what}: appearance did not parse"));
        return diag;
    };
    let resources = ap_dict
        .get(b"Resources")
        .and_then(pdfcer_core::object::Object::as_dict)
        .cloned()
        .unwrap_or_default();
    crate::interpret::run(
        doc,
        &content,
        &resources,
        &options.fonts,
        crate::gstate::GraphicsState::default_with_ctm(transform),
        pixmap,
        None,
        options.policy(),
    )
}
