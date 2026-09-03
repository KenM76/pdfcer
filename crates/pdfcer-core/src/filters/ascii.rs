//! # ASCIIHexDecode and ASCII85Decode (ISO 32000-1 §7.4.2, §7.4.3)
//!
//! The two *ASCII-armouring* filters. Spec source: `filter__ascii.md` in
//! the PDF-spec RAG (which quotes §7.4.2/§7.4.3 verbatim); clause
//! numbers are ISO 32000-1:2008.
//!
//! ## Why these two land together, and why they land early
//!
//! Neither compresses anything — they exist so binary data can survive a
//! 7-bit transport — so a filter-coverage roadmap sorted by "how much
//! does this unlock" would normally rank them last. They are here first
//! for a structural reason instead: **they are the only two filters that
//! make an inline image (`BI`/`ID`/`EI`, §8.9.7) safely delimitable.**
//!
//! An inline image carries no `/Length`. §8.9.7's own analysis (see
//! `iso32000__s__8.9.7.md`) gives exactly three sound ways to find the
//! end of the data, and two of them are these filters' self-terminating
//! EOD markers (`>` for hex, `~>` for base-85). `pdfcer_core::content`
//! already relies on that when it *locates* the data; without decoders
//! the located bytes could not be turned into pixels. Corpus evidence
//! (Pass 1.1's 2,914-file run) is that inline images in the wild
//! overwhelmingly use `AHx`/`A85` for precisely this reason.
//!
//! ## Shared contracts
//!
//! - **Neither filter takes parameters** (Table 6). `/DecodeParms` for
//!   these positions is ignored, not validated.
//! - **All white-space characters are ignored** (§7.2 Table 1's set,
//!   NUL included) — everywhere, including *inside* the `~>` EOD
//!   sequence, which is why the base-85 EOD scan is a small state
//!   machine and not a two-byte `windows(2)` compare.
//! - **Any other character is an error** (both clauses say so
//!   verbatim). Fail-clean per the module docs of
//!   [`crate::filters`]: a corrupt stream returns `Err`, never
//!   plausible-looking partial output.
//! - **A missing EOD is tolerated** with an implicit end-of-data. The
//!   spec defines no behaviour for it; real producers omit it; refusing
//!   would fail files every other reader renders. This is the single
//!   documented divergence from strictness in this module, and it can
//!   only ever *add* data that was physically present in the stream.
//! - Both enforce [`MAX_DECODED_LEN`] incrementally (ARCHITECTURE.md
//!   §10.1). Neither can expand data (hex is 2:1 *shrinkage*, base-85
//!   is 5:4), so the guard is belt-and-braces rather than a real
//!   bomb defence — but the ceiling is checked in the loop anyway so
//!   that a hostile *multi-gigabyte* armoured stream aborts early
//!   instead of allocating its whole output first.

use super::{FilterError, MAX_DECODED_LEN};

/// Filter name used in [`FilterError`] payloads for the hex filter.
const HEX: &str = "ASCIIHexDecode";
/// Filter name used in [`FilterError`] payloads for the base-85 filter.
const A85: &str = "ASCII85Decode";

/// Is `b` one of §7.2 Table 1's white-space characters?
///
/// Duplicated rather than reaching for [`crate::lexer::is_whitespace`]
/// only in the sense that it *delegates* — the set is defined once, in
/// the lexer, and both filters must agree with it exactly or a stream
/// that lexes will fail to decode (or vice versa).
fn is_ws(b: u8) -> bool {
    crate::lexer::is_whitespace(b)
}

