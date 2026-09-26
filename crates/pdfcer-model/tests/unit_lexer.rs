//! Tests for `pdfcer_model::lexer`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_model::lexer::*;

/// Lex everything, panicking (in tests only) on error.
fn lex_all(input: &[u8]) -> Vec<Token> {
    let mut lx = Lexer::new(input);
    let mut out = Vec::new();
    while let Some(t) = lx.next_token().unwrap() {
        out.push(t);
    }
    out
}

fn kinds(input: &[u8]) -> Vec<TokenKind> {
    lex_all(input).into_iter().map(|t| t.kind).collect()
}

fn lex_err(input: &[u8]) -> LexErrorKind {
    let mut lx = Lexer::new(input);
    loop {
        match lx.next_token() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected a lex error, got clean EOF"),
            Err(e) => return e.kind,
        }
    }
}

// ---- §7.2 byte classes, whitespace, comments ----

#[test]
fn nul_is_whitespace() {
    // Table 1: NUL is one of the six whitespace characters.
    assert_eq!(
        kinds(b"12\x0034"),
        vec![TokenKind::Integer(12), TokenKind::Integer(34)]
    );
}

#[test]
fn comment_separates_tokens_spec_example() {
    // §7.2.3 EXAMPLE: equivalent to just `abc` and `123`.
    let ks = kinds(b"abc% comment ( /% ) blah blah blah\n123");
    assert_eq!(ks.len(), 2);
    assert!(matches!(ks[0], TokenKind::Keyword));
    assert!(matches!(ks[1], TokenKind::Integer(123)));
}

#[test]
fn comment_at_eof_without_eol() {
    assert_eq!(kinds(b"1 % trailing"), vec![TokenKind::Integer(1)]);
}

#[test]
fn delimiter_terminates_previous_token_without_whitespace() {
    // §7.2.2 / §7.3.5 gotcha: `123/Name` is two tokens.
    let ks = kinds(b"123/Name");
    assert_eq!(ks.len(), 2);
    assert!(matches!(ks[0], TokenKind::Integer(123)));
    assert!(matches!(ks[1], TokenKind::Name(ref n) if n == b"Name"));
}

// ---- §7.3.3 numerics ----

#[test]
fn spec_example_integers() {
    // §7.3.3 EXAMPLE 1.
    assert_eq!(
        kinds(b"123 43445 +17 -98 0"),
        vec![
            TokenKind::Integer(123),
            TokenKind::Integer(43445),
            TokenKind::Integer(17),
            TokenKind::Integer(-98),
            TokenKind::Integer(0),
        ]
    );
}

#[test]
fn spec_example_reals() {
    // §7.3.3 EXAMPLE 2 — including trailing-period and
    // leading-period forms.
    let ks = kinds(b"34.5 -3.62 +123.6 4. -.002 0.0");
    let vals: Vec<f64> = ks
        .iter()
        .map(|k| match k {
            TokenKind::Real(v) => *v,
            other => panic!("expected real, got {other:?}"),
        })
        .collect();
    assert_eq!(vals, vec![34.5, -3.62, 123.6, 4.0, -0.002, 0.0]);
}

#[test]
fn malformed_number_is_keyword_not_error() {
    // `1.2.3` and `.` violate the numeric grammar; the lexer hands
    // them to the parser as keywords (see scan_regular_run docs).
    assert!(matches!(kinds(b"1.2.3")[0], TokenKind::Keyword));
    assert!(matches!(kinds(b".")[0], TokenKind::Keyword));
}

#[test]
fn huge_integer_overflow_is_an_error() {
    assert_eq!(
        lex_err(b"99999999999999999999999999"),
        LexErrorKind::IntegerOverflow
    );
}

// ---- §7.3.4.2 literal strings ----

#[test]
fn literal_string_balanced_parens_no_escape() {
    // §7.3.4.2: balanced pairs need no special treatment.
    let ks = kinds(b"(Rate: 50% (approx))");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"Rate: 50% (approx)"));
}

