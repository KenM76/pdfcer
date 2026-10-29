//! `pdfcer xfa-layout` prints the pages laid from an XFA template, with its
//! warnings and settings. Fixtures are assembled inline (docs/LEGAL.md §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page PDF whose `/AcroForm /XFA` stream holds `template` (with no
/// `/XFA` at all when `None`).
fn pdf(template: Option<&str>) -> Vec<u8> {
    let xfa = template.map_or_else(String::new, |_| {
        " /AcroForm << /Fields [] /XFA 4 0 R >>".into()
    });
    let xdp = template.map_or_else(String::new, |t| {
        format!(
            r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template>{t}</template></xdp:xdp>"#
        )
    });
    let objects = [
        format!("<< /Type /Catalog /Pages 2 0 R /NeedsRendering true{xfa} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{xdp}\nendstream", xdp.len()),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn run(name: &str, bytes: &[u8], extra: &[&str]) -> (u8, String, String) {
    let path = std::env::temp_dir().join(format!(
        "pdfcer-xfa-layout-{}-{name}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new(BIN)
        .arg("xfa-layout")
        .arg(&path)
        .args(extra)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    (
        u8::try_from(out.status.code().unwrap()).unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

/// `a` (500pt) then `b` (200pt, keep next by page) then `c`, flowed.
fn flowed() -> String {
    format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="a" w="10pt" h="500pt"/><field name="b" w="10pt" h="200pt"><keep next="pageArea"/></field><field name="c" w="10pt" h="100pt"/></subform>"#
    )
}

#[test]
fn prints_pages_items_and_a_summary_and_discloses_the_inference() {
    let (code, stdout, stderr) = run("basic", &pdf(Some(&flowed())), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains(r#"page index=0 name="P" width=612 height=792"#),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#"item page=0 kind=field name="a" path=0.1 x=36 y=36 w=10 h=500 area=0"#),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#"item page=1 kind=field name="b""#),
        "{stdout}"
    );
    assert!(
        stdout.contains("warning kind=page-area-keep path=0.2"),
        "{stdout}"
    );
    assert!(
        stdout.ends_with("xfa-layout pages=2 items=3 warnings=1\n"),
        "{stdout}"
    );
    assert!(stderr.contains("not stored in the file"), "{stderr}");
}

#[test]
fn a_setting_changes_the_layout() {
    let spec = run("spec", &pdf(Some(&flowed())), &[]).1;
    let adobe = run(
        "adobe",
        &pdf(Some(&flowed())),
        &["--page-area-keep", "adobe"],
    )
    .1;
    assert!(
        spec.contains(r#"item page=1 kind=field name="b""#),
        "{spec}"
    );
    assert!(
        adobe.contains(r#"item page=1 kind=field name="c""#),
        "{adobe}"
    );
    assert!(
        adobe.contains(r#"item page=0 kind=field name="b""#),
        "{adobe}"
    );
}

#[test]
fn png_dir_writes_one_image_per_page_and_lists_what_was_not_drawn() {
    let dir = std::env::temp_dir().join(format!("pdfcer-xfa-png-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let t = format!(
        r#"<subform name="f">{PAGE}<field name="f1" x="10pt" y="10pt" w="40pt" h="40pt"><border><fill><color value="255,0,0"/></fill></border><caption placement="inline"/></field></subform>"#
    );
    let dir_arg = dir.to_str().unwrap();
    let (code, stdout, stderr) = run("png", &pdf(Some(&t)), &["--png-dir", dir_arg]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("rendered page=0 ") && stdout.contains("width=612 height=792"),
        "{stdout}"
    );
    assert!(
        stdout.contains("paint-note page=0 kind=content-not-drawn path=0.1"),
        "{stdout}"
    );
    assert!(stdout.ends_with("xfa-render pages=1 notes=1\n"), "{stdout}");
    let png = std::fs::read(dir.join("page-000.png")).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn png_dir_draws_value_text_and_names_a_substituted_font() {
    let dir = std::env::temp_dir().join(format!("pdfcer-xfa-text-{}", std::process::id()));
    let t = format!(
        r#"<subform name="f">{PAGE}<draw name="d" x="10pt" y="10pt" w="100pt" h="20pt"><font typeface="Arial"/><value><text>Hi</text></value></draw></subform>"#
    );
    let dir_arg = dir.to_str().unwrap();
    let (code, stdout, stderr) = run("text", &pdf(Some(&t)), &["--png-dir", dir_arg]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("paint-note page=0 kind=font-substituted path=0.1"),
        "{stdout}"
    );
    assert!(stdout.ends_with("xfa-render pages=1 notes=1\n"), "{stdout}");
}

#[test]
fn png_dir_draws_a_caption_in_the_field_font_when_asked() {
    let t = format!(
        r#"<subform name="f">{PAGE}<field name="c" x="10pt" y="10pt" w="200pt" h="40pt"><caption reserve="120pt"><value><text>Name</text></value></caption><font typeface="Arial" size="24pt"/><ui><textEdit/></ui><value><text>v</text></value></field></subform>"#
    );
    let image = |caption_font: &str| {
        let dir = std::env::temp_dir().join(format!(
            "pdfcer-xfa-cap-{caption_font}-{}",
            std::process::id()
        ));
        let dir_arg = dir.to_str().unwrap();
        let flags = ["--png-dir", dir_arg, "--caption-font", caption_font];
        let (code, stdout, stderr) = run(caption_font, &pdf(Some(&t)), &flags);
        let png = std::fs::read(dir.join("page-000.png"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(code, 0, "{stderr}");
        assert!(!stdout.contains("content-not-drawn"), "{stdout}");
        assert!(
            stdout.contains("paint-note page=0 kind=font-substituted path=0.1"),
            "{stdout}"
        );
        png.unwrap()
    };
    assert_ne!(
        image("own"),
        image("field"),
        "a 24pt Helvetica caption draws differently from a 10pt Courier one"
    );
}

#[test]
fn a_file_with_no_xfa_form_exits_one_by_name() {
    let (code, stdout, stderr) = run("none", &pdf(None), &[]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("has no XFA form"), "{stderr}");
}
