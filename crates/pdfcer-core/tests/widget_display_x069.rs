//! What a widget appearance shows: a text field's `/AA /F` format helper
//! output (ISO 32000-1 §12.6.3 Table 196, §12.7.3.3), never changing `/V`;
//! and a list box's row window and selection band (§12.7.4.4 Table 231).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms::{self, FieldValue};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::widget_draw::WidgetAppearances;
use pdfcer_core::writer::SaveOptions;

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
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

/// One field "f" (object 4, merged with its widget, no `/AP`), 180 x 42 pt
/// with `/DA` 10 pt: room for exactly three list rows. `field` is the rest
/// of its dictionary.
fn doc(field: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /T (f) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 92] /DA (/Helv 10 Tf 0 g) {field} >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    assemble(&bodies)
}

fn session(field: &str) -> EditSession {
    EditSession::new(Document::from_bytes(doc(field)).expect("fixture parses"))
}

fn the_field(s: &EditSession) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "f")
        .unwrap()
}

fn normal_ap(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let w = g
        .resolved(the_field(s).widgets[0].id)
        .as_dict()
        .cloned()
        .unwrap();
    let ap = w.get(b"AP").map(|o| g.resolve(o).clone()).unwrap();
    let n = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone());
    let Some(Object::Stream(st)) = n else {
        panic!("no /AP /N stream: {n:?}");
    };
    s.view().slice(st.data_span).unwrap().to_vec()
}

/// What a content stream draws: each `Tj` string with its baseline y, and
/// each highlight-band rectangle as (y, y + h).
#[derive(Debug, Default)]
struct Drawn {
    text: Vec<(String, f64)>,
    bands: Vec<(f64, f64)>,
    red: bool,
}

fn is_highlight(rgb: &[f64]) -> bool {
    let want = [0.600_006, 0.756_866, 0.854_904];
    rgb.len() == 3 && rgb.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3)
}

fn drawn(content: &[u8]) -> Drawn {
    let src = String::from_utf8_lossy(content);
    let mut out = Drawn::default();
    let mut nums: Vec<f64> = Vec::new();
    let mut last_str = String::new();
    let mut pen = (0.0, 0.0);
    let mut banding = false;
    let mut in_text = false;
    let mut chars = src.chars().peekable();
    let mut tok = String::new();
    let mut tokens: Vec<String> = Vec::new();
    while let Some(c) = chars.next() {
        if c == '(' {
            let mut s = String::from("(");
            while let Some(d) = chars.next() {
                match d {
                    '\\' => s.push(chars.next().unwrap()),
                    ')' => break,
                    _ => s.push(d),
                }
            }
            tokens.push(s);
        } else if c.is_whitespace() {
            if !tok.is_empty() {
                tokens.push(std::mem::take(&mut tok));
            }
        } else {
            tok.push(c);
        }
    }
    if !tok.is_empty() {
        tokens.push(tok);
    }
    for t in tokens {
        if let Some(s) = t.strip_prefix('(') {
            last_str = s.to_owned();
            continue;
        }
        if let Ok(v) = t.parse::<f64>() {
            nums.push(v);
            continue;
        }
        match t.as_str() {
            "BT" => in_text = true,
            "ET" => in_text = false,
            "rg" => {
                banding = !in_text && is_highlight(&nums);
                if in_text && nums == [1.0, 0.0, 0.0] {
                    out.red = true;
                }
            }
            "re" if banding => out.bands.push((nums[1], nums[1] + nums[3])),
            "f" => banding = false,
            "Tm" => pen = (nums[4], nums[5]),
            "Td" => pen = (pen.0 + nums[0], pen.1 + nums[1]),
            "Tj" => out.text.push((std::mem::take(&mut last_str), pen.1)),
            _ => {}
        }
        nums.clear();
    }
    out
}

impl Drawn {
    fn strings(&self) -> Vec<&str> {
        self.text.iter().map(|(s, _)| s.as_str()).collect()
    }

    /// The drawn strings whose baseline sits inside a highlight band.
    fn highlighted(&self) -> Vec<&str> {
        self.text
            .iter()
            .filter(|(_, y)| self.bands.iter().any(|&(lo, hi)| *y > lo && *y < hi))
            .map(|(s, _)| s.as_str())
            .collect()
    }
}

/// The appearance the view-time synthesizer draws, and its notes.
fn synthesized(field: &str) -> (Drawn, pdfcer_core::field_display::DisplayNotes) {
    let s = session(field);
    let view = s.view();
    let wa = WidgetAppearances::read(&view).unwrap();
    let a = wa.appearance(&view, the_field(&s).widgets[0].id).unwrap();
    (drawn(&a.content), a.notes)
}

fn text_field(v: &str, js: &str) -> String {
    format!("/FT /Tx /V ({v}) /AA << /F << /S /JavaScript /JS ({js}) >> >>")
}

// ---------------------------------------------------------------- formats

#[test]
fn a_number_format_draws_formatted_and_leaves_v_unchanged() {
    let mut s = session(&text_field(
        "1234.56",
        r#"AFNumber_Format(2,0,0,0,"$",true);"#,
    ));
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.formatted, 1);
    assert_eq!(out.display.unformatted, 0);
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["$1,234.56"]);

    let (saved, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let back = EditSession::new(Document::from_bytes(saved).unwrap());
    assert_eq!(
        the_field(&back).value,
        FieldValue::Text(b"1234.56".to_vec()),
        "/V was rewritten"
    );
    assert_eq!(drawn(&normal_ap(&back)).strings(), ["$1,234.56"]);
}

