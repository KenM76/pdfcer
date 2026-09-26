use super::*;

/// Run `f` on a worker thread with a large (16 MiB) stack.
///
/// clap builds and validates the command tree by **recursion**
/// proportional to the subcommand/argument count, and this CLI's surface
/// (dozens of subcommands, and growing — the Pass 9c-min `object-move`/
/// `object-delete`/`node-move` additions among them) now needs more stack
/// than a default Windows test thread's ~2 MiB to run `Command::debug_assert`
/// and to walk `Cli::command()`. This is a scaling property of clap's
/// recursion, not a bug in the CLI: **production is unaffected** — the real
/// binary parses on the process main thread (8 MiB) and never calls
/// `debug_assert`. The worker keeps the full validation without shrinking
/// the CLI surface; a panic (a failed assertion) propagates through `join`.
fn on_large_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("spawn the large-stack validation thread")
        .join()
        .expect("the large-stack validation thread panicked");
}

/// clap's own invariant check — catches malformed `#[command]`/`#[arg]`
/// wiring (duplicate flags, bad defaults) at test time rather than on
/// first run. Recommended by clap's docs for any derive-based CLI. Run on
/// [`on_large_stack`] because `debug_assert` recurses over the whole tree.
#[test]
fn cli_definition_is_valid() {
    on_large_stack(|| {
        use clap::CommandFactory as _;
        Cli::command().debug_assert();
    });
}

/// Byte offset of the first `Pass <digit>` in `s`, if any.
fn pass_id_at(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = s[from..].find("Pass ") {
        let at = from + rel;
        // A DOTTED number is required, so that ordinary English -- "Pass 0
        // to REMOVE the limit", in `edit-field --max-len` -- is not read as
        // a roadmap reference. Every internal ID this project mints has a
        // minor part: `Pass 138.0`, `Pass 12.M2`.
        let rest = &s[at + 5..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with('.') {
            return Some(at);
        }
        from = at + 5;
    }
    None
}

/// A short, char-boundary-safe window around `at`, for a failure message
/// that names the sentence rather than only the command.
fn snippet(s: &str, at: usize) -> String {
    let mut start = at.saturating_sub(40);
    while start > 0 && !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + 60).min(s.len());
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    s[start..end].replace('\n', " ")
}

