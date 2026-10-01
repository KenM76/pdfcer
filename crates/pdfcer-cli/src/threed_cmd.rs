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
    let data = match artwork_bytes(input, index) {
        Ok(data) => data,
        Err(code) => return code,
    };
    mesh_from_bytes(input, index, &data, output, format)
}

/// The decoded bytes of the 3D artwork at `index`, or the exit code after
/// the reason is printed.
fn artwork_bytes(input: &Path, index: usize) -> Result<Vec<u8>, u8> {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return Err(exit_code_for_doc(&err));
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
        return Err(exit::EDIT_REFUSED);
    };
    match extract_3d(&doc.view(), art) {
        Ok(extracted) => Ok(extracted.data),
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            Err(exit::EDIT_REFUSED)
        }
    }
}

/// A PRC model's triangle meshes, placed where its tree draws them, and
/// counts of what was not drawn.
#[cfg(feature = "3d")]
struct Assembled {
    meshes: Vec<pdfcer_3d::TriangleMesh>,
    /// Each mesh's colour from the model's tree, straight RGBA.
    colours: Vec<Option<[u8; 4]>>,
    triangles: usize,
    wires: usize,
    markups: usize,
    rebuilt: usize,
    compressed: usize,
    /// Why placements were not applied, when they were not.
    unplaced: Option<String>,
}

/// A 0-1 colour as 8-bit straight RGBA.
#[cfg(feature = "3d")]
fn to_rgba8(c: [f64; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `mesh` split into one mesh per distinct entry of `per` (one per
/// triangle), in order of first appearance, so each draws in its own colour.
#[cfg(feature = "3d")]
fn split_by_colour(
    mesh: &pdfcer_3d::TriangleMesh,
    per: &[Option<[u8; 4]>],
) -> Vec<(pdfcer_3d::TriangleMesh, Option<[u8; 4]>)> {
    type Group = (Option<[u8; 4]>, Vec<usize>);
    let mut groups: Vec<Group> = Vec::new();
    for (k, colour) in per.iter().enumerate().take(mesh.triangles.len()) {
        match groups.iter_mut().find(|g| g.0 == *colour) {
            Some(g) => g.1.push(k),
            None => groups.push((*colour, vec![k])),
        }
    }
    let pick = |from: &[[u32; 3]], keep: &[usize]| -> Vec<[u32; 3]> {
        keep.iter().filter_map(|&k| from.get(k).copied()).collect()
    };
    groups
        .into_iter()
        .map(|(colour, keep)| {
            let mut part = mesh.clone();
            part.triangles = pick(&mesh.triangles, &keep);
            if !mesh.triangle_normals.is_empty() {
                part.triangle_normals = pick(&mesh.triangle_normals, &keep);
            }
            part.faces = std::iter::once(0..part.triangles.len()).collect();
            (part, colour)
        })
        .collect()
}

/// Decode and assemble a PRC model, or say why it has nothing to draw.
#[cfg(feature = "3d")]
fn assemble(data: &[u8]) -> Result<Assembled, String> {
    use pdfcer_3d::{PrcFile, Tessellation};
    if !data.starts_with(b"PRC") {
        return Err(
            "not a PRC model; only PRC is decoded (use `3d-extract` for the bytes)".to_owned(),
        );
    }
    let prc = PrcFile::parse(data).map_err(|err| err.to_string())?;
    let (mut meshes, mut wires, mut markups) = (Vec::new(), 0usize, 0usize);
    let mut colours = Vec::new();
    let (mut rebuilt, mut compressed) = (0usize, 0usize);
    // Per file structure, each tessellation's triangle mesh (if it has one).
    let mut by_index: Vec<Vec<Option<pdfcer_3d::TriangleMesh>>> = Vec::new();
    for fs in &prc.file_structures {
        let tess = fs.tessellations().map_err(|err| err.to_string())?;
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
                    let placed = mesh.transformed(&p.matrix);
                    match p.triangle_colours(mesh) {
                        Some(per) => {
                            let per: Vec<_> = per.into_iter().map(|c| c.map(to_rgba8)).collect();
                            for (part, colour) in split_by_colour(&placed, &per) {
                                meshes.push(part);
                                colours.push(colour);
                            }
                        }
                        None => {
                            meshes.push(placed);
                            colours.push(p.colour.map(to_rgba8));
                        }
                    }
                }
            }
            None
        }
        Ok(_) => Some("the model's tree places no tessellation".to_owned()),
        Err(err) => Some(err.to_string()),
    };
    if unplaced.is_some() {
        meshes.extend(by_index.into_iter().flatten().flatten());
        colours.clear();
    }
    let triangles: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    if triangles == 0 {
        return Err(if compressed > 0 {
            format!(
                "the model's {compressed} mesh(es) use compressed tessellation in a form \
                 pdfcer does not yet rebuild into triangles"
            )
        } else {
            "the model holds no triangle tessellation".to_owned()
        });
    }
    Ok(Assembled {
        meshes,
        colours,
        triangles,
        wires,
        markups,
        rebuilt,
        compressed,
        unplaced,
    })
}

