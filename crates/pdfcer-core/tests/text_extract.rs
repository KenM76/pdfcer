//! # Pass 4 integration tests — the §9.10 extraction contract, end to end
//!
//! Unit tests in `text_extract::{cmap, layout}` cover the pieces in
//! isolation. These drive the whole pipeline over the synthetic fixtures
//! in `fixtures/synthetic/text/` (provenance: that directory's
//! `PROVENANCE.md`), one fixture per clause, so a failure names the
//! clause it broke.
//!
//! The organising assertion in almost every test below is the pair
//! `plain_text()` / `sourced_text()`. That pair is the executable form
//! of the module's central claim — that pdfcer knows which characters
//! came from the file and which it invented — and a bug that blurs the
//! two shows up here as the two accessors returning the same string when
//! they should differ, or differing when they should not.

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::settings::{ActualTextPrecedence, UnmappableCode};
use pdfcer_core::text_extract::{
    self, ArtifactKind, Editability, ExtractOptions, ExtractedText, LadderRung, TextOrigin,
};

/// A fixture path under `fixtures/synthetic/text/`.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

/// Load and extract with default options.
fn extract(name: &str) -> ExtractedText {
    extract_with(name, &ExtractOptions::default())
}

/// Load and extract with explicit options.
fn extract_with(name: &str, options: &ExtractOptions) -> ExtractedText {
    let doc = Document::load(&fixture(name)).expect("fixture loads");
    text_extract::extract_document(&doc, options).expect("extraction runs")
}

// ---------------------------------------------------------------------------
// Ladder rung 2 — simple font, standard encoding, AGL
// ---------------------------------------------------------------------------

#[test]
fn rung2_resolves_a_winansi_simple_font() {
    let text = extract("simple-winansi.pdf");
    assert_eq!(text.sourced_text(), "HelloworldSecond line");
    let d = &text.diagnostics;
    assert_eq!(d.codes_total, 21);
    assert_eq!(
        d.via_encoding_agl, 21,
        "every code must come from §9.10.2 method 2"
    );
    assert_eq!(d.via_to_unicode, 0);
    assert_eq!(d.ladder_failures, 0);
    assert_eq!(d.via_glyph_name_extension, 0, "the precondition HOLDS here");
    assert_eq!(d.sourced_fraction(), Some(1.0));
}

#[test]
fn a_tj_offset_with_no_space_glyph_derives_a_space() {
    // S3/S4: the gap between "Hello" and "world" is a TJ offset and
    // nothing else — there is no space character anywhere in the file.
    // The derived space must appear in plain text, must NOT appear in
    // sourced text, and must be counted.
    let text = extract("simple-winansi.pdf");
    assert!(
        text.plain_text().contains("Hello world"),
        "plain text: {:?}",
        text.plain_text()
    );
    assert!(
        !text.sourced_text().contains("Hello world"),
        "the space is pdfcer's, not the file's"
    );
    assert_eq!(text.diagnostics.spaces_derived, 1);
}

#[test]
fn a_new_td_line_derives_a_line_break() {
    // S5: no line markers exist in a content stream.
    let text = extract("simple-winansi.pdf");
    assert!(text.plain_text().contains("world\nSecond"));
    assert!(text.sourced_text().contains("worldSecond"));
    assert_eq!(text.diagnostics.lines_derived, 1);
}

#[test]
fn an_untagged_document_says_so_by_name() {
    let text = extract("simple-winansi.pdf");
    assert!(!text.diagnostics.tagged);
    assert!(
        text.diagnostics
            .notes
            .iter()
            .any(|n| n.contains("untagged document")),
        "notes: {:?}",
        text.diagnostics.notes
    );
}

// ---------------------------------------------------------------------------
// Ladder rung 1 — /ToUnicode, all three §9.10.3 forms
// ---------------------------------------------------------------------------

#[test]
fn rung1_resolves_identity_h_through_to_unicode() {
    let text = extract("identity-h-tounicode.pdf");
    let sourced = text.sourced_text();
    // Form B (<0001>..<0003> over <0048>) gives H, I, J.
    assert!(sourced.starts_with("HIJ"), "sourced: {sourced:?}");
    // Form C's middle array element is a THREE code point ligature.
    assert!(sourced.contains("ffl"), "one-to-many mapping lost");
    // Form A's surrogate pair is a supplementary-plane character.
    assert!(
        sourced.contains('\u{2003E}'),
        "surrogate pair truncated — a UCS-2 decoder would do exactly this"
    );
}

#[test]
fn an_uncovered_code_falls_through_the_ladder_and_is_counted() {
    // <0099> is deliberately absent from the fixture's ToUnicode CMap.
    // §9.10.3 N4 records that the standard says nothing about an
    // uncovered code; pdfcer's per-code fallthrough finds nothing else
    // (a composite font has no rung 2), so it must reach the failure
    // clause — U+FFFD, counted, never invented.
    let text = extract("identity-h-tounicode.pdf");
    let d = &text.diagnostics;
    assert_eq!(d.ladder_failures, 1);
    assert!(text.sourced_text().contains('\u{FFFD}'));
    assert_eq!(d.via_to_unicode, d.codes_total - 1);
}

#[test]
fn extraction_succeeds_on_a_file_rendering_correctly_refuses() {
    // §9.7.5.2 forbids Identity-H with a non-embedded font, and
    // pdfcer-render refuses such a font outright. §9.10.2 rung 1 needs
    // only the /ToUnicode entry. The two directions have different
    // requirements and this is the file that proves it.
    let text = extract("identity-h-tounicode.pdf");
    assert!(text.diagnostics.codes_total > 0);
    assert!(text.diagnostics.via_to_unicode > 0);
}

