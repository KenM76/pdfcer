//! # `Pass 69.0` — the ce-dimension STYLE CASCADE, through the CLI
//!
//! What these tests pin is the **cascade**, not the drawing: that a group
//! default reaches a ce dimension that does not override it, that an override
//! wins where it is set, that clearing an override restores inheritance, and
//! that the inheritance state is **reported** rather than merely stored.
//!
//! ## Why these assertions run through the CLI and read the FILE back
//!
//! `pdfcer-core`'s own unit tests pin `resolve_style`'s arithmetic. They cannot
//! catch the failure this Pass was most exposed to: a cascade that resolves
//! correctly in memory and is **discarded on the way to the document**. That
//! bug has exactly one visible symptom — the override works in the panel and
//! vanishes in the saved file — and only a test that saves, reopens and reads
//! the sidecar back can see it. Every assertion below therefore reads
//! `dimension-list --style` on a file that has been through a save.
//!
//! ## The terminology, per project rule 15
//!
//! Everything here is a **ce dimension** — the ones pdfcer authors. No **pdf
//! dimension** (a CAD-exported one already in the page content) is touched by
//! any of this; a style is a property of pdfcer's own annotation, and a
//! CAD-exported dimension's appearance is page content pdfcer must not alter.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-dimension-style-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Author one linear ce dimension into a fresh copy of the fixture.
fn with_dimension(name: &str) -> PathBuf {
    let out = temp_out(name);
    let (code, stdout, stderr) = run(&[
        "dimension-add",
        fixture().to_str().expect("utf-8 path"),
        "--kind",
        "linear",
        "--points",
        "100,100 300,100",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "authoring must succeed:\n{stdout}\n{stderr}");
    out
}

/// Every `style …` line `dimension-list --style` prints for the first ce
/// dimension, as `(property, value, source)` triples.
fn styles(path: &Path) -> Vec<(String, String, String)> {
    let (code, stdout, stderr) = run(&[
        "dimension-list",
        path.to_str().expect("utf-8 path"),
        "--style",
    ]);
    assert_eq!(code, 0, "listing must succeed:\n{stdout}\n{stderr}");
    stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let rest = l.strip_prefix("style ")?;
            let (pair, src) = rest.rsplit_once(" (")?;
            let (name, value) = pair.split_once('=')?;
            Some((
                name.to_owned(),
                value.to_owned(),
                src.trim_end_matches(')').to_owned(),
            ))
        })
        .collect()
}

fn prop<'a>(list: &'a [(String, String, String)], name: &str) -> &'a (String, String, String) {
    list.iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("`{name}` must be listed; got {list:?}"))
}

/// A brand-new ce dimension inherits everything, and says so.
#[test]
fn a_fresh_ce_dimension_reports_every_property_as_inherited() {
    let doc = with_dimension("fresh.pdf");
    let s = styles(&doc);
    assert_eq!(
        s.len(),
        11,
        "all eleven properties must be disclosed: {s:?}"
    );
    assert_eq!(prop(&s, "line-width").1, "0.75", "the factory stroke width");
    assert_eq!(prop(&s, "line-width").2, "factory");
    assert_eq!(prop(&s, "arrow-form").1, "filled");
    assert_eq!(prop(&s, "arrow-form").2, "factory");
    // The measurement-side properties always have a group answer, so they
    // report `group`, never `factory`.
    assert_eq!(prop(&s, "unit").2, "group");
}

/// A GROUP default reaches a member that does not override it — and the
/// listing attributes it to the group, not to the factory.
#[test]
fn a_group_default_reaches_a_member_that_does_not_override_it() {
    let doc = with_dimension("group-default.pdf");
    let out = temp_out("group-default-2.pdf");
    let (code, stdout, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--group",
        "0",
        "--line-width",
        "1.5",
        "--arrow-form",
        "slash",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("regenerated=1"),
        "the member must be regenerated, and the count reported:\n{stdout}"
    );

    let s = styles(&out);
    assert_eq!(prop(&s, "line-width").1, "1.5");
    assert_eq!(prop(&s, "line-width").2, "group");
    assert_eq!(prop(&s, "arrow-form").1, "slash");
    assert_eq!(prop(&s, "arrow-form").2, "group");
    // Untouched by the group ⇒ still factory. Setting one property must not
    // detach the member from the rest of the cascade.
    assert_eq!(prop(&s, "arrow-length").2, "factory");
}