/// Decode an `ASCIIHexDecode` stream (§7.4.2).
///
/// "Produces one byte of binary data for each pair of ASCII hexadecimal
/// digits (`0`–`9`, `A`–`F`, `a`–`f`). All white-space characters shall
/// be ignored. A GREATER-THAN SIGN (`>`) indicates EOD. Any other
/// characters shall cause an error. If the filter encounters the EOD
/// marker after reading an **odd** number of hexadecimal digits, it
/// shall behave as if a `0` (zero) followed the last digit."
///
/// The odd-digit rule applies **only at EOD** — it is not a general
/// "pad whatever you have" rule — which is why the pending nibble is
/// flushed in exactly two places (the `>` arm and the implicit
/// end-of-data at the bottom) and nowhere else.
///
/// # Errors
///
/// - [`FilterError::Corrupt`] — a byte that is neither a hex digit, nor
///   white space, nor the EOD marker.
/// - [`FilterError::OutputTooLarge`] — output crossed
///   [`MAX_DECODED_LEN`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::filters::ascii::decode_hex;
///
/// assert_eq!(decode_hex(b"48656C6C6F>").unwrap(), b"Hello");
/// // White space anywhere is ignored; an odd final digit implies a 0.
/// assert_eq!(decode_hex(b"41 4\n>").unwrap(), b"A\x40");
/// ```
pub fn decode_hex(data: &[u8]) -> Result<Vec<u8>, FilterError> {
    // 2:1 shrinkage is exact, so this allocation is never wasteful and
    // never needs to grow.
    let mut out = Vec::with_capacity(data.len() / 2 + 1);
    // The high nibble of a byte still being assembled, if any.
    let mut pending: Option<u8> = None;

    for &b in data {
        if is_ws(b) {
            continue;
        }
        if b == b'>' {
            // EOD. Flush a half-assembled byte as if a `0` followed.
            if let Some(hi) = pending {
                out.push(hi << 4);
            }
            return Ok(out);
        }
        let Some(nibble) = hex_value(b) else {
            return Err(FilterError::Corrupt {
                filter: HEX,
                detail: format!("byte {b:#04x} is not a hex digit, white space, or the EOD '>'"),
            });
        };
        match pending {
            None => pending = Some(nibble),
            Some(hi) => {
                pending = None;
                out.push((hi << 4) | nibble);
                if out.len() > MAX_DECODED_LEN {
                    return Err(FilterError::OutputTooLarge);
                }
            }
        }
    }

    // No EOD: tolerated (module docs). The odd-digit rule still applies
    // — an implicit EOD is still an EOD.
    if let Some(hi) = pending {
        out.push(hi << 4);
    }
    Ok(out)
}

/// The numeric value of an ASCII hex digit, or `None`.
const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode an `ASCII85Decode` stream (§7.4.3).
///
/// ## The encoding relation, and therefore the decoding one
///
/// "Each group of 4 binary input bytes (b1 b2 b3 b4) shall be converted
/// to a group of 5 output bytes (c1 … c5) using the relation
/// `(b1×256³)+(b2×256²)+(b3×256)+b4 = (c1×85⁴)+(c2×85³)+(c3×85²)+(c4×85)+c5`",
/// with 33 (`!`) added to each base-85 digit. Decoding is the exact
/// inverse: accumulate five digits into a `u32`, emit four big-endian
/// bytes.
///
/// ## The three rules that are not just "the inverse"
///
/// 1. **`z` is a whole group of four zero bytes** — and is legal *only*
///    at a group boundary. `!z!!!` is one of §7.4.3's three named
///    "impossible combinations."
/// 2. **A partial final group is truncated, not padded on output.**
///    The encoder appended `4 − n` zero bytes and then wrote only the
///    first `n + 1` characters. The decoder therefore pads the group
///    with **`u`** (digit 84, the maximum) to five characters, decodes,
///    and keeps the first `m − 1` bytes where `m` is the count of real
///    characters. Padding with `u` rather than `!` is what makes the
///    kept bytes exact: `!` would round the truncated tail *down* and
///    could borrow out of the last kept byte. **This decode-side detail
///    is derived, not quoted** — §7.4.3 states only the encode
///    direction (`filter__ascii.md` flags it as such).
/// 3. **A final partial group of exactly one character is an error** —
///    the second named impossible combination. (Zero characters is
///    fine: the input length was a multiple of 4.)
///
/// The third impossible combination — a five-digit group whose value
/// exceeds 2³² − 1 — is checked with a widening accumulate, because it
/// is *not* vacuous: `uuuuu` is 85⁵ − 1 ≈ 4.44 × 10⁹.
///
/// ## Tolerated non-conformance
///
/// A leading `<~` (the Adobe/PostScript convention; ISO 32000-1
/// specifies **no** prefix) is skipped. A missing `~>` EOD is treated as
/// an implicit end-of-data. Both are repair heuristics, both are
/// documented in `filter__ascii.md`'s gotchas, and neither can
/// misinterpret conforming data.
///
/// # Errors
///
/// - [`FilterError::Corrupt`] — a character outside `!`–`u`/`z`/white
///   space, a `z` inside a group, a group value over 2³² − 1, a
///   one-character final group, or a `~` not followed by `>`.
/// - [`FilterError::OutputTooLarge`] — output crossed
///   [`MAX_DECODED_LEN`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::filters::ascii::decode_85;
///
/// // §7.4.3's own arithmetic, on the classic four-byte group.
/// assert_eq!(decode_85(b"9jqo^~>").unwrap(), b"Man ");
/// // `z` is four zero bytes.
/// assert_eq!(decode_85(b"z~>").unwrap(), &[0, 0, 0, 0]);
/// // A two-byte tail encodes to three characters.
/// assert_eq!(decode_85(b"9jn~>").unwrap(), b"Ma");
/// ```
pub fn decode_85(data: &[u8]) -> Result<Vec<u8>, FilterError> {
    // 4:5 shrinkage; the +4 covers a partial final group.
    let mut out = Vec::with_capacity(data.len() / 5 * 4 + 4);
    // Digits of the group being assembled (at most 5).
    let mut group = [0u8; 5];
    let mut n = 0usize;

    // Tolerated `<~` prefix (module docs). Only stripped at the very
    // start, and only as the exact two-byte sequence.
    let mut rest = data;
    if let Some(after) = rest.strip_prefix(b"<~") {
        rest = after;
    }

    let mut iter = rest.iter().copied().enumerate();
    while let Some((i, b)) = iter.next() {
        if is_ws(b) {
            continue;
        }
        match b {
            b'~' => {
                // EOD candidate: `~>` with white space permitted between
                // (§7.4.3's "shall ignore all white-space characters"
                // applies to the EOD too — a strict two-byte compare
                // misses `~` LF `>`).
                let next = iter.by_ref().map(|(_, c)| c).find(|&c| !is_ws(c));
                if next != Some(b'>') {
                    return Err(FilterError::Corrupt {
                        filter: A85,
                        detail: format!("'~' at offset {i} is not followed by '>'"),
                    });
                }
                flush_group(&mut out, &group, n)?;
                return Ok(out);
            }
            b'z' => {
                // Rule 1: legal only at a group boundary.
                if n != 0 {
                    return Err(FilterError::Corrupt {
                        filter: A85,
                        detail: format!("'z' at offset {i} occurs in the middle of a group"),
                    });
                }
                out.extend_from_slice(&[0, 0, 0, 0]);
                if out.len() > MAX_DECODED_LEN {
                    return Err(FilterError::OutputTooLarge);
                }
            }
            b'!'..=b'u' => {
                // Safe: `n < 5` is the loop invariant restored below.
                if let Some(slot) = group.get_mut(n) {
                    *slot = b - b'!';
                }
                n += 1;
                if n == 5 {
                    flush_group(&mut out, &group, n)?;
                    n = 0;
                    if out.len() > MAX_DECODED_LEN {
                        return Err(FilterError::OutputTooLarge);
                    }
                }
            }
            other => {
                return Err(FilterError::Corrupt {
                    filter: A85,
                    detail: format!(
                        "byte {other:#04x} at offset {i} is outside the base-85 alphabet"
                    ),
                });
            }
        }
    }

    // No `~>`: implicit end-of-data (module docs).
    flush_group(&mut out, &group, n)?;
    Ok(out)
}

