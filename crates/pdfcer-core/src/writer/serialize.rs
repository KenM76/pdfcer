//! # COS object serialization (ISO 32000-1 §7.3)
//!
//! Value tree → bytes. The inverse of `crate::parser`, and deliberately
//! **not** its exact inverse: PDF syntax is non-canonical, so a decoded
//! value does not determine its source bytes (see `crate::span`'s module
//! docs for the three worked cases — `/A#42` vs `/AB`, a bare CRLF
//! inside a literal string, `4.` vs `+4` vs `4.0`).
//!
//! ## What this module is for, and what it is NOT for
//!
//! Under `ARCHITECTURE.md` §5, an object pdfcer did not logically modify
//! re-emits its **retained source bytes verbatim**. That path never
//! enters this module — it is a `memcpy` from the buffer, in
//! `crate::writer::save`. This module serializes only:
//!
//! - objects with [`Provenance::ObjectStream`] provenance that a full
//!   rewrite has to promote out of their container (they have no
//!   file-level bytes to copy — see `crate::object::Provenance`);
//! - dictionaries pdfcer constructs itself: the trailer (§7.5.5), a
//!   cross-reference stream's dictionary (§7.5.8.2), and — only under
//!   an explicit [`ProducerPolicy`](super::ProducerPolicy) — the
//!   document information dictionary (§14.3.3);
//! - objects a future editing Pass actually changed.
//!
//! Because of that split, "byte-exact" here means **exact per §7.3's
//! grammar and unambiguous on re-parse**, not "identical to whatever
//! the original producer wrote". Round-trip identity for untouched
//! content is the verbatim path's job, not this one's.
//!
//! ## Emission choices, and why each one
//!
//! | Type | Form emitted | Clause / reason |
//! |---|---|---|
//! | `null` | `null` | §7.3.9 |
//! | boolean | `true` / `false` | §7.3.2 |
//! | integer | decimal, no `+`, no padding | §7.3.3 |
//! | real | fixed-point, **never exponential**, always one `.` | §7.3.3 |
//! | string | literal `(…)` when printable, else hex `<…>` | §7.3.4 |
//! | name | `/` + `#`-escaped non-regular bytes | §7.3.5 |
//! | array | `[` … `]`, single SP between elements | §7.3.6 |
//! | dictionary | `<<` … `>>`, `/Key value` pairs | §7.3.7 |
//! | stream | dict + `stream` LF + data + LF + `endstream` | §7.3.8 |
//! | reference | `N G R` | §7.3.10 |
//!
//! ### Reals must not use exponential notation
//!
//! §7.3.3, verbatim: *"A conforming writer shall not use the PostScript
//! syntax for numbers with non-decimal radices (such as `16#FFFE`) …"*
//! and, on real numbers, the grammar admits only an optional sign, a
//! digit run, a period and a digit run. **`1e-5` is not a PDF real.**
//! Rust's `{}`/`{:?}` float formatting reaches for exponential notation
//! at both extremes, so [`write_real`] detects and expands it. A writer
//! that skips this produces files that fail to parse only for very
//! large or very small numbers — i.e. rarely, and catastrophically.
//!
//! ### Integral reals keep their decimal point
//!
//! `Object::Real(4.0)` emits `4.0`, not `4`. Emitting `4` would be
//! re-parsed as `Object::Integer(4)`, silently collapsing the
//! type distinction §7.3.3 draws and `crate::object` deliberately
//! preserves. That matters for the parse→write→parse fuzz oracle, which
//! compares value trees.
//!
//! ### Strings choose literal or hex by content, never by preference
//!
//! Both forms are legal for any byte sequence (§7.3.4.2/§7.3.4.3). The
//! rule here is mechanical: literal form when every byte is printable
//! ASCII (0x20–0x7E), hex form otherwise. Rationale — a literal string
//! carrying raw control bytes is legal but hostile to `diff`, and the
//! §7.3.4.2 EOL rule (*"an end-of-line marker appearing within a
//! literal string … shall be treated as a byte value of (0Ah)"*) means
//! a raw CR inside a literal string **does not survive a round trip**.
//! Hex form has no such hazard. Within literal form, `\`, `(` and `)`
//! are always escaped, which makes paren balance a non-issue by
//! construction rather than by counting.
//!
//! ### `/Length` is always recomputed, never trusted
//!
//! §7.3.8.2 Table 5: `/Length` is *"the number of bytes from the
//! beginning of the line following the keyword `stream` to the last
//! byte just before the keyword `endstream`"*. [`write_stream`]
//! overwrites any `/Length` in the source dictionary with the actual
//! post-encoder byte count and **drops an indirect `/Length`
//! outright**, because carrying a reference to a length object whose
//! value we just changed would emit a self-contradicting file.

