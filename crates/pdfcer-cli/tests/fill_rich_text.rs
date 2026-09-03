//! CLI-level tests for **filling a rich-text form field** — the
//! `fill-field --downgrade-rich-text` opt-in (ISO 32000-1 §12.7.3.4).
//!
//! # Why this file exists
//!
//! Until the flag landed, a rich-text field was **unfillable from the CLI at
//! all**. `fill-field` called `EditSession::fill_text_field`, which refuses
//! one, and never exposed `fill_text_field_downgrading_rich_text` — the verb
//! the GUI had been using for some time. The refusal message even told the
//! operator to *"use the explicit convert-to-plain-text fill instead"*,
//! naming a route the CLI did not have. That is the defect these tests pin.
//!
//! It survived because nothing tested it and because `docs/FEATURES.md`
//! asserted the **opposite** of the truth in both directions — that the GUI
//! refused and core/CLI did not (corrected in `aac321c`). A false
//! description and no test is how a whole surface goes missing.
//!
//! # The four assertions, and why these
//!
//! 1. **Without the flag, a rich-text field is REFUSED** and no output file
//!    is written. This is the default and must stay the default: writing
//!    `/V` while a live `/RV` remains makes conforming readers regenerate
//!    the appearance from the OLD text, so the document would display words
//!    the operator never typed.
//! 2. **With the flag, the fill succeeds and the field is genuinely
//!    converted** — bit 26 cleared, `/V` holding the new text. Asserted by
//!    reading the output back through `list-fields`, i.e. through pdfcer's
//!    own parser on a real file, not by inspecting bytes here. A byte grep
//!    would be actively WRONG: an incremental save appends, so the base
//!    revision's `/RV` bytes legitimately remain in the file while the
//!    document-level value is gone.
//! 3. **The conversion is disclosed BY NAME on stderr.** A count would tell
//!    the operator that something lost its formatting without saying which
//!    — the only question worth answering on a scripted run. stderr rather
//!    than stdout so a pipeline capturing stdout still shows a human.
//! 4. **The flag is INERT on a field with no formatting to lose.** It must
//!    not change the outcome, and must not emit the note, for a plain text
//!    field. This is the assertion that stops the flag from quietly becoming
//!    "route everything through the lossy path".
//!
//! # Contract with the fixture
//!
//! `fixtures/synthetic/forms/radio-choice-form.pdf` object 50 (`Notes`)
//! carries `/Ff 33554432`, a `/DS` default-style string, an `/RV` XHTML body
//! and a `/V` whose wording **deliberately differs** from the `/RV` wording
//! (`RICH ORIGINAL` vs `<b>RICH</b> <i>ORIGINAL</i>`). That difference is
//! load-bearing for test 2: with `/V` and `/RV` saying the same thing, a
//! writer that left `/RV` in place would be indistinguishable from one that
//! removed it.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// `exit::EDIT_REFUSED` — the file was readable and pdfcer declined the edit.
const EDIT_REFUSED: u8 = 9;

/// A self-deleting scratch directory (see `render_page.rs` for why this
/// project carries no `tempfile` dependency).
struct TempDir(PathBuf);

