//! `EditSession::set_layer_properties`: a layer's name, default
//! visibility, lock, print/export state and intent (ISO 32000-1 §8.11).

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    CommandKind, EditError, EditSession, LayerEdit, LayerIntent, LayerOutputState,
};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::layers::read_layers;
use pdfcer_core::object::{Dict, ObjId, Object};

fn fixture(name: &str) -> EditSession {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/layers/"
    );
    let bytes = std::fs::read(format!("{path}{name}")).expect("fixture readable");
    EditSession::new(Document::from_bytes(bytes).expect("fixture parses"))
}

fn assemble(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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

/// `/OCProperties` (object 3), its `/D` (object 4) and `/D /OFF` (object 5)
/// all indirect; one layer, object 6, hidden by default.
fn indirect_session() -> EditSession {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties 3 0 R >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
        "<< /OCGs [6 0 R] /D 4 0 R >>",
        "<< /Order [6 0 R] /OFF 5 0 R >>",
        "[6 0 R]",
        "<< /Type /OCG /Name (Welds) >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

fn resolved(s: &EditSession, id: ObjId) -> Object {
    s.graph().resolve(&Object::Reference(id)).clone()
}

fn dict(s: &EditSession, id: ObjId) -> Dict {
    match resolved(s, id) {
        Object::Dict(d) => d,
        other => panic!("{id} is {other:?}"),
    }
}

/// `/OCProperties /D` of the catalog, resolved through any references.
fn default_config(s: &EditSession) -> Dict {
    let g = s.graph();
    let root = Object::Reference(g.catalog_id().expect("catalog"));
    let cat = g.resolve(&root);
    let ocp = g
        .resolve(
            cat.as_dict()
                .and_then(|c| c.get(b"OCProperties"))
                .expect("ocp"),
        )
        .clone();
    let d = ocp.as_dict().and_then(|o| o.get(b"D")).expect("/D").clone();
    g.resolve(&d).as_dict().expect("/D dict").clone()
}

fn members(s: &EditSession, d: &Dict, key: &[u8]) -> Vec<ObjId> {
    d.get(key)
        .map(|o| s.graph().resolve(o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
        .iter()
        .filter_map(Object::as_reference)
        .collect()
}

fn layer(read: &pdfcer_core::layers::Layers, id: u32) -> &pdfcer_core::layers::Layer {
    read.layers
        .iter()
        .find(|l| l.id == ObjId::new(id, 0))
        .expect("layer listed")
}

/// A rename writes `/Name` and nothing but the group object.
#[test]
fn a_rename_writes_only_the_group() {
    let mut s = fixture("basic-layers.pdf");
    let out = s
        .set_layer_properties(ObjId::new(4, 0), &LayerEdit::new().name("Welds"))
        .unwrap();
    assert!(out.changed);
    assert_eq!(layer(&read_layers(&s.graph()), 4).name, "Welds");
    assert_eq!(s.dirty_set().len(), 1);
    assert_eq!(
        s.undo(),
        Some(CommandKind::SetLayerProperties {
            layer: ObjId::new(4, 0)
        })
    );
    assert_eq!(layer(&read_layers(&s.graph()), 4).name, "Dimensions");
}

/// Hiding a layer under the default ON base adds it to `/OFF`; showing it
/// removes it and adds nothing to `/ON`.
#[test]
fn visibility_is_written_against_the_base_state() {
    let mut s = fixture("basic-layers.pdf");
    s.set_layer_properties(
        ObjId::new(4, 0),
        &LayerEdit::new().visible_by_default(false),
    )
    .unwrap();
    s.set_layer_properties(ObjId::new(5, 0), &LayerEdit::new().visible_by_default(true))
        .unwrap();
    let read = read_layers(&s.graph());
    assert!(!layer(&read, 4).visible_by_default);
    assert!(layer(&read, 5).visible_by_default);
    let d = default_config(&s);
    assert_eq!(members(&s, &d, b"OFF"), vec![ObjId::new(4, 0)]);
    assert!(d.get(b"ON").is_none(), "an ON-base config gained /ON");
}

/// Under `/BaseState /OFF` showing goes through `/ON`, and a layer listed in
/// both is taken out of `/OFF` too.
#[test]
fn an_off_base_state_shows_through_on() {
    let mut s = fixture("basestate-off.pdf");
    s.set_layer_properties(ObjId::new(6, 0), &LayerEdit::new().visible_by_default(true))
        .unwrap();
    s.set_layer_properties(
        ObjId::new(5, 0),
        &LayerEdit::new().visible_by_default(false),
    )
    .unwrap();
    let read = read_layers(&s.graph());
    assert!(layer(&read, 6).visible_by_default);
    assert!(!layer(&read, 5).visible_by_default);
    let d = default_config(&s);
    assert_eq!(members(&s, &d, b"ON"), vec![ObjId::new(6, 0)]);
    assert!(members(&s, &d, b"OFF").is_empty());
}

/// Locking adds to `/Locked`, unlocking removes.
#[test]
fn lock_and_unlock() {
    let mut s = fixture("radio-locked.pdf");
    s.set_layer_properties(ObjId::new(4, 0), &LayerEdit::new().locked(false))
        .unwrap();
    s.set_layer_properties(ObjId::new(7, 0), &LayerEdit::new().locked(true))
        .unwrap();
    let read = read_layers(&s.graph());
    assert!(!layer(&read, 4).locked);
    assert!(layer(&read, 7).locked);
}

/// "Never prints" writes `/Usage /Print /PrintState /OFF` and lists the
/// group in a `/D /AS` Print entry; "when visible" takes both back out and
/// drops the emptied entry.
#[test]
fn print_state_writes_usage_and_an_auto_state_entry() {
    let mut s = fixture("basic-layers.pdf");
    let id = ObjId::new(4, 0);
    s.set_layer_properties(id, &LayerEdit::new().print(LayerOutputState::Never))
        .unwrap();
    let g = dict(&s, id);
    let usage = g.get(b"Usage").and_then(Object::as_dict).expect("/Usage");
    let print = usage
        .get(b"Print")
        .and_then(Object::as_dict)
        .expect("/Print");
    assert_eq!(
        print
            .get(b"PrintState")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"OFF".to_vec())
    );
    let d = default_config(&s);
    let entries = d.get(b"AS").and_then(Object::as_array).expect("/AS");
    assert_eq!(entries.len(), 1);
    let e = entries[0].as_dict().expect("entry");
    assert_eq!(
        e.get(b"Event")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"Print".to_vec())
    );
    assert_eq!(members(&s, e, b"OCGs"), vec![id]);

    s.set_layer_properties(id, &LayerEdit::new().print(LayerOutputState::WhenVisible))
        .unwrap();
    assert!(dict(&s, id).get(b"Usage").is_none());
    assert!(default_config(&s).get(b"AS").is_none());
    assert!(s.dirty_set().is_empty(), "the round trip left a net change");
}

/// Export writes the Export pair and leaves an existing Print entry alone.
#[test]
fn export_state_is_independent_of_print() {
    let mut s = fixture("basic-layers.pdf");
    let id = ObjId::new(6, 0);
    s.set_layer_properties(
        id,
        &LayerEdit::new()
            .print(LayerOutputState::Always)
            .export(LayerOutputState::Never),
    )
    .unwrap();
    let d = default_config(&s);
    let entries = d.get(b"AS").and_then(Object::as_array).expect("/AS");
    assert_eq!(entries.len(), 2);
    let usage = dict(&s, id);
    let usage = usage
        .get(b"Usage")
        .and_then(Object::as_dict)
        .expect("/Usage");
    assert!(usage.get(b"Print").is_some() && usage.get(b"Export").is_some());
}

/// Intent: `Design`, then `Both`, then back to `View`.
#[test]
fn intent_is_written() {
    let mut s = fixture("basic-layers.pdf");
    let id = ObjId::new(4, 0);
    s.set_layer_properties(id, &LayerEdit::new().intent(LayerIntent::Design))
        .unwrap();
    assert!(!layer(&read_layers(&s.graph()), 4).intent_view);
    s.set_layer_properties(id, &LayerEdit::new().intent(LayerIntent::Both))
        .unwrap();
    assert!(layer(&read_layers(&s.graph()), 4).intent_view);
    assert_eq!(
        dict(&s, id)
            .get(b"Intent")
            .and_then(Object::as_array)
            .map(<[Object]>::len),
        Some(2)
    );
}

/// An edit that changes nothing records no undo entry.
#[test]
fn a_no_op_records_nothing() {
    let mut s = fixture("basic-layers.pdf");
    let out = s
        .set_layer_properties(
            ObjId::new(4, 0),
            &LayerEdit::new()
                .name("Dimensions")
                .visible_by_default(true)
                .intent(LayerIntent::View)
                .print(LayerOutputState::WhenVisible),
        )
        .unwrap();
    assert!(!out.changed);
    assert_eq!(s.undo_depth(), 0);
}

/// Indirect `/OCProperties`, `/D` and `/OFF` are edited in place: the
/// catalog and `/D` are not rewritten, only the `/OFF` array object.
#[test]
fn indirect_objects_are_edited_in_place() {
    let mut s = indirect_session();
    s.set_layer_properties(ObjId::new(6, 0), &LayerEdit::new().visible_by_default(true))
        .unwrap();
    assert!(layer(&read_layers(&s.graph()), 6).visible_by_default);
    assert_eq!(s.dirty_set().len(), 1);
    assert_eq!(resolved(&s, ObjId::new(5, 0)), Object::Array(Vec::new()));
    assert_eq!(
        dict(&s, ObjId::new(4, 0)).get(b"OFF"),
        Some(&Object::Reference(ObjId::new(5, 0)))
    );
}

/// Refusals: not a registered layer, and an empty name.
#[test]
fn refusals() {
    let mut s = fixture("basic-layers.pdf");
    assert!(matches!(
        s.set_layer_properties(ObjId::new(2, 0), &LayerEdit::new().locked(true)),
        Err(EditError::LayerNotFound { .. })
    ));
    assert!(matches!(
        s.set_layer_properties(ObjId::new(4, 0), &LayerEdit::new().name("")),
        Err(EditError::EmptyLayerName)
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// A new layer joins `/OCGs` and the end of `/Order`, visible, in one undo
/// entry; the other layers are untouched.
#[test]
fn add_layer_appends_to_ocgs_and_order() {
    let mut s = fixture("basic-layers.pdf");
    let before = read_layers(&s.graph()).layers.len();
    let id = s.add_layer("Welds", &LayerEdit::new()).unwrap();
    let read = read_layers(&s.graph());
    assert_eq!(read.layers.len(), before + 1);
    let l = layer(&read, id.num);
    assert_eq!(l.name, "Welds");
    assert!(l.visible_by_default);
    let d = default_config(&s);
    assert_eq!(members(&s, &d, b"Order").last(), Some(&id));
    assert_eq!(s.undo(), Some(CommandKind::AddLayer { layer: id }));
    assert_eq!(read_layers(&s.graph()).layers.len(), before);
}

/// A document with no layers gains `/OCProperties` with `/Order [new]`.
#[test]
fn add_layer_to_a_document_without_layers() {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
    ]);
    let mut s = EditSession::new(Document::from_bytes(bytes).expect("parses"));
    let id = s.add_layer("Notes", &LayerEdit::new()).unwrap();
    let read = read_layers(&s.graph());
    assert_eq!(read.layers.len(), 1);
    assert!(layer(&read, id.num).visible_by_default);
    assert_eq!(members(&s, &default_config(&s), b"Order"), vec![id]);
}

/// Options apply to the new layer: hidden under an OFF base state goes
/// nowhere, shown goes to `/ON`; lock and print are written.
#[test]
fn add_layer_applies_the_other_options() {
    let mut s = fixture("basestate-off.pdf");
    let shown = s
        .add_layer("Shown", &LayerEdit::new().locked(true))
        .unwrap();
    let hidden = s
        .add_layer(
            "Hidden",
            &LayerEdit::new()
                .visible_by_default(false)
                .print(LayerOutputState::Never)
                .name("ignored"),
        )
        .unwrap();
    let read = read_layers(&s.graph());
    assert!(layer(&read, shown.num).visible_by_default);
    assert!(layer(&read, shown.num).locked);
    assert!(!layer(&read, hidden.num).visible_by_default);
    assert_eq!(layer(&read, hidden.num).name, "Hidden");
    let d = default_config(&s);
    assert!(members(&s, &d, b"ON").contains(&shown));
    assert!(!members(&s, &d, b"OFF").contains(&hidden));
    assert!(dict(&s, hidden).get(b"Usage").is_some());
}

/// Indirect `/OCGs` and `/Order` arrays are appended in place.
#[test]
fn add_layer_edits_indirect_arrays_in_place() {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs 3 0 R /D << /Order 4 0 R >> >> >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
        "[5 0 R]",
        "[5 0 R]",
        "<< /Type /OCG /Name (Old) >>",
    ]);
    let mut s = EditSession::new(Document::from_bytes(bytes).expect("parses"));
    let id = s.add_layer("New", &LayerEdit::new()).unwrap();
    let expect = Object::Array(vec![
        Object::Reference(ObjId::new(5, 0)),
        Object::Reference(id),
    ]);
    assert_eq!(resolved(&s, ObjId::new(3, 0)), expect);
    assert_eq!(resolved(&s, ObjId::new(4, 0)), expect);
    // The new group plus the two arrays; the catalog is not rewritten.
    assert_eq!(s.dirty_set().len(), 3);
}

/// An empty name is refused before anything is allocated.
#[test]
fn add_layer_refuses_an_empty_name() {
    let mut s = fixture("basic-layers.pdf");
    assert!(matches!(
        s.add_layer("", &LayerEdit::new()),
        Err(EditError::EmptyLayerName)
    ));
    assert_eq!(s.undo_depth(), 0);
}
