#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::richtext::parse;

fn run(text: &str, style: Style, paragraph: usize) -> Run {
    Run {
        text: text.to_owned(),
        style,
        paragraph,
    }
}

fn plain(text: &str, paragraph: usize) -> Run {
    run(text, Style::default(), paragraph)
}

fn round_trip(runs: &[Run]) -> Vec<Run> {
    let rv = to_xhtml(runs);
    parse(&rv, None).unwrap_or_else(|e| panic!("{e}: {rv}"))
}

/// Every Table 225 attribute at once, each at a non-default value.
fn every_attribute() -> Style {
    Style {
        align: Some(Align::Right),
        baseline_shift_pt: Some(-2.5),
        size_pt: Some(13.25),
        italic: Some(true),
        weight: Some(300),
        family: vec!["Times New Roman".to_owned(), "serif".to_owned()],
        color: Some([10.0 / 255.0, 1.0, 128.0 / 255.0]),
        underline: Some(true),
        strikethrough: Some(true),
        stretch: Some(Stretch::SemiExpanded),
    }
}

#[test]
fn every_attribute_round_trips() {
    let runs = vec![plain("a ", 0), run("b", every_attribute(), 0)];
    assert_eq!(round_trip(&runs), runs);
}

#[test]
fn each_attribute_round_trips_alone() {
    let mut styles = vec![
        Style {
            italic: Some(false),
            ..Style::default()
        },
        Style {
            weight: Some(400),
            ..Style::default()
        },
        Style {
            baseline_shift_pt: Some(4.0),
            ..Style::default()
        },
        Style {
            size_pt: Some(0.5),
            ..Style::default()
        },
        Style {
            family: vec!["O'Brien Sans".to_owned(), "Say \"Hi\" Sans".to_owned()],
            ..Style::default()
        },
    ];
    for a in [Align::Left, Align::Center, Align::Right] {
        styles.push(Style {
            align: Some(a),
            ..Style::default()
        });
    }
    for s in [
        Stretch::UltraCondensed,
        Stretch::ExtraCondensed,
        Stretch::Condensed,
        Stretch::SemiCondensed,
        Stretch::Normal,
        Stretch::SemiExpanded,
        Stretch::Expanded,
        Stretch::ExtraExpanded,
        Stretch::UltraExpanded,
    ] {
        styles.push(Style {
            stretch: Some(s),
            ..Style::default()
        });
    }
    for k in [0u8, 1, 127, 128, 254, 255] {
        let c = f64::from(k) / 255.0;
        styles.push(Style {
            color: Some([c, f64::from(255 - k) / 255.0, c]),
            ..Style::default()
        });
    }
    for style in styles {
        let runs = vec![plain("x", 0), run("y", style, 0)];
        assert_eq!(round_trip(&runs), runs, "{}", to_xhtml(&runs));
    }
}

/// Seven decoration pairs are exact; the two with an explicit `false`
/// beside an unset partner read back as both off.
#[test]
fn decoration_pairs_round_trip_or_normalise() {
    let all = [None, Some(false), Some(true)];
    for u in all {
        for s in all {
            let style = Style {
                underline: u,
                strikethrough: s,
                ..Style::default()
            };
            let got = round_trip(&[run("x", style, 0)]);
            let want = match (u, s) {
                (Some(false), None) | (None, Some(false)) => (Some(false), Some(false)),
                pair => pair,
            };
            assert_eq!(
                (got[0].style.underline, got[0].style.strikethrough),
                want,
                "{u:?} {s:?}"
            );
        }
    }
}

#[test]
fn markup_characters_and_controls_are_escaped_and_kept() {
    let text = "a & b < c > d \"q\" 'q' \t tab\nline\r\n\u{1}\u{0}\u{FFFF} é ✓";
    let runs = vec![
        plain(text, 0),
        run(
            "<b>not markup</b>",
            Style {
                weight: Some(700),
                ..Style::default()
            },
            0,
        ),
    ];
    let rv = to_xhtml(&runs);
    assert!(rv.contains("a &amp; b &lt; c &gt; d"), "{rv}");
    assert!(rv.contains("&#xD;") && rv.contains("&#x1;") && rv.contains("&#x0;"));
    assert!(!rv.contains('\r') && !rv.contains('\u{1}'));
    assert!(rv.contains("&lt;b&gt;not markup&lt;/b&gt;"), "{rv}");
    assert_eq!(round_trip(&runs), runs);
}

