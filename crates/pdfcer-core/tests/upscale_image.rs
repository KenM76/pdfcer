//! Enlarging a placed image in place (pdfcer-gui request G198): the samples
//! grow by the factor, the colour space, `/Decode` and bit depth stay, and
//! the placement keeps its size on the page.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::ocr::upscale::{Upscale, UpscaleFilter};
use pdfcer_core::vector::{Matrix, VectorObject, decompose_page};
use pdfcer_core::writer::SaveOptions;

const SCAN: ObjId = ObjId {
    num: 5,
    generation: 0,
};

/// Objects: 0 the scan (5 0 R) at 200x100 pt, 1 an inline image, 2 a path.
/// `scan_dict` and `samples` describe the `w x h` scan.
fn fixture(w: u32, h: u32, scan_dict: &str, samples: &[u8]) -> Vec<u8> {
    let page = "q 200 0 0 100 10 10 cm /Scan Do Q\n\
                q 10 0 0 10 0 60 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q\n\
                0 0 m 10 10 l S\n";
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
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /XObject \
          << /Scan 5 0 R >> >> /Contents 4 0 R >>",
    );
    obj(
        &mut buf,
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()).as_bytes(),
    );
    let mut scan = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} {scan_dict} /Length {} >>\nstream\n",
        samples.len()
    )
    .into_bytes();
    scan.extend_from_slice(samples);
    scan.extend_from_slice(b"\nendstream");
    obj(&mut buf, &scan);
    obj(&mut buf, b"<< >>");
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

fn session(w: u32, h: u32, dict: &str, samples: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(fixture(w, h, dict, samples)).unwrap())
}

fn twice() -> Upscale {
    Upscale::new(2.0, UpscaleFilter::Bicubic).unwrap()
}

/// The saved scan placement's matrix and its image's dictionary and samples.
fn saved_scan(s: &EditSession) -> (Matrix, Dict, Vec<u8>) {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let objects = decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects;
    let VectorObject::Image(img) = &objects[0] else {
        panic!("object 0 is not an image");
    };
    let image_ref = Object::Reference(img.xobject.unwrap());
    let Object::Stream(image) = doc.resolve(&image_ref) else {
        panic!("no image stream");
    };
    let raw = image.data_span.slice(doc.bytes()).unwrap();
    let samples = pdfcer_core::filters::decode_stream(&image.dict, raw).unwrap();
    (img.ctm, image.dict.clone(), samples)
}

fn get<'a>(dict: &'a Dict, key: &[u8]) -> Option<&'a Object> {
    dict.get(key)
}

#[test]
fn an_rgb_scan_doubles_its_pixels_and_keeps_its_place() {
    // 4x2 RGB, a red half and a blue half.
    let samples: Vec<u8> = [[255u8, 0, 0], [255, 0, 0], [0, 0, 255], [0, 0, 255]]
        .concat()
        .repeat(2);
    let mut s = session(
        4,
        2,
        "/BitsPerComponent 8 /ColorSpace /DeviceRGB /Decode [0 1 0 1 0 1]",
        &samples,
    );
    let (before, ..) = saved_scan(&s);
    let depth = s.undo_kinds().len();
    let out = s.upscale_image(0, 0, twice()).unwrap();
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!((out.old_size, out.new_size), ((4, 2), (8, 4)));
    assert_eq!(out.replaced, SCAN);
    assert_eq!(out.old_stream_bytes, samples.len());
    assert!(out.new_stream_bytes > 0);
    assert!(out.notes[0].contains("4x2 to 8x4"), "{:?}", out.notes);

    let (ctm, dict, pixels) = saved_scan(&s);
    assert_eq!(ctm, before, "the placement keeps its size and place");
    assert_eq!(get(&dict, b"Width"), Some(&Object::Integer(8)));
    assert_eq!(get(&dict, b"Height"), Some(&Object::Integer(4)));
    assert_eq!(get(&dict, b"BitsPerComponent"), Some(&Object::Integer(8)));
    assert!(get(&dict, b"Decode").is_some(), "/Decode is kept");
    assert_eq!(pixels.len(), 8 * 4 * 3);
    assert_eq!(&pixels[..3], &[255, 0, 0], "the red half stays red");
    assert_eq!(&pixels[21..24], &[0, 0, 255], "the blue half stays blue");

    assert_eq!(s.undo(), Some(CommandKind::UpscaleImage));
    assert_eq!(saved_scan(&s).2, samples, "undo restores the original");
}

#[test]
fn a_bilevel_scan_stays_one_bit() {
    // 16x2 at 1 bpc: black left, white right.
    let mut s = session(
        16,
        2,
        "/BitsPerComponent 1 /ColorSpace /DeviceGray",
        &[0x00, 0xFF, 0x00, 0xFF],
    );
    let out = s.upscale_image(0, 0, twice()).unwrap();
    assert_eq!(out.new_size, (32, 4));
    let (_, dict, pixels) = saved_scan(&s);
    assert_eq!(get(&dict, b"BitsPerComponent"), Some(&Object::Integer(1)));
    assert_eq!(pixels, [0x00u8, 0x00, 0xFF, 0xFF].repeat(4));
}

#[test]
fn what_cannot_be_enlarged_is_refused_and_changes_nothing() {
    let mut s = session(2, 1, "/BitsPerComponent 8 /ColorSpace /DeviceGray", &[0, 9]);
    let depth = s.undo_kinds().len();
    let reason = |r: Result<_, EditError>| match r {
        Err(EditError::ImageUpscaleUnsupported { reason }) => reason,
        other => panic!("{other:?}"),
    };
    assert!(reason(s.upscale_image(0, 1, twice())).contains("inline"));
    let one = Upscale::new(1.0, UpscaleFilter::Bicubic).unwrap();
    assert!(reason(s.upscale_image(0, 0, one)).contains("factor of 1"));
    assert!(matches!(
        s.upscale_image(0, 2, twice()),
        Err(EditError::ReplaceImageOnOther { kind: "path", .. })
    ));
    assert_eq!(s.undo_kinds().len(), depth);

    // 8192x8192 doubled is 2^28 pixels, over the 2^27 ceiling.
    let mut big = session(
        8192,
        8192,
        "/BitsPerComponent 1 /ColorSpace /DeviceGray",
        &vec![0u8; 8192 * 8192 / 8],
    );
    assert!(reason(big.upscale_image(0, 0, twice())).contains("would exceed"));

    let mut masked = session(
        2,
        1,
        "/BitsPerComponent 8 /ColorSpace /DeviceGray /SMask 6 0 R",
        &[0, 9],
    );
    assert!(reason(masked.upscale_image(0, 0, twice())).contains("/SMask"));
}

#[test]
fn a_run_over_pages_folds_into_one_undo() {
    let mut s = session(2, 1, "/BitsPerComponent 8 /ColorSpace /DeviceGray", &[0, 9]);
    s.upscale_image(0, 0, twice()).unwrap();
    s.upscale_image(0, 0, twice()).unwrap();
    assert!(s.coalesce_last_same(2, CommandKind::UpscaleImage));
    assert_eq!(
        s.undo_kinds().collect::<Vec<_>>(),
        [CommandKind::UpscaleImage]
    );
}
