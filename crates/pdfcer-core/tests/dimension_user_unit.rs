//! A ce dimension on a `/UserUnit` page measures physical length
//! (ISO 32000-1 §14.11.2 Table 30; pdfcer-gui G277).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionKind, NumberFormat, ScaleState, Unit, format_measurement_on,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::page_tree::pages;
use pdfcer_core::vector::{AxisConstraint, Point};

/// Page 0 is a 612 x 792 box; page 1 is a 306 x 396 box with `/UserUnit 2`,
/// so both are physically Letter.
fn pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 306 396] /UserUnit 2 /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn across(width: f64) -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(0.0, 100.0),
        b: Point::new(width, 100.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(pdf()).unwrap())
}

/// The `/C` of the first `/X` element of a ce dimension's `/Measure`.
fn measure_c(s: &EditSession, annot: pdfcer_core::object::ObjId) -> f64 {
    let dict = s.resolved(annot).as_dict().unwrap();
    let measure = s.resolve(dict.get(b"Measure").unwrap()).as_dict().unwrap();
    let x = s.resolve(measure.get(b"X").unwrap()).as_array().unwrap();
    let first = s.resolve(&x[0]).as_dict().unwrap();
    first.get(b"C").unwrap().as_number().unwrap()
}

#[test]
fn the_raw_readout_is_physical_points() {
    let mut s = session();
    let (_, full) = s.add_dimension(0, DEFAULT_GROUP_ID, across(612.0)).unwrap();
    let (_, half) = s.add_dimension(1, DEFAULT_GROUP_ID, across(306.0)).unwrap();
    assert_eq!(s.dimension_display(full).unwrap().text, "612.00 pt");
    assert_eq!(s.dimension_display(half).unwrap().text, "612.00 pt");
    // The model alone has no page: it still reads box units.
    assert_eq!(s.dimension_model().display(half).unwrap().text, "306.00 pt");
}

#[test]
fn a_scaled_group_reads_the_same_printed_distance_on_both_pages() {
    let mut s = session();
    let scale = ScaleState::Calibrated { scale: 0.5 };
    let format = NumberFormat::decimal(Unit::Millimeter, 1);
    s.set_group_scale(DEFAULT_GROUP_ID, scale, format).unwrap();
    let (full_annot, full) = s.add_dimension(0, DEFAULT_GROUP_ID, across(612.0)).unwrap();
    let (half_annot, half) = s.add_dimension(1, DEFAULT_GROUP_ID, across(306.0)).unwrap();
    assert_eq!(s.dimension_display(full).unwrap().text, "306.0 mm");
    assert_eq!(s.dimension_display(half).unwrap().text, "306.0 mm");

    // §12.9 measures in default user space, so the unit composes into /C.
    assert!((measure_c(&s, full_annot) - 0.5).abs() < 1e-9);
    assert!((measure_c(&s, half_annot) - 1.0).abs() < 1e-9);

    // Regeneration keeps it: a group-wide change rebakes both.
    s.set_group_unit(DEFAULT_GROUP_ID, Unit::Centimeter)
        .unwrap();
    assert_eq!(
        s.dimension_display(full).unwrap().text,
        s.dimension_display(half).unwrap().text
    );
    assert!((measure_c(&s, half_annot) / measure_c(&s, full_annot) - 2.0).abs() < 1e-9);
}

#[test]
fn the_placing_preview_matches_what_is_written() {
    let mut s = session();
    let kind = across(306.0);
    let preview = s
        .new_dimension_preview_on(1, DEFAULT_GROUP_ID, &kind)
        .unwrap();
    assert_eq!(preview.appearance.label, "612.00 pt");
    let plain = s.new_dimension_preview(DEFAULT_GROUP_ID, &kind).unwrap();
    assert_eq!(plain.appearance.label, "306.00 pt");
    let (_, id) = s.add_dimension(1, DEFAULT_GROUP_ID, kind).unwrap();
    assert_eq!(
        s.dimension_display(id).unwrap().text,
        preview.appearance.label
    );
    assert!(
        s.new_dimension_preview_on(9, DEFAULT_GROUP_ID, &across(1.0))
            .is_err()
    );
}

#[test]
fn the_free_function_multiplies_by_the_page_unit() {
    let doc = Document::from_bytes(pdf()).unwrap();
    let page = &pages(&doc).unwrap()[1];
    let format = NumberFormat::decimal(Unit::Millimeter, 1);
    let shown = format_measurement_on(page, 306.0, ScaleState::Calibrated { scale: 0.5 }, format);
    assert_eq!(shown.text, "306.0 mm");
}
