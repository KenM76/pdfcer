//! `3d-embed` and `3d-poster`: write a `/3D` annotation, or replace its
//! poster (ISO 32000-1 §13.6.2).

use super::*;
use pdfcer_core::threed::{PlaceholderReason, RenderedPoster, ThreeDPoster, ThreeDSpec};

/// The arguments of `3d-embed`, borrowed from the parsed command.
pub(crate) struct EmbedThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) model: &'a Path,
    pub(crate) page: u32,
    pub(crate) rect: &'a str,
    pub(crate) format: ThreeDFormatArg,
    pub(crate) poster: Option<&'a Path>,
    pub(crate) placeholder_poster: bool,
    pub(crate) activate: ThreeDActivateArg,
    pub(crate) desc: Option<&'a str>,
    pub(crate) color: Option<&'a str>,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// Read and import a poster image, or the exit code after the reason.
fn read_poster(path: &Path) -> Result<pdfcer_core::image_import::ImportedImage, u8> {
    let bytes = std::fs::read(path).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::IO_ERROR
    })?;
    pdfcer_core::image_import::import(&bytes).map_err(|err| {
        eprintln!("pdfcer: --poster {}: {err}", path.display());
        exit::EDIT_REFUSED
    })
}

/// The spec `3d-embed`'s options describe, or the exit code after the reason.
fn embed_spec(a: &EmbedThreeDArgs<'_>) -> Result<ThreeDSpec, u8> {
    use pdfcer_core::threed::{ThreeDActivation, ThreeDFormat};
    let refuse = |what: &str, err: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {what}: {err}");
        exit::EDIT_REFUSED
    };
    let rect = crate::annot_parse::rect_from(a.rect).map_err(|err| refuse("--rect", &err))?;
    let color = a
        .color
        .map(crate::annot_parse::parse_color)
        .transpose()
        .map_err(|err| refuse("--color", &err))?;
    let data = std::fs::read(a.model).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", a.model.display());
        exit::IO_ERROR
    })?;
    let built = match a.format {
        ThreeDFormatArg::Auto => ThreeDSpec::new(rect, data),
        ThreeDFormatArg::U3d => ThreeDSpec::with_format(rect, ThreeDFormat::U3d, data),
        ThreeDFormatArg::Prc => ThreeDSpec::with_format(rect, ThreeDFormat::Prc, data),
    };
    let mut spec = built.map_err(|err| refuse(&a.model.display().to_string(), &err))?;
    if let Some(path) = a.poster {
        spec.poster = Some(read_poster(path)?);
    }
    spec.render_poster = !a.placeholder_poster;
    spec.activation = match a.activate {
        ThreeDActivateArg::Click => ThreeDActivation::Click,
        ThreeDActivateArg::PageOpen => ThreeDActivation::PageOpen,
        ThreeDActivateArg::PageVisible => ThreeDActivation::PageVisible,
    };
    if let Some(c) = color {
        spec.color = c;
    }
    Ok(spec)
}

