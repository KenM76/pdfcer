//! `paste-clipboard`: place what the OS clipboard holds on a page, as
//! vectors whenever the clipboard has a vector form.

use super::*;
use crate::clipboard_read::{self, PasteFormat, Snapshot, Source};
use pdfcer_core::page_tree::Rect;

/// How the pasted content is sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteFit {
    /// Its own size, centred; made smaller only when it does not fit.
    Natural,
    /// As large as fits, proportions kept, centred.
    Contain,
    /// Fill the rectangle exactly.
    Stretch,
}

/// Arguments for [`cmd_paste_clipboard`].
pub(crate) struct PasteClipboardArgs<'a> {
    pub(crate) input: &'a Path,
    /// 1-based.
    pub(crate) page: usize,
    /// `--rect`; `None` is the page's crop box.
    pub(crate) rect: Option<&'a str>,
    pub(crate) fit: PasteFit,
    /// `--format`; `None` takes the best form held.
    pub(crate) format: Option<PasteFormat>,
    pub(crate) stamp: bool,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// The bytes to place, in a form one of the add commands reads.
struct Payload {
    format: PasteFormat,
    bytes: Vec<u8>,
    /// Width and height in points, when the bytes could be measured.
    natural: Option<(f64, f64)>,
}

/// `paste-clipboard`.
///
/// ## Contract
///
/// - Prints `paste-clipboard took=<form> from=<clipboard format>
///   held=<every known format present> vector=<0|1>
///   placed=<llx,lly,urx,ury> fit=<natural|contain|stretch> shrunk=<0|1>`,
///   then the line and disclosures of the command that placed it
///   (`place-stamp`, `add-svg`, `add-emf` or `add-image`).
/// - A raster form is taken only when no vector form is held (or `--format`
///   names it), and stderr says so.
/// - `EDIT_REFUSED` when the clipboard holds nothing readable or not the
///   `--format` asked for; `RUNTIME_ERROR` when it cannot be opened or this
///   is not Windows. Nothing is written in either case.
pub(crate) fn cmd_paste_clipboard(args: &PasteClipboardArgs<'_>) -> u8 {
    let snapshot = match clipboard_read::read(args.format, true) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            eprintln!("pdfcer: paste-clipboard: {err}");
            return exit::RUNTIME_ERROR;
        }
    };
    let held = held_names(&snapshot);
    let Some((source, bytes)) = snapshot.taken else {
        eprintln!("pdfcer: paste-clipboard: {}", nothing_taken(args, &held));
        return exit::EDIT_REFUSED;
    };
    let payload = match payload(source, bytes) {
        Ok(payload) => payload,
        Err(msg) => {
            eprintln!("pdfcer: paste-clipboard: {}: {msg}", source.name);
            return exit::EDIT_REFUSED;
        }
    };
    let requested = match requested_rect(args) {
        Ok(rect) => rect,
        Err(code) => return code,
    };
    let (rect, shrunk) = paste_rect(requested, payload.natural, args.fit);
    if !source.format.is_vector() {
        if args.format.is_some() {
            eprintln!(
                "pdfcer: paste-clipboard: --format {} was asked for, so this is pasted as an image",
                source.format.token()
            );
        } else {
            eprintln!(
                "pdfcer: paste-clipboard: the clipboard holds no vector form ({held}), so this is pasted as an image"
            );
        }
    }
    if shrunk {
        eprintln!(
            "pdfcer: paste-clipboard: the content is larger than the rectangle and was made smaller to fit; its proportions are kept"
        );
    }
    println!(
        "paste-clipboard took={} from={:?} held={held} vector={} placed={:.3},{:.3},{:.3},{:.3} fit={} shrunk={}",
        source.format.token(),
        source.name,
        u32::from(source.format.is_vector()),
        rect.llx,
        rect.lly,
        rect.urx,
        rect.ury,
        match args.fit {
            PasteFit::Natural => "natural",
            PasteFit::Contain => "contain",
            PasteFit::Stretch => "stretch",
        },
        u32::from(shrunk),
    );
    let ext = match payload.format {
        PasteFormat::Pdf => "pdf",
        PasteFormat::Svg => "svg",
        PasteFormat::Emf => "emf",
        PasteFormat::Png | PasteFormat::Dib => "png",
    };
    let temp = std::env::temp_dir().join(format!("pdfcer-paste-{}.{ext}", std::process::id()));
    if let Err(err) = std::fs::write(&temp, &payload.bytes) {
        eprintln!("pdfcer: {}: {err}", temp.display());
        return exit::IO_ERROR;
    }
    let code = place(args, payload.format, &temp, rect);
    // Best effort: a leftover temp file is not a failed paste.
    let _ = std::fs::remove_file(&temp);
    code
}

