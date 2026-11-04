#![allow(clippy::unwrap_used)] // a panic here is the test failing

use super::*;

#[test]
fn each_standard_key_maps_to_its_xmp_property() {
    let name = |key: &[u8]| property_for(key).unwrap().qualified();
    assert_eq!(name(b"Title"), "dc:title");
    assert_eq!(name(b"Author"), "dc:creator");
    assert_eq!(name(b"Subject"), "dc:description");
    assert_eq!(name(b"Keywords"), "pdf:Keywords");
    assert_eq!(name(b"Creator"), "xmp:CreatorTool");
    assert_eq!(name(b"Dept"), "pdfx:Dept");
    assert!(
        property_for(b"\xFF").is_none(),
        "a non-UTF-8 key has no XMP name"
    );
}

#[test]
fn only_the_four_machine_set_keys_are_reserved() {
    for key in [&b"Producer"[..], b"CreationDate", b"ModDate", b"Trapped"] {
        assert!(reserved(key).is_some());
    }
    for field in InfoField::all() {
        assert!(reserved(field.key()).is_none());
    }
}