impl TempDir {
    /// The scratch directory seeded with a repo fixture as `in.pdf`.
    ///
    /// Copied rather than used in place because these tests write output
    /// beside their input and must never touch a committed fixture.
    fn seeded_with(tag: &str, rel: &str) -> (Self, PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-richtext-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic")
            .join(rel);
        let input = path.join("in.pdf");
        std::fs::copy(&src, &input).expect("could not copy fixture");
        (Self(path), input)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

fn code(out: &Output) -> u8 {
    u8::try_from(out.status.code().expect("process was killed by a signal")).unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout must be valid UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `Notes` row of `list-fields` for a given file.
fn notes_row(path: &std::path::Path) -> String {
    let out = run(&["list-fields", &path.display().to_string()]);
    assert_eq!(code(&out), 0, "list-fields failed: {}", stderr(&out));
    stdout(&out)
        .lines()
        .find(|l| l.contains(r#"name="Notes""#))
        .unwrap_or_else(|| panic!("no Notes row in:\n{}", stdout(&out)))
        .to_owned()
}

/// 1. Without the flag the fill is refused, and no output file appears.
///
/// The "no output file" half matters as much as the exit code: a refusal
/// that still produced a plausible-looking artefact is the worst outcome,
/// because the operator sees an error and a file and has to guess which
/// one is authoritative.
#[test]
fn rich_text_fill_is_refused_without_the_flag() {
    let (dir, input) = TempDir::seeded_with("refuse", "forms/radio-choice-form.pdf");
    let output = dir.join("out.pdf");

    let out = run(&[
        "fill-field",
        &input.display().to_string(),
        "--set",
        "Notes=plain replacement",
        "-o",
        &output.display().to_string(),
    ]);

    assert_eq!(
        code(&out),
        EDIT_REFUSED,
        "a rich-text field must be refused by default; stderr: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("rich"),
        "the refusal must say WHY, naming rich text: {}",
        stderr(&out)
    );
    assert!(
        !output.exists(),
        "a refused fill must not leave an output file behind"
    );
}

/// 2 + 3. With the flag the field is converted for real, and said so by name.
///
/// The input assertion is not decoration — it is what makes the output
/// assertion non-vacuous. If the fixture ever lost its `/Ff` bit 26, every
/// "the flag cleared it" check below would pass against a field that was
/// never rich text in the first place, which is exactly how a guard like
/// this goes quiet.
#[test]
fn the_flag_converts_the_field_and_names_it() {
    let (dir, input) = TempDir::seeded_with("convert", "forms/radio-choice-form.pdf");
    let output = dir.join("out.pdf");

    let before = notes_row(&input);
    assert!(
        before.contains("flags=0x2000000"),
        "fixture precondition: Notes must carry /Ff bit 26 (RichText): {before}"
    );
    assert!(
        before.contains(r#"value="RICH ORIGINAL""#),
        "fixture precondition: Notes starts with its original /V: {before}"
    );

    let out = run(&[
        "fill-field",
        &input.display().to_string(),
        "--set",
        "Notes=plain replacement",
        "--downgrade-rich-text",
        "-o",
        &output.display().to_string(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    // 3. Disclosed BY NAME, on stderr.
    let err = stderr(&out);
    assert!(
        err.contains("Notes") && err.contains("discarded"),
        "the conversion must be disclosed naming the field: {err}"
    );

    // 2. Converted for real, read back through pdfcer's own parser.
    let after = notes_row(&output);
    assert!(
        after.contains("flags=0x0"),
        "the RichText flag must be cleared: {after}"
    );
    assert!(
        after.contains(r#"value="plain replacement""#),
        "the new plain value must be stored: {after}"
    );

    // The field must now be an ordinary text field: a second fill with NO
    // flag has to succeed. Without this, "converted" could mean "still
    // refused, but with different flags", which is not a fill at all.
    let again = dir.join("again.pdf");
    let out = run(&[
        "fill-field",
        &output.display().to_string(),
        "--set",
        "Notes=second edit",
        "-o",
        &again.display().to_string(),
    ]);
    assert_eq!(
        code(&out),
        0,
        "after conversion the field must fill plainly; stderr: {}",
        stderr(&out)
    );
}

/// 4. The flag is inert on a field that has no formatting to lose.
///
/// `demo-form.pdf`'s `FullName` is a plain `/Tx`. Passing the flag must
/// change nothing about the outcome and must emit no note — otherwise the
/// disclosure becomes noise operators learn to ignore, and the lossy verb
/// silently becomes the default path for every text field.
#[test]
fn the_flag_is_inert_on_a_plain_text_field() {
    let (dir, input) = TempDir::seeded_with("inert", "forms/demo-form.pdf");

    let with = dir.join("with.pdf");
    let out_with = run(&[
        "fill-field",
        &input.display().to_string(),
        "--set",
        "FullName=hello",
        "--downgrade-rich-text",
        "-o",
        &with.display().to_string(),
    ]);
    assert_eq!(code(&out_with), 0, "stderr: {}", stderr(&out_with));
    assert!(
        !stderr(&out_with).contains("discarded"),
        "no conversion note may appear for a plain field: {}",
        stderr(&out_with)
    );

    let without = dir.join("without.pdf");
    let out_without = run(&[
        "fill-field",
        &input.display().to_string(),
        "--set",
        "FullName=hello",
        "-o",
        &without.display().to_string(),
    ]);
    assert_eq!(code(&out_without), 0, "stderr: {}", stderr(&out_without));

    // Same stored value either way. Compared through `list-fields` rather
    // than by hashing the two files, because an incremental save embeds a
    // changing `/ID` and timestamps — a byte comparison would fail for
    // reasons that have nothing to do with the flag.
    //
    // Only the `field ` rows are compared: `list-fields` ends with a summary
    // line that echoes the INPUT PATH, so the full stdout differs between
    // `with.pdf` and `without.pdf` no matter what the flag did. Comparing
    // whole output here fails for a reason that has nothing to do with the
    // property under test — which is how it failed on first run.
    let rows = |p: &std::path::Path| -> Vec<String> {
        stdout(&run(&["list-fields", &p.display().to_string()]))
            .lines()
            .filter(|l| l.starts_with("field "))
            .map(ToOwned::to_owned)
            .collect()
    };
    assert_eq!(
        rows(&with),
        rows(&without),
        "the flag must not change the outcome for a plain text field"
    );
}

/// `list-fields` says WHAT the formatting is, not merely that it exists.
///
/// The row's `rich=<n>runs` token answers "does this field have
/// formatting". `--rich-text` answers "what would I lose by downgrading
/// it", which is the question that actually precedes a decision — and
/// until this flag existed, pdfcer could parse the answer and had no way
/// to say it.
///
/// The middle run is the load-bearing assertion, as everywhere else in
/// this feature: it is a bare space inside no styling element, so it
/// carries `/DS`'s size, family and colour and nothing of its own. A
/// reader that only reported styles where an element had set one would
/// show it as unstyled, and a flattening reader would not show it at all.
#[test]
fn list_fields_describes_rich_text_run_by_run() {
    let (_dir, input) = TempDir::seeded_with("describe", "forms/radio-choice-form.pdf");

    // Without the flag: the row states there IS formatting, in three runs.
    let plain = stdout(&run(&["list-fields", &input.display().to_string()]));
    let row = plain
        .lines()
        .find(|l| l.contains(r#"name="Notes""#))
        .expect("a Notes row");
    assert!(row.contains("rich=3runs"), "{row}");

    // A field with no /RV must say so, not be silently blank.
    let colour = plain
        .lines()
        .find(|l| l.contains(r#"name="Colour""#))
        .expect("a Colour row");
    assert!(colour.contains("rich=-"), "{colour}");

    // With the flag: each run, its text, and its resolved style.
    let out = run(&["list-fields", &input.display().to_string(), "--rich-text"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    let detail = stdout(&out);

    assert!(
        detail.contains(r#"run 0 p=0 text="RICH""#),
        "runs must be numbered and quoted:\n{detail}"
    );
    assert!(
        detail.contains("weight=700(bold)"),
        "the weight must name the keyword an operator recognises:\n{detail}"
    );
    assert!(
        detail.contains(r#"run 2 p=0 text="ORIGINAL""#) && detail.contains("italic"),
        "the italic half must be identified:\n{detail}"
    );

    // The bare space between them carries /DS and only /DS.
    let space = detail
        .lines()
        .find(|l| l.contains(r#"run 1 p=0 text=" ""#))
        .expect("the space between the two styled runs is its own run");
    assert!(
        space.contains("12pt") && space.contains("Helvetica") && space.contains("#FF0000"),
        "/DS must reach the unstyled run too: {space}"
    );
    assert!(
        !space.contains("bold") && !space.contains("italic"),
        "no style may leak into the space between two styled runs: {space}"
    );

    // The colour is reported as the #rrggbb the file wrote, not as the
    // DeviceRGB triple the model holds.
    assert!(
        !detail.contains("1.0, 0.0, 0.0"),
        "raw DeviceRGB components must not reach the operator:\n{detail}"
    );
}