/// Emit the bytes for a group of `n` base-85 digits (`0 ≤ n ≤ 5`).
///
/// `n == 0` emits nothing; `n == 5` emits four bytes; `2 ≤ n ≤ 4` is the
/// partial-final-group case (pad with `u`, keep `n − 1` bytes);
/// `n == 1` is §7.4.3's named impossible combination.
fn flush_group(out: &mut Vec<u8>, group: &[u8; 5], n: usize) -> Result<(), FilterError> {
    if n == 0 {
        return Ok(());
    }
    if n == 1 {
        return Err(FilterError::Corrupt {
            filter: A85,
            detail: "final partial group contains only one character".into(),
        });
    }
    // Widen to u64 so the >2³²−1 overflow is detectable rather than
    // wrapping (`uuuuu` = 85⁵ − 1 really does exceed u32).
    let mut value: u64 = 0;
    for slot in 0..5 {
        // Pad the partial group with `u` (digit 84) — see the fn docs
        // on why `u` and not `!`.
        let digit = if slot < n {
            u64::from(group.get(slot).copied().unwrap_or(0))
        } else {
            84
        };
        value = value * 85 + digit;
    }
    if value > u64::from(u32::MAX) {
        return Err(FilterError::Corrupt {
            filter: A85,
            detail: "group value exceeds 2^32 - 1".into(),
        });
    }
    // `value` is now known to fit; the cast is exact.
    let bytes = (value as u32).to_be_bytes();
    // A full group keeps 4 bytes; a partial group of n characters keeps
    // n − 1.
    let keep = if n == 5 { 4 } else { n - 1 };
    out.extend_from_slice(bytes.get(..keep).unwrap_or(&bytes));
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // ---- ASCIIHexDecode (§7.4.2) ----

    #[test]
    fn hex_decodes_pairs() {
        assert_eq!(decode_hex(b"48656C6C6F>").unwrap(), b"Hello");
        // Case-insensitive.
        assert_eq!(decode_hex(b"deadBEEF>").unwrap(), [0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn hex_ignores_all_whitespace_including_nul() {
        // §7.2 Table 1's set: NUL, HT, LF, FF, CR, SP.
        let input = b"4\x00 8\t6\n5\r6\x0CC>";
        assert_eq!(decode_hex(input).unwrap(), b"Hel");
    }

    #[test]
    fn hex_odd_digit_count_at_eod_implies_trailing_zero() {
        // "shall behave as if a 0 followed the last digit"
        assert_eq!(decode_hex(b"41 4>").unwrap(), b"A\x40");
        assert_eq!(decode_hex(b"F>").unwrap(), [0xF0]);
    }

    #[test]
    fn hex_stops_at_eod_and_ignores_the_tail() {
        // The `EI` that follows an inline image's data must not decode.
        assert_eq!(decode_hex(b"FF> EI garbage").unwrap(), [0xFF]);
    }

    #[test]
    fn hex_missing_eod_is_tolerated() {
        // Module docs: implicit end-of-data, not a refusal.
        assert_eq!(decode_hex(b"0102").unwrap(), [0x01, 0x02]);
    }

    #[test]
    fn hex_rejects_a_non_hex_byte() {
        let e = decode_hex(b"41G2>").unwrap_err();
        assert!(
            matches!(
                e,
                FilterError::Corrupt {
                    filter: "ASCIIHexDecode",
                    ..
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn hex_empty_is_empty() {
        assert_eq!(decode_hex(b">").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_hex(b"").unwrap(), Vec::<u8>::new());
    }

    // ---- ASCII85Decode (§7.4.3) ----

    #[test]
    fn a85_decodes_the_spec_relation() {
        // 77·256³ + 97·256² + 110·256 + 32 = 1 298 230 816
        //   = 24·85⁴ + 73·85³ + 80·85² + 78·85 + 61
        //   → '9' 'j' 'q' 'o' '^'
        assert_eq!(decode_85(b"9jqo^~>").unwrap(), b"Man ");
    }

    #[test]
    fn a85_z_is_four_zero_bytes() {
        assert_eq!(decode_85(b"z~>").unwrap(), [0, 0, 0, 0]);
        assert_eq!(decode_85(b"zz~>").unwrap(), [0u8; 8]);
        // …and mixes with real groups.
        assert_eq!(decode_85(b"z9jqo^~>").unwrap(), b"\0\0\0\0Man ");
    }

    #[test]
    fn a85_z_inside_a_group_is_an_impossible_combination() {
        let e = decode_85(b"!z!!!~>").unwrap_err();
        assert!(
            matches!(
                e,
                FilterError::Corrupt {
                    filter: "ASCII85Decode",
                    ..
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a85_partial_final_group_truncates() {
        // 2 real bytes → 3 characters; 3 real bytes → 4 characters.
        assert_eq!(decode_85(b"9jn~>").unwrap(), b"Ma");
        assert_eq!(decode_85(b"9jqo~>").unwrap(), b"Man");
        // Round-trip sanity across every tail length.
        assert_eq!(decode_85(b"9jqo^9jn~>").unwrap(), b"Man Ma");
    }

    #[test]
    fn a85_one_character_final_group_is_an_error() {
        let e = decode_85(b"9jqo^!~>").unwrap_err();
        assert!(matches!(e, FilterError::Corrupt { .. }), "{e:?}");
    }

    #[test]
    fn a85_eod_may_be_split_by_whitespace() {
        // "shall ignore all white-space characters" applies to `~>` too.
        assert_eq!(decode_85(b"9jqo^~\n>").unwrap(), b"Man ");
        assert_eq!(decode_85(b"9j\nqo\r\n^~>").unwrap(), b"Man ");
    }

    #[test]
    fn a85_tolerates_the_nonconformant_leading_prefix() {
        // `<~` is the Adobe/PostScript convention, not ISO 32000-1.
        assert_eq!(decode_85(b"<~9jqo^~>").unwrap(), b"Man ");
        // …and its absence is the conforming form.
        assert_eq!(decode_85(b"9jqo^~>").unwrap(), b"Man ");
    }

    #[test]
    fn a85_missing_eod_is_tolerated() {
        assert_eq!(decode_85(b"9jqo^").unwrap(), b"Man ");
    }

    #[test]
    fn a85_group_over_u32_max_is_rejected() {
        // 'u' is digit 84; uuuuu = 85⁵ − 1 ≈ 4.44e9 > 2³² − 1.
        let e = decode_85(b"uuuuu~>").unwrap_err();
        assert!(matches!(e, FilterError::Corrupt { .. }), "{e:?}");
        // …while the largest legal group, s8W-!, is 2³² − 1.
        assert_eq!(decode_85(b"s8W-!~>").unwrap(), [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn a85_rejects_a_byte_outside_the_alphabet() {
        let e = decode_85(b"9jq{^~>").unwrap_err();
        assert!(matches!(e, FilterError::Corrupt { .. }), "{e:?}");
    }

    #[test]
    fn a85_lone_tilde_is_an_error() {
        let e = decode_85(b"9jqo^~").unwrap_err();
        assert!(matches!(e, FilterError::Corrupt { .. }), "{e:?}");
    }

    #[test]
    fn a85_empty_is_empty() {
        assert_eq!(decode_85(b"~>").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_85(b"").unwrap(), Vec::<u8>::new());
    }
}