/// The notes both `3d-mesh` and `3d-render` print about what was inferred.
#[cfg(feature = "3d")]
fn print_assembly_notes(a: &Assembled, drawn: &str) {
    if let Some(why) = &a.unplaced {
        println!(
            "note: part placements are not applied ({why}); each mesh is {drawn} once, in its own coordinates"
        );
    }
    if a.rebuilt > 0 {
        println!(
            "note: {} compressed mesh(es) were rebuilt by pdfcer's reconstruction of an \
             undocumented encoding; each step is exact to the model's stated tolerance, and \
             small drift can accumulate across a mesh",
            a.rebuilt
        );
    }
}

#[cfg(feature = "3d")]
fn mesh_from_bytes(
    input: &Path,
    index: usize,
    data: &[u8],
    output: &Path,
    format: MeshFormat,
) -> u8 {
    let refuse = |why: String| {
        eprintln!("pdfcer: {}: 3D artwork {index}: {why}", input.display());
        exit::EDIT_REFUSED
    };
    let a = match assemble(data) {
        Ok(a) => a,
        Err(why) => return refuse(why),
    };
    let bytes = match format {
        MeshFormat::Stl => match pdfcer_3d::to_stl(&a.meshes) {
            Ok(bytes) => bytes,
            Err(err) => return refuse(err.to_string()),
        },
        MeshFormat::Obj => pdfcer_3d::to_obj(&a.meshes).into_bytes(),
    };
    if let Err(err) = write_output(output, &bytes) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "meshed index={index} meshes={} triangles={} wires_skipped={} \
         markup_skipped={} compressed_rebuilt={} compressed_skipped={} -> {}",
        a.meshes.len(),
        a.triangles,
        a.wires,
        a.markups,
        a.rebuilt,
        a.compressed,
        output.display()
    );
    print_assembly_notes(&a, "written");
    let recalculated = a.meshes.iter().filter(|m| m.normals_recalculated).count();
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
    no_3d_feature(input, index, "3d-mesh")
}

#[cfg(not(feature = "3d"))]
fn no_3d_feature(input: &Path, index: usize, command: &str) -> u8 {
    eprintln!(
        "pdfcer: {}: 3D artwork {index}: this pdfcer was built without the `3d` feature; \
         {command} is unavailable",
        input.display()
    );
    exit::EDIT_REFUSED
}

/// The arguments of `3d-render`, borrowed from the parsed command.
pub(crate) struct RenderThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) index: usize,
    pub(crate) output: &'a Path,
    pub(crate) view: ThreeDView,
    pub(crate) up: Axis3,
    pub(crate) eye: Option<[f64; 3]>,
    pub(crate) target: Option<[f64; 3]>,
    pub(crate) ortho: bool,
    pub(crate) fov: f64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) transparent: bool,
}

/// `3d-render` — draw one PRC model to a PNG from a camera.
pub(crate) fn cmd_render_3d(a: &RenderThreeDArgs<'_>) -> u8 {
    let data = match artwork_bytes(a.input, a.index) {
        Ok(data) => data,
        Err(code) => return code,
    };
    render_from_bytes(a, &data)
}