/// Nothing the operator reads may carry source-only markup.
///
/// WHY THIS IS A TEST AND NOT A `tools/` SCRIPT: the defect is in the
/// RENDERED string, and only clap can produce that. A text scan of
/// `main.rs` would have to model how clap joins hard-wrapped `///` lines
/// into a paragraph, which half of the markup straddles, and it would
/// still miss the part `plain_help` legitimately rewrites at run time.
/// Rendering the help and reading it is the same thing the operator does.
///
/// WHAT IT REJECTS, AND WHY EACH IS SHIPPED COPY:
///
/// * `**` — Markdown emphasis. `cargo doc` renders it; a terminal prints
///   the asterisks. `pdfcer --help` shipped 99 subcommand summaries this
///   way through v0.54.0.
/// * `` ` `` — Markdown code spans, same split.
/// * `Pass <n>` — an internal roadmap identifier. True inside this
///   repository, meaningless outside it, and 70 summaries named one.
///
/// [`plain_help`] removes the first two everywhere and the third from
/// parentheticals. What this test therefore catches in practice is a Pass
/// ID written into running prose, and markup in the one place the scrub
/// cannot reach: a `ValueEnum` variant's doc comment, whose text becomes a
/// `PossibleValue` help string with no setter that preserves the typed
/// parser. Both are source fixes.
#[test]
fn cli_help_ships_no_internal_markup() {
    on_large_stack(|| {
        use clap::CommandFactory as _;
        let mut offenders: Vec<String> = Vec::new();
        let root = scrub_help(Cli::command());
        let mut queue: Vec<(String, clap::Command)> = vec![(String::from("pdfcer"), root.clone())];
        while let Some((path, mut cmd)) = queue.pop() {
            for sub in cmd.get_subcommands() {
                queue.push((format!("{path} {}", sub.get_name()), sub.clone()));
            }
            let rendered = cmd.render_long_help().to_string();
            for (needle, what) in [("**", "Markdown bold"), ("`", "a Markdown code span")] {
                if let Some(at) = rendered.find(needle) {
                    offenders.push(format!("{path}: {what}: {}", snippet(&rendered, at)));
                }
            }
            if let Some(at) = pass_id_at(&rendered) {
                offenders.push(format!(
                    "{path}: an internal Pass ID: {}",
                    snippet(&rendered, at)
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "`--help` ships source-only markup in {} place(s):\n  {}\n\n\
             A `///` on a clap item IS the operator-facing help text. \
             `plain_help` strips Markdown everywhere and Pass IDs from \
             parentheticals; what reaches here is either a Pass ID written \
             into prose (reword the sentence) or a `ValueEnum` variant's \
             doc comment (the scrub cannot reach a PossibleValue's help).",
            offenders.len(),
            offenders.join("\n  ")
        );
    });
}

#[test]
fn io_error_maps_to_io_exit_code() {
    let err = PdfError::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert_eq!(exit_code_for(&err), exit::IO_ERROR);
}

// decision 012: the --font-dir walk (shell-side, R61).

#[test]
fn font_dir_empty_gives_bundled_only_and_no_notes() {
    // No --font-dir → the deterministic default path is untouched:
    // zero registrations, zero notes (R63).
    let (_env, registered, notes) = build_font_environment(&[]);
    assert_eq!(registered, 0);
    assert!(notes.is_empty());
}

#[test]
fn font_dir_registers_a_readable_face_under_its_filename_stem() {
    // A bundled Foxit CFF copied into a temp dir as `Calibri.cff`
    // must register under the stem `Calibri` (and its advertised
    // name(s)) so a document's non-embedded `Calibri` matches. This
    // exercises the real read → parse → face_names → insert_named
    // path without shipping any third-party font.
    let dir = std::env::temp_dir().join(format!("pdfcer-fontdir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let face = pdfcer_render::font::bundled::faces();
    let bytes = face
        .get(&pdfcer_render::FallbackKey::Serif)
        .unwrap()
        .bytes()
        .to_vec();
    let path = dir.join("Calibri.cff");
    std::fs::write(&path, &bytes).unwrap();

    let (env, registered, notes) = build_font_environment(std::slice::from_ref(&dir));
    assert!(registered >= 1, "at least the stem registers: {notes:?}");
    assert!(
        env.named("Calibri").is_some(),
        "must match the filename stem: {notes:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn font_dir_skips_corrupt_file_without_error_and_notes_it() {
    // Acceptance: a corrupt/misnamed supplied file fails CLEAN — it
    // is skipped and noted, never fatal, and the bundled default is
    // still available for the render to fall back to.
    let dir = std::env::temp_dir().join(format!("pdfcer-fontbad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Calibri.ttf"), b"this is not a font at all").unwrap();

    let (env, registered, notes) = build_font_environment(std::slice::from_ref(&dir));
    assert_eq!(registered, 0, "a corrupt file registers nothing");
    assert!(
        notes.iter().any(|n| n.contains("not a usable font")),
        "the skip must be disclosed: {notes:?}"
    );
    // The environment is still a usable bundled one.
    assert!(env.fallback(pdfcer_render::FallbackKey::Sans).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn font_dir_missing_directory_is_a_note_not_a_panic() {
    let missing = std::env::temp_dir().join("pdfcer-does-not-exist-xyz-012");
    let (_env, registered, notes) = build_font_environment(&[missing]);
    assert_eq!(registered, 0);
    assert_eq!(notes.len(), 1, "one note for the unreadable dir");
}

#[test]
fn header_errors_map_to_not_a_pdf() {
    assert_eq!(
        exit_code_for(&PdfError::MissingHeader { searched: 8 }),
        exit::NOT_A_PDF
    );
    assert_eq!(
        exit_code_for(&PdfError::MalformedVersion {
            found: "x.y".to_owned()
        }),
        exit::NOT_A_PDF
    );
}

#[test]
fn exit_codes_are_distinct_and_avoid_claps_reserved_two() {
    // The exit-code table IS the scripting contract (module docs):
    // two failure modes sharing a code silently merges them for
    // every caller that branches on it, and `2` belongs to clap.
    let codes = [
        exit::SUCCESS,
        exit::RUNTIME_ERROR,
        exit::IO_ERROR,
        exit::NOT_A_PDF,
        exit::NOT_BYTE_IDENTICAL,
        exit::RELOAD_FAILED,
        exit::RASTER_DIFFERS,
        exit::SAVE_REFUSED,
        exit::EDIT_REFUSED,
        exit::UNIMPLEMENTED,
    ];
    let mut seen = codes;
    seen.sort_unstable();
    let before = seen.len();
    let mut dedup = seen.to_vec();
    dedup.dedup();
    assert_eq!(dedup.len(), before, "two exit codes collide: {codes:?}");
    assert!(!codes.contains(&2), "2 is reserved by clap");
}

#[test]
fn round_trip_mode_names_match_the_stdout_contract() {
    // `mode=` is part of the stable stdout line, so these strings
    // are a compatibility surface, not cosmetics. They are also the
    // clap value names, which must stay in step.
    assert_eq!(mode_name(RoundTripMode::Incremental), "incremental");
    assert_eq!(mode_name(RoundTripMode::Full), "full");
    assert_eq!(mode_name(RoundTripMode::AppendIdentity), "append-identity");
}

#[test]
fn save_mode_names_match_the_stdout_contract() {
    // `mode=` is part of the stable stdout line for both editing
    // subcommands, so these strings are a compatibility surface.
    assert_eq!(SaveMode::Incremental.name(), "incremental");
    assert_eq!(SaveMode::Full.name(), "full");
}

#[test]
fn editing_subcommands_default_to_the_signature_safe_save_mode() {
    // Incremental is the default because it is the only mode that
    // preserves an existing signature's byte range (§12.8.1 NOTE 1)
    // and the only one that leaves prior revisions recoverable. A
    // default of `full` would silently destroy signatures on every
    // batch edit.
    on_large_stack(|| {
        use clap::CommandFactory as _;
        let cmd = Cli::command();
        for name in ["set-info", "rotate-page"] {
            let sub = cmd
                .get_subcommands()
                .find(|c| c.get_name() == name)
                .unwrap_or_else(|| panic!("{name} subcommand is missing"));
            let mode = sub
                .get_arguments()
                .find(|a| a.get_id() == "mode")
                .unwrap_or_else(|| panic!("{name} has no --mode flag"));
            assert_eq!(
                mode.get_default_values()
                    .first()
                    .map(|v| v.to_string_lossy().into_owned()),
                Some("incremental".to_owned()),
                "{name}"
            );
        }
    });
}

#[test]
fn info_field_args_map_one_to_one_onto_the_core_enum() {
    // A CLI enum that silently mapped two flags onto one field
    // would make `--clear` unpredictable; the round trip pins it.
    use pdfcer_core::edit::InfoField;
    let pairs = [
        (InfoFieldArg::Title, InfoField::Title),
        (InfoFieldArg::Author, InfoField::Author),
        (InfoFieldArg::Subject, InfoField::Subject),
        (InfoFieldArg::Keywords, InfoField::Keywords),
    ];
    for (arg, expected) in pairs {
        assert_eq!(InfoField::from(arg), expected);
    }
    assert_eq!(pairs.len(), InfoField::all().len());
}

#[test]
fn round_trip_defaults_are_the_verification_safe_ones() {
    // The CLI's `--producer` default is `preserve`, deliberately
    // NOT pdfcer-core's `Set`: this subcommand's job is verification,
    // and a stamped /Producer is a byte change that would make the
    // per-object identity check fail for one object by design.
    on_large_stack(|| {
        use clap::CommandFactory as _;
        let cmd = Cli::command();
        let rt = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "round-trip")
            .expect("round-trip subcommand is missing");
        let producer = rt
            .get_arguments()
            .find(|a| a.get_id() == "producer")
            .expect("--producer flag is missing");
        assert_eq!(
            producer
                .get_default_values()
                .first()
                .map(|v| v.to_string_lossy().into_owned()),
            Some("preserve".to_owned())
        );
        let mode = rt
            .get_arguments()
            .find(|a| a.get_id() == "mode")
            .expect("--mode flag is missing");
        assert_eq!(
            mode.get_default_values()
                .first()
                .map(|v| v.to_string_lossy().into_owned()),
            Some("incremental".to_owned())
        );
    });
}