/// ★ The operator's actual request: a group default, with ONE member set
/// differently, and the two states distinguishable afterwards.
#[test]
fn a_per_ce_dimension_override_beats_the_group_and_is_reported_as_such() {
    let doc = with_dimension("override.pdf");
    let grouped = temp_out("override-group.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--line-width",
        "1.5",
        "-o",
        grouped.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let overridden = temp_out("override-dim.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        grouped.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--line-width",
        "3",
        "--unit",
        "in",
        "-o",
        overridden.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("overrides=2"),
        "the override count must be reported:\n{stdout}"
    );

    let s = styles(&overridden);
    assert_eq!(prop(&s, "line-width").1, "3");
    assert_eq!(prop(&s, "line-width").2, "dimension");
    assert_eq!(prop(&s, "unit").1, "in");
    assert_eq!(prop(&s, "unit").2, "dimension");
}

/// Clearing an override restores inheritance completely — the value goes back
/// to the group's, not to whatever it happened to be when it was overridden.
///
/// This is a deliberate divergence from the reference tool, whose
/// `DeleteStyle` leaves the annotation carrying the attributes the style had
/// pushed into it (`SolidWorks_Dimensions` §F.4). pdfcer's cascade is a live
/// link in both directions.
#[test]
fn clearing_an_override_restores_the_group_value_not_the_old_one() {
    let doc = with_dimension("clear.pdf");
    let grouped = temp_out("clear-group.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--line-width",
        "1.5",
        "-o",
        grouped.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let overridden = temp_out("clear-dim.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        grouped.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--line-width",
        "3",
        "-o",
        overridden.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let cleared = temp_out("clear-back.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        overridden.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--clear",
        "line-width",
        "-o",
        cleared.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("overrides=0"), "{stdout}");

    let s = styles(&cleared);
    assert_eq!(
        prop(&s, "line-width").1,
        "1.5",
        "the GROUP's value, not the overridden 3 and not the factory 0.75"
    );
    assert_eq!(prop(&s, "line-width").2, "group");
}

/// Flags not given leave the other properties alone — the read-modify-write
/// contract. Setting the colour must not silently clear the arrow form.
#[test]
fn setting_one_property_does_not_clear_the_others() {
    let doc = with_dimension("rmw.pdf");
    let first = temp_out("rmw-1.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--arrow-form",
        "dot",
        "-o",
        first.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let second = temp_out("rmw-2.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        first.to_str().expect("utf-8 path"),
        "--color",
        "#ff0000",
        "-o",
        second.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let s = styles(&second);
    assert_eq!(
        prop(&s, "arrow-form").1,
        "dot",
        "must survive the second set"
    );
    assert_eq!(prop(&s, "color").1, "1,0,0");
    assert_eq!(prop(&s, "color").2, "group");
}

/// ★ The override reaches the BAKED APPEARANCE, not just the sidecar.
///
/// Every other test here reads the model back through `dimension-list`, which
/// reads the `/PieceInfo` sidecar. A build that stored the override faithfully
/// and then regenerated the `/AP` from the GROUP alone would pass all of them
/// and still draw the wrong thing in every reader — the exact failure the
/// cascade's single resolution point exists to prevent, and one that is
/// invisible from anywhere except the drawn bytes.
///
/// So this asserts on the content stream: a 3-point stroke width must appear
/// as `3 w`, and the factory `0.75 w` must be **gone**. The save is a FULL
/// rewrite deliberately — an incremental save keeps the superseded appearance
/// object in the file, so `0.75 w` would still be findable and the absence
/// half of the assertion would pass vacuously. (A vacuously-passing byte
/// assertion is exactly what nearly shipped a broken degree sign in
/// `Pass 68.0`.)
#[test]
fn an_override_reaches_the_baked_appearance_stream() {
    let doc = with_dimension("baked.pdf");
    let out = temp_out("baked-out.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--line-width",
        "3",
        "--mode",
        "full",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let bytes = std::fs::read(&out).expect("the output must exist");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("3 w"),
        "the overridden 3pt stroke width must be in the baked appearance"
    );
    assert!(
        !text.contains("0.75 w"),
        "and the factory width must be gone from a full rewrite"
    );
}

/// A metric that is not a metric is refused by name, before anything is
/// written — not clamped to something plausible.
#[test]
fn a_non_positive_metric_is_refused_by_name() {
    let doc = with_dimension("refuse.pdf");
    let out = temp_out("refuse-out.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--line-width",
        "0",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "stderr:\n{stderr}");
    assert!(
        stderr.contains("line-width"),
        "the refusal must name the flag:\n{stderr}"
    );
    assert!(
        !out.exists(),
        "a refusal must not leave a half-written output file behind"
    );
}

/// Two different number formats in one invocation is an ambiguous request, and
/// is refused rather than silently resolved by argument order.
#[test]
fn places_and_denominator_together_are_refused() {
    let doc = with_dimension("ambiguous.pdf");
    let out = temp_out("ambiguous-out.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--places",
        "2",
        "--denominator",
        "16",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "stderr:\n{stderr}");
    assert!(stderr.contains("--places"), "{stderr}");
}

/// A group-tier `--clear` naming a ce-dimension-only property is refused by
/// name, rather than accepted and ignored.
#[test]
fn clearing_a_ce_dimension_only_property_at_the_group_tier_is_refused() {
    let doc = with_dimension("wrong-tier.pdf");
    let out = temp_out("wrong-tier-out.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--clear",
        "unit",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "stderr:\n{stderr}");
    assert!(
        stderr.contains("per-ce-dimension property"),
        "the refusal must explain the tier, not just fail:\n{stderr}"
    );
}

/// An unknown id is refused before any mutation, from both commands.
#[test]
fn unknown_ids_are_refused() {
    let doc = with_dimension("unknown.pdf");
    let out = temp_out("unknown-out.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "77",
        "--line-width",
        "2",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{stderr}");

    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--group",
        "77",
        "--line-width",
        "2",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{stderr}");
}

// ---- Pass 69.1: tolerance -------------------------------------------------

/// A group DEFAULT tolerance reaches a member that does not override it, and
/// the baked label says so.
#[test]
fn a_group_default_tolerance_reaches_its_members() {
    let doc = with_dimension("tol-group.pdf");
    let out = temp_out("tol-group-out.pdf");
    let (code, stdout, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--tolerance",
        "sym:0.5",
        "--tolerance-places",
        "1",
        "--mode",
        "full",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");

    let s = styles(&out);
    assert_eq!(prop(&s, "tolerance").1, "sym:0.5");
    assert_eq!(prop(&s, "tolerance").2, "group");
    assert_eq!(prop(&s, "tolerance-places").1, "1");

    // ★ WinAnsi, not UTF-8 — the `Pass 68.0` regression in its second costume.
    // The label font is declared `/WinAnsiEncoding`, and `±` (U+00B1) is only
    // the SECOND non-ASCII character this writer has ever emitted; the first
    // (the degree sign) shipped broken. The assertion is therefore in the
    // encoding the writer actually emits: the octal escape is present and the
    // UTF-8 pair is not. Asserting on RAW bytes would pass vacuously — the
    // writer octal-escapes every high byte, so neither form appears raw.
    let bytes = std::fs::read(&out).expect("output");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("\\261"),
        "the plus-minus must be WinAnsi-escaped in the baked label"
    );
    assert!(
        !text.contains("\\302\\261"),
        "and must not be the UTF-8 pair"
    );
}

/// A per-ce-dimension tolerance beats the group's, and an explicit "no
/// tolerance on THIS one" is expressible.
#[test]
fn a_ce_dimension_can_override_a_group_tolerance_with_no_tolerance() {
    let doc = with_dimension("tol-override.pdf");
    let grouped = temp_out("tol-override-group.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--tolerance",
        "sym:0.5",
        "-o",
        grouped.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let off = temp_out("tol-override-off.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        grouped.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        "none",
        "--mode",
        "full",
        "-o",
        off.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");

    let s = styles(&off);
    assert_eq!(prop(&s, "tolerance").1, "none");
    assert_eq!(
        prop(&s, "tolerance").2,
        "dimension",
        "an explicit `none` is an OVERRIDE, not inheritance — a group that \
         tolerances everything and one feature that must not be toleranced is \
         a real drawing"
    );
    let bytes = std::fs::read(&off).expect("output");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains("\\261"),
        "and the label must carry no tolerance at all"
    );
}

/// A limit tolerance SUPPRESSES the nominal and prints its two limits — the
/// reference's own behaviour for this type.
#[test]
fn a_limit_tolerance_replaces_the_nominal_value() {
    let doc = with_dimension("tol-limit.pdf");
    let out = temp_out("tol-limit-out.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        "limit:200.2/199.8",
        "--mode",
        "full",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let bytes = std::fs::read(&out).expect("output");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("200.20/199.80"),
        "the two limits must be drawn"
    );
    assert!(
        !text.contains("(200.00 pt"),
        "and the nominal must be suppressed, not printed alongside"
    );
}

/// `basic` draws a box and changes no text — the box IS the notation.
#[test]
fn a_basic_tolerance_draws_a_box_and_leaves_the_label_alone() {
    let doc = with_dimension("tol-basic.pdf");
    let out = temp_out("tol-basic-out.pdf");
    let (code, stdout, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        "basic",
        "--mode",
        "full",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");

    let boxed = std::fs::read(&out).expect("output");
    let plain = std::fs::read(&doc).expect("the untoleranced original");
    let stroke_ops = |b: &[u8]| b.windows(3).filter(|w| *w == b"\nS\n").count();
    assert!(
        stroke_ops(&boxed) > stroke_ops(&plain),
        "the box must add a stroked path: {} vs {}",
        stroke_ops(&boxed),
        stroke_ops(&plain)
    );
    let text = String::from_utf8_lossy(&boxed);
    assert!(
        text.contains("(200.00 pt)"),
        "and the label text must be untouched — Basic prints no characters"
    );
}

/// An inverted limit pair is refused by name rather than silently swapped: a
/// drawing stating that the maximum is below the minimum is a manufacturing
/// defect delivered by an editor being helpful.
#[test]
fn an_inverted_limit_pair_is_refused_not_swapped() {
    let doc = with_dimension("tol-inverted.pdf");
    let out = temp_out("tol-inverted-out.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        "limit:1/2",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{stderr}");
    assert!(stderr.contains("upper limit"), "{stderr}");
    assert!(!out.exists(), "and nothing must be written");
}

/// A negative symmetric magnitude is a typo, not a tolerance.
#[test]
fn a_negative_symmetric_magnitude_is_refused() {
    let doc = with_dimension("tol-negative.pdf");
    let out = temp_out("tol-negative-out.pdf");
    let (code, _, stderr) = run(&[
        "group-style",
        doc.to_str().expect("utf-8 path"),
        "--tolerance",
        "sym:-0.1",
        "-o",
        out.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, EDIT_REFUSED, "{stderr}");
    assert!(stderr.contains("magnitude"), "{stderr}");
}

/// The listing prints a tolerance in the exact grammar `--tolerance` accepts,
/// so a value read out of a listing can be fed straight back into a script.
#[test]
fn the_listed_tolerance_spec_is_accepted_back_verbatim() {
    let doc = with_dimension("tol-roundtrip.pdf");
    let first = temp_out("tol-roundtrip-1.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        doc.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        "dev:0.2/-0.1",
        "-o",
        first.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "{stderr}");

    let listed = prop(&styles(&first), "tolerance").1.clone();
    assert_eq!(listed, "dev:0.2/-0.1");

    let second = temp_out("tol-roundtrip-2.pdf");
    let (code, _, stderr) = run(&[
        "dimension-style",
        first.to_str().expect("utf-8 path"),
        "--dimension",
        "0",
        "--tolerance",
        &listed,
        "-o",
        second.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(code, 0, "feeding the listed spec back must work:\n{stderr}");
    assert_eq!(prop(&styles(&second), "tolerance").1, listed);
}
