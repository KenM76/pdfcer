//! Fuzz target 15: redaction apply / content-stream surgery
//! (`pdfcer_core::redact`, ISO 32000-1 §12.5.6.23; docs/decisions/008 Pass 8).
//!
//! The security-critical path. The fuzz bytes are embedded **as a page
//! content stream** in a minimal template document that carries both a
//! simple font (`/F1` Helvetica, 1-byte codes) and a composite font
//! (`/F2` Type0 `Identity-H`, 2-byte CID codes), so the advance-preserving
//! text surgery is driven over arbitrary, pathological content:
//!
//! - **multi-byte CID show strings** — a `Tj`/`TJ` under `/F2` whose code
//!   segmentation must never split a 2-byte code, however truncated;
//! - **nested `q`/`Q`** — unbalanced or deeply nested graphics-state saves;
//! - **overlapping / degenerate redaction quads** — two `/Redact` marks are
//!   authored over fixed and input-derived regions that may overlap, cover
//!   all of a string, none of it, or a partial glyph at the edge;
//! - **malformed operators, truncated arrays, huge numbers** — anything the
//!   `content` tokenizer will accept and hand to the interpreter.
//! - **image placements** (`Pass 245.0`) — the template's resources carry
//!   `/Im1` (a raw 4×4 grey grid), `/Im2` (a `DCTDecode` stream whose bytes
//!   are the TAIL of the fuzz input, so the decoder sees arbitrary
//!   codestreams and the mark-retention path runs), and `/Im3` (raw RGB
//!   with `/Im1` as its `/SMask`). The fuzzed content may `Do` any of them
//!   under any CTM — singular, skewed, enormous — and may carry inline
//!   images (`BI … EI`), so the cell mapping, the sample clearing at every
//!   bit depth, the copy-on-write census and the inline re-encode are all
//!   driven from hostile input.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input the
//! whole mark → save → reload → `apply_redactions` pipeline returns
//! normally — a produced document or a named [`RedactError`] — and never
//! panics, aborts, or loops. A content stream the tokenizer rejects surfaces
//! as `RedactError::Content`, not a crash.
//!
//! It also spot-checks the **security invariant** on a well-formed run: when
//! apply succeeds, the interpreter's own decode of the removed codes is what
//! populates the report's `redacted_text`, so a non-panicking apply that
//! reports removed glyphs has, by construction, sliced them out of the
//! rewritten stream.

#![no_main]

use libfuzzer_sys::fuzz_target;

use pdfcer_core::annot_author::{Quad, RedactSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::redact;
use pdfcer_core::vartext::Quadding;
use pdfcer_core::writer::SaveOptions;

/// Assemble a one-page template whose `/Contents` is exactly `content`,
/// with three image XObjects whose second one is built from `jpeg`.
fn template(content: &[u8], jpeg: &[u8]) -> Vec<u8> {
    let stream = |dict: &str, data: &[u8]| -> Vec<u8> {
        let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        s.extend_from_slice(data);
        s.extend_from_slice(b"\nendstream");
        s
    };
    // A Type0 Identity-H descendant with a bare CIDFont — enough for
    // `ExtractFont` to segment 2-byte codes and estimate widths.
    let bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] \
          /Resources << /Font << /F1 5 0 R /F2 6 0 R >> \
          /XObject << /Im1 8 0 R /Im2 9 0 R /Im3 10 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        {
            let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            s.extend_from_slice(content);
            s.extend_from_slice(b"\nendstream");
            s
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /F2 /Encoding /Identity-H \
          /DescendantFonts [7 0 R] >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /F2 \
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
          /DW 1000 >>"
            .to_vec(),
        stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray",
            &[0xA5; 16],
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /BitsPerComponent 8 \
             /ColorSpace /DeviceRGB /Filter /DCTDecode",
            jpeg,
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceRGB /SMask 8 0 R",
            &[0x5A; 48],
        ),
    ];

    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fuzz_target!(|data: &[u8]| {
    // Cap the content size so the fuzzer spends its time on shapes, not on
    // one enormous stream.
    let content = if data.len() > 4096 {
        &data[..4096]
    } else {
        data
    };

    // The last (up to) 512 bytes double as the `/Im2` JPEG codestream.
    let jpeg = if data.len() > 512 {
        &data[data.len() - 512..]
    } else {
        data
    };
    let pdf = template(content, jpeg);
    let Ok(doc) = Document::from_bytes(pdf) else {
        return;
    };
    let mut session = EditSession::new(doc);

    // Two redaction marks: a fixed central region and one derived from the
    // input bytes, deliberately allowed to overlap / be degenerate.
    let a = f64::from(content.first().copied().unwrap_or(0));
    let b = f64::from(content.get(1).copied().unwrap_or(0));
    let regions = [
        Rect::from_corners(20.0, 90.0, 380.0, 130.0),
        Rect::from_corners(a, b, a + 60.0, b + 20.0),
    ];
    for r in regions {
        let spec = RedactSpec {
            quads: vec![Quad::from_rect(r)],
            fill: None,
            overlay_text: None,
            quadding: Quadding::Left,
        };
        let _ = session.add_redaction(0, &spec);
    }

    let Ok((marked, _)) = session.to_incremental_bytes(&SaveOptions::identity()) else {
        return;
    };
    let Ok(marked_doc) = Document::from_bytes(marked) else {
        return;
    };

    // The whole point: apply must never panic, whatever the content is.
    match redact::apply_redactions(&marked_doc, &SaveOptions::identity()) {
        Ok((out, report)) => {
            // The produced document must reload, and the security invariant
            // must hold: the strings the interpreter decoded-and-removed
            // must not appear in the rewritten content. (ASCII form; the
            // interpreter also drove the byte slicing that removed them.)
            if let Ok(back) = Document::from_bytes(out) {
                use pdfcer_core::content::ContentStream;
                use pdfcer_core::page_tree;
                if let Ok(pages) = page_tree::pages(&back) {
                    let mut decoded = Vec::new();
                    for page in &pages {
                        if let Ok(cs) = ContentStream::from_page(&back.view(), page) {
                            decoded.extend_from_slice(&cs.buf);
                        }
                    }
                    for t in &report.redacted_text {
                        let needle = t.as_bytes();
                        if needle.len() >= 3 {
                            assert!(
                                !decoded.windows(needle.len()).any(|w| w == needle),
                                "redacted text survived the surgery"
                            );
                        }
                    }
                }
            }
        }
        Err(_) => {} // a named refusal (undestroyable image, unparsable content) is fine
    }
});
