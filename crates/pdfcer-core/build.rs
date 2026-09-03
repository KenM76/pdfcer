//! Build-time provenance capture for pdfcer — `Pass 101.0`.
//!
//! # What this exists for
//!
//! The operator asked, 2026-08-18: *"whenever you build a new version can
//! you include the build date and time, and also include the build
//! revision, date, and time for the version of iccce used in the version?"*
//!
//! This script answers the first half. It emits four `cargo::rustc-env`
//! variables that [`pdfcer_core::build`] reads with `env!`, so the values are
//! baked into the binary at compile time and cost nothing at run time.
//!
//! | variable | meaning |
//! |---|---|
//! | `PDFCER_BUILD_TIMESTAMP` | when this binary was built, RFC 3339 UTC |
//! | `PDFCER_BUILD_REVISION` | `git describe --tags --always --dirty`, or `unknown` |
//! | `PDFCER_BUILD_COMMIT_TIMESTAMP` | the committer date of that revision, RFC 3339 UTC, or `unknown` |
//! | `PDFCER_ICCCE_PROVENANCE` | see "the second half" below |
//!
//!
//! # ★ The second half of the request, and the six days it was answered wrongly
//!
//! pdfcer **does depend on `iccce`**, as of `Pass 199.2` (`3194f1b`):
//! `iccce-profile` and `iccce-cmm`, git dependencies pinned to tag `v0.3.0`
//! in `crates/pdfcer-render/Cargo.toml`. So there *is* a "version of iccce
//! used in this build" to report, and this script reports it — version,
//! pin, resolved revision, and that revision's commit date.
//!
//! ## What it read before, and why the correction is worth the words
//!
//! From `Pass 101.0` to `Pass 223.0` this emitted the literal string
//! `not-linked-yet`, together with prose explaining that the integration
//! was pending. Every word of that was true when written. `Pass 199.2`
//! made it false, and it went on being printed for six days — in the one
//! output surface whose entire purpose is to be believed without checking.
//!
//! ★★ **It did not self-correct, and the reason is the transferable part.**
//! The old `iccce_provenance` read `DEP_ICCCE_PROVENANCE`, an environment
//! variable Cargo sets only for a dependency that declares a `links` key.
//! `iccce` declares none. The function's own doc comment promised *"this
//! begins reporting the moment that becomes true"* — the moment came and
//! nothing happened, **because the mechanism it was waiting for is not the
//! mechanism that arrived.**
//!
//! From the outside those are indistinguishable: a detector waiting on a
//! signal its subject never emits looks exactly like a subject that has not
//! arrived. Which is this project's own recurring lesson — *when a
//! measurement looks clean, ask what the instrument cannot see* — landing
//! on a self-describing string rather than on a renderer.
//!
//! ## Why `Cargo.lock` is a legitimate source and a sibling checkout is not
//!
//! The old prose warned, correctly, that reading `D:\Dev\iccce`'s
//! `git describe` would answer *"which iccce is on this machine"* while
//! appearing to answer *"which iccce is in this binary"*. **That argument
//! still stands and this does not violate it.** `Cargo.lock` is the
//! *resolved* graph — the exact version and revision `rustc` is about to
//! compile. It answers the second question directly.
//!
//! The commit **date** is then looked up by that revision in Cargo's own
//! bare mirror under `$CARGO_HOME/git/db`. A repository is used there as a
//! lookup table keyed by a revision that came from the lock, not as a
//! source of truth about what is linked — see `iccce_commit_time`.
//!
//! ## The caveat, stated rather than buried
//!
//! `iccce` is `pdfcer-render`'s dependency and this script runs for
//! `pdfcer-core`, which does not depend on it. The lock is workspace-wide,
//! so inside this workspace the stamp is right for every shipped binary
//! (all of them link `pdfcer-render`). A project depending on `pdfcer-core`
//! alone finds no entry and gets `not-linked` — the truth for that build.
//!
//! # Reproducibility — the trade this makes, stated rather than buried
//!
//! Embedding a build timestamp makes builds **non-reproducible by
//! construction**: two builds of byte-identical source produce different
//! binaries. That is inherent in what was asked for, not a shortcoming of
//! how it is done.
//!
//! The standard escape hatch is honoured: if `SOURCE_DATE_EPOCH` is set in
//! the environment, it is used instead of the wall clock, which is the
//! convention reproducible-build systems already drive. So the capability
//! and the property remain simultaneously available, and choosing between
//! them stays the operator's call.
//!
//! # Failure behaviour
//!
//! Every git lookup can fail — no `git` on PATH, a source tarball with no
//! `.git`, a shallow clone with no tags. Each failure yields the literal
//! `unknown` rather than a plausible-looking substitute. A version banner
//! that guesses is worse than one that admits it does not know, because a
//! wrong revision is acted on and a missing one is questioned.
//!
//! ## ★ One operational consequence, so it is not discovered from a release
//!
//! `actions/checkout@v4` defaults to a **depth-1** clone, which has no tags
//! and no history — so a binary built by CI as things stand today would
//! report `revision: unknown`. That is harmless for the CI jobs, which test
//! rather than ship, and pdfcer's releases are built locally where the full
//! history is present.
//!
//! But if a release build ever moves into CI, that workflow needs
//! `fetch-depth: 0` or the shipped binary will not be able to say what it
//! is. Recorded here rather than in the workflow because this is the file
//! whose behaviour explains it, and because the failure is silent: the build
//! succeeds, and only the banner is empty.

