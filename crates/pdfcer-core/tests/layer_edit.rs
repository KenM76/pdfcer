//! `EditSession::set_layer_properties`: a layer's name, default
//! visibility, lock, print/export state and intent (ISO 32000-1 §8.11).

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    CommandKind, EditError, EditSession, HiddenLayerPolicy, LayerContentPolicy, LayerEdit,
    LayerIntent, LayerOutputState,
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

/// Page 1's content stream, as saved by a full rewrite and reloaded.
fn saved_content(s: &EditSession) -> String {
    let (bytes, _) = s
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("full rewrite");
    let doc = Document::from_bytes(bytes).expect("reopens");
    let view = doc.view();
    let Some(Object::Stream(stream)) = view.graph().value(ObjId::new(8, 0)).cloned() else {
        panic!("object 8 is not a stream");
    };
    assert!(!stream.dict.contains_key(b"Filter"));
    String::from_utf8_lossy(view.slice(stream.data_span).expect("in range")).into_owned()
}

/// Deleting a layer unwraps its section, keeps what it drew and what is
/// nested in it, and removes it from `/OCGs`, `/Order`, `/OFF` and
/// `/Properties`. One undo entry restores it all.
#[test]
fn delete_layer_unwraps_its_sections() {
    let mut s = fixture("painted-layers.pdf");
    let l2 = ObjId::new(5, 0);
    let outcome = s
        .delete_layer(l2, LayerContentPolicy::KeepUnlayered)
        .expect("deletes");
    assert!(outcome.changed);
    assert_eq!((outcome.sections, outcome.streams), (1, 1));

    let content = saved_content(&s);
    assert!(!content.contains("/L2"), "{content}");
    assert!(
        content.contains("0 0 0 rg 400 60 120 120 re f"),
        "{content}"
    );
    assert!(content.contains("/OC /L4 BDC"), "{content}");
    assert_eq!(content.matches("EMC").count(), 3, "{content}");
    assert_eq!(content.matches("BDC").count(), 3, "{content}");

    let read = read_layers(&s.graph());
    assert!(read.layers.iter().all(|l| l.id != l2));
    let d = default_config(&s);
    assert!(!members(&s, &d, b"OFF").contains(&l2));
    assert!(!members(&s, &d, b"Order").contains(&l2));
    let page = dict(&s, ObjId::new(3, 0));
    let props = page
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Properties"))
        .and_then(Object::as_dict)
        .expect("/Properties");
    assert!(props.get(b"L2").is_none());
    assert!(props.get(b"L4").is_some());

    assert_eq!(s.undo(), Some(CommandKind::DeleteLayer { layer: l2 }));
    assert!(read_layers(&s.graph()).layers.iter().any(|l| l.id == l2));
    assert!(saved_content(&s).contains("/OC /L2 BDC"));
}

