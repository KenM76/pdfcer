//! Build provenance — what this binary is, and when and from what it was
//! made (`Pass 101.0`).
//!
//! # The request this answers
//!
//! Ken, 2026-08-18: *"whenever you build a new version can you include the
//! build date and time, and also include the build revision, date, and time
//! for the version of iccce used in the version?"*
//!
//! Every value here is captured at **compile time** by
//! `crates/pdfcer-core/build.rs` and read with `env!`, so reading it costs
//! nothing and cannot fail at run time. Read that build script for the
//! capture rules; this module is the shape they are read back in.
//!
//! # Why it lives in `pdfcer-core` and not in each binary
//!
//! Because there must be exactly one answer. `pdfcer` and `pdfce-gui`
//! both depend on `pdfcer-core`, so both report the same stamp by
//! construction — whereas a build script per binary would let two shells
//! shipped in the same folder disagree about which build they are, which is
//! the one thing a provenance banner exists to prevent.
//!
//!
//! # `iccce` — reported from the resolved dependency graph
//!
//! pdfcer depends on `iccce` as of `Pass 199.2`: `iccce-profile` and
//! `iccce-cmm`, pinned in `crates/pdfcer-render` — to tag `v0.3.0` until
//! 2026-09-02, and to that tag's revision `a4d9003b` since (decision 123,
//! at iccce's own request). So [`BuildInfo::iccce`] carries a real answer —
//! version, pin, resolved revision, and when that revision was committed:
//!
//! ```text
//!   iccce:     0.3.0 (rev a4d9003b, committed 2026-09-01T08:54:36Z)
//! ```
//!
//! (Under the tag pin the same line read
//! `0.3.0 (tag v0.3.0, a4d9003b, committed …)`; a `rev` pin and its
//! resolved revision are one number, so it is printed once.)
//!
//! That is all four halves of what the operator asked for on 2026-08-18:
//! *"the build revision, date, and time for the version of iccce used"*.
//!
//! ## ★★ This field said `not-linked-yet` for six days after that stopped
//! being true
//!
//! The text here used to argue at length that the absence was **pending,
//! not architectural** — and that argument was right, and the operator had
//! already corrected an earlier version of it that called the absence a
//! settled boundary. What neither version anticipated is that the
//! *presence* would arrive and the string would not notice.
//!
//! `Pass 199.2` added the dependency. `Pass 223.0` fixed the stamp. In
//! between, `pdfcer --version` told the operator that pdfcer does not link
//! `iccce`, while linking it.
//!
//! The mechanism is in `build.rs`'s own docs and is worth reading once: the
//! detector waited on `DEP_ICCCE_PROVENANCE`, which Cargo only ever sets for
//! a dependency declaring a `links` key. `iccce` declares none, so the
//! detector could not have fired however long it waited — and a detector
//! that cannot fire is indistinguishable, from outside, from a condition
//! that has not occurred.
//!
//! ## Where the value comes from
//!
//! The workspace `Cargo.lock` — the *resolved* graph, i.e. the exact
//! revision this build compiles. Deliberately **not** the sibling checkout
//! at `D:\Dev\iccce`, which would answer *"which iccce is on this
//! machine"* while appearing to answer *"which iccce is in this binary"*.
//! Those coincide only by accident, and the accident is invisible to whoever
//! reads the banner later.
//!

