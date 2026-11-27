//! A pasted field that leaves its parent carries what it inherited
//! (ISO 32000-1 §12.7.3.1 Table 220; the form defaults, Table 218), and the
//! field and widget dictionaries travel whole minus their source-bound keys.
//!
//! The fixture is a kid `grp.a` that sets only `/T` and `/Q`: its type,
//! flags, value, default value and length limit come from `grp`, its `/DA`
//! from the `/AcroForm`. Every inherited value differs from what a fresh
//! field would get, so a paste that carried nothing cannot pass.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::formclip::{FieldClip, FieldPastePolicy, PasteTooltip};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;

use super::block_layout::build_pdf;

fn form() -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 9 Tf 0 0 1 rg) /Q 2 /DR << /Font << /Helv 6 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R] >>",
        "<< /T (grp) /FT /Tx /Ff 4096 /V (inherited value) /DV (dflt) /MaxLen 9 \
         /Kids [5 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /T (a) /Q 1 /Parent 4 0 R /P 3 0 R \
         /Rect [100 600 300 630] /F 4 /NM (n1) /StructParent 3 /XXpdfcerKeep (kept) >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let bodies: Vec<String> = bodies.iter().map(|b| (*b).to_owned()).collect();
    EditSession::new(Document::from_bytes(build_pdf(&bodies)).expect("fixture"))
}

fn paste(s: &mut EditSession, clip: &FieldClip, copy_value: bool) -> Vec<String> {
    let policy = FieldPastePolicy::NewField {
        name: "solo".to_owned(),
        tooltip: PasteTooltip::Declined,
        copy_value,
        copy_actions: false,
    };
    let rect = Rect {
        llx: 100.0,
        lly: 100.0,
        urx: 300.0,
        ury: 130.0,
    };
    let outcome = s.paste_field(clip, 0, rect, &policy).expect("paste");
    let materialized = outcome.materialized_keys.clone();
    assert_eq!(outcome.dropped_keys.get("StructParent"), Some(&1));
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("structure-tree entry")),
        "{:?}",
        outcome.disclosures
    );
    if !materialized.is_empty() {
        assert!(
            outcome.disclosures.iter().any(|d| d.contains("inherited")),
            "{:?}",
            outcome.disclosures
        );
    }
    materialized
}

/// The pasted field's own dictionary, read back from the saved bytes.
fn saved_field(s: &EditSession) -> (forms::Field, Dict) {
    let (bytes, _) = s.to_full_bytes(&Default::default()).expect("save");
    let doc = Document::from_bytes(bytes).expect("reopen");
    let view = doc.view();
    let field = forms::parse_acroform(&view)
        .and_then(|f| f.field_by_name("solo").cloned())
        .expect("pasted field");
    let dict = view.resolved(field.id).as_dict().cloned().expect("dict");
    (field, dict)
}

#[test]
fn a_field_leaving_its_parent_carries_what_it_inherited() {
    let mut s = form();
    let clip = FieldClip::from_bytes(&s.copy_field("grp.a").unwrap().to_bytes()).unwrap();
    // `/Q` is the kid's own, so the form's `/Q 2` must not replace it.
    assert_eq!(
        clip.inherited_keys(),
        ["FT", "Ff", "V", "DV", "MaxLen", "DA"]
    );
    let materialized = paste(&mut s, &clip, true);
    assert_eq!(materialized, ["FT", "Ff", "V", "DV", "MaxLen", "DA"]);
    let (field, dict) = saved_field(&s);
    assert!(dict.get(b"Parent").is_none(), "pasted at the top level");
    assert_eq!(field.fully_qualified_name, "solo");
    for (key, want) in [
        (&b"Ff"[..], Object::Integer(4096)),
        (b"MaxLen", Object::Integer(9)),
        (b"Q", Object::Integer(1)),
        (b"V", Object::String(b"inherited value".to_vec())),
        (b"DV", Object::String(b"dflt".to_vec())),
        (b"DA", Object::String(b"/Helv 9 Tf 0 0 1 rg".to_vec())),
        (b"XXpdfcerKeep", Object::String(b"kept".to_vec())),
    ] {
        assert_eq!(
            dict.get(key),
            Some(&want),
            "{}",
            String::from_utf8_lossy(key)
        );
    }
    assert!(dict.get(b"StructParent").is_none());
    assert_ne!(dict.get(b"NM"), Some(&Object::String(b"n1".to_vec())));
    assert!(
        dict.get(b"NM").is_some(),
        "a fresh /NM replaces the source's"
    );
}

#[test]
fn a_declined_value_is_not_materialized_from_the_parent() {
    let mut s = form();
    let clip = s.copy_field("grp.a").unwrap();
    let materialized = paste(&mut s, &clip, false);
    assert_eq!(materialized, ["FT", "Ff", "DV", "MaxLen", "DA"]);
    let (_, dict) = saved_field(&s);
    assert!(dict.get(b"V").is_none());
    assert_eq!(dict.get(b"DV"), Some(&Object::String(b"dflt".to_vec())));
}