use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run when HEAD moves or the index changes, so a rebuild after a
    // commit does not keep reporting the previous revision. Without these,
    // Cargo would cache this script's output against the source files
    // alone — which do not change when you commit them.
    let git_dir = locate_git_dir();
    if let Some(dir) = &git_dir {
        println!("cargo::rerun-if-changed={}/HEAD", dir.display());
        println!("cargo::rerun-if-changed={}/index", dir.display());
        // A branch's own ref file, so switching branches is noticed too.
        let refs = dir.join("refs");
        if refs.is_dir() {
            println!("cargo::rerun-if-changed={}", refs.display());
        }
    }
    println!("cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo::rerun-if-changed=src/civil_time.rs");

    println!("cargo::rustc-env=PDFCER_BUILD_TIMESTAMP={}", build_time());
    println!(
        "cargo::rustc-env=PDFCER_BUILD_REVISION={}",
        git(&["describe", "--tags", "--always", "--dirty"])
    );
    println!(
        "cargo::rustc-env=PDFCER_BUILD_COMMIT_TIMESTAMP={}",
        commit_time()
    );
    println!(
        "cargo::rustc-env=PDFCER_ICCCE_PROVENANCE={}",
        iccce_provenance()
    );
}

/// The build's wall-clock time as RFC 3339 UTC, or `SOURCE_DATE_EPOCH` when
/// the environment supplies one.
///
/// Formatted by hand from a Unix timestamp rather than by pulling in a date
/// crate: a build script's dependencies are compiled for the host on every
/// clean build, and a calendar conversion is about thirty lines. See
/// [`format_rfc3339_utc`] for the conversion and its one real subtlety.
fn build_time() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
        });
    format_rfc3339_utc(secs)
}

// The calendar arithmetic is SHARED with the crate rather than copied into
// this script, and lives in `src/civil_time.rs` -- read that file for why,
// and for the tests that pin it. A build script cannot depend on the crate
// it builds, and a build script's own `#[cfg(test)]` module is never run by
// `cargo test`, so arithmetic that lived only here would be arithmetic
// nobody could assert.
include!("src/civil_time.rs");

/// The committer date of `HEAD`, as RFC 3339 **UTC**.
///
/// Read as a Unix timestamp (`%ct`) and formatted by the same function that
/// formats the build time, rather than taken from git's own `%cI`.
///
/// `%cI` is strict ISO 8601 but carries the **committer's local offset**, so
/// the two timestamps in the stamp would be in different time zones — and
/// the whole reason both are printed is so they can be compared at a glance
/// (how stale was the source when this was built?). Two instants in
/// different zones cannot be compared at a glance; they can only be
/// compared carefully, which is the same as not being compared.
fn commit_time() -> String {
    let raw = git(&["log", "-1", "--format=%ct"]);
    raw.parse::<i64>()
        .map_or_else(|_| "unknown".to_owned(), format_rfc3339_utc)
}

