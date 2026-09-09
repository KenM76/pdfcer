//! `Pass 284.0`/`285.0` — redaction sweeps the FILE, not the document graph.
//!
//! # The defect these tests pin
//!
//! Every carrier pass in `redact` found its target by **navigating the
//! document graph**: the trailer's `/Info`, the catalog's `/Metadata`, the
//! page tree's content streams. `writer::save_full` emits objects by
//! **enumerating the cross-reference table**. Those are different sets, and
//! every object in the difference was re-emitted **verbatim** into a redacted
//! file, having never been offered to a carrier — while the report said
//! `info action=scrubbed`.
//!
//! ★ **The worst combination**: no report line was false, and the content was
//! still there. `prior_revisions action=dropped_by_rewrite` is true — it is
//! about superseded *byte ranges*, not about objects the cross-reference table
//! still names.
//!
//! # ★★ Why the fix sweeps by EVIDENCE and never computes reachability
//!
//! §12.5.6.23 is an outcome test on the saved artifact — *"they shall remove
//! all traces of the specified content"* — and scopes carriers by *"all
//! content that can exist in a PDF document"*. It never mentions the object
//! graph. Meanwhile reachability is a trap: object streams are reached by a
//! **type-2 xref entry**, cross-reference streams by **byte offset**, and the
//! linearization dictionary is unreferenced by a `shall` (Annex F.3.3) — and
//! §7.3.10 makes a wrong drop **silent**, because a reference to a missing
//! object is *"not … an error"*.
//!
//! `examples/unreachable_census.rs` made that exact error twice within an hour
//! of the clause being read. So the remedy does not depend on the computation
//! that failed: it looks for the redacted words, wherever they are.
//!
//! # `Pass 285.0` — the abandoned content stream, and what still is not done
//!
//! `Pass 284.0` **named** a non-metadata stream carrying redacted text rather
//! than editing it, because blanking arbitrary bytes would corrupt a font
//! programme or an image on a coincidental match. `Pass 285.0` found the
//! discriminator that makes the edit safe: **parse the buffer as a content
//! stream and blank only the spans of operands belonging to `Tj`/`TJ`/`'`/`"`
//! (§9.4.3).** Parsing is also the discriminator — a font or an image does not
//! parse as a content stream — and the edit is length-preserving.
//!
//! ★★ **A sabotage survived the first cut of these tests and is why the
//! fixture looks the way it does.** Replacing the operand span with the whole
//! buffer left all eight green, because the fixture put the redacted word only
//! inside a string. The orphan stream now also carries it in a **resource
//! name** (`/CONFIDENTIALIm Do`), which a whole-buffer blank would rewrite
//! into a name resolving to nothing — silently stopping an image from drawing.
//! `an_abandoned_content_streams_drawn_text_is_blanked` asserts that name
//! survives.
//!
//! **Still declined, and still disclosed:** a stream that does not parse as a
//! content stream, and text drawn through a subset font whose operand bytes
//! are glyph codes rather than characters.
//! `a_stream_that_cannot_be_blanked_is_still_named` pins that the honest half
//! of `Pass 284.0` survives exactly where it is still correct.

use pdfcer_core::document::Document;
use pdfcer_core::redact::{CarrierAction, RedactionReport};
use pdfcer_core::writer::SaveOptions;

