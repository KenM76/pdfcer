//! `compare --visual` (`Pass 645.2`): tight regions for a moved image, a
//! white-text edit caught by the text half, inserted / deleted / moved pages
//! matched, a scan pair, and the marked result.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-compare-visual-{}", std::process::id()));
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

/// One page: its content stream and, optionally, a 64x64 DeviceGray image
/// named `/Im1`.
struct Page {
    content: String,
    image: Option<Vec<u8>>,
}

fn page(content: impl Into<String>) -> Page {
    Page {
        content: content.into(),
        image: None,
    }
}

/// A Letter-size PDF of `pages`, Helvetica as `/F1` on every page.
fn pdf(pages: &[Page]) -> Vec<u8> {
    let n = pages.len();
    // Objects: 1 catalog, 2 pages, 3 font, then per page: page, content, image.
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            (0..n)
                .map(|i| format!("{} 0 R", 4 + 3 * i))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    for (i, p) in pages.iter().enumerate() {
        let (pg, ct, im) = (4 + 3 * i, 5 + 3 * i, 6 + 3 * i);
        bodies.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {ct} 0 R \
                 /Resources << /Font << /F1 3 0 R >> /XObject << /Im1 {im} 0 R >> >> >>"
            )
            .into_bytes(),
        );
        let _ = pg;
        let mut c = format!("<< /Length {} >>\nstream\n", p.content.len()).into_bytes();
        c.extend_from_slice(p.content.as_bytes());
        c.extend_from_slice(b"\nendstream");
        bodies.push(c);
        let data = p.image.clone().unwrap_or_else(|| vec![255; 64 * 64]);
        let mut img = format!(
            "<< /Type /XObject /Subtype /Image /Width 64 /Height 64 /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        img.extend_from_slice(&data);
        img.extend_from_slice(b"\nendstream");
        bodies.push(img);
    }
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f\r\n", bodies.len() + 1).as_bytes(),
    );
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    buf
}

/// A sheet with a frame and a title, plus a filled 60 pt square at `(x, y)`.
fn sheet(x: u32, y: u32) -> Page {
    page(format!(
        "2 w 20 20 572 752 re S BT /F1 14 Tf 40 40 Td (Sheet) Tj ET {x} {y} 60 60 re f"
    ))
}

fn write(name: &str, pages: &[Page]) -> PathBuf {
    let p = temp(name);
    std::fs::write(&p, pdf(pages)).expect("write fixture");
    p
}

