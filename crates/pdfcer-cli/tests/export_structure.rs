//! `pdfcer export-docx/-xlsx/-ods --structure` (G066): a tagged PDF's own
//! structure tree drives the export, and the result line says so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn build_pdf(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => (*body).to_owned(),
        };
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A heading, a paragraph, a list item, a non-standard `Blurb`, a 3x2
/// table with a header row and a two-column total, and an artifact page
/// number. No rules are drawn, so layout detection finds no ruled table.
const CONTENT: &str = "STREAM:\
/H1 <</MCID 0>> BDC BT /F1 18 Tf 72 720 Td (Quarterly Report) Tj ET EMC\n\
/P <</MCID 1>> BDC BT /F1 11 Tf 72 690 Td (Sales rose in every region.) Tj ET EMC\n\
/Lbl <</MCID 2>> BDC BT /F1 11 Tf 72 665 Td (1.) Tj ET EMC\n\
/LBody <</MCID 3>> BDC BT /F1 11 Tf 90 665 Td (First item) Tj ET EMC\n\
/Blurb <</MCID 4>> BDC BT /F1 11 Tf 72 640 Td (A styled note) Tj ET EMC\n\
/TH <</MCID 5>> BDC BT /F1 11 Tf 72 600 Td (Name) Tj ET EMC\n\
/TH <</MCID 6>> BDC BT /F1 11 Tf 250 600 Td (Qty) Tj ET EMC\n\
/TD <</MCID 7>> BDC BT /F1 11 Tf 72 580 Td (Widget) Tj ET EMC\n\
/TD <</MCID 8>> BDC BT /F1 11 Tf 250 580 Td (12) Tj ET EMC\n\
/TD <</MCID 9>> BDC BT /F1 11 Tf 72 560 Td (Total 12) Tj ET EMC\n\
/Artifact BMC BT /F1 9 Tf 300 40 Td (Page 1) Tj ET EMC";

const HEAD: [&str; 5] = [
    "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R /MarkInfo << /Marked true >> >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
     /Resources << /Font << /F1 5 0 R >> >> >>",
    CONTENT,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
];

const TREE: [&str; 20] = [
    "<< /Type /StructTreeRoot /K 7 0 R >>",
    "<< /S /Document /P 6 0 R /K [8 0 R 9 0 R 10 0 R 14 0 R 15 0 R] >>",
    "<< /S /H1 /P 7 0 R /Pg 3 0 R /K 0 >>",
    "<< /S /P /P 7 0 R /Pg 3 0 R /K 1 >>",
    "<< /S /L /P 7 0 R /K 11 0 R >>",
    "<< /S /LI /P 10 0 R /K [12 0 R 13 0 R] >>",
    "<< /S /Lbl /P 11 0 R /Pg 3 0 R /K 2 >>",
    "<< /S /LBody /P 11 0 R /Pg 3 0 R /K 3 >>",
    "<< /S /Blurb /P 7 0 R /Pg 3 0 R /K 4 >>",
    "<< /S /Table /P 7 0 R /K [16 0 R 20 0 R] >>",
    "<< /S /THead /P 15 0 R /K 17 0 R >>",
    "<< /S /TR /P 16 0 R /K [18 0 R 19 0 R] >>",
    "<< /S /TH /P 17 0 R /Pg 3 0 R /K 5 >>",
    "<< /S /TH /P 17 0 R /Pg 3 0 R /K 6 >>",
    "<< /S /TBody /P 15 0 R /K [21 0 R 24 0 R] >>",
    "<< /S /TR /P 20 0 R /K [22 0 R 23 0 R] >>",
    "<< /S /TD /P 21 0 R /Pg 3 0 R /K 7 >>",
    "<< /S /TD /P 21 0 R /Pg 3 0 R /K 8 >>",
    "<< /S /TR /P 20 0 R /K 25 0 R >>",
    "<< /S /TD /P 24 0 R /Pg 3 0 R /A << /O /Table /ColSpan 2 >> /K 9 >>",
];

