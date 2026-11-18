//! `FieldOutput::Fillable`: a materialized XFA form's fields written as
//! AcroForm fields whose widgets show what the flat page draws (Pass 647.12,
//! pdfcer-gui request G301).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, XfaMaterializeOutcome, XfaSignatures};
use pdfcer_core::filters::decode_stream;
use pdfcer_core::forms::{AcroForm, ButtonKind, FieldFlags, FieldType, FieldValue, parse_acroform};
use pdfcer_core::object::Object;
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::xfa::{
    FieldOutput, LayoutOptions, MergeOptions, PaintOptions, XfaFieldNoteKind, XfaFonts,
    layout_template, merge_data, read_template, read_xdp,
};

use crate::xfa_materialize::{DYNAMIC, contents, pdf_with};

const XDP: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1" layout="tb"><pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/><medium short="612pt" long="792pt"/></pageArea></pageSet>
<field name="name" w="200pt" h="20pt"><ui><textEdit/></ui><bind match="dataRef" ref="$record.name"/></field>
<field name="again" w="200pt" h="20pt"><ui><textEdit/></ui><bind match="dataRef" ref="$record.name"/></field>
<field name="agree" w="20pt" h="20pt"><ui><checkButton/></ui><items><text>yes</text><text>no</text></items></field>
<exclGroup name="size" layout="tb"><field name="s" w="20pt" h="20pt"><ui><checkButton shape="round"/></ui><items><text>S</text></items></field><field name="m" w="20pt" h="20pt"><ui><checkButton shape="round"/></ui><items><text>M</text></items></field></exclGroup>
<field name="colour" w="100pt" h="20pt"><ui><choiceList/></ui><items><text>Red</text><text>Blue</text></items><items save="1"><text>r</text><text>b</text></items></field>
<field name="ro" access="readOnly" w="100pt" h="20pt"><ui><textEdit/></ui><calculate><script>1</script></calculate><validate nullTest="error"/></field>
<field name="ghost" presence="invisible" w="100pt" h="20pt"><ui><textEdit/></ui></field>
<field name="pic" w="100pt" h="20pt"><ui><imageEdit/></ui></field>
</subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form1><name>Ken</name><agree>yes</agree><size>M</size><colour>b</colour><ro>fixed</ro><ghost>boo</ghost></form1></xfa:data></xfa:datasets></xdp:xdp>"#;

fn materialized(fields: FieldOutput) -> (Document, XfaMaterializeOutcome) {
    let original = pdf_with(XDP, DYNAMIC, "", &["<< >>"]);
    let doc = Document::from_bytes(original.clone()).unwrap();
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    let template = read_template(&xdp).unwrap().unwrap();
    let merged = merge_data(&template, &xdp, &MergeOptions::new()).unwrap();
    let layout = layout_template(&merged.template, &LayoutOptions::new()).unwrap();
    let mut s = EditSession::new(doc);
    let out = s
        .materialize_merged_xfa(
            &merged,
            &layout.pages,
            &PaintOptions::new().with_fields(fields),
            &XfaFonts::new(),
            XfaSignatures::Refuse,
        )
        .unwrap();
    let bytes = s.to_full_bytes(&SaveOptions::identity()).unwrap().0;
    (Document::from_bytes(bytes).unwrap(), out)
}

fn form(doc: &Document) -> AcroForm {
    parse_acroform(doc).unwrap()
}

fn appearance(doc: &Document, ap: &Object) -> String {
    let Object::Stream(s) = doc.resolve(ap) else {
        panic!("no appearance stream")
    };
    let raw = s.data_span.slice(doc.bytes()).unwrap();
    String::from_utf8_lossy(&decode_stream(&s.dict, raw).unwrap()).into_owned()
}

#[test]
fn flat_is_the_default_and_writes_no_fields() {
    let (doc, out) = materialized(FieldOutput::Flat);
    assert_eq!(out.fields_written, 0);
    assert!(form(&doc).fields.is_empty());
    assert!(contents(&doc)[0].contains("Ken"));
}

