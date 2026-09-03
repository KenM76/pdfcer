//! # `Pass 176.0` — the four ce-dimension GROUP-MANAGEMENT verbs, through the CLI
//!
//! `rename_dimension_group`, `delete_dimension_group`,
//! `delete_dimension_group_with` and `set_dimension_group` have shipped in
//! `pdfcer-core` since `Pass 25.7`. Until this Pass **no subcommand reached any
//! of them**, so from a script a ce dimension group was create-only: it could
//! be made, scaled, styled and given a drafting standard, and then never
//! renamed, never removed, and never had a member moved out of it.
//!
//! That is the `R151` shape — a core API callable and uncalled — and
//! `docs/FEATURES.md` carried it as an explicit `[x] core / [ ] cli` row.
//!
//! ## Why these tests drive the BINARY and read the FILE back
//!
//! The same argument `dimension_style.rs` makes, plus one this Pass is
//! specifically exposed to. Three of the four verbs are reached through a
//! **flag combination** that core never sees:
//! `--members reassign --to N` is recombined in the CLI into
//! `GroupDeletion::Reassign(GroupId(N))`, and the two mismatched combinations
//! are refused in the CLI, before the document is even opened.
//!
//! A unit test calling core directly cannot exercise any of that — it would
//! construct the policy itself and pass every assertion on a build whose flag
//! was parsed and never read. Only running the binary finds a half-wired flag.
//!
//! ## The terminology, per project rule 15
//!
//! Everything here is a **ce dimension** and a ce-dimension GROUP — pdfcer's
//! own authored objects. No **pdf dimension** (a CAD-exported one already in
//! the page content) is touched: a group is a pdfcer construct that exists only
//! in the `/PieceInfo` sidecar.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED`, spelled out so a change to the number is a visible
/// test failure rather than a silent contract break.
const EDIT_REFUSED: i32 = 9;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/dimension/plain-base.pdf")
}

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-dimension-group-tests");
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