#[test]
fn paragraphs_and_gaps_round_trip() {
    let runs = vec![
        plain("one", 0),
        run(
            " more",
            Style {
                italic: Some(true),
                ..Style::default()
            },
            0,
        ),
        plain("two", 1),
        plain("after two blanks", 4),
    ];
    let rv = to_xhtml(&runs);
    assert_eq!(rv.matches("<p></p>").count(), 2, "{rv}");
    assert_eq!(round_trip(&runs), runs);
}

#[test]
fn a_decrease_starts_the_next_paragraph() {
    let runs = vec![plain("a", 3), plain("b", 1), plain("c", 1)];
    let paras: Vec<usize> = round_trip(&runs).iter().map(|r| r.paragraph).collect();
    assert_eq!(paras, [3, 4, 4]);
}

#[test]
fn a_huge_paragraph_gap_is_bounded() {
    let runs = vec![plain("a", 0), plain("b", usize::MAX)];
    let rv = to_xhtml(&runs);
    assert!(rv.len() < 2 * MAX_RICH_BYTES, "{}", rv.len());
}

/// Equal-style neighbours stay two runs, unstyled ones included: two bare
/// text nodes would read back as one.
#[test]
fn adjacent_equal_styles_stay_separate_runs() {
    let bold = Style {
        weight: Some(700),
        ..Style::default()
    };
    let runs = vec![
        plain("a", 0),
        plain("b", 0),
        run("c", bold.clone(), 0),
        run("d", bold, 0),
        plain("e", 0),
    ];
    assert_eq!(round_trip(&runs), runs);
}

#[test]
fn empty_runs_are_dropped() {
    let runs = vec![plain("", 0), plain("a", 0), plain("", 1), plain("b", 1)];
    assert_eq!(round_trip(&runs), [plain("a", 0), plain("b", 1)]);
}

#[test]
fn shared_alignment_goes_on_the_paragraph_and_mixed_on_spans() {
    let center = Style {
        align: Some(Align::Center),
        ..Style::default()
    };
    let shared = vec![run("a", center.clone(), 0), run("b", center.clone(), 0)];
    let rv = to_xhtml(&shared);
    assert!(rv.contains("<p style=\"text-align:center\">a"), "{rv}");
    assert_eq!(round_trip(&shared), shared);

    let mixed = vec![run("a", center, 0), plain("b", 0)];
    let rv = to_xhtml(&mixed);
    assert!(
        rv.contains("<p><span style=\"text-align:center\">a</span>"),
        "{rv}"
    );
    assert_eq!(round_trip(&mixed), mixed);
}

#[test]
fn values_outside_table_225_are_written_in_normal_form() {
    let odd = Style {
        weight: Some(350),
        size_pt: Some(f64::NAN),
        baseline_shift_pt: Some(f64::INFINITY),
        color: Some([0.5, -1.0, 2.0]),
        family: vec![" 'A,rial; ' ".to_owned(), ",;".to_owned(), "B".to_owned()],
        ..Style::default()
    };
    let got = round_trip(&[run("x", odd, 0)]);
    let want = Style {
        color: Some([128.0 / 255.0, 0.0, 1.0]),
        family: vec!["Arial".to_owned(), "B".to_owned()],
        ..Style::default()
    };
    assert_eq!(got, [run("x", want, 0)]);
}

#[test]
fn the_body_carries_the_table_224_attributes() {
    let rv = to_xhtml(&[]);
    for attr in [
        "xmlns=\"http://www.w3.org/1999/xhtml\"",
        "xmlns:xfa=\"http://www.xfa.org/schema/xfa-data/1.0/\"",
        "xfa:contentType=\"text/html\"",
        "xfa:APIVersion=\"pdfcer:",
        "xfa:spec=\"2.4\"",
    ] {
        assert!(rv.contains(attr), "{attr} missing: {rv}");
    }
    assert_eq!(parse(&rv, None).unwrap(), [plain("", 0)]);
}

