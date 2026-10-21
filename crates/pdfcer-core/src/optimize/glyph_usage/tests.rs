#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;

use super::*;
use crate::document::Document;
use crate::optimize::tests::{build, id};
use crate::space_audit::tests::stream;

/// 1 catalog, 2 pages, 3 page with `page_extra` drawing `content` (4),
/// 5 a TrueType font; `extra` appended from 6.
fn doc(page_extra: &str, content: &[u8], extra: &[Vec<u8>]) -> Document {
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R {page_extra} >>"
        )
        .into_bytes(),
        stream("", content),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /F >>".to_vec(),
    ];
    bodies.extend_from_slice(extra);
    Document::from_bytes(build(&bodies)).unwrap()
}

const RES: &str = "/Resources << /Font << /F1 5 0 R >> >>";

fn usage(d: &Document) -> GlyphUsage {
    enumerate(&d.view())
}

fn codes(u: &GlyphUsage, font: u32) -> Vec<u32> {
    u.codes
        .get(&id(font))
        .map(|c| c.iter().copied().collect())
        .unwrap_or_default()
}

#[test]
fn page_shows_record_one_byte_codes_from_every_show_operator() {
    let d = doc(
        RES,
        b"BT /F1 9 Tf (AB) Tj [(C) -20 (D)] TJ (E) ' 1 2 (F) \" ET",
        &[],
    );
    let u = usage(&d);
    assert!(u.faults.is_empty(), "{:?}", u.faults);
    assert_eq!(
        codes(&u, 5),
        b"ABCDEF".iter().map(|&b| u32::from(b)).collect::<Vec<_>>()
    );
    assert!(u.holders.contains(&id(3)) && u.holders.contains(&id(2)));
}

#[test]
fn a_form_inherits_the_font_in_force_and_q_restores_it() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 9 9]",
        b"BT (X) Tj ET",
    );
    let other = b"<< /Type /Font /Subtype /TrueType /BaseFont /O >>".to_vec();
    let res = "/Resources << /Font << /F1 5 0 R /F2 7 0 R >> /XObject << /Fm 6 0 R >> >>";
    let d = doc(
        res,
        b"BT /F1 9 Tf ET /Fm Do q BT /F2 9 Tf (Z) Tj ET Q (Y) Tj",
        &[form, other],
    );
    let u = usage(&d);
    assert_eq!(codes(&u, 5), vec![u32::from(b'X'), u32::from(b'Y')]);
    assert_eq!(codes(&u, 7), vec![u32::from(b'Z')]);
    assert!(u.holders.contains(&id(6)));
}

#[test]
fn a_show_with_no_font_is_a_fault() {
    let u = usage(&doc(RES, b"BT (Z) Tj ET", &[]));
    assert_eq!(u.faults.len(), 1, "{:?}", u.faults);
}

#[test]
fn a_tf_naming_a_missing_font_is_a_fault() {
    let u = usage(&doc(RES, b"BT /F9 9 Tf (Z) Tj ET", &[]));
    assert!(!u.faults.is_empty());
}

#[test]
fn identity_h_splits_two_byte_codes_and_another_cmap_is_unsplit() {
    let fonts = vec![
        b"<< /Type /Font /Subtype /Type0 /BaseFont /G /Encoding /Identity-H >>".to_vec(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /H /Encoding /UniGB-UCS2-H >>".to_vec(),
    ];
    let res = "/Resources << /Font << /G 6 0 R /H 7 0 R >> >>";
    let d = doc(
        res,
        b"BT /G 9 Tf <0102 0304> Tj /H 9 Tf <0001> Tj ET",
        &fonts,
    );
    let u = usage(&d);
    assert_eq!(codes(&u, 6), vec![0x0102, 0x0304]);
    assert_eq!(u.unsplit, BTreeSet::from([id(7)]));
}

#[test]
fn appearance_states_patterns_and_soft_masks_are_walked() {
    let shows = |c: u8| format!("BT /F1 9 Tf ({}) Tj ET", c as char).into_bytes();
    let form =
        "/Type /XObject /Subtype /Form /BBox [0 0 9 9] /Resources << /Font << /F1 5 0 R >> >>";
    let pat = "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 9 9] /XStep 9 /YStep 9 \
               /Resources << /Font << /F1 5 0 R >> >>";
    let extra = vec![
        b"<< /Type /Annot /Subtype /Widget /Rect [0 0 9 9] /AP << /N << /On 7 0 R /Off 8 0 R >> /D 9 0 R >> >>"
            .to_vec(),
        stream(form, &shows(b'a')),
        stream(form, &shows(b'b')),
        stream(form, &shows(b'c')),
        stream(pat, &shows(b'd')),
        stream(form, &shows(b'e')),
    ];
    let res = "/Annots [6 0 R] /Resources << /Pattern << /P 10 0 R >> \
               /ExtGState << /M << /SMask << /S /Alpha /G 11 0 R >> >> >> >>";
    let d = doc(res, b"/Pattern cs /P scn 0 0 9 9 re f /M gs", &extra);
    let u = usage(&d);
    assert!(u.faults.is_empty(), "{:?}", u.faults);
    assert_eq!(
        codes(&u, 5),
        b"abcde".iter().map(|&b| u32::from(b)).collect::<Vec<_>>()
    );
}

#[test]
fn type3_glyph_procedures_are_walked_in_the_font_resources() {
    let t3 = b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
              /CharProcs << /g 7 0 R >> /Encoding << /Differences [65 /g] >> \
              /FirstChar 65 /LastChar 65 /Widths [1] /Resources << /Font << /F1 5 0 R >> >> >>"
        .to_vec();
    let proc_ = stream("", b"1 0 d0 BT /F1 9 Tf (q) Tj ET");
    let res = "/Resources << /Font << /T 6 0 R >> >>";
    let u = usage(&doc(res, b"BT /T 9 Tf (A) Tj ET", &[t3, proc_]));
    assert!(u.faults.is_empty(), "{:?}", u.faults);
    assert_eq!(codes(&u, 5), vec![u32::from(b'q')]);
    assert!(u.holders.contains(&id(6)) && u.holders.contains(&id(7)));
}

#[test]
fn a_form_that_draws_itself_terminates() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 9 9] /Resources << /XObject << /Fm 6 0 R >> >>",
        b"/Fm Do",
    );
    let res = "/Resources << /XObject << /Fm 6 0 R >> >>";
    let u = usage(&doc(res, b"/Fm Do", &[form]));
    assert!(u.faults.is_empty(), "{:?}", u.faults);
}

#[test]
fn indirect_resources_and_font_dicts_are_holders() {
    let extra = vec![b"<< /Font 7 0 R >>".to_vec(), b"<< /F1 5 0 R >>".to_vec()];
    let u = usage(&doc("/Resources 6 0 R", b"BT /F1 9 Tf (A) Tj ET", &extra));
    assert!(u.holders.contains(&id(6)) && u.holders.contains(&id(7)));
    assert_eq!(codes(&u, 5), vec![u32::from(b'A')]);
}
