//! Separation plates as vector EMF metafiles (`Pass 700.11`, request X072):
//! one metafile per ink, grey at the tint, patches only where pixels are
//! unavoidable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::export_emf::{DPI, assert_well_formed, count, page, record_offsets};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{
    EmfOptions, EmfPlate, EmfPlates, PlateMetafileError, PlateRaster, export_emf_plates,
    export_emf_plates_strict, walk_records,
};
use pdfcer_render::RenderOptions;

/// Separation SpotA: alternate M = 1.
const SPOT_A: &str = "[/Separation /SpotA /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 0 0] /N 1 >>]";

const FILLPATH: u32 = 0x3E;
const ALPHABLEND: u32 = 0x72;

fn doc_page(bytes: Vec<u8>) -> (Document, page_tree::Page) {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    (doc, p)
}

fn emf_options() -> EmfOptions {
    EmfOptions::default().with_raster_dpi(DPI)
}

fn separated(bytes: Vec<u8>) -> Vec<EmfPlate> {
    let (doc, p) = doc_page(bytes);
    match export_emf_plates(&doc, &p, &RenderOptions::default(), &emf_options()).expect("plates") {
        EmfPlates::Separated(plates) => plates,
        other => panic!("expected separated plates, got {other:?}"),
    }
}

/// The solid brush colours of `emf`'s filled paths, in order.
fn fill_greys(emf: &[u8]) -> Vec<u8> {
    assert_well_formed(emf);
    let records = walk_records(emf).unwrap();
    let offsets = record_offsets(&records);
    let mut out = Vec::new();
    let mut brush = None;
    for ((kind, _), off) in records.iter().zip(offsets) {
        match *kind {
            0x27 => {
                let rgb = &emf[off + 16..off + 19];
                assert!(rgb[0] == rgb[1] && rgb[1] == rgb[2], "plates are grey");
                brush = Some(rgb[0]);
            }
            FILLPATH => out.push(brush.expect("a brush precedes the path")),
            _ => {}
        }
    }
    out
}

fn plate<'a>(plates: &'a [EmfPlate], ink: &str) -> &'a EmfPlate {
    plates.iter().find(|p| p.ink == ink).expect("plate exists")
}

/// A CMYK square (C 0.5, K 1) and, apart from it, a SpotA square.
fn cmyk_and_spot() -> Vec<u8> {
    page(
        &format!("/ColorSpace << /CS0 {SPOT_A} >>"),
        "0.5 0 0 1 k 10 10 30 30 re f /CS0 cs 1 scn 60 40 30 30 re f",
    )
}

#[test]
fn process_and_spot_squares_give_one_grey_path_per_inked_plate() {
    let plates = separated(cmyk_and_spot());
    let inks: Vec<&str> = plates.iter().map(|p| p.ink.as_str()).collect();
    assert_eq!(inks, ["Cyan", "Magenta", "Yellow", "Black", "SpotA"]);
    // 50% tint is grey 128 (round(255 * 0.5)); full tint is black.
    assert_eq!(fill_greys(&plate(&plates, "Cyan").emf), [128]);
    assert_eq!(fill_greys(&plate(&plates, "Black").emf), [0]);
    assert_eq!(fill_greys(&plate(&plates, "SpotA").emf), [0]);
    assert!(fill_greys(&plate(&plates, "Magenta").emf).is_empty());
    assert!(fill_greys(&plate(&plates, "Yellow").emf).is_empty());
    for p in &plates {
        let records = walk_records(&p.emf).unwrap();
        assert_eq!(
            count(&records, ALPHABLEND),
            0,
            "{}: no pixel patches",
            p.ink
        );
        assert_eq!(p.outcome.patches.rasters, 0);
        assert_eq!(p.outcome.knockouts, 0, "{}: nothing beneath", p.ink);
    }
}

/// A SpotA square, then a K square over part of it, overprinting or not.
fn k_over_spot(overprint: bool) -> Vec<u8> {
    let flag = if overprint { "true" } else { "false" };
    page(
        &format!(
            "/ColorSpace << /CS0 {SPOT_A} >> /ExtGState << /GS0 << /OP {flag} /op {flag} >> >>"
        ),
        "/CS0 cs 1 scn 10 10 60 50 re f /GS0 gs 0 0 0 1 k 40 30 50 40 re f",
    )
}

#[test]
fn an_overprinting_k_square_leaves_the_spot_path_whole() {
    let plates = separated(k_over_spot(true));
    let spot = plate(&plates, "SpotA");
    assert_eq!(fill_greys(&spot.emf), [0], "the spot path alone");
    assert_eq!(spot.outcome.overprints_kept, 1);
    assert_eq!(spot.outcome.knockouts, 0);
    assert_eq!(fill_greys(&plate(&plates, "Black").emf), [0]);
}

#[test]
fn a_knocking_out_k_square_paints_paper_white_on_the_spot_plate() {
    let plates = separated(k_over_spot(false));
    let spot = plate(&plates, "SpotA");
    assert_eq!(fill_greys(&spot.emf), [0, 255], "spot, then the knockout");
    assert_eq!(spot.outcome.knockouts, 1);
    assert_eq!(spot.outcome.overprints_kept, 0);
}

/// A 2 × 2 cyan-only image.
fn cyan_image() -> Vec<u8> {
    page(
        "",
        "q 40 0 0 40 20 20 cm BI /W 2 /H 2 /CS /CMYK /BPC 8 /F /AHx ID \
         FF000000FF000000FF000000FF000000> EI Q",
    )
}

#[test]
fn an_image_is_one_counted_patch_on_each_plate_it_inks() {
    let plates = separated(cyan_image());
    for p in &plates {
        let records = walk_records(&p.emf).unwrap();
        let expected = usize::from(p.ink == "Cyan");
        assert_eq!(p.outcome.patches.images, expected, "{}", p.ink);
        assert_eq!(p.outcome.patches.rasters, expected, "{}", p.ink);
        assert_eq!(count(&records, ALPHABLEND), expected, "{}", p.ink);
    }
}

#[test]
fn the_strict_variant_refuses_an_image_and_says_why() {
    let (doc, p) = doc_page(cyan_image());
    let err = export_emf_plates_strict(&doc.view(), &p, &RenderOptions::default(), &emf_options())
        .expect_err("an image needs pixels");
    match &err {
        PlateMetafileError::Raster { ink, reason } => {
            assert_eq!(ink, "Cyan");
            assert_eq!(*reason, PlateRaster::Image);
        }
        other => panic!("expected a raster refusal, got {other}"),
    }
    assert_eq!(err.to_string(), "the Cyan plate needs pixels for image");
    // The vector-only page passes the strict variant.
    let (doc, p) = doc_page(cmyk_and_spot());
    let ok = export_emf_plates_strict(&doc.view(), &p, &RenderOptions::default(), &emf_options());
    assert!(matches!(ok, Ok(EmfPlates::Separated(_))));
}

#[test]
fn a_page_with_no_inks_is_one_composite_metafile() {
    // An empty page still has the four process plates, all empty.
    let plates = separated(page("", ""));
    assert_eq!(plates.len(), 4);
    assert!(plates.iter().all(|p| fill_greys(&p.emf).is_empty()));
    // A refused colorant buffer leaves no inks: the ordinary metafile.
    let (doc, p) = doc_page(cmyk_and_spot());
    let render = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(0));
    let out = export_emf_plates(&doc, &p, &render, &emf_options());
    match out.expect("export") {
        EmfPlates::Composite(e) => assert_well_formed(&e.emf),
        other => panic!("expected the composite metafile, got {other:?}"),
    }
}