use crate::object::{Dict, Name, ObjId, Object, Stream};

use super::encoder::ObjectEncoder;

/// Serialize one object into `out`.
///
/// `owner` is the indirect object this value belongs to, threaded
/// through solely for the [`ObjectEncoder`] seam (§7.6 per-object keys
/// — see [`super::encoder`]). `source` is the retained buffer that
/// stream data spans index into.
///
/// A stream whose `data_span` does not lie inside `source` emits an
/// **empty** stream with `/Length 0` rather than failing: a span
/// mismatch is a caller bug (a span applied to the wrong buffer), and
/// `pdfcer-core`'s panic-free policy plus the writer's
/// fail-clean-at-the-boundary posture make degrading here strictly
/// better than an unwrap. The condition is detectable by the caller
/// through [`stream_data`] returning `None`, and `save_full` checks it
/// explicitly rather than relying on this fallback.
///
/// ## No wildcard match arm, deliberately
///
/// [`Object`] is `#[non_exhaustive]` for downstream crates, but within
/// `pdfcer-core` the match below is checked exhaustively. That is the
/// point: adding a ninth COS type becomes a **compile error here**, at
/// the one place that must consciously decide how to emit it. A `_` arm
/// would instead ship the new type as `null` and lose data silently.
/// The same reasoning applies to every `XrefEntry` and `SectionShape`
/// match in [`super::xref_out`] and [`super::save`].
pub fn write_object(
    out: &mut Vec<u8>,
    obj: &Object,
    owner: ObjId,
    source: &[u8],
    encoder: &dyn ObjectEncoder,
) {
    match obj {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(true) => out.extend_from_slice(b"true"),
        Object::Boolean(false) => out.extend_from_slice(b"false"),
        Object::Integer(v) => out.extend_from_slice(itoa(*v).as_bytes()),
        Object::Real(v) => write_real(out, *v),
        Object::String(s) => write_string(out, &encoder.encode_string(owner, s)),
        Object::Name(n) => write_name(out, n),
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object(out, item, owner, source, encoder);
            }
            out.push(b']');
        }
        Object::Dict(d) => write_dict(out, d, owner, source, encoder),
        Object::Stream(s) => write_stream(out, s, owner, source, encoder),
        Object::Reference(id) => {
            out.extend_from_slice(itoa(i64::from(id.num)).as_bytes());
            out.push(b' ');
            out.extend_from_slice(itoa(i64::from(id.generation)).as_bytes());
            out.extend_from_slice(b" R");
        }
    }
}

/// Serialize a complete indirect-object definition —
/// `N G obj` … `endobj` — exactly as the classic body form requires
/// (§7.3.10).
///
/// The emitted shape is `N G obj\n<value>\nendobj\n`. The two EOLs are
/// §7.5.1 line discipline; the trailing one terminates the `endobj`
/// line so the next object begins on a fresh line, which §7.5.1's
/// *"each line shall be terminated by an end-of-line marker"* requires
/// and which keeps the file `diff`-friendly.
pub fn write_indirect(
    out: &mut Vec<u8>,
    id: ObjId,
    value: &Object,
    source: &[u8],
    encoder: &dyn ObjectEncoder,
) {
    out.extend_from_slice(itoa(i64::from(id.num)).as_bytes());
    out.push(b' ');
    out.extend_from_slice(itoa(i64::from(id.generation)).as_bytes());
    out.extend_from_slice(b" obj\n");
    write_object(out, value, id, source, encoder);
    out.extend_from_slice(b"\nendobj\n");
}