#[test]
fn literal_string_table_3_escapes() {
    let ks = kinds(br"(\n\r\t\b\f\(\)\\)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\x0A\x0D\x09\x08\x0C()\\"));
}

#[test]
fn literal_string_unknown_escape_drops_backslash() {
    // §7.3.4.2: unknown escape → REVERSE SOLIDUS ignored. `\8`,
    // `\9` are not octal digits so they take this path too.
    let ks = kinds(br"(\q\8\9)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"q89"));
}

#[test]
fn literal_string_octal_spec_examples() {
    // §7.3.4.2 EXAMPLE 5: (\0053) is TWO bytes 05h '3';
    // (\053) and (\53) are one byte 2Bh.
    let ks = kinds(br"(\0053) (\053) (\53)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\x053"));
    assert!(matches!(ks[1], TokenKind::String(ref s) if s == b"+"));
    assert!(matches!(ks[2], TokenKind::String(ref s) if s == b"+"));
}

#[test]
fn literal_string_octal_overflow_keeps_low_8_bits() {
    // §7.3.4.2: "high-order overflow shall be ignored."
    // \777 = 511 = 0x1FF → 0xFF.
    let ks = kinds(br"(\777)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\xFF"));
}

#[test]
fn literal_string_line_continuation() {
    // §7.3.4.2 EXAMPLE 2: backslash-EOL removed entirely; CRLF
    // counts as one EOL marker.
    let ks = kinds(b"(These \\\ntwo)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"These two"));
    let ks = kinds(b"(These \\\r\ntwo)");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"These two"));
}

#[test]
fn literal_string_bare_eol_normalizes_to_lf() {
    // §7.3.4.2: bare CR, LF, or CRLF each decode to a single 0Ah.
    for input in [&b"(a\rb)"[..], &b"(a\nb)"[..], &b"(a\r\nb)"[..]] {
        let ks = kinds(input);
        assert!(
            matches!(ks[0], TokenKind::String(ref s) if s == b"a\nb"),
            "failed for {input:?}"
        );
    }
}

#[test]
fn literal_string_unterminated_is_error() {
    assert_eq!(lex_err(b"(oops"), LexErrorKind::UnterminatedString);
    assert_eq!(lex_err(b"(oops\\"), LexErrorKind::UnterminatedString);
}

// ---- §7.3.4.3 hex strings ----

#[test]
fn hex_string_odd_digit_pads_zero_spec_example() {
    // §7.3.4.3 EXAMPLE 2: <901FA> = 90 1F A0.
    let ks = kinds(b"<901FA>");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\x90\x1F\xA0"));
}

#[test]
fn hex_string_ignores_interior_whitespace() {
    let ks = kinds(b"< 90 1F\nA3 >");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\x90\x1F\xA3"));
}

#[test]
fn hex_string_lowercase_and_empty() {
    let ks = kinds(b"<deadbeef> <>");
    assert!(matches!(ks[0], TokenKind::String(ref s) if s == b"\xDE\xAD\xBE\xEF"));
    assert!(matches!(ks[1], TokenKind::String(ref s) if s.is_empty()));
}

#[test]
fn hex_string_invalid_byte_is_error() {
    assert_eq!(lex_err(b"<90ZZ>"), LexErrorKind::InvalidHexStringByte(b'Z'));
}

// ---- §7.3.5 names ----

#[test]
fn name_table_4_examples() {
    // Selected Table 4 rows, including the #-escape equivalences.
    let cases: &[(&[u8], &[u8])] = &[
        (b"/Name1", b"Name1"),
        (
            b"/A;Name_With-Various***Characters?",
            b"A;Name_With-Various***Characters?",
        ),
        (b"/1.2", b"1.2"),
        (b"/$$", b"$$"),
        (b"/@pattern", b"@pattern"),
        (b"/.notdef", b".notdef"),
        (b"/lime#20Green", b"lime Green"),
        (b"/paired#28#29parentheses", b"paired()parentheses"),
        (b"/The_Key_of_F#23_Minor", b"The_Key_of_F#_Minor"),
        (b"/A#42", b"AB"),
    ];
    for (written, decoded) in cases {
        let ks = kinds(written);
        assert!(
            matches!(ks[0], TokenKind::Name(ref n) if n == decoded),
            "failed for {written:?}"
        );
    }
}

