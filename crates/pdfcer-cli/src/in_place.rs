//! `--in-place` for every subcommand that saves an edited copy of its input.
//!
//! The flag is added to the parsed command tree rather than declared on 100+
//! enum variants: [`with_in_place`] adds `--in-place` to each subcommand in
//! [`IN_PLACE_COMMANDS`] and relaxes a required `--output` to "required unless
//! `--in-place`". [`resolve_in_place`] then rewrites the argument vector so
//! `--in-place` becomes `--output <INPUT>`, and the command parses exactly as
//! if the operator had typed that. Writing over the input is safe because every
//! output write is a temp file plus rename (`edit_common::write_output`).
//!
//! `ocr` declares its own `--in-place` and is not listed here.

use std::ffi::OsString;

use clap::{Arg, ArgAction, Command};

/// The [`IN_PLACE_COMMANDS`] compiled only with the `signing` feature. Any
/// other listed name missing from the tree is a defect and panics.
pub(crate) const SIGNING_COMMANDS: &[&str] = &["add-ltv", "sign", "timestamp"];

/// Subcommands whose `--output` is the edited PDF and whose `input`
/// positional is the PDF it edits. A command whose `--output` is anything
/// else (a clipboard file, extracted attachment bytes) must never be listed:
/// `--in-place` would overwrite the PDF with it.
pub(crate) const IN_PLACE_COMMANDS: &[&str] = &[
    "3d-embed",
    "3d-poster",
    "3d-views",
    "add-bookmark",
    "add-caret",
    "add-check-box",
    "add-choice-field",
    "add-emf",
    "add-image",
    "add-image-stamp",
    "add-link",
    "add-ltv",
    "add-named-dest",
    "add-push-button",
    "add-radio-button",
    "add-reply",
    "add-screen",
    "add-sound",
    "add-svg",
    "add-text",
    "add-text-field",
    "adopt-widget",
    "annotate",
    "annotation-vertex",
    "attach-file",
    "attach-file-annotation",
    "bookmark-paste",
    "delete-annotation",
    "delete-bookmark",
    "delete-field",
    "delete-field-group",
    "delete-pages",
    "delete-widget",
    "deskew",
    "detach-file",
    "dimension-add",
    "dimension-area",
    "dimension-delete",
    "dimension-display",
    "dimension-extension-gap",
    "dimension-group",
    "dimension-label",
    "dimension-offset",
    "dimension-rotate",
    "dimension-style",
    "dimension-vertex",
    "edit-block-text",
    "edit-field",
    "edit-text",
    "edit-widget",
    "embed-font",
    "encrypt",
    "fill-field",
    "flatten",
    "flatten-annotations",
    "format-text",
    "group-add",
    "group-delete",
    "group-rename",
    "group-set-scale",
    "group-set-standard",
    "group-set-unit",
    "group-style",
    "handle-move",
    "import-data",
    "import-structure",
    "ink-edit",
    "insert-pages",
    "layer-add",
    "layer-delete",
    "layer-edit",
    "layer-flatten",
    "layer-folder-add",
    "layer-folder-delete",
    "layer-folder-rename",
    "layer-merge",
    "layer-move",
    "layer-toggle",
    "merge-document",
    "move-annotation",
    "move-bookmark",
    "move-widget",
    "node-convert",
    "node-delete",
    "node-insert",
    "node-move",
    "nodes-move",
    "object-delete",
    "object-move",
    "object-move-each",
    "object-paste",
    "object-transform",
    "object-transform-each",
    "page-paste",
    "paste-field",
    "place-stamp",
    "promote-dr-fonts",
    "purge-password-values",
    "recompute",
    "redact-apply",
    "redact-mark",
    "reflow",
    "remove-encryption",
    "remove-metadata",
    "regenerate-appearances",
    "rename-bookmark",
    "rename-field",
    "reorder-annotations",
    "reorder-pages",
    "replace-image",
    "reset-form",
    "resize-annotation",
    "respan-markup",
    "rotate",
    "rotate-annotation",
    "rotate-page",
    "rotate-widget",
    "scale-pages",
    "segment-convert",
    "set-annotation-flags",
    "set-annotation-layer",
    "set-annotation-open",
    "set-bookmark-open",
    "set-button-action",
    "set-crop-box",
    "set-field-script",
    "set-info",
    "set-annot-opacity",
    "set-link-border",
    "set-link-target",
    "set-marker-color",
    "set-markup-note",
    "set-markup-style",
    "set-object-layer",
    "set-object-paint",
    "set-object-stroke-style",
    "restack-objects",
    "set-page-size",
    "set-page-tabs",
    "set-page-labels",
    "clear-page-labels",
    "set-permissions",
    "set-review-state",
    "set-text-annot-style",
    "sign",
    "timestamp",
    "subpath-delete",
    "subpath-move",
    "text-object-split",
    "text-run-delete",
    "text-run-merge",
    "text-run-move",
    "text-run-width",
    "to-pdfa",
    "turn-widget",
    "unembed-font",
    "unshare-form",
];