/// The raw (still filter-encoded) bytes of `stream` within `source`, or
/// `None` when the span does not lie inside the buffer.
///
/// Exposed so callers can distinguish "this stream is genuinely empty"
/// from "this span belongs to another buffer" before serializing.
#[must_use]
pub fn stream_data<'a>(stream: &Stream, source: &'a [u8]) -> Option<&'a [u8]> {
    stream.data_span.slice(source)
}

/// Emit a dictionary (§7.3.7).
///
/// Entry order is the dictionary's stored order, which `crate::object`
/// preserves from the parse. §7.3.7 says written order *"shall be
/// ignored"*, so preserving it is always semantically safe, and it
/// makes a re-serialized dictionary diff minimally against its source.
fn write_dict(
    out: &mut Vec<u8>,
    dict: &Dict,
    owner: ObjId,
    source: &[u8],
    encoder: &dyn ObjectEncoder,
) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        write_name(out, key);
        // A SPACE between key and value is required only where the
        // value's first byte would otherwise extend the name token —
        // i.e. for numbers, keywords and references. `/` `(` `<` `[`
        // are delimiters and self-separating (§7.2.2), but emitting the
        // space unconditionally costs one byte and removes an entire
        // class of "works until the value happens to be a number" bug.
        out.push(b' ');
        write_object(out, value, owner, source, encoder);
    }
    out.extend_from_slice(b">>");
}

/// Emit a stream object (§7.3.8): dictionary, framing keywords, data.
///
/// Framing rules honoured here, each a `shall`/`should` from §7.3.8.1:
///
/// - *"The keyword `stream` … shall be followed by an end-of-line
///   marker consisting of either a CARRIAGE RETURN and a LINE FEED or
///   just a LINE FEED, **and not by a CARRIAGE RETURN alone**."* — a
///   bare LF is emitted.
/// - *"There should be an end-of-line marker after the data and before
///   `endstream`; this marker **shall not** be included in the stream
///   length."* — a bare LF is emitted and excluded from `/Length`.
/// - *"There shall not be any extra bytes, other than white space,
///   between `endstream` and `endobj`."* — satisfied by
///   [`write_indirect`], which emits exactly one LF.
fn write_stream(
    out: &mut Vec<u8>,
    stream: &Stream,
    owner: ObjId,
    source: &[u8],
    encoder: &dyn ObjectEncoder,
) {
    let raw = stream_data(stream, source).unwrap_or(&[]);
    let encoded = encoder.encode_stream(owner, raw);

    // Table 5: `/Length` is the post-filter, post-encryption byte
    // count. Any `/Length` the source dictionary carried — direct or
    // indirect — is replaced; see the module docs for why an indirect
    // one cannot simply be forwarded.
    let mut dict = Dict::new();
    for (key, value) in stream.dict.iter() {
        if key.as_bytes() == b"Length" {
            continue;
        }
        dict.insert(key.clone(), value.clone());
    }
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(encoded.len()).unwrap_or(i64::MAX)),
    );

    write_dict(out, &dict, owner, source, encoder);
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&encoded);
    out.extend_from_slice(b"\nendstream");
}

