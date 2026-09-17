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
use pdfcer_core::redact::{CarrierAction, RedactionReport, ResidualScope};
use pdfcer_core::writer::SaveOptions;

/// Build a one-page PDF whose trailer `/Info` names object 5, while objects 6,
/// 8 and 9 are metadata-, content- and XMP-shaped objects the trailer names
/// **nothing** about — listed in the cross-reference table and reachable from
/// no reference in the file.
fn pdf_with_orphans() -> Vec<u8> {
    pdf_with_orphans_drawing("CONFIDENTIAL", &["CONFIDENTIAL stream orphan"])
}

/// [`pdf_with_orphans`] with the drawn text of the live page and of the
/// orphaned stream chosen by the caller.
///
/// The orphan draws one show-string per entry of `orphan_texts`, on its own
/// line. Two shapes matter to the scope tests and neither is reachable with a
/// single string: an orphan that shares only a **word** of the redacted phrase,
/// and an orphan that carries a whole redacted run **and** an unrelated line
/// starting with the same word.
fn pdf_with_orphans_drawing(page_text: &str, orphan_texts: &[&str]) -> Vec<u8> {
    let content = format!("BT /F1 24 Tf 40 200 Td ({page_text}) Tj ET");
    // ★ The resource name carries the redacted word TOO, deliberately. It is
    // what makes a whole-buffer blank distinguishable from a span-scoped one:
    // a sweep that overwrote every occurrence would rewrite this name into
    // one that resolves to nothing, silently stopping an image from drawing.
    // A sabotage that did exactly that survived the first cut of these tests,
    // because the fixture put the word only inside the string.
    let shows: String = orphan_texts
        .iter()
        .map(|t| format!("0 -30 Td ({t}) Tj "))
        .collect();
    let orphan_stream = format!("q /CONFIDENTIALIm Do Q BT /F1 24 Tf 40 130 Td {shows}ET");
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
    redact_orphan_fixture_at(ResidualScope::default())
}