#[test]
fn the_view_time_synthesizer_draws_the_same_formatted_text() {
    let (d, notes) = synthesized(&text_field(
        "1234.56",
        r#"AFNumber_Format(2,0,0,0,"$",true);"#,
    ));
    assert_eq!(d.strings(), ["$1,234.56"]);
    assert!(notes.formatted);
}

#[test]
fn without_the_format_action_the_stored_value_is_drawn() {
    let mut s = session("/FT /Tx /V (1234.56)");
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.formatted, 0);
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["1234.56"]);
}

#[test]
fn a_percent_format_draws_a_percentage() {
    let mut s = session(&text_field("0.085", "AFPercent_Format(1,0);"));
    s.regenerate_appearances().unwrap();
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["8.5%"]);
}

#[test]
fn a_date_format_draws_the_date_in_its_pattern() {
    let mut s = session(&text_field(
        "D:20260314120000",
        r#"AFDate_FormatEx("yyyy-mm-dd");"#,
    ));
    s.regenerate_appearances().unwrap();
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["2026-03-14"]);
}

#[test]
fn a_special_format_draws_a_zip_plus_four() {
    let mut s = session(&text_field("123456789", "AFSpecial_Format(1);"));
    s.regenerate_appearances().unwrap();
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["12345-6789"]);
}

#[test]
fn a_custom_format_script_draws_v_and_is_reported() {
    let mut s = session(&text_field("1234.56", "customFormat();"));
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.unformatted, 1);
    assert_eq!(out.display.formatted, 0);
    assert_eq!(drawn(&normal_ap(&s)).strings(), ["1234.56"]);
    let (_, notes) = synthesized(&text_field("1234.56", "customFormat();"));
    assert!(notes.unformatted.is_some());
}

#[test]
fn a_red_negative_style_draws_the_value_in_red() {
    // negStyle 1: red, minus sign kept (no parentheses).
    let mut s = session(&text_field("-5", r#"AFNumber_Format(2,0,1,0,"",true);"#));
    s.regenerate_appearances().unwrap();
    let d = drawn(&normal_ap(&s));
    assert_eq!(d.strings(), ["-5.00"]);
    assert!(d.red, "not drawn red");
    let mut plain = session(&text_field("5", r#"AFNumber_Format(2,0,1,0,"",true);"#));
    plain.regenerate_appearances().unwrap();
    assert!(!drawn(&normal_ap(&plain)).red, "a positive value drawn red");
}

// -------------------------------------------------------------- list boxes

const OPTS: &str = "/Opt [(A) (B) (C) (D) (E) (F)]";

fn list(extra: &str) -> String {
    format!("/FT /Ch {OPTS} {extra}")
}

fn regen(field: &str) -> (Drawn, pdfcer_core::field_display::DisplayCounts) {
    let mut s = session(field);
    let out = s.regenerate_appearances().unwrap();
    (drawn(&normal_ap(&s)), out.display)
}

#[test]
fn a_selection_below_the_fold_is_scrolled_into_view_and_highlighted() {
    for (d, _) in [
        regen(&list("/V (E)")),
        (synthesized(&list("/V (E)")).0, Default::default()),
    ] {
        assert_eq!(d.strings(), ["D", "E", "F"]);
        assert_eq!(d.highlighted(), ["E"]);
    }
}

#[test]
fn a_valid_top_index_sets_the_first_row() {
    let (d, counts) = regen(&list("/V (E) /TI 1"));
    assert_eq!(d.strings(), ["B", "C", "D"]);
    assert!(d.highlighted().is_empty(), "E is outside the window");
    assert_eq!(counts.list_indices_ignored, 0);
}

#[test]
fn multi_select_highlights_every_selected_row() {
    let (d, _) = regen(&list("/Ff 2097152 /I [0 2]"));
    assert_eq!(d.strings(), ["A", "B", "C"]);
    assert_eq!(d.highlighted(), ["A", "C"]);
}

#[test]
fn out_of_range_indices_are_ignored_and_reported() {
    let (d, counts) = regen(&list("/TI 99 /I [99]"));
    assert_eq!(d.strings(), ["A", "B", "C"]);
    assert!(d.highlighted().is_empty());
    assert_eq!(counts.list_indices_ignored, 2);
    let (_, notes) = synthesized(&list("/TI 99 /I [99]"));
    assert_eq!(notes.ignored_top_index, Some(99));
    assert_eq!(notes.ignored_selected, [99]);
}

#[test]
fn a_huge_option_list_draws_only_the_rows_that_fit() {
    let opts: String = (0..5000).map(|i| format!("(o{i}) ")).collect();
    let (d, _) = regen(&format!("/FT /Ch /Opt [{opts}] /V (o4000)"));
    assert_eq!(d.strings().len(), 3);
    assert_eq!(d.highlighted(), ["o4000"]);
}

#[test]
fn a_combo_box_still_draws_only_its_value() {
    let (d, counts) = regen(&format!("/FT /Ch /Ff 131072 {OPTS} /V (E)"));
    assert_eq!(d.strings(), ["E"]);
    assert!(d.bands.is_empty());
    assert_eq!(counts, Default::default());
}

#[test]
fn choosing_a_list_value_draws_the_new_selection() {
    let mut s = session(&list("/V (A)"));
    s.set_choice_value("f", &["E"]).unwrap();
    let d = drawn(&normal_ap(&s));
    assert_eq!(d.strings(), ["D", "E", "F"]);
    assert_eq!(d.highlighted(), ["E"]);
}
