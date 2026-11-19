//! `xfa::field_widget`: the entry a field takes and the values it writes
//! (pdfcer-gui request G300).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use pdfcer_core::xfa::{ChoiceOpen, FieldWidget, Template, field_widget, read_template, read_xdp};

fn template(body: &str) -> Template {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1">{body}</subform></template></xdp:xdp>"#
    );
    let bytes = crate_pdf(&xdp);
    let doc = pdfcer_core::document::Document::from_bytes(bytes).unwrap();
    let x = read_xdp(&doc.view()).unwrap().unwrap();
    read_template(&x).unwrap().unwrap()
}

fn crate_pdf(xdp: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /XFA 4 0 R >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{xdp}\nendstream", xdp.len()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for o in at {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// The widget of the `i`th child of the root subform (path `[0, i]`).
fn kind(t: &Template, i: usize) -> Option<FieldWidget> {
    field_widget(t, &[0, i])
}

#[test]
fn text_numeric_and_date_widgets_and_their_defaults() {
    let t = template(concat!(
        r#"<field name="a"><ui><textEdit multiLine="1"/></ui><value><text maxChars="12"/></value></field>"#,
        r#"<field name="b"/>"#,
        r#"<field name="c"><ui><numericEdit/></ui></field>"#,
        r#"<field name="d"><value><decimal/></value></field>"#,
        r#"<field name="e"><ui><dateTimeEdit/></ui></field>"#,
        r#"<field name="f"><ui><defaultUi/></ui><value><date/></value></field>"#,
        r#"<field name="g"><ui><passwordEdit/></ui></field>"#,
        r#"<draw name="h"/>"#,
    ));
    let text = |multiline, max_chars| FieldWidget::Text {
        multiline,
        max_chars,
    };
    assert_eq!(kind(&t, 0), Some(text(true, Some(12))));
    assert_eq!(kind(&t, 1), Some(text(false, None)));
    assert_eq!(kind(&t, 2), Some(FieldWidget::Numeric));
    assert_eq!(kind(&t, 3), Some(FieldWidget::Numeric));
    assert_eq!(kind(&t, 4), Some(FieldWidget::Date));
    assert_eq!(kind(&t, 5), Some(FieldWidget::Date));
    assert_eq!(kind(&t, 6), Some(FieldWidget::Other("passwordEdit".into())));
    assert_eq!(kind(&t, 7), None, "a draw is not a field");
    assert_eq!(field_widget(&t, &[0, 99]), None);
}

#[test]
fn a_check_button_names_its_on_off_and_neutral_values() {
    let t = template(concat!(
        r#"<field name="a"><ui><checkButton/></ui><items><text>Y</text><text>N</text><text>?</text></items></field>"#,
        r#"<field name="b"><ui><checkButton/></ui></field>"#,
        r#"<field name="c"><ui><checkButton/></ui><items><text>Yes</text></items></field>"#,
        r#"<field name="d"><value><boolean/></value></field>"#,
        r#"<exclGroup name="g"><field name="r1"><ui><checkButton/></ui><items><text>A</text><text/><text>?</text></items></field></exclGroup>"#,
    ));
    let check = |on: &str, off: &str, neutral: Option<&str>, group: Option<Vec<usize>>| {
        FieldWidget::CheckButton {
            on: on.into(),
            off: off.into(),
            neutral: neutral.map(Into::into),
            group,
        }
    };
    assert_eq!(kind(&t, 0), Some(check("Y", "N", Some("?"), None)));
    assert_eq!(kind(&t, 1), Some(check("1", "0", None, None)));
    assert_eq!(
        kind(&t, 2),
        Some(check("Yes", "", None, None)),
        "a missing off is null (p.759)"
    );
    assert_eq!(kind(&t, 3), Some(check("1", "0", None, None)));
    assert_eq!(
        field_widget(&t, &[0, 4, 0]),
        Some(check("A", "", None, Some(vec![0, 4]))),
        "a radio button ignores its neutral value and names its group"
    );
    assert_eq!(
        field_widget(&t, &[0, 4]),
        None,
        "the group itself is not a field"
    );
}

#[test]
fn a_choice_list_pairs_display_and_save_items() {
    let t = template(concat!(
        r#"<field name="a"><ui><choiceList open="onEntry" textEntry="1"/></ui>"#,
        r#"<items><text>Red</text><text>Blue</text></items>"#,
        r#"<items save="1"><text>r</text><text>b</text></items></field>"#,
        r#"<field name="b"><ui><choiceList open="multiSelect"/></ui><items><text>x</text></items></field>"#,
    ));
    assert_eq!(
        kind(&t, 0),
        Some(FieldWidget::ChoiceList {
            items: vec![("Red".into(), "r".into()), ("Blue".into(), "b".into())],
            open: ChoiceOpen::OnEntry,
            text_entry: true,
        })
    );
    let Some(FieldWidget::ChoiceList {
        items,
        open,
        text_entry,
    }) = kind(&t, 1)
    else {
        panic!("a choice list");
    };
    assert_eq!(
        items,
        vec![("x".into(), "x".into())],
        "one list serves as both"
    );
    assert_eq!((open, text_entry), (ChoiceOpen::MultiSelect, false));
    assert!(!open.is_drop_down() && ChoiceOpen::default().is_drop_down());
}

#[test]
fn a_choice_lists_value_reads_back_as_its_selections() {
    // G304: the one rule the paint and fill paths share, published.
    let t = template(concat!(
        r#"<field name="m"><ui><choiceList open="multiSelect"/></ui>"#,
        r#"<items><text>New York</text><text>Pears</text><text>Bananas</text></items>"#,
        r#"<items save="1"><text>New York</text><text>pears</text><text>bananas</text></items></field>"#,
        r#"<field name="d"><ui><choiceList/></ui><items><text>New York</text></items></field>"#,
        r#"<field name="n"><ui><numericEdit/></ui></field>"#,
    ));
    let multi = kind(&t, 0).unwrap();
    let lines = ["pears", "bananas"].join("\n");
    assert_eq!(multi.selections(&lines), Some(vec!["pears", "bananas"]));
    assert_eq!(
        multi.selections("New York"),
        Some(vec!["New York"]),
        "a one-line save value is one selection, not split on its space"
    );
    assert_eq!(
        multi.selections("bananas pears"),
        Some(vec!["bananas", "pears"]),
        "a one-line value that is not a save value reads space-separated"
    );
    assert_eq!(multi.selections(""), Some(vec![]));
    let single = kind(&t, 1).unwrap();
    assert_eq!(single.selections("New York"), Some(vec!["New York"]));
    assert_eq!(single.selections(""), Some(vec![]));
    assert_eq!(kind(&t, 2).unwrap().selections("1"), None);
}