fn held_names(snapshot: &Snapshot) -> String {
    if snapshot.held.is_empty() {
        return String::from("none");
    }
    let names: Vec<String> = snapshot
        .held
        .iter()
        .map(|s| s.name.replace(' ', "_"))
        .collect();
    names.join(",")
}

fn nothing_taken(args: &PasteClipboardArgs<'_>, held: &str) -> String {
    match args.format {
        Some(f) if held != "none" => format!(
            "the clipboard has no {} form (it holds {held}); nothing was pasted",
            f.token()
        ),
        _ if held == "none" => String::from(
            "the clipboard holds nothing pdfcer can place (PDF, SVG, EMF, PNG or a bitmap); nothing was pasted",
        ),
        _ => format!(
            "the clipboard's content ({held}) could not be read or is over the size limit; nothing was pasted"
        ),
    }
}

/// The clipboard bytes as a file one of the add commands reads, measured.
fn payload(source: &Source, mut bytes: Vec<u8>) -> Result<Payload, String> {
    let mut format = source.format;
    let natural = match format {
        PasteFormat::Pdf => pdf_size(&bytes),
        PasteFormat::Svg => {
            // Clipboard text formats end in NULs the SVG parser refuses.
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            svg_size(&bytes)
        }
        PasteFormat::Emf => pdfcer_core::emf_import::import(&bytes)
            .ok()
            .map(|emf| emf.natural_size_pt()),
        PasteFormat::Png => image_size(&bytes),
        PasteFormat::Dib => {
            let image = clipboard_read::dib_to_pixmap(&bytes)?;
            if image.alpha_ignored {
                eprintln!(
                    "pdfcer: paste-clipboard: the bitmap's fourth byte per pixel is not an alpha channel (or is zero everywhere), so the image is pasted opaque"
                );
            }
            bytes = pdfcer_render::export::encode_png(&image.pixmap, None)
                .map_err(|err| err.to_string())?;
            format = PasteFormat::Png;
            image_size(&bytes)
        }
    };
    Ok(Payload {
        format,
        bytes,
        natural: natural.filter(|(w, h)| *w > 0.0 && *h > 0.0),
    })
}

fn pdf_size(bytes: &[u8]) -> Option<(f64, f64)> {
    let doc = Document::from_bytes(bytes.to_vec()).ok()?;
    let pages = pdfcer_core::page_tree::pages(&doc).ok()?;
    let page = pages.first()?;
    Some((page.crop_box.width(), page.crop_box.height()))
}

#[cfg(feature = "svg-import")]
fn svg_size(bytes: &[u8]) -> Option<(f64, f64)> {
    pdfcer_core::svg_import::import(bytes)
        .ok()
        .map(|svg| svg.natural_size_pt())
}

#[cfg(not(feature = "svg-import"))]
fn svg_size(_bytes: &[u8]) -> Option<(f64, f64)> {
    None
}

fn image_size(bytes: &[u8]) -> Option<(f64, f64)> {
    pdfcer_core::image_import::import(bytes)
        .ok()
        .map(|img| img.natural_size_pt())
}

/// `--rect`, or the target page's crop box.
fn requested_rect(args: &PasteClipboardArgs<'_>) -> Result<Rect, u8> {
    if let Some(spec) = args.rect {
        return rect_from(spec).map_err(|msg| {
            eprintln!("pdfcer: --rect: {msg}");
            exit::EDIT_REFUSED
        });
    }
    let doc = open_for_read(args.input)?;
    let pages = pdfcer_core::page_tree::pages(&doc).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", args.input.display());
        exit::RUNTIME_ERROR
    })?;
    let Some(page) = args.page.checked_sub(1).and_then(|i| pages.get(i)) else {
        eprintln!(
            "pdfcer: {}: the document has {} page(s), so page {} does not exist",
            args.input.display(),
            pages.len(),
            args.page
        );
        return Err(exit::EDIT_REFUSED);
    };
    Ok(page.crop_box)
}