/// Run `git` with `args` in the manifest's directory, trimmed; `unknown` on
/// any failure at all.
fn git(args: &[&str]) -> String {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The `.git` directory for this workspace, if there is one.
///
/// Walks up from the manifest rather than assuming a fixed depth, so this
/// keeps working if the crate moves within the workspace. Handles the
/// worktree case, where `.git` is a *file* containing a `gitdir:` pointer —
/// which matters here because this project verifies changes against git
/// worktrees, and a build script that silently stopped re-running inside one
/// would report the wrong revision precisely during a comparison.
fn locate_git_dir() -> Option<std::path::PathBuf> {
    let mut dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").ok()?).to_path_buf();
    loop {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let path = text.trim().strip_prefix("gitdir:")?.trim();
            return Some(dir.join(path));
        }
        if !dir.pop() {
            return None;
        }
    }
}
/// The `iccce` build linked into this one, read from the workspace
/// `Cargo.lock`.
///
/// # ★★ Why `Cargo.lock`, when the doc this replaced forbade reading a
/// sibling checkout
///
/// The previous version of this function returned the literal
/// `"not-linked-yet"` and argued — correctly — that stamping the sibling
/// checkout's `git describe` would answer *"which iccce is on this
/// machine"* while appearing to answer *"which iccce is in this binary"*.
/// **That argument still stands, and `Cargo.lock` is not an instance of
/// it.** The lock file is the *resolved* dependency graph: the exact
/// version and the exact git revision `rustc` is about to compile into
/// this build. It answers the second question directly, which is why the
/// old objection does not transfer.
///
/// # ★ It reports what the WORKSPACE links, not what `pdfcer-core` links
///
/// This is the honesty caveat, and it is the reason the old mechanism
/// could never have worked. `iccce-profile` and `iccce-cmm` are
/// dependencies of **`pdfcer-render`**; `pdfcer-core` does not depend on
/// either, and this build script runs for `pdfcer-core`. So:
///
/// - Inside the workspace — every shipped binary (`pdfcer`,
///   `pdfce-gui`) links `pdfcer-render`, so the lock's answer is the
///   binary's answer.
/// - Outside it — a project depending on `pdfcer-core` alone has no
///   `iccce` in its graph, finds no entry, and gets `not-linked`, which
///   is the truth for that build.
///
/// # Why the old mechanism never fired, recorded so it is not rebuilt
///
/// It read `DEP_ICCCE_PROVENANCE`, an environment variable Cargo sets
/// only for a dependency that declares a `links` key in its manifest.
/// `iccce` declares none. Its doc comment promised *"this begins
/// reporting the moment that becomes true"* — the moment came in
/// `Pass 199.2` and it did not begin reporting, **because the mechanism
/// it was waiting for is not the mechanism that arrived.** A detector
/// waiting on a signal its subject never emits is indistinguishable, from
/// the outside, from a subject that never arrived.
///
/// # Output shape
///
/// - `0.3.0 (rev a4d9003b)` — a git dependency pinned to a revision, which
///   is how iccce is pinned since 2026-09-02 (decision 123). The pin and the
///   resolved revision are the same number, so it is printed once.
/// - `0.3.0 (tag v0.3.0, a4d9003b)` — pinned by tag: the pin the manifest
///   asked for, then the revision the lock resolved it to.
/// - `0.3.0 (branch main, a4d9003b)` — pinned by branch, same shape.
/// - `0.3.0 (git a4d9003b)` — a git dependency with no query at all.
/// - `0.3.0 (registry)` / `0.3.0 (path)` — if it ever comes from
///   crates.io or a path override.
/// - `not-linked` — no `iccce` package in the lock.
/// - `disagreement: …` — the `iccce-*` packages resolved to **different**
///   versions or sources. Reported rather than reduced to one of them:
///   two halves of one colour engine at different revisions is a real
///   defect, and picking a winner would hide it in the one output whose
///   job is to be believed.
///
/// Never `unknown` on a parse failure that finds no entry — the absence
/// of an entry *is* `not-linked`, and inventing a third state for "the
/// file was there but I could not read it" would need the parser to be
/// able to fail, which a line scan over three keys cannot.
fn iccce_provenance() -> String {
    let Some(lock) = locate_lockfile() else {
        return "not-linked".to_owned();
    };
    println!("cargo::rerun-if-changed={}", lock.display());
    let Ok(text) = std::fs::read_to_string(&lock) else {
        return "not-linked".to_owned();
    };

    // A line scan rather than a TOML parser, for the same reason the rest
    // of this script shells out to `git` instead of linking `gix`: a build
    // script's dependencies are compiled for the host on every clean
    // build, and `Cargo.lock`'s `[[package]]` blocks are three flat
    // string keys with no nesting, no arrays-of-tables inside them and no
    // multi-line strings. There is nothing here a parser would get right
    // that this gets wrong.
    let mut found: Vec<(String, String, String)> = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut source = String::new();
    let mut flush = |name: &mut String, version: &mut String, source: &mut String| {
        if name.starts_with("iccce") && !version.is_empty() {
            found.push((name.clone(), version.clone(), source.clone()));
        }
        name.clear();
        version.clear();
        source.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut source);
        } else if let Some(value) = unquote(line, "name") {
            name = value;
        } else if let Some(value) = unquote(line, "version") {
            version = value;
        } else if let Some(value) = unquote(line, "source") {
            source = value;
        }
    }
    flush(&mut name, &mut version, &mut source);

    let Some((_, first_version, first_source)) = found.first() else {
        return "not-linked".to_owned();
    };
    // Every `iccce-*` crate is published from one repository at one tag,
    // so a disagreement means something went wrong upstream of this
    // build. See the doc comment for why it is surfaced rather than
    // resolved.
    if let Some((odd_name, odd_version, _)) = found
        .iter()
        .find(|(_, v, s)| v != first_version || s != first_source)
    {
        return format!(
            "disagreement: {} of {} iccce crate(s) differ (e.g. {odd_name} {odd_version} vs {first_version})",
            found
                .iter()
                .filter(|(_, v, s)| v != first_version || s != first_source)
                .count(),
            found.len(),
        );
    }
    let described = describe_source(first_source);
    match iccce_commit_time(first_source, &git_revision(first_source)) {
        Some(when) => format!("{first_version} ({described}, committed {when})"),
        None => format!("{first_version} ({described})"),
    }
}