#[test]
fn one_code_may_produce_several_glyphs_worth_of_text() {
    let text = extract("identity-h-tounicode.pdf");
    let ligature = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .flat_map(|r| r.glyphs.iter().map(move |g| (r, g)))
        .find(|(_, g)| g.code == 0x0011)
        .expect("the ligature code was shown");
    let (run, glyph) = ligature;
    assert_eq!(glyph.text_len, 3, "ffl is three bytes, not one");
    let start = glyph.text_start as usize;
    assert_eq!(&run.text[start..start + glyph.text_len as usize], "ffl");
}

// ---------------------------------------------------------------------------
// The Identity-H dead end — Pass 4's headline honesty metric
// ---------------------------------------------------------------------------

#[test]
fn identity_h_without_to_unicode_recovers_nothing_and_says_why() {
    let text = extract("identity-h-no-tounicode.pdf");
    let d = &text.diagnostics;

    assert!(d.codes_total > 0, "codes were shown");
    assert_eq!(
        d.ladder_failures, d.codes_total,
        "§9.10.2 excludes Identity-H from rung 3 by name and an \
         Adobe-Identity-0 descendant satisfies neither half of the second \
         disjunct — EVERY code must fail"
    );
    assert_eq!(d.sourced_codes(), 0);
    assert_eq!(d.sourced_fraction(), Some(0.0));
    assert_eq!(d.identity_fonts_without_to_unicode, 1);

    // Every extracted character is the replacement character, and none
    // of them is a plausible-looking guess reconstructed from glyph
    // indices.
    let sourced = text.sourced_text();
    assert!(!sourced.is_empty());
    assert!(
        sourced
            .chars()
            .all(|c| c == '\u{FFFD}' || c.is_whitespace()),
        "a fabricated character escaped: {sourced:?}"
    );

    assert!(
        d.notes
            .iter()
            .any(|n| n.contains("Identity-H") && n.contains("no Unicode is recoverable")),
        "the dead end must be named, not merely counted: {:?}",
        d.notes
    );
}

#[test]
fn every_failed_glyph_is_marked_as_such_individually() {
    let text = extract("identity-h-no-tounicode.pdf");
    let glyphs: Vec<_> = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .flat_map(|r| r.glyphs.iter())
        .collect();
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|g| g.rung == LadderRung::Failed));
    assert!(glyphs.iter().all(|g| !g.rung.is_sourced()));
}

// ---------------------------------------------------------------------------
// /ActualText — §14.9.4's own example
// ---------------------------------------------------------------------------

#[test]
fn actual_text_replaces_the_glyphs_it_covers() {
    // §14.9.4's EXAMPLE: the glyphs read Dru / k- / ker, the sequence's
    // /ActualText is (c), and the clause's own gloss is that the
    // character content is "Drucker".
    let text = extract("actual-text-drucker.pdf");
    assert_eq!(
        text.sourced_text(),
        "Drucker",
        "the clause's own worked example is the assertion"
    );
    assert_eq!(text.diagnostics.actual_text_applied, 1);
}

#[test]
fn plain_text_keeps_the_derived_line_break_the_sourced_text_omits() {
    // The `'` operator moves to the next line, so pdfcer derives a line
    // break there — honestly, because the baseline really did move. The
    // pair of accessors is what makes both answers available: the
    // standard's "Drucker" is the sourced one.
    let text = extract("actual-text-drucker.pdf");
    assert_eq!(text.plain_text(), "Druc\nker");
    assert_eq!(text.sourced_text(), "Drucker");
    assert_eq!(text.diagnostics.lines_derived, 1);
}

#[test]
fn an_actual_text_run_is_atomic_and_carries_no_glyphs() {
    // §14.9.4 N4: no length relationship exists between replacement and
    // replaced content (2 shown characters become 1 here), so
    // character-level mapping back to glyph positions is impossible —
    // not merely unimplemented. The API has to say so.
    let text = extract("actual-text-drucker.pdf");
    let run = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .find(|r| r.origin == TextOrigin::ActualText)
        .expect("the replacement run exists");
    assert_eq!(run.text, "c");
    assert!(run.glyphs.is_empty());
    assert!(
        run.is_sourced(),
        "the VALUE is sourced; only its offsets are not"
    );
    assert!(
        run.bbox.is_some(),
        "the covered region is the only positional information available"
    );
}

#[test]
fn no_derived_word_space_is_inserted_next_to_a_replacement() {
    // §14.9.4 NOTE 2 makes ActualText a *character* substitution, and
    // requires no word break between consecutive ones. Inserting one
    // here would give "Dru c ker".
    let text = extract("actual-text-drucker.pdf");
    assert!(!text.plain_text().contains(' '));
    assert_eq!(text.diagnostics.spaces_derived, 0);
}

// ---------------------------------------------------------------------------
// TX-A1 / AT-A1 — the two EXTRACT-radius operator settings (R169)
// ---------------------------------------------------------------------------
//
// Both are genuine spec silences, both default to exactly what pdfcer
// extracted before the settings existed, and both change CHARACTER
// OFFSETS — which is why they are tested at the pipeline level rather
// than as unit tests on the ladder. An offset change is what moves a text
// search's hit and a text-based redaction's coverage (R35), and that only
// becomes visible once runs have been assembled.

#[test]
fn the_unmappable_sentinel_defaults_to_the_shipped_one() {
    // R169's non-negotiable: the knob exists, the behaviour does not
    // move. `identity-h-no-tounicode.pdf` is the fixture where every rung
    // fails, so every character is the sentinel.
    let with_default = extract("identity-h-no-tounicode.pdf");
    let explicit = extract_with(
        "identity-h-no-tounicode.pdf",
        &ExtractOptions::default().with_unmappable_code(UnmappableCode::ReplacementChar),
    );
    assert_eq!(with_default.sourced_text(), explicit.sourced_text());
    assert!(
        with_default
            .sourced_text()
            .contains(char::REPLACEMENT_CHARACTER)
    );
}