/// Build a one-page PDF whose trailer `/Info` names object 5, while objects 6,
/// 8 and 9 are metadata-, content- and XMP-shaped objects the trailer names
/// **nothing** about — listed in the cross-reference table and reachable from
/// no reference in the file.
fn pdf_with_orphans() -> Vec<u8> {
    let content = "BT /F1 24 Tf 40 200 Td (CONFIDENTIAL) Tj ET";
    // ★ The resource name carries the redacted word TOO, deliberately. It is
    // what makes a whole-buffer blank distinguishable from a span-scoped one:
    // a sweep that overwrote every occurrence would rewrite this name into
    // one that resolves to nothing, silently stopping an image from drawing.
    // A sabotage that did exactly that survived the first cut of these tests,
    // because the fixture put the word only inside the string.
    let orphan_stream =
        "q /CONFIDENTIALIm Do Q BT /F1 24 Tf 40 100 Td (CONFIDENTIAL stream orphan) Tj ET";
    let xmp = "<?xpacket begin='' ?><x:xmpmeta><dc:title>CONFIDENTIAL xmp orphan</dc:title>\
</x:xmpmeta><?xpacket end='w'?>";

    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
             /Resources << /Font << /F1 7 0 R >> >> >>"
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
            "<< /Title (Live info) /Keywords (CONFIDENTIAL live) >>".to_string(),
        ),
        (
            6,
            "<< /Title (Superseded info) /Keywords (CONFIDENTIAL orphan) >>".to_string(),
        ),
        (
            7,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ),
        (
            8,
            format!(
                "<< /Length {} >>\nstream\n{orphan_stream}\nendstream",
                orphan_stream.len()
            ),
        ),
        (
            9,
            format!(
                "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xmp}\nendstream",
                xmp.len()
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
        format!(
            "trailer\n<< /Size {high} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    buf
}

/// The orphan fixture plus object 10: a stream carrying the redacted word in a
/// string literal that **no text-showing operator consumes**.
///
/// This is the shape `blank_show_strings` must decline — it can see the word
/// but cannot prove the bytes are drawn text, and blanking on that basis is
/// the coincidence-corrupts-content trade the sweep refuses.
fn pdf_with_unblankable_stream() -> Vec<u8> {
    let base = pdf_with_orphans();
    let text = String::from_utf8(base).expect("the fixture is ASCII");

    let blob = "(CONFIDENTIAL not drawn) 0 0 0 rg";
    let obj = format!(
        "10 0 obj\n<< /Length {} >>\nstream\n{blob}\nendstream\nendobj\n",
        blob.len()
    );

    // Splice the object in before the xref and rebuild the table, so the
    // fixture stays offset-consistent rather than relying on recovery.
    let head = text
        .split_once("xref\n")
        .map(|(h, _)| h.to_string())
        .expect("the fixture has a classic table");
    let mut buf = head;
    let obj_at = buf.len();
    buf.push_str(&obj);

    // Re-derive every object's offset from the spliced body.
    let xref_at = buf.len();
    let mut lines = String::from("xref\n0 11\n0000000000 65535 f \n");
    for n in 1..=10u32 {
        let needle = format!("\n{n} 0 obj\n");
        let off = if n == 10 {
            obj_at
        } else {
            buf.find(&needle)
                .map(|i| i + 1)
                .unwrap_or_else(|| panic!("object {n} is in the spliced body"))
        };
        lines.push_str(&format!("{off:010} 00000 n \n"));
    }
    buf.push_str(&lines);
    buf.push_str(&format!(
        "trailer\n<< /Size 11 /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
    ));
    buf.into_bytes()
}

/// Redact `CONFIDENTIAL` from the orphan fixture and return the saved bytes
/// plus the report.
fn redact_orphan_fixture() -> (Vec<u8>, RedactionReport) {
    let doc = Document::from_bytes(pdf_with_orphans()).expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let marked = session
        .mark_redactions_by_search("CONFIDENTIAL", false)
        .expect("mark the region");
    assert!(!marked.is_empty(), "the search found the drawn text");
    let report = session.apply_redactions().expect("apply the redaction");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("a redacted session saves");
    (bytes, report)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn carrier(report: &RedactionReport, name: &str) -> Option<CarrierAction> {
    report
        .carriers
        .iter()
        .find(|c| c.carrier == name)
        .map(|c| c.action)
}

// ------------------------------------------------- 1. the reported defect

/// ★★★ THE DEFECT: an `/Info`-shaped dictionary the trailer does not name
/// survived a redaction with the redacted word intact.
#[test]
fn a_superseded_info_dictionary_is_scrubbed_even_though_nothing_points_at_it() {
    let (bytes, _report) = redact_orphan_fixture();
    assert!(
        !contains(&bytes, b"CONFIDENTIAL orphan"),
        "the orphaned /Info-shaped dictionary still carries the redacted word"
    );
}

/// The live `/Info` is still scrubbed — the control that proves the new sweep
/// did not displace the carrier it was added beside.
///
/// Without this, a sweep that scrubbed orphans while silently breaking
/// `carrier_info` would pass every other assertion in this file.
#[test]
fn the_trailers_own_info_is_still_scrubbed() {
    let (bytes, report) = redact_orphan_fixture();
    assert!(!contains(&bytes, b"CONFIDENTIAL live"));
    assert_eq!(carrier(&report, "info"), Some(CarrierAction::Scrubbed));
    assert_eq!(report.info_strings_scrubbed, 1);
}

/// An XMP packet attached to nothing is scrubbed wherever it sits.
///
/// ★ §14.3.2 NOTE 3 is why this one cannot be left as a disclosure: an XMP
/// packet is designed to be found *"by simple scanning rather than requiring
/// the document file to be parsed"*. Reachability is irrelevant to its
/// exposure **by design**, so "no reader will read it" is not available as an
/// argument for leaving it.
#[test]
fn an_orphaned_xmp_packet_is_scrubbed_wherever_it_is_attached() {
    let (bytes, _report) = redact_orphan_fixture();
    assert!(
        !contains(&bytes, b"CONFIDENTIAL xmp"),
        "an unreferenced XMP packet still carries the redacted word"
    );
}

// -------------------------------- 2. the abandoned content stream (`Pass 285.0`)

/// ★★★ An abandoned content stream's drawn text is **blanked**, not merely
/// named.
///
/// # This test was AMENDED, not replaced, and the old assertion is below
///
/// At `Pass 284.0` this test asserted the opposite — that the stream was still
/// present and the sweep reported `DisclosedNotScrubbed` naming object `8 0`.
/// That was **right for its Pass**: blanking arbitrary stream bytes on a
/// coincidental match would corrupt a font programme or an image, and naming
/// the object was the honest answer available at the time.
///
/// `Pass 285.0` found the discriminator that makes blanking safe, so the
/// disclosure is no longer the best pdfcer can do. Both halves are kept: this
/// one pins that the text is **gone**, and
/// `a_stream_that_cannot_be_blanked_is_still_named` pins that the old
/// behaviour survives exactly where it is still correct.
///
/// ★ **The discriminator is parsing itself.** A font programme or an image
/// does not parse as a content stream, so the same call that locates the
/// strings proves the object is one — no `/Subtype` sniffing and no list of
/// types to keep in step with reality. And only the spans of operands
/// belonging to `Tj`/`TJ`/`'`/`"` are touched, so a resource name such as
/// `/CONFIDENTIAL Do` is never rewritten into one that resolves to nothing.
#[test]
fn an_abandoned_content_streams_drawn_text_is_blanked() {
    let (bytes, report) = redact_orphan_fixture();

    assert!(
        !contains(&bytes, b"CONFIDENTIAL stream"),
        "an abandoned content stream still draws the redacted word"
    );
    assert_eq!(
        report.residual_content_streams_blanked, 1,
        "and the edit is counted apart from the metadata scrubs"
    );

    // ★★ THE ASSERTION THAT MAKES SPAN-SCOPING MEASURABLE, and it exists
    // because a sabotage survived without it.
    //
    // Replacing the operand span with the WHOLE BUFFER — blanking every
    // occurrence of the evidence rather than only the show operator's
    // operands — left all eight tests green, because the fixture put the word
    // only inside the string. The resource name `/CONFIDENTIALIm` was added
    // for exactly this: a whole-buffer blank rewrites it to `/XXXXXXXXXXXXIm`,
    // which resolves to nothing and silently stops the image drawing —
    // content destroyed to fix a leak, the trade this sweep refuses
    // everywhere else.
    assert!(
        contains(&bytes, b"/CONFIDENTIALIm"),
        "the resource NAME must survive — only the show operator's string \
         operands may be blanked, or a redaction quietly breaks the page"
    );

    // ~~The `Pass 284.0` assertion, kept legible:~~
    // ~~assert!(contains(&bytes, b"CONFIDENTIAL stream"));~~
    // ~~assert_eq!(carrier(&report, "residual_sweep"),~~
    // ~~          Some(CarrierAction::DisclosedNotScrubbed));~~
}

/// ★ THE CONTROL FOR THE BLANKING: a stream carrying the redacted word that
/// **cannot** be blanked safely is still named.
///
/// Without this, a `blank_show_strings` that gave up silently — returning
/// `None` and skipping the disclosure — would pass every other assertion in
/// this file, and the honest half of `Pass 284.0` would have been deleted by
/// accident rather than by decision.
///
/// The fixture's object 10 carries the word inside a string literal that **no
/// show operator consumes**, which is the shape of a leak pdfcer can see and
/// must not guess about.
#[test]
fn a_stream_that_cannot_be_blanked_is_still_named() {
    let doc = Document::from_bytes(pdf_with_unblankable_stream()).expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session
        .mark_redactions_by_search("CONFIDENTIAL", false)
        .expect("mark");
    let report = session.apply_redactions().expect("apply");

    assert_eq!(
        carrier(&report, "residual_sweep"),
        Some(CarrierAction::DisclosedNotScrubbed),
        "a stream pdfcer cannot blank must still be disclosed"
    );
    assert!(
        report.notes.iter().any(|n| n.contains("10 0")),
        "the note must NAME the object, not merely say one exists: {:?}",
        report.notes
    );
}

// ------------------------------------------------- 3. the counts and controls

/// The sweep's counts are reported separately from `/Info`'s.
///
/// ★ A single total would hide the fact that the second number is the one
/// nobody expected to be non-zero.
#[test]
fn the_sweep_reports_its_own_counts() {
    let (_bytes, report) = redact_orphan_fixture();
    assert!(
        report.residual_sweep_entries_scrubbed >= 1,
        "the orphaned /Keywords entry is counted"
    );
    assert!(
        report.residual_sweep_objects_scrubbed >= 2,
        "the orphan dictionary and the orphan XMP packet are both counted"
    );
}

/// ★ THE CONTROL: a clean file reports the sweep as having found nothing.
///
/// Without it, an implementation that disclosed a residual for every document
/// would satisfy every assertion above and make the disclosure worthless —
/// which is exactly the failure mode the old `info action=scrubbed` had, in
/// the opposite direction.
#[test]
fn a_file_with_no_orphans_reports_the_sweep_clean() {
    let doc = Document::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/hello.pdf"),
    )
    .expect("load hello.pdf");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session
        .mark_redactions_by_search("Hello", false)
        .expect("mark");
    let report = session.apply_redactions().expect("apply");

    assert_eq!(
        carrier(&report, "residual_sweep"),
        Some(CarrierAction::CheckedClean),
        "a file with nothing off the graph must report CHECKED, not a residual"
    );
    assert!(!report.has_disclosed_residuals());
    assert_eq!(report.residual_sweep_entries_scrubbed, 0);
}

/// ★ A redaction that removes NO TEXT reports the sweep as not applicable —
/// not as a residual.
///
/// `redacted` empty and `evidence` empty are different facts: the first means
/// there was nothing for a text sweep to look for, the second means there was
/// something and it was too short to match on safely. The first cut of the
/// sweep collapsed them and turned two passing image-redaction tests red,
/// each asserting `!has_disclosed_residuals()` — correctly, because an
/// image-only redaction leaves no text residual.
#[test]
fn an_image_only_redaction_reports_the_sweep_as_not_applicable() {
    let doc = Document::from_bytes(pdf_with_orphans()).expect("loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    // A region over empty page area: a mark that covers no glyphs. The text
    // sits at y=200; this quad is nowhere near it.
    let spec = pdfcer_core::annot_author::RedactSpec {
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (10.0, 20.0),
            ur: (20.0, 20.0),
            ll: (10.0, 10.0),
            lr: (20.0, 10.0),
        }],
        fill: None,
        overlay_text: None,
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    session
        .add_redaction(0, &spec)
        .expect("mark an empty region");
    let report = session.apply_redactions().expect("apply");

    assert!(
        report.redacted_text.is_empty(),
        "the fixture for this test must redact no text, got {:?}",
        report.redacted_text
    );
    assert_eq!(
        carrier(&report, "residual_sweep"),
        Some(CarrierAction::Absent),
        "no text redacted means no text sweep applies"
    );
    // NOTE: `has_disclosed_residuals()` is deliberately NOT asserted here.
    // Other carriers disclose on this fixture for their own reasons, and
    // asserting the global flag would make this test fail for something it
    // does not own. The claim under test is the sweep's own carrier line.
    let disclosing: Vec<&str> = report
        .carriers
        .iter()
        .filter(|c| c.action == CarrierAction::DisclosedNotScrubbed)
        .map(|c| c.carrier)
        .collect();
    assert!(
        !disclosing.contains(&"residual_sweep"),
        "the sweep must not be among the disclosing carriers, got {disclosing:?}"
    );
}
