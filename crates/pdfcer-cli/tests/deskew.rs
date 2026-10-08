//! `deskew` (pdfcer-gui request G165): a dry run reports, a real run
//! straightens the skewed page and skips the straight one.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const W: u32 = 600;
const H: u32 = 400;

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-deskew-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

/// Dark "text lines" rising to the right by `angle` degrees on white.
fn skewed_grey(angle: f64) -> Vec<u8> {
    let tan = angle.to_radians().tan();
    let mut grey = vec![255u8; (W * H) as usize];
    for y in 0..H {
        for x in 30..570 {
            let y0 = f64::from(y) + f64::from(x) * tan;
            if (40.0..360.0).contains(&y0) && (y0 as u32 % 24) < 5 {
                grey[(y * W + x) as usize] = 0;
            }
        }
    }
    grey
}

/// Two pages, each one full-page grey scan: page 1 skewed 2°, page 2 straight.
fn fixture(name: &str) -> PathBuf {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |buf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut buf, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut buf, b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>");
    let content = format!("q {W} 0 0 {H} 0 0 cm /Scan Do Q\n");
    for (page, angle) in [(3, 2.0), (6, 0.0)] {
        obj(
            &mut buf,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources \
                 << /XObject << /Scan {} 0 R >> >> /Contents {} 0 R >>",
                page + 2,
                page + 1
            )
            .as_bytes(),
        );
        obj(
            &mut buf,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            )
            .as_bytes(),
        );
        let samples = skewed_grey(angle);
        let mut scan = format!(
            "<< /Type /XObject /Subtype /Image /Width {W} /Height {H} /BitsPerComponent 8 \
             /ColorSpace /DeviceGray /Length {} >>\nstream\n",
            samples.len()
        )
        .into_bytes();
        scan.extend_from_slice(&samples);
        scan.extend_from_slice(b"\nendstream");
        obj(&mut buf, &scan);
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = temp_out(name);
    std::fs::write(&path, buf).expect("fixture written");
    path
}

#[test]
fn a_dry_run_reports_without_writing() {
    let input = fixture("dry.pdf");
    let (code, stdout, stderr) = run(&["deskew", s(&input)]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    // About 1 px over a 540 px line: within 0.1°.
    assert!(
        stdout.contains("page 1 object 0: skew=+1.9")
            || stdout.contains("page 1 object 0: skew=+2.0"),
        "{stdout}"
    );
    assert!(stdout.contains("would correct"), "{stdout}");
    assert!(stdout.contains("page 2 object 0: skew=+0.00"), "{stdout}");
    assert!(stdout.contains("would_correct=1 skipped=1"), "{stdout}");
}

#[test]
fn the_skewed_page_is_straightened_and_the_straight_one_skipped() {
    let input = fixture("in-straight.pdf");
    let out = temp_out("straight.pdf");
    let (code, stdout, stderr) = run(&["deskew", s(&input), "-o", s(&out)]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("corrected=1 skipped=1"), "{stdout}");
    assert!(
        stderr.contains("resampled"),
        "the resample is disclosed: {stderr}"
    );

    let (code, stdout, _) = run(&["deskew", s(&out), "--pages", "1"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("would_correct=0 skipped=1"), "{stdout}");
}

#[test]
fn a_given_angle_is_applied_and_an_impossible_one_refused() {
    let input = fixture("in-given.pdf");
    let out = temp_out("given.pdf");
    let (code, stdout, stderr) = run(&[
        "deskew",
        s(&input),
        "--pages",
        "1",
        "--angle",
        "2",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("angle=+2.00 (given)"), "{stdout}");

    let refused = temp_out("refused.pdf");
    let (code, _, stderr) = run(&[
        "deskew",
        s(&input),
        "--pages",
        "1",
        "--object",
        "0",
        "--angle",
        "40",
        "-o",
        s(&refused),
    ]);
    assert_eq!(code, 9, "{stderr}");
    assert!(stderr.contains("cannot be deskewed"), "{stderr}");
    assert!(!refused.exists());
}
