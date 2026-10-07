//! `object-copy`: write a selection to a self-contained clipboard payload —
//! page objects and annotations, or with `--leaf` objects inside one form
//! XObject.

use super::*;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::ObjectClip;

/// Arguments for [`cmd_object_copy`].
pub(crate) struct ObjectCopyArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--objects`, unparsed.
    pub(crate) objects: &'a str,
    /// `--objects` are form-leaf indices (`copy_objects_in_form`).
    pub(crate) leaf: bool,
    /// `--annotations`, unparsed.
    pub(crate) annotations: &'a str,
    pub(crate) clip: &'a Path,
    pub(crate) pdf: Option<&'a Path>,
    pub(crate) cut: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

fn refuse(message: &str) -> u8 {
    eprintln!("pdfcer: object-copy refused: {message}");
    exit::EDIT_REFUSED
}

/// `object-copy` — write a selection to a self-contained clipboard payload
/// (Pass 120.0/120.1; `--leaf` Pass 537.0), optionally cutting it from a copy
/// of the document.
///
/// # Why a FILE rather than the system clipboard
///
/// The requesting shell asked for exactly this split: *"I am not asking you to
/// touch the OS clipboard. That is mine. What I need from you is `to_bytes`."*
/// A one-shot CLI has no session to hold a clip in either, so a file is the
/// only place a payload can live between two invocations — which makes this
/// subcommand the CLI's whole clipboard, not a debug affordance.
pub(crate) fn cmd_object_copy(args: &ObjectCopyArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let (indices, annots) = match selection(args) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let clip = match take_clip(args, &mut session, page_index, &indices, &annots) {
        Ok(clip) => clip,
        Err(code) => return code,
    };
    let payload = clip.to_bytes();
    if let Err(err) = write_output(args.clip, &payload) {
        eprintln!("pdfcer: {}: {err}", args.clip.display());
        return exit::IO_ERROR;
    }
    let pdf_note = match export_pdf(args, &clip) {
        Ok(note) => note,
        Err(code) => return code,
    };
    // Constant `true` since the clip format carries annotations (pinned by
    // `object_clipboard`'s `a_clip_says_whether_serialisation_would_lose_anything`);
    // kept as the place a future unserialisable kind must be disclosed.
    if !clip.annotations_survive_serialisation() {
        eprintln!(
            "pdfcer: object-copy: {} annotation(s) were copied but are NOT carried by the clipboard file, so a paste from this file will place the content and not the annotations.",
            clip.annotation_count()
        );
    }
    let cut_note = match save_cut(args, &mut session, &source) {
        Ok(note) => note,
        Err(code) => return code,
    };
    println!(
        "object-copy {} page {} objects={} leaf={} -> {} ({} bytes); kinds={} resources={} {cut_note}",
        args.input.display(),
        args.page,
        clip.len(),
        u32::from(args.leaf),
        args.clip.display(),
        payload.len(),
        clip.kinds().join("+"),
        clip.resource_count(),
    );
    println!(
        "  annotations={} annotations_serialise={}",
        clip.annotation_count(),
        u32::from(clip.annotations_survive_serialisation())
    );
    println!("  {pdf_note}");
    exit::SUCCESS
}

/// `--objects` and `--annotations`, parsed, with the combinations refused.
fn selection(args: &ObjectCopyArgs<'_>) -> Result<(Vec<usize>, Vec<usize>), u8> {
    let indices = parse_object_indices(args.objects).map_err(|m| refuse(&m))?;
    let annots = parse_object_indices(args.annotations).map_err(|m| refuse(&m))?;
    if indices.is_empty() && annots.is_empty() {
        return Err(refuse(
            "nothing selected -- give --objects, --annotations, or both",
        ));
    }
    if args.leaf && (!annots.is_empty() || args.cut.is_some()) {
        return Err(refuse(
            "--leaf copies objects inside a form only: it takes neither --annotations nor --cut (delete them with object-delete --leaf)",
        ));
    }
    Ok((indices, annots))
}

/// Copy, or cut, the selection. Copy happens before any deletion so a
/// selection that cannot be copied is refused with nothing removed; a cut is
/// `cut_selection`, one undo entry covering objects and annotations alike.
fn take_clip(
    args: &ObjectCopyArgs<'_>,
    session: &mut EditSession,
    page_index: usize,
    indices: &[usize],
    annots: &[usize],
) -> Result<ObjectClip, u8> {
    let taken = if args.leaf {
        session.copy_objects_in_form(page_index, indices)
    } else if args.cut.is_some() {
        session.cut_selection(page_index, indices, annots)
    } else {
        session.copy_selection(page_index, indices, annots)
    };
    taken.map_err(|err| report_edit_error(args.input, &err))
}

/// `--pdf`: the interchange export, a second write of the same read.
fn export_pdf(args: &ObjectCopyArgs<'_>, clip: &ObjectClip) -> Result<String, u8> {
    let Some(path) = args.pdf else {
        return Ok(String::from("pdf=0"));
    };
    let exported = clip.to_pdf();
    if exported.size_substituted {
        eprintln!(
            "pdfcer: object-copy: the selection has no area in one direction, so the exported page was given a minimum size of {:.2}x{:.2} pt -- a zero-area /MediaBox produces a file readers refuse to open.",
            exported.size.0, exported.size.1
        );
    }
    if let Err(err) = write_output(path, &exported.bytes) {
        eprintln!("pdfcer: {}: {err}", path.display());
        return Err(exit::IO_ERROR);
    }
    Ok(format!(
        "pdf=1 pdf_out={} pdf_size={:.2}x{:.2} pdf_size_substituted={}",
        path.display(),
        exported.size.0,
        exported.size.1,
        u32::from(exported.size_substituted)
    ))
}

/// `--cut`: save the document the cut already edited.
fn save_cut(
    args: &ObjectCopyArgs<'_>,
    session: &mut EditSession,
    source: &[u8],
) -> Result<String, u8> {
    let Some(output) = args.cut else {
        return Ok(String::from("cut=0"));
    };
    let outcome = save_edited(
        session,
        source,
        output,
        args.mode,
        ProducerArg::Preserve,
        false,
    )?;
    let code = finish_edit(args.input, &outcome);
    if code != exit::SUCCESS {
        return Err(code);
    }
    Ok(format!("cut=1 cut_out={}", output.display()))
}