#[test]
fn the_unmappable_sentinel_changes_the_characters_but_never_the_count() {
    // §9.10.2 names NO sentinel, so all three values are equally
    // conforming. What must NOT change is the honesty metric:
    // `ladder_failures` is documented as "the headline honesty metric",
    // and a setting that could quietly reduce it would be a setting that
    // hides the failure rather than styling it.
    let baseline = extract("identity-h-no-tounicode.pdf");
    let failures = baseline.diagnostics.ladder_failures;
    assert!(failures > 0, "the fixture must actually fail the ladder");
    let baseline_sentinels = baseline
        .sourced_text()
        .chars()
        .filter(|c| *c == char::REPLACEMENT_CHARACTER)
        .count();

    let question = extract_with(
        "identity-h-no-tounicode.pdf",
        &ExtractOptions::default().with_unmappable_code(UnmappableCode::QuestionMark),
    );
    assert_eq!(question.diagnostics.ladder_failures, failures);
    assert!(
        !question
            .sourced_text()
            .contains(char::REPLACEMENT_CHARACTER)
    );
    assert_eq!(
        question
            .sourced_text()
            .chars()
            .filter(|c| *c == '?')
            .count(),
        baseline_sentinels,
        "`question_mark` must be length-preserving, like the default"
    );

    let omitted = extract_with(
        "identity-h-no-tounicode.pdf",
        &ExtractOptions::default().with_unmappable_code(UnmappableCode::Omit),
    );
    assert_eq!(
        omitted.diagnostics.ladder_failures, failures,
        "omitting the CHARACTERS must not omit the FAILURE"
    );
    assert!(!omitted.sourced_text().contains(char::REPLACEMENT_CHARACTER));
    assert!(!omitted.sourced_text().contains('?'));
    assert!(
        omitted.sourced_text().chars().count() < baseline.sourced_text().chars().count(),
        "`omit` must actually shorten the text"
    );

    // AND THE THING TO KNOW ABOUT `omit`, pinned here because it is
    // surprising and is documented on the setting itself: on this fixture
    // EVERY code fails the ladder, so every run's text is empty, and
    // `layout::Builder::close_run` drops a character-less run — glyph
    // records included. So `omit` does not merely shorten the text; where
    // a whole run is unmappable it removes the run. The failure is still
    // fully counted (asserted above), which is what keeps this honest
    // rather than silent, but a caller that needs per-glyph positions for
    // unmappable codes must not choose `omit`.
    let glyphs = omitted
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .flat_map(|r| r.glyphs.iter())
        .count();
    assert_eq!(
        glyphs, 0,
        "an all-unmappable page under `omit` yields no runs at all — if this starts failing, `close_run` changed and the setting's docs need updating with it"
    );
}

#[test]
fn actual_text_precedence_defaults_to_replacement() {
    // §14.9.4's `shall` is the only one in the set, so `always` is the
    // default — and the clause's own worked example is the assertion.
    let explicit = extract_with(
        "actual-text-drucker.pdf",
        &ExtractOptions::default().with_actual_text(ActualTextPrecedence::Always),
    );
    assert_eq!(explicit.sourced_text(), "Drucker");
    assert_eq!(
        explicit.sourced_text(),
        extract("actual-text-drucker.pdf").sourced_text(),
        "the default must be `always`"
    );
}

#[test]
fn glyphs_precedence_shows_the_page_and_still_counts_the_entry() {
    // The forensic setting: what is extracted is what the page draws. The
    // §14.9.4 example's glyphs read "Dru" / "k-" / "ker" with the middle
    // pair covered by the replacement, so turning substitution off must
    // bring the covered glyphs back.
    let text = extract_with(
        "actual-text-drucker.pdf",
        &ExtractOptions::default().with_actual_text(ActualTextPrecedence::Glyphs),
    );
    assert!(
        !text.sourced_text().contains("Drucker"),
        "the replacement must not have been applied: {:?}",
        text.sourced_text()
    );
    assert!(
        text.pages
            .iter()
            .flat_map(|p| p.runs.iter())
            .all(|r| r.origin != TextOrigin::ActualText),
        "no replacement run may be emitted"
    );
    assert_eq!(
        text.diagnostics.actual_text_applied, 1,
        "the COUNT is a property of the document, not of the setting — an \
         operator who turned substitution off still needs to see the entry \
         is there"
    );
}

#[test]
fn tagged_only_precedence_declines_an_untagged_span() {
    // "Inside tagged content" is tested as an /MCID in scope, because
    // §14.7.4.2 makes /MCID the join key between a marked-content
    // sequence and a structure element. This fixture's /Span carries
    // /ActualText and no /MCID — it is not part of any structure tree —
    // so `tagged_only` behaves as `glyphs` here, and would behave as
    // `always` on a genuinely tagged file.
    let tagged_only = extract_with(
        "actual-text-drucker.pdf",
        &ExtractOptions::default().with_actual_text(ActualTextPrecedence::TaggedOnly),
    );
    let glyphs = extract_with(
        "actual-text-drucker.pdf",
        &ExtractOptions::default().with_actual_text(ActualTextPrecedence::Glyphs),
    );
    assert_eq!(tagged_only.sourced_text(), glyphs.sourced_text());
    assert_eq!(tagged_only.diagnostics.actual_text_applied, 1);
}