/// Where this binary came from.
///
/// Obtain with [`BuildInfo::current`]; render with its [`std::fmt::Display`]
/// implementation, or read the fields for a structured report.
///
/// Every field is `&'static str` because every one is baked in at compile
/// time. Fields that could not be determined hold the literal `"unknown"`
/// rather than an empty string or a guess — see [`BuildInfo::is_complete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct BuildInfo {
    /// The crate version — `CARGO_PKG_VERSION`, e.g. `0.7.0`.
    pub version: &'static str,
    /// When this binary was built, RFC 3339 UTC, e.g.
    /// `2026-08-18T14:03:11Z`.
    ///
    /// Taken from `SOURCE_DATE_EPOCH` when the environment supplies one, so
    /// a reproducible-build system can pin it; otherwise the wall clock at
    /// compile time.
    pub built_at: &'static str,
    /// `git describe --tags --always --dirty` at build time, e.g.
    /// `v0.7.0-73-g6b797db` or `v0.7.0-73-g6b797db-dirty`.
    ///
    /// **The `-dirty` suffix is the load-bearing part.** A build made from
    /// an edited working tree is not the commit it names, and a banner that
    /// hid that would let an untracked change be mistaken for a released
    /// one — which is exactly the situation somebody reads a version string
    /// to rule out.
    pub revision: &'static str,
    /// The committer date of [`Self::revision`], RFC 3339 UTC.
    ///
    /// Distinct from [`Self::built_at`], and the pair is more useful than
    /// either alone: together they say *how stale the source was when this
    /// was built*. A binary built today from a commit six weeks old is a
    /// different situation from one built today from this morning's commit,
    /// and only the two timestamps side by side distinguish them.
    pub committed_at: &'static str,
    /// The `iccce` build linked into this one — its version, the pin the
    /// manifest asked for, the resolved git revision, and when that
    /// revision was committed. `not-linked` when there is none.
    ///
    /// Example: `0.3.0 (rev a4d9003b, committed 2026-09-01T08:54:36Z)` under
    /// a `rev` pin; `0.3.0 (tag v0.3.0, a4d9003b, committed …)` under a tag.
    ///
    /// ## ★ It reports what the WORKSPACE links, not what `pdfcer-core` links
    ///
    /// `iccce-profile` and `iccce-cmm` are dependencies of
    /// **`pdfcer-render`**, and this stamp is baked by `pdfcer-core`'s build
    /// script. Every shipped binary links `pdfcer-render`, so inside this
    /// workspace the two coincide. A project depending on `pdfcer-core`
    /// alone has no `iccce` in its graph and gets `not-linked`, which is
    /// the truth for that build.
    ///
    /// ## This field read `not-linked-yet` until `Pass 223.0`, wrongly
    ///
    /// `Pass 199.2` added the dependency; this went on saying the
    /// integration was pending for six days, in the one output surface
    /// whose entire purpose is to be believed without checking. The
    /// mechanism it waited on (`DEP_ICCCE_PROVENANCE`) is only ever set
    /// for a dependency declaring a `links` key, which `iccce` does not —
    /// so the detector could not have fired however long it waited. See
    /// `build.rs`'s `iccce_provenance`.
    pub iccce: &'static str,
}

impl BuildInfo {
    /// This binary's provenance.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            built_at: env!("PDFCER_BUILD_TIMESTAMP"),
            revision: env!("PDFCER_BUILD_REVISION"),
            committed_at: env!("PDFCER_BUILD_COMMIT_TIMESTAMP"),
            iccce: env!("PDFCER_ICCCE_PROVENANCE"),
        }
    }

    /// Whether every field was determined at build time.
    ///
    /// `false` when git was unavailable — a source tarball, a shallow clone,
    /// a machine without `git` on `PATH`. Exposed rather than hidden so a
    /// caller that *needs* provenance (a support report, a bug template) can
    /// say "this build cannot identify itself" instead of quietly printing
    /// the word `unknown` three times and hoping somebody notices.
    ///
    /// Note that [`Self::iccce`] is **not** part of this: `not-linked` is a
    /// determined answer, not a missing one, and so is a provenance string
    /// that omits the commit date because no repository could supply it.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.built_at != "unknown" && self.revision != "unknown" && self.committed_at != "unknown"
    }

    /// Whether this binary was built from a modified working tree.
    ///
    /// Derived from the `-dirty` suffix `git describe --dirty` appends. A
    /// separate accessor because it is the single most consequential fact in
    /// the stamp and reading it should not require string handling at the
    /// call site.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.revision.ends_with("-dirty")
    }
}

