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
/// Loads and parses the font on every call; the cost is the font's, not
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
    let loaded = match text::load(doc, &preview.font, env) {
        Ok(l) => l,
        Err(e) => return none(OutlineSkip::Unsupported(e)),
    };
    if loaded.is_type3() {
        return PreviewOutlines {
            source: Some(loaded.source),
            ..none(OutlineSkip::Type3)
        };
    }
    let Ok(program) = FontProgram::parse(loaded.data.bytes()) else {
        return PreviewOutlines {
            source: Some(loaded.source),
            ..none(OutlineSkip::ProgramUnreadable)
        };
    };
    let upem = match program.upem() {
        u if u > 0.0 => u,
        _ => 1000.0,
    };
    let glyphs = preview
        .glyphs
        .iter()
        .map(|g| {
            let gid = loaded.gid(g.code, Some(&program))?;
            let path = program.outline(gid).ok()??;
            path.transform(to_transform(g.matrix).pre_scale(1.0 / upem, 1.0 / upem))
        })
        .collect();
    PreviewOutlines {
        glyphs,
        source: Some(loaded.source),
        skipped: None,
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
    let Ok(content) = pdfcer_core::content::ContentStream::parse(ap.ap_content.clone()) else {
        let mut diag = crate::Diagnostics::default();
        diag.sample_ops
            .push("ce dimension preview: appearance did not parse".to_owned());
        return diag;
    };
    let resources = ap
        .ap_dict
        .get(b"Resources")
        .and_then(pdfcer_core::object::Object::as_dict)
        .cloned()
        .unwrap_or_default();
    crate::interpret::run(
        doc,
        &content,
        &resources,
        &options.fonts,
        crate::gstate::GraphicsState::default_with_ctm(page_to_device),
        pixmap,
        None,
        options.policy(),
    )
}
