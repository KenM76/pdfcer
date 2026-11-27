//! Every font any `/DA` on a pasted field or its widgets names resolves in the
//! destination's `/AcroForm /DR /Font` (ISO 32000-2 §12.7.3.3, §12.7.4.3).
//!
//! The source field `f` names `/Helv`; its two widgets name `/Cour` and
//! `/Cour_1`. The destination binds `/Cour` to a DIFFERENT font, so the copied
//! Courier needs a fresh name, and that name must not be `/Cour_1`, which the
//! clip itself installs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::formclip::{FieldCopyNaming, FieldPastePolicy, FieldSetPastePolicy, PasteTooltip};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;

use super::block_layout::build_pdf;

fn session(bodies: &[&str]) -> EditSession {
    let bodies: Vec<String> = bodies.iter().map(|b| (*b).to_owned()).collect();
    EditSession::new(Document::from_bytes(build_pdf(&bodies)).expect("fixture"))
}

fn source() -> EditSession {
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 10 0 R] \
         /DR << /Font << /Helv 7 0 R /Cour 8 0 R /Cour_1 9 0 R /ZaDb 13 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Annots [5 0 R 6 0 R 11 0 R 12 0 R] >>",
        "<< /T (f) /FT /Tx /DA (/Helv 9 Tf 0 g) /Kids [5 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R \
         /Rect [100 600 300 630] /F 4 /DA (/Cour 10 Tf 0 g) >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R \
         /Rect [100 500 300 530] /F 4 /DA (/Cour_1 11 Tf 0 g) >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>",
        "<< /T (g) /FT /Tx /DA (/Helv 8 Tf 0 g) /Kids [11 0 R 12 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /Parent 10 0 R /P 3 0 R \
         /Rect [100 400 300 430] /F 4 /DA (/ZaDb 8 Tf 0 g) >>",
        "<< /Type /Annot /Subtype /Widget /Parent 10 0 R /P 3 0 R \
         /Rect [100 300 300 330] /F 4 >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>",
    ])
}

/// `/Cour` here is Symbol; `f` is a merged field naming it.
fn destination() -> EditSession {
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DR << /Font << /Cour 5 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /T (f) /FT /Tx /DA (/Cour 12 Tf 0 g) \
         /P 3 0 R /Rect [50 50 150 70] /F 4 >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>",
    ])
}

fn rect() -> Rect {
    Rect {
        llx: 100.0,
        lly: 100.0,
        urx: 300.0,
        ury: 130.0,
    }
}

fn new_field(name: &str) -> FieldPastePolicy {
    FieldPastePolicy::NewField {
        name: name.to_owned(),
        tooltip: PasteTooltip::Declined,
        copy_value: false,
        copy_actions: false,
    }
}

fn reload(s: &EditSession) -> EditSession {
    let (bytes, _) = s.to_full_bytes(&Default::default()).expect("save");
    EditSession::new(Document::from_bytes(bytes).expect("reload"))
}

fn dict_of(s: &EditSession, id: ObjId) -> Dict {
    s.graph().resolved(id).as_dict().cloned().expect("dict")
}

fn dr_fonts(s: &EditSession) -> Dict {
    let graph = s.graph();
    let catalog = graph.catalog_dict().expect("catalog");
    let af = graph
        .resolve(catalog.get(b"AcroForm").unwrap())
        .as_dict()
        .cloned()
        .unwrap();
    let dr = graph
        .resolve(af.get(b"DR").unwrap())
        .as_dict()
        .cloned()
        .unwrap();
    graph
        .resolve(dr.get(b"Font").unwrap())
        .as_dict()
        .cloned()
        .unwrap()
}

fn da_of(dict: &Dict) -> Option<String> {
    match dict.get(b"DA")? {
        Object::String(da) => Some(String::from_utf8_lossy(da).into_owned()),
        _ => None,
    }
}

/// The `/BaseFont` the dictionary's `/DA` selects through the saved `/DR`.
fn da_base_font(s: &EditSession, dict: &Dict) -> Option<String> {
    let da = da_of(dict)?;
    let name = da
        .split_whitespace()
        .next()?
        .trim_start_matches('/')
        .to_owned();
    let fonts = dr_fonts(s);
    let font = s
        .graph()
        .resolve(fonts.get(name.as_bytes())?)
        .as_dict()
        .cloned()?;
    match font.get(b"BaseFont")? {
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

fn kids(s: &EditSession, field: ObjId) -> Vec<ObjId> {
    match dict_of(s, field).get(b"Kids") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|k| match k {
                Object::Reference(id) => *id,
                other => panic!("kid {other:?}"),
            })
            .collect(),
        _ => vec![field],
    }
}

/// `[field font, widget fonts...]` for a pasted field, read from saved bytes.
fn fonts_used(s: &EditSession, field: ObjId) -> Vec<Option<String>> {
    let mut out = vec![da_base_font(s, &dict_of(s, field))];
    for kid in kids(s, field) {
        if kid != field {
            out.push(da_base_font(s, &dict_of(s, kid)));
        }
    }
    out
}

