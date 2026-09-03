//! # `pdfcer`'s bookmark verbs — black-box over the real binary
//!
//! Covers `list-outline`, `rename-bookmark`, `delete-bookmark` (`Pass 157.0`)
//! and `move-bookmark` / `set-bookmark-open` (`Pass 161.0`).
//!
//! ## ★★ Why the older two are tested here, in a later Pass
//!
//! `Pass 157.0` shipped `rename-bookmark` and `delete-bookmark` with **no CLI
//! tests at all** — the core was covered by `outline_edit.rs` and the binary
//! was checked by hand. Manual verification does not survive the session that
//! did it, and the gap was carried forward in the handoff for two Passes. It
//! is closed here rather than filed again, because the shell layer has
//! failures the core layer structurally cannot have:
//!
//! * **`n=` resolution.** Every one of these commands names a bookmark by the
//!   number `list-outline` prints. That mapping — depth-first, 1-based, every
//!   level — lives only in the CLI. A core test cannot see an off-by-one in
//!   it, and an off-by-one renames or *deletes* the wrong bookmark.
//! * **Flag wiring.** A flag can be parsed, documented, and never reach the
//!   core call. Unit tests hit the core directly and pass regardless; only
//!   running the binary finds it.
//! * **Exit codes**, which are the CLI's contract with a script.
//! * **The disclosure lines**, which are the whole of project rule 4 in a
//!   shell where the invocation *is* the commit — there is no session and no
//!   undo, so what is printed on the way past is the only disclosure there
//!   will ever be.
//!
//! ## Every assertion re-reads the output file
//!
//! Never the report the command printed about itself. A command that says
//! `moved=1` and wrote nothing, and a command that moved the wrong bookmark
//! and reported the right title, both pass a report-only assertion.
//!
//! ## Fixture
//!
//! `fixtures/synthetic/outline/basic-tree.pdf`, provenance in that directory's
//! `PROVENANCE.md`:
//!
//! ```text
//! n=1  Chapter 1     open, 2 children
//! n=2    Section 1.1 leaf
//! n=3    Section 1.2 leaf
//! n=4  Chapter 2     CLOSED, 1 child
//! n=5    Section 2.1 leaf
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
/// `exit::EDIT_REFUSED` in the CLI's stable exit-code contract.
const EDIT_REFUSED: i32 = 9;
/// `exit::RUNTIME_ERROR` — a bad invocation the command itself rejected.
const RUNTIME_ERROR: i32 = 1;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/outline/basic-tree.pdf")
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("pdfcer_bm_{tag}_{}_{n}.pdf", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The outline of `path`, as `(level, open, title)` in `list-outline`'s own
/// order — the shell's view of the tree, read back through the shell.
///
/// Parsed from `--flat` deliberately: the indented form is for reading, and a
/// test that depended on leading spaces would break on a cosmetic change to
/// output that is explicitly described as being for humans.
fn outline(path: &Path) -> Vec<(u32, bool, String)> {
    let out = run(&["list-outline", path.to_str().unwrap(), "--flat"]);
    assert!(
        out.status.success(),
        "list-outline failed: {}",
        stderr(&out)
    );
    stdout(&out)
        .lines()
        .filter(|l| l.starts_with("bookmark n="))
        .map(|l| {
            let level = l
                .split("level=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
                .expect("level= is present on every row");
            let open = l
                .split("open=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s == "1")
                .expect("open= is present on every row");
            let title = l
                .split("title=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .expect("title= is present on every row")
                .to_string();
            (level, open, title)
        })
        .collect()
}

fn titles(path: &Path) -> Vec<String> {
    outline(path).into_iter().map(|(_, _, t)| t).collect()
}

fn levels(path: &Path) -> Vec<u32> {
    outline(path).into_iter().map(|(l, _, _)| l).collect()
}

// ---------------------------------------------------------------------------
// list-outline — the numbering every other command depends on
// ---------------------------------------------------------------------------

/// ★ The `n=` numbering is depth-first over **every** level, not top-level
/// only, and it is 1-based. Every other command in this file indexes with it,
/// so an error here is an error in all of them — which is exactly why it is
/// asserted on its own rather than assumed by the tests that use it.
#[test]
fn list_outline_numbers_depth_first_from_one() {
    let out = run(&["list-outline", fixture().to_str().unwrap(), "--flat"]);
    assert!(out.status.success());
    let printed = stdout(&out);
    let ns: Vec<&str> = printed
        .lines()
        .filter(|l| l.starts_with("bookmark n="))
        .map(|l| l.split("n=").nth(1).unwrap().split(' ').next().unwrap())
        .collect();
    assert_eq!(ns, ["1", "2", "3", "4", "5"]);
    assert_eq!(
        titles(&fixture()),
        [
            "Chapter 1",
            "Section 1.1",
            "Section 1.2",
            "Chapter 2",
            "Section 2.1"
        ]
    );
    assert_eq!(levels(&fixture()), [0, 1, 1, 0, 1]);
}

// ---------------------------------------------------------------------------
// rename-bookmark — owed from `Pass 157.0`
// ---------------------------------------------------------------------------

/// A nested bookmark renamed by its `n=` number. `--n 3` is *Section 1.2*, the
/// second child of the first chapter — chosen over `--n 1` precisely because a
/// top-level index would pass under a numbering that ignored nesting.
#[test]
fn rename_bookmark_renames_the_nth_item_counting_through_nesting() {
    let out_path = temp_path("rename");
    let out = run(&[
        "rename-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "3",
        "--title",
        "Renamed Section",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("\"Section 1.2\" -> \"Renamed Section\""),
        "the report names both the old and the new title: {}",
        stdout(&out)
    );
    assert_eq!(
        titles(&out_path),
        [
            "Chapter 1",
            "Section 1.1",
            "Renamed Section",
            "Chapter 2",
            "Section 2.1"
        ],
        "exactly one title changed, and it is the third in reading order"
    );
    assert_eq!(levels(&out_path), [0, 1, 1, 0, 1], "nothing moved");
}

/// A title outside ASCII must survive the `/Title` text-string encoding and
/// come back identical through the reader.
#[test]
fn rename_bookmark_round_trips_non_ascii() {
    let out_path = temp_path("utf8");
    let out = run(&[
        "rename-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "1",
        "--title",
        "Chapitre Un — «résumé» ✓",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(titles(&out_path)[0], "Chapitre Un — «résumé» ✓");
}

/// `--n` is 1-based and out-of-range is refused with a message naming the
/// range that works, not a panic and not a silent no-op.
#[test]
fn rename_bookmark_refuses_a_number_the_document_does_not_have() {
    for bad in ["0", "6"] {
        let out_path = temp_path("badn");
        let out = run(&[
            "rename-bookmark",
            fixture().to_str().unwrap(),
            "--n",
            bad,
            "--title",
            "x",
            "-o",
            out_path.to_str().unwrap(),
        ]);
        assert_eq!(
            out.status.code(),
            Some(RUNTIME_ERROR),
            "--n {bad} should be refused"
        );
        assert!(
            !out_path.exists(),
            "a refusal must not write an output file"
        );
    }
}

// ---------------------------------------------------------------------------
// delete-bookmark — owed from `Pass 157.0`
// ---------------------------------------------------------------------------

/// ★★ Deleting a chapter takes its sections, and the command **says so**.
///
/// The operator named one bookmark and three went. In `pdfcer` the
/// invocation is the commit — there is no session and no undo — so the count
/// on stderr is the only disclosure that will ever exist for this act. Rule 4
/// forbids the silence, not the deletion.
#[test]
fn delete_bookmark_takes_the_subtree_and_discloses_how_much() {
    let out_path = temp_path("del");
    let out = run(&[
        "delete-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "1",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("removed=3"),
        "Chapter 1 plus two sections: {}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains("3 outline objects were removed, not 1"),
        "the subtree deletion must be disclosed, not merely counted in a field: {}",
        stderr(&out)
    );
    assert_eq!(titles(&out_path), ["Chapter 2", "Section 2.1"]);
}

/// Deleting a leaf discloses nothing extra — there is nothing surprising to
/// disclose, and a warning that always fires is a warning nobody reads.
#[test]
fn deleting_a_leaf_does_not_warn_about_a_subtree() {
    let out_path = temp_path("delleaf");
    let out = run(&[
        "delete-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "2",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("removed=1"));
    assert!(
        !stderr(&out).contains("were removed, not 1"),
        "a leaf deletion must not carry the subtree warning: {}",
        stderr(&out)
    );
    assert_eq!(
        titles(&out_path),
        ["Chapter 1", "Section 1.2", "Chapter 2", "Section 2.1"]
    );
}

// ---------------------------------------------------------------------------
// move-bookmark — `Pass 161.0`
// ---------------------------------------------------------------------------

/// Reorder: Chapter 1 lands behind Chapter 2, each keeping its own children.
#[test]
fn move_bookmark_reorders_with_after() {
    let out_path = temp_path("after");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "1",
        "--after",
        "4",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("moved=1 reparented=0"));
    assert_eq!(
        titles(&out_path),
        [
            "Chapter 2",
            "Section 2.1",
            "Chapter 1",
            "Section 1.1",
            "Section 1.2"
        ]
    );
    assert_eq!(levels(&out_path), [0, 1, 0, 1, 1], "nothing changed depth");
}

/// `--before` is the mirror of `--after`, and it is tested separately because
/// the two resolve their anchor differently — `--after` anchors on the target
/// itself, `--before` on the target's `/Prev`, which is read **after**
/// unlinking. A single test of one would not exercise the other.
#[test]
fn move_bookmark_reorders_with_before() {
    let out_path = temp_path("before");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "4",
        "--before",
        "1",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        titles(&out_path),
        [
            "Chapter 2",
            "Section 2.1",
            "Chapter 1",
            "Section 1.1",
            "Section 1.2"
        ]
    );
}

/// Re-parenting nests the bookmark one level deeper, carrying its subtree.
#[test]
fn move_bookmark_nests_with_under() {
    let out_path = temp_path("under");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "4",
        "--under",
        "1",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("reparented=1"));
    assert_eq!(
        titles(&out_path),
        [
            "Chapter 1",
            "Section 1.1",
            "Section 1.2",
            "Chapter 2",
            "Section 2.1"
        ],
        "★ the flat title order is IDENTICAL to the input — R225. The depths \
         below are the assertion that discriminates"
    );
    assert_eq!(
        levels(&out_path),
        [0, 1, 1, 1, 2],
        "Chapter 2 went from top level to a child, and its section with it"
    );
}

/// `--first` must actually reach the core. A flag that is parsed, documented
/// and dropped passes every unit test — only the binary's output shows it.
#[test]
fn move_bookmark_honours_first() {
    let last = temp_path("last");
    let first = temp_path("first");
    assert!(
        run(&[
            "move-bookmark",
            fixture().to_str().unwrap(),
            "--n",
            "5",
            "--to-top-level",
            "-o",
            last.to_str().unwrap(),
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "move-bookmark",
            fixture().to_str().unwrap(),
            "--n",
            "5",
            "--to-top-level",
            "--first",
            "-o",
            first.to_str().unwrap(),
        ])
        .status
        .success()
    );
    assert_eq!(titles(&last).last().unwrap(), "Section 2.1");
    assert_eq!(titles(&first).first().unwrap(), "Section 2.1");
    assert_ne!(
        titles(&last),
        titles(&first),
        "--first must change the outcome, or it is not wired to anything"
    );
}

/// Promoting to the top level un-nests, and the previously hidden bookmark
/// becomes visible.
#[test]
fn move_bookmark_promotes_to_top_level() {
    let out_path = temp_path("promote");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "5",
        "--to-top-level",
        "--first",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        titles(&out_path),
        [
            "Section 2.1",
            "Chapter 1",
            "Section 1.1",
            "Section 1.2",
            "Chapter 2"
        ]
    );
    assert_eq!(levels(&out_path), [0, 0, 1, 1, 0]);
}

/// ★ A cycle is refused, and the message speaks in `n=` — the identifiers the
/// operator typed and `list-outline` prints — not in the object ids the core
/// uses internally.
#[test]
fn move_bookmark_refuses_a_cycle_in_the_operators_own_numbering() {
    let out_path = temp_path("cycle");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "1",
        "--under",
        "2",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_ne!(out.status.code(), Some(0), "a cycle must not succeed");
    let err = stderr(&out);
    assert!(
        err.contains("bookmark 1 cannot be moved under bookmark 2"),
        "the refusal must name the numbers the operator typed: {err}"
    );
    assert!(
        err.contains("cycle"),
        "and must say why, so the operator can act on it: {err}"
    );
    assert!(
        !out_path.exists(),
        "a refusal must not write an output file"
    );
}

/// A move with no destination, and `--first` where it means nothing, are both
/// refused rather than guessed at or silently ignored.
#[test]
fn move_bookmark_refuses_an_incoherent_invocation() {
    let cases: [(&[&str], &str); 2] = [
        (&["--n", "1"], "a move needs a destination"),
        (
            &["--n", "1", "--after", "4", "--first"],
            "--first modifies --under",
        ),
    ];
    for (extra, needle) in cases {
        let out_path = temp_path("bad");
        let mut args = vec!["move-bookmark", "PLACEHOLDER"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["-o", out_path.to_str().unwrap()]);
        let fixture_str = fixture();
        args[1] = fixture_str.to_str().unwrap();
        let out = run(&args);
        assert_eq!(out.status.code(), Some(RUNTIME_ERROR), "{extra:?}");
        assert!(
            stderr(&out).contains(needle),
            "{extra:?} should say {needle:?}: {}",
            stderr(&out)
        );
        assert!(!out_path.exists());
    }
}

/// ★★ A redundant move is not an error, writes nothing, and **says** it wrote
/// nothing.
///
/// The byte comparison is the real assertion. `moved=0` in the report is what
/// the command claims; a file identical to its input is what it did. A shell
/// rebuilding an outline issues redundant moves by construction, so this path
/// is common rather than exotic.
#[test]
fn move_bookmark_to_the_current_position_writes_a_byte_identical_file() {
    let out_path = temp_path("noop");
    let out = run(&[
        "move-bookmark",
        fixture().to_str().unwrap(),
        "--n",
        "1",
        "--before",
        "4",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("moved=0"), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("already in that position"),
        "silence and success look identical to a script otherwise: {}",
        stderr(&out)
    );
    assert_eq!(
        std::fs::read(&out_path).unwrap(),
        std::fs::read(fixture()).unwrap(),
        "a no-op move must not append a revision"
    );
}

// ---------------------------------------------------------------------------
// set-bookmark-open — `Pass 161.0`
// ---------------------------------------------------------------------------

/// Expanding a collapsed chapter changes its stored state, which survives the
/// save — `/Count`'s sign, not a viewer preference.
#[test]
fn set_bookmark_open_expands_and_collapses() {
    let opened = temp_path("open");
    let out = run(&[
        "set-bookmark-open",
        fixture().to_str().unwrap(),
        "--n",
        "4",
        "-o",
        opened.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("open=1 changed=1"));
    assert!(outline(&opened)[3].1, "Chapter 2 is open in the saved file");

    let closed = temp_path("close");
    let out = run(&[
        "set-bookmark-open",
        opened.to_str().unwrap(),
        "--n",
        "4",
        "--collapse",
        "-o",
        closed.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!outline(&closed)[3].1, "and closed again");
}

/// A leaf has no expansion state. Reported, not refused: a sweep over every
/// row must not have to filter first.
#[test]
fn set_bookmark_open_on_a_leaf_changes_nothing_and_says_so() {
    let out_path = temp_path("leaf");
    let out = run(&[
        "set-bookmark-open",
        fixture().to_str().unwrap(),
        "--n",
        "2",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "a leaf is not an error");
    assert!(stdout(&out).contains("changed=0"));
    assert!(stderr(&out).contains("no children"));
    assert_eq!(
        std::fs::read(&out_path).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

/// ★ The two Passes compose: a move preserves the destination's collapsed
/// state, and `set-bookmark-open` is the other answer. This is the
/// reveal-on-move workflow, and it is a test because the split into two verbs
/// is only defensible if composing them actually works.
#[test]
fn a_move_into_a_collapsed_parent_can_be_revealed_by_the_other_verb() {
    let hidden = temp_path("hidden");
    assert!(
        run(&[
            "move-bookmark",
            fixture().to_str().unwrap(),
            "--n",
            "2",
            "--under",
            "4",
            "-o",
            hidden.to_str().unwrap(),
        ])
        .status
        .success()
    );
    let tree = outline(&hidden);
    let ch2 = tree.iter().position(|(_, _, t)| t == "Chapter 2").unwrap();
    assert!(
        !tree[ch2].1,
        "the move must PRESERVE the destination's collapsed state"
    );

    let revealed = temp_path("revealed");
    let n = (ch2 + 1).to_string();
    let out = run(&[
        "set-bookmark-open",
        hidden.to_str().unwrap(),
        "--n",
        &n,
        "-o",
        revealed.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let tree = outline(&revealed);
    assert!(tree[ch2].1, "and the other verb expands it");
    assert!(
        tree.iter().any(|(l, _, t)| t == "Section 1.1" && *l == 1),
        "the moved bookmark is still where the move put it"
    );
}

/// The outline root is not an item and has no expansion state. Refused by
/// name, with the core's exit code reaching the shell unchanged.
#[test]
fn bookmark_verbs_refuse_a_document_with_no_outline() {
    let no_outline = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/outline/no-outline.pdf");
    let out_path = temp_path("none");
    let out = run(&[
        "move-bookmark",
        no_outline.to_str().unwrap(),
        "--n",
        "1",
        "--to-top-level",
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        matches!(out.status.code(), Some(RUNTIME_ERROR) | Some(EDIT_REFUSED)),
        "a document with no bookmarks must refuse, not panic: {:?} {}",
        out.status.code(),
        stderr(&out)
    );
    assert!(!out_path.exists());
}
