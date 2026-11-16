//! Extract and split carry the ce-dimension sidecar for the copied pages
//! (pdfcer-gui G279): a ce dimension stays an editable ce dimension.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind, NumberFormat, ScaleState, Unit};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::pageops::{self, AssembleOptions, DocumentView};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn two_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
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

fn view_of(d: &Document) -> DocumentView<'_> {
    DocumentView::new(d, d.bytes(), d.version())
}

fn linear(y: f64) -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, y),
        b: Point::new(200.0, y),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

/// Two pages; page 0 has one ce dimension in the default group, page 1 has
/// two in a calibrated "Arch" group.
fn dimensioned() -> Document {
    let mut s = EditSession::new(Document::from_bytes(two_page_pdf()).unwrap());
    let arch = s.add_dimension_group("Arch", Unit::FeetInches).unwrap();
    s.set_group_scale(
        arch,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::feet_inches(16, true),
    )
    .unwrap();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear(200.0)).unwrap();
    s.add_dimension(1, arch, linear(150.0)).unwrap();
    s.add_dimension(1, arch, linear(250.0)).unwrap();
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    Document::from_bytes(bytes).unwrap()
}

#[test]
fn extract_keeps_the_ce_dimensions_of_the_copied_pages() {
    let source = dimensioned();
    let view = DocumentView::new(&source, source.bytes(), source.version());
    let (bytes, report) = pageops::extract(&view, &[1]).unwrap();
    assert_eq!(
        (report.ce_dimensions_carried, report.ce_dimensions_left),
        (2, 1)
    );

    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let model = s.dimension_model();
    assert_eq!(model.groups().len(), 2, "every group is carried");
    let arch = model.groups().iter().find(|g| g.name == "Arch").unwrap();
    assert_eq!(arch.scale, ScaleState::Calibrated { scale: 0.01 });
    assert_eq!(arch.format, NumberFormat::feet_inches(16, true));
    assert_eq!(model.dimensions().len(), 2);
    assert!(model.dimensions().iter().all(|d| d.group == arch.id));

    // The records are wired to the copies: a group edit regenerates both.
    let arch = arch.id;
    assert_eq!(s.set_group_unit(arch, Unit::Millimeter).unwrap(), 2);
    let first = s.dimension_model().dimensions()[0].id;
    s.delete_dimension(first).unwrap();
    assert_eq!(s.dimension_model().dimensions().len(), 1);
}

#[test]
fn the_sidecar_can_be_left_behind() {
    let source = dimensioned();
    let view = DocumentView::new(&source, source.bytes(), source.version());
    let options = AssembleOptions::default().with_dimension_sidecar(false);
    let (bytes, report) = pageops::assemble(&[view], &[(0, 1)], &options).unwrap();
    assert_eq!(report.ce_dimensions_carried, 0);
    let s = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(s.dimension_model().dimensions().is_empty());
}

#[test]
fn a_document_without_ce_dimensions_gains_no_sidecar() {
    let source = Document::from_bytes(two_page_pdf()).unwrap();
    let view = DocumentView::new(&source, source.bytes(), source.version());
    let (bytes, report) = pageops::extract(&view, &[0]).unwrap();
    assert_eq!(report.ce_dimensions_carried, 0);
    assert!(!bytes.windows(9).any(|w| w == b"PieceInfo"));
}

#[test]
fn a_newer_sidecar_is_left_behind_and_said_so() {
    let mut bytes = dimensioned().bytes().to_vec();
    let at = bytes
        .windows(10)
        .rposition(|w| w.starts_with(b"/Version ") && w[9].is_ascii_digit())
        .expect("the sidecar is stored uncompressed");
    bytes[at + 9] = b'9';
    let source = Document::from_bytes(bytes).unwrap();
    let view = DocumentView::new(&source, source.bytes(), source.version());
    let (bytes, report) = pageops::extract(&view, &[1]).unwrap();
    assert!(report.ce_dimension_sidecar_newer);
    assert_eq!(
        (report.ce_dimensions_carried, report.ce_dimensions_left),
        (0, 3)
    );
    assert!(!bytes.windows(9).any(|w| w == b"PieceInfo"));
}

/// Merge and insert do not carry the sidecar; each says how many ce
/// dimensions arrived as plain line annotations (pdfcer-gui G286).
#[test]
fn merge_and_insert_count_the_ce_dimensions_that_arrive_as_lines() {
    let source = dimensioned();
    let plain = Document::from_bytes(two_page_pdf()).unwrap();
    let of = view_of;
    let (view, plain_view) = (of(&source), of(&plain));

    let (_, report) = pageops::merge(&[of(&source), of(&plain), of(&source)], &[], &[]).unwrap();
    assert_eq!(report.ce_dimensions_as_lines, [3, 0, 3]);
    assert_eq!(report.ce_dimensions_carried, 0);

    let (_, report) = pageops::extract(&view, &[1]).unwrap();
    assert_eq!(report.ce_dimensions_as_lines, [0]);

    let mut s = EditSession::new(Document::from_bytes(two_page_pdf()).unwrap());
    let out = s
        .insert_pages(&view, &[1], pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(out.ce_dimensions_as_lines, 2);
    let out = s
        .insert_pages(&plain_view, &[0], pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(out.ce_dimensions_as_lines, 0);
    let out = s
        .merge_document(&view, pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(out.ce_dimensions_as_lines, 3);
    assert!(s.dimension_model().dimensions().is_empty());
}
