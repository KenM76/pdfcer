//! Signature fields (ISO 32000-1 §12.7.4.5): an unsigned one with no `/AP`
//! draws its `/MK` placeholder, a signed one draws its signature
//! dictionary's `/Name` and `/M` only (§12.8.1 Table 252) and never a
//! validity claim, one with an `/AP` paints that `/AP`, and the build API's
//! output stays inside its rectangle.

use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::widget_draw::{
    APPEARANCE_MAX_SIZE, APPEARANCE_MIN_SIZE, SignatureAppearanceBuilder, SignatureAppearanceError,
    SignatureImage, SynthesizedKind, WidgetAppearances,
};
use pdfcer_render::{RenderOptions, RenderedPage, WidgetsWithoutAp, render_page_with_view};

/// The widget's `/Rect`, page space on a 300 pt page.
const RECT: [u32; 4] = [40, 200, 240, 260];
/// The widget's object number in every fixture.
const WIDGET: u32 = 4;

/// A one-page file whose `/Annots` is `4 0 R`; `bodies` are objects 4 on.
fn pdf(bodies: &[Vec<u8>]) -> Vec<u8> {
    pdf_form("", bodies)
}

/// [`pdf`] with `acroform` entries added to the `/AcroForm`.
fn pdf_form(acroform: &str, bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut all: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] {acroform} >> >>")
            .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
          /Annots [4 0 R] >>"
            .to_vec(),
    ];
    all.extend_from_slice(bodies);
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in all.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = all.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A signature widget with `extra` entries (`/V`, `/AP`, `/MK`, ...).
fn sig_widget(extra: &str) -> Vec<u8> {
    let [x0, y0, x1, y1] = RECT;
    format!(
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (sig) /F 4 /Rect [{x0} {y0} {x1} {y1}] \
         /P 3 0 R {extra} >>"
    )
    .into_bytes()
}

/// A signed field (`/V 5 0 R`) whose signature dictionary carries `/Name`
/// and `/M` and a `/Reason` the view-time look must not draw.
fn signed_pdf() -> Vec<u8> {
    pdf(&[
        sig_widget("/V 5 0 R /MK << /BC [0 0 0] >>"),
        b"<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
          /Name (A. Tester) /M (D:20261111120000Z) /Reason (Approved) \
          /ByteRange [0 0 0 0] /Contents <00> >>"
            .to_vec(),
    ])
}

fn render(bytes: Vec<u8>, mode: WidgetsWithoutAp) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let options = RenderOptions::default().with_widgets_without_ap(mode);
    render_page_with_view(&doc.view(), &pages[0], 1.0, &options).expect("the page renders")
}

/// Non-paper pixels in page box `[x0 y0 x1 y1]` (scale 1: row = 300 - y),
/// as `(col, row, r, g, b)`.
fn ink_in(page: &RenderedPage, [x0, y0, x1, y1]: [u32; 4]) -> Vec<(u32, u32, u8, u8, u8)> {
    let w = page.pixmap.width();
    let mut out = Vec::new();
    for row in (300 - y1)..(300 - y0) {
        for col in x0..x1 {
            let p = page.pixmap.pixels()[(row * w + col) as usize];
            if p.alpha() > 0 && (p.red(), p.green(), p.blue()) != (255, 255, 255) {
                out.push((col, row, p.red(), p.green(), p.blue()));
            }
        }
    }
    out
}

/// The view-time appearance core synthesizes for the fixture's widget.
fn synthesized(bytes: Vec<u8>) -> pdfcer_core::widget_draw::SynthesizedAppearance {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let view = doc.view();
    WidgetAppearances::read(&view)
        .expect("a form")
        .appearance(&view, ObjId::new(WIDGET, 0))
        .expect("a signature widget is drawn")
}

/// Every `(...) Tj` string in `content`, unescaped only for `\(`/`\)`.
fn shown_strings(content: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(content);
    let mut out = Vec::new();
    let mut rest = text.as_ref();
    while let Some(start) = rest.find('(') {
        let tail = &rest[start + 1..];
        let mut s = String::new();
        let mut chars = tail.char_indices();
        let mut end = tail.len();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = chars.next() {
                        s.push(n);
                    }
                }
                ')' => {
                    end = i;
                    break;
                }
                _ => s.push(c),
            }
        }
        out.push(s);
        rest = &tail[(end + 1).min(tail.len())..];
    }
    out
}

