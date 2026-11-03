#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::check_tests::{kinds, painted};
use super::*;

#[test]
fn a_typed_value_shows_through_its_display_picture() {
    let f = r#"<field name="d" w="200pt" h="20pt"><ui><dateTimeEdit/></ui><format><picture>date{MMMM D, YYYY}</picture></format><value><date>2020-01-05</date></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(January 5, 2020) Tj"), "{ops}");
    assert!(
        !kinds(&notes).contains(&PaintNoteKind::Content),
        "{notes:?}"
    );
    let f = r#"<field name="n" w="200pt" h="20pt"><ui><numericEdit/></ui><format><picture>num{$z,zz9.99}</picture></format><value><decimal>1234.5</decimal></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("($1,234.50) Tj"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn an_unformattable_picture_or_foreign_locale_says_so() {
    let f = r#"<field name="n" w="200pt" h="20pt"><format><picture>num{99.999E}</picture></format><value><decimal>12345</decimal></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(12345) Tj"), "{ops}");
    assert_eq!(kinds(&notes), [PaintNoteKind::Picture]);
    let f = r#"<field name="d" w="200pt" h="20pt" locale="fr_CA"><format><picture>date{MMMM}</picture></format><value><date>2020-01-05</date></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(January) Tj"), "{ops}");
    assert_eq!(kinds(&notes), [PaintNoteKind::Picture]);
}

#[test]
fn a_password_is_masked() {
    let f = r#"<field name="p" w="80pt" h="20pt"><ui><passwordEdit passwordChar="x"/></ui><value><text>abc</text></value></field>"#;
    let (ops, _) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(xxx) Tj") && !ops.contains("abc"), "{ops}");
    let f = r#"<field name="p" w="80pt" h="20pt"><ui><passwordEdit/></ui><value><text>ab</text></value></field>"#;
    let (ops, _) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(**) Tj"), "{ops}");
}

#[test]
fn a_button_shows_its_caption_and_no_content_note() {
    let f = r#"<field name="b" w="80pt" h="20pt"><ui><button/></ui><border><edge/></border><caption><value><text>Clear</text></value><para hAlign="center" vAlign="middle"/></caption></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(Clear) Tj"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_signature_border_is_drawn_on_screen_only() {
    let f = r#"<field name="s" w="80pt" h="20pt"><ui><signature><border><edge/></border></signature></ui></field>"#;
    let (screen, notes) = painted(f, &PaintOptions::new());
    assert!(screen.contains(" S"), "{screen}");
    assert!(notes.is_empty(), "{notes:?}");
    let (paper, _) = painted(f, &PaintOptions::new().with_medium(Medium::Paper));
    assert!(!paper.contains(" S"), "{paper}");
}

#[test]
fn a_barcode_value_is_shown_as_text_and_disclosed() {
    let f = r#"<field name="c" w="80pt" h="20pt"><ui><barcode type="code128"/></ui><value><text>A1</text></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(A1) Tj"), "{ops}");
    assert_eq!(kinds(&notes), [PaintNoteKind::Barcode]);
}

#[test]
fn a_comb_puts_one_character_per_cell_between_tines() {
    let f = r#"<field name="c" w="40pt" h="20pt"><ui><textEdit><border hand="right"><edge thickness="1pt"/><edge/><edge thickness="2pt"><color value="255,0,0"/></edge></border><comb numberOfCells="4"/></textEdit></ui><value><text>abe&#x301;</text></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert_eq!(ops.matches(" Tj").count(), 3, "{ops}");
    assert!(ops.contains("2 w") && ops.contains("1 0 0 RG"), "{ops}");
    assert_eq!(
        ops.matches(" l").count(),
        3 + 4,
        "three tines and four edges: {ops}"
    );
    assert!(
        kinds(&notes).contains(&PaintNoteKind::Text),
        "a combining mark is not in WinAnsiEncoding: {notes:?}"
    );
    let f = r#"<field name="c" w="40pt" h="20pt"><ui><textEdit><comb/></textEdit></ui><value><text maxChars="2">abc</text></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert_eq!(ops.matches(" Tj").count(), 2, "{ops}");
    assert_eq!(kinds(&notes), [PaintNoteKind::Text]);
}

#[test]
fn an_inline_caption_runs_before_the_value() {
    let f = r#"<field name="i" w="200pt" h="20pt"><caption placement="inline"><value><text>Name: </text></value></caption><value><text>Ada</text></value></field>"#;
    let (ops, notes) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(Name: Ada) Tj"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
    let f = r#"<field name="i" w="200pt" h="20pt"><caption placement="inline"><value><text>Empty</text></value></caption></field>"#;
    let (ops, _) = painted(f, &PaintOptions::new());
    assert!(ops.contains("(Empty) Tj"), "{ops}");
    let f = r#"<field name="i" w="200pt" h="20pt"><caption placement="inline"><value><exData contentType="text/html"><body xmlns="http://www.w3.org/1999/xhtml"><p>R</p></body></exData></value></caption><value><text>v</text></value></field>"#;
    let (_, notes) = painted(f, &PaintOptions::new());
    assert!(kinds(&notes).contains(&PaintNoteKind::Content), "{notes:?}");
}
