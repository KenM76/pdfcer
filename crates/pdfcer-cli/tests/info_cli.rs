//! `pdfcer set-info --set/--remove` and `list-info` against the real binary,
//! on a synthetic file with an XMP packet: custom entries round-trip, the
//! packet follows, and every skipped packet update is disclosed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A one-page file: `/Info` 4 holds `/Title (Old)`, and the catalog's
/// `/Metadata` 5 is an XMP packet carrying `extra` on its description.
fn sample(extra: &str) -> Vec<u8> {
    let xmp = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"{extra}>\n\
<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Old</rdf:li></rdf:Alt></dc:title>\n\
</rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    );
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /Metadata 5 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        "<< /Title (Old) >>".to_owned(),
        format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xmp}\nendstream",
            xmp.len()
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pdfcer_info_cli_{tag}_{}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn metric(line: &str, name: &str) -> u64 {
    let key = format!(" {name}=");
    let at = line
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} in {line}"))
        + key.len();
    line[at..]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// Run `set-info <in> <args> -o <out>`.
fn set_info(input: &Path, output: &Path, args: &[&str]) -> Output {
    let mut all = vec!["set-info", input.to_str().unwrap()];
    all.extend_from_slice(args);
    all.extend_from_slice(&["-o", output.to_str().unwrap()]);
    run(&all)
}

fn list(path: &Path) -> String {
    let out = run(&["list-info", path.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    stdout(&out)
}

#[test]
fn a_custom_entry_is_added_edited_and_removed_with_the_packet_in_step() {
    let a = scratch("a");
    let b = scratch("b");
    std::fs::write(&a, sample("")).unwrap();

    let out = set_info(&a, &b, &["--set", "Dept=Tooling", "--title", "New"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let line = stdout(&out);
    assert_eq!(metric(&line, "info_edits"), 2);
    assert_eq!(metric(&line, "xmp_updated"), 2, "{line}");
    assert_eq!(metric(&line, "xmp_skipped"), 0);
    let saved = std::fs::read(&b).unwrap();
    let text = String::from_utf8_lossy(&saved);
    assert!(
        text.contains(
            "<pdfx:Dept xmlns:pdfx=\"http://ns.adobe.com/pdfx/1.3/\">Tooling</pdfx:Dept>"
        )
    );
    assert!(text.contains(">New</rdf:li>"));
    let listed = list(&b);
    assert!(
        listed
            .contains("{\"key\":\"Dept\",\"kind\":\"text\",\"value\":\"Tooling\",\"exact\":true}"),
        "{listed}"
    );
    assert!(listed.contains("\"value\":\"New\""));

    let out = set_info(&b, &a, &["--set", "Dept=Dies"]);
    assert_eq!(metric(&stdout(&out), "xmp_updated"), 1);
    assert!(list(&a).contains("\"value\":\"Dies\""));

    let out = set_info(&a, &b, &["--remove", "Dept", "--set", "Dept=ignored"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!list(&b).contains("Dept"), "removal wins over --set");
    let text = String::from_utf8_lossy(&std::fs::read(&b).unwrap()).into_owned();
    let last_packet = &text[text.rfind("<?xpacket begin").unwrap()..];
    assert!(!last_packet.contains("pdfx:Dept>"));

    for p in [a, b] {
        std::fs::remove_file(p).ok();
    }
}

#[test]
fn a_pdfa_file_keeps_custom_keys_out_of_its_packet_and_says_so() {
    let a = scratch("pdfa_in");
    let b = scratch("pdfa_out");
    let pdfa = " xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"2\"";
    std::fs::write(&a, sample(pdfa)).unwrap();
    let out = set_info(&a, &b, &["--set", "Dept=Tooling"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(metric(&stdout(&out), "xmp_skipped"), 1);
    assert!(
        stderr(&out).contains("pdfx:Dept not updated (pdfa-custom-key)"),
        "{}",
        stderr(&out)
    );
    assert!(
        list(&b).contains("\"value\":\"Tooling\""),
        "/Info still written"
    );
    for p in [a, b] {
        std::fs::remove_file(p).ok();
    }
}

#[test]
fn reserved_keys_are_refused_and_xmp_off_is_disclosed() {
    let a = scratch("res_in");
    let b = scratch("res_out");
    std::fs::write(&a, sample("")).unwrap();
    let out = set_info(&a, &b, &["--set", "Producer=x"]);
    assert_eq!(out.status.code(), Some(9), "{}", stderr(&out));
    assert!(stderr(&out).contains("Producer"));
    assert!(!b.exists(), "a refused edit writes nothing");

    let out = set_info(&a, &b, &["--creator", "CAD", "--xmp", "off"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("xmp:CreatorTool not updated (off)"));
    for p in [a, b] {
        std::fs::remove_file(p).ok();
    }
}