/// (a) An unsigned field with no `/AP` and `/MK /BC [0 0 1]` draws a blue
/// border and the grey "sign here" marker, and writes no text.
#[test]
fn an_unsigned_field_draws_its_mk_border_and_the_marker() {
    let bytes = pdf(&[sig_widget("/MK << /BC [0 0 1] >>")]);
    let drawn = synthesized(bytes.clone());
    assert_eq!(drawn.kind, SynthesizedKind::SignaturePlaceholder);
    assert!(!drawn.content.windows(2).any(|w| w == b"BT"), "no text");
    let out = render(bytes, WidgetsWithoutAp::Draw);
    assert_eq!(out.diagnostics.annotations_widget_synthesized, 1);
    let [x0, y0, x1, y1] = RECT;
    let border = ink_in(&out, [x0, y1 - 2, x1, y1]);
    assert!(!border.is_empty(), "no top border");
    assert!(
        border.iter().all(|&(.., r, g, b)| b > r && b > g),
        "the border is not blue: {:?}",
        &border[..border.len().min(4)]
    );
    let marker = ink_in(&out, [x0 + 4, y0 + 4, x1 - 4, y1 - 4]);
    assert!(marker.len() > 20, "no marker: {}", marker.len());
    assert!(
        marker
            .iter()
            .all(|&(.., r, g, b)| r.abs_diff(g) < 8 && g.abs_diff(b) < 8),
        "the marker is not neutral grey"
    );
    assert!(
        out.diagnostics
            .annotation_notes
            .iter()
            .any(|n| n.contains("unsigned signature field's placeholder")),
        "{:?}",
        out.diagnostics.annotation_notes
    );
}

/// (b) A signed field with no `/AP` draws `/Name` and the humanised `/M`,
/// and no other words: not the `/Reason`, not a validity claim.
#[test]
fn a_signed_field_draws_its_name_and_date_and_nothing_else() {
    let drawn = synthesized(signed_pdf());
    assert_eq!(drawn.kind, SynthesizedKind::Signature);
    let shown = shown_strings(&drawn.content);
    assert_eq!(shown, ["A. Tester", "2026.11.11 12:00:00 Z"]);
    assert_eq!(drawn.notes.signature_text, shown);
    assert!(drawn.notes.signature_omitted.is_empty());
    let out = render(signed_pdf(), WidgetsWithoutAp::Draw);
    let [x0, y0, x1, y1] = RECT;
    assert!(ink_in(&out, [x0 + 3, y0 + 3, x1 - 3, y1 - 3]).len() > 30);
    let note = out
        .diagnostics
        .annotation_notes
        .iter()
        .find(|n| n.contains("signature dictionary"))
        .expect("a disclosure note");
    assert!(note.contains("A. Tester | 2026.11.11 12:00:00 Z"), "{note}");
    assert!(note.contains("no validity is claimed"), "{note}");
}

/// A signed field without `/Name` draws the date and says what it left out.
#[test]
fn a_missing_name_is_disclosed() {
    let bytes = pdf(&[
        sig_widget("/V 5 0 R"),
        b"<< /Type /Sig /M (D:20261111120000Z) /Contents <00> >>".to_vec(),
    ]);
    let drawn = synthesized(bytes);
    assert_eq!(shown_strings(&drawn.content), ["2026.11.11 12:00:00 Z"]);
    assert!(
        drawn
            .notes
            .signature_omitted
            .iter()
            .any(|n| n.starts_with("/Name")),
        "{:?}",
        drawn.notes.signature_omitted
    );
}

/// The builder fixture: the appearance as `/AP /N` (object 5) with a 2x2 red
/// image (object 6). The form's `/BBox` is 40 pt wider than `[0 0 w h]` on
/// every side so ink the content drew outside its rectangle would show.
fn built_pdf(content: &[u8], w: f64, h: f64) -> Vec<u8> {
    let (ox, oy) = (60.0, 100.0);
    let m = 40.0;
    let rect = format!("{} {} {} {}", ox - m, oy - m, ox + w + m, oy + h + m);
    let widget = format!(
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (sig) /F 4 /Rect [{rect}] /P 3 0 R \
         /AP << /N 5 0 R >> >>"
    );
    let form = format!(
        "/Type /XObject /Subtype /Form /BBox [{} {} {} {}] /Resources << /Font << /Helv \
         << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> >> \
         /XObject << /Im0 6 0 R >> >>",
        -m,
        -m,
        w + m,
        h + m
    );
    pdf(&[
        widget.into_bytes(),
        stream(&form, content),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8",
            &[255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0],
        ),
    ])
}