#[test]
fn each_field_kind_is_written_fillable() {
    let (doc, out) = materialized(FieldOutput::Fillable);
    let f = form(&doc);
    let get = |n: &str| f.field_by_name(n).unwrap_or_else(|| panic!("no {n}"));

    let name = get("form1[0].name[0]");
    assert_eq!(name.field_type, Some(FieldType::Text));
    assert_eq!(name.value, FieldValue::Text(b"Ken".to_vec()));
    assert!(name.default_appearance.is_some());

    let agree = get("form1[0].agree[0]");
    assert_eq!(agree.button_kind, Some(ButtonKind::Check));
    assert_eq!(agree.value, FieldValue::Name(b"yes".to_vec()));

    let size = get("form1[0].size[0]");
    assert_eq!(size.button_kind, Some(ButtonKind::Radio));
    assert_eq!(size.value, FieldValue::Name(b"M".to_vec()));
    assert_eq!(size.widgets.len(), 2);

    let colour = get("form1[0].colour[0]");
    assert_eq!(colour.field_type, Some(FieldType::Choice));
    assert!(colour.flags.0 & FieldFlags::COMBO != 0);
    assert_eq!(colour.options.len(), 2);

    let ro = get("form1[0].ro[0]");
    assert!(ro.flags.read_only() && ro.flags.required());

    assert!(f.field_by_name("form1[0].pic[0]").is_none());
    let notes: Vec<_> = out.field_notes.iter().map(|n| n.kind.clone()).collect();
    assert!(notes.contains(&XfaFieldNoteKind::Script));
    assert!(notes.contains(&XfaFieldNoteKind::Flat("imageEdit".into())));
}

#[test]
fn fields_bound_to_one_data_node_share_a_field() {
    let (doc, _) = materialized(FieldOutput::Fillable);
    let f = form(&doc);
    let name = f.field_by_name("form1[0].name[0]").unwrap();
    assert_eq!(name.widgets.len(), 2);
    assert!(f.field_by_name("form1[0].again[0]").is_none());
}

#[test]
fn the_value_moves_from_the_page_into_the_widget() {
    let (flat, _) = materialized(FieldOutput::Flat);
    let (doc, _) = materialized(FieldOutput::Fillable);
    let page = &contents(&doc)[0];
    assert!(!page.contains("(Ken)"), "{page}");
    let name = form(&doc);
    let name = name.field_by_name("form1[0].name[0]").unwrap();
    let id = Object::Reference(name.widgets[0].id);
    let w = doc.resolve(&id).as_dict().unwrap().clone();
    let ap = doc
        .resolve(w.get(b"AP").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let normal = appearance(&doc, ap.get(b"N").unwrap());
    assert!(normal.contains("Ken"), "{normal}");
    assert!(contents(&flat)[0].contains(normal.trim()), "{normal}");
}

#[test]
fn an_invisible_field_is_a_hidden_widget_and_a_page_lists_its_widgets() {
    let (doc, _) = materialized(FieldOutput::Fillable);
    let f = form(&doc);
    let ghost = f.field_by_name("form1[0].ghost[0]").unwrap();
    let id = Object::Reference(ghost.widgets[0].id);
    let w = doc.resolve(&id).as_dict().unwrap().clone();
    assert_eq!(w.get(b"F"), Some(&Object::Integer(2)));
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let page = Object::Reference(pages[0].id);
    let page = doc.resolve(&page).as_dict().unwrap().clone();
    assert!(page.get(b"Annots").is_some());
    assert_eq!(
        page.get(b"Tabs")
            .and_then(Object::as_name)
            .map(|n| &n.0[..]),
        Some(&b"S"[..])
    );
}

#[test]
fn a_written_field_can_be_filled() {
    let (doc, _) = materialized(FieldOutput::Fillable);
    let mut s = EditSession::new(doc);
    s.fill_text_field("form1[0].name[0]", "Emily").unwrap();
    let bytes = s.to_full_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let f = form(&doc);
    let name = f.field_by_name("form1[0].name[0]").unwrap();
    assert_eq!(name.value, FieldValue::Text(b"Emily".to_vec()));
}

#[test]
fn a_check_widget_holds_its_whole_border_stroke() {
    let (doc, _) = materialized(FieldOutput::Fillable);
    let f = form(&doc);
    let agree = f.field_by_name("form1[0].agree[0]").unwrap();
    let id = Object::Reference(agree.widgets[0].id);
    let w = doc.resolve(&id).as_dict().unwrap().clone();
    let Some(Object::Array(r)) = w.get(b"Rect") else {
        panic!("no Rect")
    };
    let n = |o: &Object| match o {
        Object::Real(v) => *v,
        Object::Integer(v) => *v as f64,
        _ => panic!("not a number"),
    };
    assert!(n(&r[2]) - n(&r[0]) > 20.0, "{r:?}");
}
