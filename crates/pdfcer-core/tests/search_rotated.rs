//! A search hit on rotated text is covered by a quad along the text, not by
//! a box built as if the glyphs ran left to right.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;

/// One page showing `HELLO` at 20 pt with `tm` as the text matrix.
fn session(tm: &str) -> EditSession {
    let content = format!("BT /F1 20 Tf {tm} Tm (HELLO) Tj ET\n");
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |buf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut buf, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut buf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut buf,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font \
          << /F1 5 0 R >> >> /Contents 4 0 R >>",
    );
    obj(
        &mut buf,
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )
        .as_bytes(),
    );
    obj(
        &mut buf,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
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
    EditSession::new(Document::from_bytes(buf).unwrap())
}

/// `(width, height)` of the hit's axis-aligned bounds.
fn span(s: &mut EditSession) -> (f64, f64) {
    let hits = s.find_text("HELLO", false);
    assert_eq!(hits.len(), 1);
    let q = hits[0].quad;
    let xs = [q.ul.0, q.ur.0, q.ll.0, q.lr.0];
    let ys = [q.ul.1, q.ur.1, q.ll.1, q.lr.1];
    let w =
        xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min);
    let h =
        ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min);
    (w, h)
}

#[test]
fn a_hit_on_a_quarter_turned_line_is_tall_and_narrow() {
    let (flat_w, flat_h) = span(&mut session("1 0 0 1 50 100"));
    assert!(
        flat_w > 50.0 && (flat_h - 21.4).abs() < 0.01,
        "{flat_w} {flat_h}"
    );

    let mut turned = session("0 1 -1 0 100 50");
    let (w, h) = span(&mut turned);
    assert!((w - flat_h).abs() < 0.01, "across the line: {w}");
    assert!((h - flat_w).abs() < 0.01, "along the line: {h}");

    // The quad's corners follow the text: upper-left is the start of the
    // line's top edge, which on a quarter turn is to the left of the
    // baseline (x below 100) at the start (y = 50).
    let q = turned.find_text("HELLO", false)[0].quad;
    assert!(
        (q.ul.0 - (100.0 - 17.0)).abs() < 0.01 && (q.ul.1 - 50.0).abs() < 0.01,
        "{q:?}"
    );
    assert!((q.ll.0 - (100.0 + 4.4)).abs() < 0.01, "{q:?}");
}
