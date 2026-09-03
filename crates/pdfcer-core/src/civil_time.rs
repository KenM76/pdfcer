// Unix timestamp -> RFC 3339 UTC, shared by the crate and by its BUILD
// SCRIPT (`Pass 101.0`).
//
// # Why this is its own file, and `include!`d rather than imported
//
// `crates/pdfcer-core/build.rs` needs this function to stamp the build time,
// and a build script cannot depend on the crate it is building. The three
// ways out are: a date crate as a build dependency (compiled for the *host*
// on every clean build of every consumer), a second copy of the arithmetic
// in the build script (two implementations of a calendar, exactly the class
// of duplication this project treats as a defect), or this -- one file,
// `include!`d by the build script and `mod`-ed by the crate.
//
// The decisive argument for the third is TESTABILITY. A build script's own
// `#[cfg(test)]` module is never run by `cargo test`, so arithmetic living
// only in `build.rs` is arithmetic nobody can assert. Here it is covered by
// the tests at the bottom of this file, which run on every `cargo test` like
// everything else. (Those tests are `#[cfg(test)]`, so the build script,
// which is never compiled in test configuration, simply does not see them.)
//
// The comments are `//` rather than `//!` for a mechanical reason worth
// recording: an `include!`d file is spliced into the middle of another item
// list, and an inner doc comment cannot appear there. The rationale lives
// here regardless; only its rustdoc status differs.
//
// # The one real subtlety
//
// The day-number -> civil-date conversion is Howard Hinnant's
// `civil_from_days`, which shifts the year to start in MARCH so the leap day
// becomes the *last* day of the year. That removes February's special case
// from the arithmetic entirely.
//
// It matters because the obvious implementation -- walk the months,
// subtracting their lengths, with an `if leap { 29 }` for February -- is
// wrong in ways that surface on ONE DAY EVERY FOUR YEARS and are invisible
// in review. The tests below pin 29 February 2024, 29 February 2000 (a leap
// year *because* of the 400-rule) and 1 March 2100 (not a leap year,
// because of the 100-rule): the three cases a hand-rolled version gets
// wrong in three different ways.

/// A Unix timestamp as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Always UTC, and there is no local-time variant on purpose. The build
/// stamp prints two instants side by side — build time and commit time — so
/// a reader can see at a glance how stale the source was when the binary was
/// made. Two instants in different zones cannot be compared at a glance,
/// only carefully, which in practice means not at all.
///
/// Timestamps before 1970 format correctly: the divisions are Euclidean, so
/// the time-of-day never comes out negative.
// NO LONGER DEAD IN THE CRATE (Pass 119.0) -- and the correction is left
// visible rather than silently deleted, because the arrangement it describes
// is still the reason this file exists. `build.rs` `include!`s it and calls
// this function for the build stamp; the crate `mod`-s it so `cargo test` can
// assert the arithmetic. What changed is that `text_edit::edit` now needs a
// wall-clock timestamp in ISO 32000-1 7.9.4 form -- to bump a form XObject's
// `/LastModified` when the form carries `/PieceInfo`, so another application's
// cached private data does not silently outlive the content it describes
// (14.5's staleness protocol is an equality comparison). It reformats this
// function's output rather than growing a second calendar, which is the whole
// point of the file.
//
// The `allow` stays: the build script compiles this file WITHOUT the crate
// around it, so from its point of view the function still has no in-tree
// caller, and dropping the attribute would warn there.
#[allow(dead_code)]
#[allow(clippy::many_single_char_names)]
#[must_use]
pub(crate) fn format_rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Howard Hinnant's `civil_from_days`, days since 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod civil_time_tests {
    use super::format_rfc3339_utc;

    /// The instants a hand-rolled calendar gets wrong, and the ones a
    /// correct one must still get right.
    ///
    /// Every expected value was cross-checked against an independent
    /// implementation (Python's `datetime`) rather than derived by the same
    /// reasoning as the code under test — a table computed from the
    /// algorithm it is testing proves only that the algorithm is
    /// self-consistent.
    #[test]
    fn known_instants_round_trip() {
        for (secs, want) in [
            // The epoch itself.
            (0_i64, "1970-01-01T00:00:00Z"),
            // ★ A leap day. The whole reason for the March-shifted algorithm.
            (1_709_208_000, "2024-02-29T12:00:00Z"),
            // The day after it, where an off-by-one in the leap handling
            // lands instead of on the leap day itself.
            (1_709_251_200, "2024-03-01T00:00:00Z"),
            // ★ 2000 IS a leap year -- divisible by 400. A naive
            // "divisible by 100 is not a leap year" rule fails here.
            (951_868_799, "2000-02-29T23:59:59Z"),
            // ★ 2100 is NOT a leap year -- divisible by 100, not by 400.
            // A naive "divisible by 4" rule fails here, and this is the
            // case that will not reproduce for seventy-four years.
            (4_107_542_400, "2100-03-01T00:00:00Z"),
            // A year boundary, one second before midnight.
            (1_798_761_599, "2026-12-31T23:59:59Z"),
        ] {
            assert_eq!(format_rfc3339_utc(secs), want, "for {secs}");
        }
    }

    /// Before the epoch, the time of day must still be a time of day.
    ///
    /// This is what the Euclidean division buys. With truncating division
    /// the remainder goes negative for negative inputs and the formatter
    /// emits something like `T-1:-30:-0`, which is not a plausible-looking
    /// wrong answer — it is an obviously broken one, but only if somebody
    /// ever feeds it a pre-1970 instant, which nothing in pdfcer does today.
    /// Pinned so that stays true by test rather than by luck.
    #[test]
    fn a_pre_epoch_instant_still_formats() {
        assert_eq!(format_rfc3339_utc(-1), "1969-12-31T23:59:59Z");
        assert_eq!(format_rfc3339_utc(-86_400), "1969-12-31T00:00:00Z");
    }

    /// The shape is fixed-width, which is what makes the stamp greppable.
    #[test]
    fn the_shape_is_always_twenty_characters() {
        for secs in [0_i64, 1_709_208_000, 4_107_542_400, -1] {
            let s = format_rfc3339_utc(secs);
            assert_eq!(s.len(), 20, "{s:?}");
            assert!(s.ends_with('Z'));
        }
    }
}