/// [`redact_orphan_fixture`] under an explicitly chosen [`ResidualScope`].
///
/// ★ `Pass 310.0`: the scope is now the variable these tests turn. A test that
/// asserts drawable content OUTSIDE the marked region was edited must say
/// `WholeDocument` out loud, because that is no longer what a caller gets by
/// default — and a test that asserts it was LEFT must say which narrower scope
/// it is claiming about.
fn redact_orphan_fixture_at(scope: ResidualScope) -> (Vec<u8>, RedactionReport) {
    let doc = Document::from_bytes(pdf_with_orphans()).expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session.set_residual_scope(scope);
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
///
/// ★★ `Pass 310.0` RE-POINTED THIS TEST, and the re-pointing is the finding.
///
/// It used to call `redact_orphan_fixture()` — the DEFAULT — and that is how
/// blanking unmarked drawable content came to look like settled behaviour. It
/// was never restricted to abandoned streams: the sweep has no liveness test
/// of any kind, so the same code blanks a live page, an annotation appearance
/// and a form XObject. The operator reported exactly that.
///
/// The claim survives unchanged, as an OPT-IN. What changed is that a caller
/// now has to ask for it by name.
///
/// ~~The `Pass 285.0` call, kept legible:~~
/// ~~    let (bytes, report) = redact_orphan_fixture();~~
#[test]
fn an_abandoned_content_streams_drawn_text_is_blanked() {
    let (bytes, report) = redact_orphan_fixture_at(ResidualScope::WholeDocument);

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
    // `WholeDocument` for the same reason as the test above: "cannot blank" is
    // only a reachable answer once blanking has been attempted, and no
    // narrower scope attempts it (`Pass 310.0`).
    session.set_residual_scope(ResidualScope::WholeDocument);
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

// ------------------------------------------------- 3. `Pass 310.0` — scope

/// ★★★ THE OPERATOR'S REPORT: redacting one region blanked matching text he
/// never selected. Under the DEFAULT scope it must not.
///
/// The orphan stream draws `CONFIDENTIAL stream orphan` — the redacted word,
/// in a content stream the surgery never touched. Before `Pass 310.0` that
/// text was blanked, and because the sweep has no liveness test the same would
/// have happened to a live page.
#[test]
fn the_default_scope_leaves_drawable_text_outside_the_marks_alone() {
    let (bytes, report) = redact_orphan_fixture_at(ResidualScope::default());

    assert!(
        contains(&bytes, b"CONFIDENTIAL stream orphan"),
        "the default scope must NOT edit drawable content outside the marked regions"
    );
    assert_eq!(
        report.residual_content_streams_blanked, 0,
        "and the blanking counter must agree that nothing was blanked"
    );
    assert!(
        report.residual_matches_left >= 1,
        "the match is still FOUND and counted — the scope changes what pdfcer \
         edits, never what it tells you"
    );
    assert_eq!(
        carrier(&report, "residual_sweep"),
        Some(CarrierAction::FoundNotScrubbed),
        "found-and-left is its own carrier action, not a silent clean"
    );
    assert!(
        report.notes.iter().any(|n| n.contains("8 0")),
        "the note must NAME the object left in place: {:?}",
        report.notes
    );
}

/// ★ THE OTHER HALF, and without it the fix above would be indistinguishable
/// from switching the sweep off.
///
/// The default scope still scrubs what the operator cannot see: the orphaned
/// `/Info`-shaped dictionary and the orphaned XMP packet.
#[test]
fn the_default_scope_still_scrubs_the_invisible_carriers() {
    let (bytes, report) = redact_orphan_fixture_at(ResidualScope::default());

    assert!(
        !contains(&bytes, b"CONFIDENTIAL orphan"),
        "the orphaned /Keywords entry must still go"
    );
    assert!(
        !contains(&bytes, b"CONFIDENTIAL xmp orphan"),
        "the orphaned XMP packet must still be blanked"
    );
    assert!(
        report.residual_sweep_entries_scrubbed >= 1,
        "and the scrub is counted"
    );
}

/// `MarkedOnly` acts on the marked region and nothing else — including the
/// invisible carriers, which it reports rather than scrubs.
#[test]
fn marked_only_leaves_every_carrier_alone_and_reports_each() {
    let (bytes, report) = redact_orphan_fixture_at(ResidualScope::MarkedOnly);

    assert!(
        contains(&bytes, b"CONFIDENTIAL orphan"),
        "marked-only must not touch the orphaned /Keywords entry"
    );
    assert!(
        contains(&bytes, b"CONFIDENTIAL stream orphan"),
        "marked-only must not touch drawable content either"
    );
    assert_eq!(
        carrier(&report, "info"),
        Some(CarrierAction::FoundNotScrubbed),
        "the live /Info quotes the redacted word and is REPORTED, not scrubbed"
    );
    assert!(
        report.residual_matches_left >= 2,
        "every declined carrier is counted: {}",
        report.residual_matches_left
    );
    assert_eq!(
        report.info_strings_scrubbed, 0,
        "`info_strings_scrubbed` means REMOVED — a declined entry must not \
         inflate it, or a shell reading the count would report a scrub that \
         did not happen"
    );
}

/// ★★ THE BLAST-RADIUS TEST: redacting `INVOICE 4412` must not blank the word
/// `INVOICE` somewhere else, in ANY scope.
///
/// This is the second of the two independent causes behind the operator's
/// report. `redaction_evidence` expands each redacted run into the run plus
/// every whitespace-delimited token over the four-character floor, so
/// `INVOICE` became a needle in its own right. Token matching earns its keep
/// on invisible carriers, where an over-scrub costs a keyword; on drawable
/// content it is destruction. `redaction_runs` is the split.
///
/// Asserted under `WholeDocument` deliberately — the WIDEST scope, where the
/// sweep is at its most destructive, is where this must still hold.
#[test]
fn a_shared_word_does_not_blank_an_unmarked_stream_even_whole_document() {
    let doc = Document::from_bytes(pdf_with_orphans_drawing(
        "INVOICE 4412",
        &["INVOICE totals follow"],
    ))
    .expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session.set_residual_scope(ResidualScope::WholeDocument);
    let marked = session
        .mark_redactions_by_search("INVOICE 4412", false)
        .expect("mark the region");
    assert!(!marked.is_empty(), "the search found the drawn phrase");
    let report = session.apply_redactions().expect("apply");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("a redacted session saves");

    assert!(
        contains(&bytes, b"INVOICE totals follow"),
        "a stream sharing only a WORD of the redacted phrase must survive \
         even the whole-document scope; report was {:?}",
        report.notes
    );
    assert_eq!(
        report.residual_content_streams_blanked, 0,
        "nothing was a whole-phrase match, so nothing may be blanked"
    );
}

/// `FoundNotScrubbed` must NOT trip the disclosed-residual flag, because that
/// flag drives the CLI's non-zero exit and its `--acknowledge-residuals`
/// override.
///
/// ★ The two facts are different and collapsing them would be the nagging that
/// project rule 4 exists to prevent: `DisclosedNotScrubbed` means pdfcer could
/// not finish the job, `FoundNotScrubbed` means it was told not to. Every
/// ordinary redaction of a phrase that also appears elsewhere would exit
/// non-zero.
#[test]
fn found_not_scrubbed_is_reported_without_failing_the_redaction() {
    let (_bytes, report) = redact_orphan_fixture_at(ResidualScope::default());

    assert!(
        report.has_unscrubbed_matches(),
        "the found-and-left question must answer yes"
    );
    assert!(
        !report
            .carriers
            .iter()
            .any(|c| c.carrier == "residual_sweep"
                && c.action == CarrierAction::DisclosedNotScrubbed),
        "and the sweep must not ALSO claim it could not act"
    );
}

/// Blanking a stream that DOES carry a whole redacted run must still take only
/// that run, not every word of it.
///
/// ★ This is the only fixture shape that measures the needle handed to
/// [`blank_show_strings`]. Its sibling above asserts a stream carrying no whole
/// run is untouched, which the DETECTION split alone satisfies — swapping the
/// blanker's whole-run needles back for tokenized evidence leaves that test
/// green, because the blanker is never reached. Here it is reached, and a
/// tokenized needle would erase `INVOICE summary` on the way past.
#[test]
fn blanking_a_matching_stream_does_not_take_its_other_words_with_it() {
    let doc = Document::from_bytes(pdf_with_orphans_drawing(
        "INVOICE 4412",
        &["INVOICE 4412", "INVOICE summary"],
    ))
    .expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session.set_residual_scope(ResidualScope::WholeDocument);
    let marked = session
        .mark_redactions_by_search("INVOICE 4412", false)
        .expect("mark the region");
    assert!(!marked.is_empty(), "the search found the drawn phrase");
    let report = session.apply_redactions().expect("apply");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("a redacted session saves");

    assert_eq!(
        report.residual_content_streams_blanked, 1,
        "the stream carrying the whole run IS blanked under whole-document; \
         notes were {:?}",
        report.notes
    );
    assert!(
        !contains(&bytes, b"INVOICE 4412"),
        "and the whole run is gone from it"
    );
    assert!(
        contains(&bytes, b"INVOICE summary"),
        "but the line sharing only a word survives — the blanker's needles are \
         whole runs, not tokens; notes were {:?}",
        report.notes
    );
}

/// A drawable stream carrying the redacted run in a DIFFERENT CASE is blanked,
/// not merely disclosed.
///
/// ★ This measures the detector and the actor against each other. The detector
/// has always been ASCII-case-insensitive; the blanker compared exact bytes,
/// so `Invoice 4412` against a redacted `INVOICE 4412` was FOUND, not removed,
/// and fell through to `DisclosedNotScrubbed` — pdfcer reporting a residual it
/// could have taken out. Both now share `text_match_ranges`, so the fixture
/// asserts the stronger fact: the object is gone, and the sweep does not claim
/// it failed.
#[test]
fn a_case_variant_of_the_run_is_blanked_rather_than_disclosed() {
    let doc = Document::from_bytes(pdf_with_orphans_drawing("INVOICE 4412", &["Invoice 4412"]))
        .expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    session.set_residual_scope(ResidualScope::WholeDocument);
    let marked = session
        .mark_redactions_by_search("INVOICE 4412", false)
        .expect("mark the region");
    assert!(!marked.is_empty(), "the search found the drawn phrase");
    let report = session.apply_redactions().expect("apply");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("a redacted session saves");

    assert_eq!(
        report.residual_content_streams_blanked, 1,
        "the case variant is blanked, not disclosed; notes were {:?}",
        report.notes
    );
    assert!(
        !contains(&bytes, b"Invoice 4412"),
        "and it is gone from the saved bytes"
    );
    assert_ne!(
        carrier(&report, "residual_sweep"),
        Some(CarrierAction::DisclosedNotScrubbed),
        "the sweep must not report a failure it did not have"
    );
}

/// An XMP packet quoting the redacted text in a DIFFERENT CASE is still
/// scrubbed.
///
/// ★ The mirror image of the test above, and the worse half of the same
/// defect. On the metadata path the actor WAS the detector — a byte-exact
/// replace whose return value decided whether anything had been found — so a
/// case-differing quote was not disclosed at all. Nothing was scrubbed and
/// nothing was reported, which is the one outcome that tells a reader there is
/// nothing to look at.
///
/// The packet in this fixture is unreferenced, so the catalog's `xmp` carrier
/// is honestly `Absent` and the sweep's metadata branch is what handles it.
/// That is the branch the assertion is about; asserting on the carrier line
/// would be asserting about a different object.
#[test]
fn a_case_variant_in_an_xmp_packet_is_still_scrubbed() {
    let doc = Document::from_bytes(pdf_with_orphans_drawing(
        "Confidential",
        &["Confidential stream orphan"],
    ))
    .expect("the fixture loads");
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let marked = session
        .mark_redactions_by_search("Confidential", false)
        .expect("mark the region");
    assert!(!marked.is_empty(), "the search found the drawn phrase");
    let report = session.apply_redactions().expect("apply");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("a redacted session saves");

    assert!(
        !contains(&bytes, b"CONFIDENTIAL xmp orphan"),
        "the packet quotes CONFIDENTIAL against a redacted Confidential and \
         must not survive a case difference; notes were {:?}",
        report.notes
    );
    assert!(
        report.residual_sweep_objects_scrubbed >= 1,
        "and the sweep must say it acted; notes were {:?}",
        report.notes
    );
}

// ------------------------------------------------- 4. the counts and controls

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
