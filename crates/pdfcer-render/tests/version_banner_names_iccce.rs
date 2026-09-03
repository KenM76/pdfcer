//! # The `--version` banner must not deny a dependency this crate links
//!
//! `Pass 223.0`. One test, guarding one class of defect that shipped for
//! six days and that nothing else in this workspace could see.
//!
//! ## What happened
//!
//! `Pass 199.2` added `iccce-profile` and `iccce-cmm` to this crate as git
//! dependencies pinned to tag `v0.3.0`. `pdfcer-core`'s build script went on
//! emitting the literal string `not-linked-yet`, so
//! `pdfcer --version` told the operator that pdfcer does not link `iccce`
//! **while linking it** — a false claim in the one output surface whose
//! entire purpose is to be believed without checking.
//!
//! It did not self-correct because the detector read
//! `DEP_ICCCE_PROVENANCE`, an environment variable Cargo sets only for a
//! dependency declaring a `links` key. `iccce` declares none. The detector
//! could not have fired however long it waited — and a detector that cannot
//! fire is indistinguishable, from outside, from a condition that has not
//! occurred.
//!
//! ## ★★ Why this test lives HERE, in `pdfcer-render`
//!
//! Because this is the only crate where **the compiler proves the
//! premise.**
//!
//! The hard part of testing "the banner should not say `not-linked`" is
//! establishing, independently, that `iccce` really is linked. A test that
//! reads `Cargo.lock` to decide would be checking the build script's answer
//! against the build script's own source of truth — circular, and it would
//! pass just as happily if both were wrong together.
//!
//! This test instead **names an `iccce` type in its own body**. If `iccce`
//! were not a dependency of this crate, the file would not compile. So by
//! the time the assertion runs, "iccce is linked into this build" is not an
//! assumption the test makes — it is a fact the compiler has already
//! enforced. The assertion then only has to check that the banner agrees.
//!
//! `pdfcer-core` could not host this: it does not depend on `iccce` and never
//! will, which is exactly the structural gap that let the defect live.
//! `pdfcer` could, but it reaches `iccce` transitively, so the proof
//! would be one link weaker.
//!
//! ## What it does NOT check
//!
//! The *content* of the provenance string — the version, the pin, the
//! revision, the commit date. Those are read from `Cargo.lock` and from
//! Cargo's git mirror, and asserting them here would pin this test to a
//! particular `iccce` release: it would go red on a routine dependency
//! bump, which is a test that trains people to edit it rather than read it.
//!
//! The claim under test is narrower and permanent: **the banner must never
//! deny a dependency that demonstrably exists.**

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::build::BuildInfo;

/// Would catch: `iccce_provenance()` returning a not-linked sentinel while
/// this crate links `iccce` — the `Pass 199.2` → `Pass 223.0` defect,
/// exactly.
///
/// Would also catch the subtler regression it invites: a future change that
/// breaks the `Cargo.lock` read (a moved lock file, a renamed key, a lock
/// format change) and silently falls back to the sentinel. That fallback is
/// correct for an out-of-workspace consumer and wrong here, and the two are
/// indistinguishable from inside the build script.
#[test]
fn the_build_banner_does_not_deny_the_iccce_this_crate_links() {
    // ★ The premise, enforced by the compiler rather than assumed: this
    // line does not typecheck unless `iccce` is a dependency of this crate.
    // Parsing four bytes of nonsense is expected to fail; the VALUE is
    // irrelevant and deliberately unused. What matters is that the symbol
    // resolves at all.
    let _proof_iccce_is_linked = iccce_profile::Profile::parse(&[0u8; 4]).is_ok();

    let iccce = BuildInfo::current().iccce;
    assert!(
        !iccce.is_empty(),
        "the provenance string is never empty; `not-linked` is a value, not an absence"
    );
    assert_ne!(
        iccce, "not-linked",
        "this crate links iccce -- the compiler just proved it above -- so the \
version banner must not report it as absent. See this file's module docs \
for the six-day defect this guards."
    );
    assert!(
        !iccce.contains("not-linked"),
        "no not-linked variant, including the historical `not-linked-yet`: {iccce}"
    );
    assert!(
        !iccce.starts_with("disagreement:"),
        "the iccce crates resolved to different versions or sources, which means \
two halves of one colour engine are at different revisions: {iccce}"
    );
}