/// (c) The build API draws the image and keeps every mark inside the rect.
#[test]
fn the_builder_uses_the_image_and_stays_inside_the_rect() {
    let (w, h) = (200.0, 60.0);
    let built = SignatureAppearanceBuilder::new("A. Tester", "D:20261111120000Z")
        .reason("Approved")
        .image(SignatureImage::new(ObjId::new(6, 0), 2, 2))
        .build(w, h)
        .expect("fits");
    assert!(built.image_drawn);
    assert!(built.dropped.is_empty());
    assert_eq!(
        built.lines,
        ["A. Tester", "2026.11.11 12:00:00 Z", "Reason: Approved"]
    );
    let text = String::from_utf8_lossy(&built.content);
    assert!(text.contains("/Im0 Do"), "{text}");
    let out = render(built_pdf(&built.content, w, h), WidgetsWithoutAp::Draw);
    // The form origin sits at page (60, 100): ink only in [60 100 260 160].
    let all = ink_in(&out, [0, 0, 300, 300]);
    assert!(!all.is_empty());
    let (c0, c1, r0, r1) = (60, 260, 300 - 160, 300 - 100);
    let outside: Vec<_> = all
        .iter()
        .filter(|&&(c, r, ..)| c < c0 || c >= c1 || r < r0 || r >= r1)
        .collect();
    assert!(outside.is_empty(), "ink outside the rect: {outside:?}");
    let red = all
        .iter()
        .filter(|&&(c, .., r, g, b)| c < 160 && r > 200 && g < 60 && b < 60)
        .count();
    assert!(red > 500, "the image was not drawn in the left half: {red}");
    let dark_right = all
        .iter()
        .filter(|&&(c, r, ..)| c > 165 && r > r0 + 2 && r < r1 - 2)
        .count();
    assert!(dark_right > 30, "no text in the right half: {dark_right}");
}

/// (c) A small rect auto-sizes down, drops the optional lines it must and
/// reports both; one too small for the name and date at the floor is
/// refused, never overflowed.
#[test]
fn a_small_rect_auto_sizes_to_the_floor_then_refuses() {
    let builder = SignatureAppearanceBuilder::new("A. Tester", "D:20261111120000Z")
        .reason("Approved")
        .location("Toronto");
    let tight = builder.build(120.0, 14.0).expect("name and date fit");
    assert!(tight.auto_sized);
    let size = tight.font_size.expect("text");
    assert!(
        (APPEARANCE_MIN_SIZE..APPEARANCE_MAX_SIZE).contains(&size),
        "{size}"
    );
    assert_eq!(tight.dropped, ["Reason: Approved", "Location: Toronto"]);
    assert_eq!(tight.lines.len(), 2);
    match builder.build(30.0, 8.0) {
        Err(SignatureAppearanceError::DoesNotFit {
            lines, min_size, ..
        }) => {
            assert_eq!(lines, 2);
            assert!((min_size - APPEARANCE_MIN_SIZE).abs() < f64::EPSILON);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// (d) A signature field that has an `/AP` paints it unchanged, even under
/// `/NeedAppearances true`: the same pixels whether or not widgets without
/// one are drawn, nothing synthesized, only the stored art on the page.
#[test]
fn an_existing_ap_renders_unchanged() {
    let [x0, y0, x1, y1] = RECT;
    let (w, h) = (x1 - x0, y1 - y0);
    let bytes = pdf_form(
        "/NeedAppearances true",
        &[
            sig_widget("/V 5 0 R /AP << /N 6 0 R >> /MK << /BC [0 0 1] >>"),
            b"<< /Type /Sig /Name (A. Tester) /M (D:20261111120000Z) /Contents <00> >>".to_vec(),
            stream(
                &format!("/Type /XObject /Subtype /Form /BBox [0 0 {w} {h}]"),
                b"0 1 0 rg 0 0 20 20 re f",
            ),
        ],
    );
    let drawn = render(bytes.clone(), WidgetsWithoutAp::Draw);
    let blank = render(bytes, WidgetsWithoutAp::Blank);
    assert_eq!(drawn.diagnostics.annotations_widget_synthesized, 0);
    assert_eq!(drawn.pixmap.data(), blank.pixmap.data());
    let ink = ink_in(&drawn, RECT);
    assert!(!ink.is_empty());
    assert!(
        ink.iter().all(|&(.., r, g, b)| g > r && g > b),
        "only the stored green square: {:?}",
        &ink[..ink.len().min(4)]
    );
}