/// Writes the tagged file (or, `tagged == false`, the same page with no
/// structure tree) and runs `command` on it with `extra`.
fn run(command: &str, name: &str, tagged: bool, extra: &[&str]) -> String {
    let dir = std::env::temp_dir().join(format!("pdfcer-export-structure-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("in.pdf");
    let mut bodies: Vec<&str> = HEAD.to_vec();
    if tagged {
        bodies.extend_from_slice(&TREE);
    } else {
        bodies[0] = "<< /Type /Catalog /Pages 2 0 R >>";
    }
    std::fs::write(&path, build_pdf(&bodies)).unwrap();
    let ext = command.trim_start_matches("export-");
    let out = dir.join(format!("out.{ext}"));
    let mut args = vec![command, path.to_str().unwrap(), "-o", out.to_str().unwrap()];
    args.extend_from_slice(extra);
    let o = Command::new(BIN).args(&args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const TREE_USED: &str = " structure=tree structure_fallback=none structure_coverage=1.000 \
structure_blocks=4 non_standard_as_paragraph=1 untyped_as_paragraph=0 \
nested_tables_flattened=0 stray_table_content=0 broken_references=0";

#[test]
fn export_docx_follows_a_tagged_files_tree_by_default() {
    let line = run("export-docx", "docx-tree", true, &[]);
    // One inferred block: the artifact page number, which the tree does
    // not own. The table is the tree's, header and span included.
    assert!(
        line.contains(
            " pages=1 inferred=1 headings=1 paragraphs=3 list_items=1 captions=0 tables=1 \
             table_cells=5 merged_cells=1 "
        ),
        "{line}"
    );
    assert!(
        line.ends_with(&format!("{TREE_USED} inferred_blocks_kept=1\n")),
        "{line}"
    );
}

#[test]
fn export_docx_structure_layout_ignores_the_tree() {
    let line = run(
        "export-docx",
        "docx-layout",
        true,
        &["--structure", "layout"],
    );
    assert!(
        line.contains(" inferred=10 headings=1 paragraphs=8 "),
        "{line}"
    );
    assert!(line.contains(" tables=0 "), "{line}");
    assert!(
        line.contains(" structure=layout structure_fallback=disabled "),
        "{line}"
    );
    assert!(line.ends_with(" inferred_blocks_kept=10\n"), "{line}");
}

#[test]
fn export_docx_on_an_untagged_file_says_there_was_no_tree() {
    let line = run("export-docx", "docx-untagged", false, &[]);
    assert!(
        line.contains(" structure=layout structure_fallback=no-tree structure_coverage=0.000 "),
        "{line}"
    );
}

#[test]
fn export_xlsx_and_ods_take_the_trees_tables() {
    let xlsx = run("export-xlsx", "xlsx-tree", true, &[]);
    assert!(
        xlsx.contains(
            " pages=1 tables=1 inferred=0 ruled=0 aligned=0 merged_cells=1 header_rows=1 \
             pages_unreadable=0 sheets=1 cells=5 "
        ),
        "{xlsx}"
    );
    assert!(xlsx.ends_with(&format!("{TREE_USED}\n")), "{xlsx}");
    let ods = run("export-ods", "ods-tree", true, &[]);
    assert!(
        ods.contains(" tables=1 inferred=0 ruled=0 aligned=0 merged_cells=1 header_rows=1 "),
        "{ods}"
    );
    assert!(ods.ends_with(&format!("{TREE_USED}\n")), "{ods}");
    // No rules and no aligned block: detection alone finds nothing.
    let layout = run(
        "export-xlsx",
        "xlsx-layout",
        true,
        &["--structure", "layout"],
    );
    assert!(layout.contains(" tables=0 "), "{layout}");
    assert!(
        layout.contains(" structure=layout structure_fallback=disabled "),
        "{layout}"
    );
}
