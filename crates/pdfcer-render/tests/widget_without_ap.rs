//! Form-field widgets with no `/AP` are drawn from their field data at view
//! time (§12.7.3.3: the reader "shall construct an appearance stream
//! dynamically at viewing time"), and nothing is written to the file.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{RenderOptions, RenderedPage, WidgetsWithoutAp, render_page_with_view};

/// The text field's `/Rect` (page space, 300 pt page).
const TEXT_RECT: [u32; 4] = [20, 220, 220, 260];

/// A one-page form: a text field, a check box, a radio button and a push
/// button, none with an `/AP`. The states are the buttons' `/AS`.
fn form_pdf(check_state: &str, radio_state: &str) -> Vec<u8> {
    let [x0, y0, x1, y1] = TEXT_RECT;
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R] \
         /NeedAppearances true /DA (/Helv 0 Tf 0 g) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R 7 0 R] >>"
            .to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (Hello) \
             /DA (/Helv 12 Tf 0 0 1 rg) /Rect [{x0} {y0} {x1} {y1}] /P 3 0 R \
             /MK << /BC [0 0 1] >> >>"
        ),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /V /{check_state} \
             /AS /{check_state} /Rect [20 150 50 180] /P 3 0 R /MK << /BC [0 0 0] >> >>"
        ),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 32768 /T (pick) /V /{radio_state} \
             /AS /{radio_state} /Rect [70 150 100 180] /P 3 0 R /MK << /BC [0 0 0] >> >>"
        ),
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (go) \
         /Rect [120 150 200 180] /P 3 0 R /MK << /BG [0.8 0.8 0.8] /CA (Go) >> >>"
            .to_owned(),
    ];
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

fn render(bytes: Vec<u8>, mode: WidgetsWithoutAp) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let options = RenderOptions::default().with_widgets_without_ap(mode);
    render_page_with_view(&doc.view(), &pages[0], 1.0, &options).expect("the page renders")
}

/// Non-paper pixels inside page-space box `[x0 y0 x1 y1]` (300 pt page,
/// scale 1, so row = 300 - y).
fn ink_in(page: &RenderedPage, [x0, y0, x1, y1]: [u32; 4]) -> Vec<(u32, u32)> {
    let w = page.pixmap.width();
    let mut out = Vec::new();
    for row in (300 - y1)..(300 - y0) {
        for col in x0..x1 {
            let p = page.pixmap.pixels()[(row * w + col) as usize];
            if p.alpha() > 0 && (p.red(), p.green(), p.blue()) != (255, 255, 255) {
                out.push((col, row));
            }
        }
    }
    out
}

#[test]
fn every_widget_kind_without_an_ap_is_drawn_and_counted() {
    let out = render(form_pdf("Yes", "On"), WidgetsWithoutAp::Draw);
    let d = &out.diagnostics;
    assert_eq!(d.annotations_widget_synthesized, 4, "{d:?}");
    assert_eq!(d.annotations_without_ap.get("Widget").copied(), Some(4));
    for rect in [
        TEXT_RECT,
        [20, 150, 50, 180],
        [70, 150, 100, 180],
        [120, 150, 200, 180],
    ] {
        assert!(!ink_in(&out, rect).is_empty(), "nothing drawn in {rect:?}");
    }
}

#[test]
fn the_text_value_is_drawn_in_its_da_colour_and_centred() {
    let out = render(form_pdf("Yes", "On"), WidgetsWithoutAp::Draw);
    // Inside the border: the glyphs only.
    let [x0, y0, x1, y1] = TEXT_RECT;
    let glyphs = ink_in(&out, [x0 + 3, y0 + 3, x1 - 3, y1 - 3]);
    assert!(
        glyphs.len() > 30,
        "the value was not drawn: {}",
        glyphs.len()
    );
    let w = out.pixmap.width();
    assert!(glyphs.iter().all(|&(c, r)| {
        let p = out.pixmap.pixels()[(r * w + c) as usize];
        p.blue() > p.red()
    }));
    let mean_row = glyphs.iter().map(|&(_, r)| f64::from(r)).sum::<f64>() / glyphs.len() as f64;
    let centre_row = f64::from(300 - (y0 + y1) / 2);
    assert!(
        (mean_row - centre_row).abs() < 4.0,
        "single-line text sits at row {mean_row}, box centre is {centre_row}"
    );
}

#[test]
fn a_check_box_draws_its_on_state_only_when_on() {
    let on = ink_in(
        &render(form_pdf("Yes", "On"), WidgetsWithoutAp::Draw),
        [22, 152, 48, 178],
    )
    .len();
    let off = ink_in(
        &render(form_pdf("Off", "On"), WidgetsWithoutAp::Draw),
        [22, 152, 48, 178],
    )
    .len();
    assert!(on > off + 20, "on={on} off={off}");
}

#[test]
fn blank_mode_paints_nothing_and_counts_nothing() {
    let out = render(form_pdf("Yes", "On"), WidgetsWithoutAp::Blank);
    assert_eq!(out.diagnostics.annotations_widget_synthesized, 0);
    assert_eq!(
        out.diagnostics
            .annotations_without_ap
            .get("Widget")
            .copied(),
        Some(4)
    );
    assert!(ink_in(&out, [0, 0, 300, 300]).is_empty());
}

#[test]
fn rendering_an_open_session_writes_nothing() {
    let original = form_pdf("Yes", "On");
    let session = EditSession::new(Document::from_bytes(original.clone()).expect("loads"));
    let pages = session.pages().expect("pages");
    let out = render_page_with_view(&session.view(), &pages[0], 1.0, &RenderOptions::default())
        .expect("renders");
    assert_eq!(out.diagnostics.annotations_widget_synthesized, 4);
    assert!(!session.is_modified());
    let (saved, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves");
    assert_eq!(saved, original);
    let form = pdfcer_core::forms::parse_acroform(&session.view()).expect("form");
    assert!(
        form.fields
            .iter()
            .flat_map(|f| &f.widgets)
            .all(|w| !w.has_normal_appearance)
    );
}

#[test]
fn a_radio_button_draws_its_on_state_only_when_on() {
    let inner = [72, 152, 98, 178];
    let on = ink_in(
        &render(form_pdf("Yes", "On"), WidgetsWithoutAp::Draw),
        inner,
    )
    .len();
    let off = ink_in(
        &render(form_pdf("Yes", "Off"), WidgetsWithoutAp::Draw),
        inner,
    )
    .len();
    assert!(on > off + 20, "on={on} off={off}");
}