#[cfg(feature = "3d")]
fn render_from_bytes(a: &RenderThreeDArgs<'_>, data: &[u8]) -> u8 {
    use pdfcer_3d::{Bounds, Camera, Projection, RenderOptions, render_coloured};
    let refuse = |why: String| {
        eprintln!(
            "pdfcer: {}: 3D artwork {}: {why}",
            a.input.display(),
            a.index
        );
        exit::EDIT_REFUSED
    };
    let model = match assemble(data) {
        Ok(model) => model,
        Err(why) => return refuse(why),
    };
    let Some(bounds) = Bounds::of(&model.meshes) else {
        return refuse("the model has no finite vertex to draw".to_owned());
    };
    let target = a.target.unwrap_or_else(|| bounds.centre());
    let (direction, up) = match a.eye {
        Some([ex, ey, ez]) => {
            let [tx, ty, tz] = target;
            ([tx - ex, ty - ey, tz - ez], a.up.vector())
        }
        None => a.view.direction(a.up),
    };
    let aspect = f64::from(a.width) / f64::from(a.height.max(1));
    let mut camera = match Camera::fit_meshes(&model.meshes, direction, up, !a.ortho, aspect) {
        Ok(camera) => camera,
        Err(err) => return refuse(err.to_string()),
    };
    if let Some(eye) = a.eye {
        camera.eye = eye;
        camera.target = target;
    }
    if let Projection::Perspective { fov_y } = &mut camera.projection {
        *fov_y = a.fov;
    }
    let options = RenderOptions {
        width: a.width,
        height: a.height,
        background: if a.transparent {
            [0, 0, 0, 0]
        } else {
            [255, 255, 255, 255]
        },
        ..RenderOptions::default()
    };
    let image = match render_coloured(&model.meshes, &model.colours, &camera, &options) {
        Ok(image) => image,
        Err(err) => return refuse(err.to_string()),
    };
    // The pixmap holds premultiplied alpha; glass over a transparent
    // background leaves partly transparent pixels.
    let mut rgba = image.rgba;
    for px in rgba.chunks_exact_mut(4) {
        if let [r, g, b, alpha] = px {
            for c in [r, g, b] {
                *c = (u16::from(*c) * u16::from(*alpha)).div_ceil(255) as u8;
            }
        }
    }
    let png = pdfcer_render::tiny_skia::IntSize::from_wh(image.width, image.height)
        .and_then(|size| pdfcer_render::tiny_skia::Pixmap::from_vec(rgba, size))
        .ok_or_else(|| "the rendered image has an invalid size".to_owned())
        .and_then(|pixmap| {
            pdfcer_render::export::encode_png(&pixmap, None).map_err(|err| err.to_string())
        });
    let png = match png {
        Ok(png) => png,
        Err(why) => return refuse(why),
    };
    if let Err(err) = write_output(a.output, &png) {
        eprintln!("pdfcer: {}: {err}", a.output.display());
        return exit::IO_ERROR;
    }
    let fmt3 = |p: [f64; 3]| p.map(|c| format!("{c:.6}")).join(",");
    println!(
        "rendered index={} meshes={} triangles={} wires_skipped={} markup_skipped={} \
         compressed_skipped={} width={} height={} projection={} eye={} target={} -> {}",
        a.index,
        model.meshes.len(),
        model.triangles,
        model.wires,
        model.markups,
        model.compressed,
        a.width,
        a.height,
        match camera.projection {
            Projection::Perspective { .. } => "perspective",
            _ => "orthographic",
        },
        fmt3(camera.eye),
        fmt3(camera.target),
        a.output.display()
    );
    print_assembly_notes(&model, "drawn");
    let uncoloured = model.meshes.len() - model.colours.iter().flatten().count();
    let translucent = model
        .colours
        .iter()
        .flatten()
        .filter(|c| c[3] < 255)
        .count();
    println!(
        "note: each part, and each face styled on its own, is drawn in the colour its model \
         tree gives it ({translucent} translucent), lit from the camera; {uncoloured} mesh(es) \
         had none and are drawn grey; textures, lights and views are not read yet"
    );
    exit::SUCCESS
}

#[cfg(not(feature = "3d"))]
fn render_from_bytes(a: &RenderThreeDArgs<'_>, _data: &[u8]) -> u8 {
    no_3d_feature(a.input, a.index, "3d-render")
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

#[cfg(all(test, feature = "3d"))]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_splits_by_triangle_colour_in_first_seen_order() {
        let mut mesh = pdfcer_3d::TriangleMesh::default();
        mesh.positions = vec![[0.0; 3]; 4];
        mesh.triangles = vec![[0, 1, 2], [0, 2, 3], [1, 2, 3]];
        mesh.normals = vec![[0.0, 0.0, 1.0]; 3];
        mesh.triangle_normals = vec![[0, 0, 0], [1, 1, 1], [2, 2, 2]];
        let red = Some([255, 0, 0, 255]);
        let parts = split_by_colour(&mesh, &[red, None, red]);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, red);
        assert_eq!(parts[0].0.triangles, [[0, 1, 2], [1, 2, 3]]);
        assert_eq!(parts[0].0.faces.len(), 1);
        assert_eq!(parts[0].0.faces[0], 0..2);
        assert_eq!(parts[1].1, None);
        assert_eq!(parts[1].0.triangles, [[0, 2, 3]]);
        assert_eq!(parts[1].0.positions.len(), 4);
        assert_eq!(parts[0].0.triangle_normals, [[0, 0, 0], [2, 2, 2]]);
        assert_eq!(parts[1].0.triangle_normals, [[1, 1, 1]]);
    }
}
