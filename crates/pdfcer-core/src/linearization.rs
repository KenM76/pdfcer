//! # Linearization (Fast Web View) detection — ISO 32000-1 Annex F
//!
//! **Detection only.** pdfcer does not write linearized files, and this
//! module deliberately implements nothing that would be needed to:
//! no hint-table parsing (Annex F.4), no part-ordering validation
//! (F.3.5, F.3.7–F.3.10). Producing a linearized file needs all of
//! that; recognizing one, and stating honestly what a save does to it,
//! needs only Table F.1's parameter dictionary. Spec source:
//! `iso32000__annex__f.md` in the PDF-spec RAG.
//!
//! ## Why pdfcer detects something it cannot produce
//!
//! Decision 007 W5 and R36. An incremental save appends past the
//! first-page cross-reference table and the hint streams, so the
//! linearization is stale afterwards. F.1 is normative and blunt about
//! it: *"Incremental update shall still be permitted, but the resulting
//! PDF is **no longer linearized** and subsequently shall be treated as
//! ordinary PDF."*
//!
//! That is spec-sanctioned and unavoidable — but it is also an
//! observable property change the operator did not ask for (the file
//! opens more slowly over a network afterwards). `CLAUDE.md` rule 4,
//! "fuzzy, never sneaky", makes silently degrading it the wrong
//! behavior. So: detect on load, name it, warn on save.
//!
//! ## What this module refuses to do, and why
//!
//! - **It never strips a stale `/Linearized` dictionary.** Removing it
//!   would be a normalization (R33) and Annex G.7's reader-side
//!   revalidation path depends on the dictionary still being there.
//! - **It never patches `L`.** `L` is not the property — the object
//!   ordering and hint validity are. A file whose `L` was "fixed" after
//!   an append *claims* to be linearized while its hints point into a
//!   stale layout, which is strictly worse for a network reader than an
//!   honestly de-linearized file. (It is also impossible in an
//!   append-only save, since `L` lives at the front.)
//!
//! ## The detection recipe (F.3.3 + Table F.1)
//!
//! 1. The parameter dictionary *"shall be entirely contained within the
//!    first 1024 bytes of the PDF file"* — so the scan is bounded, and
//!    that bound exists precisely so a reader can decide cheaply.
//! 2. It is *"the first object in the body of the file"*, an indirect
//!    dictionary, and *"all values in this dictionary shall be direct
//!    objects"* — so detection needs no cross-reference table and can
//!    run before the xref is parsed at all.
//! 3. `/Linearized` present ⇒ **candidate**.
//! 4. `L` (required) *"shall be exactly equal to the actual length of
//!    the PDF file. A mismatch indicates that the file is not
//!    linearized and shall be treated as ordinary PDF."* This is the
//!    **liveness check**, and it is why step 3 alone is not enough: a
//!    stale `/Linearized` dictionary survives any append.

use crate::object::Object;
use crate::parser::Parser;

/// How far into the file the linearization parameter dictionary is
/// searched for.
///
/// F.3.3, verbatim: *"The linearization parameter dictionary shall be
/// entirely contained within the first 1024 bytes of the PDF file.
/// This limits the amount of data a conforming reader must read before
/// deciding whether the file is linearized."* The bound is the spec's,
/// not a pdfcer policy guard.
pub const LINEARIZATION_SCAN_WINDOW: usize = 1024;

/// What Annex F detection concluded about a loaded file.
///
/// Three states, not two, because "was linearized and then updated" is
/// operationally different from both "is linearized" and "never was":
/// it is the state in which a *previous* save already spent the Fast
/// Web View property, so pdfcer has nothing left to warn about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Linearization {
    /// No `/Linearized` parameter dictionary in the first
    /// [`LINEARIZATION_SCAN_WINDOW`] bytes.
    #[default]
    None,
    /// `/Linearized` present **and** `L` equals the actual file length:
    /// the hint tables are trustworthy and Fast Web View is live.
    ///
    /// This is the state a save must warn about (R36).
    Live {
        /// The declared file length (`L`), which equals the real one.
        declared_length: u64,
    },
    /// `/Linearized` present but `L` disagrees with the file length.
    ///
    /// Per F.1 the file *"shall be treated as ordinary PDF"*. The two
    /// sub-cases are distinguished because they mean different things:
    /// a file **longer** than `L` had an update appended (G.7's case);
    /// a file **shorter** than `L` is truncated, i.e. damaged.
    Stale {
        /// The `L` value the dictionary declares.
        declared_length: u64,
        /// The file's real length.
        actual_length: u64,
    },
}