/// Emit a name (§7.3.5) with `#`-escaping.
///
/// §7.3.5's rule set, applied exactly:
///
/// - *"the NUMBER SIGN shall be written as `#23`"* — `#` is always
///   escaped, or the escape mechanism becomes ambiguous.
/// - *"Regular characters that are outside the range EXCLAMATION MARK
///   (21h) to TILDE (7Eh) shall be written using the hexadecimal
///   notation."* — so is every byte below `!` and above `~`.
/// - Delimiters (`( ) < > [ ] { } / %`, §7.2.2 Table 2) are not regular
///   characters and would terminate the name token, so they are escaped
///   too. §7.3.5's own EXAMPLE (`/A#42` for `AB`) confirms `#`-escaping
///   is permitted for *any* byte, so over-escaping is always legal.
///
/// NOTE: this deliberately does **not** try to reproduce the source
/// file's escaping choices. `/AB` and `/A#42` are the same name
/// (§7.3.5 NOTE 1) and the parser decodes both to the same bytes;
/// preserving the original spelling is the verbatim path's job.
///
/// ## The one byte with no representation: NUL
///
/// §7.3.5 defines a name as *"a sequence of any characters except
/// null (0)"*, and `crate::lexer` enforces it (`NulInName` — `/A#00B`
/// is a lex error, not a name). So a `Name` holding a NUL byte is
/// **unrepresentable in PDF**, and no such `Name` can arise from
/// parsing a file: the only way to build one is for pdfcer's own code
/// to construct it, which is a caller bug.
///
/// This function nevertheless emits `#00` for it rather than dropping
/// the byte or truncating the name. Dropping would silently produce a
/// *different, valid* name — `/A\0B` becoming `/AB` — which is the
/// worst outcome available: a plausible, working, wrong file. Emitting
/// `#00` produces a file pdfcer's own strict lexer refuses to reload,
/// so the bug surfaces immediately at the round-trip gate instead of
/// shipping as silent data loss.
pub(crate) fn write_name(out: &mut Vec<u8>, name: &Name) {
    out.push(b'/');
    for &b in name.as_bytes() {
        let regular = (b'!'..=b'~').contains(&b)
            && b != b'#'
            && !matches!(
                b,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
            );
        if regular {
            out.push(b);
        } else {
            out.push(b'#');
            push_hex_byte(out, b);
        }
    }
}

/// Append `b` as two uppercase hexadecimal digits.
///
/// Computed rather than table-indexed because `pdfcer-core` denies
/// `clippy::indexing_slicing` crate-wide (the panic-free policy): a
/// lookup table would need either an `#[allow]` or a `.get().unwrap()`,
/// and arithmetic on a nibble cannot fail at all. Uppercase because
/// §7.3.4.3's and §7.3.5's own EXAMPLEs use it; both cases are legal on
/// read (§7.3.4.3 accepts "0–9, A–F, or a–f").
fn push_hex_byte(out: &mut Vec<u8>, b: u8) {
    out.push(hex_digit(b >> 4));
    out.push(hex_digit(b & 0x0F));
}

/// One nibble (0–15) as an uppercase ASCII hexadecimal digit.
const fn hex_digit(nibble: u8) -> u8 {
    let n = nibble & 0x0F;
    if n < 10 { b'0' + n } else { b'A' + (n - 10) }
}

/// Emit a string (§7.3.4), choosing literal or hexadecimal form by
/// content (module docs).
pub(crate) fn write_string(out: &mut Vec<u8>, data: &[u8]) {
    let printable = data.iter().all(|&b| (0x20..=0x7E).contains(&b));
    if printable {
        out.push(b'(');
        for &b in data {
            // §7.3.4.2 Table 3: `\(`, `\)`, `\\`. Escaping all three
            // unconditionally makes paren balance structural.
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b')');
    } else {
        out.push(b'<');
        for &b in data {
            push_hex_byte(out, b);
        }
        out.push(b'>');
    }
}

/// Emit a real number (§7.3.3) in fixed-point form.
///
/// Guarantees, in order of importance:
///
/// 1. **Never exponential.** §7.3.3's grammar has no exponent; see the
///    module docs. Rust's shortest-round-trip formatter switches to
///    `1e20`/`1e-7` outside a middle band, so those are expanded by
///    hand via [`expand_exponent`].
/// 2. **Always contains a `.`**, so a re-parse yields `Object::Real`
///    rather than `Object::Integer`.
/// 3. **Round-trips through `f64`** for every value inside the
///    representable band, because `{:?}` is the shortest such form.
///
/// Non-finite inputs (`NaN`, `±∞`) cannot arise from parsing a
/// conforming file — §7.3.3's grammar admits neither — but `f64`
/// permits them, so they emit `0.0`. Silently substituting is the
/// least-harmful option available to a panic-free crate: the
/// alternatives are a panic (forbidden here) or emitting a token no
/// PDF reader can parse.
pub(crate) fn write_real(out: &mut Vec<u8>, v: f64) {
    if !v.is_finite() {
        out.extend_from_slice(b"0.0");
        return;
    }
    let s = format!("{v:?}");
    let s = if s.contains(['e', 'E']) {
        expand_exponent(v)
    } else {
        s
    };
    if s.contains('.') {
        out.extend_from_slice(s.as_bytes());
    } else {
        out.extend_from_slice(s.as_bytes());
        out.extend_from_slice(b".0");
    }
}

