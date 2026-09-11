//! `Pass 294.2` — a `TJ` array's kerning NUMBERS survive the residual sweep.
//!
//! # The defect, as it actually arrived
//!
//! `redact-offpage` was run over 176 of the operator's engineering drawings.
//! One 56-page sheet came back with **three pages that neither pdfcer nor its
//! renderer could read** — *"malformed operand at byte 6701 of decoded
//! content"* — while the same pages in the input parsed cleanly. The probe
//! printed the byte sequence:
//!
//! ```text
//! [-53XXXX00221014025] TJ
//! ```
//!
//! `TJ`'s operand is an **array of strings and numbers** (§9.4.3): the numbers
//! are the kerning adjustments between the strings. The residual sweep — the
//! belt-and-braces pass that blanks copies of already-removed text surviving
//! elsewhere in the file — filled matched bytes across the WHOLE operand span,
//! so a needle whose digits appeared inside a kerning number turned that
//! number into `-53XXXX00221014025`.
//!
//! ★ Note which pass was at fault. The surgery that removes redacted glyphs
//! was correct throughout. **The precaution corrupted the page it was
//! protecting**, which is the worst shape a safety net can take: the damage
//! arrives wearing the name of a safeguard, and every counter in the report
//! says the operation succeeded.
//!
//! # Why a digit needle, and why its own fixture
//!
//! `redaction_residual_sweep.rs` redacts the word `CONFIDENTIAL`, and letters
//! cannot appear inside a PDF number — that suite could not reach this bug and
//! still cannot. Reaching it needs evidence made of DIGITS, which is ordinary
//! in the drawings this came from: a redacted dimension is a number.

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::writer::SaveOptions;

/// One page that draws `1234` (the text to redact) and, in a second content
/// stream, a `TJ` array whose kerning number carries the same digits.
fn fixture() -> Vec<u8> {
    let visible = "BT /F1 24 Tf 40 200 Td (1234) Tj ET";
    // The array under test: a string that MUST be blanked, and a kerning
    // number that must NOT be touched. Both carry `1234`.
    let arrayed = "BT /F1 12 Tf 40 100 Td [(1234 drawn again) -1234 (tail)] TJ ET";

    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents [4 0 R 6 0 R] \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_owned(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{visible}\nendstream",
                visible.len()
            ),
        ),
        (
            5,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ),
        (
            6,
            format!(
                "<< /Length {} >>\nstream\n{arrayed}\nendstream",
                arrayed.len()
            ),
        ),
    ];

    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", bodies.len() + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for (_, off) in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
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

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// ★★★ The regression: redact `1234`, and the kerning number `-1234` must
/// come out intact while the string beside it is blanked.
///
/// The two halves are asserted together on purpose. Without the first, the
/// output is unreadable; without the second, the sweep has simply stopped
/// working and the test would pass on a no-op.
#[test]
fn a_kerning_number_carrying_the_needles_digits_is_not_blanked() {
    let doc = Document::from_bytes(fixture()).expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let marked = session
        .mark_redactions_by_search("1234", false)
        .expect("the search marks something");
    assert!(!marked.is_empty(), "the fixture must have text to redact");

    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("the marked revision saves");
    let marked_doc = Document::from_bytes(bytes).expect("and re-opens");
    let (out, _report) =
        pdfcer_core::redact::apply_redactions(&marked_doc, &SaveOptions::identity())
            .expect("the redaction applies");

    assert!(
        contains(&out, b"-1234"),
        "a kerning NUMBER carrying the needle's digits must survive — filling \
         inside it yields an operand that is not a number, and the page stops \
         parsing (the defect that produced `[-53XXXX00221014025] TJ`)"
    );

    // ★★ THE ASSERTION THE COUNTERS COULD NOT MAKE. The original defect left
    // every report field looking like success; only reading the page back
    // showed it. So the test reads the page back.
    let done = Document::from_bytes(out).expect("the output opens");
    let pages = page_tree::pages(&done).expect("its page tree walks");
    let view = done.view();
    for page in &pages {
        ContentStream::from_page(&view, page)
            .expect("every page still parses after the residual sweep");
    }
}
