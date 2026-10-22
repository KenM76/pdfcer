#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn bits_are_msb_first_with_cid_zero_in_bit_seven() {
    assert_eq!(bits(&[0].into()), [0x80]);
    assert_eq!(bits(&[7, 8].into()), [0x01, 0x80]);
    assert_eq!(bits(&[0, 15].into()), [0x80, 0x01]);
}

#[test]
fn a_charset_names_each_glyph_once_in_order_without_notdef() {
    let names = [b"b".to_vec(), b".notdef".to_vec(), b"a.sc".to_vec()];
    assert_eq!(char_set(&names.into()).unwrap(), b"/a.sc/b");
}

#[test]
fn a_name_a_charset_cannot_carry_is_refused() {
    for bad in [&b""[..], b"a b", b"a/b", b"a%", b"\xC9"] {
        let err = char_set(&[bad.to_vec()].into()).unwrap_err();
        assert_eq!(err.token, "pdfa1-list-owed", "{bad:?}");
    }
}
