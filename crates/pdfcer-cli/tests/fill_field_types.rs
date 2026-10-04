//! `pdfcer fill-field` on non-text fields: check-box aliases resolve to the
//! box's own on-state, and a multi-select list takes `|`-separated values.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `Agree`: a check box whose only on-state is `Accept` (not the
/// conventional `Yes`). `Colours`: a multi-select list box.
fn form_pdf() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 6 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 6 0 R] >>",
        "<< /FT /Btn /T (Agree) /V /Off /AS /Off /Type /Annot /Subtype /Widget \
/Rect [10 10 30 30] /P 3 0 R /AP << /N << /Accept 5 0 R /Off 5 0 R >> >> >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream",
        "<< /FT /Ch /Ff 2097152 /T (Colours) /Opt [(Red) (Green) (Blue)] /Type /Annot \
/Subtype /Widget /Rect [10 40 90 90] /P 3 0 R >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pdfcer_fill_types_{tag}_{}.pdf",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> String {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Fills `input` with `sets` into `output` and returns `name`'s `value=`.
fn fill_and_read(input: &Path, output: &Path, sets: &[&str], name: &str) -> String {
    let mut args = vec!["fill-field", input.to_str().unwrap()];
    for s in sets {
        args.extend(["--set", s]);
    }
    args.extend(["-o", output.to_str().unwrap()]);
    run(&args);
    let listing = run(&["list-fields", output.to_str().unwrap()]);
    let key = format!("field name=\"{name}\" ");
    let line = listing
        .lines()
        .find(|l| l.starts_with(&key))
        .unwrap_or_else(|| panic!("no line for {name}: {listing}"));
    let v = line.split(" value=").nth(1).unwrap();
    v.split(" widgets=").next().unwrap().to_owned()
}

#[test]
fn every_on_alias_selects_the_boxes_own_on_state_and_every_off_alias_clears_it() {
    let input = scratch("cb_in");
    let on = scratch("cb_on");
    let off = scratch("cb_off");
    std::fs::write(&input, form_pdf()).unwrap();
    for alias in ["on", "TRUE", "1", "yes", "checked"] {
        let set = format!("Agree={alias}");
        assert_eq!(
            fill_and_read(&input, &on, &[&set], "Agree"),
            "\"Accept\"",
            "{alias}"
        );
    }
    for alias in ["off", "false", "0", "no", "unchecked", ""] {
        let set = format!("Agree={alias}");
        assert_eq!(
            fill_and_read(&on, &off, &[&set], "Agree"),
            "\"Off\"",
            "{alias:?}"
        );
    }
    for p in [&input, &on, &off] {
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn a_multi_select_list_takes_pipe_separated_values() {
    let input = scratch("ch_in");
    let output = scratch("ch_out");
    std::fs::write(&input, form_pdf()).unwrap();
    assert_eq!(
        fill_and_read(&input, &output, &["Colours=Red|Blue"], "Colours"),
        "\"Red, Blue\""
    );
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}