fn some(names: &[&str]) -> Vec<Option<String>> {
    names.iter().map(|n| Some((*n).to_owned())).collect()
}

#[test]
fn the_clip_carries_every_font_a_widget_names() {
    let clip = source().copy_field("f").unwrap();
    let fonts: Vec<&[u8]> = clip.carried_fonts();
    assert_eq!(fonts, [&b"Helv"[..], b"Cour", b"Cour_1"]);
    assert_eq!(clip.carried_font(), Some(&b"Helv"[..]));
}

#[test]
fn every_widget_font_lands_and_a_collision_never_takes_a_name_the_clip_installs() {
    let clip = source().copy_field("f").unwrap();
    let mut s = destination();
    let outcome = s
        .paste_field(&clip, 0, rect(), &new_field("pasted"))
        .unwrap();
    assert_eq!(
        outcome.font_renames,
        BTreeMap::from([("Cour".to_owned(), "Cour_2".to_owned())])
    );
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("already uses the font resource /Cour")),
        "{:?}",
        outcome.disclosures
    );
    let r = reload(&s);
    assert_eq!(
        fonts_used(&r, outcome.field_id),
        some(&["Helvetica", "Courier", "Times-Roman"])
    );
    let widgets = kids(&r, outcome.field_id);
    assert_eq!(
        da_of(&dict_of(&r, widgets[0])).as_deref(),
        Some("/Cour_2 10 Tf 0 g")
    );
    assert_eq!(
        da_of(&dict_of(&r, widgets[1])).as_deref(),
        Some("/Cour_1 11 Tf 0 g")
    );
    // The destination's own field still names its own Symbol font.
    assert_eq!(
        fonts_used(
            &r,
            ObjId {
                num: 4,
                generation: 0
            }
        ),
        some(&["Symbol"])
    );
}

#[test]
fn pasting_into_the_source_renames_nothing() {
    let mut s = source();
    let clip = s.copy_field("f").unwrap();
    let before = dr_fonts(&s).len();
    let outcome = s.paste_field(&clip, 0, rect(), &new_field("f2")).unwrap();
    assert!(
        outcome.font_renames.is_empty(),
        "{:?}",
        outcome.font_renames
    );
    let r = reload(&s);
    assert_eq!(dr_fonts(&r).len(), before);
    assert_eq!(
        fonts_used(&r, outcome.field_id),
        some(&["Helvetica", "Courier", "Times-Roman"])
    );
}

#[test]
fn an_additional_widget_installs_its_own_font() {
    let clip = source().copy_field("f").unwrap();
    let mut s = destination();
    let policy = FieldPastePolicy::AdditionalWidget {
        existing: "f".to_owned(),
    };
    let outcome = s.paste_field(&clip, 0, rect(), &policy).unwrap();
    assert!(!outcome.created);
    assert_eq!(
        outcome.font_renames,
        BTreeMap::from([("Cour".to_owned(), "Cour_1".to_owned())])
    );
    let r = reload(&s);
    let added = *outcome.widget_ids.last().unwrap();
    assert_eq!(
        da_base_font(&r, &dict_of(&r, added)).as_deref(),
        Some("Courier")
    );
    // The shared field keeps its own /DA and its own font.
    let field = dict_of(&r, outcome.field_id);
    assert_eq!(da_of(&field).as_deref(), Some("/Cour 12 Tf 0 g"));
    assert_eq!(da_base_font(&r, &field).as_deref(), Some("Symbol"));
}

#[test]
fn a_set_paste_installs_every_members_widget_fonts() {
    let set = source().copy_fields(&["f", "g"]).unwrap();
    let mut s = destination();
    let policy = FieldSetPastePolicy::NewFields {
        names: Some(vec!["f1".to_owned(), "g1".to_owned()]),
        naming: FieldCopyNaming::ContinueNumber,
        tooltip: PasteTooltip::Declined,
        copy_value: false,
        copy_actions: false,
    };
    let outcome = s.paste_fields(&set, 0, (100.0, 100.0), &policy).unwrap();
    assert_eq!(
        outcome.fields[0].font_renames,
        BTreeMap::from([("Cour".to_owned(), "Cour_2".to_owned())])
    );
    assert!(outcome.fields[1].font_renames.is_empty());
    let r = reload(&s);
    assert_eq!(
        fonts_used(&r, outcome.fields[0].field_id),
        some(&["Helvetica", "Courier", "Times-Roman"])
    );
    let mut g = fonts_used(&r, outcome.fields[1].field_id);
    assert_eq!(
        g.pop(),
        Some(None),
        "the second widget has no /DA of its own"
    );
    assert_eq!(g, some(&["Helvetica", "ZapfDingbats"]));
}
