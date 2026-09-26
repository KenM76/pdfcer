//! Tests for `pdfcer_core::form_script::shape`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::form_script::shape::*;

/// Recognise the canonical generated calculate call, in both array
/// spellings, and read the operand names out of it.
#[test]
fn the_canonical_calculate_call_is_recognised_in_both_array_spellings() {
    for src in [
        br#"AFSimple_Calculate("SUM", new Array("Item.1","Item.2"));"#.as_slice(),
        br#"AFSimple_Calculate("SUM", ["Item.1","Item.2"]);"#.as_slice(),
    ] {
        let call = parse_single_call(src).expect("canonical generated form");
        assert_eq!(call.name, "AFSimple_Calculate");
        assert_eq!(
            call.arg(0).and_then(Literal::as_str),
            Some(b"SUM".as_slice())
        );
        let names = call.arg(1).and_then(Literal::as_array).expect("field list");
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].as_str(), Some(b"Item.1".as_slice()));
    }
}

/// The canonical generated format call, including the boolean and the
/// empty-string arguments that make its shape distinctive.
#[test]
fn the_canonical_format_call_is_recognised() {
    let call = parse_single_call(br#"AFNumber_Format(2, 0, 0, 0, "", true);"#).expect("canonical");
    assert_eq!(call.name, "AFNumber_Format");
    assert_eq!(call.args.len(), 6);
    assert_eq!(call.arg(0).and_then(Literal::as_int), Some(2));
    assert_eq!(call.arg(4).and_then(Literal::as_str), Some(b"".as_slice()));
    assert_eq!(call.arg(5).and_then(Literal::as_bool), Some(true));
}

/// **Everything that is not exactly one literal call is refused.**
///
/// This is the module's whole safety argument, so it is asserted as a
/// table rather than scattered across cases: each entry is a script that
/// a looser recogniser might accept, and each would produce a wrong
/// recompute if it were.
#[test]
fn anything_beyond_one_literal_call_is_refused() {
    let refused: &[(&[u8], &str)] = &[
        (
            b"x = 1; AFSimple_Calculate(\"SUM\", [\"A\"]);",
            "a second statement",
        ),
        (
            b"if (a) AFSimple_Calculate(\"SUM\", [\"A\"]);",
            "conditional",
        ),
        (
            b"AFSimple_Calculate(\"SUM\", flds);",
            "an identifier operand",
        ),
        (
            b"AFSimple_Calculate(\"SU\" + \"M\", [\"A\"]);",
            "concatenation",
        ),
        (
            b"event.value = AFSimple_Calculate(\"SUM\", [\"A\"]);",
            "assignment",
        ),
        (
            b"AFSimple_Calculate(\"SUM\", [\"A\"]) + 1",
            "a trailing operator",
        ),
        (
            b"function f() { AFNumber_Format(2,0,0,0,\"\",true); }",
            "a wrapper",
        ),
        (
            b"AFSimple_Calculate(\"SUM\", new Date());",
            "a non-Array constructor",
        ),
        (
            b"AFNumber_Format(2, 0, 0, 0, \"\", true) /* unterminated",
            "bad lex",
        ),
        (
            b"AFSimple_Calculate(\"SUM\", [\"A\",]);",
            "a trailing comma",
        ),
        (
            b"AFSimple_Calculate(\"SUM\", [\"A\"]); AFNumber_Format(0,0,0,0,\"\",true);",
            "two calls",
        ),
        (b"AFSimple_Calculate", "no call at all"),
        (b"", "empty"),
    ];
    for (src, why) in refused {
        assert!(
            parse_single_call(src).is_none(),
            "{why} must not be recognised: {}",
            String::from_utf8_lossy(src)
        );
    }
}

/// A near-miss NAME is refused by the caller, not here — but the parse
/// must still read it faithfully, so the classifier can disclose what it
/// actually saw rather than a guess.
#[test]
fn a_lookalike_name_parses_as_itself_and_is_not_normalised() {
    let call = parse_single_call(b"myAFNumber_Format(2);").expect("parses");
    assert_eq!(call.name, "myAFNumber_Format", "no prefix stripping");
    let call = parse_single_call(b"afnumber_format(2);").expect("parses");
    assert_eq!(
        call.name, "afnumber_format",
        "case is preserved for the matcher"
    );
}

/// Comments and whitespace are skipped wherever they may appear, so an
/// operator's annotation of a generated script does not silently turn
/// off the recompute.
#[test]
fn comments_and_whitespace_do_not_defeat_recognition() {
    let src = br#"
        // Acrobat generated
        AFSimple_Calculate (
            "SUM" /* op */ ,
            [ "A" , "B" ]
        ) ;
        // trailing note
    "#;
    let call = parse_single_call(src).expect("trivia is not structure");
    assert_eq!(call.name, "AFSimple_Calculate");
    assert_eq!(
        call.arg(1).and_then(Literal::as_array).map(<[_]>::len),
        Some(2)
    );
}

/// String escapes are resolved, because a field name is looked up by the
/// bytes the script meant, not the bytes it was written with.
#[test]
fn string_escapes_resolve_to_the_name_the_script_meant() {
    let call = parse_single_call(br#"F("a\tb", "c\u00e9d", 'single');"#).expect("parses");
    assert_eq!(
        call.arg(0).and_then(Literal::as_str),
        Some(b"a\tb".as_slice())
    );
    assert_eq!(
        call.arg(1).and_then(Literal::as_str),
        Some("céd".as_bytes()),
        "\\u escapes encode as UTF-8"
    );
    assert_eq!(
        call.arg(2).and_then(Literal::as_str),
        Some(b"single".as_slice())
    );
}

/// **No coercion.** JavaScript would equate several of these; this
/// module does not, because a type mismatch against the canonical call
/// means the script was edited, and an edited script is `Custom`.
#[test]
fn literal_accessors_do_not_coerce() {
    let call = parse_single_call(br#"F("2", 2, 2.5, true, null);"#).expect("parses");
    assert_eq!(
        call.arg(0).and_then(Literal::as_num),
        None,
        "\"2\" is not 2"
    );
    assert_eq!(
        call.arg(1).and_then(Literal::as_str),
        None,
        "2 is not \"2\""
    );
    assert_eq!(
        call.arg(1).and_then(Literal::as_int),
        Some(2),
        "2.0 is integral"
    );
    assert_eq!(
        call.arg(2).and_then(Literal::as_int),
        None,
        "2.5 is not an int"
    );
    assert_eq!(call.arg(3).and_then(Literal::as_num), None, "true is not 1");
    assert_eq!(
        call.arg(4).and_then(Literal::as_bool),
        None,
        "null is not false"
    );
}

/// Numeric forms that a generated call never uses are refused rather
/// than half-read.
#[test]
fn only_plain_decimal_numbers_are_read() {
    for src in [
        b"F(0x10);".as_slice(),
        b"F(1abc);",
        b"F(.);",
        b"F(-);",
        b"F(Infinity);",
        b"F(NaN);",
        b"F(1e);",
    ] {
        assert!(
            parse_single_call(src).is_none(),
            "{} must not parse",
            String::from_utf8_lossy(src)
        );
    }
    assert_eq!(
        parse_single_call(b"F(-1.5e2);").and_then(|c| c.arg(0).and_then(Literal::as_num)),
        Some(-150.0),
        "but a signed decimal with an exponent is unambiguous"
    );
}

/// Bounds hold: an oversized script and an over-nested literal are both
/// refused without recursing away the stack.
#[test]
fn the_recogniser_is_bounded_against_a_hostile_document() {
    let big = vec![b' '; MAX_LEN + 1];
    assert!(parse_single_call(&big).is_none(), "length is capped");

    let mut deep = b"F(".to_vec();
    deep.extend(std::iter::repeat_n(b'[', MAX_DEPTH + 2));
    deep.extend(std::iter::repeat_n(b']', MAX_DEPTH + 2));
    deep.extend_from_slice(b");");
    assert!(parse_single_call(&deep).is_none(), "depth is capped");
}

/// The `Display` form shows what pdfcer understood, which is what a
/// disclosure line must say.
#[test]
fn display_shows_the_understood_call() {
    let call = parse_single_call(br#"AFSimple_Calculate("SUM", new Array("A","B"))"#).unwrap();
    assert_eq!(call.to_string(), r#"AFSimple_Calculate("SUM", ["A", "B"])"#);
}
