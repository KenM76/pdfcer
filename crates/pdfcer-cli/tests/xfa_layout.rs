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

/// A `w` by `h` 24-bit red BMP, recording no resolution, in base64.
fn red_bmp_base64(w: u32, h: u32) -> String {
    let stride = (w * 3).div_ceil(4) * 4;
    let mut b = b"BM".to_vec();
    for v in [54 + stride * h, 0, 54, 40, w, h] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&[1, 0, 24, 0]);
    for v in [0, stride * h, 0, 0, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for _ in 0..h {
        let mut row: Vec<u8> = (0..w).flat_map(|_| [0, 0, 255]).collect();
        row.resize(usize::try_from(stride).unwrap(), 0);
        b.extend_from_slice(&row);
    }
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, v)| n | (u32::from(*v) << (16 - 8 * i)));
        for i in 0..=c.len() {
            out.push(char::from(
                *A.get(((n >> (18 - 6 * i)) & 63) as usize).unwrap(),
            ));
        }
    }
    out
}

/// The page-0 PNG `xfa-layout --png-dir` draws for `t` under `flags`, and
/// its stdout.
fn image_run(t: &str, name: &str, flags: &[&str]) -> (Vec<u8>, String) {
    let dir = std::env::temp_dir().join(format!("pdfcer-xfa-img-{name}-{}", std::process::id()));
    let dir_arg = dir.to_str().unwrap();
    let mut all = vec!["--png-dir", dir_arg];
    all.extend_from_slice(flags);
    let (code, stdout, stderr) = run(name, &pdf(Some(t)), &all);
    let png = std::fs::read(dir.join("page-000.png"));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 0, "{stderr}");
    (png.unwrap(), stdout)
}

#[test]
fn png_dir_draws_an_inline_image_under_each_image_setting() {
    let img = red_bmp_base64(20, 10);
    let small = format!(
        r#"<subform name="f">{PAGE}<field name="i" x="10pt" y="10pt" w="100pt" h="40pt"><ui><imageEdit/></ui><para hAlign="right"/><value><image aspect="actual">{img}</image></value></field></subform>"#
    );
    let (base, stdout) = image_run(&small, "base", &[]);
    assert!(stdout.ends_with("xfa-render pages=1 notes=0\n"), "{stdout}");
    let (para, _) = image_run(&small, "para", &["--image-placement", "para"]);
    assert_ne!(base, para, "para alignment moves the image right");
    let (dpi, _) = image_run(&small, "dpi", &["--image-dpi", "144"]);
    assert_ne!(base, dpi, "144 dpi halves its natural size");
    let big = small.replace(r#"w="100pt" h="40pt""#, r#"w="10pt" h="5pt""#);
    let (crop, _) = image_run(&big, "crop", &[]);
    let (show, _) = image_run(&big, "show", &["--image-overflow", "show"]);
    assert_ne!(crop, show, "show draws past the box");
}

#[test]
fn png_dir_names_an_image_link_the_file_does_not_hold() {
    let t = format!(
        r#"<subform name="f">{PAGE}<field name="i" x="10pt" y="10pt" w="100pt" h="40pt"><ui><imageEdit/></ui><value><image href="logo.png"/></value></field></subform>"#
    );
    let (_, stdout) = image_run(&t, "href", &[]);
    assert!(
        stdout.contains("paint-note page=0 kind=image-not-drawn path=0.1"),
        "{stdout}"
    );
    assert!(stdout.ends_with("xfa-render pages=1 notes=1\n"), "{stdout}");
}

/// Three X020-shaped rows: an `exclGroup` of two round radio buttons with a
/// `stroke` edge, `minW` and no `w`, and a negative caption reserve.
fn radio_rows(stroke: &str) -> String {
    let radio = |name: &str, x: &str, on: &str| {
        format!(
            r#"<field name="{name}" x="{x}" y="0pt" minW="20mm" h="6mm"><ui><checkButton shape="round"><border><edge stroke="{stroke}"/><fill/></border></checkButton></ui><para hAlign="center"/><caption reserve="-5.08mm"><value><text>{name}</text></value></caption><items><text>{on}</text></items></field>"#
        )
    };
    let rows: String = (0..3)
        .map(|i| {
            format!(
                r#"<exclGroup name="g{i}" x="10pt" y="{}pt"><bind match="none"/>{}{}</exclGroup>"#,
                20 + 30 * i,
                radio("Yes", "0pt", "1"),
                radio("No", "30mm", "2")
            )
        })
        .collect();
    format!(r#"<subform name="data">{PAGE}{rows}</subform>"#)
}

/// Non-white pixels in the first Yes button's box (x 78-89pt, y 55-67pt from
/// the top of the 612x792 page at scale 1).
fn inked_in_first_box(dir: &std::path::Path) -> usize {
    let bytes = std::fs::read(dir.join("page-000.png")).unwrap();
    let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let px = info.color_type.samples();
    let mut n = 0;
    for y in 55..67 {
        for x in 78..89 {
            let i = (y * info.width as usize + x) * px;
            if buf[i..i + 3].iter().any(|&c| c < 250) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn png_dir_draws_round_radio_buttons_with_three_d_edges() {
    let base = std::env::temp_dir().join(format!("pdfcer-xfa-radio-{}", std::process::id()));
    let drawn = |stroke: &str, flags: &[&str]| {
        let dir = base.join(stroke).join(flags.len().to_string());
        let _ = std::fs::remove_dir_all(&dir);
        let mut args = vec!["--png-dir", dir.to_str().unwrap()];
        args.extend_from_slice(flags);
        let (code, stdout, stderr) = run("radio", &pdf(Some(&radio_rows(stroke))), &args);
        assert_eq!(code, 0, "{stderr}");
        (stdout, inked_in_first_box(&dir))
    };
    let (stdout, ink) = drawn("lowered", &[]);
    assert!(!stdout.contains("stroke-not-drawn"), "{stdout}");
    assert!(stdout.ends_with("xfa-render pages=1 notes=0\n"), "{stdout}");
    assert!(ink > 20, "a bevelled circle: {ink}");
    let (stdout, ink) = drawn("lowered", &["--three-d-stroke", "plain"]);
    assert_eq!(
        stdout.matches("kind=stroke-approximated").count(),
        6,
        "{stdout}"
    );
    assert!(ink > 20, "a plain circle: {ink}");
    let (stdout, ink) = drawn("dashed", &[]);
    assert_eq!(
        stdout.matches("kind=stroke-not-drawn").count(),
        6,
        "{stdout}"
    );
    assert_eq!(ink, 0, "a dashed edge is still left out");
    let _ = std::fs::remove_dir_all(&base);
}
