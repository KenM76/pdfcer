//! `export-image --format emf --emf-plates`: one vector metafile per
//! separation plate (`pdfcer_render::emf::export_emf_plates`).

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;
use pdfcer_render::RenderOptions;
use pdfcer_render::emf::{
    EmfOptions, EmfPlate, EmfPlates, PlateMetafileError, export_emf_plates,
    export_emf_plates_strict,
};

use crate::edit_common::write_output;
use crate::exit;
use crate::render_export::{num_px, print_emf_disclosure};

/// One page's plates request.
pub(crate) struct PlatesPage<'a> {
    pub(crate) input: &'a Path,
    pub(crate) doc: &'a Document,
    pub(crate) page: &'a Page,
    pub(crate) page_number: usize,
    /// The path the page's single metafile would have taken; each plate
    /// inserts `_<ink>` before its extension.
    pub(crate) path: &'a Path,
    pub(crate) dpi: f32,
    pub(crate) strict: bool,
}

/// Export one page's plates and print one stable line per plate:
/// `exported <input> page <N> plate <ink> -> <path> <W>x<H> format=emf
/// dpi=<D>; plate: ops= knockouts= knockouts_elided= overprints_kept=
/// images= shadings= soft_masks= transparency= rasters=`. A page with no
/// inks writes its composite metafile to the unsuffixed path with
/// `plate composite`.
///
/// # Errors
///
/// The exit code to stop with: `RUNTIME_ERROR` for a failed export or a
/// strict refusal (nothing of the page is written), `IO_ERROR` for a write
/// failure.
pub(crate) fn export_page_plates(
    req: &PlatesPage<'_>,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<(), u8> {
    let plates = if req.strict {
        export_emf_plates_strict(&req.doc.view(), req.page, render, emf)
    } else {
        export_emf_plates(req.doc, req.page, render, emf).map_err(PlateMetafileError::from)
    };
    let plates = plates.map_err(|err| {
        let hint = match err {
            PlateMetafileError::Raster { .. } => {
                "; drop --emf-plates-strict to write it as a counted pixel patch"
            }
            _ => "",
        };
        eprintln!(
            "pdfcer: {}: page {}: {err}{hint}",
            req.input.display(),
            req.page_number
        );
        exit::RUNTIME_ERROR
    })?;
    match plates {
        EmfPlates::Separated(plates) => {
            for plate in &plates {
                write_plate(req, plate)?;
            }
            Ok(())
        }
        EmfPlates::Composite(export) => {
            write(req.path, &export.emf)?;
            let o = &export.outcome;
            println!(
                "exported {} page {} plate composite -> {} {}x{} format=emf dpi={}",
                req.input.display(),
                req.page_number,
                req.path.display(),
                num_px(o.width_pt * req.dpi / 72.0),
                num_px(o.height_pt * req.dpi / 72.0),
                req.dpi
            );
            print_emf_disclosure(o, req.page_number, false);
            eprintln!(
                "pdfcer: note: page {}: the page could not be composited in ink, so it is one composite metafile, not plates",
                req.page_number
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

fn write_plate(req: &PlatesPage<'_>, plate: &EmfPlate) -> Result<(), u8> {
    let path = plate_path(req.path, &plate.ink);
    write(&path, &plate.emf)?;
    let o = &plate.outcome;
    let p = &o.patches;
    println!(
        "exported {} page {} plate {} -> {} {}x{} format=emf dpi={}; plate: ops={} knockouts={} \
knockouts_elided={} overprints_kept={} images={} shadings={} soft_masks={} transparency={} rasters={}",
        req.input.display(),
        req.page_number,
        plate.ink,
        path.display(),
        num_px(o.width_pt * req.dpi / 72.0),
        num_px(o.height_pt * req.dpi / 72.0),
        req.dpi,
        o.ops,
        o.knockouts,
        o.knockouts_elided,
        o.overprints_kept,
        p.images,
        p.shadings,
        p.soft_masks,
        p.transparency,
        p.rasters
    );
    if p.rasters > 0 {
        eprintln!(
            "pdfcer: note: page {} plate {}: {} object(s) needing pixels are written as {} pixel patch(es); the rest of the plate stays vector",
            req.page_number,
            plate.ink,
            p.cases(),
            p.rasters
        );
    }
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), u8> {
    write_output(path, bytes).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::IO_ERROR
    })
}

/// `dir/name.emf` -> `dir/name_<ink>.emf`, the ink reduced to characters
/// safe in a file name on every platform.
pub(crate) fn plate_path(path: &Path, ink: &str) -> PathBuf {
    let safe: String = ink
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    path.with_file_name(format!("{stem}_{safe}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::plate_path;
    use std::path::Path;

    #[test]
    fn a_plate_path_suffixes_the_ink_before_the_extension() {
        assert_eq!(
            plate_path(Path::new("out/page_p1.emf"), "Cyan"),
            Path::new("out/page_p1_Cyan.emf")
        );
        assert_eq!(
            plate_path(Path::new("p.emf"), "Spot Gold 1"),
            Path::new("p_Spot_Gold_1.emf")
        );
    }
}
