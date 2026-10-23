//! `pdfcer optimize`: plan, rules, overrides, profiles, refusals and the
//! never-larger rule.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-optimize-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Object 4 is a compressible content stream; object 5 a tiny one.
fn write_pdf(dir: &std::path::Path, extra_catalog: &str) -> PathBuf {
    write_pdf_with(dir, extra_catalog, &[])
}

/// [`write_pdf`] with `extra` objects numbered from 8.
fn write_pdf_with(dir: &std::path::Path, extra_catalog: &str, extra: &[&str]) -> PathBuf {
    write_pdf_page(
        dir,
        extra_catalog,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 5 0 R] >>",
        extra,
    )
}

/// [`write_pdf_with`] with `page` as the page's (object 3) body.
fn write_pdf_page(
    dir: &std::path::Path,
    extra_catalog: &str,
    page: &str,
    extra: &[&str],
) -> PathBuf {
    let content = b"0 0 m 100 100 l S\n".repeat(100);
    let mut bodies: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {extra_catalog} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        page.as_bytes().to_vec(),
    ];
    for data in [&content[..], b"q Q"] {
        let mut s = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        s.extend_from_slice(data);
        s.extend_from_slice(b"\nendstream");
        bodies.push(s);
    }
    bodies.push(
        b"<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 0 0 0] /Contents <00> >>".to_vec(),
    );
    bodies.push(b"<< /FT /Sig /T (s) /V 6 0 R /Rect [0 0 0 0] /Subtype /Widget >>".to_vec());
    bodies.extend(extra.iter().map(|e| e.as_bytes().to_vec()));
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    let path = dir.join("in.pdf");
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("optimize")
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn optimize_writes_a_smaller_file_and_reports_each_stream() {
    let d = dir("basic");
    let input = write_pdf(&d, "");
    let out = d.join("out.pdf");
    let o = run(&[input.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let report = text(&o);
    assert!(
        report.contains("4 0") && report.contains("recompress-9"),
        "{report}"
    );
    assert!(report.contains("not smaller"), "{report}");
    assert!(std::fs::metadata(&out).unwrap().len() < std::fs::metadata(&input).unwrap().len());
    let check = Command::new(BIN)
        .args(["inspect", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(0));
}

#[test]
fn dry_run_writes_nothing_and_json_is_one_object() {
    let d = dir("dry");
    let input = write_pdf(&d, "");
    let o = run(&[input.to_str().unwrap(), "--dry-run", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    let j = text(&o);
    assert!(j.starts_with('{') && j.trim_end().ends_with('}'), "{j}");
    assert!(
        j.contains("\"smaller\":true") && j.contains("\"written\":null"),
        "{j}"
    );
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
}

#[test]
fn rules_and_overrides_can_leave_every_stream() {
    let d = dir("leave");
    let input = write_pdf(&d, "");
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--rule",
        "category=content-streams action=leave",
        "--streams",
        "leave",
    ]);
    // The rewrite alone may still shave the xref; what must hold is that no
    // stream was rewritten.
    assert!(text(&o).contains("planned: 0 stream(s)"), "{}", text(&o));
    let o = run(&[input.to_str().unwrap(), "--dry-run", "--object", "4=leave"]);
    assert!(!text(&o).contains("  4 0 "), "{}", text(&o));
}

#[test]
fn a_signed_file_is_refused_unless_signatures_may_break() {
    let d = dir("signed");
    let input = write_pdf(&d, "/AcroForm << /Fields [7 0 R] >>");
    let o = run(&[input.to_str().unwrap(), "--dry-run"]);
    assert_eq!(o.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--invalidate-signatures"));
    let o = run(&[
        input.to_str().unwrap(),
        "--dry-run",
        "--invalidate-signatures",
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).contains("invalidated"), "{}", text(&o));
}

#[test]
fn a_written_profile_drives_the_same_plan() {
    let d = dir("profile");
    let input = write_pdf(&d, "");
    let profile = d.join("mine.toml");
    let o = run(&[
        "--preset",
        "lossless",
        "--object",
        "4=leave",
        "--write-profile",
        profile.to_str().unwrap(),
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let child = d.join("child.toml");
    std::fs::write(&child, "extends = \"mine.toml\"\n").unwrap();
    let o = run(&[
        input.to_str().unwrap(),
        "--dry-run",
        "--profile",
        child.to_str().unwrap(),
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text(&o).contains("planned: 0 stream(s)"), "{}", text(&o));
}

#[test]
fn usage_errors_exit_2() {
    let d = dir("usage");
    let input = write_pdf(&d, "");
    let i = input.to_str().unwrap();
    for args in [
        vec![i],
        vec![i, "-o", i],
        vec![i, "--dry-run", "--preset", "tiny"],
        vec![i, "--dry-run", "--rule", "action=shrink"],
        vec![i, "--dry-run", "--smooth-tolerance", "0"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn the_structure_rebuild_reports_and_each_part_can_be_kept() {
    let d = dir("structure");
    // Objects 6 and 7 are referenced by nothing without the /AcroForm entry.
    let input = write_pdf(&d, "");
    let o = run(&[input.to_str().unwrap(), "--dry-run", "--json"]);
    let j = text(&o);
    assert!(
        j.contains("\"unreferenced\":[\"6 0\",\"7 0\"]") && j.contains("\"version_raised\":null"),
        "{j}"
    );
    let o = run(&[input.to_str().unwrap(), "--dry-run"]);
    assert!(text(&o).contains("2 unreferenced removed"), "{}", text(&o));
    let o = run(&[
        input.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--keep-unreferenced",
    ]);
    assert!(text(&o).contains("\"unreferenced\":[]"), "{}", text(&o));
    let o = run(&[
        input.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--keep-structure",
    ]);
    assert!(text(&o).contains("\"structure\":null"), "{}", text(&o));
}

#[test]
fn unreferenced_destinations_go_only_when_asked_and_are_listed() {
    let d = dir("cleanup");
    let input = write_pdf(
        &d,
        "/Dests << /old [3 0 R /Fit] >> /Names << /Dests << /Names [(unused) [3 0 R /Fit]] >> >>",
    );
    let path = input.to_str().unwrap();
    let o = run(&[path, "--dry-run", "--json"]);
    assert!(
        text(&o).contains("\"cleanup\":{\"links\":[],\"bookmarks\":[],\"destinations\":[]}"),
        "{}",
        text(&o)
    );
    let o = run(&[path, "--dry-run", "--remove-unreferenced-destinations"]);
    let t = text(&o);
    assert!(
        t.contains("0 bookmark(s), 2 named destination(s) removed")
            && t.contains("dest     \"old\"") // string-gap-exempt: the report pads its kind column
            && t.contains("another file"),
        "{t}"
    );
    let o = run(&[path, "--dry-run", "--json", "--preset", "screen"]);
    assert!(
        text(&o).contains("\"destinations\":[\"old\",\"unused\"]"),
        "{}",
        text(&o)
    );
    let o = run(&[
        path,
        "--dry-run",
        "--json",
        "--preset",
        "screen",
        "--keep-unreferenced-destinations",
    ]);
    assert!(text(&o).contains("\"destinations\":[]"), "{}", text(&o));
    let out = d.join("out.pdf");
    let o = run(&[
        path,
        "-o",
        out.to_str().unwrap(),
        "--remove-unreferenced-destinations",
        "--invalidate-signatures",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let o = run(&[
        out.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--remove-unreferenced-destinations",
    ]);
    assert!(text(&o).contains("\"destinations\":[]"), "{}", text(&o));
}

#[test]
fn compression_modes_choose_streams_and_object_streams() {
    let d = dir("compression");
    let input = write_pdf(&d, "");
    let path = input.to_str().unwrap();
    let json = |mode: &str| text(&run(&[path, "--dry-run", "--json", "--compression", mode]));
    let entire = json("entire");
    assert!(
        entire.contains("\"changes\":[{") && !entire.contains("\"object_streams\":0"),
        "{entire}"
    );
    let structure = json("structure");
    assert!(
        structure.contains("\"changes\":[]") && !structure.contains("\"object_streams\":0"),
        "{structure}"
    );
    let leave = json("leave");
    assert!(
        leave.contains("\"changes\":[]") && leave.contains("\"object_streams\":0"),
        "{leave}"
    );
    let o = run(&[
        path,
        "--dry-run",
        "--compression",
        "leave",
        "--streams",
        "leave",
    ]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn discard_removes_the_named_kinds_and_keep_data_wins() {
    let d = dir("discard");
    let input = write_pdf(
        &d,
        "/PieceInfo << /App << /LastModified (D:2020) /Private << /Data (secret app data) >> >> >>",
    );
    let path = input.to_str().unwrap();
    let json = |extra: &[&str]| {
        let mut args = vec![path, "--dry-run", "--json"];
        args.extend_from_slice(extra);
        text(&run(&args))
    };
    assert!(json(&[]).contains("\"discard\":[]"), "{}", json(&[]));
    let asked = json(&["--discard", "pieceinfo,thumbnail"]);
    assert!(asked.contains("\"kind\":\"pieceinfo\""), "{asked}");
    assert!(json(&["--preset", "screen"]).contains("\"kind\":\"pieceinfo\""));
    let kept = json(&["--preset", "screen", "--keep-data", "pieceinfo"]);
    assert!(kept.contains("\"discard\":[]"), "{kept}");
    assert!(json(&["--discard", "all", "--keep-data", "all"]).contains("\"discard\":[]"));

    let out = d.join("out.pdf");
    let o = run(&[path, "-o", out.to_str().unwrap(), "--discard", "pieceinfo"]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard: 1 item(s) removed"), "{report}");
    let written = std::fs::read(&out).unwrap();
    assert!(!String::from_utf8_lossy(&written).contains("secret app data"));

    let bad = run(&[path, "--dry-run", "--discard", "thumbs"]);
    assert_ne!(bad.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown data kind"));
}

#[test]
fn discard_takes_document_object_classes_by_name() {
    let d = dir("discard-objects");
    let input = write_pdf(
        &d,
        "/Outlines << /Type /Outlines /Count 0 >> /PageMode /UseOutlines /ViewerPreferences << /Duplex /Simplex /HideToolbar true >>",
    );
    let path = input.to_str().unwrap();
    let json = |extra: &[&str]| {
        let mut args = vec![path, "--dry-run", "--json"];
        args.extend_from_slice(extra);
        text(&run(&args))
    };
    assert!(json(&[]).contains("\"discard_objects\":[]"));
    let asked = json(&["--discard", "bookmarks,print-settings"]);
    assert!(asked.contains("\"object\":\"bookmarks\""), "{asked}");
    assert!(asked.contains("\"object\":\"print-settings\""), "{asked}");
    let kept = json(&["--discard", "all", "--keep-data", "bookmarks"]);
    assert!(!kept.contains("\"object\":\"bookmarks\"") && kept.contains("print-settings"));

    let out = d.join("out.pdf");
    let o = run(&[
        path,
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--discard",
        "bookmarks,print-settings",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 2 removed"), "{report}");
    assert!(
        report.contains("fall back to the printer dialog"),
        "{report}"
    );
    // --keep-structure leaves the catalog uncompressed, so a grep can see it.
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/HideToolbar"), "the catalog is readable");
    assert!(!written.contains("/Duplex") && !written.contains("/Outlines"));

    let bad = run(&[path, "--dry-run", "--discard", "embedded-index"]);
    let err = String::from_utf8_lossy(&bad.stderr).into_owned();
    assert!(
        err.contains("alternate-images") && err.contains("all"),
        "{err}"
    );
}

#[test]
fn discard_removes_form_actions_and_external_references() {
    let d = dir("discard-actions");
    let input = write_pdf(
        &d,
        "/OpenAction << /S /Launch /F (calc.exe) >> /AA << /WC << /S /ResetForm /Next << /S /JavaScript /JS (KEPTSCRIPT) >> >> >>",
    );
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--discard",
        "form-actions,external-references",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 1 removed"), "{report}");
    assert!(report.contains("do nothing when clicked"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("KEPTSCRIPT"), "the chained script stays");
    assert!(!written.contains("/Launch") && !written.contains("/ResetForm"));
}

#[test]
fn discard_removes_object_data_and_keeps_the_tags() {
    let d = dir("discard-object-data");
    let input = write_pdf_with(
        &d,
        "/StructTreeRoot 8 0 R /MarkInfo << /Marked true /UserProperties true >>",
        &[
            "<< /Type /StructTreeRoot /K 9 0 R >>",
            "<< /Type /StructElem /S /Figure /P 8 0 R /A [<< /O /Layout /Placement /Block >> << /O /UserProperties /P [<< /N (Part) /V (PARTNUMBER) >>] >>] >>",
        ],
    );
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--discard",
        "object-data",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 2 removed"), "{report}");
    assert!(report.contains("object data goes"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(!written.contains("PARTNUMBER") && !written.contains("/UserProperties"));
    assert!(
        written.contains("/Figure") && written.contains("/Layout"),
        "the tags stay"
    );
}

#[test]
fn discard_removes_form_fields_and_multimedia() {
    let d = dir("discard-forms-media");
    let input = write_pdf_with(
        &d,
        "/AcroForm << /Fields [] /DA (/Helv 0 Tf 0 g) >> /Names << /Renditions 8 0 R >>",
        &[
            "<< /Names [(clip) 9 0 R] >>",
            "<< /S /MR /C << /S /MCD /D (CLIP-FILE.mp4) >> >>",
        ],
    );
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--discard",
        "form-fields,multimedia,3d-models",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 2 removed"), "{report}");
    assert!(report.contains("form fields go") && report.contains("multimedia goes"));
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(!written.contains("/AcroForm") && !written.contains("CLIP-FILE"));
}

#[test]
fn discard_flatten_layers_keeps_the_hidden_one() {
    let d = dir("discard-flatten-layers");
    let input = write_pdf_with(
        &d,
        "/OCProperties << /OCGs [8 0 R 9 0 R] /D << /OFF [9 0 R] >> >>",
        &[
            "<< /Type /OCG /Name (VISIBLE-ONE) >>",
            "<< /Type /OCG /Name (HIDDEN-ONE) >>",
        ],
    );
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--discard",
        "flatten-layers",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 1 removed"), "{report}");
    assert!(report.contains("visible layers merge"), "{report}");
    let listed = Command::new(BIN)
        .args(["list-layers", out.to_str().unwrap()])
        .output()
        .unwrap();
    let listed = text(&listed);
    assert!(listed.contains("layers=1"), "{listed}");
    assert!(listed.contains("HIDDEN-ONE") && !listed.contains("VISIBLE-ONE"));
}

#[test]
fn discard_search_index_keeps_other_page_piece_data() {
    let d = dir("discard-search-index");
    let input = write_pdf_with(
        &d,
        "/PieceInfo << /SearchIndex 8 0 R /Other << /Private (KEEP-ME) >> >>",
        &["<< /LastModified (D:20260101000000Z) /Private (INDEX-DATA) >>"],
    );
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--discard",
        "search-index",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 1 removed"), "{report}");
    assert!(report.contains("search index goes"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("KEEP-ME") && !written.contains("INDEX-DATA"));
}

#[test]
fn discard_merge_image_fragments_draws_one_image() {
    let d = dir("discard-merge-image-fragments");
    let mut names = String::new();
    let mut draw = String::new();
    let mut extra = Vec::new();
    for i in 0..10 {
        names.push_str(&format!("/S{i} {} 0 R ", i + 9));
        draw.push_str(&format!("q 100 0 0 10 0 {} cm /S{i} Do Q\n", 90 - 10 * i));
        let row = char::from(b'a' + i as u8).to_string().repeat(64);
        extra.push(format!(
            "<< /Type /XObject /Subtype /Image /Width 32 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 64 >>\nstream\n{row}\nendstream"
        ));
    }
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 8 0 R /Resources << /XObject << {names}>> >> >>"
    );
    extra.insert(
        0,
        format!("<< /Length {} >>\nstream\n{draw}endstream", draw.len()),
    );
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let input = write_pdf_page(&d, "", &page, &extra);
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--discard",
        "merge-image-fragments",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("discard objects: 1 removed"), "{report}");
    assert!(report.contains("thin strips"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert_eq!(written.matches("/Subtype /Image").count(), 1, "{written}");
}

#[test]
fn discard_smooth_lines_redraws_an_arc_as_curves() {
    let d = dir("discard-smooth-lines");
    let mut draw = String::from("1 w\n");
    for i in 0..=60 {
        let a = (f64::from(i) * 3.0).to_radians();
        let op = if i == 0 { "m" } else { "l" };
        draw.push_str(&format!(
            "{:.3} {:.3} {op}\n",
            50.0 + 40.0 * a.cos(),
            50.0 + 40.0 * a.sin()
        ));
    }
    draw.push_str("S\n");
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 8 0 R >>";
    let stream = format!("<< /Length {} >>\nstream\n{draw}endstream", draw.len());
    let input = write_pdf_page(&d, "", page, &[&stream]);
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "leave",
        "--discard",
        "smooth-lines",
        "--smooth-tolerance",
        "0.05",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("60 line segments into "), "{report}");
    assert!(report.contains("redrawn as curves"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains(" c\n"), "no curve written");
    assert!(!written.contains("50.000 90.000 l"), "the lines survived");
}

#[test]
fn a_requested_removal_is_written_and_exits_0_even_when_not_smaller() {
    let d = dir("removal-not-smaller");
    let input = write_pdf(&d, "/ViewerPreferences << /Duplex /Simplex >>");
    let packed = d.join("packed.pdf");
    let o = run(&[input.to_str().unwrap(), "-o", packed.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    // Unpacking the object streams makes the rewrite larger than `packed`.
    let out = d.join("out.pdf");
    let args = |extra: &[&str]| {
        let mut a = vec![packed.to_str().unwrap(), "--keep-structure"];
        a.extend_from_slice(extra);
        run(&a)
    };
    let o = args(&["-o", out.to_str().unwrap(), "--discard", "print-settings"]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("not smaller: kept anyway"), "{report}");
    assert!(std::fs::metadata(&out).unwrap().len() >= std::fs::metadata(&packed).unwrap().len());
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Catalog"), "the catalog is readable");
    assert!(!written.contains("/Duplex"), "the removal is in the file");
    let json = text(&args(&[
        "--dry-run",
        "--json",
        "--discard",
        "print-settings",
    ]));
    assert!(json.contains("\"smaller\":false"), "{json}");

    let none = args(&["-o", d.join("none.pdf").to_str().unwrap()]);
    assert_eq!(none.status.code(), Some(14), "{}", text(&none));
}

/// A 200 x 200 grey image of letter noise drawn at 50 pt on one page.
fn noise_image_pdf(d: &std::path::Path) -> std::path::PathBuf {
    noise_image_drawn_by(d, "q 50 0 0 50 0 0 cm /Im9 Do Q")
}

/// [`noise_image_pdf`] with `draw` as the page content.
fn noise_image_drawn_by(d: &std::path::Path, draw: &str) -> std::path::PathBuf {
    let mut x = 0x9E37_79B9u32;
    let px: String = (0..200 * 200)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            char::from(b'a' + (x >> 24) as u8 % 26)
        })
        .collect();
    let extra = [
        format!(
            "<< /Length {} >>\nstream\n{draw}\nendstream",
            draw.len() + 1
        ),
        format!(
            "<< /Type /XObject /Subtype /Image /Width 200 /Height 200 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n{px}\nendstream",
            px.len()
        ),
    ];
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 8 0 R /Resources << /XObject << /Im9 9 0 R >> >> >>";
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    write_pdf_page(d, "", page, &extra)
}

#[test]
fn an_image_setting_downsamples_and_reports_size_page_and_source() {
    let d = dir("image-downsample");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--preset",
        "lossless",
        "--streams",
        "image(ppi=72)",
        "--rule",
        "image-class=gray action=image(method=average)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{report}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    // 200 px over 50 pt is 288 ppi; 72 ppi there is 50 px.
    assert!(
        report.contains("200x200 -> 50x50 px, average (rule 1)"),
        "{report}"
    );
    assert!(report.contains("target 72 ppi (images)"), "{report}");
    // The preset still recompresses the content stream beside the image.
    assert!(
        report
            .lines()
            .any(|l| l.contains("8 0") && l.contains("recompress-9")),
        "{report}"
    );
    assert!(
        report.contains("binding use page 1, 50x50 pt, 288 -> 72 ppi"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Width 50"), "{written}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(j.contains("\"size_after\":[50,50]"), "{j}");
    assert!(
        j.contains("\"method\":{\"value\":\"average\",\"source\":\"rule 1\"}"),
        "{j}"
    );
}

#[test]
fn max_pixels_caps_an_image_drawn_on_no_page() {
    let d = dir("image-max-pixels");
    let input = noise_image_drawn_by(&d, "q Q");
    let out = d.join("out.pdf");
    let base = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--preset",
        "lossless",
        "--streams",
    ];
    let skipped = text(&run(&[&base[..], &["image(ppi=72)", "--dry-run"]].concat()));
    assert!(skipped.contains("max-pixels=N caps it"), "{skipped}");
    let args = [&base[..], &["image(ppi=72,max-pixels=2500)"]].concat();
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("200x200 -> 50x50 px"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Width 50"), "{written}");
    let j = text(&run(&[&args[..], &["--dry-run", "--json"]].concat()));
    assert!(
        j.contains("\"max_pixels\":{\"value\":2500,\"source\":\"images\"}"),
        "{j}"
    );
}

#[test]
fn identical_images_are_planned_as_one_and_reported() {
    let d = dir("image-copies");
    let draw = "q 100 0 0 100 0 0 cm /Im9 Do Q q 50 0 0 50 0 0 cm /Im10 Do Q";
    let px = "ab".repeat(200 * 100);
    let image = format!(
        "<< /Type /XObject /Subtype /Image /Width 200 /Height 200 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n{px}\nendstream",
        px.len()
    );
    let content = format!(
        "<< /Length {} >>\nstream\n{draw}\nendstream",
        draw.len() + 1
    );
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 8 0 R /Resources << /XObject << /Im9 9 0 R /Im10 10 0 R >> >> >>";
    let input = write_pdf_page(&d, "", page, &[&content, &image, &image]);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--preset",
        "lossless",
        "--streams",
        "image(ppi=72)",
        "--dry-run",
    ];
    // The 100 pt use (144 ppi) binds both copies: 200 px -> 100 px.
    let report = text(&run(&args));
    assert!(
        report.contains("identical to 10 0 R (planned as one)"),
        "{report}"
    );
    assert_eq!(
        report.matches("200x200 -> 100x100 px").count(),
        2,
        "{report}"
    );
    let j = text(&run(&[&args[..], &["--json"]].concat()));
    assert!(j.contains("\"copies\":[[10,0]]"), "{j}");
    let alone = text(&run(&[&args[..], &["--keep-duplicates"]].concat()));
    assert!(alone.contains("200x200 -> 50x50 px"), "{alone}");
    assert!(!alone.contains("planned as one"), "{alone}");
}

#[test]
fn the_report_ends_with_one_row_per_image() {
    let d = dir("image-table");
    let input = noise_image_drawn_by(&d, "q Q");
    let out = d.join("out.pdf");
    let base = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--preset",
        "lossless",
        "--dry-run",
        "--streams",
    ];
    let skipped = text(&run(&[&base[..], &["image(ppi=72)"]].concat()));
    let rows: Vec<&str> = skipped
        .lines()
        .skip_while(|l| !l.starts_with("images:"))
        .collect();
    assert_eq!(
        rows.first(),
        Some(&"images: 0 rewritten, 1 skipped"),
        "{skipped}"
    );
    assert!(
        rows[1]
            .split_whitespace()
            .eq(["object", "page", "pixels", "ppi", "format", "bytes"])
    );
    assert!(
        rows[2].starts_with("  9 0     -     -       -    -       skipped: drawn on no page"), // string-gap-exempt: the table's column padding
        "{skipped}"
    );
    let capped = text(&run(
        &[&base[..], &["image(ppi=72,max-pixels=2500)"]].concat()
    ));
    let row = capped
        .lines()
        .skip_while(|l| !l.starts_with("images: 1 rewritten, 0 skipped"))
        .nth(2)
        .unwrap();
    let cells: Vec<&str> = row
        .split("  ")
        .filter(|c| !c.is_empty())
        .map(str::trim)
        .collect();
    assert_eq!(
        cells[..4],
        ["9 0", "-", "200x200 -> 50x50", "-"],
        "{capped}"
    );
    assert_eq!(cells[4], "raw/DeviceGray 8 bit -> Flate/DeviceGray 8 bit");
    assert!(cells[5].starts_with("40000 -> "), "{capped}");
}

#[test]
fn an_image_written_as_jpeg_reports_format_and_quality() {
    let d = dir("image-jpeg");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(encode=jpeg,quality=low)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("200x200 -> 200x200 px; written as jpeg quality 40 (asked 40)"),
        "{report}"
    );
    assert!(report.contains("(lossy)"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/DCTDecode"), "{written}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(j.contains("\"encoded_as\":\"jpeg\""), "{j}");
    assert!(
        j.contains("\"jpeg\":{\"asked\":40,\"used\":40,\"source_estimate\":null,\"ssim\":null}"),
        "{j}"
    );
    assert!(
        j.contains("\"quality\":{\"value\":40,\"source\":\"images\"}"),
        "{j}"
    );
}

#[test]
fn an_ssim_target_reports_the_quality_it_picked_and_the_score() {
    let d = dir("image-ssim");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(encode=jpeg,quality=40,ssim=0.9999)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    let line = report
        .lines()
        .find(|l| l.contains("written as jpeg quality 40 (asked 40, ssim "))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(line.contains(" misses 0.9999 in 1 trials)"), "{line}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(
        j.contains("\"ssim\":{\"target\":0.9999,\"achieved\":"),
        "{j}"
    );
    assert!(j.contains("\"met\":false,\"trials\":1}"), "{j}");
    assert!(
        j.contains("\"ssim\":{\"value\":0.9999,\"source\":\"images\"}"),
        "{j}"
    );
}

#[test]
fn a_bilevel_conversion_reports_format_and_threshold() {
    let d = dir("image-bilevel");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(convert=bilevel,threshold=110)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("format raw/DeviceGray 8 bit -> Flate/DeviceGray 1 bit; converted from DeviceGray, threshold 110"),
        "{report}"
    );
    assert!(report.contains("(lossy)"), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/BitsPerComponent 1"), "{written}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(j.contains("\"bpc\":[8,1]"), "{j}");
    assert!(
        j.contains("\"conversion\":{\"to\":\"bilevel\",\"from\":\"DeviceGray\",\"formula\":null,\"not_colour_managed\":false,\"threshold\":110,\"colors\":null,\"exact\":false}"),
        "{j}"
    );
    assert!(
        j.contains("\"threshold\":{\"value\":\"110\",\"source\":\"images\"}"),
        "{j}"
    );
}

#[test]
fn a_png_predicted_image_reports_it_and_writes_decode_parms() {
    let d = dir("image-predictor");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(convert=bilevel,predictor=png)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("written as flate-9 with PNG prediction"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Predictor 15"), "{written}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(
        j.contains("\"predictor\":{\"value\":\"png\",\"source\":\"images\"},\"predicted\":true"),
        "{j}"
    );
    assert!(
        j.contains("\"keyed\":{\"value\":\"subsample\",\"source\":\"default\"}"),
        "{j}"
    );
}

/// `noise_image_pdf`'s image with a `/Matte` soft mask of its size.
fn matte_pair_pdf(d: &std::path::Path) -> std::path::PathBuf {
    let px: String = (0..200 * 200u32)
        .map(|i| char::from(b'a' + (i.wrapping_mul(2_654_435_761) >> 27) as u8))
        .collect();
    let draw = "q 50 0 0 50 0 0 cm /Im9 Do Q";
    let image = |extra: &str| {
        format!(
            "<< /Type /XObject /Subtype /Image /Width 200 /Height 200 /ColorSpace /DeviceGray /BitsPerComponent 8 {extra} /Length {} >>\nstream\n{px}\nendstream",
            px.len()
        )
    };
    let extra = [
        format!(
            "<< /Length {} >>\nstream\n{draw}\nendstream",
            draw.len() + 1
        ),
        image("/SMask 10 0 R"),
        image("/Matte [0]"),
    ];
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 8 0 R /Resources << /XObject << /Im9 9 0 R >> >> >>";
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    write_pdf_page(d, "", page, &extra)
}

#[test]
fn a_matte_soft_mask_is_resampled_with_its_image_and_says_so() {
    let d = dir("image-matte");
    let input = matte_pair_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(ppi=72)",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("/Matte soft mask sized with its image 9 0 R"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert_eq!(written.matches("/Width 50").count(), 2, "{written}");
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = text(&run(&json));
    assert!(j.contains("\"resampled_with\":[9,0]"), "{j}");
    assert!(j.contains("\"resampled_with\":null"), "{j}");
}

#[test]
fn a_runlength_image_reports_its_format_and_decodes() {
    let d = dir("image-runlength");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(convert=bilevel,threshold=110,encode=runlength)",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("format raw/DeviceGray 8 bit -> RunLength/DeviceGray 1 bit"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Filter /RunLengthDecode"), "{written}");
}

#[test]
fn an_lzw_image_reports_its_format_and_the_flate_size() {
    let d = dir("image-lzw");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(convert=bilevel,threshold=110,encode=lzw)",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("format raw/DeviceGray 8 bit -> LZW/DeviceGray 1 bit"),
        "{report}"
    );
    assert!(report.contains("written as lzw"), "{report}");
    assert!(report.contains("(Flate would be "), "{report}");
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Filter /LZWDecode"), "{written}");
}

#[test]
fn a_brotli_image_reports_its_format_and_declares_the_extension() {
    let d = dir("image-brotli");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        "image(encode=brotli-9)",
    ]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("-> Brotli/DeviceGray 8 bit"), "{report}");
    assert!(report.contains("written as brotli-9"), "{report}");
    assert!(report.contains("(Flate would be "), "{report}");
    assert!(report.contains("note: Brotli streams need"), "{report}");
    assert!(
        report.contains("Acrobat Reader opens the file without an error but draws every page that uses one blank"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Filter /BrotliDecode"), "{written}");
    assert!(written.contains("extension-brotli"), "no extension");
    assert!(written.contains("/Version /2.0"), "no version raise");
}

#[test]
fn a_ccitt_image_needs_bilevel_output_and_reports_its_format() {
    let d = dir("image-ccitt");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let base = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
    ];
    let refused = text(&run(
        &[&base[..], &["image(encode=ccitt)", "--dry-run"]].concat()
    ));
    assert!(
        refused.contains("not bilevel: ccitt codes 1-bit"),
        "{refused}"
    );
    let o = run(&[
        &base[..],
        &["image(convert=bilevel,threshold=110,encode=ccitt)"],
    ]
    .concat());
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("format raw/DeviceGray 8 bit -> CCITT G4/DeviceGray 1 bit"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Filter /CCITTFaxDecode"), "{written}");
    assert!(written.contains("/K -1"), "{written}");
}

#[test]
fn a_jbig2_image_needs_bilevel_output_and_reports_its_format() {
    let d = dir("image-jbig2");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let base = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
    ];
    let refused = text(&run(
        &[&base[..], &["image(encode=jbig2)", "--dry-run"]].concat()
    ));
    assert!(
        refused.contains("not bilevel: jbig2 codes 1-bit"),
        "{refused}"
    );
    let o = run(&[
        &base[..],
        &["image(convert=bilevel,threshold=110,encode=jbig2)"],
    ]
    .concat());
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(
        report.contains("format raw/DeviceGray 8 bit -> JBIG2/DeviceGray 1 bit"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.contains("/Filter /JBIG2Decode"), "{written}");
    assert!(!written.contains("/JBIG2Globals"), "{written}");
}

#[test]
fn encode_auto_reports_every_candidate_in_text_and_json() {
    let d = dir("image-auto");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let action = "image(convert=bilevel,threshold=110,encode=auto)";
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
        action,
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("(auto, smallest of flate-9 "), "{report}");
    for name in [", ccitt ", ", runlength ", ", jbig2 "] {
        assert!(report.contains(name), "{name}: {report}");
    }
    let json = text(&run(&[&args[..], &["--dry-run", "--json"]].concat()));
    assert!(
        json.contains("\"candidates\":[{\"encode\":\"flate-9\",\"bytes\":"),
        "{json}"
    );
}

/// JBIG2 into a PDF 1.3 document raises its catalog `/Version` to 1.4 and
/// says so in text and JSON; `version-raise=refuse` skips the image.
#[test]
fn jbig2_in_a_pdf_1_3_document_raises_and_discloses_the_version() {
    let d = dir("image-jbig2-raise");
    let mut bytes = std::fs::read(noise_image_pdf(&d)).unwrap();
    assert!(bytes.starts_with(b"%PDF-1."));
    bytes[7] = b'3';
    let input = d.join("v13.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let out = d.join("out.pdf");
    let base = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--keep-structure",
        "--streams",
    ];
    let action = "image(convert=bilevel,threshold=110,encode=jbig2)";
    let o = run(&[&base[..], &[action]].concat());
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains(", needs PDF 1.4"), "{report}");
    assert!(
        report.contains("version: raised to PDF 1.4 in the catalog's /Version"),
        "{report}"
    );
    let written = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(written.starts_with("%PDF-1.3"), "header changed");
    assert!(written.contains("/Version /1.4"), "{written}");
    let json = text(&run(&[&base[..], &[action, "--dry-run", "--json"]].concat()));
    assert!(json.contains("\"raises_version\":\"1.4\""), "{json}");
    assert!(json.contains("\"version_raise\":\"1.4\""), "{json}");
    assert!(
        json.contains("\"version_raise\":{\"value\":\"allow\",\"source\":\"default\"}"),
        "{json}"
    );
    let refuse = "image(convert=bilevel,threshold=110,encode=jbig2,version-raise=refuse)";
    let refused = text(&run(&[&base[..], &[refuse, "--dry-run"]].concat()));
    assert!(refused.contains("version-raise=refuse"), "{refused}");
    assert!(!refused.contains("version: raised"), "{refused}");
}

#[test]
fn streams_brotli_writes_brotli_and_declares_the_extension() {
    let d = dir("brotli");
    let input = write_pdf(&d, "");
    let out = d.join("out.pdf");
    let o = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--streams",
        "brotli",
        "--keep-structure",
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let report = text(&o);
    assert!(report.contains("brotli-11"), "{report}");
    assert!(report.contains("note: Brotli streams need"), "{report}");
    let saved = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(saved.contains("/BrotliDecode"), "no Brotli stream");
    assert!(
        saved.contains("/PDFa") && saved.contains("extension-brotli"),
        "no extension dictionary"
    );
    assert!(saved.contains("/Version /2.0"), "no version raise");
    let check = Command::new(BIN)
        .args(["inspect", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(0));
}

#[test]
fn linearize_puts_the_parameter_dictionary_first_and_reports_it() {
    let d = dir("linearize");
    let input = write_pdf(&d, "");
    let out = d.join("out.pdf");
    let path = input.to_str().unwrap();
    let o = run(&[path, "-o", out.to_str().unwrap(), "--linearize", "--json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text(&o).contains("\"linearized\":true"), "{}", text(&o));
    let saved = std::fs::read(&out).unwrap();
    let head = String::from_utf8_lossy(&saved[..saved.len().min(200)]).into_owned();
    let first = head.find(" 0 obj").expect("no first object");
    assert!(
        head[first..]
            .trim_start_matches(" 0 obj")
            .trim_start()
            .starts_with("<<")
            && head.contains("/Linearized 1"),
        "{head}"
    );
    let check = Command::new(BIN)
        .args(["inspect", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(0));
    let o = run(&[path, "--dry-run", "--json"]);
    assert!(text(&o).contains("\"linearized\":false"), "{}", text(&o));
    let o = run(&[path, "--dry-run", "--linearize", "--keep-structure"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn a_target_size_the_settings_meet_stops_at_step_0() {
    let d = dir("budget-met");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--target-size",
        "10MB",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("budget: target 10000000 B\n"), "{report}");
    assert!(
        report.contains("budget: met at step 0 (your settings as given)"),
        "{report}"
    );
}

#[test]
fn an_unreachable_target_size_writes_the_closest_file_and_exits_15() {
    let d = dir("budget-over");
    let input = noise_image_pdf(&d);
    let out = d.join("out.pdf");
    let args = [
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--target-size",
        "100",
    ];
    let o = run(&args);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(15), "{report}");
    assert!(
        report.contains("step 11 (JPEG quality <= 30, <= 72 ppi)"),
        "{report}"
    );
    assert!(report.contains("budget: NOT met"), "{report}");
    let written = std::fs::metadata(&out).unwrap().len();
    assert!(written < std::fs::metadata(&input).unwrap().len() / 2);
    let mut json = args.to_vec();
    json.extend(["--dry-run", "--json"]);
    let j = run(&json);
    assert_eq!(j.status.code(), Some(15));
    let j = text(&j);
    assert!(
        j.contains("\"budget\":{\"target_bytes\":100,\"met\":false,\"step\":11,\"attempts\":[{\"step\":0,\"quality\":null,\"ppi\":null,"),
        "{j}"
    );
    assert!(j.contains("{\"step\":11,\"quality\":30,\"ppi\":72,"), "{j}");
}

#[test]
fn a_target_size_that_is_not_a_size_is_a_usage_error() {
    let d = dir("budget-usage");
    let input = noise_image_pdf(&d);
    let o = run(&[
        input.to_str().unwrap(),
        "--dry-run",
        "--target-size",
        "lots",
    ]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn visual_diff_reports_a_lossless_run_unchanged_and_a_lossy_one_changed() {
    let d = dir("visual-diff");
    let plain = write_pdf(&d, "");
    let o = run(&[plain.to_str().unwrap(), "--dry-run", "--visual-diff"]);
    let report = text(&o);
    assert!(
        report.contains("visual diff (72 dpi, threshold 8): 1 page(s), 0 changed"),
        "{report}"
    );
    let noisy = noise_image_pdf(&d);
    let pictures = d.join("pictures");
    let o = run(&[
        noisy.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--target-size",
        "1KB",
        "--visual-diff-dir",
        pictures.to_str().unwrap(),
    ]);
    let j = text(&o);
    assert!(
        j.contains(
            "\"visual_diff\":{\"dpi\":72,\"threshold\":8,\"pages\":[{\"page\":1,\"ssim\":0."
        ),
        "{j}"
    );
    assert!(!j.contains("\"changed_pixels\":0,"), "{j}");
    assert!(j.contains("\"pictures\":1,\"failure\":null}"), "{j}");
    assert!(pictures.join("page-0001.png").is_file());
}

#[test]
fn a_compat_target_holds_the_output_and_lists_what_is_above_it() {
    let d = dir("compat");
    let input = write_pdf_page(
        &d,
        "/OCProperties << /OCGs [] /D << >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 5 0 R] \
         /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >> >>",
        &[],
    );
    let i = input.to_str().unwrap();
    let o = run(&[i, "--dry-run", "--compat", "1.4"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let report = text(&o);
    for want in [
        "compatibility: target PDF 1.4",
        "uses transparency (PDF 1.4): 1 object(s), e.g. object 3 0\n",
        "uses layers (PDF 1.5): 1 object(s), above the target",
        "1 feature(s) above PDF 1.4 kept, not removed; the declared version 1.7 is not lowered",
        "note: object streams not packed",
        "compatibility: the content needs PDF 1.5
",
        "  needs PDF 1.5: /OCProperties (1 object(s), e.g. 1 0 R)
",
    ] {
        assert!(report.contains(want), "{want:?} in\n{report}");
    }
    assert!(!report.contains("uses object streams"), "{report}");

    let j = text(&run(&[i, "--dry-run", "--compat", "retain", "--json"]));
    assert!(
        j.contains("\"compatibility\":{\"target\":\"1.7\",\"declared\":\"1.7\",")
            && j.contains("\"above\":0,\"floor\":{\"version\":\"1.5\",\"proven\":true,")
            && j.contains("\"icc_profiles\":[]"),
        "{j}"
    );
    assert!(text(&run(&[i, "--dry-run", "--json"])).contains("\"compatibility\":null"));
    assert_eq!(
        run(&[i, "--dry-run", "--compat", "1.9"]).status.code(),
        Some(2)
    );
    assert_eq!(
        run(&[i, "--dry-run", "--strict-icc"]).status.code(),
        Some(2)
    );
}

#[test]
fn lower_version_rewrites_the_header_and_refuses_content_above_the_target() {
    let d = dir("lower");
    let input = write_pdf(&d, "/Version /1.7");
    let i = input.to_str().unwrap();
    let out = d.join("out.pdf");
    let o = run(&[
        i,
        "-o",
        out.to_str().unwrap(),
        "--compat",
        "1.4",
        "--lower-version",
    ]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let report = text(&o);
    for want in [
        "the declared version was lowered from 1.7 to 1.4",
        "version: lowered to PDF 1.4: ",
        ", catalog /Version removed",
    ] {
        assert!(report.contains(want), "{want:?} in\n{report}");
    }
    let written = std::fs::read(&out).unwrap();
    assert!(written.starts_with(b"%PDF-1.4"), "{report}");

    let j = text(&run(&[
        i,
        "--dry-run",
        "--json",
        "--compat",
        "1.4",
        "--lower-version",
    ]));
    assert!(
        j.contains(
            "\"lowered\":{\"from\":\"1.7\",\"to\":\"1.4\",\"catalog_version_removed\":true,"
        ),
        "{j}"
    );

    let glass = write_pdf_page(
        &d,
        "",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 5 0 R] \
         /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >> >>",
        &[],
    );
    let refused_out = d.join("refused.pdf");
    let o = run(&[
        glass.to_str().unwrap(),
        "-o",
        refused_out.to_str().unwrap(),
        "--compat",
        "1.3",
        "--lower-version",
    ]);
    assert_eq!(o.status.code(), Some(9));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("the content needs PDF 1.4"), "{err}");
    assert!(!refused_out.exists());
    assert_eq!(
        run(&[i, "--dry-run", "--lower-version"]).status.code(),
        Some(2)
    );
}
