//! Tests for `pdfcer_text::text_extract::cmap`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_text::text_extract::cmap::*;

/// Build a CMap from `beginbfchar` entries, for the R110 inverse tests.
fn bfchar_cmap(pairs: &[(u16, &str)]) -> ToUnicodeCMap {
    let mut body =
        String::from("begincmap\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    body.push_str(&format!("{} beginbfchar\n", pairs.len()));
    for (code, dst) in pairs {
        let hex: String = dst.encode_utf16().map(|u| format!("{u:04X}")).collect();
        body.push_str(&format!("<{code:04X}> <{hex}>\n"));
    }
    body.push_str("endbfchar\nendcmap\n");
    ToUnicodeCMap::parse(body.as_bytes())
}

/// The ordinary case R110 exists to allow: a one-to-one CMap inverts.
#[test]
fn a_one_to_one_cmap_inverts() {
    let hiragana_a = '\u{3042}';
    let cmap = bfchar_cmap(&[(1, "A"), (2, "B"), (3, "\u{3042}")]);
    let inv = cmap.injective_inverse().expect("this CMap is injective");
    assert_eq!(inv.get(&'A'), Some(&1));
    assert_eq!(inv.get(&'B'), Some(&2));
    assert_eq!(inv.get(&hiragana_a), Some(&3), "non-Latin must invert too");
    assert_eq!(inv.len(), 3);
}

/// Two codes, one character: the inverse is a relation, not a function.
///
/// pdfcer would have to CHOOSE which code to write back, and either
/// choice silently changes which glyph appears. Both codes are named in
/// the error, because knowing only the character leaves the operator
/// unable to find the problem in the font.
#[test]
fn two_codes_mapping_to_one_character_is_refused_with_both_codes_named() {
    let cmap = bfchar_cmap(&[(1, "A"), (7, "A")]);
    let err = cmap.injective_inverse().unwrap_err();
    match err {
        NotInjective::Collision { ch, first, second } => {
            assert_eq!(ch, 'A');
            assert_eq!((first, second), (1, 7));
        }
        other => panic!("expected Collision, got {other:?}"),
    }
}

/// A CODE COVERED BY BOTH A `bfchar` AND A `bfrange` IS ONE CODE, NOT
/// A COLLISION (`Pass 121.0`).
///
/// The materialising loop used to push the singles and then push
/// `lookup(code)` for every code of every range — but `lookup` consults
/// the singles FIRST, so a code present in both tiers was pushed twice
/// with identical text. The injectivity check then reported a collision
/// **of a code with itself**:
///
/// ```text
/// codes 361 and 361 both map to 'Ʃ'
/// ```
///
/// A nonsense sentence and a **false refusal** — the map is perfectly
/// invertible. It fired on the operator's own benchmark CAD drawing, where
/// it read as "pdfcer cannot edit this text" for a reason that did not
/// exist. Note the shape: the message was *visibly* absurd (the same
/// number twice) and had been shipping regardless, because nothing reads a
/// refusal message it never expects to see.
#[test]
fn a_code_in_both_a_bfchar_and_a_bfrange_is_not_a_collision_with_itself() {
    let body = concat!(
        "begincmap
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
",
        "1 beginbfchar
<0005> <0041>
endbfchar
",
        "1 beginbfrange
<0005> <0007> <0041>
endbfrange
",
        "endcmap
"
    );
    let cmap = ToUnicodeCMap::parse(body.as_bytes());
    let inverse = cmap
        .injective_inverse()
        .expect("one code in two tiers is one code, not two");
    // And the answer agrees with `lookup`'s own precedence: the single
    // wins, so 'A' inverts to 5 rather than to a range-derived code.
    assert_eq!(inverse.get(&'A'), Some(&5));
}

/// Two OVERLAPPING RANGES produced the same false collision by the same
/// route — `lookup` resolves an overlap last-wins and returned one
/// range's answer for both iterations. Pinned separately because the two
/// share a fix but not a trigger, and a fix verified on one of two
/// triggers is a fix verified on one of two triggers.
#[test]
fn overlapping_bfranges_are_not_a_collision_with_themselves() {
    let body = concat!(
        "begincmap
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
",
        "2 beginbfrange
<0010> <0012> <0041>
<0011> <0013> <0042>
endbfrange
",
        "endcmap
"
    );
    let cmap = ToUnicodeCMap::parse(body.as_bytes());
    // Asserted as a POSITIVE, not as "no self-collision". The first
    // draft of this test was `if let Err(Collision) { assert_ne!(first,
    // second) }` — which passes vacuously whenever the call succeeds AND
    // whenever it fails for any other reason, and it duly passed against
    // a deliberately re-broken build. A conditional assertion about an
    // error that may not occur tests nothing.
    //
    // The map's own answer is determinate: `lookup` resolves overlaps
    // last-wins, so 0x11..0x13 come from the SECOND range and 0x10 from
    // the first — codes 16..19 mapping to 'A'..'D', which is injective.
    let inverse = cmap
        .injective_inverse()
        .expect("overlapping ranges resolve to one answer per code");
    assert_eq!(inverse.get(&'A'), Some(&0x10));
    assert_eq!(inverse.get(&'B'), Some(&0x11));
    assert_eq!(inverse.get(&'C'), Some(&0x12));
    assert_eq!(inverse.get(&'D'), Some(&0x13));
}

/// A ligature — one code, several characters — has no single-character
/// inverse, so the run stays refused rather than pdfcer picking an
/// interpretation of "edit the f in ffi".
#[test]
fn a_ligature_destination_is_refused_by_name() {
    let cmap = bfchar_cmap(&[(1, "A"), (2, "ffi")]);
    let err = cmap.injective_inverse().unwrap_err();
    match err {
        NotInjective::MultiCharDestination { code, text } => {
            assert_eq!(code, 2);
            assert_eq!(text, "ffi");
        }
        other => panic!("expected MultiCharDestination, got {other:?}"),
    }
}

/// A CMap pdfcer did NOT author must be evaluated on its merits, or R110
/// is a rule that only ever says yes to pdfcer's own output — which
/// would make the whole lift self-serving rather than general.
///
/// The §9.10.3 EXAMPLE 2 body is the least pdfcer-shaped CMap available:
/// it is the standard's own, written years before this project.
#[test]
fn the_standards_own_example_cmap_is_evaluated_on_its_merits() {
    let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
    // Deliberately NOT asserting "it inverts". Whether the standard's
    // example happens to be injective is a fact about that example, not
    // about pdfcer. What matters is that the check RUNS on a foreign
    // CMap and reaches a decision with a stated reason, rather than
    // special-casing provenance.
    match cmap.injective_inverse() {
        Ok(inv) => assert!(!inv.is_empty(), "an Ok inverse must not be empty"),
        Err(e) => assert!(!e.to_string().is_empty(), "a refusal must state its reason"),
    }
}

/// An empty CMap has nothing to invert, and must say so rather than
/// returning an empty map a caller would read as "everything is
/// editable".
#[test]
fn an_empty_cmap_is_refused_rather_than_inverting_to_nothing() {
    let cmap = ToUnicodeCMap::parse(b"begincmap\nendcmap\n");
    assert_eq!(cmap.injective_inverse().unwrap_err(), NotInjective::Empty);
}

/// The §9.10.3 EXAMPLE 2 body, verbatim from the standard.
const EXAMPLE_2: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfrange
<0000> <005E> <0020>
<005F> <0061> [<00660066> <00660069> <00660066006C>]
endbfrange
1 beginbfchar
<3A51> <D840DC3E>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

#[test]
fn example_2_form_b_increments_the_last_byte() {
    let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
    // The spec's own gloss: "<0000> to <005E> are mapped to the
    // Unicode values U+0020 to U+007E".
    assert_eq!(cmap.lookup(0x0000).as_deref(), Some(" "));
    assert_eq!(cmap.lookup(0x0001).as_deref(), Some("!"));
    assert_eq!(cmap.lookup(0x005E).as_deref(), Some("~"));
    assert!(cmap.lookup(0x005F).is_some(), "form C takes over");
}

#[test]
fn example_2_form_c_is_one_to_many() {
    let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
    // The three ligature decompositions, from the standard's own
    // example. A `code -> char` model cannot represent these.
    assert_eq!(cmap.lookup(0x005F).as_deref(), Some("ff"));
    assert_eq!(cmap.lookup(0x0060).as_deref(), Some("fi"));
    assert_eq!(cmap.lookup(0x0061).as_deref(), Some("ffl"));
}

#[test]
fn example_2_form_a_surrogate_pair() {
    let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
    // <D840DC3E> is U+2003E in UTF-16BE. A UCS-2 decoder truncates
    // this silently; this assertion is the guard against that.
    assert_eq!(cmap.lookup(0x3A51).as_deref(), Some("\u{2003E}"));
    assert_eq!(cmap.lookup(0x3A51).unwrap().chars().count(), 1);
}

#[test]
fn example_2_ignores_table_120_entries_entirely() {
    // /CIDSystemInfo, /CMapName and the undocumented /CMapType 2 are
    // all "not pertinent" (§9.10.3) — the parse must neither reject
    // nor be confused by them, and `def`/`begin`/`end` must not be
    // mistaken for block terminators in a way that loses entries.
    let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
    assert_eq!(cmap.stats().singles, 4, "3 ligatures + 1 bfchar");
    assert_eq!(cmap.stats().ranges, 1);
    assert_eq!(cmap.codespace_widths(), &[2]);
}

#[test]
fn simple_font_one_byte_codespace() {
    let cmap = ToUnicodeCMap::parse(
        b"1 begincodespacerange <00> <FF> endcodespacerange
          2 beginbfchar <41> <0041> <42> <00C4> endbfchar",
    );
    assert_eq!(cmap.codespace_widths(), &[1]);
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
    assert_eq!(cmap.lookup(0x42).as_deref(), Some("\u{00C4}"));
    assert_eq!(cmap.lookup(0x43), None, "uncovered code maps to nothing");
}

#[test]
fn form_b_overflow_past_255_is_refused_not_carried() {
    // §9.10.3: "the value of the last byte in the string shall be
    // less than or equal to 255 - (srcCode2 - srcCode1) … otherwise
    // the result of mapping is undefined." dst 0x00FE over a range
    // of 4 overflows at the third code.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0010> <0014> <00FE> endbfrange");
    assert_eq!(cmap.lookup(0x0010).as_deref(), Some("\u{00FE}"));
    assert_eq!(cmap.lookup(0x0011).as_deref(), Some("\u{00FF}"));
    assert_eq!(
        cmap.lookup(0x0012),
        None,
        "past 255 the standard declares the result undefined"
    );
    assert_eq!(cmap.stats().range_overflows, 1);
}

#[test]
fn form_b_increments_bytes_not_code_points() {
    // The two coincide only when no carry occurs. A destination
    // whose low byte is 0xFF proves the difference: a code-point
    // increment would give U+0100, a byte increment overflows.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0000> <0001> <00FF> endbfrange");
    assert_eq!(cmap.lookup(0x0000).as_deref(), Some("\u{00FF}"));
    assert_eq!(cmap.lookup(0x0001), None);
}

#[test]
fn bfchar_wins_over_an_overlapping_bfrange() {
    let cmap = ToUnicodeCMap::parse(
        b"1 beginbfrange <0000> <00FF> <0041> endbfrange
          1 beginbfchar <0005> <005A> endbfchar",
    );
    assert_eq!(cmap.lookup(0x0004).as_deref(), Some("E"));
    assert_eq!(
        cmap.lookup(0x0005).as_deref(),
        Some("Z"),
        "bfchar is more specific"
    );
}

#[test]
fn overlapping_ranges_are_last_wins() {
    // §9.10.3 N5: no non-overlap rule exists for bf mappings and no
    // precedence is stated. pdfcer documents last-wins; this pins it.
    let cmap = ToUnicodeCMap::parse(
        b"2 beginbfrange <0000> <000F> <0041> <0000> <000F> <0061> endbfrange",
    );
    assert_eq!(cmap.lookup(0x0000).as_deref(), Some("a"));
}

#[test]
fn oversize_destination_is_rejected_and_counted() {
    // §9.10.3's own 512-byte cap. 600 bytes of hex = 300 code units.
    let mut src = b"1 beginbfchar <0001> <".to_vec();
    src.extend(std::iter::repeat_n(b'0', 1200));
    src.extend_from_slice(b"> endbfchar");
    let cmap = ToUnicodeCMap::parse(&src);
    assert_eq!(cmap.lookup(0x0001), None);
    assert_eq!(cmap.stats().oversize_destinations, 1);
}

#[test]
fn form_c_array_length_mismatch_is_counted_not_fatal() {
    // m must equal hi - lo + 1 (= 3 here); the array has 2.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0000> <0002> [<0041> <0042>] endbfrange");
    assert_eq!(cmap.lookup(0x0000).as_deref(), Some("A"));
    assert_eq!(cmap.lookup(0x0001).as_deref(), Some("B"));
    assert_eq!(cmap.lookup(0x0002), None);
    assert_eq!(cmap.stats().array_length_mismatches, 1);
}

#[test]
fn name_destination_is_the_documented_extension() {
    // §9.10.3 N2: name destinations are NOT described by the clause,
    // but real producers emit them. Accepted via the AGL, counted.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> /Adieresis endbfchar");
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("\u{00C4}"));
    assert_eq!(cmap.stats().name_destinations, 1);
}