/// Where the content goes inside `requested`, and whether a natural-size
/// paste had to be made smaller. Unmeasured content fills `requested` the
/// way the placing command's own contain fit decides.
pub(crate) fn paste_rect(
    requested: Rect,
    natural: Option<(f64, f64)>,
    fit: PasteFit,
) -> (Rect, bool) {
    let r = Rect::from_corners(requested.llx, requested.lly, requested.urx, requested.ury);
    let Some((nw, nh)) = natural else {
        return (r, false);
    };
    let fill = (r.width() / nw).min(r.height() / nh);
    let (k, shrunk) = match fit {
        PasteFit::Stretch => return (r, false),
        PasteFit::Contain => (fill, false),
        PasteFit::Natural if fill < 1.0 => (fill, true),
        PasteFit::Natural => (1.0, false),
    };
    if !k.is_finite() || k <= 0.0 {
        return (r, false);
    }
    let (w, h) = (nw * k, nh * k);
    let llx = r.llx + (r.width() - w) / 2.0;
    let lly = r.lly + (r.height() - h) / 2.0;
    (Rect::from_corners(llx, lly, llx + w, lly + h), shrunk)
}

/// Hand the temp file to the command that places its kind. `rect` already
/// has the content's proportions when they are known, so the vector and
/// image commands are asked to contain, which then changes nothing.
fn place(args: &PasteClipboardArgs<'_>, format: PasteFormat, file: &Path, rect: Rect) -> u8 {
    let spec = format!("{},{},{},{}", rect.llx, rect.lly, rect.urx, rect.ury);
    let fit = if args.fit == PasteFit::Stretch {
        SvgFit::Stretch
    } else {
        SvgFit::Contain
    };
    match format {
        PasteFormat::Pdf => cmd_place_stamp(
            args.input,
            file,
            Some(1),
            None,
            args.page,
            Some(&spec),
            None,
            !args.stamp,
            args.output,
            args.mode,
        ),
        PasteFormat::Svg => cmd_add_svg(&AddSvgArgs {
            input: args.input,
            svg: file,
            page: args.page,
            rect: &spec,
            fit,
            stamp: args.stamp,
            markup: StampMarkup::default(),
            output: args.output,
            mode: args.mode,
            verify_undo: args.verify_undo,
        }),
        PasteFormat::Emf => cmd_add_emf(&AddEmfArgs {
            input: args.input,
            emf: file,
            page: args.page,
            rect: &spec,
            fit,
            stamp: args.stamp,
            markup: StampMarkup::default(),
            output: args.output,
            mode: args.mode,
            verify_undo: args.verify_undo,
        }),
        PasteFormat::Png | PasteFormat::Dib => cmd_add_image(&AddImageArgs {
            input: args.input,
            layer: None,
            hand_signature: None,
            image: file,
            page: args.page,
            rect: &spec,
            stretch: args.fit == PasteFit::Stretch,
            natural: false,
            compression: pdfcer_core::image_import::ImageCompression::default(),
            output: args.output,
            mode: args.mode,
            verify_undo: args.verify_undo,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Rect {
        Rect::from_corners(0.0, 0.0, 600.0, 800.0)
    }

    #[test]
    fn a_natural_paste_keeps_its_size_and_is_centred() {
        let (r, shrunk) = paste_rect(page(), Some((100.0, 50.0)), PasteFit::Natural);
        assert!(!shrunk);
        assert_eq!((r.llx, r.lly, r.urx, r.ury), (250.0, 375.0, 350.0, 425.0));
    }

    #[test]
    fn a_natural_paste_larger_than_the_page_is_made_smaller_and_says_so() {
        let (r, shrunk) = paste_rect(page(), Some((1200.0, 800.0)), PasteFit::Natural);
        assert!(shrunk);
        assert_eq!((r.width(), r.height()), (600.0, 400.0));
        assert_eq!(r.lly, 200.0);
    }

    #[test]
    fn contain_fills_and_stretch_takes_the_rectangle() {
        let (r, shrunk) = paste_rect(page(), Some((100.0, 50.0)), PasteFit::Contain);
        assert!(!shrunk);
        assert_eq!((r.width(), r.height()), (600.0, 300.0));
        let (r, _) = paste_rect(page(), Some((100.0, 50.0)), PasteFit::Stretch);
        assert_eq!((r.width(), r.height()), (600.0, 800.0));
        let (r, shrunk) = paste_rect(page(), None, PasteFit::Natural);
        assert_eq!((r.width(), r.height(), shrunk), (600.0, 800.0, false));
    }
}