/// A line then Enter: the trailing blank paragraph survives, written and
/// read, and `/V` keeps the line feed the operator typed.
#[test]
fn a_trailing_blank_paragraph_round_trips() {
    let runs = vec![plain("a", 0), plain("", 1)];
    let rv = to_xhtml(&runs);
    assert!(rv.ends_with("<p>a</p><p></p></body>"), "{rv}");
    assert_eq!(round_trip(&runs), runs);
    let flat = super::super::flat_text(&runs, super::super::FlatText::ParagraphLines);
    assert_eq!(flat, "a\n");

    let several = vec![plain("a", 0), plain("", 3)];
    assert_eq!(to_xhtml(&several).matches("<p></p>").count(), 3);
    assert_eq!(round_trip(&several), several);
}

/// A cleared field is one empty paragraph, whether given as no runs or as
/// one empty run.
#[test]
fn a_cleared_field_writes_one_empty_paragraph() {
    for runs in [vec![], vec![plain("", 0)]] {
        let rv = to_xhtml(&runs);
        assert!(rv.ends_with("\"><p></p></body>"), "{rv}");
        assert_eq!(round_trip(&runs), [plain("", 0)]);
    }
}

/// Leading blank paragraphs are kept as the first run's index.
#[test]
fn leading_blank_paragraphs_round_trip() {
    let runs = vec![plain("a", 2)];
    let rv = to_xhtml(&runs);
    assert!(rv.contains("\"><p></p><p></p><p>a</p>"), "{rv}");
    assert_eq!(round_trip(&runs), runs);
    assert_eq!(round_trip(&[plain("", 0), plain("a", 1)]), [plain("a", 1)]);
    let flat = super::super::flat_text(&runs, super::super::FlatText::ParagraphLines);
    assert_eq!(flat, "\n\na");
}

/// The operator's lines, one run each, keep their count through
/// `parse(to_xhtml(..))` and through `/V`.
#[test]
fn typed_lines_keep_their_count() {
    for typed in ["a\n", "\na", "a\n\nb", "\n", "", "a\nb\n\n"] {
        let runs: Vec<Run> = typed
            .split('\n')
            .enumerate()
            .map(|(i, line)| plain(line, i))
            .collect();
        let back = round_trip(&runs);
        let flat = super::super::flat_text(&back, super::super::FlatText::ParagraphLines);
        assert_eq!(flat, typed, "{typed:?} -> {back:?}");
    }
}

/// `parse(to_xhtml(parse(x))) == parse(x)` over markup the writer never
/// produces: nesting, shorthand, `/DS`, prefixes, unknown elements, loose
/// body text and empty paragraphs.
#[test]
fn parse_output_is_a_fixed_point() {
    let cases: [(&str, Option<&str>); 8] = [
        (
            "<body><p>Hello <b>bold <i>both</i></b> world</p></body>",
            None,
        ),
        (
            "<body><p style='font: italic bold 9pt \"Times New Roman\", serif'>x</p></body>",
            None,
        ),
        (
            "<body><p>base <span style='font-size:20pt'>big</span></p></body>",
            Some("font-size: 10pt; color: #FF0000; text-align: right"),
        ),
        (
            "<xhtml:body xmlns:xhtml='http://www.w3.org/1999/xhtml'>\
             <xhtml:p><xhtml:b>x</xhtml:b></xhtml:p></xhtml:body>",
            None,
        ),
        (
            "<body>loose <p>a<blink>b</blink>c</p><p></p><p>d</p></body>",
            None,
        ),
        (
            "<body><p><span style='text-decoration:none'>n<span \
             style='text-decoration:underline'>u</span></span></p></body>",
            None,
        ),
        (
            "<body><p>&amp;&lt;&#x9;&#13;&#1;<b style='font-weight:normal'>w</b></p></body>",
            Some("font-family: a\tb, 'c'"),
        ),
        (
            "<body><p style='vertical-align:-3pt;font-stretch:condensed'>s</p></body>",
            None,
        ),
    ];
    for (rv, ds) in cases {
        let first = parse(rv, ds).unwrap();
        assert!(!first.is_empty(), "{rv}");
        assert_eq!(round_trip(&first), first, "{rv}");
    }
}