// ---------------------------------------------------------------------------
// Artifacts and ReversedChars — §14.8
// ---------------------------------------------------------------------------

#[test]
fn artifacts_are_classified_kept_and_excluded_by_policy() {
    let text = extract("artifact-and-reversed.pdf");
    let d = &text.diagnostics;
    assert_eq!(
        d.artifact_sequences, 2,
        "one with a property list, one bare"
    );
    assert!(d.artifact_chars > 0);

    // Excluded from plain text by DEFAULT policy...
    let plain = text.plain_text();
    assert!(!plain.contains("Running head"));
    assert!(plain.contains("Real content"));

    // ...but always present in the run list, because §14.8.2.2's A1
    // records that no `shall` requires a reader to exclude them.
    let artifact_runs: Vec<_> = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter(|r| r.artifact.is_some())
        .collect();
    assert!(
        artifact_runs
            .iter()
            .any(|r| r.text.contains("Running head"))
    );
    assert!(
        artifact_runs
            .iter()
            .any(|r| r.artifact == Some(ArtifactKind::Pagination)),
        "Table 330's /Type must be read"
    );
    assert!(
        artifact_runs
            .iter()
            .any(|r| r.artifact == Some(ArtifactKind::Unspecified)),
        "the bare /Artifact BMC form is a generic artifact, not an error"
    );
}

#[test]
fn including_artifacts_is_a_caller_decision() {
    let options = ExtractOptions::default().with_artifacts(true);
    let text = extract_with("artifact-and-reversed.pdf", &options);
    assert!(text.plain_text().contains("Running head"));
    assert!(text.includes_artifacts());
}

#[test]
fn reversed_chars_reverses_within_each_string_not_across_them() {
    // §14.8.2.3.3's own example: "( olleH ) Tj -200 0 Td ( . dlrow ) Tj"
    // "represents the text Hello world .". Reversing the SEQUENCE
    // instead of each STRING is the classic bug and produces the words
    // in the wrong order.
    let text = extract("artifact-and-reversed.pdf");
    let sourced = text.sourced_text();
    assert!(
        sourced.contains("Hello") && sourced.contains("world ."),
        "sourced: {sourced:?}"
    );
    let hello = sourced.find("Hello").expect("Hello present");
    let world = sourced.find("world").expect("world present");
    assert!(
        hello < world,
        "the strings themselves stay in reading order"
    );
    assert_eq!(text.diagnostics.reversed_chars_sequences, 1);
}

// ---------------------------------------------------------------------------
// Document-level facts — §14.8.1, §14.8.2.3.1, §14.7
// ---------------------------------------------------------------------------

#[test]
fn tagged_suspects_and_struct_tree_are_all_reported() {
    let text = extract("tagged-marked.pdf");
    let d = &text.diagnostics;
    assert!(d.tagged);
    assert!(d.suspects);
    assert!(d.struct_tree_present);
    assert_eq!(d.tag_suspect_sequences, 1);

    for expected in ["Suspects", "StructTreeRoot", "TagSuspect"] {
        assert!(
            d.notes.iter().any(|n| n.contains(expected)),
            "missing a named diagnostic for {expected}: {:?}",
            d.notes
        );
    }
    // A tagged document must NOT get the untagged warning.
    assert!(!d.notes.iter().any(|n| n.contains("untagged document")));
}

#[test]
fn mcid_is_recorded_for_a_later_structure_pass() {
    let text = extract("tagged-marked.pdf");
    let run = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .find(|r| r.text.contains("Inside MCID"))
        .expect("the MCID-tagged run exists");
    assert_eq!(run.mcid, Some(0));
}

// ---------------------------------------------------------------------------
// API-shape and cross-cutting behaviour
// ---------------------------------------------------------------------------

#[test]
fn every_glyph_text_range_indexes_its_own_run() {
    // A range that is out of bounds, or that does not fall on a char
    // boundary, would panic a caller doing the obvious slice — so this
    // is the invariant the whole per-glyph provenance model rests on.
    for name in [
        "simple-winansi.pdf",
        "identity-h-tounicode.pdf",
        "actual-text-drucker.pdf",
        "artifact-and-reversed.pdf",
        "tagged-marked.pdf",
    ] {
        let text = extract(name);
        for page in &text.pages {
            for run in &page.runs {
                let mut expected_start = 0usize;
                for g in &run.glyphs {
                    let start = g.text_start as usize;
                    let end = start + g.text_len as usize;
                    assert!(end <= run.text.len(), "{name}: range past the run");
                    assert!(run.text.is_char_boundary(start), "{name}: split a char");
                    assert!(run.text.is_char_boundary(end), "{name}: split a char");
                    assert_eq!(start, expected_start, "{name}: glyph ranges must tile");
                    expected_start = end;
                }
                if !run.glyphs.is_empty() {
                    assert_eq!(
                        expected_start,
                        run.text.len(),
                        "{name}: a glyph run's text must be fully covered by its glyphs"
                    );
                }
            }
        }
    }
}

#[test]
fn derived_runs_never_carry_glyphs_and_sourced_runs_always_do() {
    for name in ["simple-winansi.pdf", "actual-text-drucker.pdf"] {
        let text = extract(name);
        for page in &text.pages {
            for run in &page.runs {
                match run.origin {
                    TextOrigin::Glyphs => assert!(!run.glyphs.is_empty(), "{name}"),
                    _ => assert!(run.glyphs.is_empty(), "{name}: {:?}", run.origin),
                }
            }
        }
    }
}