/// The resolved git revision out of a `Cargo.lock` `source` value, or the
/// empty string when the source is not a git one.
///
/// Split out of [`describe_source`] rather than returned alongside its
/// text because the two are wanted at different fidelities: the banner
/// shows eight characters, and [`iccce_commit_time`] must look the object
/// up by the FULL revision — an abbreviated one is ambiguous to `git` in
/// principle and is simply the wrong string to hand a lookup.
fn git_revision(source: &str) -> String {
    source
        .strip_prefix("git+")
        .and_then(|git| git.split_once('#'))
        .map_or_else(String::new, |(_, rev)| rev.to_owned())
}

/// The value of `key = "…"` on one `Cargo.lock` line, if that is the key.
///
/// Anchored on `key = "` rather than on `contains(key)` so that a
/// `source` line whose URL happens to contain the word `name` cannot be
/// read as a `name` line.
fn unquote(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    let rest = line.strip_prefix(&prefix)?;
    Some(rest.strip_suffix('"')?.to_owned())
}

/// A lock-file `source` value reduced to the shortest phrase that
/// identifies the build.
///
/// The git revision is abbreviated to eight characters — enough to be
/// unambiguous in any real repository, short enough that the banner line
/// stays readable. The full revision is in `Cargo.lock`, which is
/// committed, so nothing is lost.
fn describe_source(source: &str) -> String {
    if source.is_empty() {
        // No `source` key at all means a path dependency or a workspace
        // member: the code is on this disk, not fetched.
        return "path".to_owned();
    }
    let Some(git) = source.strip_prefix("git+") else {
        return "registry".to_owned();
    };
    let rev = git
        .split_once('#')
        .map(|(_, rev)| rev)
        .unwrap_or_default()
        .chars()
        .take(8)
        .collect::<String>();
    // `?tag=v0.3.0`, `?branch=main`, `?rev=abc123` — the pin the manifest
    // actually asked for, which is what a reader recognises. The resolved
    // revision is what they get; both are worth printing.
    let pin = git
        .split_once('?')
        .and_then(|(_, query)| query.split('#').next())
        .and_then(|query| query.split_once('='));
    match (pin, rev.is_empty()) {
        // ★ A `rev` pin IS a revision, so printing it and then the resolved
        // revision again is the same eight characters twice -- and before
        // this arm the full forty-character pin was printed ahead of them,
        // which is what the banner did from the day iccce moved from a tag
        // to a rev (2026-09-02) until the worked examples in this file were
        // found to disagree with the live output. One abbreviated revision,
        // once; if the lock ever resolved a `rev` pin to a DIFFERENT
        // revision, that would be a broken lock and worth both numbers.
        (Some(("rev", value)), false) if value.starts_with(rev.as_str()) => {
            format!("rev {rev}")
        }
        (Some((kind, value)), false) => format!("{kind} {value}, {rev}"),
        (Some((kind, value)), true) => format!("{kind} {value}"),
        (None, false) => format!("git {rev}"),
        (None, true) => "git".to_owned(),
    }
}

