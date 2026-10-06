//! `add-image-stamp`, and the push-button icon flags of `edit-widget`.

use super::*;
use pdfcer_core::annot_author::CaptionPosition;

/// `--caption-position`: `/MK /TP`, ISO 32000-1 Table 189.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CaptionPositionArg {
    /// Caption only, no icon (TP 0).
    CaptionOnly,
    /// Icon only, no caption (TP 1).
    IconOnly,
    /// Caption below the icon (TP 2).
    Below,
    /// Caption above the icon (TP 3).
    Above,
    /// Caption right of the icon (TP 4).
    Right,
    /// Caption left of the icon (TP 5).
    Left,
    /// Caption drawn over the icon (TP 6).
    Overlaid,
}

/// `--foreign-appearance`: when a widget edit may replace button artwork
/// another producer drew (`pdfcer_core::edit::ForeignAppearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ForeignAppearanceArg {
    /// Never replace it; the edit is recorded but not painted.
    Keep,
    /// Replace a push button's when the edit changes its icon or caption
    /// position; keep it for every other edit.
    #[default]
    OnIconEdit,
    /// Replace it whenever the edit needs a redraw.
    Replace,
}

impl From<ForeignAppearanceArg> for pdfcer_core::edit::ForeignAppearance {
    fn from(v: ForeignAppearanceArg) -> Self {
        match v {
            ForeignAppearanceArg::Keep => Self::Keep,
            ForeignAppearanceArg::OnIconEdit => Self::ReplaceOnIconEdit,
            ForeignAppearanceArg::Replace => Self::Replace,
        }
    }
}

impl CaptionPositionArg {
    const fn position(self) -> CaptionPosition {
        match self {
            Self::CaptionOnly => CaptionPosition::CaptionOnly,
            Self::IconOnly => CaptionPosition::IconOnly,
            Self::Below => CaptionPosition::CaptionBelow,
            Self::Above => CaptionPosition::CaptionAbove,
            Self::Right => CaptionPosition::CaptionRight,
            Self::Left => CaptionPosition::CaptionLeft,
            Self::Overlaid => CaptionPosition::Overlaid,
        }
    }
}

/// Read and import an image file, or the exit code after the reason.
fn read_image(flag: &str, path: &Path) -> Result<pdfcer_core::image_import::ImportedImage, u8> {
    let bytes = std::fs::read(path).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::IO_ERROR
    })?;
    pdfcer_core::image_import::import(&bytes).map_err(|err| {
        eprintln!("pdfcer: {flag} {}: {err}", path.display());
        exit::EDIT_REFUSED
    })
}

/// Apply `edit-widget`'s `--button-icon`, `--clear-button-icon` and
/// `--caption-position`.
pub(crate) fn with_button_icon_args(
    mut edit: pdfcer_core::edit::WidgetEdit,
    icon: Option<&Path>,
    clear: bool,
    position: Option<CaptionPositionArg>,
) -> Result<pdfcer_core::edit::WidgetEdit, u8> {
    if let Some(path) = icon {
        edit = edit.with_button_icon(&read_image("--button-icon", path)?);
    }
    if clear {
        edit = edit.without_button_icon();
    }
    if let Some(p) = position {
        edit = edit.with_caption_position(p.position());
    }
    Ok(edit)
}

/// The markup flags every stamp command takes: `--opacity`, `--note`,
/// `--author` and the layer pick.
#[derive(Default)]
pub(crate) struct StampMarkup<'a> {
    pub(crate) opacity: Option<f64>,
    pub(crate) note: Option<&'a str>,
    pub(crate) author: Option<&'a str>,
    pub(crate) layer: Option<LayerPick>,
}

impl StampMarkup<'_> {
    /// The layer `--layer` / `--layer-id` names, or the exit code after the
    /// lookup's reason.
    pub(crate) fn layer(
        &self,
        input: &Path,
        session: &pdfcer_core::edit::EditSession,
    ) -> Result<Option<pdfcer_core::object::ObjId>, u8> {
        resolve_add_layer(input, session, self.layer.as_ref())
    }

    /// The [`pdfcer_core::edit::MarkupOptions`] these flags ask for, or the
    /// exit code after the layer lookup's reason.
    pub(crate) fn options(
        &self,
        input: &Path,
        session: &pdfcer_core::edit::EditSession,
    ) -> Result<pdfcer_core::edit::MarkupOptions, u8> {
        use pdfcer_core::edit::{MarkupNote, MarkupOptions};
        let layer = resolve_add_layer(input, session, self.layer.as_ref())?;
        Ok(MarkupOptions {
            note: self.note.map(|t| {
                let note = MarkupNote::new(t);
                match self.author {
                    Some(who) => note.by(who),
                    None => note,
                }
            }),
            opacity: self.opacity,
            layer,
            ..Default::default()
        })
    }
}

/// Everything `add-image-stamp` takes.
pub(crate) struct AddImageStampArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) image: &'a Path,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) markup: StampMarkup<'a>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `add-image-stamp` — a `/Stamp` annotation whose face is an image.
///
/// Prints one `add-image-stamp …` line on stdout (`stamp=` is the new
/// annotation's object number, `smask=1` when the image carried
/// an alpha channel, written as an `/SMask`), then defers to [`finish_edit`].
/// Every refusal is reported before anything is written.
pub(crate) fn cmd_add_image_stamp(a: &AddImageStampArgs<'_>) -> u8 {
    let (page_index, rect) = match parse_page_and_rect(a.input, a.page, a.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let image = match read_image("--image", a.image) {
        Ok(img) => img,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let options = match a.markup.options(a.input, &session) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let id = match session.add_image_stamp(page_index, rect, &image, &options) {
        Ok(id) => id,
        Err(err) => return report_edit_error(a.input, &err),
    };
    let saved = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "add-image-stamp {} page={} stamp={} pixels={}x{} smask={} -> {} changed_objects={}",
        a.input.display(),
        a.page,
        id.num,
        image.width,
        image.height,
        u32::from(image.soft_mask.is_some()),
        a.output.display(),
        saved.changed,
    );
    finish_edit(a.input, &saved)
}
