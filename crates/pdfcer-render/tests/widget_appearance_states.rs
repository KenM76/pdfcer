//! Drawing a widget's rollover (`/R`) or down (`/D`) appearance on request
//! (ISO 32000-1 §12.5.5 Table 168): each state paints its own stream, a
//! missing state falls back to `/N` and is counted, and a push button with
//! no `/AP` is built pressed with its `/MK /AC` caption (Table 189).

use pdfcer_core::annot::AppearanceKind;
use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with_view};

/// A form XObject that fills `w` x `h` in `rgb`.
fn solid_form(rgb: &str, w: u32, h: u32) -> String {
    let content = format!("{rgb} rg 0 0 {w} {h} re f");
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 {w} {h}] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Serialise `bodies` as objects 1.. with a classic xref table.
fn pdf_of(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
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

/// One push button, object 4, on [20 100 220 160], with `widget_extra` in
/// its dictionary and `streams` as objects 5.. .
fn button_pdf(widget_extra: &str, streams: &[String]) -> Vec<u8> {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /DA (/Helv 0 Tf 0 g) \
         /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> /Annots [4 0 R] >>"
            .to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Widget /F 4 /FT /Btn /Ff 65536 /T (go) \
             /DA (/Helv 18 Tf 0 g) /Rect [20 100 220 160] /P 3 0 R {widget_extra} >>"
        ),
    ];
    bodies.extend_from_slice(streams);
    pdf_of(&bodies)
}

/// A push button whose `/N`, `/R` and `/D` are red, green and blue.
fn three_state_pdf() -> Vec<u8> {
    button_pdf(
        "/H /P /AP << /N 5 0 R /R 6 0 R /D 7 0 R >>",
        &[
            solid_form("1 0 0", 200, 60),
            solid_form("0 1 0", 200, 60),
            solid_form("0 0 1", 200, 60),
        ],
    )
}

fn render_with(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    render_page_with_view(&doc.view(), &pages[0], 1.0, options).expect("the page renders")
}

fn render_in(bytes: Vec<u8>, kind: AppearanceKind) -> RenderedPage {
    render_with(bytes, &RenderOptions::default().with_widget_state(kind))
}

/// The pixel covering page point (`x`, `y`) on a 300 pt page at scale 1.
fn px(page: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
    let w = page.pixmap.width();
    let p = page.pixmap.pixels()[((300 - y - 1) * w + x) as usize];
    (p.red(), p.green(), p.blue())
}

/// How many pixels in page box `[x0 y0 x1 y1]` satisfy `pred`.
fn count_in(
    page: &RenderedPage,
    [x0, y0, x1, y1]: [u32; 4],
    pred: impl Fn((u8, u8, u8)) -> bool,
) -> usize {
    let w = page.pixmap.width();
    let mut n = 0;
    for row in (300 - y1)..(300 - y0) {
        for col in x0..x1 {
            let p = page.pixmap.pixels()[(row * w + col) as usize];
            if pred((p.red(), p.green(), p.blue())) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn each_requested_state_paints_its_own_stream() {
    for (kind, want) in [
        (AppearanceKind::Normal, (255, 0, 0)),
        (AppearanceKind::Rollover, (0, 255, 0)),
        (AppearanceKind::Down, (0, 0, 255)),
    ] {
        let out = render_in(three_state_pdf(), kind);
        assert_eq!(px(&out, 120, 130), want, "{kind:?}");
        assert_eq!(out.diagnostics.annotations_state_fallback, 0, "{kind:?}");
        assert_eq!(out.diagnostics.annotations_painted, 1, "{kind:?}");
    }
}

#[test]
fn a_per_annotation_choice_overrides_the_widget_state() {
    let options = RenderOptions::default()
        .with_widget_state(AppearanceKind::Rollover)
        .with_appearance_states([(ObjId::new(4, 0), AppearanceKind::Down)]);
    let out = render_with(three_state_pdf(), &options);
    assert_eq!(px(&out, 120, 130), (0, 0, 255));
    let other =
        RenderOptions::default().with_appearance_states([(ObjId::new(9, 0), AppearanceKind::Down)]);
    let out = render_with(three_state_pdf(), &other);
    assert_eq!(
        px(&out, 120, 130),
        (255, 0, 0),
        "an entry for another id changes nothing"
    );
}

#[test]
fn a_missing_down_appearance_falls_back_to_normal_and_is_reported() {
    let bytes = || button_pdf("/AP << /N 5 0 R >>", &[solid_form("1 0 0", 200, 60)]);
    let out = render_in(bytes(), AppearanceKind::Down);
    assert_eq!(px(&out, 120, 130), (255, 0, 0));
    assert_eq!(out.diagnostics.annotations_state_fallback, 1);
    let notes = out.diagnostics.annotation_notes.join("\n");
    assert!(notes.contains("no /D appearance"), "{notes}");
    assert!(
        notes.contains("Table 188"),
        "absent /H is I, which overrides /D: {notes}"
    );
    let normal = render_in(bytes(), AppearanceKind::Normal);
    assert_eq!(normal.diagnostics.annotations_state_fallback, 0);
}

/// A beveled push button with no `/AP`: a blue plate, a 3 pt `/BS /S /B`
/// border, caption "Go" and down caption "Going".
fn beveled_pdf() -> Vec<u8> {
    button_pdf(
        "/MK << /CA (Go) /AC (Going) /BG [0 0 1] >> /BS << /S /B /W 3 >>",
        &[],
    )
}

fn grey(level: u8) -> impl Fn((u8, u8, u8)) -> bool {
    move |(r, g, b)| r.abs_diff(level) < 12 && g.abs_diff(level) < 12 && b.abs_diff(level) < 12
}

fn ink((r, g, b): (u8, u8, u8)) -> bool {
    r < 90 && g < 90 && b < 90
}

#[test]
fn a_push_button_without_ap_is_drawn_pressed_with_its_down_caption() {
    let normal = render_in(beveled_pdf(), AppearanceKind::Normal);
    let down = render_in(beveled_pdf(), AppearanceKind::Down);
    // The bevel band sits one border width inside the frame, 3..6 pt in.
    let top = [40, 155, 200, 157];
    let bottom = [40, 103, 200, 105];
    assert!(
        count_in(&normal, top, grey(255)) > 200,
        "raised: lit on top"
    );
    assert!(
        count_in(&normal, bottom, grey(128)) > 200,
        "raised: shaded below"
    );
    assert!(
        count_in(&down, top, grey(128)) > 200,
        "pressed: shaded on top"
    );
    assert!(
        count_in(&down, bottom, grey(255)) > 200,
        "pressed: lit below"
    );
    // "Going" puts more ink on the plate than "Go".
    let plate = [30, 110, 210, 150];
    let (go, going) = (count_in(&normal, plate, ink), count_in(&down, plate, ink));
    assert!(go > 20 && going > go + go / 2, "Go {go}, Going {going}");
    assert_eq!(down.diagnostics.annotations_state_fallback, 0);
    assert_eq!(down.diagnostics.annotations_widget_synthesized, 1);
}

#[test]
fn a_rollover_without_a_rollover_caption_shows_the_normal_caption() {
    let normal = render_in(beveled_pdf(), AppearanceKind::Normal);
    let rollover = render_in(beveled_pdf(), AppearanceKind::Rollover);
    assert_eq!(normal.pixmap.data(), rollover.pixmap.data());
}
