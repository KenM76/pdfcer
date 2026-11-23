//! `richtext::to_xhtml` from outside the crate: runs a consumer builds
//! survive `parse` (ISO 32000-1 §12.7.3.4, Tables 223-225).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::richtext::{self, Align, FlatText, Run, Stretch, Style};

#[test]
fn consumer_built_runs_round_trip_through_the_writer() {
    let styled = Style {
        align: Some(Align::Center),
        baseline_shift_pt: Some(3.0),
        size_pt: Some(11.5),
        italic: Some(true),
        weight: Some(700),
        family: vec!["Helvetica".to_owned()],
        color: Some([0.0, 0.0, 1.0]),
        underline: Some(true),
        strikethrough: Some(false),
        stretch: Some(Stretch::Condensed),
    };
    let runs = vec![
        Run {
            text: "Fish & chips <".to_owned(),
            style: Style::default(),
            paragraph: 0,
        },
        Run {
            text: "styled".to_owned(),
            style: styled,
            paragraph: 0,
        },
        Run {
            text: "line two".to_owned(),
            style: Style::default(),
            paragraph: 1,
        },
    ];
    let rv = richtext::to_xhtml(&runs);
    assert_eq!(richtext::parse(&rv, None).unwrap(), runs, "{rv}");
    assert_eq!(
        richtext::flat_text(&runs, FlatText::ParagraphLines),
        "Fish & chips <styled\nline two"
    );
}

/// The edit the request exists for: change one run's text, keep the
/// other run's bold.
#[test]
fn an_edit_keeps_the_styles_it_did_not_touch() {
    let stored = "<body><p>Total: <b>42</b></p></body>";
    let mut runs = richtext::parse(stored, None).unwrap();
    runs[0].text = "Sum: ".to_owned();
    let again = richtext::parse(&richtext::to_xhtml(&runs), None).unwrap();
    assert_eq!(again[0].text, "Sum: ");
    assert_eq!(again[1].text, "42");
    assert_eq!(again[1].style.weight, Some(700));
}