/// A document with a second group at a **different scale** from the default,
/// and one ce dimension in the default group.
///
/// The differing scale is the point: it is what makes a re-parent observable.
/// A fixture whose two groups shared a scale would let a `set_dimension_group`
/// that moved the record and skipped the regeneration pass every assertion.
fn two_groups(name: &str) -> PathBuf {
    let a = temp_out(&format!("{name}-a.pdf"));
    let b = temp_out(&format!("{name}-b.pdf"));
    let c = temp_out(&format!("{name}-c.pdf"));
    let d = temp_out(&format!("{name}.pdf"));

    // Default group (0) at 0.025 m/pt: a 200 pt line reads 5.000 m.
    let (code, _, err) = run(&[
        "group-set-scale",
        fixture().to_str().unwrap(),
        "--group",
        "0",
        "--real-length",
        "5m",
        "--drawn",
        "200",
        "--precision",
        "3",
        "-o",
        a.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "group-set-scale on the default group: {err}");

    let (code, _, err) = run(&[
        "group-add",
        a.to_str().unwrap(),
        "--name",
        "Detail",
        "--unit",
        "m",
        "-o",
        b.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "group-add: {err}");

    // Group 1 at HALF the scale: the same line reads 2.500 m there.
    let (code, _, err) = run(&[
        "group-set-scale",
        b.to_str().unwrap(),
        "--group",
        "1",
        "--real-length",
        "2.5m",
        "--drawn",
        "200",
        "--precision",
        "3",
        "-o",
        c.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "group-set-scale on the new group: {err}");

    let (code, _, err) = run(&[
        "dimension-add",
        c.to_str().unwrap(),
        "--page",
        "1",
        "--kind",
        "linear",
        "--points",
        "100,200 300,200",
        "--group",
        "0",
        "-o",
        d.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "dimension-add: {err}");
    d
}

fn list(path: &Path) -> String {
    let (code, out, err) = run(&["dimension-list", path.to_str().unwrap()]);
    assert_eq!(code, 0, "dimension-list: {err}");
    out
}

// ---------------------------------------------------------------------------
// group-rename
// ---------------------------------------------------------------------------

/// **A rename reaches the saved file, and reports the name it replaced.**
///
/// Both names are asserted on the result line. The old one is the half a
/// caller cannot recover afterwards — it is gone from the document the moment
/// the write succeeds — and it is load-bearing beyond the group list: a ce
/// dimension pasted into another document is matched to a destination group
/// **by name**, so a rename silently changes where a later paste lands.
#[test]
fn a_rename_lands_in_the_file_and_names_what_it_replaced() {
    let src = two_groups("rename");
    let out = temp_out("rename-out.pdf");
    let (code, stdout, err) = run(&[
        "group-rename",
        src.to_str().unwrap(),
        "--group",
        "1",
        "--name",
        "Site plan 1:100",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "group-rename: {err}");
    assert!(
        stdout.contains("was=\"Detail\""),
        "the replaced name is reported: {stdout}"
    );
    assert!(
        stdout.contains("now=\"Site plan 1:100\""),
        "and so is the new one: {stdout}"
    );

    let listing = list(&out);
    assert!(
        listing.contains("\"Site plan 1:100\""),
        "the rename survives the save: {listing}"
    );
    assert!(
        !listing.contains("\"Detail\""),
        "and the old name is gone: {listing}"
    );
}

/// **An unknown group is refused by name, and the output is not written.**
#[test]
fn renaming_an_unknown_group_is_refused_and_writes_nothing() {
    let src = two_groups("rename-unknown");
    let out = temp_out("rename-unknown-out.pdf");
    let (code, _, err) = run(&[
        "group-rename",
        src.to_str().unwrap(),
        "--group",
        "99",
        "--name",
        "x",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, EDIT_REFUSED, "refused by name: {err}");
    assert!(!out.exists(), "a refusal writes no output file");
}

// ---------------------------------------------------------------------------
// group-delete
// ---------------------------------------------------------------------------

/// **An empty group deletes; a populated one is REFUSED by default and says
/// how many members it has.**
///
/// The default policy is the safe one, and the refusal carries the count so a
/// script learns what it was about to destroy rather than only that it failed.
#[test]
fn deleting_an_empty_group_works_and_a_populated_one_refuses_with_its_count() {
    let src = two_groups("delete");

    // Group 1 is empty -- the ce dimension went into group 0.
    let out = temp_out("delete-empty-out.pdf");
    let (code, stdout, err) = run(&[
        "group-delete",
        src.to_str().unwrap(),
        "--group",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "an empty group deletes: {err}");
    assert!(
        stdout.contains("members_moved=0"),
        "and moves nothing: {stdout}"
    );
    assert!(
        !list(&out).contains("\"Detail\""),
        "the group is gone from the saved file"
    );

    // Now populate group 1 and try again. Deliberately group 1 and NOT
    // group 0: the default group is refused for a different reason entirely
    // (see the sibling test), and asserting the not-empty refusal on it would
    // pass on the wrong message and stop testing this rule at all.
    let staged = temp_out("delete-staged.pdf");
    let (code, _, err) = run(&[
        "dimension-group",
        src.to_str().unwrap(),
        "--dimension",
        "0",
        "--group",
        "1",
        "-o",
        staged.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "staging move into group 1: {err}");

    let out2 = temp_out("delete-populated-out.pdf");
    let (code, _, err) = run(&[
        "group-delete",
        staged.to_str().unwrap(),
        "--group",
        "1",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code, EDIT_REFUSED, "a populated group refuses by default");
    assert!(
        err.contains("member"),
        "the refusal is the NOT-EMPTY one, not some other refusal: {err}"
    );
    assert!(err.contains('1'), "and it names the member count: {err}");
    assert!(!out2.exists(), "a refusal writes no output file");
}

/// **`--members reassign --to N` moves the members AND RE-MEASURES them.**
///
/// The re-measurement is the assertion that matters. A version of this that
/// moved the record and skipped the regeneration would pass a member count
/// and leave a ce dimension on the page displaying a number its new group
/// disagrees with -- right for the group it left.
///
/// 200 pt at group 1's 0.0125 m/pt reads `2.500 m`; at group 0's 0.025 it
/// reads `5.000 m`. The dimension starts in group 0 and is moved out of a
/// **non-default** group here -- see the sibling test below for why deleting
/// the default group is refused outright rather than tested this way.
#[test]
fn reassigning_on_delete_moves_the_members_and_re_measures_them() {
    let src = two_groups("reassign");
    // Put the ce dimension in group 1 first, so the group being deleted is a
    // deletable one.
    let staged = temp_out("reassign-staged.pdf");
    let (code, _, err) = run(&[
        "dimension-group",
        src.to_str().unwrap(),
        "--dimension",
        "0",
        "--group",
        "1",
        "-o",
        staged.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "staging move into group 1: {err}");
    assert!(
        list(&staged).contains("value=\"2.500 m\""),
        "precondition: the ce dimension reads at group 1's scale"
    );

    let out = temp_out("reassign-out.pdf");
    let (code, stdout, err) = run(&[
        "group-delete",
        staged.to_str().unwrap(),
        "--group",
        "1",
        "--members",
        "reassign",
        "--to",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "reassign-on-delete: {err}");
    assert!(
        stdout.contains("members_moved=1"),
        "one member moved: {stdout}"
    );

    let listing = list(&out);
    assert!(
        listing.contains("value=\"5.000 m\""),
        "the member was RE-MEASURED against its new group's scale: {listing}"
    );
    assert!(
        listing.contains("group=0"),
        "and it belongs to the destination group now: {listing}"
    );
    assert!(
        !listing.contains("\"Detail\""),
        "and the emptied group is gone: {listing}"
    );
}

/// ★★ **The DEFAULT group cannot be deleted, and that refusal was missing
/// until this Pass.**
///
/// # What this pins, and why it is the most important test in the file
///
/// `EditSession::delete_dimension_group_with` had **no default-group guard**.
/// The pure-model sibling `DimensionModel::delete_group` has had one since
/// `Pass 25.7` -- one rule, two paths, enforced only in the path nobody
/// ships through. It stayed latent because nothing had ever called the
/// session verb with group `0`; giving the CLI a `group-delete` subcommand
/// and passing `--group 0` is what reached it.
///
/// # The damage it did, measured on the release binary before the fix
///
/// The sidecar pdfcer WROTE was correct -- group `0` removed, group `1` kept,
/// the member re-parented. The **reader** then rejected it: `deserialize_model`
/// requires group `0` as a coherence check and returns `None` without it, and
/// the session turns `None` into a FRESH, EMPTY model. So:
///
/// ```text
///   before:  groups=2 dimensions=1   dim 0 group=0 value="5.000 m"
///   after:   groups=1 dimensions=0   group 0 "Default" scale=no-scale
/// ```
///
/// Every group, every calibrated scale and the ce dimension itself, gone --
/// and gone **silently**, because the `/Line` annotation keeps rendering
/// perfectly off its baked `/AP`. Nothing looks wrong until the next save
/// writes the empty model over the good sidecar and makes it permanent.
///
/// The refusal is asserted to happen with the output file **not written**, so
/// this also pins that it fires before any mutation (rule 4).
#[test]
fn the_default_group_cannot_be_deleted_and_the_refusal_writes_nothing() {
    let src = two_groups("default-delete");
    let before = list(&src);
    assert!(
        before.contains("dimensions=1"),
        "precondition: there is something to lose"
    );

    for extra in [vec![], vec!["--members", "reassign", "--to", "1"]] {
        let out = temp_out("default-delete-out.pdf");
        let mut argv = vec![
            "group-delete",
            src.to_str().unwrap(),
            "--group",
            "0",
            "-o",
            out.to_str().unwrap(),
        ];
        argv.extend(extra.iter().copied());
        let (code, _, err) = run(&argv);
        assert_eq!(
            code, EDIT_REFUSED,
            "deleting the default group is refused whatever the policy: {err}"
        );
        assert!(
            err.contains("default group"),
            "and the refusal says WHY, in the engine's own words: {err}"
        );
        assert!(
            !out.exists(),
            "a refusal before any mutation writes no output file"
        );
    }

    assert_eq!(
        list(&src),
        before,
        "and the input document is untouched by any of it"
    );
}

/// **The two mismatched flag combinations are refused BY NAME, before the
/// document is opened.**
///
/// This is the assertion a core-level test structurally cannot make: core
/// never sees `--members`/`--to`, only the recombined `GroupDeletion`. A
/// `--to` silently ignored under `refuse` would let a script believe it had
/// specified a destination that was never read, and a `reassign` defaulted to
/// some group would re-measure every dimension moved into it.
///
/// The input is deliberately a path that does not exist: if either refusal
/// ever moved to after the document is opened, the exit code would change to
/// the I/O one and this test would say so.
#[test]
fn a_mismatched_members_and_to_pair_is_refused_before_the_file_is_touched() {
    let out = temp_out("mismatch-out.pdf");

    let (code, _, err) = run(&[
        "group-delete",
        "no-such-file-anywhere.pdf",
        "--group",
        "0",
        "--members",
        "reassign",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, EDIT_REFUSED,
        "reassign without --to is refused: {err}"
    );
    assert!(
        err.contains("--to"),
        "and the message names the missing flag: {err}"
    );

    let (code, _, err) = run(&[
        "group-delete",
        "no-such-file-anywhere.pdf",
        "--group",
        "0",
        "--to",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, EDIT_REFUSED,
        "--to without reassign is refused: {err}"
    );
    assert!(
        err.contains("reassign"),
        "and the message names the policy it needs: {err}"
    );
    assert!(!out.exists());
}

// ---------------------------------------------------------------------------
// dimension-group (re-parent)
// ---------------------------------------------------------------------------

/// **Re-parenting one ce dimension RE-MEASURES it, and the CLI says so
/// unasked.**
///
/// The before/after pair on the result line exists because the re-measurement
/// is the single fact about this verb most likely to be reported as a defect:
/// an operator moves a dimension between groups and the number on the page
/// changes. It is correct, and the line says so at the moment it happens --
/// which is `CLAUDE.md` rule 4's CLI half, where the invocation is the commit
/// and there is no panel to put the disclosure in.
#[test]
fn re_parenting_re_measures_and_reports_both_values() {
    let src = two_groups("reparent");
    let out = temp_out("reparent-out.pdf");
    let (code, stdout, err) = run(&[
        "dimension-group",
        src.to_str().unwrap(),
        "--dimension",
        "0",
        "--group",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "dimension-group: {err}");
    assert!(
        stdout.contains("was=\"5.000 m\""),
        "the pre-move value is reported: {stdout}"
    );
    assert!(
        stdout.contains("now=\"2.500 m\""),
        "so is the post-move one: {stdout}"
    );
    assert!(
        stdout.contains("remeasured=1"),
        "and the fact that they differ is flagged, not left to be compared: {stdout}"
    );

    let listing = list(&out);
    assert!(
        listing.contains("value=\"2.500 m\""),
        "and the saved file agrees with what was reported: {listing}"
    );
}

/// **Re-parenting into the group it is already in reports `remeasured=0`.**
///
/// The contrast case. Without it, `remeasured=1` could be hard-coded and every
/// other assertion in this file would still pass.
#[test]
fn re_parenting_into_the_same_group_re_measures_nothing() {
    let src = two_groups("reparent-same");
    let out = temp_out("reparent-same-out.pdf");
    let (code, stdout, err) = run(&[
        "dimension-group",
        src.to_str().unwrap(),
        "--dimension",
        "0",
        "--group",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "dimension-group into the same group: {err}");
    assert!(
        stdout.contains("remeasured=0"),
        "nothing was re-measured: {stdout}"
    );
    assert!(
        stdout.contains("was=\"5.000 m\"") && stdout.contains("now=\"5.000 m\""),
        "and both values are the same: {stdout}"
    );
}

/// **An unknown destination group is refused by name and writes nothing.**
#[test]
fn re_parenting_into_an_unknown_group_is_refused() {
    let src = two_groups("reparent-unknown");
    let out = temp_out("reparent-unknown-out.pdf");
    let (code, _, err) = run(&[
        "dimension-group",
        src.to_str().unwrap(),
        "--dimension",
        "0",
        "--group",
        "99",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, EDIT_REFUSED, "refused by name: {err}");
    assert!(!out.exists(), "a refusal writes no output file");
}

// ---------------------------------------------------------------------------
// group-set-scale -- the unit named in --real-length
// ---------------------------------------------------------------------------

/// ★★ **A unit named in `--real-length` sets the GROUP'S unit, which is what
/// this command's own `--help` has always promised and what it did not do.**
///
/// # The defect, measured on the release binary before the fix
///
/// `--help` says, verbatim: *"A notation that names a unit sets the group's
/// unit too, so `--unit` is only needed for a bare number — the same rule the
/// GUI field follows, so a command and a click produce the same result from
/// the same text."*
///
/// It did not. The handler built the `NumberFormat` from `--unit` FIRST, and
/// the `--real-length` branch then **shadowed** `unit` with the one it read
/// out of the text. The shadow was local to that block, so the text-named
/// unit reached the SCALE and never reached the LABEL:
///
/// ```text
///   group-set-scale --real-length '55 5/8"' --drawn 200
///   -> group 0 unit=mm scale=0.278125 mm/pt
///   -> a 200 pt line reads   55.62 mm
/// ```
///
/// The magnitude is the INCH value; the label says MILLIMETRES. On a drawing
/// handed to a fabricator that is a 25.4x error wearing a plausible number,
/// and nothing reported it -- the success line did not print the unit at all,
/// which is part of why it survived. It does now.
///
/// # Why the assertions are on `unit=` AND on a measured dimension
///
/// `unit=` alone would pass on a build that labelled the group correctly and
/// still computed the scale from the wrong unit. The value read off a real
/// 200 pt ce dimension is what pins the two together.
#[test]
fn a_unit_named_in_real_length_reaches_the_label_and_not_only_the_scale() {
    // (notation, expected group unit token, expected reading of a 200 pt line)
    let cases: [(&str, &str, &str); 4] = [
        // Names metres: 200 pt = 5 m. Note the THREE decimals -- each unit
        // brings its own default precision (`Unit::default_format`), which is
        // the second thing the old code could not reach: it picked the format
        // from `--unit`'s default before the text's unit was known.
        ("5m", "m", "5.000 m"),
        // Names inches via the double-prime, with a vulgar fraction.
        ("55 5/8\"", "in", "55.62 in"),
        // Architectural feet-and-inches picks the feet-inches FORMAT too.
        //
        // `4/8` and not `1/2` is CORRECT and is not a rounding artefact: the
        // feet-inches format defaults to `reduce = false`, the architectural
        // convention that keeps the denominator (ISO 32000-1's `/FD`), so a
        // drawing dimensioned in eighths reads in eighths throughout. See
        // `NumberFormat::feet_inches`. Asserted in its unreduced form
        // deliberately -- a future change that started reducing would be a
        // visible behaviour change and should fail here.
        ("4'-7 1/2\"", "ft-in", "4'-7 4/8\""),
        // A BARE number still falls back to --unit's default. The contrast
        // case: without it, "always use the parsed unit" would pass every
        // other row here and break the documented fallback.
        ("12", "mm", "12.00 mm"),
    ];

    for (i, (notation, want_unit, want_value)) in cases.iter().enumerate() {
        let scaled = temp_out(&format!("unit-{i}-a.pdf"));
        let (code, stdout, err) = run(&[
            "group-set-scale",
            fixture().to_str().unwrap(),
            "--group",
            "0",
            "--real-length",
            notation,
            "--drawn",
            "200",
            "-o",
            scaled.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "group-set-scale {notation}: {err}");
        assert!(
            stdout.contains(&format!("unit={want_unit}")),
            "the result line reports the unit that won, for {notation}: {stdout}"
        );

        let listing = list(&scaled);
        assert!(
            listing.contains(&format!("unit={want_unit}")),
            "the GROUP carries it, for {notation}: {listing}"
        );

        let dimmed = temp_out(&format!("unit-{i}-b.pdf"));
        let (code, _, err) = run(&[
            "dimension-add",
            scaled.to_str().unwrap(),
            "--page",
            "1",
            "--kind",
            "linear",
            "--points",
            "100,200 300,200",
            "--group",
            "0",
            "-o",
            dimmed.to_str().unwrap(),
        ]);
        assert_eq!(code, 0, "dimension-add for {notation}: {err}");

        let listing = list(&dimmed);
        assert!(
            listing.contains(&format!("value=\"{want_value}\"")),
            "a 200 pt line under {notation} must read {want_value}: {listing}"
        );
    }
}

// ---------------------------------------------------------------------------
// layer-toggle -- two refusals it did not have (Pass 178.0)
// ---------------------------------------------------------------------------

/// ★★ **Hiding the DEFAULT group is refused, and an UNKNOWN group is refused
/// — this verb reported success for both.**
///
/// # What it did before, measured on the release binary
///
/// ```text
///   layer-toggle base.pdf --group 0  --hide
///   layer-toggle base.pdf --group 99 --hide
///   -> visible=true changed=1 appended=556   (exit 0, both)
/// ```
///
/// Group `0` is un-hideable by a rule that has existed since `Pass 12.M2`;
/// group `99` does not exist at all. Both got a **success**, a file **556
/// bytes larger**, and an undo entry that undoes nothing visible. A shell's
/// visibility switch flips back on with no explanation, which reads as a
/// broken switch rather than as a rule.
///
/// # Why the model could not report it, and why that is the pattern
///
/// `DimensionModel::set_group_visible` returns the resulting visibility and
/// nothing else, so it answers `true` for BOTH cases — un-hideable, and no
/// such group. It is a pure-model setter with no channel for a refusal. The
/// verb that ships through it is the policy boundary and had no guard.
///
/// **That is the same shape as `DimensionGroupIsDefault` one Pass earlier**
/// (`delete_dimension_group_with` vs `DimensionModel::delete_group`), and it
/// was found by going to look for a second instance rather than by a report.
///
/// # The unknown-group half is also an INCONSISTENCY
///
/// Every sibling group verb — `group-rename`, `group-set-scale`,
/// `group-set-standard`, `group-style`, `group-delete` — refuses an unknown
/// id by name. This one alone returned success, so a script that mistyped a
/// group id got a green exit code from it and a refusal from every other.
#[test]
fn hiding_the_default_group_and_naming_an_unknown_one_are_both_refused() {
    let src = two_groups("layer");
    let before = list(&src);

    for (group, why) in [
        ("0", "the default group is un-hideable"),
        ("99", "there is no group 99"),
    ] {
        let out = temp_out(&format!("layer-{group}.pdf"));
        let (code, stdout, err) = run(&[
            "layer-toggle",
            src.to_str().unwrap(),
            "--group",
            group,
            "--hide",
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_eq!(code, EDIT_REFUSED, "{why}: {err}{stdout}");
        assert!(
            !out.exists(),
            "{why} -- a refusal before any mutation writes no output file"
        );
    }

    // ★ The contrast case, and it is what keeps the two above from being a
    // verb that simply stopped working: a NON-default group still hides.
    let out = temp_out("layer-ok.pdf");
    let (code, stdout, err) = run(&[
        "layer-toggle",
        src.to_str().unwrap(),
        "--group",
        "1",
        "--hide",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "a non-default group hides: {err}");
    assert!(stdout.contains("visible=false"), "{stdout}");
    assert!(
        list(&out).contains("visible=false"),
        "and it stays hidden in the saved file: {}",
        list(&out)
    );

    // And SHOWING the default group is fine -- only hiding it is refused.
    let out = temp_out("layer-show.pdf");
    let (code, stdout, err) = run(&[
        "layer-toggle",
        src.to_str().unwrap(),
        "--group",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "showing the default group is not refused: {err}");
    assert!(stdout.contains("visible=true"), "{stdout}");

    assert_eq!(
        list(&src),
        before,
        "and the input document is untouched by any of it"
    );
}

/// ★★ **Authoring into a group that does not exist is refused — it used to
/// report the group it did NOT use.**
///
/// # What it did before, measured on the release binary
///
/// On a document whose only group is `0`:
///
/// ```text
///   dimension-add --group 99 ...
///   -> "dimension-add ... group=99 ... dim=1"   (exit 0)
///   -> dimension-list: dim 1 group=0
/// ```
///
/// **The success line named a group the ce dimension did not go into.**
///
/// `DimensionModel::add_dimension` falls back to the default group for an
/// unknown id, which is reasonable IN THE MODEL — a record whose `group`
/// resolved to nothing would be an orphan every reader would have to handle.
/// But it is a model invariant expressed as a silent substitution, and the
/// verb passed it through.
///
/// # Why it is worse than a wrong label
///
/// The group is the authority for scale, unit, precision and drafting
/// standard. A ce dimension that lands in the wrong group is **measured at
/// the wrong scale** — a wrong number on a drawing, from a mistyped id, with
/// a green exit code. This fixture's two groups differ by exactly 2x, so the
/// substitution is visible in the value rather than only in the id.
///
/// Third instance of the shape `Pass 176.0` and `Pass 178.0` fixed.
#[test]
fn authoring_into_an_unknown_group_is_refused_rather_than_silently_redirected() {
    let src = two_groups("addgrp");
    let before = list(&src);

    let out = temp_out("addgrp-out.pdf");
    let (code, stdout, err) = run(&[
        "dimension-add",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--kind",
        "linear",
        "--points",
        "100,300 300,300",
        "--group",
        "99",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        code, EDIT_REFUSED,
        "an unknown group is refused, not silently swapped for the default: {err}{stdout}"
    );
    assert!(!out.exists(), "a refusal writes no output file");
    assert_eq!(list(&src), before, "and the input document is untouched");

    // ★ The contrast case: a REAL non-default group still works, and the ce
    // dimension is measured at ITS scale. Without this the refusal above
    // would be indistinguishable from a verb that stopped accepting a
    // `--group` argument at all.
    let ok = temp_out("addgrp-ok.pdf");
    let (code, _, err) = run(&[
        "dimension-add",
        src.to_str().unwrap(),
        "--page",
        "1",
        "--kind",
        "linear",
        "--points",
        "100,300 300,300",
        "--group",
        "1",
        "-o",
        ok.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "a real group still works: {err}");
    let listing = list(&ok);
    assert!(
        listing.contains("group=1"),
        "the ce dimension belongs to the group that was named: {listing}"
    );
    assert!(
        listing.contains("value=\"2.500 m\""),
        "and is measured at THAT group's scale -- half the default's, which is \
         what the silent fallback used to get wrong: {listing}"
    );
}

/// ★ **`group-set-scale` was the LAST group verb accepting an unknown id**,
/// and it was found by probing all eight rather than by reading them.
///
/// # The sweep, and why it beat the reading
///
/// After three instances of `R235` were fixed by inspection, every
/// ce-dimension verb was driven with a bogus id against the shipped binary.
/// Seven refused; `group-set-scale` returned exit 0 with a success line
/// naming group 99 and changed nothing.
///
/// It had been *read* earlier in the same session and *asserted* to be
/// guarded — the assertion came from reading `set_group_style`, which is
/// guarded, and generalising. A claim about a caller is a measurement, and
/// that one was made and was wrong.
///
/// `DimensionModel::set_group_scale` returns `()` and is documented "No-op
/// for an unknown group", so it has no channel to refuse — `R235`'s shape
/// exactly, and the fourth instance of it.
///
/// # This test covers the whole family, not just the one that was broken
///
/// A test for `group-set-scale` alone would have to be written again for the
/// next verb added to the family. Driving all of them is what found this one.
#[test]
fn every_ce_dimension_group_verb_refuses_an_id_that_does_not_exist() {
    let src = two_groups("family");
    let before = list(&src);

    // (label, argv after the input path). Group 99 does not exist; the
    // fixture has 0 and 1.
    let cases: [(&str, Vec<&str>); 5] = [
        (
            "group-set-scale",
            vec!["group-set-scale", "--group", "99", "--ratio", "1:50"],
        ),
        (
            "group-set-standard",
            vec!["group-set-standard", "--group", "99", "--standard", "iso"],
        ),
        (
            "group-style",
            vec!["group-style", "--group", "99", "--text-height", "12"],
        ),
        (
            "group-rename",
            vec!["group-rename", "--group", "99", "--name", "x"],
        ),
        (
            "layer-toggle",
            vec!["layer-toggle", "--group", "99", "--hide"],
        ),
    ];

    for (label, args) in &cases {
        let out = temp_out(&format!("family-{label}.pdf"));
        let mut argv = vec![args[0], src.to_str().unwrap()];
        argv.extend(args[1..].iter().copied());
        argv.extend(["-o", out.to_str().unwrap()]);
        let (code, stdout, err) = run(&argv);
        assert_eq!(
            code, EDIT_REFUSED,
            "{label} must refuse a group that does not exist: {err}{stdout}"
        );
        assert!(
            !out.exists(),
            "{label} -- a refusal before any mutation writes no output file"
        );
    }

    assert_eq!(
        list(&src),
        before,
        "and the input document is untouched after all of them"
    );
}
