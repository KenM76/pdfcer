//! A text box's appearance and where its characters land, with nothing
//! staged (pdfcer-gui request G282).

use super::{AuthoredTextAnnot, TextAnnotSpec, build_text_annotation, free_text_da, positive_rect};
use crate::page_tree::Rect;
use crate::vartext::{self, TextLayout, VarTextError};

/// What a `/FreeText` will look like once committed, and where each
/// character of its text is drawn.
///
/// Built by [`free_text_preview`] for a box not yet in the document and by
/// [`EditSession::free_text_note_preview`](crate::edit::EditSession::free_text_note_preview)
/// for one being retyped. `pdfcer_render::edit_preview::paint_text_annotation_preview`
/// paints [`Self::authored`].
///
/// # Coordinates
///
/// [`Self::layout`] is in the appearance's own box: origin at the lower-left
/// corner of `authored.rect`, x right, y up, in points. Add `authored.rect.llx`
/// and `authored.rect.lly` for page user space. Offsets are byte offsets into
/// the spec's text.
///
/// # What the baker lays out
///
/// Left to right, horizontal, in one standard-14 Latin face. There is no
/// vertical writing and no right-to-left run: a character outside `WinAnsi`
/// is drawn as `?` and counted in `authored.unencodable_chars`.
///
/// # Overflow
///
/// The appearance clips to the box; the box never grows. A wrapped box
/// shorter than [`Self::needed_height`] loses its last lines, and a
/// single-line box narrower than [`Self::needed_width`] loses the end of its
/// line. A word wider than a wrapped box is clipped too.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FreeTextPreview {
    /// The spec that was baked. For a retyped box it carries the new text
    /// and the wrapping the box was found to use.
    pub spec: TextAnnotSpec,
    /// The appearance and dictionary the commit will write.
    pub authored: AuthoredTextAnnot,
    /// The lines of [`Self::authored`], top to bottom.
    pub layout: TextLayout,
}

impl FreeTextPreview {
    /// The box height that shows every line.
    #[must_use]
    pub fn needed_height(&self) -> f64 {
        self.layout.needed_height()
    }

    /// The box width that shows the widest line whole.
    #[must_use]
    pub fn needed_width(&self) -> f64 {
        self.layout.needed_width()
    }

    /// Whether the box clips any of the text.
    #[must_use]
    pub fn clips(&self) -> bool {
        let rect = self.authored.rect;
        // A hair of slack: a box sized from `needed_height` must not report
        // clipping through rounding.
        self.needed_height() > rect.height() + 1e-6 || self.needed_width() > rect.width() + 1e-6
    }

    /// `authored.rect` with its bottom edge lowered so every line shows, the
    /// top edge and width kept. The rectangle itself when nothing is clipped
    /// vertically.
    #[must_use]
    pub fn rect_grown_downward(&self) -> Rect {
        let mut rect = self.authored.rect;
        let need = self.needed_height();
        if need > rect.height() {
            rect.lly = rect.ury - need;
        }
        rect
    }
}

/// Bake a `/FreeText` spec and lay its text out, staging nothing.
///
/// `Ok(None)` for a spec that is not [`TextAnnotSpec::FreeText`].
///
/// # Errors
///
/// What [`build_text_annotation`] returns for the same spec.
///
/// # Examples
///
/// ```
/// use pdfcer_core::annot_author::{FreeTextFrame, TextAnnotSpec, free_text_preview};
/// use pdfcer_core::fontdata::Std14;
/// use pdfcer_core::page_tree::Rect;
/// use pdfcer_core::vartext::{Quadding, TextColor};
///
/// let spec = TextAnnotSpec::FreeText {
///     rect: Rect { llx: 100.0, lly: 100.0, urx: 160.0, ury: 120.0 },
///     text: "one two three four five".to_owned(),
///     font: Std14::Helvetica,
///     font_size: 12.0,
///     color: TextColor::Gray(0.0),
///     quadding: Quadding::Left,
///     multiline: true,
///     border: None,
///     border_width: 0.0,
///     frame: FreeTextFrame::default(),
/// };
/// let preview = free_text_preview(&spec)?.expect("a FreeText spec");
/// assert!(preview.layout.lines.len() > 1);
/// assert!(preview.clips());
/// assert_eq!(preview.rect_grown_downward().ury, 120.0);
/// # Ok::<(), pdfcer_core::vartext::VarTextError>(())
/// ```
pub fn free_text_preview(spec: &TextAnnotSpec) -> Result<Option<FreeTextPreview>, VarTextError> {
    let TextAnnotSpec::FreeText {
        rect,
        text,
        font,
        font_size,
        color,
        quadding,
        multiline,
        ..
    } = spec
    else {
        return Ok(None);
    };
    let authored = build_text_annotation(spec)?;
    let rect = positive_rect(*rect);
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: rect.width(),
        ury: rect.height(),
    };
    let (da, resources) = free_text_da(*font, *font_size, *color);
    let layout = vartext::layout_variable_text(bbox, text, &da, *quadding, *multiline, &resources)?;
    Ok(Some(FreeTextPreview {
        spec: spec.clone(),
        authored,
        layout,
    }))
}
