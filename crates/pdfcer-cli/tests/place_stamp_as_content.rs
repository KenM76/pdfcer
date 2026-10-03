//! `place-stamp --as-content`: the artwork drawn in the page's content, no
//! `/Stamp` annotation, the new content stream named on stdout.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn one_page(media: &str, body: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [{media}] /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-place-content-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

/// Runs `place-stamp` with `extra`, returning (exit code, stdout, output bytes).
fn place(tag: &str, extra: &[&str]) -> (i32, String, Vec<u8>) {
    let src = temp(&format!("{tag}-src.pdf"));
    let input = temp(&format!("{tag}-in.pdf"));
    let out = temp(&format!("{tag}-out.pdf"));
    std::fs::write(
        &src,
        one_page("0 0 144 72", "1 0 0 RG 4 w 10 10 m 130 60 l S"),
    )
    .unwrap();
    std::fs::write(&input, one_page("0 0 612 792", "0 0 1 rg 0 0 50 50 re f")).unwrap();
    let mut args = vec![
        "place-stamp".to_owned(),
        input.display().to_string(),
        "--from".into(),
        src.display().to_string(),
        "--stamp-page".into(),
        "1".into(),
        "--page".into(),
        "1".into(),
        "--at".into(),
        "100,100".into(),
        "-o".into(),
        out.display().to_string(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let run = Command::new(BIN).args(&args).output().expect("pdfcer runs");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let bytes = std::fs::read(&out).unwrap_or_default();
    (run.status.code().unwrap_or(-1), stdout, bytes)
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn as_content_draws_in_the_page_and_writes_no_stamp() {
    let (code, stdout, bytes) = place("content", &["--as-content"]);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("; content="), "{stdout}");
    assert!(stdout.contains("distorted=0"), "{stdout}");
    assert!(!has(&bytes, b"/Stamp"), "no stamp annotation");
    assert!(has(&bytes, b" Do"), "the page invokes the form");
}

#[test]
fn without_the_flag_it_is_still_a_stamp() {
    let (code, stdout, bytes) = place("stamp", &[]);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("; obj="), "{stdout}");
    assert!(has(&bytes, b"/Stamp"));
}
