//! `/NeedAppearances` is honoured at display time (§12.7.2 Table 218): text
//! and choice widgets are drawn from `/V`, buttons keep their stored `/AP`,
//! and nothing is written. The field highlight is editor chrome behind
//! fillable widgets (Table 221 bits 1 and 2), off by default and never on a
//! print render.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{
    FieldHighlight, NeedAppearances, RenderOptions, RenderedPage, render_page_with_view,
};

/// A stored appearance that is a solid fill in `rgb` over `w` x `h`.
fn solid_form(rgb: &str, w: u32, h: u32) -> String {
    form_of(&format!("{rgb} rg 0 0 {w} {h} re f"), w, h)
}

/// A form XObject drawing `content` over `w` x `h`.
fn form_of(content: &str, w: u32, h: u32) -> String {
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

/// Text field [20 220 220 260], combo box [20 160 220 200] and check box
/// [20 100 50 130], each with a stale red-or-green stored `/AP`; the text
/// and choice values are "new" in blue.
fn stale_form_pdf(need_appearances: bool) -> Vec<u8> {
    let flag = if need_appearances {
        "/NeedAppearances true"
    } else {
        ""
    };
    pdf_of(&[
        format!(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R] {flag} \
             /DA (/Helv 0 Tf 0 g) >> >>"
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /F 4 /FT /Tx /T (name) /V (new) \
         /DA (/Helv 24 Tf 0 0 1 rg) /Rect [20 220 220 260] /P 3 0 R /AP << /N 7 0 R >> >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /F 4 /FT /Ch /Ff 131072 /T (pick) /Opt [(new)] \
         /V (new) /DA (/Helv 24 Tf 0 0 1 rg) /Rect [20 160 220 200] /P 3 0 R \
         /AP << /N 7 0 R >> >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /F 4 /FT /Btn /T (agree) /V /Yes /AS /Yes \
         /Rect [20 100 50 130] /P 3 0 R /AP << /N << /Yes 8 0 R /Off 8 0 R >> >> >>"
            .to_owned(),
        solid_form("1 0 0", 200, 40),
        solid_form("0 1 0", 30, 30),
    ])
}

fn render_with(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("the fixture loads");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    render_page_with_view(&doc.view(), &pages[0], 1.0, options).expect("the page renders")
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

fn red((r, g, b): (u8, u8, u8)) -> bool {
    r > 200 && g < 60 && b < 60
}
fn green((r, g, b): (u8, u8, u8)) -> bool {
    g > 200 && r < 60 && b < 60
}
fn blue((r, g, b): (u8, u8, u8)) -> bool {
    b > 150 && r < 100 && g < 100
}

const TEXT: [u32; 4] = [20, 220, 220, 260];
const CHOICE: [u32; 4] = [20, 160, 220, 200];
const CHECK: [u32; 4] = [20, 100, 50, 130];

#[test]
fn honour_draws_the_value_over_a_stale_appearance() {
    let out = render_with(stale_form_pdf(true), &RenderOptions::default());
    for rect in [TEXT, CHOICE] {
        assert_eq!(
            count_in(&out, rect, red),
            0,
            "stale /AP painted in {rect:?}"
        );
        assert!(
            count_in(&out, rect, blue) > 30,
            "value not drawn in {rect:?}"
        );
    }
    assert_eq!(out.diagnostics.widgets_redrawn_for_need_appearances, 2);
}

#[test]
fn as_stored_paints_the_stored_appearance() {
    let options = RenderOptions::default().with_need_appearances(NeedAppearances::AsStored);
    let out = render_with(stale_form_pdf(true), &options);
    for rect in [TEXT, CHOICE] {
        assert!(
            count_in(&out, rect, red) > 7000,
            "stored /AP missing in {rect:?}"
        );
        assert_eq!(count_in(&out, rect, blue), 0);
    }
    assert_eq!(out.diagnostics.widgets_redrawn_for_need_appearances, 0);
}

#[test]
fn without_the_flag_the_stored_appearance_stands() {
    let out = render_with(stale_form_pdf(false), &RenderOptions::default());
    assert!(count_in(&out, TEXT, red) > 7000);
    assert_eq!(count_in(&out, TEXT, blue), 0);
    assert_eq!(out.diagnostics.widgets_redrawn_for_need_appearances, 0);
}

#[test]
fn a_check_box_keeps_its_stored_appearance_under_honour() {
    let out = render_with(stale_form_pdf(true), &RenderOptions::default());
    assert!(count_in(&out, CHECK, green) > 800);
}

#[test]
fn honouring_the_flag_writes_nothing() {
    let original = stale_form_pdf(true);
    let session = EditSession::new(Document::from_bytes(original.clone()).expect("loads"));
    let pages = session.pages().expect("pages");
    let out = render_page_with_view(&session.view(), &pages[0], 1.0, &RenderOptions::default())
        .expect("renders");
    assert_eq!(out.diagnostics.widgets_redrawn_for_need_appearances, 2);
    assert!(!session.is_modified());
    let (saved, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves");
    assert_eq!(saved, original);
}

/// Fillable text [20 220 220 260] showing "Hi", read-only text
/// [20 160 220 200], required text [20 100 220 140] and a push button
/// [20 40 120 80] whose stored `/AP` is empty; no other `/AP`, no `/MK`
/// borders.
fn highlight_pdf() -> Vec<u8> {
    let widget = |extra: &str, rect: &str| {
        format!(
            "<< /Type /Annot /Subtype /Widget /F 4 {extra} /DA (/Helv 24 Tf 0 g) \
             /Rect [{rect}] /P 3 0 R >>"
        )
    };
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R] \
         /DA (/Helv 0 Tf 0 g) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R 7 0 R] >>"
            .to_owned(),
        widget("/FT /Tx /T (a) /V (Hi)", "20 220 220 260"),
        widget("/FT /Tx /Ff 1 /T (b) /V (Hi)", "20 160 220 200"),
        widget("/FT /Tx /Ff 2 /T (c)", "20 100 220 140"),
        widget(
            "/FT /Btn /Ff 65536 /T (d) /AP << /N 8 0 R >>",
            "20 40 120 80",
        ),
        form_of("", 100, 40),
    ])
}