/// One line per fact, `key: value`, stable enough to paste into a bug
/// report and to grep out of one.
///
/// Deliberately not a single line: five facts on one line is a line nobody
/// reads to the end, and the two timestamps are only useful when they can be
/// compared at a glance.
impl std::fmt::Display for BuildInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "pdfcer {}", self.version)?;
        writeln!(f, "  built:     {}", self.built_at)?;
        writeln!(f, "  revision:  {}", self.revision)?;
        writeln!(f, "  committed: {}", self.committed_at)?;
        write!(f, "  iccce:     {}", self.iccce)?;
        if self.iccce == "not-linked" {
            // The operator asked for iccce's revision BY NAME, so an answer
            // of "there isn't one" has to say why -- otherwise it reads as a
            // defect in the stamp rather than as a fact about the build.
            //
            // ★ This branch is now the OUT-OF-WORKSPACE case, not the
            // not-yet-integrated one. Inside the workspace every binary
            // links iccce and this never fires; it fires for a project
            // depending on `pdfcer-core` alone, where the absence is real
            // and permanent rather than pending.
            write!(
                f,
                " (this build links pdfcer-core alone; iccce is pdfcer-render's dependency)"
            )?;
        }
        if self.is_dirty() {
            write!(
                f,
                "\n  NOTE: built from a MODIFIED working tree - this binary is not the commit it names"
            )?;
        }
        Ok(())
    }
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

    /// The stamp is populated at all — i.e. the build script ran and its
    /// variables reached the compiler.
    ///
    /// Worth asserting because the failure mode of a missing `build.rs` is
    /// not a compile error in the usual place: `env!` fails at compile time
    /// with a message about an environment variable, which reads like a
    /// toolchain problem rather than like a missing build script.
    #[test]
    fn the_stamp_is_populated() {
        let b = BuildInfo::current();
        assert!(!b.version.is_empty());
        assert!(!b.built_at.is_empty());
        assert!(!b.revision.is_empty());
        assert!(!b.committed_at.is_empty());
        assert!(!b.iccce.is_empty());
    }

    /// The build timestamp is a real RFC 3339 UTC instant, not a debug
    /// formatting of something.
    ///
    /// This is the assertion that catches the calendar arithmetic in
    /// `build.rs` going wrong — which it would do silently, on one day in
    /// four years, if the leap-day handling were the obvious version.
    #[test]
    fn the_build_timestamp_is_a_plausible_rfc3339_instant() {
        let t = BuildInfo::current().built_at;
        assert_eq!(t.len(), 20, "expected YYYY-MM-DDTHH:MM:SSZ, got {t:?}");
        assert!(t.ends_with('Z'), "{t:?} is not UTC-suffixed");
        let (date, time) = t[..19].split_at(10);
        let d: Vec<&str> = date.split('-').collect();
        let time: Vec<&str> = time[1..].split(':').collect();
        assert_eq!(d.len(), 3);
        assert_eq!(time.len(), 3);
        let year: i32 = d[0].parse().expect("year");
        let month: u32 = d[1].parse().expect("month");
        let day: u32 = d[2].parse().expect("day");
        // A lower bound that is a real event rather than a round number:
        // pdfcer did not exist before its founding conversation.
        assert!((2026..=2100).contains(&year), "year {year} out of range");
        assert!((1..=12).contains(&month), "month {month} out of range");
        assert!((1..=31).contains(&day), "day {day} out of range");
        for (i, part) in time.iter().enumerate() {
            let v: u32 = part.parse().expect("time component");
            let max = if i == 0 { 23 } else { 59 };
            assert!(v <= max, "time component {i} = {v} out of range");
        }
    }

    /// The rendered form names every fact, including the `iccce` line the
    /// operator asked for by name.
    #[test]
    fn the_rendered_stamp_names_every_fact() {
        let text = BuildInfo::current().to_string();
        for key in ["built:", "revision:", "committed:", "iccce:"] {
            assert!(text.contains(key), "{key:?} missing from:\n{text}");
        }
    }

    /// `is_dirty` reads the suffix, and the suffix is the whole point.
    #[test]
    fn a_dirty_revision_is_flagged() {
        let clean = BuildInfo {
            revision: "v0.7.0-73-g6b797db",
            ..BuildInfo::current()
        };
        let dirty = BuildInfo {
            revision: "v0.7.0-73-g6b797db-dirty",
            ..BuildInfo::current()
        };
        assert!(!clean.is_dirty());
        assert!(dirty.is_dirty());
        assert!(
            dirty.to_string().contains("MODIFIED working tree"),
            "a dirty build must SAY so in the rendered stamp, not only in a \
             predicate nobody calls"
        );
    }
}