#[test]
fn ligature_name_destination_resolves_to_many_code_points() {
    let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> /f_i endbfchar");
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("fi"));
}

#[test]
fn cid_operators_are_skipped_and_counted() {
    // §9.7.5.4 constraint (c) forbids cidrange in a ToUnicode CMap.
    // The usable bfchar entries must survive the violation.
    let cmap = ToUnicodeCMap::parse(
        b"1 begincidrange <0000> <00FF> 0 endcidrange
          1 beginbfchar <41> <0041> endbfchar",
    );
    assert_eq!(cmap.stats().foreign_operators, 1);
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
}

#[test]
fn usecmap_is_recognized_but_not_followed() {
    let cmap =
        ToUnicodeCMap::parse(b"/Adobe-Identity-UCS usecmap 1 beginbfchar <41> <0041> endbfchar");
    assert_eq!(cmap.stats().usecmap_references, 1);
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
}

#[test]
fn unterminated_block_keeps_what_it_parsed() {
    // §9.10.3 states no recovery for a missing `endbfchar`.
    let cmap = ToUnicodeCMap::parse(b"2 beginbfchar <41> <0041> <42> <0042>");
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
    assert_eq!(cmap.lookup(0x42).as_deref(), Some("B"));
}

#[test]
fn empty_input_is_an_empty_map() {
    let cmap = ToUnicodeCMap::parse(b"");
    assert!(cmap.is_empty());
    assert_eq!(cmap.lookup(0), None);
}

