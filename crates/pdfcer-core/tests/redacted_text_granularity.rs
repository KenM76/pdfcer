//! `Pass 286.0` — `RedactionReport::redacted_text` carries the words a MARK
//! covered, not one entry per show operator.
//!
//! # The defect, reported from a real file
//!
//! `SW41177-obselete.pdf` (GPL Ghostscript 8.15) draws **one glyph per show
//! operator**. Marking the run `3.5 TYP` and applying gave:
//!
//! ```text
//! redacted_text (7): ["3", ".", "5", " ", "T", "Y", "P"]
//! ```
//!
//! The consuming shell's absence proof greps every content stream of the
//! output for each entry, finds `"3"` on all 24 pages — because every
//! engineering drawing has a 3 on it — and refuses the redaction. In the
//! operator's words, *"found 35 piece(s) of the supposedly-removed text still
//! in it"*. **The proof was not finding leaked text; it was finding the
//! alphabet.**
//!
//! # ★★ Three consumers, and why joining is safe for all of them
//!
//! `redacted_text` is not read by one caller:
//!
//! 1. the consuming shell's absence proof (greps the output for each entry);
//! 2. `carrier_info` (derives matching evidence via `redaction_evidence`);
//! 3. `residual_sweep` (`Pass 284.0`, same evidence function).
//!
//! Joining makes every one of them **strictly better**: longer strings match
//! more precisely, and they clear `MIN_MATCH_LEN` — the 4-character floor that
//! single glyphs could never reach, which is why `carrier_info` reported
//! `DISCLOSED_NOT_SCRUBBED` on exactly these files. **A change that SPLIT
//! entries would not have been safe**, and that asymmetry is the reason this
//! Pass could touch a field with three readers at once.
//!
//! # How the grouping is derived
//!
//! `Surgeon::glyph` returns the **index of the region** a glyph landed in
//! rather than a bare `bool`; removed characters accumulate per region; and
//! `box_marks` (region index → the `/Redact` annotation that contributed it,
//! already built for other reasons) folds several quads of one mark into one
//! string. Nothing new is inferred — the attribution already existed, it was
//! being discarded one line before it was needed.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;