/// A layer named by a membership dictionary is refused, by name, and
/// nothing is written.
#[test]
fn delete_layer_refuses_a_membership_member() {
    let mut s = fixture("ocmd-membership.pdf");
    for (layer, ocmd) in [(4, 10), (5, 12)] {
        match s.delete_layer(ObjId::new(layer, 0), LayerContentPolicy::KeepUnlayered) {
            Err(EditError::LayerInMembership { layer: l, ocmd: m }) => {
                assert_eq!(l, ObjId::new(layer, 0));
                assert_eq!(m.num, ocmd);
            }
            other => panic!("layer {layer}: {other:?}"),
        }
    }
    assert!(matches!(
        s.delete_layer(ObjId::new(7, 0), LayerContentPolicy::KeepUnlayered),
        Err(EditError::LayerNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// The unfiltered data of stream `num` after a full rewrite and reload.
fn saved_stream(s: &EditSession, num: u32) -> String {
    let (bytes, _) = s
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("full rewrite");
    let doc = Document::from_bytes(bytes).expect("reopens");
    let view = doc.view();
    let Some(Object::Stream(stream)) = view.graph().value(ObjId::new(num, 0)).cloned() else {
        panic!("object {num} is not a stream");
    };
    String::from_utf8_lossy(view.slice(stream.data_span).expect("in range")).into_owned()
}

/// Removing a layer's content turns its paints into `n` (a nested layer's
/// too) and leaves other layers and state alone.
#[test]
fn delete_layer_remove_content_paints_nothing() {
    let mut s = fixture("painted-layers.pdf");
    let outcome = s
        .delete_layer(ObjId::new(5, 0), LayerContentPolicy::RemoveContent)
        .expect("deletes");
    assert_eq!((outcome.sections, outcome.paints), (1, 2));
    let content = saved_stream(&s, 8);
    assert!(content.contains("400 60 120 120 re n"), "{content}");
    assert!(content.contains("/OC /L4 BDC"), "{content}");
    assert!(content.contains("400 220 120 120 re n"), "{content}");
    assert!(content.contains("60 60 120 120 re f"), "{content}");
    assert!(content.contains("0 600 612 60 re f"), "{content}");
    assert!(!content.contains("/L2"), "{content}");
}

/// A clip on the layer still clips once its content is removed.
#[test]
fn delete_layer_remove_content_keeps_a_clip() {
    let mut s = fixture("painted-layers.pdf");
    let outcome = s
        .delete_layer(ObjId::new(6, 0), LayerContentPolicy::RemoveContent)
        .expect("deletes");
    assert_eq!((outcome.sections, outcome.paints), (1, 0));
    assert!(saved_stream(&s, 8).contains("0 0 300 792 re W n"));
}

/// A page with a text run, an XObject and an annotation on the layer.
fn remove_session(content: &str, subtype: &str) -> EditSession {
    let form = "0 0 5 5 re f";
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] /D << /Order [4 0 R] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Properties << /L1 4 0 R >> /XObject << /X1 7 0 R >> >> /Contents 5 0 R /Annots [6 0 R 8 0 R] >>",
        "<< /Type /OCG /Name (Welds) >>",
        &format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        &format!("<< /Type /Annot /Subtype /{subtype} /Rect [0 0 10 10] /OC 4 0 R /Popup 8 0 R >>"),
        &format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /OC 4 0 R /Length {} >>\nstream\n{form}\nendstream",
            form.len() + 1
        ),
        "<< /Type /Annot /Subtype /Popup /Rect [0 0 10 10] /Parent 6 0 R >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

/// Text, an on-layer XObject's `Do` and an annotation with its pop-up are
/// removed; the text after a `Td` stays where it was.
#[test]
fn delete_layer_remove_content_removes_text_calls_and_annotations() {
    let mut s = remove_session(
        "BT 72 700 Td /OC /L1 BDC (abc) Tj EMC 0 -14 Td (def) Tj ET /X1 Do",
        "Square",
    );
    let outcome = s
        .delete_layer(ObjId::new(4, 0), LayerContentPolicy::RemoveContent)
        .expect("deletes");
    assert_eq!(
        (outcome.paints, outcome.xobject_calls, outcome.annotations),
        (1, 1, 1)
    );
    let content = saved_stream(&s, 5);
    assert!(!content.contains("abc"), "{content}");
    assert!(!content.contains("Do"), "{content}");
    assert!(content.contains("0 -14 Td (def) Tj"), "{content}");
    let Object::Dict(page) = resolved(&s, ObjId::new(3, 0)) else {
        panic!("page");
    };
    assert_eq!(page.get(b"Annots"), Some(&Object::Array(vec![])));
    assert_eq!(s.undo_depth(), 1);
}

/// Removing a run whose advance places later visible text is refused.
#[test]
fn delete_layer_remove_content_refuses_moving_text() {
    let mut s = remove_session(
        "BT 72 700 Td /OC /L1 BDC (abc) Tj EMC (def) Tj ET",
        "Square",
    );
    assert!(matches!(
        s.delete_layer(ObjId::new(4, 0), LayerContentPolicy::RemoveContent),
        Err(EditError::LayerContentNotRewritable { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// A form field's widget on the layer is refused.
#[test]
fn delete_layer_remove_content_refuses_a_widget() {
    let mut s = remove_session("0 0 1 1 re f", "Widget");
    match s.delete_layer(ObjId::new(4, 0), LayerContentPolicy::RemoveContent) {
        Err(EditError::LayerHasWidget { layer, annot }) => {
            assert_eq!((layer.num, annot.num), (4, 6));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.undo_depth(), 0);
}

/// Layers A (4), B (5), C (6), D (7), E (9). `/Order` is object 3,
/// `[A, 8 0 R, D, [E]]`, where object 8 is the folder `[(Parts) B C]`.
fn order_session() -> EditSession {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R 9 0 R] /D << /Order 3 0 R >> >> >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
        "[4 0 R 8 0 R 7 0 R [9 0 R]]",
        "<< /Type /OCG /Name (A) >>",
        "<< /Type /OCG /Name (B) >>",
        "<< /Type /OCG /Name (C) >>",
        "<< /Type /OCG /Name (D) >>",
        "[(Parts) 5 0 R 6 0 R]",
        "<< /Type /OCG /Name (E) >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

/// The layer panel as text: a folder is `(label: …)`, sublayers `{…}`.
fn shape<G: pdfcer_core::graph::ObjectGraph + ?Sized>(g: &G) -> String {
    fn walk(
        read: &pdfcer_core::layers::Layers,
        nodes: &[pdfcer_core::layers::OrderNode],
    ) -> String {
        nodes
            .iter()
            .map(|n| {
                let kids = walk(read, &n.children);
                match (&n.label, n.group) {
                    (Some(l), _) => {
                        format!("({l}:{}{kids})", if kids.is_empty() { "" } else { " " })
                    }
                    (None, Some(id)) => {
                        let name = &layer(read, id.num).name;
                        if kids.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}{{{kids}}}")
                        }
                    }
                    (None, None) => format!("[{kids}]"),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    let read = read_layers(g);
    walk(&read, &read.order)
}

/// A folder added at the top level rewrites only the `/Order` array object,
/// is one undo entry, and discloses a folder placed right after a layer.
#[test]
fn add_layer_folder_at_the_top_level() {
    let mut s = order_session();
    assert_eq!(shape(&s.graph()), "A (Parts: B C) D{E}");
    let out = s.add_layer_folder(&[], 1, "New").unwrap();
    assert_eq!(shape(&s.graph()), "A (New:) (Parts: B C) D{E}");
    assert!(out.changed);
    assert_eq!(out.path, vec![1]);
    assert!(out.follows_layer);
    assert_eq!(s.dirty_set().len(), 1);
    assert_eq!(s.undo(), Some(CommandKind::EditLayerOrder));
    assert_eq!(shape(&s.graph()), "A (Parts: B C) D{E}");

    let first = s.add_layer_folder(&[], 0, "Top").unwrap();
    assert!(!first.follows_layer);
    assert_eq!(shape(&s.graph()), "(Top:) A (Parts: B C) D{E}");
}

/// A folder inside a folder edits only that folder's array object.
#[test]
fn add_layer_folder_inside_a_folder() {
    let mut s = order_session();
    s.add_layer_folder(&[1], 2, "Sub").unwrap();
    assert_eq!(shape(&s.graph()), "A (Parts: B C (Sub:)) D{E}");
    assert_eq!(s.dirty_set().len(), 1);
    assert!(matches!(
        resolved(&s, ObjId::new(8, 0)),
        Object::Array(a) if a.len() == 4
    ));
}

/// Renaming writes the label in the folder's own array object; a layer is
/// not a folder.
#[test]
fn rename_layer_folder() {
    let mut s = order_session();
    s.rename_layer_folder(&[1], "Sheet metal").unwrap();
    assert_eq!(shape(&s.graph()), "A (Sheet metal: B C) D{E}");
    assert_eq!(s.dirty_set().len(), 1);
    assert!(matches!(
        s.rename_layer_folder(&[0], "X"),
        Err(EditError::NotALayerFolder { .. })
    ));
    assert!(matches!(
        s.rename_layer_folder(&[1], ""),
        Err(EditError::EmptyLayerName)
    ));
    assert!(!s.rename_layer_folder(&[1], "Sheet metal").unwrap().changed);
}

/// Removing a folder lifts its entries into its place; no layer goes.
#[test]
fn delete_layer_folder_lifts_its_entries() {
    let mut s = order_session();
    let layers = read_layers(&s.graph()).layers.len();
    let out = s.delete_layer_folder(&[1]).unwrap();
    assert_eq!(shape(&s.graph()), "A B C D{E}");
    assert_eq!(out.path, vec![1]);
    assert_eq!(read_layers(&s.graph()).layers.len(), layers);
    assert_eq!(s.dirty_set().len(), 1);
    assert!(matches!(
        s.delete_layer_folder(&[0]),
        Err(EditError::NotALayerFolder { .. })
    ));
}

/// Moves: into a folder (the folder keeps its object), a layer with its
/// sublayers, under a layer with none, and a last sublayer out.
#[test]
fn move_layer_node_cases() {
    let mut s = order_session();
    s.move_layer_node(&[0], &[0], 0).unwrap();
    assert_eq!(shape(&s.graph()), "(Parts: A B C) D{E}");
    assert_eq!(
        resolved(&s, ObjId::new(3, 0))
            .as_array()
            .and_then(|a| a.first().cloned()),
        Some(Object::Reference(ObjId::new(8, 0)))
    );
    s.undo();

    s.move_layer_node(&[2], &[], 0).unwrap();
    assert_eq!(shape(&s.graph()), "D{E} A (Parts: B C)");
    s.undo();

    s.move_layer_node(&[0], &[0, 0], 0).unwrap();
    assert_eq!(shape(&s.graph()), "(Parts: B{A} C) D{E}");
    s.undo();

    let out = s.move_layer_node(&[2, 0], &[], 0).unwrap();
    assert_eq!(shape(&s.graph()), "E A (Parts: B C) D");
    assert_eq!(out.path, vec![0]);
    // D's emptied sublayer array is removed, not left as `[]`.
    let r = |n| Object::Reference(ObjId::new(n, 0));
    assert_eq!(
        resolved(&s, ObjId::new(3, 0)),
        Object::Array(vec![r(9), r(4), r(8), r(7)])
    );
    s.undo();
    assert_eq!(shape(&s.graph()), "A (Parts: B C) D{E}");
}

/// Refusals: a folder as a layer's first sublayer reads back as its
/// sibling; positions that do not exist.
#[test]
fn layer_order_refusals() {
    let mut s = order_session();
    assert!(matches!(
        s.add_layer_folder(&[0], 0, "X"),
        Err(EditError::LayerOrderInexpressible)
    ));
    // Read after the folder is taken out, `[1, 0]` is D's sublayer E: a
    // folder as E's first sublayer is inexpressible too.
    assert!(matches!(
        s.move_layer_node(&[1], &[1, 0], 0),
        Err(EditError::LayerOrderInexpressible)
    ));
    assert!(matches!(
        s.add_layer_folder(&[7], 0, "X"),
        Err(EditError::LayerOrderPathNotFound { .. })
    ));
    assert!(matches!(
        s.add_layer_folder(&[], 9, "X"),
        Err(EditError::LayerOrderPathNotFound { .. })
    ));
    assert!(matches!(
        s.move_layer_node(&[], &[], 0),
        Err(EditError::LayerOrderPathNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// An arrangement edit survives an incremental save and reopen, and the
/// objects it did not touch are not in the update.
#[test]
fn layer_order_edit_round_trips() {
    let mut s = order_session();
    s.move_layer_node(&[0], &[0], 1).unwrap();
    let (bytes, _) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("reopens");
    let view = doc.view();
    assert_eq!(shape(view.graph()), "(Parts: B A C) D{E}");
    assert_eq!(s.dirty_set().len(), 2);
}

/// Layers Welds (4) and Holes (9); object 7 is a membership dictionary on
/// Welds. Annotation 6 carries `extra` and has the pop-up 8.
fn annot_session(extra: &str, subtype: &str) -> EditSession {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 9 0 R] /D << /Order [4 0 R 9 0 R] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [6 0 R 8 0 R] >>",
        "<< /Type /OCG /Name (Welds) >>",
        "<< /Length 0 >>\nstream\n\nendstream",
        &format!("<< /Type /Annot /Subtype /{subtype} /Rect [0 0 10 10] /Popup 8 0 R {extra} >>"),
        "<< /Type /OCMD /OCGs [4 0 R] >>",
        "<< /Type /Annot /Subtype /Popup /Rect [0 0 10 10] /Parent 6 0 R >>",
        "<< /Type /OCG /Name (Holes) >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

fn oc_of(s: &EditSession, num: u32) -> Option<&'static str> {
    match dict(s, ObjId::new(num, 0)).get(b"OC") {
        None => None,
        Some(Object::Reference(r)) if r.num == 4 => Some("Welds"),
        Some(Object::Reference(r)) if r.num == 9 => Some("Holes"),
        Some(other) => panic!("unexpected /OC {other:?}"),
    }
}

/// Put on a layer, move to another, take off: the annotation and its pop-up
/// change together, one undo entry each.
#[test]
fn set_annotation_layer_puts_moves_and_clears() {
    let mut s = annot_session("", "Square");
    let annot = ObjId::new(6, 0);
    let put = s
        .set_annotation_layer(annot, Some(ObjId::new(4, 0)))
        .unwrap();
    assert!(put.changed && put.popup_written);
    assert_eq!((put.before, put.subtype.as_str()), (None, "Square"));
    assert_eq!((oc_of(&s, 6), oc_of(&s, 8)), (Some("Welds"), Some("Welds")));

    let moved = s
        .set_annotation_layer(annot, Some(ObjId::new(9, 0)))
        .unwrap();
    assert_eq!(moved.before, Some(ObjId::new(4, 0)));
    assert_eq!((oc_of(&s, 6), oc_of(&s, 8)), (Some("Holes"), Some("Holes")));

    let cleared = s.set_annotation_layer(annot, None).unwrap();
    assert!(cleared.changed && cleared.after.is_none());
    assert_eq!((oc_of(&s, 6), oc_of(&s, 8)), (None, None));
    assert_eq!(s.undo_depth(), 3);

    s.undo().unwrap();
    assert_eq!((oc_of(&s, 6), oc_of(&s, 8)), (Some("Holes"), Some("Holes")));
}

/// Already there: nothing written, no undo entry.
#[test]
fn set_annotation_layer_no_op() {
    let mut s = annot_session("", "Square");
    s.set_annotation_layer(ObjId::new(6, 0), None)
        .map(|c| assert!(!c.changed))
        .unwrap();
    assert_eq!(s.undo_depth(), 0);
}

/// A membership dictionary is replaced and reported; a widget is accepted.
#[test]
fn set_annotation_layer_replaces_a_membership_and_takes_widgets() {
    let mut s = annot_session("/OC 7 0 R", "Widget");
    let c = s
        .set_annotation_layer(ObjId::new(6, 0), Some(ObjId::new(9, 0)))
        .unwrap();
    assert_eq!(c.before, Some(ObjId::new(7, 0)));
    assert_eq!(oc_of(&s, 6), Some("Holes"));
}

#[test]
fn set_annotation_layer_refusals() {
    let mut s = annot_session("", "Square");
    // The membership dictionary is not a registered group.
    assert!(matches!(
        s.set_annotation_layer(ObjId::new(6, 0), Some(ObjId::new(7, 0))),
        Err(EditError::LayerNotFound { .. })
    ));
    let mut locked = annot_session("/F 128", "Square");
    assert!(matches!(
        locked.set_annotation_layer(ObjId::new(6, 0), Some(ObjId::new(4, 0))),
        Err(EditError::AnnotationLocked { .. })
    ));
    assert_eq!(s.undo_depth() + locked.undo_depth(), 0);
}

/// Saved incrementally and reopened, the annotation reads as on the layer.
#[test]
fn set_annotation_layer_round_trips() {
    let mut s = annot_session("", "Square");
    s.set_annotation_layer(ObjId::new(6, 0), Some(ObjId::new(9, 0)))
        .unwrap();
    assert_eq!(s.dirty_set().len(), 2);
    let (bytes, _) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("reopens");
    let view = doc.view();
    let annots = pdfcer_core::annot::page_annotations(view.graph(), ObjId::new(3, 0));
    assert_eq!(annots.first().and_then(|a| a.oc), Some(ObjId::new(9, 0)));
}

// ---- Pass 358.4: page objects onto a layer --------------------------------

/// Objects 4 = Welds (bound as /L1), 9 = Holes (unbound); page 3's content 5.
fn content_session(content: &str) -> EditSession {
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    );
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 9 0 R] /D << /Order [4 0 R 9 0 R] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << /Properties << /L1 4 0 R >> /XObject << /Im 6 0 R >> >> >>",
        "<< /Type /OCG /Name (Welds) >>",
        &stream,
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /OC 4 0 R /Length 1 >>\nstream\n\u{0}\nendstream",
        "<< >>",
        "<< >>",
        "<< /Type /OCG /Name (Holes) >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

const THREE_PATHS: &str = "/OC /L1 BDC 0 0 m 10 10 l S 20 20 m 30 30 l S EMC 40 40 m 50 50 l S";

fn layers_of(s: &mut EditSession) -> Vec<Option<u32>> {
    s.page_objects(0)
        .unwrap()
        .objects
        .iter()
        .map(|o| o.oc().map(|id| id.num))
        .collect()
}

/// Put an unlayered object on a layer, split one out of an enclosing
/// section, take one off every layer; neighbours keep their layer.
#[test]
fn set_objects_layer_puts_splits_and_clears() {
    let mut s = content_session(THREE_PATHS);
    assert_eq!(layers_of(&mut s), [Some(4), Some(4), None]);

    let c = s
        .set_objects_layer(0, &[2], Some(ObjId::new(9, 0)))
        .unwrap();
    assert_eq!((c.moved, c.unchanged, c.binding_added), (1, 0, true));
    assert_eq!(c.property_name.as_deref(), Some("OC1"));
    assert_eq!(layers_of(&mut s), [Some(4), Some(4), Some(9)]);

    let c = s
        .set_objects_layer(0, &[0], Some(ObjId::new(9, 0)))
        .unwrap();
    assert_eq!((c.moved, c.binding_added), (1, false), "reuses /OC1");
    assert_eq!(layers_of(&mut s), [Some(9), Some(4), Some(9)]);

    s.set_objects_layer(0, &[1], None).unwrap();
    assert_eq!(layers_of(&mut s), [Some(9), None, Some(9)]);

    assert_eq!(s.undo_depth(), 3);
    while s.undo_depth() > 0 {
        s.undo().unwrap();
    }
    assert_eq!(layers_of(&mut s), [Some(4), Some(4), None]);
}

/// A selection already on the layer writes nothing.
#[test]
fn set_objects_layer_no_op() {
    let mut s = content_session(THREE_PATHS);
    let c = s
        .set_objects_layer(0, &[0, 1], Some(ObjId::new(4, 0)))
        .unwrap();
    assert_eq!((c.moved, c.unchanged), (0, 2));
    let c = s.set_objects_layer(0, &[2], None).unwrap();
    assert_eq!((c.moved, c.unchanged), (0, 1));
    assert_eq!(s.undo_depth(), 0);
}

/// A structure tag OUTSIDE the layer section is left whole; one inside it,
/// or a section opened at another `q` depth, refuses by name (§14.6).
#[test]
fn set_objects_layer_respects_nesting() {
    let mut s =
        content_session("/P << /MCID 0 >> BDC /OC /L1 BDC 0 0 m 1 1 l S 2 2 m 3 3 l S EMC EMC");
    s.set_objects_layer(0, &[0], None).unwrap();
    assert_eq!(layers_of(&mut s), [None, Some(4)]);

    let mut s = content_session("/OC /L1 BDC /P << /MCID 0 >> BDC 0 0 m 1 1 l S EMC EMC");
    assert!(matches!(
        s.set_objects_layer(0, &[0], None),
        Err(EditError::VectorEdit(
            pdfcer_core::vector::VectorEditError::LayerSectionHoldsTaggedContent { .. }
        ))
    ));

    // A text object that opens a section it does not close.
    let mut s = content_session("/OC /L1 BDC BT /P << /MCID 0 >> BDC 0 0 Td (a) Tj ET EMC EMC");
    assert!(matches!(
        s.set_objects_layer(0, &[0], None),
        Err(EditError::VectorEdit(
            pdfcer_core::vector::VectorEditError::LayerSpanUnbalanced { .. }
        ))
    ));

    let mut s = content_session("/OC /L1 BDC q 0 0 m 1 1 l S Q EMC");
    assert!(matches!(
        s.set_objects_layer(0, &[0], Some(ObjId::new(9, 0))),
        Err(EditError::VectorEdit(
            pdfcer_core::vector::VectorEditError::LayerSectionCrossesNesting { .. }
        ))
    ));
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn set_objects_layer_refusals() {
    let mut s = content_session("0 0 m 1 1 l S q 1 0 0 1 0 0 cm /Im Do Q");
    assert!(matches!(
        s.set_objects_layer(0, &[0], Some(ObjId::new(7, 0))),
        Err(EditError::LayerNotFound { .. })
    ));
    assert!(matches!(
        s.set_objects_layer(0, &[0, 5], Some(ObjId::new(9, 0))),
        Err(EditError::VectorEdit(
            pdfcer_core::vector::VectorEditError::ObjectOutOfRange { index: 5, .. }
        ))
    ));
    // The image's own /OC can only be intersected by a section.
    assert!(matches!(
        s.set_objects_layer(0, &[1], Some(ObjId::new(9, 0))),
        Err(EditError::LayerContentNotRewritable { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// Saved and reopened, the membership reads back; the page's resources
/// gained the binding.
#[test]
fn set_objects_layer_round_trips() {
    let mut s = content_session(THREE_PATHS);
    s.set_objects_layer(0, &[0, 2], Some(ObjId::new(9, 0)))
        .unwrap();
    let (bytes, _) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("incremental save");
    let mut reopened = EditSession::new(Document::from_bytes(bytes).expect("reopens"));
    assert_eq!(layers_of(&mut reopened), [Some(9), Some(4), Some(9)]);
}

// ---- Pass 358.5: new content onto a layer ----------------------------------

const ONE_PATH: &str = "0 0 m 10 10 l S";

fn holes() -> ObjId {
    ObjId::new(9, 0)
}

fn text_request() -> pdfcer_core::text_edit::AddTextRequest {
    pdfcer_core::text_edit::AddTextRequest::new(0, (100.0, 100.0), "Hi".to_owned())
}

/// New text lands on the layer in its own section; the page's original
/// stream is untouched and the add is one undo entry.
#[test]
fn add_text_on_a_layer() {
    let mut s = content_session(ONE_PATH);
    let original = saved_stream(&s, 5);
    s.add_text(&text_request().on_layer(holes())).unwrap();
    assert_eq!(layers_of(&mut s), [None, Some(9)]);
    assert_eq!(saved_stream(&s, 5), original);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo_kind(), Some(CommandKind::AddText));
    let props = s
        .graph()
        .resolve(&Object::Reference(ObjId::new(3, 0)))
        .as_dict()
        .and_then(|d| d.get(b"Resources"))
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Properties"))
        .and_then(Object::as_dict)
        .and_then(|p| p.get(b"OC1"))
        .and_then(Object::as_reference);
    assert_eq!(props, Some(holes()));
    s.undo().unwrap();
    assert_eq!(layers_of(&mut s), [None]);
    assert!(s.dirty_set().is_empty());
}

/// A layer the page already names reuses the name and adds no binding.
#[test]
fn add_text_reuses_the_page_binding() {
    let mut s = content_session(ONE_PATH);
    s.add_text(&text_request().on_layer(ObjId::new(4, 0)))
        .unwrap();
    assert_eq!(layers_of(&mut s), [None, Some(4)]);
    // Only the new stream and the page's /Contents array: no binding.
    let props = s
        .graph()
        .resolve(&Object::Reference(ObjId::new(3, 0)))
        .as_dict()
        .and_then(|d| d.get(b"Resources"))
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Properties"))
        .and_then(Object::as_dict)
        .map(|p| p.0.len());
    assert_eq!(props, Some(1));
}

/// An unregistered layer is refused before anything is written.
#[test]
fn add_text_refuses_an_unregistered_layer() {
    let mut s = content_session(ONE_PATH);
    let err = s
        .add_text(&text_request().on_layer(ObjId::new(7, 0)))
        .unwrap_err();
    assert!(matches!(
        err,
        pdfcer_core::text_edit::AddTextError::Layer(ref inner)
            if matches!(**inner, EditError::LayerNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
    assert!(s.dirty_set().is_empty());
    // The one-shot route has no session to hold the writes.
    let doc = Document::from_bytes(
        s.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
            .unwrap()
            .0,
    )
    .unwrap();
    assert!(matches!(
        pdfcer_core::text_edit::add_text(&doc, &text_request().on_layer(holes())),
        Err(pdfcer_core::text_edit::AddTextError::LayerNeedsSession)
    ));
}

/// Saved and reopened, the new text reads as on the layer.
#[test]
fn add_text_on_a_layer_round_trips() {
    let mut s = content_session(ONE_PATH);
    s.add_text(&text_request().on_layer(holes())).unwrap();
    let (bytes, _) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap();
    let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(layers_of(&mut reopened), [None, Some(9)]);
}

#[test]
fn add_image_on_a_layer() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images/rgb8.png");
    let image = pdfcer_core::image_import::import(&std::fs::read(path).unwrap()).unwrap();
    let rect = pdfcer_core::page_tree::Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 110.0,
        ury: 110.0,
    };
    let mut s = content_session(ONE_PATH);
    let original = saved_stream(&s, 5);
    s.add_image(&pdfcer_core::edit::NewImage::new(0, rect, &image).on_layer(holes()))
        .unwrap();
    assert_eq!(layers_of(&mut s), [None, Some(9)]);
    assert_eq!(saved_stream(&s, 5), original);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo_kind(), Some(CommandKind::AddImage));
    assert!(matches!(
        s.add_image(&pdfcer_core::edit::NewImage::new(0, rect, &image).on_layer(ObjId::new(7, 0))),
        Err(EditError::LayerNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 1);
}

fn square() -> pdfcer_core::annot_author::MarkupSpec {
    pdfcer_core::annot_author::MarkupSpec::Square {
        rect: pdfcer_core::page_tree::Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 200.0,
            ury: 150.0,
        },
        border: Some(pdfcer_core::annot_author::Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 2.0,
        border_effect: None,
    }
}

fn on_holes() -> pdfcer_core::edit::MarkupOptions {
    pdfcer_core::edit::MarkupOptions {
        layer: Some(holes()),
        ..Default::default()
    }
}

/// An authored annotation gets `/OC`; the page content is untouched.
#[test]
fn add_markup_annotation_on_a_layer() {
    let mut s = content_session(ONE_PATH);
    let id = s.add_markup_with(0, &square(), &on_holes()).unwrap();
    assert_eq!(
        dict(&s, id).get(b"OC").and_then(Object::as_reference),
        Some(holes())
    );
    assert_eq!(s.undo_depth(), 1);
    assert!(matches!(
        s.undo_kind(),
        Some(CommandKind::AddAnnotation { .. })
    ));
    assert!(!s.dirty_set().contains(ObjId::new(5, 0)));
    s.undo().unwrap();
    assert!(s.dirty_set().is_empty());
}

/// Drawn as content, the shape's objects are on the layer.
#[test]
fn add_markup_as_content_on_a_layer() {
    let mut s = content_session(ONE_PATH);
    let original = saved_stream(&s, 5);
    let drawn = s.add_markup_as_content(0, &square(), &on_holes()).unwrap();
    assert!(!drawn.objects.is_empty());
    let layers = layers_of(&mut s);
    assert_eq!(layers.first(), Some(&None));
    assert!(layers.iter().skip(1).all(|l| *l == Some(9)), "{layers:?}");
    assert_eq!(saved_stream(&s, 5), original);
    assert_eq!(s.undo_depth(), 1);
}

/// A pasted selection is placed on the layer as one gesture.
#[test]
fn paste_objects_on_a_layer() {
    let mut s = content_session(THREE_PATHS);
    let original = saved_stream(&s, 5);
    let clip = s.copy_objects(0, &[2]).unwrap();
    s.paste_objects_on_layer(
        0,
        &clip,
        pdfcer_core::vector::Matrix::IDENTITY,
        Some(holes()),
    )
    .unwrap();
    assert_eq!(layers_of(&mut s), [Some(4), Some(4), None, Some(9)]);
    assert_eq!(saved_stream(&s, 5), original);
    assert_eq!(s.undo_depth(), 1);
    // `None` is plain paste: the copy keeps no layer.
    s.paste_objects_on_layer(0, &clip, pdfcer_core::vector::Matrix::IDENTITY, None)
        .unwrap();
    assert_eq!(layers_of(&mut s), [Some(4), Some(4), None, Some(9), None]);
}

// ---- Pass 358.6: merge layers ---------------------------------------------

/// Merging rebinds `/Properties`; the content stream is not rewritten, the
/// merged groups leave `/OCGs`, `/Order` and `/OFF`, and one undo restores
/// them.
#[test]
fn merge_layers_rebinds_without_rewriting_content() {
    let mut s = fixture("painted-layers.pdf");
    let original = saved_stream(&s, 8);
    let (visible, hidden, nested) = (ObjId::new(4, 0), ObjId::new(5, 0), ObjId::new(7, 0));
    let outcome = s.merge_layers(visible, &[hidden, nested]).unwrap();
    assert!(outcome.changed);
    assert_eq!((outcome.layers, outcome.bindings), (2, 2));
    let said = outcome.disclosures.join("\n");
    assert!(
        said.contains("\"Hidden Box\"") && said.contains("\"Visible Box\""),
        "{said}"
    );
    assert!(!s.dirty_set().contains(ObjId::new(8, 0)));
    assert_eq!(saved_stream(&s, 8), original);

    let page = dict(&s, ObjId::new(3, 0));
    let props = page
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Properties"))
        .and_then(Object::as_dict)
        .expect("/Properties")
        .clone();
    for (name, want) in [(b"L1", 4), (b"L2", 4), (b"L3", 6), (b"L4", 4)] {
        assert_eq!(
            props
                .get(name)
                .and_then(Object::as_reference)
                .map(|r| r.num),
            Some(want)
        );
    }
    let ids: Vec<u32> = read_layers(&s.graph())
        .layers
        .iter()
        .map(|l| l.id.num)
        .collect();
    assert_eq!(ids, [4, 6]);
    let d = default_config(&s);
    assert_eq!(members(&s, &d, b"Order"), [visible, ObjId::new(6, 0)]);
    assert_eq!(members(&s, &d, b"OFF"), [ObjId::new(6, 0)]);

    assert_eq!(s.undo(), Some(CommandKind::MergeLayers { target: visible }));
    assert!(s.dirty_set().is_empty());
}

/// An image XObject's `/OC` follows the merge, and the page objects drawn
/// in the section report the target.
#[test]
fn merge_layers_retargets_xobject_oc() {
    let mut s = content_session(THREE_PATHS);
    let outcome = s.merge_layers(holes(), &[ObjId::new(4, 0)]).unwrap();
    assert_eq!((outcome.bindings, outcome.xobjects), (1, 1));
    assert_eq!(layers_of(&mut s), [Some(9), Some(9), None]);
    assert_eq!(
        dict_or_stream(&s, ObjId::new(6, 0))
            .get(b"OC")
            .and_then(Object::as_reference),
        Some(holes())
    );
}

/// An annotation's `/OC` follows the merge.
#[test]
fn merge_layers_retargets_annotation_oc() {
    let mut s = content_session(ONE_PATH);
    let id = s.add_markup_with(0, &square(), &on_holes()).unwrap();
    let outcome = s.merge_layers(ObjId::new(4, 0), &[holes()]).unwrap();
    assert_eq!(outcome.annotations, 1);
    assert_eq!(
        dict(&s, id).get(b"OC").and_then(Object::as_reference),
        Some(ObjId::new(4, 0))
    );
    assert_eq!(s.undo_depth(), 2);
}

/// A membership dictionary naming a merged group names the target
/// afterwards; one that does not is left alone.
#[test]
fn merge_layers_retargets_membership() {
    let mut s = fixture("ocmd-membership.pdf");
    let a = ObjId::new(4, 0);
    let outcome = s.merge_layers(a, &[ObjId::new(5, 0)]).unwrap();
    assert_eq!(outcome.memberships, 1);
    assert_eq!(
        dict(&s, ObjId::new(12, 0))
            .get(b"OCGs")
            .and_then(Object::as_reference),
        Some(a)
    );
    assert!(!s.dirty_set().contains(ObjId::new(10, 0)));
    assert!(!s.dirty_set().contains(ObjId::new(22, 0)));
}

/// Merging a layer into itself writes nothing; an unregistered layer is
/// refused before any write.
#[test]
fn merge_layers_no_op_and_refusal() {
    let mut s = fixture("painted-layers.pdf");
    let visible = ObjId::new(4, 0);
    let outcome = s.merge_layers(visible, &[visible]).unwrap();
    assert!(!outcome.changed);
    assert!(matches!(
        s.merge_layers(visible, &[ObjId::new(5, 0), ObjId::new(3, 0)]),
        Err(EditError::LayerNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

fn dict_or_stream(s: &EditSession, id: ObjId) -> Dict {
    match resolved(s, id) {
        Object::Dict(d) => d,
        Object::Stream(st) => st.dict,
        other => panic!("{id} is {other:?}"),
    }
}

// ---- Pass 358.6: flatten layers ----

/// Hidden layers refuse a flatten until told what to do with them; nothing
/// is written.
#[test]
fn flatten_layers_refuses_hidden_layers_by_default() {
    let mut s = fixture("painted-layers.pdf");
    match s.flatten_layers(HiddenLayerPolicy::default()) {
        Err(EditError::HiddenLayersNeedPolicy { layers }) => {
            assert_eq!(layers, [ObjId::new(5, 0), ObjId::new(6, 0)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.undo_depth(), 0);
    assert_eq!(read_layers(&s.graph()).layers.len(), 4);
}

/// `Remove` keeps what visible layers draw, removes what hidden ones draw
/// (a visible layer nested in a hidden one with it), keeps a hidden clip,
/// and leaves no layer and no section. One undo entry restores it all.
#[test]
fn flatten_layers_remove_keeps_what_the_page_showed() {
    let mut s = fixture("painted-layers.pdf");
    let original = saved_stream(&s, 8);
    let outcome = s
        .flatten_layers(HiddenLayerPolicy::Remove)
        .expect("flattens");
    assert!(outcome.changed);
    assert_eq!((outcome.layers, outcome.hidden_layers), (4, 2));
    assert_eq!(outcome.sections, 4);
    let content = saved_stream(&s, 8);
    assert!(!content.contains("BDC"), "{content}");
    assert!(!content.contains("EMC"), "{content}");
    assert!(content.contains("60 60 120 120 re f"), "{content}");
    assert!(content.contains("400 60 120 120 re n"), "{content}");
    assert!(content.contains("400 220 120 120 re n"), "{content}");
    assert!(content.contains("0 0 300 792 re W n"), "{content}");
    assert!(content.contains("0 600 612 60 re f"), "{content}");
    assert!(read_layers(&s.graph()).layers.is_empty());
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.starts_with("removed what hidden \"Hidden Box\", \"Clip Only\" drew")),
        "{:?}",
        outcome.disclosures
    );
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo(), Some(CommandKind::FlattenLayers));
    assert_eq!(saved_stream(&s, 8), original);
    assert_eq!(read_layers(&s.graph()).layers.len(), 4);
}

/// `Show` keeps what hidden layers draw; a second flatten has nothing to do.
#[test]
fn flatten_layers_show_keeps_hidden_content() {
    let mut s = fixture("painted-layers.pdf");
    let outcome = s.flatten_layers(HiddenLayerPolicy::Show).expect("flattens");
    assert_eq!((outcome.layers, outcome.paints), (4, 0));
    let content = saved_stream(&s, 8);
    assert!(!content.contains("BDC"), "{content}");
    assert!(content.contains("400 60 120 120 re f"), "{content}");
    assert!(content.contains("400 220 120 120 re f"), "{content}");
    let again = s.flatten_layers(HiddenLayerPolicy::Refuse).expect("no-op");
    assert!(!again.changed);
    assert_eq!(s.undo_depth(), 1);
}

/// A refusal part-way through undoes the layers already flattened and
/// keeps the redo stack.
#[test]
fn flatten_layers_rolls_back_a_refusal() {
    let mut s = fixture("ocmd-membership.pdf");
    for id in [4, 5] {
        s.set_layer_properties(
            ObjId::new(id, 0),
            &LayerEdit::new().visible_by_default(false),
        )
        .unwrap();
    }
    s.add_layer("Extra", &LayerEdit::new()).unwrap();
    let redo = s.add_layer("Redo", &LayerEdit::new()).unwrap();
    s.undo();
    let depth = s.undo_depth();
    let layers = read_layers(&s.graph()).layers.len();
    assert!(matches!(
        s.flatten_layers(HiddenLayerPolicy::Show),
        Err(EditError::LayerInMembership { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(read_layers(&s.graph()).layers.len(), layers);
    assert_eq!(s.redo(), Some(CommandKind::AddLayer { layer: redo }));
}