impl Linearization {
    /// Whether a save would destroy a *live* Fast Web View property —
    /// i.e. whether the operator is owed a warning (R36).
    ///
    /// False for [`Linearization::Stale`]: the property was already
    /// spent by whoever appended the previous update, and warning about
    /// a loss that already happened is noise, not honesty.
    #[must_use]
    pub const fn save_invalidates_fast_web_view(self) -> bool {
        matches!(self, Self::Live { .. })
    }

    /// Whether the file carries a `/Linearized` dictionary at all,
    /// live or stale — the fact worth reporting as a diagnostic.
    #[must_use]
    pub const fn is_marked(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Detect Annex F linearization in `buf`.
///
/// Follows the F.3.3 recipe in the module docs. Never allocates beyond
/// the parsed dictionary, never touches the cross-reference table, and
/// never fails: every malformed shape is simply "not linearized",
/// because F.1 already prescribes exactly that outcome for a file whose
/// linearization information does not check out.
///
/// # Examples
///
/// ```
/// use pdfcer_core::linearization::{detect, Linearization};
///
/// // A file with no parameter dictionary is simply not linearized.
/// assert_eq!(detect(b"%PDF-1.7\n1 0 obj\n<</Type /Catalog>>\nendobj\n"),
///            Linearization::None);
/// ```
#[must_use]
pub fn detect(buf: &[u8]) -> Linearization {
    let window_len = buf.len().min(LINEARIZATION_SCAN_WINDOW);
    let window = buf.get(..window_len).unwrap_or(buf);

    let Some(obj_start) = find_first_object(window) else {
        return Linearization::None;
    };

    // F.3.3: all values are direct, so a null `/Length` resolver is not
    // merely adequate — resolving anything would be a bug, since no
    // cross-reference table has been read at this point.
    let Ok(io) = Parser::at(window, obj_start).parse_indirect_object(&mut |_| None) else {
        // "If parsing runs past 1024 bytes without a complete
        // dictionary → not linearized" (F.3.3's bound, enforced by the
        // window slice rather than by a byte count).
        return Linearization::None;
    };
    let Some(dict) = io.value.as_dict() else {
        return Linearization::None;
    };
    // Table F.1: `/Linearized` is required; "its mere presence is the
    // detection marker".
    if dict.get(b"Linearized").is_none() {
        return Linearization::None;
    }

    let actual_length = buf.len() as u64;
    // `L` is required and is the liveness check. A `/Linearized`
    // dictionary with no usable `L` cannot be validated, and F.1's
    // "treat as ordinary PDF" is the prescribed outcome — reported as
    // Stale with a declared length of 0 so the marker is still visible
    // to diagnostics.
    let declared = dict
        .get(b"L")
        .and_then(Object::as_int)
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(0);

    if declared == actual_length {
        Linearization::Live {
            declared_length: declared,
        }
    } else {
        Linearization::Stale {
            declared_length: declared,
            actual_length,
        }
    }
}

/// Find the offset of the first `N G obj` header in `window`.
///
/// F.3.3 puts the parameter dictionary at *"the first object in the
/// body of the file (part 2)"*, immediately after the header line and
/// its optional binary-comment line. Rather than model that layout,
/// this skips comment lines and blank lines and stops at the first line
/// that begins with a digit — which is the same thing, and tolerates
/// the leading-junk case pdfcer's header probe already allows.
fn find_first_object(window: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    while pos < window.len() {
        // Skip leading whitespace on the line.
        while matches!(window.get(pos), Some(b' ' | b'\t' | b'\r' | b'\n' | b'\0')) {
            pos += 1;
        }
        match window.get(pos) {
            None => return None,
            // A comment (`%PDF-…`, the binary marker line, or any
            // other) runs to the end of the line (§7.2.3).
            Some(b'%') => {
                while !matches!(window.get(pos), None | Some(b'\r' | b'\n')) {
                    pos += 1;
                }
            }
            Some(b) if b.is_ascii_digit() => return Some(pos),
            // Anything else at the start of a line is not the body's
            // first object; F.3.3's guarantee has already failed.
            Some(_) => return None,
        }
    }
    None
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

    /// Build a file whose linearization dictionary declares `l`, padded
    /// to `total` bytes so the real length is controllable.
    fn linearized_file(l: u64, total: usize) -> Vec<u8> {
        let mut buf =
            format!("%PDF-1.4\n%\u{80}\u{81}\u{82}\u{83}\n1 0 obj\n<< /Linearized 1.0 /L {l} /H [100 200] /O 5 /E 900 /N 3 /T 1000 >>\nendobj\n")
                .into_bytes();
        while buf.len() < total {
            buf.push(b' ');
        }
        buf
    }

    #[test]
    fn absent_dictionary_is_not_linearized() {
        assert_eq!(detect(b""), Linearization::None);
        assert_eq!(detect(b"%PDF-1.7\n"), Linearization::None);
        assert_eq!(
            detect(b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n"),
            Linearization::None
        );
    }

    #[test]
    fn live_linearization_requires_l_to_match_the_file_length() {
        // Table F.1: L "shall be exactly equal to the actual length of
        // the PDF file".
        let probe = linearized_file(0, 400);
        let total = probe.len() as u64;
        let buf = linearized_file(total, 400);
        assert_eq!(buf.len() as u64, total);
        assert_eq!(
            detect(&buf),
            Linearization::Live {
                declared_length: total
            }
        );
        assert!(detect(&buf).save_invalidates_fast_web_view());
        assert!(detect(&buf).is_marked());
    }

    #[test]
    fn appended_update_makes_the_marker_stale_not_absent() {
        // The G.7 case: file longer than L. Still *marked*, so the
        // diagnostic reports it, but no warning is owed — a previous
        // save already spent the property.
        let probe = linearized_file(0, 400);
        let mut buf = linearized_file(probe.len() as u64, 400);
        let declared = probe.len() as u64;
        buf.extend_from_slice(b"\n% appended update\n");
        let got = detect(&buf);
        assert!(
            matches!(got, Linearization::Stale { declared_length, .. } if declared_length == declared)
        );
        assert!(!got.save_invalidates_fast_web_view());
        assert!(got.is_marked());
    }

    #[test]
    fn truncated_file_is_stale_too() {
        let probe = linearized_file(0, 400);
        let buf = linearized_file(probe.len() as u64 + 5_000, 400);
        assert!(matches!(detect(&buf), Linearization::Stale { .. }));
    }

    #[test]
    fn dictionary_beyond_the_1024_byte_window_is_not_detected() {
        // F.3.3's bound is normative: a "linearization" dictionary that
        // does not fit in the first 1024 bytes is not one, and pdfcer
        // must not scan further looking for it.
        let mut buf = b"%PDF-1.4\n".to_vec();
        buf.extend(std::iter::repeat_n(b'%', 1_100));
        buf.push(b'\n');
        buf.extend_from_slice(b"1 0 obj\n<< /Linearized 1.0 /L 9 >>\nendobj\n");
        assert_eq!(detect(&buf), Linearization::None);
    }

    #[test]
    fn missing_l_is_marked_but_never_live() {
        // A dictionary that cannot be validated: F.1's "treat as
        // ordinary PDF" applies, but the marker is still reportable.
        let buf = b"%PDF-1.4\n1 0 obj\n<< /Linearized 1.0 /O 5 >>\nendobj\n".to_vec();
        let got = detect(&buf);
        assert!(matches!(
            got,
            Linearization::Stale {
                declared_length: 0,
                ..
            }
        ));
        assert!(!got.save_invalidates_fast_web_view());
    }

    #[test]
    fn first_object_scan_skips_the_binary_comment_line() {
        // The header's mandatory-if-binary comment line (§7.5.2 A5)
        // sits between the header and part 2; a scanner that stops at
        // the first `%` line finds nothing.
        let buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n1 0 obj\n<< /Linearized 1.0 /L 0 >>\nendobj\n";
        assert!(detect(buf).is_marked());
    }

    #[test]
    fn a_non_object_first_line_is_not_linearized() {
        assert_eq!(detect(b"%PDF-1.4\nxref\n0 1\n"), Linearization::None);
    }
}