#[test]
fn empty_name_is_valid() {
    // §7.3.5: SOLIDUS with no regular characters is the empty name.
    let ks = kinds(b"/ 5");
    assert!(matches!(ks[0], TokenKind::Name(ref n) if n.is_empty()));
    assert!(matches!(ks[1], TokenKind::Integer(5)));
}

#[test]
fn name_terminated_by_delimiter_not_consumed() {
    // RAG gotcha: `/Name(` is name `Name` then a string opener.
    let ks = kinds(b"/Name(x)");
    assert!(matches!(ks[0], TokenKind::Name(ref n) if n == b"Name"));
    assert!(matches!(ks[1], TokenKind::String(ref s) if s == b"x"));
}

#[test]
fn name_malformed_escape_is_error() {
    assert_eq!(lex_err(b"/A#5 "), LexErrorKind::MalformedNameEscape);
    assert_eq!(lex_err(b"/A#ZZ"), LexErrorKind::MalformedNameEscape);
    assert_eq!(lex_err(b"/A#00B"), LexErrorKind::NulInName);
}

// ---- structure tokens + keywords ----

#[test]
fn dict_and_array_delimiters() {
    assert_eq!(
        kinds(b"<< /K [ 1 ] >>"),
        vec![
            TokenKind::DictOpen,
            TokenKind::Name(b"K".to_vec()),
            TokenKind::ArrayOpen,
            TokenKind::Integer(1),
            TokenKind::ArrayClose,
            TokenKind::DictClose,
        ]
    );
}

#[test]
fn lone_close_delimiters_are_errors() {
    assert_eq!(lex_err(b" ) "), LexErrorKind::UnexpectedByte(b')'));
    assert_eq!(lex_err(b" > "), LexErrorKind::UnexpectedByte(b'>'));
}

#[test]
fn braces_lex_as_brace_tokens() {
    // Type 4 function bodies use { }; the base parser rejects them,
    // but the lexer must classify them (they are Table 2
    // delimiters).
    assert_eq!(
        kinds(b"{ 2 mul }"),
        vec![
            TokenKind::BraceOpen,
            TokenKind::Integer(2),
            TokenKind::Keyword,
            TokenKind::BraceClose,
        ]
    );
}

#[test]
fn keywords_and_spans() {
    // Spans are exact: `lexeme` recovers the raw source bytes —
    // the mechanism minimal-diff re-emission is built on.
    let buf: &[u8] = b"12 0 obj true endobj";
    let toks = lex_all(buf);
    let lexemes: Vec<&[u8]> = toks.iter().map(|t| t.lexeme(buf).unwrap()).collect();
    assert_eq!(lexemes, vec![&b"12"[..], b"0", b"obj", b"true", b"endobj"]);
    assert!(matches!(toks[2].kind, TokenKind::Keyword));
    assert!(matches!(toks[3].kind, TokenKind::Keyword));
}

#[test]
fn lexer_at_offset_and_past_end() {
    let buf: &[u8] = b"junk 42";
    let mut lx = Lexer::at(buf, 4);
    assert!(matches!(
        lx.next_token().unwrap().unwrap().kind,
        TokenKind::Integer(42)
    ));
    let mut past = Lexer::at(buf, 999);
    assert!(past.next_token().unwrap().is_none());
}

#[test]
fn name_lexeme_preserves_escaped_source_form() {
    // Round-trip discipline: /A#42 decodes to AB but the SPAN still
    // covers the original 5 source bytes (§7.3.5 NOTE 1 —
    // non-unique encodings; ARCHITECTURE.md §5).
    let buf: &[u8] = b"/A#42";
    let toks = lex_all(buf);
    assert!(matches!(toks[0].kind, TokenKind::Name(ref n) if n == b"AB"));
    assert_eq!(toks[0].lexeme(buf).unwrap(), b"/A#42");
}