fn highlighted() -> RenderOptions {
    RenderOptions::default().with_field_highlight(Some(FieldHighlight::default()))
}

fn tinted((r, g, b): (u8, u8, u8)) -> bool {
    b > 245 && r < 235 && g < 240
}

#[test]
fn a_fillable_field_is_tinted_and_a_read_only_one_is_not() {
    let out = render_with(highlight_pdf(), &highlighted());
    assert!(tinted(px(&out, 200, 240)), "{:?}", px(&out, 200, 240));
    assert_eq!(px(&out, 200, 180), (255, 255, 255), "read-only tinted");
    assert_eq!(px(&out, 100, 60), (255, 255, 255), "push button tinted");
}

#[test]
fn the_highlight_is_off_by_default() {
    let out = render_with(highlight_pdf(), &RenderOptions::default());
    assert_eq!(count_in(&out, [0, 0, 300, 300], tinted), 0);
    assert_eq!(count_in(&out, [0, 0, 300, 300], red), 0);
}

#[test]
fn a_required_field_shows_the_border_colour() {
    let out = render_with(highlight_pdf(), &highlighted());
    assert!(red(px(&out, 20, 120)), "{:?}", px(&out, 20, 120));
    assert!(red(px(&out, 120, 139)), "{:?}", px(&out, 120, 139));
    assert!(tinted(px(&out, 120, 120)), "{:?}", px(&out, 120, 120));
    assert!(!red(px(&out, 20, 240)), "a non-required field got a border");
}

#[test]
fn glyph_pixels_are_the_text_colour_not_the_tint() {
    let out = render_with(highlight_pdf(), &highlighted());
    let solid = count_in(&out, TEXT, |(r, g, b)| r < 8 && g < 8 && b < 8);
    let plain = render_with(highlight_pdf(), &RenderOptions::default());
    let solid_plain = count_in(&plain, TEXT, |(r, g, b)| r < 8 && g < 8 && b < 8);
    assert!(solid > 20, "no solid glyph pixels: {solid}");
    assert_eq!(solid, solid_plain);
}

#[test]
fn a_print_render_shows_no_highlight() {
    let options = highlighted().with_print_intent(true);
    let out = render_with(highlight_pdf(), &options);
    assert_eq!(count_in(&out, [0, 0, 300, 300], tinted), 0);
    assert_eq!(count_in(&out, [0, 0, 300, 300], red), 0);
}
