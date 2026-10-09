//! Skew detection and correction (pdfcer-gui request G165): a synthetic
//! skewed "scan" is measured, straightened into a new image XObject, and
//! measured again.

use pdfcer_core::deskew::{self, MAX_SKEW_DEGREES, detect_skew};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{Matrix, VectorObject, decompose_page};
use pdfcer_core::writer::SaveOptions;

const W: u32 = 1000;
const H: u32 = 700;
const PAPER: u8 = 230;

/// A page of "text lines": dark bars rising to the right by `angle` degrees
/// (counter-clockwise as displayed), on tinted paper. Row-major, top first.
fn skewed_grey(angle: f64) -> Vec<u8> {
    let tan = angle.to_radians().tan();
    let mut grey = vec![PAPER; (W * H) as usize];
    for y in 0..H {
        for x in 0..W {
            // The row this pixel would sit on before the skew.
            let y0 = f64::from(y) + f64::from(x) * tan;
            let in_line = (80.0..620.0).contains(&y0) && (y0 as u32 % 30) < 6;
            if in_line && (50..950).contains(&x) {
                grey[(y * W + x) as usize] = 10;
            }
        }
    }
    grey
}

/// Pack 8-bit grey into 1-bit samples: 1 = paper (white), rows padded.
fn to_bilevel(grey: &[u8]) -> Vec<u8> {
    let row = W.div_ceil(8) as usize;
    let mut out = vec![0u8; row * H as usize];
    for y in 0..H as usize {
        for x in 0..W as usize {
            if grey[y * W as usize + x] >= 128 {
                out[y * row + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// Objects: 0 a small image XObject, 1 a path, 2 the scan (5 0 R) filling
/// the page, 3 an inline image. `scan` is the dictionary body and samples of
/// object 5; object 6 is the small image.
fn fixture(scan_dict: &str, samples: &[u8]) -> Vec<u8> {
    let page = "q 20 0 0 20 5 5 cm /Small Do Q\n0 0 m 10 10 l S\n\
                q 1000 0 0 700 0 0 cm /Scan Do Q\n\
                q 10 0 0 10 0 60 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q\n";
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |buf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut buf, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut buf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut buf,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1000 700] /Resources << /XObject \
          << /Scan 5 0 R /Small 6 0 R >> >> /Contents 4 0 R >>",
    );
    obj(
        &mut buf,
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()).as_bytes(),
    );
    let mut scan = format!(
        "<< /Type /XObject /Subtype /Image /Width {W} /Height {H} {scan_dict} /Length {} >>\nstream\n",
        samples.len()
    )
    .into_bytes();
    scan.extend_from_slice(samples);
    scan.extend_from_slice(b"\nendstream");
    obj(&mut buf, &scan);
    obj(
        &mut buf,
        b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 \
          /ColorSpace /DeviceGray /Length 1 >>\nstream\nA\nendstream",
    );
    let xref_at = buf.len();
    let size = offsets.len() + 1;
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

const SCAN: ObjId = ObjId {
    num: 5,
    generation: 0,
};

fn grey_session(angle: f64) -> EditSession {
    let bytes = fixture(
        "/BitsPerComponent 8 /ColorSpace /DeviceGray",
        &skewed_grey(angle),
    );
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn reopened(s: &EditSession) -> (Document, Vec<VectorObject>) {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let objects = decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects;
    (doc, objects)
}

fn xobject_of(objs: &[VectorObject], i: usize) -> Option<ObjId> {
    match &objs[i] {
        VectorObject::Image(img) => img.xobject,
        other => panic!("object {i} is not an image: {other:?}"),
    }
}

fn measured(s: &mut EditSession) -> f64 {
    s.detect_image_skew(0, 2).unwrap().unwrap().angle_degrees
}

#[test]
fn a_raster_skew_is_measured_with_its_sign() {
    for angle in [2.0, -3.0, 0.0, 7.5] {
        let skew = detect_skew(W, H, &skewed_grey(angle)).unwrap();
        assert!(
            (skew.angle_degrees - angle).abs() < 0.1,
            "{angle}: {skew:?}"
        );
        assert!(skew.confidence > 0.5, "{angle}: {skew:?}");
    }
    assert!(detect_skew(W, H, &vec![PAPER; (W * H) as usize]).is_none());
    assert!(detect_skew(W, H, &[0; 10]).is_none(), "short buffer");
}

#[test]
fn the_scan_is_the_largest_image() {
    let mut s = grey_session(2.0);
    assert_eq!(s.page_scan_image(0).unwrap(), Some(2));
}

#[test]
fn a_skewed_scan_is_straightened_as_a_new_xobject() {
    let mut s = grey_session(2.0);
    assert!((measured(&mut s) - 2.0).abs() < 0.1);
    let depth = s.undo_kinds().len();
    let out = s.deskew_image(0, 2, 2.0).unwrap();
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!(out.replaced, SCAN);
    assert!(out.notes[0].contains("resampled"), "{:?}", out.notes);
    assert!(out.new_stream_bytes > 0);
    assert_eq!(out.old_stream_bytes, (W * H) as usize);

    let after = measured(&mut s);
    assert!(after.abs() < 0.1, "still skewed by {after}");

    let (doc, objects) = reopened(&s);
    assert_eq!(objects.len(), 4, "nothing added or removed");
    assert_eq!(xobject_of(&objects, 2), Some(out.image_id));
    let image_ref = Object::Reference(out.image_id);
    let Object::Stream(image) = doc.resolve(&image_ref) else {
        panic!("no image stream");
    };
    for (key, value) in [
        (&b"Width"[..], Object::Integer(i64::from(W))),
        (b"Height", Object::Integer(i64::from(H))),
        (b"BitsPerComponent", Object::Integer(8)),
    ] {
        assert_eq!(image.dict.get(key), Some(&value));
    }
    let raw = image.data_span.slice(doc.bytes()).unwrap();
    let samples = pdfcer_core::filters::decode_stream(&image.dict, raw).unwrap();
    assert_eq!(samples.len(), (W * H) as usize);
    // The uncovered corners take the paper tint, not white or black.
    for corner in [0, W as usize - 1, samples.len() - 1] {
        assert_eq!(samples[corner], PAPER, "corner {corner}");
    }

    let reopened_session = &mut EditSession::new(doc);
    assert!(measured(reopened_session).abs() < 0.1);

    assert_eq!(s.undo(), Some(CommandKind::DeskewImage));
    assert_eq!(xobject_of(&reopened(&s).1, 2), Some(SCAN));
}

#[test]
fn a_bilevel_scan_is_measured_and_straightened() {
    let bytes = fixture(
        "/BitsPerComponent 1 /ColorSpace /DeviceGray",
        &to_bilevel(&skewed_grey(-1.5)),
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!((measured(&mut s) + 1.5).abs() < 0.1);
    s.deskew_image(0, 2, -1.5).unwrap();
    let after = measured(&mut s);
    assert!(after.abs() < 0.1, "still skewed by {after}");
}

#[test]
fn an_image_mask_and_an_indexed_image_are_straightened() {
    // With an /ImageMask's default /Decode [0 1], a 0 sample paints (§8.9.6.2).
    let mask = to_bilevel(&skewed_grey(3.0));
    let indexed: Vec<u8> = skewed_grey(3.0)
        .iter()
        .map(|&g| if g < 128 { 1 } else { 0 })
        .collect();
    for (dict, samples) in [
        ("/ImageMask true", mask),
        (
            "/BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 1 <FFFFFF000000>]",
            indexed,
        ),
    ] {
        let mut s = EditSession::new(Document::from_bytes(fixture(dict, &samples)).unwrap());
        assert!((measured(&mut s) - 3.0).abs() < 0.1, "{dict}");
        s.deskew_image(0, 2, 3.0).unwrap();
        assert!(measured(&mut s).abs() < 0.1, "{dict}");
    }
}

fn reason(result: Result<impl std::fmt::Debug, EditError>) -> String {
    match result {
        Err(EditError::DeskewUnsupported { reason }) => reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn what_cannot_be_deskewed_is_refused_and_changes_nothing() {
    let mut s = grey_session(2.0);
    let depth = s.undo_kinds().len();
    assert!(reason(s.deskew_image(0, 3, 1.0)).contains("inline"));
    assert!(reason(s.detect_image_skew(0, 3)).contains("inline"));
    assert!(reason(s.deskew_image(0, 2, MAX_SKEW_DEGREES + 1.0)).contains("outside"));
    assert!(reason(s.deskew_image(0, 2, f64::NAN)).contains("outside"));
    assert!(reason(s.deskew_image(0, 2, 0.0)).contains("nothing to correct"));
    assert!(matches!(
        s.deskew_image(0, 1, 1.0),
        Err(EditError::ReplaceImageOnOther { kind: "path", .. })
    ));
    assert_eq!(s.undo_kinds().len(), depth);

    let bytes = fixture(
        "/BitsPerComponent 8 /ColorSpace /DeviceGray /SMask 6 0 R",
        &skewed_grey(2.0),
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(reason(s.deskew_image(0, 2, 2.0)).contains("/SMask"));
}

#[test]
fn a_blank_image_has_no_measurable_skew() {
    let bytes = fixture(
        "/BitsPerComponent 8 /ColorSpace /DeviceGray",
        &vec![PAPER; (W * H) as usize],
    );
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(s.detect_image_skew(0, 2).unwrap(), None);
}

#[test]
fn the_view_measures_what_the_session_measures() {
    let mut s = grey_session(2.0);
    let session_skew = s.detect_image_skew(0, 2).unwrap().unwrap();
    let doc = Document::from_bytes(fixture(
        "/BitsPerComponent 8 /ColorSpace /DeviceGray",
        &skewed_grey(2.0),
    ))
    .unwrap();
    for view in [doc.view(), s.view()] {
        assert_eq!(deskew::page_scan_image(&view, 0).unwrap(), Some(2));
        let skew = deskew::detect_image_skew(&view, 0, 2).unwrap().unwrap();
        assert_eq!(skew, session_skew);
    }
    // The session view reads the session's edits: after the correction it
    // measures the new, straight image.
    s.deskew_image(0, 2, 2.0).unwrap();
    let after = deskew::detect_image_skew(&s.view(), 0, 2).unwrap().unwrap();
    assert!(after.angle_degrees.abs() < 0.15, "{after:?}");
}

#[test]
fn the_view_refuses_what_the_session_refuses() {
    let s = grey_session(2.0);
    let view = s.view();
    let refusal =
        |r: Result<Option<deskew::SkewEstimate>, EditError>| format!("{:?}", r.unwrap_err());
    assert!(refusal(deskew::detect_image_skew(&view, 0, 3)).contains("inline image"));
    assert!(matches!(
        deskew::detect_image_skew(&view, 0, 1),
        Err(EditError::ReplaceImageOnOther { index: 1, .. })
    ));
    assert!(matches!(
        deskew::page_scan_image(&view, 4),
        Err(EditError::PageOutOfRange { index: 4, count: 1 })
    ));
}

#[test]
fn a_run_of_one_kind_folds_and_a_mixed_run_does_not() {
    let mut s = grey_session(2.0);
    s.deskew_image(0, 2, 2.0).unwrap();
    s.deskew_image(0, 2, 1.0).unwrap();
    let kinds: Vec<CommandKind> = s.undo_kinds().collect();
    assert!(!s.coalesce_last_same(2, CommandKind::ReplaceImage));
    assert!(
        !s.coalesce_last_same(3, CommandKind::DeskewImage),
        "too short"
    );
    assert_eq!(s.undo_kinds().collect::<Vec<_>>(), kinds, "unchanged");
    assert!(s.coalesce_last_same(2, CommandKind::DeskewImage));
    assert_eq!(
        s.undo_kinds().collect::<Vec<_>>(),
        [CommandKind::DeskewImage]
    );
    assert!(s.coalesce_last_same(0, CommandKind::ReplaceImage));
}
