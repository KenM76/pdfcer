//! `pdfcer xfa-fill` writes a value into an XFA form's data, reports which
//! other fields share it, and refuses an unbound path without writing.
//! Fixtures are assembled inline (docs/LEGAL.md §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A page set, two fields named `a` (both `match="global"`, sharing one
/// value) and `c`: template paths 0.1, 0.2 and 0.3.
const TEMPLATE: &str = r#"<subform name="form1" layout="tb"><pageSet><pageArea><contentArea w="200" h="200"/></pageArea></pageSet><field name="a" w="50" h="10"><bind match="global"/></field><field name="a" w="50" h="10"><bind match="global"/></field><field name="c" w="50" h="10"/></subform>"#;
const DATA: &str = "<form1><a>one</a><c>three</c></form1>";

fn pdf() -> Vec<u8> {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template>{TEMPLATE}</template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data>{DATA}</xfa:data></xfa:datasets></xdp:xdp>"#
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /XFA 4 0 R >> >>".to_owned(),
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

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer-xfa-fill-{}-{name}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> (u8, String) {
    let out = Command::new(BIN).args(args).output().unwrap();
    (
        u8::try_from(out.status.code().unwrap()).unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn a_fill_reads_back_and_names_the_fields_sharing_it() {
    let input = temp("in");
    let output = temp("out");
    std::fs::write(&input, pdf()).unwrap();
    let (code, stdout) = run(&[
        "xfa-fill",
        input.to_str().unwrap(),
        "--set",
        "0.1=new & <one>",
        "--set",
        "0.3=3",
        "-o",
        output.to_str().unwrap(),
        "--verify-undo",
    ]);
    assert_eq!(code, 0, "{stdout}");
    assert!(
        stdout.contains("xfa-set path=0.1 data=0.0 stream=4 shared_with=0.2"),
        "{stdout}"
    );
    assert!(
        stdout.contains("xfa-set path=0.3 data=0.1 stream=4 shared_with=-"),
        "{stdout}"
    );
    assert!(stdout.contains("undo_identical=1"), "{stdout}");

    let (code, layout) = run(&["xfa-layout", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{layout}");
    assert_eq!(
        layout.matches(r#"value="new & <one>""#).count(),
        2,
        "{layout}"
    );
    assert!(layout.contains(r#"value="3""#), "{layout}");
    let _ = std::fs::remove_file(input);
    let _ = std::fs::remove_file(output);
}

#[test]
fn an_unbound_or_malformed_path_is_refused_and_writes_nothing() {
    let input = temp("refuse-in");
    std::fs::write(&input, pdf()).unwrap();
    for set in ["0.9=x", "a=x", "0.0"] {
        let output = temp("refuse-out");
        let (code, stdout) = run(&[
            "xfa-fill",
            input.to_str().unwrap(),
            "--set",
            set,
            "-o",
            output.to_str().unwrap(),
        ]);
        assert_ne!(code, 0, "{set}: {stdout}");
        assert!(!output.exists(), "{set} wrote an output");
    }
    let _ = std::fs::remove_file(input);
}
