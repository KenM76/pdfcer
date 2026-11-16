//! A text box's preview: the lines it reports, what it clips, and that a
//! retyped box previews exactly the appearance the commit writes
//! (pdfcer-gui request G282).

use pdfcer_core::annot_author::{Color, StickyIcon, TextAnnotSpec, free_text_preview};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::{Quadding, TextColor, text_width};

const WORDS: &str = "one two three four five six seven eight nine ten";

fn free_text(text: &str, rect: Rect, multiline: bool, quadding: Quadding) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect,
        text: text.to_owned(),
        font: Std14::Helvetica,
        font_size: 12.0,
        color: TextColor::Gray(0.0),
        quadding,
        multiline,
        border: Some(Color::Gray(0.0)),
        border_width: 1.0,
        frame: Default::default(),
    }
}

const fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect { llx, lly, urx, ury }
}

fn session(bodies: &[&str]) -> EditSession {
    let bodies: Vec<String> = bodies.iter().map(|b| (*b).to_string()).collect();
    EditSession::new(Document::from_bytes(crate::block_layout::build_pdf(&bodies)).unwrap())
}

fn blank_session() -> EditSession {
    session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << >> >>",
    ])
}

#[test]
fn lines_tile_the_text_and_sit_inside_the_box() {
    let spec = free_text(WORDS, rect(50.0, 100.0, 130.0, 200.0), true, Quadding::Left);
    let p = free_text_preview(&spec).unwrap().unwrap();
    assert_eq!(p.authored.rect, rect(50.0, 100.0, 130.0, 200.0));
    assert!(p.layout.lines.len() >= 4, "{}", p.layout.lines.len());

    // Every byte of the text belongs to exactly one line, in order.
    let mut next = 0usize;
    for line in &p.layout.lines {
        assert_eq!(line.start, next);
        for c in &line.chars {
            assert_eq!(c.offset, next);
            assert!(c.x1 >= c.x0);
            next = c.end;
        }
        assert_eq!(line.end, next);
        assert!(line.x0 >= 2.0 - 1e-9 && line.x1 <= 80.0 - 2.0 + 1e-9);
    }
    assert_eq!(next, WORDS.len());

    // Baselines step down by one line height; the first sits an ascent
    // below the top padding.
    let first = &p.layout.lines[0];
    assert!((first.baseline - (100.0 - 2.0 - p.layout.ascent)).abs() < 1e-9);
    let step = first.baseline - p.layout.lines[1].baseline;
    assert!((step - 12.0 * 1.15).abs() < 1e-9);

    // The drawn width of the first line is the advance of its words.
    let drawn = WORDS[first.start..first.end].trim_end();
    assert!((first.x1 - first.x0 - text_width(Std14::Helvetica, 12.0, drawn)).abs() < 1e-9);
}

#[test]
fn a_short_box_clips_and_the_grown_rectangle_does_not() {
    let spec = free_text(WORDS, rect(50.0, 180.0, 130.0, 200.0), true, Quadding::Left);
    let p = free_text_preview(&spec).unwrap().unwrap();
    assert!(p.clips());
    assert!(p.needed_height() > 20.0);

    let grown = p.rect_grown_downward();
    assert_eq!((grown.llx, grown.urx, grown.ury), (50.0, 130.0, 200.0));
    assert!((grown.height() - p.needed_height()).abs() < 1e-9);

    let again = free_text_preview(&free_text(WORDS, grown, true, Quadding::Left))
        .unwrap()
        .unwrap();
    assert!(!again.clips());
    assert_eq!(again.layout.lines.len(), p.layout.lines.len());
    assert_eq!(again.rect_grown_downward(), grown);
}

#[test]
fn a_single_line_box_reports_the_width_it_needs() {
    let spec = free_text(WORDS, rect(0.0, 0.0, 60.0, 20.0), false, Quadding::Left);
    let p = free_text_preview(&spec).unwrap().unwrap();
    assert_eq!(p.layout.lines.len(), 1);
    assert!(p.clips());
    let need = text_width(Std14::Helvetica, 12.0, WORDS) + 4.0;
    assert!((p.needed_width() - need).abs() < 1e-9);
    // Vertically one line fits a 20 pt box.
    assert!(p.needed_height() <= 20.0);
    assert_eq!(p.rect_grown_downward(), rect(0.0, 0.0, 60.0, 20.0));
}

