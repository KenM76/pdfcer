//! `3d-list` / `3d-extract` / `3d-mesh`: embedded 3D artwork (ISO 32000-1
//! §13.6, ISO 32000-2 §13.7).

use super::*;
use pdfcer_core::threed::{ThreeDArtwork, ThreeDSource, extract_3d, list_3d_with_notes};

fn format_label(art: &ThreeDArtwork) -> String {
    art.declared
        .as_ref()
        .map_or_else(|| "-".to_owned(), |f| f.label())
}

/// `3d-list` — one line per artwork, then one line per listing note.
pub(crate) fn cmd_list_3d(input: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let (found, notes) = list_3d_with_notes(&doc);
    for (index, art) in found.iter().enumerate() {
        let source = match &art.source {
            ThreeDSource::Stream { shared: false } => "stream".to_owned(),
            ThreeDSource::Stream { shared: true } => "shared-stream".to_owned(),
            ThreeDSource::RichMediaAsset { name, .. } => {
                format!("richmedia name={:?}", name.as_deref().unwrap_or("-"))
            }
            _ => "unknown".to_owned(),
        };
        println!(
            "3d index={index} page={} format={} views={} poster={} source={source}",
            art.page_index + 1,
            format_label(art),
            art.view_count,
            if art.has_poster { "yes" } else { "no" },
        );
    }
    println!("count={}", found.len());
    if notes.annotations_without_stream > 0 {
        println!(
            "note: {} 3D annotation(s) name no readable data stream",
            notes.annotations_without_stream
        );
    }
    if notes.truncated {
        println!("note: listing truncated at a safety limit");
    }
    if notes.page_tree_unwalkable {
        println!("note: the page tree could not be walked; nothing was listed");
    }
    exit::SUCCESS
}

/// `3d-extract` — write one artwork's decoded bytes to `output`.
pub(crate) fn cmd_extract_3d(input: &Path, index: usize, output: &Path) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let Some(art) = found.get(index) else {
        eprintln!(
            "pdfcer: {}: no 3D artwork at index {index} (the document has {}). Run \
             `pdfcer 3d-list` to see them.",
            input.display(),
            found.len()
        );
        return exit::EDIT_REFUSED;
    };
    let extracted = match extract_3d(&doc.view(), art) {
        Ok(extracted) => extracted,
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if let Err(err) = write_output(output, &extracted.data) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let sniffed = extracted
        .sniffed
        .as_ref()
        .map_or_else(|| "unrecognised".to_owned(), |f| f.label());
    println!(
        "extracted index={index} format={} bytes={} content={sniffed} -> {}",
        format_label(art),
        extracted.data.len(),
        output.display()
    );
    if extracted.contradicts(art.declared.as_ref()) {
        println!(
            "note: the document declares {} but the bytes are {sniffed}; written unchanged",
            format_label(art)
        );
    }
    exit::SUCCESS
}

/// `3d-mesh` — decode one PRC model's tessellation and write its triangle
/// meshes as STL or OBJ.
pub(crate) fn cmd_mesh_3d(input: &Path, index: usize, output: &Path, format: MeshFormat) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let Some(art) = found.get(index) else {
        eprintln!(
            "pdfcer: {}: no 3D artwork at index {index} (the document has {}). Run \
             `pdfcer 3d-list` to see them.",
            input.display(),
            found.len()
        );
        return exit::EDIT_REFUSED;
    };
    let extracted = match extract_3d(&doc.view(), art) {
        Ok(extracted) => extracted,
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    mesh_from_bytes(input, index, &extracted.data, output, format)
}

