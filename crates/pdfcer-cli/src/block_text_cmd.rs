//! `pdfcer edit-block-text`: replace a recognised block's whole text and
//! re-wrap it (`EditSession::edit_block_text`).

use super::*;
use pdfcer_core::text_edit::{BlockEditError, BlockEditOptions, BlockEditReport, EditOptions};

/// The flags `edit-block-text` takes, bundled to keep the call site
/// readable.
pub(crate) struct BlockTextArgs<'a> {
    pub(crate) input: &'a Path,
    /// 1-based.
    pub(crate) page: usize,
    pub(crate) block: Option<usize>,
    pub(crate) at: Option<&'a str>,
    pub(crate) text: Option<&'a str>,
    pub(crate) text_file: Option<&'a Path>,
    pub(crate) width: Option<f64>,
    pub(crate) sibling_fonts: bool,
    /// `--cid-font-program`.
    pub(crate) cid_font_program: pdfcer_core::text_edit::CidFontProgram,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// Implement `pdfcer edit-block-text`: resolve the block, edit, print the
/// report before saving (the invocation is the commit), save.
pub(crate) fn cmd_edit_block_text(args: &BlockTextArgs<'_>) -> u8 {
    let input = args.input;
    if args.page == 0 {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let text = match new_text(args) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let page = args.page - 1;
    let block = match resolve_block(&session, page, args) {
        Ok(b) => b,
        Err(code) => return code,
    };
    let edit = EditOptions::default()
        .with_embedded_glyphs(&pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs)
        .with_sibling_fonts(args.sibling_fonts)
        .with_cid_font_program(args.cid_font_program);
    let opts = BlockEditOptions::new()
        .with_wrap_width_opt(args.width)
        .with_edit_options(edit);
    let report = match session.edit_block_text(page, block, &text, &opts) {
        Ok(r) => r,
        Err(err) => return refused(input, &err),
    };
    print_report(&report);
    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(o) => o,
        Err(code) => return code,
    };
    finish_edit(input, &saved)
}

fn new_text(args: &BlockTextArgs<'_>) -> Result<String, u8> {
    match (args.text, args.text_file) {
        (Some(t), None) => Ok(t.to_owned()),
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|err| {
            eprintln!("pdfcer: {}: {err}", path.display());
            exit::IO_ERROR
        }),
        _ => {
            eprintln!(
                "pdfcer: {}: give exactly one of --text or --text-file",
                args.input.display()
            );
            Err(exit::RUNTIME_ERROR)
        }
    }
}

/// `--block N`, or the block under `--at x,y`, which is printed with its
/// current text so a script sees what it is about to replace.
fn resolve_block(
    session: &pdfcer_core::edit::EditSession,
    page: usize,
    args: &BlockTextArgs<'_>,
) -> Result<usize, u8> {
    let input = args.input;
    let Some(at) = args.at else {
        return args.block.ok_or_else(|| {
            eprintln!("pdfcer: {}: give --block N or --at x,y", input.display());
            exit::RUNTIME_ERROR
        });
    };
    let (x, y) = parse_xy(at).map_err(|e| {
        eprintln!("pdfcer: {}: --at: {e}", input.display());
        exit::RUNTIME_ERROR
    })?;
    match session.block_at_point(page, x, y) {
        Ok(Some(hit)) => {
            println!("block: {} (at {x},{y})", hit.block_index);
            println!("old text: {:?}", hit.text);
            Ok(hit.block_index)
        }
        Ok(None) => {
            eprintln!(
                "pdfcer: {}: no text block at {x},{y} on page {}",
                input.display(),
                page + 1
            );
            Err(exit::EDIT_REFUSED)
        }
        Err(err) => Err(refused(input, &err)),
    }
}

fn parse_xy(s: &str) -> Result<(f64, f64), String> {
    let mut parts = s.split(',').map(str::trim);
    let (Some(x), Some(y), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("expected \"x,y\", got {s:?}"));
    };
    let num = |v: &str| v.parse::<f64>().map_err(|e| format!("{v:?}: {e}"));
    Ok((num(x)?, num(y)?))
}

fn refused(input: &Path, err: &BlockEditError) -> u8 {
    use pdfcer_core::text_edit::{EditError, ReflowApplyError};
    eprintln!(
        "pdfcer: {}: edit-block-text refused: {err}",
        input.display()
    );
    match err {
        BlockEditError::Block(ReflowApplyError::Write(_))
        | BlockEditError::Text(EditError::Write(_)) => exit::SAVE_REFUSED,
        BlockEditError::Block(
            ReflowApplyError::Extract(_)
            | ReflowApplyError::Content(_)
            | ReflowApplyError::PageTree(_),
        )
        | BlockEditError::Text(EditError::Content(_) | EditError::PageTree(_)) => {
            exit::RUNTIME_ERROR
        }
        _ => exit::EDIT_REFUSED,
    }
}

/// Every report field, then every disclosure (rule 4: the invocation is the
/// commit, so what was inferred or substituted is printed).
fn print_report(r: &BlockEditReport) {
    println!("block: {}", r.block_index);
    println!("lines: {} -> {}", r.lines_before, r.lines_after);
    println!("wrap width: {:.2} pt", r.wrap_width);
    println!("alignment: {:?}", r.alignment);
    println!("looks: {}", r.looks);
    println!("height change: {:+.2} pt", r.height_delta);
    if let Some(pt) = r.overflow_pt {
        println!("overflow: {pt:.2} pt below the original bottom");
    }
    if let Some(po) = r.page_overflow {
        println!(
            "page overflow: {:.2} pt below, {:.2} pt right, {} line(s) outside",
            po.past_bottom_pt, po.past_right_pt, po.lines_outside
        );
    }
    println!("font: {}", r.base_font);
    if let Some(from) = &r.font_substituted_from {
        println!("font substituted from: {from}");
    }
    if !r.glyphs_added.is_empty() {
        let added: String = r.glyphs_added.iter().collect();
        println!("glyphs added: {added:?}");
    }
    if let Some(mcid) = r.tagged_mcid {
        println!("tagged: MCID {mcid}");
    }
    println!(
        "marked-content sequences removed: {}",
        r.marked_content_removed
    );
    println!("content object: {}", r.content_object);
    if r.extra_objects_emptied > 0 {
        println!(
            "further content streams emptied: {}",
            r.extra_objects_emptied
        );
    }
    for d in &r.disclosures {
        println!("note: {d}");
    }
}