#[test]
fn alignment_moves_the_lines_and_the_preview_is_the_baked_appearance() {
    let r = rect(10.0, 10.0, 210.0, 60.0);
    let left = free_text_preview(&free_text("Hi there", r, true, Quadding::Left))
        .unwrap()
        .unwrap();
    let right_spec = free_text("Hi there", r, true, Quadding::Right);
    let right = free_text_preview(&right_spec).unwrap().unwrap();
    assert!((left.layout.lines[0].x0 - 2.0).abs() < 1e-9);
    assert!((right.layout.lines[0].x1 - 198.0).abs() < 1e-9);
    assert_eq!(
        right.authored,
        pdfcer_core::annot_author::build_text_annotation(&right_spec).unwrap()
    );
    assert_eq!(right.spec, right_spec);
}

#[test]
fn a_character_outside_winansi_is_counted_and_keeps_its_offsets() {
    let spec = free_text(
        "a\u{05D0}b",
        rect(0.0, 0.0, 100.0, 20.0),
        false,
        Quadding::Left,
    );
    let p = free_text_preview(&spec).unwrap().unwrap();
    assert_eq!(p.authored.unencodable_chars, 1);
    let chars = &p.layout.lines[0].chars;
    assert_eq!(chars.len(), 3);
    assert_eq!((chars[1].offset, chars[1].end), (1, 3));
    assert!(chars[1].x1 > chars[1].x0);
}

#[test]
fn a_spec_that_is_not_free_text_has_no_preview() {
    let sticky = TextAnnotSpec::Sticky {
        rect: rect(0.0, 0.0, 20.0, 20.0),
        icon: StickyIcon::Note,
        contents: "x".to_owned(),
        color: Color::Rgb(1.0, 1.0, 0.0),
        open: false,
    };
    assert!(free_text_preview(&sticky).unwrap().is_none());
}

#[test]
fn a_retyped_box_previews_the_appearance_the_commit_writes() {
    for multiline in [false, true] {
        let mut s = blank_session();
        let placed = free_text(
            "first words",
            rect(40.0, 40.0, 120.0, 100.0),
            multiline,
            Quadding::Center,
        );
        let id = s.add_text_annotation(0, &placed).unwrap();
        let before = s.undo_depth();

        let p = s.free_text_note_preview(id, WORDS).unwrap().unwrap();
        assert_eq!(s.undo_depth(), before, "the preview staged something");
        assert_eq!(p.authored.rect, rect(40.0, 40.0, 120.0, 100.0));
        assert_eq!(p.layout.lines.len() > 1, multiline);
        let TextAnnotSpec::FreeText {
            text,
            multiline: m,
            quadding,
            ..
        } = &p.spec
        else {
            panic!("not a FreeText spec");
        };
        assert_eq!(
            (text.as_str(), *m, *quadding),
            (WORDS, multiline, Quadding::Center)
        );

        let change = s.set_markup_note(id, &MarkupNote::new(WORDS)).unwrap();
        assert!(change.appearance_rebaked);
        // Previewing the words now on the page finds the committed
        // appearance to be the bake the first preview showed.
        let after = s.free_text_note_preview(id, WORDS).unwrap().unwrap();
        assert_eq!(after.authored.ap_content, p.authored.ap_content);
        assert_eq!(after.layout, p.layout);
    }
}

#[test]
fn a_box_someone_else_drew_and_a_non_text_box_have_no_retype_preview() {
    let s = session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << >> \
         /Annots [4 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /FreeText /Rect [50 50 200 100] /F 4 \
         /DA (/Helv 12 Tf 0 g) /Contents (hello) /AP << /N 5 0 R >> >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 150 50] /Length 3 >>\nstream\nq Q\nendstream",
        "<< /Type /Annot /Subtype /Square /Rect [10 10 40 40] /F 4 >>",
    ]);
    let preview = |n: u32| s.free_text_note_preview(ObjId::new(n, 0), "new");
    assert!(preview(4).unwrap().is_none());
    assert!(preview(6).unwrap().is_none());
    assert!(preview(99).is_err());
}
