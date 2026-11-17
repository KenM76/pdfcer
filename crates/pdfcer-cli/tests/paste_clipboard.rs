//! # `pdfcer paste-clipboard` and `object-copy --clipboard`
//!
//! The argument refusals run everywhere. The round trip writes the OS
//! clipboard, so it runs only on Windows and only when
//! `PDFCER_CLIPBOARD_TESTS=1` is set, as `copy_page` does.
//!
//! What the round trip holds: a copy that has a vector form is pasted as
//! paths, never as an image, and the printed line says which form was
//! taken.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::copy_page::{TempDir, one_page, run, stderr};

fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn argument_refusals_fire_before_the_clipboard_is_touched() {
    let dir = TempDir::new("paste-refusals");
    let input = dir.write("one.pdf", &one_page());
    let i = input.to_str().unwrap();
    let out = dir.0.join("out.pdf");
    let out = out.to_str().unwrap();

    let o = run(&["paste-clipboard", i, "-o", out, "--contain", "--stretch"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));

    let o = run(&["paste-clipboard", i, "-o", out, "--format", "jpeg"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));

    // Neither a payload file nor the clipboard: nowhere to copy to.
    let o = run(&["object-copy", i, "--page", "1", "--objects", "0"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("--clip"), "{}", stderr(&o));

    let o = run(&[
        "object-copy",
        i,
        "--page",
        "1",
        "--objects",
        "0",
        "--clip",
        out,
        "--clipboard-dpi",
        "72",
    ]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(!std::path::Path::new(out).exists());
}

#[cfg(not(windows))]
#[test]
fn a_non_windows_build_refuses_by_name() {
    let dir = TempDir::new("paste-unsupported");
    let input = dir.write("one.pdf", &one_page());
    let out = dir.0.join("out.pdf");
    let o = run(&[
        "paste-clipboard",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("Windows"), "{}", stderr(&o));
    assert!(!out.exists());
}

/// What `object-list` reports for `pdf`, leaves inside forms included.
#[cfg(windows)]
fn kinds(pdf: &str) -> String {
    let o = run(&["object-list", pdf, "--page", "1"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    stdout(&o)
}

#[cfg(windows)]
#[test]
fn a_vector_copy_is_pasted_as_vectors_when_asked_for() {
    if std::env::var_os("PDFCER_CLIPBOARD_TESTS").is_none() {
        println!("paste_clipboard: PDFCER_CLIPBOARD_TESTS not set; the round trip did not run");
        return;
    }
    let _clipboard = super::copy_page::CLIPBOARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempDir::new("paste-roundtrip");
    let input = dir.write("one.pdf", &one_page());
    let i = input.to_str().unwrap();
    let path = |name: &str| dir.0.join(name).to_str().unwrap().to_owned();

    let o = run(&["copy-page", i, "--dpi", "72"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));

    // The best form held is the PDF, and it arrives as a path.
    let best = path("best.pdf");
    let o = run(&["paste-clipboard", i, "-o", &best]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let line = stdout(&o);
    assert!(line.contains("took=pdf "), "{line}");
    assert!(line.contains(" vector=1 "), "{line}");
    assert!(line.contains("shrunk=0"), "{line}");
    let listed = kinds(&best);
    assert!(listed.contains("kind=path"), "{listed}");
    assert!(!listed.contains("kind=image"), "{listed}");

    // Each vector form named outright stays a vector.
    for form in ["svg", "emf"] {
        let out = path(&format!("{form}.pdf"));
        let o = run(&["paste-clipboard", i, "--format", form, "-o", &out]);
        assert_eq!(o.status.code(), Some(0), "{form}: {}", stderr(&o));
        assert!(stdout(&o).contains(&format!("took={form} ")), "{form}");
        assert!(stdout(&o).contains(" vector=1 "), "{form}");
        let listed = kinds(&out);
        assert!(listed.contains("kind=path"), "{form}: {listed}");
        assert!(!listed.contains("kind=image"), "{form}: {listed}");
    }

    // A pixel form is an image only when it is asked for, and says so.
    let png = path("png.pdf");
    let o = run(&["paste-clipboard", i, "--format", "png", "-o", &png]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains(" vector=0 "), "{}", stdout(&o));
    assert!(stderr(&o).contains("--format png was asked for"));
    assert!(kinds(&png).contains("kind=image"));

    // Cut to the clipboard: the object leaves the document and what is
    // pasted elsewhere is a path.
    let cut = path("cut.pdf");
    let o = run(&[
        "object-copy",
        i,
        "--page",
        "1",
        "--objects",
        "0",
        "--clipboard",
        "--cut",
        &cut,
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("clipboard=1"), "{}", stdout(&o));
    assert!(!kinds(&cut).contains("kind=path"), "{}", kinds(&cut));
    let pasted = path("pasted.pdf");
    let o = run(&["paste-clipboard", &cut, "-o", &pasted]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("took=pdf "), "{}", stdout(&o));
    let listed = kinds(&pasted);
    assert!(listed.contains("kind=path"), "{listed}");
    assert!(!listed.contains("kind=image"), "{listed}");
}
