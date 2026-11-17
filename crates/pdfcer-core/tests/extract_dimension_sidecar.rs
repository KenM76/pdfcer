//! Extract, split, merge and insert carry the ce-dimension sidecar for the
//! copied pages (pdfcer-gui G279, G286): a ce dimension stays an editable
//! ce dimension.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind, NumberFormat, ScaleState, Unit};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
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

/// The bytes of `dimensioned()` with the sidecar version set to one this
/// build does not read.
fn newer() -> Document {
    let mut bytes = dimensioned().bytes().to_vec();
    let at = bytes
        .windows(10)
        .rposition(|w| w.starts_with(b"/Version ") && w[9].is_ascii_digit())
        .expect("the sidecar is stored uncompressed");
    bytes[at + 9] = b'9';
    Document::from_bytes(bytes).unwrap()
}

/// The layers `bytes` registers in `/OCProperties` `/OCGs`, and whether
/// each ce dimension's `/OC` names the layer of its group.
fn layers(bytes: &[u8]) -> (usize, bool) {
    let d = Document::from_bytes(bytes.to_vec()).unwrap();
    let registered = d
        .catalog_dict()
        .and_then(|c| c.get(b"OCProperties"))
        .and_then(|o| d.resolve(o).as_dict())
        .and_then(|ocp| ocp.get(b"OCGs"))
        .and_then(|o| d.resolve(o).as_array())
        .map_or(0, <[Object]>::len);
    let model = EditSession::new(Document::from_bytes(bytes.to_vec()).unwrap()).dimension_model();
    let on_layer = model.dimensions().iter().all(|record| {
        let layer = model.group(record.group).and_then(|g| g.ocg);
        let oc = d
            .resolve(&Object::Reference(record.annot.unwrap()))
            .as_dict()
            .and_then(|a| a.get(b"OC"))
            .and_then(Object::as_reference);
        layer.is_some() && oc == layer
    });
    (registered, on_layer)
}

fn reopened(s: &EditSession) -> EditSession {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

#[test]
fn a_file_merge_combines_the_sidecars() {
    let source = dimensioned();
    let plain = Document::from_bytes(two_page_pdf()).unwrap();
    let of = view_of;
    let (bytes, report) =
        pageops::merge(&[of(&source), of(&plain), of(&source)], &[], &[]).unwrap();
    assert_eq!(report.ce_dimensions_as_lines, [0, 0, 0]);
    assert_eq!(
        (report.ce_dimensions_carried, report.ce_dimensions_left),
        (6, 0)
    );
    assert_eq!(
        (
            report.ce_dimension_groups_carried,
            report.ce_dimension_groups_matched,
            report.ce_dimension_groups_renamed
        ),
        (1, 2, 0)
    );

    // One layer per group: the second copy's layers are not kept.
    assert_eq!(layers(&bytes), (2, true));
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let model = s.dimension_model();
    assert_eq!(model.groups().len(), 2);
    assert_eq!(model.dimensions().len(), 6);
    let arch = model.groups().iter().find(|g| g.name == "Arch").unwrap();
    assert_eq!(arch.scale, ScaleState::Calibrated { scale: 0.01 });
    // Both copies follow the one group.
    assert_eq!(s.set_group_unit(arch.id, Unit::Millimeter).unwrap(), 4);
    let first = s.dimension_model().dimensions()[0].id;
    s.delete_dimension(first).unwrap();
    assert_eq!(reopened(&s).dimension_model().dimensions().len(), 5);

    let (_, report) = pageops::extract(&of(&source), &[1]).unwrap();
    assert_eq!(report.ce_dimensions_as_lines, [0]);
}

#[test]
fn a_file_merge_leaves_a_newer_sidecar_and_carries_the_rest() {
    let (source, newer) = (dimensioned(), newer());
    let (bytes, report) = pageops::merge(&[view_of(&newer), view_of(&source)], &[], &[]).unwrap();
    assert!(report.ce_dimension_sidecar_newer);
    assert_eq!(report.ce_dimensions_as_lines, [3, 0]);
    assert_eq!(
        (report.ce_dimensions_carried, report.ce_dimensions_left),
        (3, 3)
    );
    let s = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(s.dimension_model().dimensions().len(), 3);
}

#[test]
fn insert_carries_the_ce_dimensions_as_one_undo_step() {
    let source = dimensioned();
    let mut s = EditSession::new(Document::from_bytes(two_page_pdf()).unwrap());
    let out = s
        .insert_pages(&view_of(&source), &[1], pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (2, 0)
    );
    assert_eq!(
        (
            out.ce_dimension_groups_carried,
            out.ce_dimension_groups_matched,
            out.ce_dimension_groups_renamed
        ),
        (1, 0, 0)
    );
    let model = s.dimension_model();
    assert_eq!(model.groups().len(), 2);
    let arch = model.groups().iter().find(|g| g.name == "Arch").unwrap().id;
    assert!(model.dimensions().iter().all(|d| d.group == arch));

    s.undo().unwrap();
    assert!(s.dimension_model().dimensions().is_empty());
    assert!(
        !reopened(&s)
            .to_incremental_bytes(&SaveOptions::identity())
            .unwrap()
            .0
            .windows(9)
            .any(|w| w == b"PieceInfo")
    );
    s.redo().unwrap();

    let mut again = reopened(&s);
    assert_eq!(again.dimension_model().dimensions().len(), 2);
    assert_eq!(again.set_group_unit(arch, Unit::Millimeter).unwrap(), 2);
}

#[test]
fn a_plain_source_stages_no_sidecar() {
    let plain = Document::from_bytes(two_page_pdf()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(two_page_pdf()).unwrap());
    let out = s
        .insert_pages(&view_of(&plain), &[0], pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (0, 0)
    );
    let out = s
        .merge_document(&view_of(&plain), pageops::InsertPosition::End)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (0, 0)
    );
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert!(!bytes.windows(9).any(|w| w == b"PieceInfo"));
}