#[test]
fn garbage_input_does_not_panic_or_hang() {
    for junk in [
        &b"beginbfchar beginbfrange endbfchar endbfrange"[..],
        b"<> <> beginbfchar <> <> endbfchar",
        b"1 beginbfrange <FFFFFFFFFF> <00> <00> endbfrange",
        b"[[[[[[[[[[",
        b"1 beginbfchar",
        b"\x00\x01\x02\xFF\xFE",
    ] {
        let _ = ToUnicodeCMap::parse(junk);
    }
}

#[test]
fn odd_length_destination_is_counted() {
    // §9.10.3 N3: no validity rule exists for the UTF-16BE bytes.
    // <004> is an odd-digit hex string, which §7.3.4.3 pads to
    // <0040> — so use an explicitly odd BYTE count instead.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> <414243> endbfchar");
    assert_eq!(cmap.stats().malformed_destinations, 1);
    assert!(cmap.lookup(0x41).is_some(), "the decodable prefix survives");
}

#[test]
fn code_width_is_taken_from_the_source_string_length() {
    // <41> is code 0x41; <0041> is code 0x0041 — the SAME numeric
    // value, but a one-byte and a two-byte code respectively. The
    // map is keyed on the value; width belongs to the font.
    let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <0041> <0058> endbfchar");
    assert_eq!(cmap.lookup(0x41).as_deref(), Some("X"));
}
