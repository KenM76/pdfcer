//! `3d-list` / `3d-extract` / `3d-mesh`: embedded 3D artwork (ISO 32000-1
//! §13.6, ISO 32000-2 §13.7).

use super::*;
use pdfcer_core::threed::{ThreeDArtwork, ThreeDSource, extract_3d, list_3d_with_notes};

#[cfg(feature = "3d")]
mod aim;
mod embed;
mod tree;
#[cfg(feature = "3d")]
use aim::aim_camera;
pub(crate) use embed::{EmbedThreeDArgs, PosterThreeDArgs, cmd_embed_3d, cmd_set_3d_poster};
pub(crate) use tree::cmd_tree_3d;

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
pub(crate) fn cmd_mesh_3d(
    input: &Path,
    index: usize,
    output: &Path,
    how: (MeshFormat, MeshFitArg, bool),
) -> u8 {
    let data = match artwork_bytes(input, index) {
        Ok((data, _)) => data,
        Err(code) => return code,
    };
    mesh_from_bytes(input, index, &data, output, how)
}

/// The decoded bytes of the 3D artwork at `index` and the view it opens
/// on, or the exit code after the reason is printed.
fn artwork_bytes(
    input: &Path,
    index: usize,
) -> Result<(Vec<u8>, Option<pdfcer_core::threed::ThreeDSavedView>), u8> {
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
        Ok(extracted) => Ok((
            extracted.data,
            pdfcer_core::threed::default_3d_view(&doc, art),
        )),
        Err(err) => {
            eprintln!("pdfcer: {}: 3D artwork {index}: {err}", input.display());
            Err(exit::EDIT_REFUSED)
        }
    }
}

/// Decode and assemble a PRC model with explicit readings of what the
/// standard leaves open, or say why it has nothing to draw.
#[cfg(feature = "3d")]
fn assemble_with(
    data: &[u8],
    options: pdfcer_3d::AssembleOptions,
) -> Result<pdfcer_3d::AssembledModel, String> {
    pdfcer_3d::assemble_with_options(data, &options).map_err(|err| match err {
        pdfcer_3d::AssembleError::NotPrc => format!("{err} (use `3d-extract` for the bytes)"),
        _ => err.to_string(),
    })
}

#[cfg(feature = "3d")]
fn mesh_fit(fit: MeshFitArg) -> pdfcer_3d::MeshFit {
    match fit {
        MeshFitArg::Best => pdfcer_3d::MeshFit::BestFit,
        MeshFitArg::Unique => pdfcer_3d::MeshFit::Unique,
    }
}

#[cfg(feature = "3d")]
fn stored_visibility(draw_hidden: bool) -> pdfcer_3d::StoredVisibility {
    if draw_hidden {
        pdfcer_3d::StoredVisibility::DrawAll
    } else {
        pdfcer_3d::StoredVisibility::Honour
    }
}

/// `3d-render`'s choices where ISO 14739-1 is silent.
#[cfg(feature = "3d")]
fn assemble_options(a: &RenderThreeDArgs<'_>) -> pdfcer_3d::AssembleOptions {
    let mut options = pdfcer_3d::AssembleOptions::default();
    options.style_alpha = match a.style_alpha {
        StyleAlphaArg::ZeroUnset => pdfcer_3d::StyleAlpha::ZeroUnset,
        StyleAlphaArg::Style => pdfcer_3d::StyleAlpha::StyleWins,
        StyleAlphaArg::Multiply => pdfcer_3d::StyleAlpha::Multiply,
    };
    options.entity_overrides = match a.entity_overrides {
        EntityOverridesArg::Subtree => pdfcer_3d::EntityOverrides::Subtree,
        EntityOverridesArg::Everywhere => pdfcer_3d::EntityOverrides::Everywhere,
        EntityOverridesArg::Ignore => pdfcer_3d::EntityOverrides::Ignore,
    };
    options.mesh_fit = mesh_fit(a.mesh_fit);
    options.stored_visibility = stored_visibility(a.draw_hidden);
    options.texture_origin = match a.texture_origin {
        TextureOriginArg::Bottom => pdfcer_3d::TextureOrigin::BottomLeft,
        TextureOriginArg::Top => pdfcer_3d::TextureOrigin::TopLeft,
    };
    options.wrap_base = match a.texture_wrap_base {
        TextureWrapBaseArg::Zero => pdfcer_3d::WrapBase::ZeroBased,
        TextureWrapBaseArg::One => pdfcer_3d::WrapBase::OneBased,
    };
    options.picture_files = match a.texture_pictures {
        TexturePicturesArg::Structure => pdfcer_3d::PictureFiles::StructureFirst,
        TexturePicturesArg::Header => pdfcer_3d::PictureFiles::HeaderFirst,
    };
    options
}

/// The note for compressed meshes only a best-fit search rebuilt.
fn print_best_fit_note(n: usize) {
    if n > 0 {
        println!(
            "note: {n} of those were rebuilt by a best-fit search: the shape uses every stored number, but the file may admit another, so a small detail can differ (`--mesh-fit unique` leaves them out)"
        );
    }
}

/// The notes both `3d-mesh` and `3d-render` print about what was inferred.
#[cfg(feature = "3d")]
fn print_assembly_notes(a: &pdfcer_3d::AssembledModel, drawn: &str) {
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
    print_best_fit_note(a.best_fit);
    for (why, n) in &a.skipped_why {
        println!("note: {n} compressed mesh(es) left out: {why}");
    }
    print_parts("parts missing a left-out mesh", &a.left_out_parts);
    print_parts("parts drawn by a best-fit mesh", &a.best_fit_parts);
}