/// `3d-embed` — one summary line, then `inferred:` / `note:` lines.
pub(crate) fn cmd_embed_3d(a: &EmbedThreeDArgs<'_>) -> u8 {
    use pdfcer_core::edit::{MarkupNote, MarkupOptions};
    let Some(page_index) = (a.page as usize).checked_sub(1) else {
        eprintln!("pdfcer: --page is 1-based; 0 names no page");
        return exit::EDIT_REFUSED;
    };
    let spec = match embed_spec(a) {
        Ok(spec) => spec,
        Err(code) => return code,
    };
    let doc = match open_document(a.input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let options = MarkupOptions {
        note: a.desc.map(MarkupNote::new),
        ..Default::default()
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let outcome = match session.add_3d_annotation(page_index, &spec, &options) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    println!(
        "3d-embed {} page={} annot={} stream={} format={} bytes={} poster={} activate={} mode={} applied={}",
        a.input.display(),
        a.page,
        outcome.annot_id.num,
        outcome.stream_id.num,
        spec.format.label(),
        spec.data.len(),
        poster_token(&spec, &outcome.poster),
        String::from_utf8_lossy(spec.activation.name()),
        mode_token(a.mode),
        u32::from(a.apply)
    );
    if a.format == ThreeDFormatArg::Auto {
        println!(
            "inferred: format {} from the file's signature",
            spec.format.label()
        );
    }
    print_poster_notes(&outcome.poster);
    if outcome.below_required_version() {
        println!(
            "note: the document is PDF {} and {} needs PDF {}; a reader may show only the poster",
            outcome.document_version,
            spec.format.label(),
            outcome.required_version
        );
    }
    if !a.apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(a.input, &mut session, a.output, a.mode)
}

/// The summary's `poster=` value: `supplied:WxH`, `rendered:WxH` or
/// `placeholder`.
fn poster_token(spec: &ThreeDSpec, poster: &ThreeDPoster) -> String {
    match (poster, &spec.poster) {
        (ThreeDPoster::Supplied, Some(img)) => {
            let (w, h) = img.display_size_px();
            format!("supplied:{w}x{h}")
        }
        (ThreeDPoster::Rendered(r), _) => format!("rendered:{}x{}", r.width, r.height),
        _ => "placeholder".to_owned(),
    }
}

/// What the poster is, when pdfcer chose it.
fn print_poster_notes(poster: &ThreeDPoster) {
    match poster {
        ThreeDPoster::Rendered(r) => print_rendered_notes(r),
        ThreeDPoster::Placeholder(PlaceholderReason::Requested) => {}
        ThreeDPoster::Placeholder(why) => {
            println!("note: the poster is pdfcer's placeholder drawing: {why}");
        }
        _ => {}
    }
}

fn print_rendered_notes(r: &RenderedPoster) {
    println!(
        "inferred: poster rendered by pdfcer from the model ({} mesh(es), {} triangle(s)), \
         seen from above the front-right corner with z up, in perspective, each part in its \
         tree colour, or its texture, on white; the file's own views and lights are not read",
        r.meshes, r.triangles
    );
    if let Some(why) = &r.unplaced {
        println!("note: part placements are not applied ({why}); each mesh is drawn once");
    }
    if r.uncoloured_meshes > 0 {
        println!(
            "note: {} mesh(es) had no colour and are drawn grey",
            r.uncoloured_meshes
        );
    }
    super::print_alpha_unset_note(r.alpha_unset_meshes, "");
    super::print_texture_notes(r.textured_meshes, &r.texture_notes);
    if r.compressed_rebuilt > 0 {
        println!(
            "note: {} compressed mesh(es) were rebuilt by pdfcer's reconstruction of an \
             undocumented encoding",
            r.compressed_rebuilt
        );
    }
    let skipped = [
        (r.compressed_skipped, "compressed mesh(es)"),
        (r.wires_skipped, "wire tessellation(s)"),
        (r.markups_skipped, "markup tessellation(s)"),
    ];
    for (n, what) in skipped {
        if n > 0 {
            println!("note: {n} {what} not drawn in the poster");
        }
    }
}

/// The arguments of `3d-poster`, borrowed from the parsed command.
pub(crate) struct PosterThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) index: usize,
    pub(crate) image: &'a Path,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `3d-poster` — replace the poster of the `/3D` annotation `3d-list`
/// numbers `index`.
pub(crate) fn cmd_set_3d_poster(a: &PosterThreeDArgs<'_>) -> u8 {
    let image = match read_poster(a.image) {
        Ok(image) => image,
        Err(code) => return code,
    };
    let doc = match open_document(a.input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let target = found.get(a.index).and_then(|art| match art.source {
        pdfcer_core::threed::ThreeDSource::Stream { .. } => {
            art.annot_id.map(|id| (art.page_index, id))
        }
        _ => None,
    });
    let Some((page_index, annot_id)) = target else {
        eprintln!(
            "pdfcer: {}: index {} is not a 3D annotation (the document has {} 3D artwork(s); \
             a RichMedia model has no 3D poster). Run `pdfcer 3d-list` to see them.",
            a.input.display(),
            a.index,
            found.len()
        );
        return exit::EDIT_REFUSED;
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let outcome = match session.set_3d_poster(page_index, annot_id, &image) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (w, h) = image.display_size_px();
    println!(
        "3d-poster {} index={} page={} annot={} appearance={} image={} poster={w}x{h} mode={} applied={}",
        a.input.display(),
        a.index,
        page_index + 1,
        outcome.annot_id.num,
        outcome.appearance_id.num,
        outcome.poster_image_id.num,
        mode_token(a.mode),
        u32::from(a.apply)
    );
    if !a.apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(a.input, &mut session, a.output, a.mode)
}