/// Render `v` without an exponent, trimming redundant trailing zeros.
///
/// `{:.*}` with a generous precision always produces fixed-point
/// notation. The precision is chosen from the exponent so that small
/// magnitudes keep their significant digits: a value near `1e-300`
/// needs ~317 fractional digits before the first non-zero one appears.
/// Annex C's real-number range is far narrower than this
/// (`±3.403 × 10^38`, ~5 significant decimal digits), so this path is
/// reached only by synthesized or hostile values — it exists to be
/// *correct*, not fast.
fn expand_exponent(v: f64) -> String {
    let magnitude = if v == 0.0 {
        0
    } else {
        v.abs().log10().floor() as i32
    };
    let precision = usize::try_from(17 - magnitude.min(0))
        .unwrap_or(17)
        .min(340);
    let mut s = format!("{v:.precision$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.push('0');
        }
    }
    s
}

/// Decimal rendering of an integer (§7.3.3): no sign for non-negative
/// values, no padding, no radix prefix.
fn itoa(v: i64) -> String {
    v.to_string()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::super::encoder::IdentityEncoder;
    use super::*;
    use crate::parser::Parser;
    use crate::span::ByteSpan;

    fn ser(obj: &Object) -> Vec<u8> {
        let mut out = Vec::new();
        write_object(&mut out, obj, ObjId::new(1, 0), &[], &IdentityEncoder);
        out
    }

    fn ser_str(obj: &Object) -> String {
        String::from_utf8(ser(obj)).unwrap()
    }

    /// Parse back what we just wrote — the property every emission rule
    /// in this module ultimately serves.
    fn roundtrip(obj: &Object) -> Object {
        let bytes = ser(obj);
        Parser::at(&bytes, 0).parse_object().unwrap()
    }

    #[test]
    fn scalars_emit_their_spec_forms() {
        assert_eq!(ser_str(&Object::Null), "null");
        assert_eq!(ser_str(&Object::Boolean(true)), "true");
        assert_eq!(ser_str(&Object::Boolean(false)), "false");
        assert_eq!(ser_str(&Object::Integer(0)), "0");
        assert_eq!(ser_str(&Object::Integer(-42)), "-42");
        // §7.3.3: no `+` sign on write, even though the grammar allows
        // one on read.
        assert_eq!(ser_str(&Object::Integer(17)), "17");
    }

    #[test]
    fn integral_real_keeps_its_decimal_point() {
        // Emitting `4` would re-parse as Integer, collapsing the
        // §7.3.3 type distinction. See module docs.
        assert_eq!(ser_str(&Object::Real(4.0)), "4.0");
        assert!(matches!(roundtrip(&Object::Real(4.0)), Object::Real(_)));
    }

    #[test]
    fn reals_never_use_exponential_notation() {
        // §7.3.3's grammar has no exponent; `{:?}` would emit `1e20`.
        for v in [1e20_f64, 1e-7, -3.5e-9, 1.5e30, f64::MIN_POSITIVE] {
            let s = ser_str(&Object::Real(v));
            assert!(
                !s.contains('e') && !s.contains('E'),
                "exponential notation leaked for {v}: {s}"
            );
            assert!(s.contains('.'), "no decimal point for {v}: {s}");
        }
    }

    #[test]
    fn finite_reals_round_trip_through_the_parser() {
        for v in [0.0_f64, -0.5, 3.14259, 1234.5678, -1e10, 2.5e-5] {
            let Object::Real(back) = roundtrip(&Object::Real(v)) else {
                panic!("{v} did not re-parse as a real");
            };
            assert!(
                (back - v).abs() <= v.abs() * 1e-12,
                "{v} round-tripped to {back}"
            );
        }
    }

    #[test]
    fn non_finite_reals_degrade_rather_than_panic() {
        // Unreachable from a conforming file, reachable from f64.
        assert_eq!(ser_str(&Object::Real(f64::NAN)), "0.0");
        assert_eq!(ser_str(&Object::Real(f64::INFINITY)), "0.0");
        assert_eq!(ser_str(&Object::Real(f64::NEG_INFINITY)), "0.0");
    }

    #[test]
    fn names_escape_every_non_regular_byte() {
        assert_eq!(ser_str(&Object::Name(Name::from(b"Type"))), "/Type");
        // §7.3.5: `#` itself must always be escaped.
        assert_eq!(ser_str(&Object::Name(Name::from(b"A#B"))), "/A#23B");
        // Space is outside 21h..7Eh.
        assert_eq!(
            ser_str(&Object::Name(Name::from(b"lime Green"))),
            "/lime#20Green"
        );
        // Delimiters would terminate the token.
        assert_eq!(ser_str(&Object::Name(Name::from(b"a(b"))), "/a#28b");
        // The empty name is legal (§7.3.5) and emits as a bare solidus.
        assert_eq!(ser_str(&Object::Name(Name::from(b""))), "/");
    }

    #[test]
    fn names_round_trip_including_hostile_bytes() {
        // NUL is deliberately absent from this list: §7.3.5 defines a
        // name as "any characters except null", so a NUL-bearing name
        // has no PDF representation at all. See the next test.
        for raw in [
            &b"Type"[..],
            b"A#B",
            b"lime Green",
            b"",
            b"\xFF\x7F\x01",
            b"/[]<>{}%()",
            b"\t\n\r ",
        ] {
            let obj = Object::Name(Name::from(raw));
            assert_eq!(roundtrip(&obj), obj, "name {raw:?} did not round-trip");
        }
    }

    #[test]
    fn nul_in_a_name_fails_loudly_rather_than_silently_changing_the_name() {
        // §7.3.5: names exclude NUL, and `crate::lexer` enforces it.
        // The writer must NOT "fix" this by dropping the byte — that
        // would turn /A\0B into the different-but-valid /AB, i.e. a
        // plausible, working, wrong file. Emitting `#00` makes the
        // caller bug fail at the reload gate instead.
        let bytes = ser(&Object::Name(Name::from(b"A\x00B")));
        assert_eq!(bytes, b"/A#00B");
        assert!(
            Parser::at(&bytes, 0).parse_object().is_err(),
            "a NUL-bearing name must not silently reload as a valid name"
        );
    }

    #[test]
    fn strings_pick_literal_or_hex_by_content() {
        assert_eq!(ser_str(&Object::String(b"hello".to_vec())), "(hello)");
        // Parens and backslash always escaped — balance by construction.
        assert_eq!(
            ser_str(&Object::String(b"a(b)c\\d".to_vec())),
            "(a\\(b\\)c\\\\d)"
        );
        // Non-printable content goes hex, never literal (module docs:
        // a raw CR in a literal string decodes to LF — lossy).
        assert_eq!(ser_str(&Object::String(vec![0x0D, 0x0A])), "<0D0A>");
        assert_eq!(ser_str(&Object::String(vec![0xFF, 0x00])), "<FF00>");
    }

    #[test]
    fn strings_round_trip_for_every_byte_value() {
        // The §7.3.4.2 CRLF hazard in one assertion: all 256 byte
        // values survive, which is only true because the non-printable
        // path takes hex form.
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(roundtrip(&Object::String(all.clone())), Object::String(all));
        for probe in [&b""[..], b"()", b"\\", b"\r", b"\n", b"\r\n"] {
            let obj = Object::String(probe.to_vec());
            assert_eq!(roundtrip(&obj), obj, "string {probe:?} did not round-trip");
        }
    }

    #[test]
    fn containers_and_references_emit_their_forms() {
        assert_eq!(
            ser_str(&Object::Array(vec![
                Object::Integer(1),
                Object::Real(2.5),
                Object::Reference(ObjId::new(3, 7)),
            ])),
            "[1 2.5 3 7 R]"
        );
        assert_eq!(ser_str(&Object::Array(vec![])), "[]");
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Page")));
        d.insert(Name::from(b"Count"), Object::Integer(3));
        assert_eq!(ser_str(&Object::Dict(d)), "<</Type /Page/Count 3>>");
    }

    #[test]
    fn adjacent_numeric_values_stay_separated() {
        // The concrete bug the unconditional key/value SPACE prevents:
        // `/A1/B2` would lex `/A` then `1` fine, but `<</A1>>` without
        // the space is the name `/A1`, not `/A` = 1.
        let mut d = Dict::new();
        d.insert(Name::from(b"A"), Object::Integer(1));
        d.insert(Name::from(b"B"), Object::Integer(2));
        let bytes = ser(&Object::Dict(d));
        let Object::Dict(back) = Parser::at(&bytes, 0).parse_object().unwrap() else {
            panic!("not a dict");
        };
        assert_eq!(back.get(b"A").unwrap().as_int(), Some(1));
        assert_eq!(back.get(b"B").unwrap().as_int(), Some(2));
    }

    #[test]
    fn nested_containers_round_trip() {
        let mut inner = Dict::new();
        inner.insert(Name::from(b"K"), Object::Array(vec![Object::Null]));
        let obj = Object::Array(vec![
            Object::Dict(inner),
            Object::Array(vec![Object::Boolean(false)]),
        ]);
        assert_eq!(roundtrip(&obj), obj);
    }

    #[test]
    fn stream_length_is_recomputed_and_indirect_length_dropped() {
        // Table 5: /Length is the actual byte count. A stale or
        // indirect /Length must never survive re-serialization.
        let source = b"XXXXpayload!XXXX";
        let mut dict = Dict::new();
        // Deliberately wrong AND indirect — both hazards at once.
        dict.insert(Name::from(b"Length"), Object::Reference(ObjId::new(9, 0)));
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
        let stream = Stream {
            dict,
            data_span: ByteSpan::new(4, 8),
        };
        let mut out = Vec::new();
        write_object(
            &mut out,
            &Object::Stream(stream),
            ObjId::new(5, 0),
            source,
            &IdentityEncoder,
        );
        let text = String::from_utf8_lossy(&out).into_owned();
        assert!(text.contains("/Length 8"), "{text}");
        assert!(!text.contains("9 0 R"), "indirect /Length survived: {text}");
        // §7.3.8.1 framing: LF after `stream`, LF before `endstream`.
        assert!(text.contains(">>\nstream\npayload!\nendstream"), "{text}");
    }

    #[test]
    fn stream_with_out_of_range_span_degrades_to_empty() {
        // A span from another buffer is a caller bug; the panic-free
        // policy says degrade, and `stream_data` lets callers detect it.
        let stream = Stream {
            dict: Dict::new(),
            data_span: ByteSpan::new(100, 10),
        };
        assert!(stream_data(&stream, b"short").is_none());
        let mut out = Vec::new();
        write_object(
            &mut out,
            &Object::Stream(stream),
            ObjId::new(1, 0),
            b"short",
            &IdentityEncoder,
        );
        assert!(String::from_utf8_lossy(&out).contains("/Length 0"));
    }

    #[test]
    fn indirect_definition_frames_with_obj_and_endobj() {
        let mut out = Vec::new();
        write_indirect(
            &mut out,
            ObjId::new(12, 3),
            &Object::Integer(7),
            &[],
            &IdentityEncoder,
        );
        assert_eq!(String::from_utf8(out).unwrap(), "12 3 obj\n7\nendobj\n");
    }

    #[test]
    fn indirect_definition_reparses_through_the_document_parser() {
        let mut out = Vec::new();
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Catalog")));
        write_indirect(
            &mut out,
            ObjId::new(1, 0),
            &Object::Dict(d),
            &[],
            &IdentityEncoder,
        );
        let io = Parser::at(&out, 0)
            .parse_indirect_object(&mut |_| None)
            .unwrap();
        assert_eq!(io.id, ObjId::new(1, 0));
        assert!(io.value.as_dict().unwrap().contains_key(b"Type"));
    }
}
