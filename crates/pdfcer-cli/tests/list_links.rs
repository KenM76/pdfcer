//! # `pdfcer list-links` — black-box over the real binary
//!
//! `Pass 222.0`. Covers the CLI shell for `pdfcer-core`'s
//! `annot::page_link_destinations` / `outline::DestinationReader`
//! (ISO 32000-1 §12.5.6.5 Table 173 → §12.3.2 destinations → §12.6.4
//! go-to actions).
//!
//! ## Why a shell test exists when the core is already covered
//!
//! `annot.rs`'s unit tests pin the resolution. They structurally cannot
//! pin any of the following, and every one of them is a way a correct
//! core ships behind a broken command:
//!
//! * **Flag wiring.** `--pages` can be declared, documented in `--help`,
//!   and never reach the core call. Unit tests hit the core directly and
//!   pass regardless; only running the binary finds it. This project has
//!   a standing lesson for exactly that shape.
//! * **The 0-based / 1-based boundary.** `Destination::Page::page_index`
//!   is 0-based by core convention and every `page=`/`target=` this CLI
//!   prints is 1-based. That conversion lives only in the shell, and an
//!   off-by-one in it sends a script to the wrong page while every core
//!   test stays green.
//! * **The five `dest=` tokens.** Collapsing `remote`, `named`,
//!   `unmapped` or `action` into one token — or into silence — turns a
//!   document full of disclosable links into a document that reports
//!   nothing. The tokens *are* the disclosure, and only the shell emits
//!   them.
//! * **The summary line.** It must print even when both counts are zero,
//!   so "no links here" stays distinguishable from "this tool did not
//!   look". A core test cannot observe a line that was never printed.
//! * **Exit codes**, the CLI's contract with a script.
//!
//! ## Fixtures
//!
//! `fixtures/synthetic/links/`, provenance in that directory's
//! `PROVENANCE.md`. Every one is multi-page and **no link targets page
//! 1**, so an implementation that resolved nothing and returned a
//! defaulted zero fails rather than passes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::RUNTIME_ERROR` — a bad invocation the command itself rejected.
const RUNTIME_ERROR: i32 = 1;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/synthetic/links/{name}"))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the CLI binary runs")
}