#[test]
fn a_merge_joins_a_group_of_the_same_settings() {
    let source = dimensioned();
    let mut s = EditSession::new(dimensioned());
    let out = s
        .merge_document(&view_of(&source), pageops::InsertPosition::End)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (3, 0)
    );
    assert_eq!(
        (
            out.ce_dimension_groups_carried,
            out.ce_dimension_groups_matched
        ),
        (0, 2)
    );
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert_eq!(layers(&saved), (2, true));
    let mut s = reopened(&s);
    let model = s.dimension_model();
    assert_eq!((model.groups().len(), model.dimensions().len()), (2, 6));
    let arch = model.groups().iter().find(|g| g.name == "Arch").unwrap().id;
    assert_eq!(s.set_group_unit(arch, Unit::Millimeter).unwrap(), 4);
}

#[test]
fn a_merge_renames_a_group_whose_name_is_taken_by_other_settings() {
    let source = dimensioned();
    let mut s = EditSession::new(dimensioned());
    let own = s.dimension_model().groups()[1].id;
    s.set_group_scale(
        own,
        ScaleState::Calibrated { scale: 0.5 },
        NumberFormat::feet_inches(16, true),
    )
    .unwrap();
    let out = s
        .merge_document(&view_of(&source), pageops::InsertPosition::End)
        .unwrap();
    assert_eq!(
        (
            out.ce_dimension_groups_carried,
            out.ce_dimension_groups_matched,
            out.ce_dimension_groups_renamed
        ),
        (1, 1, 1)
    );
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert_eq!(layers(&saved), (3, true));
    let mut s = reopened(&s);
    let model = s.dimension_model();
    let brought = model
        .groups()
        .iter()
        .find(|g| g.name == "Arch (2)")
        .unwrap();
    assert_eq!(brought.scale, ScaleState::Calibrated { scale: 0.01 });
    assert_eq!(
        model.group(own).unwrap().scale,
        ScaleState::Calibrated { scale: 0.5 }
    );
    let count = |group| {
        model
            .dimensions()
            .iter()
            .filter(|d| d.group == group)
            .count()
    };
    assert_eq!((count(own), count(brought.id)), (2, 2));
    assert_eq!(s.set_group_unit(brought.id, Unit::Millimeter).unwrap(), 2);
}

#[test]
fn a_newer_sidecar_on_either_side_arrives_as_lines() {
    let (source, newer) = (dimensioned(), newer());
    let mut s = EditSession::new(Document::from_bytes(two_page_pdf()).unwrap());
    let out = s
        .insert_pages(&view_of(&newer), &[1], pageops::InsertPosition::Start)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (0, 2)
    );
    assert!(out.ce_dimension_sidecar_newer);
    assert!(s.dimension_model().dimensions().is_empty());

    let mut s = EditSession::new(newer);
    let out = s
        .merge_document(&view_of(&source), pageops::InsertPosition::End)
        .unwrap();
    assert_eq!(
        (out.ce_dimensions_carried, out.ce_dimensions_as_lines),
        (0, 3)
    );
    assert!(out.ce_dimension_sidecar_newer);
}