/// A one-page PDF that draws `3.5 TYP` **one glyph per `Tj`**, the way GPL
/// Ghostscript 8.15 does.
///
/// ★ The fixture is the finding. A file that draws the run in a single `Tj`
/// cannot distinguish the old behaviour from the new one — both report
/// `["3.5 TYP"]` — so a test written on an ordinary producer would have been
/// green before this Pass and green after it, measuring nothing.
fn per_glyph_pdf() -> Vec<u8> {
    let mut content = String::from("BT /F1 12 Tf 40 200 Td\n");
    for (i, ch) in "3.5 TYP".chars().enumerate() {
        // One show operator per glyph, each positioned absolutely, exactly as
        // the reported producer emits.
        let x = 40.0 + (i as f64) * 8.0;
        content.push_str(&format!("1 0 0 1 {x} 200 Tm ({ch}) Tj\n"));
    }
    content.push_str("ET\n");

    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_string(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
        (
            5,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ),
    ];

    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let high = bodies.len() as u32 + 1;
    buf.extend_from_slice(format!("xref\n0 {high}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for n in 1..high {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map(|(_, o)| *o)
            .expect("every object number has an offset");
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {high} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Mark one rectangle covering the whole run and apply.
fn redact_whole_run() -> pdfcer_core::redact::RedactionReport {
    let doc = Document::from_bytes(per_glyph_pdf()).expect("the fixture loads");
    let mut session = EditSession::new(doc);
    let spec = pdfcer_core::annot_author::RedactSpec {
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (35.0, 215.0),
            ur: (110.0, 215.0),
            ll: (35.0, 195.0),
            lr: (110.0, 195.0),
        }],
        fill: None,
        overlay_text: None,
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    session.add_redaction(0, &spec).expect("mark the run");
    session.apply_redactions().expect("apply")
}

/// ★★★ THE DEFECT: one entry, carrying the words, not seven carrying letters.
#[test]
fn a_per_glyph_producer_yields_one_entry_per_mark() {
    let report = redact_whole_run();

    assert_eq!(
        report.redacted_text.len(),
        1,
        "one mark must produce one entry, got {:?}",
        report.redacted_text
    );
    assert_eq!(
        report.redacted_text[0], "3.5 TYP",
        "the entry must be the words the mark covered"
    );
}

/// ★★ THE CONSEQUENCE THAT MOTIVATED THE REPORT: the entry is long enough to
/// verify against.
///
/// The consuming shell's proof, and pdfcer's own `redaction_evidence`, both
/// apply a 4-character floor. Seven single characters cleared it zero times;
/// `"3.5 TYP"` clears it. Asserting the *length* rather than the exact string
/// states the property the consumers actually depend on — a future change that
/// joined differently but still produced a verifiable run would keep this
/// green, and one that regressed to per-operator would not.
#[test]
fn the_entry_clears_the_four_character_verification_floor() {
    let report = redact_whole_run();
    assert!(
        report.redacted_text.iter().all(|t| t.chars().count() >= 4),
        "every entry must be long enough to grep for without matching the \
         alphabet, got {:?}",
        report.redacted_text
    );
}

/// ★ THE CONTROL: the glyphs really were removed.
///
/// Without it, an implementation that reported a tidy joined string while
/// removing nothing — or removing the wrong glyphs — would satisfy both
/// assertions above. The report's own count and the saved bytes are checked,
/// because the report is the thing under test and cannot be its own witness.
#[test]
fn the_joined_entry_describes_a_removal_that_actually_happened() {
    let doc = Document::from_bytes(per_glyph_pdf()).expect("loads");
    let mut session = EditSession::new(doc);
    let spec = pdfcer_core::annot_author::RedactSpec {
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (35.0, 215.0),
            ur: (110.0, 215.0),
            ll: (35.0, 195.0),
            lr: (110.0, 195.0),
        }],
        fill: None,
        overlay_text: None,
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    session.add_redaction(0, &spec).expect("mark");
    let report = session.apply_redactions().expect("apply");

    assert_eq!(
        report.glyphs_removed, 7,
        "all seven glyphs of the run were removed"
    );

    let (bytes, _) = session
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
        .expect("save");
    assert!(
        !bytes.windows(5).any(|w| w == b"(T) Tj"[..5].as_ref()),
        "the show operators that drew the run must be gone"
    );
}

/// ★★ TWO marks produce TWO entries, each with its own words.
///
/// This is what makes the grouping *per mark* rather than *per page*. A
/// single-mark fixture cannot tell the two apart: joining everything on the
/// page into one string would satisfy every assertion above.
#[test]
fn two_marks_produce_two_entries_and_do_not_merge() {
    let doc = Document::from_bytes(per_glyph_pdf()).expect("loads");
    let mut session = EditSession::new(doc);

    let quad = |x0: f64, x1: f64| pdfcer_core::annot_author::RedactSpec {
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (x0, 215.0),
            ur: (x1, 215.0),
            ll: (x0, 195.0),
            lr: (x1, 195.0),
        }],
        fill: None,
        overlay_text: None,
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    // `3.5` sits at x = 40..64; `TYP` at x = 64..88.
    session.add_redaction(0, &quad(35.0, 63.0)).expect("mark 1");
    session
        .add_redaction(0, &quad(65.0, 110.0))
        .expect("mark 2");
    let report = session.apply_redactions().expect("apply");

    assert_eq!(
        report.redacted_text.len(),
        2,
        "two marks, two entries, got {:?}",
        report.redacted_text
    );
    assert!(
        report.redacted_text.iter().any(|t| t.contains('T')),
        "one entry carries the second run: {:?}",
        report.redacted_text
    );
}