const IN_PLACE_HELP: &str = "Write the result back over the input file instead of to --output. \
The file is replaced only after the new one is fully written, so a failure leaves the \
original untouched. Cannot be combined with --output.";

/// Adds `--in-place` to every [`IN_PLACE_COMMANDS`] subcommand of `root`.
///
/// # Panics
///
/// If a listed name is not a subcommand, or lacks an `input` or `output`
/// argument — a programming error the unit tests catch.
///
/// A positional `OUTPUT` (`encrypt` and its siblings) becomes optional too;
/// clap allows that because it is the last positional.
pub(crate) fn with_in_place(mut root: Command) -> Command {
    for &name in IN_PLACE_COMMANDS {
        if !cfg!(feature = "signing") && SIGNING_COMMANDS.contains(&name) {
            continue;
        }
        root = root.mut_subcommand(name, |sub| {
            let output_required = sub
                .get_arguments()
                .find(|a| a.get_id() == "output")
                .unwrap_or_else(|| panic!("`{name}` has no `output` argument"))
                .is_required_set();
            assert!(
                sub.get_arguments().any(|a| a.get_id() == "input"),
                "`{name}` has no `input` argument"
            );
            let sub = sub.arg(
                Arg::new("in_place")
                    .long("in-place")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("output")
                    .help(IN_PLACE_HELP),
            );
            if output_required {
                sub.mut_arg("output", |a| {
                    a.required(false).required_unless_present("in_place")
                })
            } else {
                sub
            }
        });
    }
    root
}

/// Parses `args` against `root` (already passed through [`with_in_place`])
/// and, if `--in-place` was given, returns `args` with that flag replaced by
/// `--output <INPUT>` — or, where `OUTPUT` is positional, drops the flag and
/// appends `<INPUT>` as the final positional. Otherwise returns `args`
/// unchanged.
///
/// # Errors
///
/// Any clap error from the first parse (unknown flag, `--in-place` with
/// `--output`, neither given, `--help`), for the caller to `exit()` on.
pub(crate) fn resolve_in_place(
    root: &Command,
    mut args: Vec<OsString>,
) -> Result<Vec<OsString>, clap::Error> {
    let matches = root.clone().try_get_matches_from(args.iter())?;
    let Some((name, sub)) = matches.subcommand() else {
        return Ok(args);
    };
    if !IN_PLACE_COMMANDS.contains(&name) || !sub.get_flag("in_place") {
        return Ok(args);
    }
    let input = sub
        .get_raw("input")
        .and_then(|mut v| v.next())
        .map(OsString::from)
        .expect("`input` is a required positional");
    // A SetTrue flag given twice is an error, so exactly one token matches;
    // `--in-place` cannot be a value of another option because clap would
    // then not have set the flag.
    let at = args
        .iter()
        .position(|a| a == "--in-place")
        .expect("clap set the flag, so the token is present");
    let positional = sub_is_positional_output(root, name);
    if positional {
        args.remove(at);
        args.push(input);
    } else {
        args.splice(at..=at, [OsString::from("--output"), input]);
    }
    Ok(args)
}

fn sub_is_positional_output(root: &Command, name: &str) -> bool {
    root.find_subcommand(name)
        .and_then(|sub| sub.get_arguments().find(|a| a.get_id() == "output"))
        .is_some_and(Arg::is_positional)
}
