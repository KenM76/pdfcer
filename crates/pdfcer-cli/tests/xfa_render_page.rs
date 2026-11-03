//! `pdfcer render-page` on a dynamic XFA form draws pdfcer's computed page
//! by default and the stored placeholder page with `--dynamic-xfa shell`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use super::xfa_layout::{flowed, pdf_with_data};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn render(name: &str, extra: &[&str]) -> (u8, String, String) {
    let dir = std::env::temp_dir().join(format!(
        "pdfcer-xfa-render-page-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("in.pdf");
    std::fs::write(&input, pdf_with_data(Some(&flowed()), "")).unwrap();
    let out = Command::new(BIN)
        .arg("render-page")
        .arg(&input)
        .args(["-o"])
        .arg(dir.join("out.png"))
        .args(extra)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (
        u8::try_from(out.status.code().unwrap()).unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_dynamic_form_renders_its_computed_page_and_says_so() {
    let (code, stdout, stderr) = render("layout", &["--page", "2"]);
    assert_eq!(code, 0, "{stderr}");
    // The computed page is the template's 612x792 page area, not the
    // 200x200 placeholder, and the form lays out two pages.
    assert!(stdout.starts_with("rendered-xfa "), "{stdout}");
    assert!(
        stdout.contains(" page 2 -> ") && stdout.contains(" 612x792; pages=2 notes="),
        "{stdout}"
    );
    assert!(stderr.contains("dynamic XFA form"), "{stderr}");
    assert!(stderr.contains("not a page stored in the file"), "{stderr}");
}

#[test]
fn shell_renders_the_stored_placeholder_page() {
    let (code, stdout, stderr) = render("shell", &["--dynamic-xfa", "shell"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.starts_with("rendered "), "{stdout}");
    assert!(stdout.contains(" 200x200; "), "{stdout}");
    assert!(!stderr.contains("dynamic XFA form"), "{stderr}");
}

#[test]
fn a_page_past_the_computed_layout_is_out_of_range() {
    let (code, _, stderr) = render("range", &["--page", "3"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("lays out 2 page(s)"), "{stderr}");
}

#[test]
fn region_needs_the_stored_page() {
    let (code, _, stderr) = render("region", &["--region", "0,0,10,10"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("--dynamic-xfa shell"), "{stderr}");
}
