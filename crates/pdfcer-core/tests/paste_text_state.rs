//! Copied text keeps the colour and text state it inherited.
//!
//! Colour, `Tc`, `Tw`, `Tz`, `TL`, `Ts` and `Tr` are graphics state, so a
//! producer may set them outside the `BT`…`ET` that relies on them. A text
//! object's bytes are its `BT`…`ET`, so a copy must carry that state beside
//! them or the paste paints in whatever its destination left in force.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{DevicePaintSpace, Matrix, PathPaint, TextObject, VectorObject};
use pdfcer_core::writer::SaveOptions;

const SPOT: &str = "/ColorSpace << /CS0 [/Separation /Spot /DeviceRGB << /FunctionType 2 \
                    /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] >>";

/// A one-page file whose page draws `page` and then form `/Fm0`, whose
/// content is `form`. Page and form share one font and one spot space.
fn pdf(page: &str, form: &str) -> Vec<u8> {
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}\nendstream",
            body.len() + 1
        )
    };
    let resources =
        format!("/Resources << /Font << /F1 6 0 R >> {SPOT} /XObject << /Fm0 5 0 R >> >>");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {resources} /Contents 4 0 R >>"
        ),
        stream("", &format!("{page}\nq 1 0 0 1 100 100 cm /Fm0 Do Q")),
        stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 80 80] /Resources << /Font << /F1 6 0 R \
                 >> {SPOT} >>"
            ),
            form,
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
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

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

/// A destination whose own content leaves green, wide-spaced, outlined text
/// state in force: everything a paste must not pick up.
fn destination() -> EditSession {
    session(pdf(
        "0 1 0 rg 0 1 0 RG 9 Tc 2 Tr 0 0 5 5 re f",
        "0 0 5 5 re f",
    ))
}

/// Pastes `clip` into [`destination`], saves, reopens, and returns the saved
/// text and the pasted text object as the reopened page reads it.
fn pasted(clip: &pdfcer_core::vector::ObjectClip) -> (String, TextObject) {
    let mut dest = destination();
    dest.paste_objects(0, clip, Matrix::IDENTITY).unwrap();
    let bytes = dest
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut reopened = session(bytes);
    let model = reopened.page_objects(0).unwrap();
    let found = model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t.clone()),
            _ => None,
        })
        .expect("the pasted text is on the page");
    (text, found)
}

fn device(paint: &PathPaint) -> (DevicePaintSpace, Vec<f64>) {
    match paint {
        PathPaint::Device { space, comps, .. } => (*space, comps.clone()),
        other => panic!("not a device colour: {other:?}"),
    }
}

#[test]
fn text_copied_from_a_page_keeps_its_inherited_colour_and_text_state() {
    let source = session(pdf(
        "1 0 0 rg 0 0 1 RG 2 Tc 3 Tw 50 Tz 4 Ts 1 Tr\nBT /F1 12 Tf 20 20 Td (hi) Tj ET",
        "0 0 5 5 re f",
    ));
    let clip = source.copy_objects(0, &[0]).unwrap();
    let (_, text) = pasted(&clip);
    assert_eq!(
        device(&text.inherited.fill),
        (DevicePaintSpace::Rgb, vec![1.0, 0.0, 0.0])
    );
    assert_eq!(
        device(&text.inherited.stroke),
        (DevicePaintSpace::Rgb, vec![0.0, 0.0, 1.0])
    );
    let state = text.inherited.text_state;
    assert_eq!(
        (state.char_spacing, state.word_spacing, state.h_scale),
        (2.0, 3.0, 0.5)
    );
    assert_eq!((state.rise, state.render_mode), (4.0, 1));
}

#[test]
fn text_that_set_nothing_pastes_black_and_unspaced_not_as_its_destination() {
    let source = session(pdf("BT /F1 12 Tf 20 20 Td (hi) Tj ET", "0 0 5 5 re f"));
    let clip = source.copy_objects(0, &[0]).unwrap();
    let (_, text) = pasted(&clip);
    assert_eq!(
        device(&text.inherited.fill),
        (DevicePaintSpace::Gray, vec![0.0])
    );
    assert_eq!(
        device(&text.inherited.stroke),
        (DevicePaintSpace::Gray, vec![0.0])
    );
}

#[test]
fn a_cmyk_colour_arrives_as_cmyk() {
    let source = session(pdf(
        "0 1 1 0 k\nBT /F1 12 Tf 20 20 Td (hi) Tj ET",
        "0 0 5 5 re f",
    ));
    let clip = source.copy_objects(0, &[0]).unwrap();
    let (_, text) = pasted(&clip);
    assert_eq!(
        device(&text.inherited.fill),
        (DevicePaintSpace::Cmyk, vec![0.0, 1.0, 1.0, 0.0])
    );
}

#[test]
fn a_spot_colour_travels_with_its_colour_space() {
    let source = session(pdf(
        "/CS0 cs 0.5 scn\nBT /F1 12 Tf 20 20 Td (hi) Tj ET",
        "0 0 5 5 re f",
    ));
    let clip = source.copy_objects(0, &[0]).unwrap();
    assert!(
        clip.items[0]
            .bindings
            .iter()
            .any(|b| b.category == b"ColorSpace"),
        "{:?}",
        clip.items[0].bindings
    );
    let (saved, text) = pasted(&clip);
    match &text.inherited.fill {
        PathPaint::Other {
            space: Some(name),
            comps,
            pattern: false,
        } => {
            assert_eq!(comps, &vec![0.5]);
            let bound = format!("/{} ", String::from_utf8_lossy(name));
            assert!(saved.contains(&bound), "{bound:?} is not bound: {saved}");
        }
        other => panic!("the spot colour did not arrive: {other:?}"),
    }
}

#[test]
fn a_colour_set_part_way_through_does_not_hide_the_inherited_one() {
    let source = session(pdf(
        "1 0 0 rg\nBT /F1 12 Tf 20 20 Td (a) Tj 0 1 0 rg (b) Tj ET",
        "0 0 5 5 re f",
    ));
    let clip = source.copy_objects(0, &[0]).unwrap();
    let prelude = String::from_utf8_lossy(&clip.items[0].prelude).into_owned();
    assert!(prelude.contains("1 0 0 rg"), "{prelude:?}");
}

#[test]
fn text_copied_from_inside_a_form_keeps_its_inherited_state_too() {
    let mut source = session(pdf(
        "0 0 5 5 re f",
        "0 0 1 rg 2 Tr 1.5 Tc\nBT /F1 6 Tf 0 14 Td (A) Tj ET",
    ));
    let leaf = source
        .page_objects(0)
        .unwrap()
        .leaves
        .iter()
        .position(|l| matches!(l.object, VectorObject::Text(_)))
        .expect("the form's text is a leaf");
    let clip = source.copy_objects_in_form(0, &[leaf]).unwrap();
    let (_, text) = pasted(&clip);
    assert_eq!(
        device(&text.inherited.fill),
        (DevicePaintSpace::Rgb, vec![0.0, 0.0, 1.0])
    );
    let state = text.inherited.text_state;
    assert_eq!((state.char_spacing, state.render_mode), (1.5, 2));
}