#[test]
fn a_moved_image_is_two_tight_regions_not_the_sheet() {
    let im = |x, y| {
        page(format!(
            "2 w 20 20 572 752 re S 40 0 0 40 {x} {y} cm /Im1 Do"
        ))
    };
    let dark = |mut p: Page| {
        p.image = Some(vec![30; 64 * 64]);
        p
    };
    let old = write("img_old.pdf", &[dark(im(100, 600))]);
    let new = write("img_new.pdf", &[dark(im(400, 200))]);
    let (code, out, err) = run(&["compare", s(&old), s(&new), "--visual", "--json"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("\"how\":\"in_order\""), "{out}");
    assert_eq!(out.matches("\"rect\":").count(), 2, "two regions: {out}");
    // Each region is about the 40 pt image, not the 612 x 792 sheet.
    assert!(
        out.contains("\"rect\":[100,600,140,640]") && out.contains("\"rect\":[400,200,440,240]"),
        "{out}"
    );
}

#[test]
fn a_white_text_edit_is_caught_by_the_text_half() {
    let w = |t: &str| page(format!("1 1 1 rg BT /F1 12 Tf 72 700 Td ({t}) Tj ET"));
    let old = write("white_old.pdf", &[w("alpha")]);
    let new = write("white_new.pdf", &[w("omega")]);
    let (code, out, err) = run(&["compare", s(&old), s(&new), "--visual", "--exit-code"]);
    assert_eq!(code, 16, "{out}\n{err}");
    assert!(out.contains("1 replaced"), "{out}");
    assert!(
        out.contains("0 changed region(s)"),
        "invisible on the page: {out}"
    );
}

#[test]
fn inserted_deleted_and_moved_pages_are_matched() {
    let a = || sheet(80, 600);
    let b = || sheet(450, 600);
    let c = || sheet(80, 120);
    let d = || sheet(450, 120);
    let x = || page("BT /F1 40 Tf 100 400 Td (Brand new page) Tj ET 0 0 612 300 re f");
    // old: A B C D   new: B X C A   (A moved to the end, D deleted, X inserted)
    let old = write("pages_old.pdf", &[a(), b(), c(), d()]);
    let new = write("pages_new.pdf", &[b(), x(), c(), a()]);
    let (code, out, err) = run(&["compare", s(&old), s(&new), "--visual"]);
    assert_eq!(code, 0, "{out}\n{err}");
    for line in [
        "old p2 = new p1",
        "new p2 inserted",
        "old p3 = new p3",
        "old p1 moved to new p4",
        "old p4 deleted",
    ] {
        assert!(out.contains(line), "{line}: {out}");
    }
    assert!(
        out.contains("pages matched by appearance"),
        "settings disclosed: {out}"
    );
}

#[test]
fn a_scan_pair_reports_the_added_mark() {
    let noise: Vec<u8> = (0..64 * 64)
        .map(|i| 200 + ((i * 7919) % 41) as u8)
        .collect();
    let mut marked = noise.clone();
    for y in 20..28 {
        for x in 30..38 {
            marked[y * 64 + x] = 0;
        }
    }
    let scan = |data: Vec<u8>| Page {
        content: "612 0 0 792 0 0 cm /Im1 Do".to_owned(),
        image: Some(data),
    };
    let old = write("scan_old.pdf", &[scan(noise)]);
    let new = write("scan_new.pdf", &[scan(marked)]);
    let (code, out, err) = run(&["compare", s(&old), s(&new), "--visual"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("1 changed region(s)"), "{out}");
    assert!(out.contains("0 change(s)"), "no text to compare: {out}");
}

#[test]
fn nothing_alike_says_so_and_a_map_overrides() {
    let old = write("far_old.pdf", &[sheet(80, 600)]);
    let new = write("far_new.pdf", &[page("0 0 612 792 re f")]);
    let (_, out, _) = run(&["compare", s(&old), s(&new), "--visual"]);
    assert!(out.contains("not \"every page changed\""), "{out}");
    let (_, out, _) = run(&["compare", s(&old), s(&new), "--visual", "--map", "1=1"]);
    assert!(out.contains("old p1 = new p1 (mapped)"), "{out}");
}

#[test]
fn the_result_carries_clouds_and_an_inserted_page_frame() {
    let old = write("res_old.pdf", &[sheet(80, 600), sheet(450, 120)]);
    let new = write(
        "res_new.pdf",
        &[
            sheet(90, 600),
            page("BT /F1 40 Tf 100 400 Td (New) Tj ET 0 0 612 300 re f"),
            sheet(450, 120),
        ],
    );
    let before = std::fs::read(&new).unwrap();
    let out_pdf = temp("res_out.pdf");
    let (code, out, err) = run(&[
        "compare",
        s(&old),
        s(&new),
        "--visual",
        "--output",
        s(&out_pdf),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("visual mark(s), 0 unmarked"), "{out}");
    assert_eq!(before, std::fs::read(&new).unwrap(), "NEW untouched");
    let result = std::fs::read(&out_pdf).unwrap();
    let update = String::from_utf8_lossy(&result[before.len()..]);
    assert!(update.matches("/Subtype /Square").count() >= 2, "{update}");
    assert!(update.contains("/BE"), "a cloud: {update}");
    assert!(update.contains("Inserted page"), "{update}");
}