/// Stdout as text, with the exit code asserted to be success first — a
/// command that failed and printed nothing would otherwise "pass" every
/// `assert!(!contains(...))` below.
fn stdout_ok(args: &[&str]) -> String {
    let out = run(args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "expected success; stderr was: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

/// Would catch: a `/GoTo` action's destination not being resolved at all
/// — the gap this Pass exists to close, where `list-annotations` could
/// say `action=GoTo` and nothing could say where.
///
/// The `target=` values are asserted **by value**, and none of them is
/// `1`. An implementation returning a defaulted page index fails here.
#[test]
fn goto_actions_resolve_to_the_right_page_and_view() {
    let path = fixture("goto-actions.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap()]);

    assert!(
        out.contains("link page=1 index=0 rect=36,700,200,730 dest=page target=2 view=Fit\n"),
        "{out}"
    );
    assert!(
        out.contains("index=1 rect=36,660,200,690 dest=page target=3 view=XYZ"),
        "{out}"
    );
    assert!(
        out.contains("index=2 rect=36,620,200,650 dest=page target=4 view=FitH"),
        "{out}"
    );
    assert!(
        out.contains("index=3 rect=36,580,200,610 dest=page target=2 view=FitR"),
        "{out}"
    );
    assert!(
        out.contains("links resolved=4 links-without-destination=0"),
        "{out}"
    );
}

/// Would catch: `--pages` being parsed and ignored, which no core test
/// can see. Page 2 of this fixture carries no annotations, so a shell
/// that dropped the selection would print the four page-1 links here.
#[test]
fn the_pages_flag_actually_reaches_the_page_loop() {
    let path = fixture("goto-actions.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap(), "--pages", "2"]);

    assert!(
        !out.contains("link page="),
        "no links live on page 2: {out}"
    );
    assert!(
        out.contains("links resolved=0 links-without-destination=0"),
        "{out}"
    );

    // …and the flag is not merely suppressing everything: asking for
    // page 1 explicitly brings them all back.
    let page1 = stdout_ok(&["list-links", path.to_str().unwrap(), "--pages", "1"]);
    assert!(page1.contains("links resolved=4"), "{page1}");
}

/// Would catch: the name-tree and legacy-dictionary namespaces not being
/// searched for a LINK, and an undefined name being dropped instead of
/// disclosed.
#[test]
fn named_destinations_resolve_through_both_namespaces() {
    let path = fixture("named-links.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap()]);

    // PDF 1.2 name tree, via a /GoTo action's /D byte string.
    assert!(
        out.contains("index=0 rect=36,700,200,730 dest=page target=2"),
        "{out}"
    );
    // PDF 1.1 catalog /Dests, via a direct /Dest name object.
    assert!(
        out.contains("index=1 rect=36,660,200,690 dest=page target=3"),
        "{out}"
    );
    // Defined by neither: kept and named, never silently dropped.
    assert!(
        out.contains(r#"index=2 rect=36,620,200,650 dest=named name="absent-target""#),
        "{out}"
    );
    assert!(
        out.contains("links resolved=3 links-without-destination=0"),
        "{out}"
    );
}

/// Would catch: a link left behind by a page delete being reported as a
/// working jump to page 1, and a link that can do nothing being dropped
/// so that a page of wholly-broken links looks like a page with none.
#[test]
fn broken_links_are_disclosed_and_the_dead_one_is_counted() {
    let path = fixture("broken-links.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap()]);

    assert!(
        out.contains("index=0 rect=36,700,200,730 dest=unmapped"),
        "{out}"
    );
    assert!(
        out.contains("index=1 rect=36,660,200,690 dest=unmapped"),
        "{out}"
    );
    assert!(
        !out.contains("index=2"),
        "the link with neither /Dest nor /A has no destination line: {out}"
    );
    assert!(
        !out.contains("dest=page target=1"),
        "an unresolvable destination must never become page 1: {out}"
    );

    // Table 173 forbids both keys; /Dest wins, and the two point at
    // DIFFERENT pages so this is observable rather than a coin flip.
    assert!(
        out.contains("index=3 rect=36,580,200,610 dest=page target=3"),
        "/Dest (page 3) must beat /A (page 2): {out}"
    );
    assert!(
        out.contains("links resolved=3 links-without-destination=1"),
        "{out}"
    );
}

/// Would catch: a `/URI` or `/JavaScript` link being reported as a page
/// jump or as nothing, and — the expensive one — a `/GoToR`'s
/// destination name being resolved against THIS document's name tree.
///
/// The fixture defines the remote link's name locally, pointing at page
/// 3, precisely so that a local resolution produces a confident wrong
/// answer instead of an obvious failure.
#[test]
fn non_navigation_actions_are_disclosed_and_a_remote_name_stays_remote() {
    let path = fixture("non-navigation-links.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap()]);

    assert!(
        out.contains("index=0 rect=36,700,200,730 dest=action action=URI"),
        "{out}"
    );
    assert!(
        out.contains("index=1 rect=36,660,200,690 dest=action action=JavaScript"),
        "{out}"
    );
    assert!(
        out.contains("index=2 rect=36,620,200,650 dest=action action=Launch"),
        "{out}"
    );
    assert!(
        out.contains(r#"index=3 rect=36,580,200,610 dest=remote file="other.pdf""#),
        "{out}"
    );
    assert!(
        out.contains("target=name:tree-target window=new"),
        "the remote name is carried verbatim, not resolved: {out}"
    );
    assert!(
        !out.contains("dest=page target=3"),
        "the local name tree defines this name; resolving it here would be \
a confident wrong answer: {out}"
    );
    assert!(
        out.contains("links resolved=4 links-without-destination=0"),
        "{out}"
    );
}

/// Would catch: the summary line being printed only when something was
/// found, which makes "no links here" and "this tool did not look"
/// indistinguishable to a script.
///
/// Sabotage note: making the summary conditional on `resolved > 0` fails
/// this test and nothing else in the suite — which is exactly why it is
/// its own test rather than an extra assertion elsewhere.
#[test]
fn a_document_with_no_links_still_prints_the_summary() {
    let path = fixture("no-links.pdf");
    let out = stdout_ok(&["list-links", path.to_str().unwrap()]);

    assert!(!out.contains("link page="), "{out}");
    assert_eq!(
        out.trim(),
        "links resolved=0 links-without-destination=0",
        "the summary is the whole output, and it is not optional"
    );
}

/// Would catch: an unparseable `--pages` value being accepted and
/// silently treated as `all`, which would make a script that asked for
/// one page get the whole document and never know.
#[test]
fn a_bad_pages_selector_is_refused_rather_than_defaulted() {
    let path = fixture("goto-actions.pdf");
    let out = run(&["list-links", path.to_str().unwrap(), "--pages", "nonsense"]);

    assert_eq!(out.status.code(), Some(RUNTIME_ERROR));
    assert!(
        out.stdout.is_empty(),
        "a refused invocation prints no link lines: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Would catch: the subcommand shipping with an empty `--help`
/// description. In clap-derive a `///` **is** the operator-facing help
/// text, so a doc comment lost to a bad splice is shipped UI, not a
/// documentation lapse.
#[test]
fn the_subcommand_has_operator_facing_help() {
    let out = stdout_ok(&["list-links", "--help"]);
    assert!(
        out.contains("List every clickable link and where it goes"),
        "{out}"
    );
    assert!(
        out.contains("dest=page"),
        "the output contract is documented in --help: {out}"
    );
}