/// The workspace `Cargo.lock`, found by walking up from this crate.
///
/// Walks rather than hard-coding `../../Cargo.lock` so that a vendored or
/// relocated checkout still resolves, and stops at the filesystem root
/// rather than after a fixed number of levels — the same shape
/// [`locate_git_dir`] uses, and for the same reason.
fn locate_lockfile() -> Option<std::path::PathBuf> {
    let start = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let mut dir: Option<&Path> = Some(Path::new(&start));
    while let Some(current) = dir {
        let candidate = current.join("Cargo.lock");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// When the linked `iccce` revision was committed, RFC 3339 UTC.
///
/// # ★ Why this is a fact about the BINARY and not about the machine
///
/// The build script's other `iccce` doc block explains why reading the
/// sibling checkout at `D:\Dev\iccce` would be dishonest: it answers
/// *"which iccce is on this machine"*. This does not have that problem,
/// and the difference is one word — **`rev`**.
///
/// `rev` comes out of `Cargo.lock`: it is the exact revision this build
/// compiles. Asking a repository *"when was `a4d9003b` committed"* has one
/// answer, the same on every machine that has the object, and it is a
/// property of the linked code rather than of the checkout that answered.
/// A repository is being used as a **lookup table keyed by the revision**,
/// not as a source of truth about what is linked.
///
/// # Where it looks, and why that one first
///
/// `$CARGO_HOME/git/db/<name>-<hash>` — Cargo's own bare mirror of the
/// dependency's repository. It is the copy Cargo fetched *in order to
/// build this*, so if the build succeeded the object is there by
/// construction. That makes it the only lookup that cannot be stale or
/// absent for a git dependency actually being compiled.
///
/// `<hash>` is Cargo's hash of the repository URL and is not reproducible
/// here, so the directory is matched by the `<name>-` prefix derived from
/// the URL's last path segment.
///
/// # Failure is silence, not a guess
///
/// Returns `None` on every failure — no `CARGO_HOME`, no `git` on PATH, a
/// registry or path dependency with no repository, an object that is
/// somehow absent. The caller then prints the provenance without a date,
/// which is a shorter true statement rather than a longer plausible one.
fn iccce_commit_time(source: &str, rev: &str) -> Option<String> {
    if rev.is_empty() {
        return None;
    }
    let url = source.strip_prefix("git+")?.split('?').next()?;
    // `https://github.com/KenM76/iccce.git` -> `iccce`
    let name = url
        .rsplit('/')
        .next()?
        .trim_end_matches(".git")
        .to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }

    let cargo_home = std::env::var("CARGO_HOME").map_or_else(
        |_| {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map(|home| Path::new(&home).join(".cargo"))
        },
        |value| Ok(std::path::PathBuf::from(value)),
    );
    let db = cargo_home.ok()?.join("git").join("db");

    let prefix = format!("{name}-");
    for entry in std::fs::read_dir(&db).ok()? {
        let Ok(entry) = entry else { continue };
        if !entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(&prefix)
        {
            continue;
        }
        let out = Command::new("git")
            .args(["show", "-s", "--format=%ct", rev])
            .current_dir(entry.path())
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        // Formatted through the same helper the build's own timestamps
        // use, so a date copied out of the `iccce` line and one copied out
        // of the `committed` line are directly comparable — which is the
        // whole point of the operator having asked for both.
        if let Some(seconds) = String::from_utf8(out.stdout)
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
        {
            return Some(format_rfc3339_utc(seconds));
        }
    }
    None
}
