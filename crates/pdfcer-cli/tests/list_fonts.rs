//! # `pdfcer list-fonts` integration tests
//!
//! Black-box tests over the **real binary** for the font inventory that
//! Pass 67.0 phase A ships. `pdfcer_core::fontinfo`'s own unit tests pin the
//! classification; these pin the *contract the CLI publishes* — the stable
//! stdout line shape a batch sweep parses, and the stderr disclosures that
//! make a refusal actionable.
//!
//! ## What these tests are protecting
//!
//! 1. **The line shape.** `font …` rows and the `list-fonts <path> …`
//!    summary are a machine-readable interface. A field that silently
//!    changes name breaks every script reading it, and the value of a
//!    corpus sweep depends on those tokens staying put.
//!
//! 2. **The disclosure.** ★ The one thing this feature does that Acrobat
//!    does not: when a font's program cannot safely be removed, pdfcer says
//!    **why**. Acrobat reaches the same refusal and communicates it by
//!    omitting the font from a list, with no reason shown anywhere
//!    (`Acrobat_Features/optimize__font_unembedding.md`, sourced to a
//!    former Adobe Principal Scientist). If the reason ever stopped being
//!    printed, the listing would still look correct and the feature would
//!    have lost its point — so the reason is asserted, not just the
//!    verdict.
//!
//! 3. **The coverage declaration.** The summary names what was searched
//!    AND what was not. A font inventory that quietly misses a surface and
//!    prints a confident list is this project's most-repeated defect shape
//!    (R186), and `not_walked=` is what stops this one from joining it.
//!
//! 4. **Read-only.** The command must not touch the input. Asserted by
//!    byte comparison, not by inspection — nothing in the code path writes,
//!    but "nothing currently writes" is not a contract.
//!
//! Fixtures (provenance in each directory's `PROVENANCE.md`):
//! `fixtures/synthetic/text/*`, `fixtures/synthetic/fontinfo/*`,
//! `fixtures/synthetic/forms/demo-form.pdf`,
//! `fixtures/synthetic/annot/ap-resources-own-font.pdf`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// Run `list-fonts` on a fixture and return `(stdout, stderr)`, asserting a
/// clean exit. Reporting is never an error path: a document with no fonts,
/// an unwalkable page tree and a damaged font program all exit 0 with the
/// facts stated, because a batch sweep needs to tell "pdfcer refused" from
/// "the document is like that".
fn run(rel: &str, args: &[&str]) -> (String, String) {
    let out = Command::new(BIN)
        .arg("list-fonts")
        .arg(fixture(rel))
        .args(args)
        .output()
        .expect("pdfcer runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "list-fonts {rel} should exit 0\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// ★ The headline case. An `Identity-H` composite font over an embedded
/// `CIDFontType2` is reported, its verdict is `blocked-identity`, and the
/// **reason names the mechanism** — that the character codes are glyph
/// indices into that specific program.
///
/// Asserting the reason text and not merely the verdict token is the point:
/// the token alone is exactly as informative as Acrobat's silent omission.
#[test]
fn an_identity_encoded_font_is_listed_with_a_reason_not_just_a_refusal() {
    let (stdout, stderr) = run("text/cidfonttype2-with-tounicode.pdf", &[]);
    assert!(stdout.contains("verdict=blocked-identity"), "{stdout}");
    assert!(stdout.contains("type=\"Type0/CIDFontType2\""), "{stdout}");
    assert!(stdout.contains("encoding=\"Identity-H\""), "{stdout}");
    assert!(stdout.contains("embedded=FontFile2"), "{stdout}");
    assert!(stdout.contains("tounicode=1"), "{stdout}");
    // The disclosure Acrobat does not make, on stderr, unconditionally.
    assert!(
        stderr.contains("glyph indices"),
        "the mechanism must be stated, not merely the verdict: {stderr}"
    );
}

/// `--reasons` puts the same sentence next to the row it belongs to. It is
/// off by default so the listing stays parseable; the sentence is on stderr
/// either way, so nothing is hidden by leaving it off.
#[test]
fn the_reasons_flag_interleaves_the_sentence_with_the_rows() {
    let (plain, _) = run("text/cidfonttype2-nocmap-embedded.pdf", &[]);
    let (verbose, _) = run("text/cidfonttype2-nocmap-embedded.pdf", &["--reasons"]);
    assert!(!plain.contains("  reason:"), "{plain}");
    assert!(verbose.contains("  reason:"), "{verbose}");
    // No /ToUnicode: the reason must say the text is unrecoverable as well
    // as undrawable. Those are two independently-bad outcomes and the
    // 64-file survey found them stacking on most real files.
    assert!(
        verbose.contains("neither renderable nor recoverable"),
        "{verbose}"
    );
}

/// ★ The exceed-Acrobat number. Acrobat exposes a per-font byte size
/// nowhere — not Document Properties → Fonts, not Audit Space Usage, which
/// gives one aggregate bucket for the whole document. Both the per-font
/// figure and a document total are printed, and the total is the sum of the
/// rows so the two cannot disagree.
#[test]
fn the_embedded_program_size_is_reported_per_font_and_in_total() {
    let (stdout, _) = run("text/subset-simple-embedded.pdf", &[]);
    let row = stdout
        .lines()
        .find(|l| l.starts_with("font "))
        .expect("one font row");
    let bytes: u64 = row
        .split_whitespace()
        .find_map(|t| t.strip_prefix("bytes="))
        .expect("a bytes= field")
        .parse()
        .expect("a number");
    assert!(bytes > 0, "an embedded program has a size: {row}");

    let summary = stdout
        .lines()
        .find(|l| l.starts_with("list-fonts "))
        .expect("a summary line");
    let total: u64 = summary
        .split_whitespace()
        .find_map(|t| t.strip_prefix("bytes="))
        .expect("a bytes= field")
        .parse()
        .expect("a number");
    assert_eq!(
        total, bytes,
        "the document total must be the sum of the rows"
    );
    assert!(summary.contains("embedded=1"), "{summary}");
    assert!(summary.contains("removable=1"), "{summary}");
}

/// ★ The coverage declaration. The summary names the surfaces that were
/// searched and the one that was not, and stderr states the omission in
/// words. Neither is conditional on anything being wrong: an operator
/// deciding what to delete needs the shape of the evidence.
#[test]
fn coverage_is_declared_in_both_directions() {
    let (stdout, stderr) = run("text/simple-winansi.pdf", &[]);
    let summary = stdout
        .lines()
        .find(|l| l.starts_with("list-fonts "))
        .expect("a summary line");
    for surface in [
        "page",
        "form-xobject",
        "pattern",
        "softmask",
        "type3-charprocs",
        "acroform-dr",
        "annotation-ap",
    ] {
        assert!(
            summary.contains(surface),
            "walked surfaces must be named: {surface} missing from {summary}"
        );
    }
    assert!(summary.contains("not_walked=unreferenced"), "{summary}");
    assert!(
        stderr.contains("NOT searched: unreferenced"),
        "the omission must be stated in words, not only as a token: {stderr}"
    );
}

/// A font reachable only through a form XObject nested inside an annotation
/// appearance stream is found. Two hops a page-resources sweep misses; this
/// is the falsifier for the coverage claim above.
#[test]
fn a_font_two_hops_deep_behind_an_appearance_stream_is_listed() {
    let (stdout, _) = run("fontinfo/nested-ap-xobject.pdf", &[]);
    assert!(stdout.contains("pdfceHiddenInAppearance"), "{stdout}");
    assert!(stdout.contains("surfaces=form-xobject"), "{stdout}");
    assert!(stdout.contains("pages=1"), "{stdout}");
}

/// A font in the AcroForm `/DR` is found and attributed to that surface
/// rather than to a page — it is reachable from no page at all.
#[test]
fn an_acroform_default_resource_font_is_listed_with_no_page() {
    let (stdout, _) = run("forms/demo-form.pdf", &[]);
    let row = stdout
        .lines()
        .find(|l| l.contains("surfaces=acroform-dr"))
        .expect("the /DR font");
    assert!(row.contains("pages=-"), "{row}");
    assert!(row.contains("std14=1"), "{row}");
}

/// `fsType` renders as four visibly different states, and **none of them
/// may look like `0`** — which genuinely means Installable, the most
/// permissive value the field can express. A blank, a dash or a zero for
/// "we could not read it" would assert the broadest embedding right there
/// is on the strength of bytes nobody read.
#[test]
fn fs_type_states_are_distinguishable_and_never_look_like_zero() {
    // A real read: the donor's OS/2 says Editable.
    let (editable, _) = run("fontinfo/symbolic-builtin-encoding.pdf", &[]);
    assert!(editable.contains("fstype=Editable/0x0008"), "{editable}");

    // A Type 1 program has no OS/2 table by construction — a different fact
    // from "unreadable", and both different from a permission.
    let (type1, _) = run("fontinfo/fontfile-type1.pdf", &[]);
    assert!(type1.contains("fstype=n/a-no-field"), "{type1}");

    // Nothing embedded: there are no bits to read at all.
    let (absent, _) = run("text/simple-winansi.pdf", &[]);
    assert!(absent.contains("fstype=n/a-not-embedded"), "{absent}");

    // None of the three renders as a permission value, and none as "0".
    for out in [&editable, &type1, &absent] {
        assert!(
            !out.contains("fstype=0") && !out.contains("fstype=-"),
            "an unread fsType must never render like Installable: {out}"
        );
    }
}

/// "Declared but unreadable" is not "not embedded". The first is a damaged
/// document, the second is a document relying on substitution, and an
/// operator's next move differs. The row says which, and the verdict is
/// `Unknown` rather than a guess in either direction.
#[test]
fn a_dangling_font_program_is_reported_as_damage_not_as_absence() {
    let (stdout, _) = run("fontinfo/unreadable-program.pdf", &[]);
    assert!(stdout.contains("embedded=FontFile2!unreadable"), "{stdout}");
    assert!(
        stdout.contains("verdict=unknown-program-unreadable"),
        "{stdout}"
    );
    assert!(stdout.contains("programs_unreadable=1"), "{stdout}");
    assert!(!stdout.contains("embedded=no"), "{stdout}");
}

/// A Type 3 font is blocked for a reason that has nothing to do with
/// embedding, and the inner font its `/CharProcs` resources name is found.
#[test]
fn a_type3_font_and_the_fonts_inside_it_are_both_listed() {
    let (stdout, stderr) = run("fontinfo/type3-charprocs.pdf", &[]);
    assert!(stdout.contains("type=\"Type3\""), "{stdout}");
    assert!(stdout.contains("verdict=blocked-type3"), "{stdout}");
    assert!(stdout.contains("surfaces=type3-charprocs"), "{stdout}");
    assert!(stdout.contains("fonts=2"), "{stdout}");
    assert!(stderr.contains("drawing procedures inside this document"));
}

/// `--by-size` reorders largest-first without changing the rows. The
/// default is first-discovery, which is stable and diff-friendly; "what is
/// costing me the most" is a separate question and gets a separate flag.
#[test]
fn by_size_sorts_largest_first() {
    let (stdout, _) = run("fontinfo/predefined-cmap.pdf", &["--by-size"]);
    let sizes: Vec<u64> = stdout
        .lines()
        .filter(|l| l.starts_with("font "))
        .filter_map(|l| {
            l.split_whitespace()
                .find_map(|t| t.strip_prefix("bytes="))?
                .parse()
                .ok()
        })
        .collect();
    assert!(!sizes.is_empty());
    assert!(
        sizes.windows(2).all(|w| w[0] >= w[1]),
        "descending by size: {sizes:?}"
    );
}

/// An unwalkable page tree is **reported**, not rendered as "no fonts".
/// Without the flag, "this document has no fonts" and "pdfcer could not
/// look" print identically, which is confident-but-blind reporting.
#[test]
fn an_unwalkable_page_tree_is_flagged_rather_than_reported_as_empty() {
    let (stdout, stderr) = run("minimal.pdf", &[]);
    assert!(stdout.contains("fonts=0"), "{stdout}");
    assert!(stdout.contains("PAGE_SCAN_FAILED"), "{stdout}");
    assert!(
        stderr.contains("not a statement about the document's fonts"),
        "{stderr}"
    );
}

/// A document with no fonts exits clean and says so, so a batch sweep can
/// tally font-free files apart from failures.
#[test]
fn a_font_free_document_exits_clean() {
    let (stdout, _) = run("vector/paths.pdf", &[]);
    assert!(stdout.contains("fonts=0 embedded=0 bytes=0"), "{stdout}");
    assert!(stdout.contains("clean"), "{stdout}");
}

/// The command is read-only. Asserted by byte comparison rather than by
/// reading the code: "nothing currently writes" is not a contract.
#[test]
fn listing_fonts_does_not_touch_the_input() {
    let path = fixture("text/subset-simple-embedded.pdf");
    let before = std::fs::read(&path).expect("fixture readable");
    let _ = run(
        "text/subset-simple-embedded.pdf",
        &["--reasons", "--by-size"],
    );
    let after = std::fs::read(&path).expect("fixture readable");
    assert_eq!(before, after, "list-fonts must not modify the input");
}

/// An encrypted document's font sizes are the plaintext's, not the
/// ciphertext's.
///
/// ★ The `data_span`-vs-`/Length` hazard, end to end through the real
/// binary. The decryption walk shortens `data_span` and leaves `/Length` at
/// the ciphertext length by design; an `/AESV2` stream carries a 16-byte IV
/// plus padding, so a size read from `/Length` overstates by at least 17
/// bytes. The reported size is compared against the same font in the
/// unencrypted source document, which is the only comparison that can tell
/// the two apart.
#[test]
fn an_encrypted_document_reports_plaintext_font_sizes() {
    let (plain, _) = run("text/subset-simple-embedded.pdf", &[]);
    let plain_decoded = plain
        .lines()
        .find(|l| l.starts_with("font "))
        .and_then(|l| {
            l.split_whitespace()
                .find_map(|t| t.strip_prefix("decoded="))
        })
        .expect("a decoded= field")
        .to_owned();

    let out = Command::new(BIN)
        .arg("list-fonts")
        .arg(fixture("fontinfo/enc-aes-128-embedded-font.pdf"))
        .arg("--open-password")
        .arg("userpw")
        .output()
        .expect("pdfcer runs");
    assert_eq!(out.status.code(), Some(0));
    let encrypted = String::from_utf8_lossy(&out.stdout);
    let enc_decoded = encrypted
        .lines()
        .find(|l| l.starts_with("font "))
        .and_then(|l| {
            l.split_whitespace()
                .find_map(|t| t.strip_prefix("decoded="))
        })
        .expect("a decoded= field")
        .to_owned();

    assert_ne!(enc_decoded, "-", "the program must decode: {encrypted}");
    assert_eq!(
        enc_decoded, plain_decoded,
        "the decoded program size must match the unencrypted source; a mismatch \
         means the ciphertext length was measured"
    );
}
