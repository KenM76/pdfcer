//! `pdfcer inspect --text-blocks` / `--reflow-preview` and `reflow` over a
//! ruled table: each cell is reported as a `table-cell` block with its
//! table/row/column, and a re-wrap that outgrows its cell is reported.
//! Fixture: `fixtures/synthetic/textblocks/ruled-table.pdf` (that
//! directory's `PROVENANCE.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textblocks/ruled-table.pdf")
}

fn inspect(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("inspect")
        .arg(fixture())
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn every_cell_is_a_table_cell_block() {
    let out = inspect(&["--text-blocks"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert_eq!(text.matches("kind=table-cell").count(), 4, "{text}");
    for cell in ["cell=t0r0c0", "cell=t0r0c1", "cell=t0r1c0", "cell=t0r1c1"] {
        assert!(text.contains(cell), "{cell} missing: {text}");
    }
    // Five lines: no line runs across a row's two cells.
    assert!(text.contains("lines=5 blocks=4"), "{text}");
    assert!(text.contains("table_cell_blocks=4"), "{text}");
}

#[test]
fn json_blocks_carry_their_cell() {
    let out = inspect(&["--text-blocks", "--json"]);
    assert!(out.status.success());
    let json = stdout(&out);
    assert!(json.contains("\"kind\": \"table-cell\""), "{json}");
    assert!(
        json.contains("\"cell\": {\"table\": 0, \"row\": 0, \"column\": 0, \"rect\": ["),
        "{json}"
    );
    assert!(json.contains("\"table_cell_blocks\": 4"), "{json}");
}

#[test]
fn a_preview_that_outgrows_its_cell_reports_it() {
    let fits = inspect(&["--reflow-preview", "--block", "0"]);
    assert!(fits.status.success());
    assert!(
        stdout(&fits).contains("cell_overflow=0"),
        "{}",
        stdout(&fits)
    );

    let out = inspect(&["--reflow-preview", "--block", "0", "--width", "30"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("cell_overflow=1"), "{text}");
    assert!(text.contains("cell_overflow: past_bottom="), "{text}");
}

#[test]
fn reflow_reports_a_cell_overflow_and_writes() {
    let dir = std::env::temp_dir().join(format!("pdfcer-cells-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let output = dir.join("out.pdf");
    let out = Command::new(BIN)
        .arg("reflow")
        .arg(fixture())
        .args(["--block", "0", "--width", "30", "-o"])
        .arg(&output)
        .output()
        .expect("the binary runs");
    let all = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{all}");
    assert!(all.contains("cell_overflow: past_bottom="), "{all}");
    assert!(all.contains("(cell not resized)"), "{all}");
    assert!(output.is_file());
    std::fs::remove_file(&output).expect("remove output");
    std::fs::remove_dir(&dir).expect("remove temp dir");
}
