//! `add-sound` (`Pass 261.2`): a WAV embedded as a sound annotation, with
//! every conversion and the PDF 2.0 deprecation reported.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-add-sound-tests-{}", std::process::id()));
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

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

/// A mono WAV with the given format tag, rate and width.
fn wav(name: &str, tag: u16, rate: u32, bits: u16, data: &[u8]) -> PathBuf {
    let align = bits / 8;
    let mut b = b"RIFF".to_vec();
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt \x10\0\0\0");
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    b.extend_from_slice(&align.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(data);
    let p = temp_out(name);
    std::fs::write(&p, b).expect("wav written");
    p
}

#[test]
fn a_float_wav_is_converted_embedded_and_reported() {
    let data: Vec<u8> = [0.25f32, -0.25, 0.5, -0.5]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let file = wav("float.wav", 3, 44_100, 32, &data);
    let out = temp_out("sound.pdf");
    let (code, stdout, stderr) = run(&[
        "add-sound",
        s(&fixture()),
        "--file",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,700,92,720",
        "--icon",
        "mic",
        "--desc",
        "narration",
        "--rate",
        "spec",
        "--apply",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    for needle in [
        "rate=22050",
        "bits=16",
        "encoding=Signed",
        "icon=Mic",
        "32-bit float samples to 16-bit signed",
        "resampled 44100 Hz to 22050 Hz",
    ] {
        assert!(stdout.contains(needle), "missing {needle}: {stdout}");
    }
    assert!(stderr.contains("deprecated in PDF 2.0"), "{stderr}");
    let text = String::from_utf8_lossy(&std::fs::read(&out).expect("output written")).into_owned();
    for needle in [
        "/Subtype /Sound",
        "/Type /Sound",
        "/Name /Mic",
        "/E /Signed",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn a_dry_run_writes_nothing_and_a_bad_wav_is_refused() {
    let file = wav("pcm.wav", 1, 8_000, 8, &[128, 129, 130]);
    let out = temp_out("dry.pdf");
    let (code, stdout, stderr) = run(&[
        "add-sound",
        s(&fixture()),
        "--file",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,700,92,720",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("encoding=Raw")
            && stdout.contains("applied=0")
            && !stdout.contains("converted:"),
        "{stdout}"
    );
    assert!(!out.exists());

    let bad = temp_out("bad.wav");
    std::fs::write(&bad, b"not a wav").expect("written");
    let input = fixture();
    let (code, _, stderr) = run(&[
        "add-sound",
        s(&input),
        "--file",
        s(&bad),
        "--page",
        "1",
        "--rect",
        "72,700,92,720",
    ]);
    assert_eq!(code, 9, "{stderr}");
    assert!(stderr.contains("not a WAV file"), "{stderr}");
}