/// One note line naming `parts`, as `3d-tree` lists them; nothing when empty.
#[cfg(feature = "3d")]
fn print_parts(what: &str, parts: &[String]) {
    if !parts.is_empty() {
        println!("note: {what} ({}): {}", parts.len(), parts.join(", "));
    }
}

#[cfg(feature = "3d")]
fn mesh_from_bytes(
    input: &Path,
    index: usize,
    data: &[u8],
    output: &Path,
    (format, fit, draw_hidden): (MeshFormat, MeshFitArg, bool),
) -> u8 {
    let refuse = |why: String| {
        eprintln!("pdfcer: {}: 3D artwork {index}: {why}", input.display());
        exit::EDIT_REFUSED
    };
    let mut options = pdfcer_3d::AssembleOptions::default();
    options.mesh_fit = mesh_fit(fit);
    options.stored_visibility = stored_visibility(draw_hidden);
    let a = match assemble_with(data, options) {
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
    if a.textured > 0 {
        println!(
            "note: {} mesh(es) carry a texture; the mesh file holds geometry only",
            a.textured
        );
    }
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
    _how: (MeshFormat, MeshFitArg, bool),
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
// Without `3d` the command refuses by name, so only `input` and `index` are read.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) struct RenderThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) index: usize,
    pub(crate) output: &'a Path,
    pub(crate) view: Option<ThreeDView>,
    pub(crate) up: Option<Axis3>,
    pub(crate) eye: Option<[f64; 3]>,
    pub(crate) target: Option<[f64; 3]>,
    pub(crate) ortho: bool,
    pub(crate) fov: f64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) transparent: bool,
    pub(crate) style_alpha: StyleAlphaArg,
    pub(crate) entity_overrides: EntityOverridesArg,
    pub(crate) mesh_fit: MeshFitArg,
    pub(crate) draw_hidden: bool,
    pub(crate) texture_origin: TextureOriginArg,
    pub(crate) texture_wrap_base: TextureWrapBaseArg,
    pub(crate) texture_pictures: TexturePicturesArg,
}

/// `3d-render` — draw one PRC model to a PNG from a camera.
pub(crate) fn cmd_render_3d(a: &RenderThreeDArgs<'_>) -> u8 {
    let (data, saved) = match artwork_bytes(a.input, a.index) {
        Ok(found) => found,
        Err(code) => return code,
    };
    render_from_bytes(a, &data, saved.as_ref())
}

#[cfg(feature = "3d")]
fn render_from_bytes(
    a: &RenderThreeDArgs<'_>,
    data: &[u8],
    saved: Option<&pdfcer_core::threed::ThreeDSavedView>,
) -> u8 {
    use pdfcer_3d::{Bounds, Camera, Projection, RenderOptions, render_model};
    let refuse = |why: String| {
        eprintln!(
            "pdfcer: {}: 3D artwork {}: {why}",
            a.input.display(),
            a.index
        );
        exit::EDIT_REFUSED
    };
    let model = match assemble_with(data, assemble_options(a)) {
        Ok(model) => model,
        Err(why) => return refuse(why),
    };
    let Some(bounds) = Bounds::of(&model.meshes) else {
        return refuse("the model has no finite vertex to draw".to_owned());
    };
    let target = a.target.unwrap_or_else(|| bounds.centre());
    let aim = aim_camera(a, target, saved);
    let (direction, up) = (aim.direction, aim.up);
    let aspect = f64::from(a.width) / f64::from(a.height.max(1));
    let mut camera = match Camera::fit_meshes(&model.meshes, direction, up, !aim.ortho, aspect) {
        Ok(camera) => camera,
        Err(err) => return refuse(err.to_string()),
    };
    if let Some(eye) = a.eye {
        camera.eye = eye;
        camera.target = target;
    }
    if let Some(saved) = &aim.framing {
        saved.frame(&mut camera);
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
    let image = match render_model(&model, &camera, &options) {
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
    println!("note: camera: {}", aim.source);
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
         had none and are drawn grey; lights are not read yet"
    );
    print_alpha_unset_note(
        model.alpha_unset,
        " (`--style-alpha style` draws them invisible, as the literal reading does)",
    );
    print_texture_notes(model.textured, &model.texture_notes);
    if model.overridden > 0 {
        println!(
            "note: {} placement(s) drawn in a colour an enclosing assembly overrides them with \
             (`--entity-overrides ignore` draws their own)",
            model.overridden
        );
    }
    exit::SUCCESS
}

/// How many meshes [`pdfcer_3d::StyleAlpha::ZeroUnset`] drew opaque, with
/// `how` saying how to get the literal reading.
pub(crate) fn print_alpha_unset_note(n: usize, how: &str) {
    if n > 0 {
        println!(
            "inferred: {n} mesh(es) drawn opaque: each material's alpha is 0 and its style \
             states no transparency, read as unset rather than invisible{how}"
        );
    }
}

/// How many meshes drew their texture, and why any others did not.
pub(crate) fn print_texture_notes(textured: usize, notes: &[(String, usize)]) {
    if textured > 0 {
        println!("note: {textured} mesh(es) drawn with their texture picture, sampled bilinearly");
    }
    for (why, n) in notes {
        println!("note: texture on {n} mesh(es): {why}");
    }
}

#[cfg(not(feature = "3d"))]
fn render_from_bytes(
    a: &RenderThreeDArgs<'_>,
    _data: &[u8],
    _saved: Option<&pdfcer_core::threed::ThreeDSavedView>,
) -> u8 {
    no_3d_feature(a.input, a.index, "3d-render")
}