#[test]
fn sourced_text_is_a_subsequence_of_plain_text() {
    // The two accessors differ only by insertions. If sourced_text ever
    // contained a character plain_text does not, pdfcer would be dropping
    // file content on the "friendly" path.
    for name in [
        "simple-winansi.pdf",
        "identity-h-tounicode.pdf",
        "identity-h-no-tounicode.pdf",
        "actual-text-drucker.pdf",
        "artifact-and-reversed.pdf",
        "tagged-marked.pdf",
    ] {
        let text = extract(name);
        let plain = text.plain_text();
        let sourced = text.sourced_text();
        let mut haystack = plain.chars();
        for ch in sourced.chars() {
            assert!(
                haystack.any(|h| h == ch),
                "{name}: sourced text is not a subsequence of plain text"
            );
        }
    }
}

#[test]
fn display_matches_plain_text() {
    let text = extract("simple-winansi.pdf");
    assert_eq!(text.to_string(), text.plain_text());
}

#[test]
fn page_and_document_extraction_agree() {
    use pdfcer_core::page_tree;

    let doc = Document::load(&fixture("simple-winansi.pdf")).expect("loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let options = ExtractOptions::default();
    let one = text_extract::extract_page(&doc, &pages[0], 0, &options).expect("page");
    let all = text_extract::extract_document(&doc, &options).expect("document");
    assert_eq!(one.plain_text(), all.pages[0].plain_text());
    assert_eq!(one.diagnostics.codes_total, all.diagnostics.codes_total);
}

#[test]
fn a_page_index_past_the_end_is_a_named_refusal() {
    let doc = Document::load(&fixture("simple-winansi.pdf")).expect("loads");
    let err = text_extract::extract_pages(&doc, &[7], &ExtractOptions::default())
        .expect_err("index 7 does not exist");
    assert!(matches!(
        err,
        text_extract::ExtractError::NoSuchPage { index: 7, count: 1 }
    ));
}

#[test]
fn extraction_leaves_the_document_bytes_untouched() {
    // Extraction is READ-ONLY. This is not a hypothetical: the walk
    // resolves objects, decodes streams and builds caches, and a future
    // change that memoised something into the document would break the
    // round-trip invariant everywhere at once.
    let doc = Document::load(&fixture("identity-h-tounicode.pdf")).expect("loads");
    let before = doc.bytes().to_vec();
    let _ = text_extract::extract_document(&doc, &ExtractOptions::default()).expect("extracts");
    assert_eq!(doc.bytes(), before.as_slice());
}

// ---------------------------------------------------------------------------
// Pass 19.0 — the composite/CID flag published on provenance
// ---------------------------------------------------------------------------

/// §9.3.3 makes `Tw` void for multi-byte codes, and the Pass 14.1 surgery
/// refuses composite re-encoding (R-INV-4). Both need to know whether a run
/// is composite **before** acting; before Pass 19.0 the only way to find
/// out from outside the crate was to attempt an edit and read the refusal.
///
/// The two fixtures below straddle the boundary: `identity-h-tounicode.pdf`
/// is a Type 0 / `Identity-H` composite (2-byte codes), `simple-winansi.pdf`
/// is a Type 1 simple font (1-byte codes).
#[test]
fn provenance_publishes_whether_a_run_is_composite() {
    let opts = ExtractOptions::default().with_provenance(true);

    let composite = extract_with("identity-h-tounicode.pdf", &opts);
    let mut composite_glyphs = 0usize;
    for page in &composite.pages {
        for run in &page.runs {
            for g in &run.glyphs {
                let p = g.provenance.as_ref().expect("provenance captured");
                assert!(
                    p.composite,
                    "an Identity-H run must report itself composite (§9.7.6.2)"
                );
                composite_glyphs += 1;
            }
        }
    }
    assert!(composite_glyphs > 0, "the fixture produced no glyphs");

    let simple = extract_with("simple-winansi.pdf", &opts);
    let mut simple_glyphs = 0usize;
    for page in &simple.pages {
        for run in &page.runs {
            for g in &run.glyphs {
                let p = g.provenance.as_ref().expect("provenance captured");
                assert!(
                    !p.composite,
                    "a Type 1 WinAnsi run is a simple font (§9.6.1)"
                );
                simple_glyphs += 1;
            }
        }
    }
    assert!(simple_glyphs > 0, "the fixture produced no glyphs");
}

/// Provenance capture stays opt-in: with the flag off the ambient state is
/// never built, so the default Pass 4 output is unchanged and no per-glyph
/// cost is paid by a caller who only wants text.
#[test]
fn the_ambient_state_is_not_built_unless_provenance_is_requested() {
    let text = extract("simple-winansi.pdf");
    for page in &text.pages {
        for run in &page.runs {
            for g in &run.glyphs {
                assert!(g.provenance.is_none());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 118.0 — `TextRun::is_editable`, the extract/edit boundary, published
// ---------------------------------------------------------------------------

/// A one-page PDF with text in BOTH buffers: one `Tj` in the page's own
/// `/Contents`, and one inside a form XObject the page invokes with `Do`.
///
/// Built inline rather than as a checked-in fixture — it exists to prove one
/// asymmetry and nothing else reads it (project rule 7 / `LEGAL.md` §5 keep
/// the corpus to what is needed).
fn page_and_form_text_pdf() -> Vec<u8> {
    let page_content = "BT /F1 12 Tf 50 700 Td (PAGE) Tj ET\nq 1 0 0 1 20 20 cm /X1 Do Q";
    let form_content = "BT /F1 12 Tf 10 10 Td (FORM) Tj ET";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{page_content}\nendstream",
            page_content.len() + 1
        ),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] \
             /Resources << /Font << /F1 6 0 R >> >> /Length {} >>\nstream\n{form_content}\nendstream",
            form_content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
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

/// ★ THE ASYMMETRY, PUBLISHED. Extraction recurses into a form XObject; the
/// edit surgery does not. So a caret can land anywhere extraction can see and
/// commit only where the surgery can reach, and until `Pass 118.0` the only
/// way a shell could tell those regions apart was by matching on
/// `ContentStreamRef` — encoding a fact about pdfcer's internals into its own
/// guard, which then outlives the limitation it was written for.
///
/// On a CAD-exported sheet this is not an edge case: the page stream holds the
/// producer's watermark and the form holds every label the operator wants.
#[test]
fn is_editable_separates_page_text_from_form_xobject_text() {
    let doc = Document::from_bytes(page_and_form_text_pdf()).expect("fixture loads");
    // Provenance ON — without it the answer is `Unknown` for everything, which
    // is the point of the sibling test below.
    let options = ExtractOptions::default().with_provenance(true);
    let text = text_extract::extract_document(&doc, &options).expect("extraction runs");
    let page = &text.pages[0];

    // Both are EXTRACTED — that is the half that already worked, and the half
    // that makes the caret land in the wrong place.
    let all: String = page.runs.iter().map(|r| r.text.as_str()).collect();
    assert!(all.contains("PAGE"), "page text must extract: {all:?}");
    assert!(all.contains("FORM"), "form text must extract too: {all:?}");

    let page_run = page
        .runs
        .iter()
        .find(|r| r.text.contains("PAGE"))
        .expect("the page-stream run");
    let form_run = page
        .runs
        .iter()
        .find(|r| r.text.contains("FORM"))
        .expect("the form-XObject run");

    assert_eq!(
        page_run.editability(),
        Editability::Editable,
        "text in the page's own /Contents is what the surgery reaches"
    );
    assert_eq!(
        form_run.editability(),
        Editability::Editable,
        "PASS 119.0: text inside a form XObject is reachable NOW, and the predicate must say so -- a shell whose caret guard reads this is the whole reason it exists, got {:?}",
        form_run.editability()
    );
}

/// ★ THE PROMISE `Pass 118.0` MADE, KEPT BY `Pass 119.0`.
///
/// That Pass published `editability()` instead of letting the shell match on
/// `GlyphProvenance::content_stream` itself, with an explicit reason: *"when
/// the capability grows, this starts answering `Editable` and every caller
/// improves without changing."* This test is that sentence, executable.
///
/// It is written as a **sweep over every run** rather than as a second look at
/// the one form run above, because the claim is about the predicate's whole
/// range: after 119.0 there is no run in this fixture that reports as
/// out-of-reach, and a regression that re-introduced the old answer for some
/// second-order case (a nested form, a form reached twice) would slip past a
/// single-run assertion.
#[test]
fn no_run_reports_as_out_of_reach_now_that_forms_are_editable() {
    let doc = Document::from_bytes(page_and_form_text_pdf()).expect("fixture loads");
    let options = ExtractOptions::default().with_provenance(true);
    let text = text_extract::extract_document(&doc, &options).expect("extraction runs");
    for run in &text.pages[0].runs {
        if run.glyphs.is_empty() {
            continue; // `NoAnchor`, a different question -- see its own test
        }
        assert_eq!(
            run.editability(),
            Editability::Editable,
            "every sourced run in this fixture is editable after Pass 119.0: {:?}",
            run.text
        );
    }
}

/// ★ THE TRAP THE `-> bool` WOULD HAVE WALKED INTO.
///
/// [`ExtractOptions::capture_provenance`] defaults to **false**, so on a
/// default extraction no glyph carries provenance at all. A boolean predicate
/// would answer "not editable" for EVERY run in the document — including
/// perfectly editable page text — while meaning *"I was not told"*, and a
/// shell trusting it would refuse every caret for a reason nobody measured.
///
/// **This is the test that made the API an enum instead of the `bool` that was
/// asked for.** It is written from the DEFAULT options deliberately, because
/// the default is the state a caller reaches without doing anything.
#[test]
fn without_provenance_the_answer_is_unknown_and_not_a_silent_no() {
    let doc = Document::from_bytes(page_and_form_text_pdf()).expect("fixture loads");
    // Default options — provenance OFF.
    let text =
        text_extract::extract_document(&doc, &ExtractOptions::default()).expect("extraction runs");
    let page_run = text.pages[0]
        .runs
        .iter()
        .find(|r| r.text.contains("PAGE"))
        .expect("the page-stream run");
    assert_eq!(
        page_run.editability(),
        Editability::Unknown,
        "editable page text must report UNKNOWN when provenance was not captured -- never a measured-looking no"
    );
}

/// The predicate must agree with the provenance it is derived from — two
/// answers to one question that could otherwise drift.
#[test]
fn is_editable_agrees_with_the_provenance_it_summarises() {
    use pdfcer_core::text_extract::ContentStreamRef;

    let doc = Document::from_bytes(page_and_form_text_pdf()).expect("fixture loads");
    let options = ExtractOptions::default().with_provenance(true);
    let text = text_extract::extract_document(&doc, &options).expect("extraction runs");

    for run in &text.pages[0].runs {
        // `Pass 119.0` widened the agreement: BOTH stream kinds are editable
        // now, so the property under test is "provenance was captured at all",
        // not "the provenance says Page". Note what changed and what did not
        // -- the predicate still may not answer `Editable` for a run whose
        // provenance is absent, which is the `Unknown`-is-not-a-no contract.
        let all_sourced = !run.glyphs.is_empty()
            && run.glyphs.iter().all(|g| {
                g.provenance.as_ref().is_some_and(|p| {
                    matches!(
                        p.content_stream,
                        ContentStreamRef::Page | ContentStreamRef::Form { .. }
                    )
                })
            });
        assert_eq!(
            run.editability() == Editability::Editable,
            all_sourced,
            "the predicate and the provenance disagree about {:?}",
            run.text
        );
    }
}

/// A run with no glyphs answers `false`, not the vacuous `true` an empty
/// `all()` would give. Offering a caret over text that cannot be committed is
/// precisely the failure this predicate exists to prevent, so the optimistic
/// default is the wrong one.
#[test]
fn a_run_with_no_glyphs_is_not_editable() {
    let text = extract_with(
        "actual-text-drucker.pdf",
        &ExtractOptions::default().with_provenance(true),
    );
    let derived: Vec<_> = text
        .pages
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter(|r| r.glyphs.is_empty())
        .collect();
    assert!(
        !derived.is_empty(),
        "this fixture is expected to carry at least one glyph-less run"
    );
    for run in derived {
        assert_eq!(
            run.editability(),
            Editability::NoAnchor,
            "a run with no show operator has its OWN reason -- it is not out of reach, it has nothing to reach for: {:?}",
            run.text
        );
    }
}

// ---------------------------------------------------------------------------
// `Pass 127.0` — the Type 3 dead end, and its control
//
// Every test below drives ONE fixture, `fixtures/synthetic/type3/
// tounicode_gate.pdf`, which carries three Type 3 fonts on one page: `/TA`
// with a `/ToUnicode` CMap, `/TB` and `/TC` with none. The fixture is
// deliberately shaped so that no single assertion here can pass for the
// wrong reason — see its generator docstring in
// `tools/gen-type3-fixtures.py`.
// ---------------------------------------------------------------------------

/// A fixture path under `fixtures/synthetic/type3/`.
///
/// A second helper rather than a parameter on [`fixture`]: the Type 3
/// fixtures are generated by their own script, documented by their own
/// `PROVENANCE.md`, and regenerated independently, and a shared path
/// helper would quietly couple two corpora that have nothing to do with
/// each other.
fn type3_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/type3")
        .join(name)
}

fn extract_type3(name: &str) -> ExtractedText {
    let doc = Document::load(&type3_fixture(name)).expect("fixture loads");
    text_extract::extract_document(&doc, &ExtractOptions::default()).expect("extraction runs")
}

/// The CONTROL. A Type 3 font carrying `/ToUnicode` extracts like any
/// other simple font, through §9.10.2 rung 1.
///
/// Without this, every other assertion in this section would also pass for
/// a reader that extracts **nothing at all** from a Type 3 font — which is
/// the failure the whole `Pass 127.0` scope exists to distinguish from the
/// legitimate dead end below.
#[test]
fn type3_with_to_unicode_extracts_as_sourced_text() {
    let text = extract_type3("tounicode_gate.pdf");

    assert!(
        text.plain_text().contains("HI!"),
        "the /TA run maps 65,66,67 -> H,I,! through its own /ToUnicode CMap: {:?}",
        text.plain_text()
    );
    assert!(
        text.sourced_text().contains("HI!"),
        "and it is SOURCED, not derived -- rung 1 is the file's own statement"
    );
    assert_eq!(
        text.diagnostics.via_to_unicode, 3,
        "exactly the three /TA codes climbed rung 1"
    );
}

/// The DEAD END, counted once per font — and the count is **2**, which is
/// the assertion that carries this test.
///
/// ISO 32000-1 Table 112 gives a Type 3 font **no `/BaseFont` entry**, so a
/// conformant one has no name. Before `Pass 127.0` the per-font diagnostic
/// de-duplicated on that name, and every unnamed font on a page shared one
/// slot.
///
/// ★ **Measured on the pre-fix code, this fixture reported `0`, not `1`.**
/// `/TA` is resolved first, is also unnamed, and has a `/ToUnicode` — so it
/// claims the empty key while emitting no note, and `/TB` and `/TC` are
/// skipped before their notes are read. One clean unnamed font silences
/// every unnamed font behind it. The exact `2` is what makes this test a
/// claim about the de-duplication key; a `> 0` assertion would have passed
/// on the pre-fix code too, and proved nothing.
#[test]
fn two_type3_fonts_without_to_unicode_are_counted_as_two() {
    let d = extract_type3("tounicode_gate.pdf").diagnostics;

    assert_eq!(
        d.type3_fonts_without_to_unicode, 2,
        "/TB and /TC are DISTINCT font objects with no /ToUnicode between them; \
         anything less means the diagnostic de-duplicated on a name Table 112 \
         does not give a Type 3 font (the pre-fix code measured 0 here)"
    );
    assert_eq!(
        d.identity_fonts_without_to_unicode, 0,
        "the Type 3 dead end must not be filed under the composite one -- they \
         are different clauses with different remedies"
    );
}

/// The dead end is **named**, not merely counted, and named per font by a
/// handle the operator can act on.
///
/// A counter says "two fonts are unreadable"; it does not say **which**.
/// With no `/BaseFont` to quote (Table 112), the only handle that exists is
/// the resource key the content stream selected the font with — `/TB`,
/// `/TC` — which is what `Tf` writes and what a hex editor finds. The old
/// `<unnamed>` placeholder was accurate and unusable, and became actively
/// misleading the moment two such fonts could be reported on one page.
#[test]
fn the_type3_dead_end_names_each_font_it_counts() {
    let d = extract_type3("tounicode_gate.pdf").diagnostics;

    for want in ["/TB", "/TC"] {
        assert!(
            d.notes
                .iter()
                .any(|n| n.contains(want) && n.contains("Type 3") && n.contains("NO /ToUnicode")),
            "font {want}'s dead end must be named: {:?}",
            d.notes
        );
    }
    assert!(
        !d.notes.iter().any(|n| n.contains("<unnamed>")),
        "a Type 3 font has no /BaseFont; reporting it as <unnamed> tells the \
         operator nothing they can use: {:?}",
        d.notes
    );
}

/// pdfcer does not FABRICATE Unicode for a Type 3 glyph whose name it cannot
/// source.
///
/// The `/TB` and `/TC` glyphs are named `/gb1` and `/gc1` — arbitrary
/// `/CharProcs` keys, which is all a Type 3 glyph name ever is. They reach
/// §9.10.2's failure clause and become U+FFFD. A reader that emitted `g`,
/// `gb1`, or a character guessed from the glyph's SHAPE would be inventing
/// document content, which is the one thing extraction must never do.
#[test]
fn type3_without_to_unicode_fails_the_ladder_rather_than_guessing() {
    let text = extract_type3("tounicode_gate.pdf");
    let d = &text.diagnostics;

    assert_eq!(
        d.ladder_failures, 2,
        "one code shown in /TB and one in /TC, both unreachable"
    );
    assert!(
        text.plain_text().contains('\u{FFFD}'),
        "the unmappable codes are present and marked, not silently dropped --          a dropped code would make the page text a LIE about its own length"
    );
    assert!(
        !text.plain_text().contains("gb1") && !text.plain_text().contains("gc1"),
        "a glyph NAME is not text; emitting one would be fabrication: {:?}",
        text.plain_text()
    );
}

/// A text SEARCH over the same document reports what it could not read.
///
/// This is the operator-facing half, and the reason the core-level counters
/// exist at all. `matches = 0` because a needle is absent and `matches = 0`
/// because the document's text was never recoverable are the same output;
/// only the diagnostics tell them apart, and
/// [`EditSession::search_text`](pdfcer_core::edit::EditSession::search_text)
/// is the verb that hands both back together.
#[test]
fn a_search_reports_the_text_it_could_not_read() {
    use pdfcer_core::edit::{EditSession, TextSearchOptions};

    let doc = Document::load(&type3_fixture("tounicode_gate.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);

    let hit = session.search_text("HI!", &TextSearchOptions::default());
    assert!(
        !hit.matches.is_empty(),
        "the /ToUnicode-carrying run is searchable"
    );
    assert_eq!(
        hit.diagnostics.type3_fonts_without_to_unicode, 2,
        "and the two that are NOT searchable are reported alongside the hit --          a successful search still owes the operator the rest of the document"
    );

    let miss = session.search_text("zzz", &TextSearchOptions::default());
    assert!(miss.matches.is_empty());
    assert_eq!(
        miss.diagnostics.type3_fonts_without_to_unicode, 2,
        "the zero-hit case is the one that MATTERS: without this the operator \
         would read 'not found' as 'not present'"
    );

    // The convenience verb still returns exactly the hits, unchanged.
    assert_eq!(
        session.find_text("HI!", false).len(),
        hit.matches.len(),
        "search_text and find_text must not be able to disagree about hits"
    );
}

/// `Pass 127.1` — a search-driven REDACTION reports what it could not read.
///
/// # Why this is a separate test from the search one
///
/// Because the consequence is different in kind, and a shared test would let
/// one path regress while the other stayed green. A search that misses is a
/// question answered badly; a redaction that misses is a document shipped
/// with the thing in it that the operator asked to have removed, after being
/// told the run succeeded.
///
/// ★ The fixture's `/TA` run IS readable and `/TB`/`/TC` are not, so this
/// exercises the case that actually ships: a **partial** result. That is the
/// more dangerous one, not the safer one — "2 marks authored" reads as
/// success, and nothing in the count hints that a third occurrence sat in a
/// font the scan could not read.
#[test]
fn a_search_driven_redaction_reports_the_text_it_could_not_read() {
    use pdfcer_core::edit::{EditSession, TextSearchOptions};

    let doc = Document::load(&type3_fixture("tounicode_gate.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);

    let marked = session
        .search_and_mark_redactions("HI!", &TextSearchOptions::default())
        .expect("marking runs");

    assert!(
        !marked.created.is_empty(),
        "the readable run must still be marked -- the disclosure is additional, not a refusal"
    );
    assert_eq!(
        marked.diagnostics.type3_fonts_without_to_unicode, 2,
        "the two fonts that could NOT be searched are reported alongside a SUCCESSFUL \
         marking run; a disclosure that only fired on zero marks would be silent in \
         exactly the mixed case real documents produce"
    );
    assert!(marked.diagnostics.ladder_failures > 0);

    // And the legacy verb still returns exactly the ids, unchanged.
    let doc2 = Document::load(&type3_fixture("tounicode_gate.pdf")).expect("fixture loads");
    let mut session2 = EditSession::new(doc2);
    let ids = session2
        .mark_redactions_by_search("HI!", false)
        .expect("marking runs");
    assert_eq!(
        ids.len(),
        marked.created.len(),
        "the reporting verb and the plain verb must not be able to disagree about what \
         they marked"
    );
}

/// An empty query marks nothing and reports nothing, without extracting.
///
/// The early return exists so a shell that clears its search box does not pay
/// for a whole-document extraction; asserting it keeps that from being
/// quietly lost to a refactor that "simplifies" the guard away.
#[test]
fn an_empty_redaction_query_is_a_no_op() {
    use pdfcer_core::edit::{EditSession, TextSearchOptions};

    let doc = Document::load(&type3_fixture("tounicode_gate.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let marked = session
        .search_and_mark_redactions("", &TextSearchOptions::default())
        .expect("an empty query is not an error");
    assert!(marked.created.is_empty());
    assert_eq!(
        marked.diagnostics.codes_total, 0,
        "no extraction should have run at all"
    );
}