#[cfg(feature = "3d")]
fn mesh_from_bytes(
    input: &Path,
    index: usize,
    data: &[u8],
    output: &Path,
    format: MeshFormat,
) -> u8 {
    use pdfcer_3d::{PrcFile, Tessellation};
    let refuse = |why: String| {
        eprintln!("pdfcer: {}: 3D artwork {index}: {why}", input.display());
        exit::EDIT_REFUSED
    };
    if !data.starts_with(b"PRC") {
        return refuse(
            "not a PRC model; only PRC is decoded (use `3d-extract` for the bytes)".to_owned(),
        );
    }
    let prc = match PrcFile::parse(data) {
        Ok(prc) => prc,
        Err(err) => return refuse(err.to_string()),
    };
    let (mut meshes, mut wires, mut markups) = (Vec::new(), 0usize, 0usize);
    let (mut rebuilt, mut compressed) = (0usize, 0usize);
    // Per file structure, each tessellation's triangle mesh (if it has one).
    let mut by_index: Vec<Vec<Option<pdfcer_3d::TriangleMesh>>> = Vec::new();
    for fs in &prc.file_structures {
        let tess = match fs.tessellations() {
            Ok(tess) => tess,
            Err(err) => return refuse(err.to_string()),
        };
        let mut row = Vec::with_capacity(tess.len());
        for t in tess {
            row.push(match t {
                Tessellation::Mesh(m) => Some(m),
                Tessellation::Wire(_) => {
                    wires += 1;
                    None
                }
                Tessellation::Compressed { mesh: Some(m), .. } => {
                    rebuilt += 1;
                    Some(m)
                }
                Tessellation::Compressed { .. } => {
                    compressed += 1;
                    None
                }
                _ => {
                    markups += 1;
                    None
                }
            });
        }
        by_index.push(row);
    }
    // Place each mesh where the assembly tree draws it; a tree pdfcer cannot
    // read yet falls back to every mesh once, in its own coordinates.
    let unplaced = match prc.placements() {
        Ok(placements) if !placements.is_empty() => {
            for p in &placements {
                let mesh = by_index
                    .get(p.file_structure)
                    .and_then(|row| row.get(p.tessellation))
                    .and_then(Option::as_ref);
                if let Some(mesh) = mesh {
                    meshes.push(mesh.transformed(&p.matrix));
                }
            }
            None
        }
        Ok(_) => Some("the model's tree places no tessellation".to_owned()),
        Err(err) => Some(err.to_string()),
    };
    if unplaced.is_some() {
        meshes.extend(by_index.into_iter().flatten().flatten());
    }
    let triangles: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    if triangles == 0 {
        return refuse(if compressed > 0 {
            format!(
                "the model's {compressed} mesh(es) use compressed tessellation in a form \
                 pdfcer does not yet rebuild into triangles"
            )
        } else {
            "the model holds no triangle tessellation".to_owned()
        });
    }
    let bytes = match format {
        MeshFormat::Stl => match pdfcer_3d::to_stl(&meshes) {
            Ok(bytes) => bytes,
            Err(err) => return refuse(err.to_string()),
        },
        MeshFormat::Obj => pdfcer_3d::to_obj(&meshes).into_bytes(),
    };
    if let Err(err) = write_output(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    let recalculated = meshes.iter().filter(|m| m.normals_recalculated).count();
    println!(
        "meshed index={index} meshes={} triangles={triangles} wires_skipped={wires} \
         markup_skipped={markups} compressed_rebuilt={rebuilt} compressed_skipped={compressed} \
         -> {}",
        meshes.len(),
        output.display()
    );
    if let Some(why) = &unplaced {
        println!(
            "note: part placements are not applied ({why}); each mesh is written once, in its own coordinates"
        );
    }
    if rebuilt > 0 {
        println!(
            "note: {rebuilt} compressed mesh(es) were rebuilt by pdfcer's reconstruction of an \
             undocumented encoding; each step is exact to the model's stated tolerance, and \
             small drift can accumulate across a mesh"
        );
    }
    if recalculated > 0 {
        println!(
            "note: {recalculated} mesh(es) store no normals; facet normals are computed from \
             the triangle winding"
        );
    }
    exit::SUCCESS
}

#[cfg(not(feature = "3d"))]
fn mesh_from_bytes(
    input: &Path,
    index: usize,
    _data: &[u8],
    _output: &Path,
    _format: MeshFormat,
) -> u8 {
    eprintln!(
        "pdfcer: {}: 3D artwork {index}: this pdfcer was built without the `3d` feature; \
         3d-mesh is unavailable",
        input.display()
    );
    exit::EDIT_REFUSED
}

/// The arguments of `3d-embed`, borrowed from the parsed command.
pub(crate) struct EmbedThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) model: &'a Path,
    pub(crate) page: u32,
    pub(crate) rect: &'a str,
    pub(crate) format: ThreeDFormatArg,
    pub(crate) poster: Option<&'a Path>,
    pub(crate) activate: ThreeDActivateArg,
    pub(crate) desc: Option<&'a str>,
    pub(crate) color: Option<&'a str>,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `3d-embed` — one summary line, then `note:` lines.
pub(crate) fn cmd_embed_3d(a: &EmbedThreeDArgs<'_>) -> u8 {
    use pdfcer_core::edit::{MarkupNote, MarkupOptions};
    use pdfcer_core::threed::{ThreeDActivation, ThreeDFormat, ThreeDSpec};

    let rect = match crate::annot_parse::rect_from(a.rect) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: --rect: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let color = match a.color.map(crate::annot_parse::parse_color).transpose() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("pdfcer: --color: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let Some(page_index) = (a.page as usize).checked_sub(1) else {
        eprintln!("pdfcer: --page is 1-based; 0 names no page");
        return exit::EDIT_REFUSED;
    };
    let data = match std::fs::read(a.model) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.model.display());
            return exit::IO_ERROR;
        }
    };
    let built = match a.format {
        ThreeDFormatArg::Auto => ThreeDSpec::new(rect, data),
        ThreeDFormatArg::U3d => ThreeDSpec::with_format(rect, ThreeDFormat::U3d, data),
        ThreeDFormatArg::Prc => ThreeDSpec::with_format(rect, ThreeDFormat::Prc, data),
    };
    let mut spec = match built {
        Ok(spec) => spec,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.model.display());
            return exit::EDIT_REFUSED;
        }
    };
    if let Some(path) = a.poster {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
        };
        match pdfcer_core::image_import::import(&bytes) {
            Ok(img) => spec.poster = Some(img),
            Err(err) => {
                eprintln!("pdfcer: --poster {}: {err}", path.display());
                return exit::EDIT_REFUSED;
            }
        }
    }
    spec.activation = match a.activate {
        ThreeDActivateArg::Click => ThreeDActivation::Click,
        ThreeDActivateArg::PageOpen => ThreeDActivation::PageOpen,
        ThreeDActivateArg::PageVisible => ThreeDActivation::PageVisible,
    };
    if let Some(c) = color {
        spec.color = c;
    }
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
        spec.poster.as_ref().map_or_else(
            || "placeholder".to_owned(),
            |img| {
                let (w, h) = img.display_size_px();
                format!("{w}x{h}")
            }
        ),
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
