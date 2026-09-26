use super::*;

#[derive(Debug, Parser)]
#[command(
    name = "pdfcer",
    version,
    // `-V` stays the bare crate version, which is what a script parses;
    // `--version` is what a HUMAN reads and is where the provenance goes.
    long_version = build_banner(),
    about = "pdfcer command-line batch shell — scriptable PDF operations.",
    // WHY THIS SENTENCE CARRIES NO COUNT: it drifts. The count lives in
    // README.md, where `tools/check-clap-help.py` checks it against this
    // enum's own variants. A number here would be a second place to be wrong.
    long_about = "pdfcer is the command-line front end to the pdfcer PDF \
engine: a scriptable shell over page operations, text and vector editing, \
forms, annotations, signatures, encryption, redaction, OCR and rendering. \
Every subcommand listed below works today except the few whose own \
description says `[not yet implemented]`. Run `pdfcer <COMMAND> --help` for \
one command's options."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,

    /// Password for an encrypted PDF (ISO 32000-1 §7.6). Either the user or
    /// the owner password opens the document.
    ///
    /// NOT NEEDED for most protected PDFs. A document with an empty user
    /// password — the common "permissions-only" PDF — opens with no password
    /// at all, because §7.6.3.1 requires a reader to try the empty one first
    /// and silently. Supply this only when pdfcer asks for it.
    ///
    /// SECURITY: a password on the command line is visible to every other
    /// process on the machine (`ps`, Task Manager) and is written to your
    /// shell's history file. Prefer --open-password-file, which is not.
    ///
    /// NAMED `--open-password`, not `--password`, because `--password` is
    /// already taken: `add-text-field --password` is the Table 228 field flag
    /// that makes a form field mask its input. Two unrelated meanings, and
    /// `clap` refuses the collision outright (it panics at run time, which is
    /// how this was found). Renaming the shipped field flag would break
    /// existing scripts, so the newcomer takes the qualified name — and
    /// "open" is the more accurate word anyway: this password opens the
    /// document, it does not set one.
    #[arg(long, global = true, value_name = "PASSWORD")]
    pub(crate) open_password: Option<String>,

    /// What to do with a file that contradicts itself or omits something the
    /// standard requires.
    ///
    /// A malformed file OPENS by default rather than being refused, and
    /// `inspect` prints every decision pdfcer made. Use this to take the other
    /// decision where there is one, or `refuse` to fail as pdfcer did before.
    #[arg(long, global = true, value_enum, default_value_t = OnMalformedArg::KeepLast)]
    pub(crate) on_malformed: OnMalformedArg,

    /// Read the PDF password from a file, or from standard input with `-`.
    ///
    /// The first line is used, with a trailing newline (and CR) stripped; a
    /// file with no newline is used whole. Nothing else in the file is read,
    /// so a one-line secrets file works unchanged.
    ///
    /// Preferred over --open-password: the value never appears in a process
    /// listing or a shell history file.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        conflicts_with = "open_password"
    )]
    pub(crate) open_password_file: Option<PathBuf>,
}

/// The password supplied by `--open-password` / `--open-password-file`,
/// resolved once.
///
/// # Why a process global rather than a threaded parameter
///
/// This is a genuinely process-wide option: it applies to every subcommand
/// that opens a document, and there are twenty-six such call sites. Threading
/// an `Option<&[u8]>` through all of them would change twenty-six function
/// signatures — and every future one — to carry a value that is constant for
/// the life of the process and that only two lines of code ever set. `clap`
/// models exactly this shape with `global = true`; this is its storage.
///
/// Written once, in [`run`], before any subcommand executes. [`OnceLock`]
/// rather than a mutable static so that ordering is enforced by the type
/// system: a read before the write yields `None`, which is the same answer as
/// "no password supplied" and therefore cannot produce a wrong decryption —
/// it can only produce a `PasswordRequired` the operator will understand.
pub(crate) static CLI_PASSWORD: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
/// `text-run-merge --fit`: how wide the merged run is (`G035`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum MergeFitArg {
    /// Scale the merged run so it spans from the first run's start to the
    /// last run's end.
    Span,
    /// Keep the first run's own horizontal scaling.
    Natural,
}

/// What pdfcer does with a file that contradicts itself or omits something the
/// standard requires (`Pass 283.0`).
///
/// NAMED FOR THE CLASS, NOT FOR ONE MEMBER. The first cut called this
/// `--duplicate-keys`, and `strict` also turned off two unrelated recoveries —
/// a flag whose name understates what it governs, which is how an operator
/// ends up surprised by a setting they thought they understood.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub(crate) enum OnMalformedArg {
    /// Open the file, and where one dictionary names a key twice keep the
    /// LAST value. The default — every other override-by-repetition in PDF is
    /// last-wins.
    #[default]
    KeepLast,
    /// Open the file, and keep the FIRST value for a duplicated key.
    KeepFirst,
    /// Refuse the file — a duplicated key, a missing /Length and a missing
    /// endobj are all fatal. For a caller whose job is to say whether a file
    /// is well-formed.
    Refuse,
}

impl OnMalformedArg {
    /// The core-side options this argument selects.
    pub(crate) fn to_load_options(self) -> pdfcer_core::document::LoadOptions {
        use pdfcer_core::parser::DuplicateKeyPolicy;
        match self {
            Self::KeepLast => pdfcer_core::document::LoadOptions::new(),
            Self::KeepFirst => pdfcer_core::document::LoadOptions::new()
                .with_duplicate_keys(DuplicateKeyPolicy::KeepFirst),
            Self::Refuse => pdfcer_core::document::LoadOptions::strict(),
        }
    }
}

/// The load options every subcommand's document open uses, set once from the
/// global flag before dispatch.
///
/// Same shape as [`CLI_PASSWORD`] and for the same reason: threading an
/// argument through ninety subcommands to serve one flag would be a bigger
/// change than the feature.
pub(crate) static CLI_LOAD_OPTIONS: std::sync::OnceLock<pdfcer_core::document::LoadOptions> =
    std::sync::OnceLock::new();

/// The load options in force, defaulting to the tolerant ones.
pub(crate) fn cli_load_options() -> pdfcer_core::document::LoadOptions {
    CLI_LOAD_OPTIONS
        .get()
        .copied()
        .unwrap_or_else(pdfcer_core::document::LoadOptions::new)
}

/// The password for opening encrypted documents, or `None` if none was given.
///
/// `None` is **not** the empty password. §7.6.3.1's silent empty-password
/// attempt happens inside `pdfcer-core` for every document regardless; `None`
/// means only that if that attempt fails there is nothing else to try.
pub(crate) fn cli_password() -> Option<&'static [u8]> {
    CLI_PASSWORD
        .get()
        .and_then(Option::as_ref)
        .map(Vec::as_slice)
}

/// Resolve `--open-password` / `--open-password-file` into [`CLI_PASSWORD`].
///
/// Returns an error string for an `--open-password-file` that cannot be read, since
/// silently proceeding without a password the operator explicitly supplied
/// would surface as "this document is password-protected" and send them
/// hunting for the wrong problem.
pub(crate) fn resolve_cli_password(
    password: Option<String>,
    password_file: Option<PathBuf>,
) -> Result<Option<Vec<u8>>, String> {
    if let Some(p) = password {
        return Ok(Some(p.into_bytes()));
    }
    let Some(path) = password_file else {
        return Ok(None);
    };

    let raw = if path.as_os_str() == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
            .map_err(|e| format!("reading the password from stdin: {e}"))?;
        s
    } else {
        std::fs::read_to_string(&path)
            .map_err(|e| format!("reading the password file {}: {e}", path.display()))?
    };

    // First line only, newline stripped. A password file is conventionally one
    // line, and a trailing newline that every editor adds must not silently
    // become part of the password — that failure looks exactly like a wrong
    // password and is unusually hard to see.
    let line = raw.split('\n').next().unwrap_or("");
    let line = line.strip_suffix('\r').unwrap_or(line);
    Ok(Some(line.as_bytes().to_vec()))
}

/// Open a document at `path`, supplying the CLI password if one was given.
///
/// Every subcommand that reads a file goes through here rather than calling
/// [`Document::load`] directly, so `--open-password` reaches all of them and a new
/// subcommand cannot forget it. That is the affordance half of the capability:
/// `pdfcer-core` gained decryption, and a core capability no shell can reach is
/// not a feature yet.
pub(crate) fn open_document(
    path: &Path,
) -> Result<pdfcer_core::document::Document, pdfcer_core::document::DocError> {
    pdfcer_core::document::Document::load_with_options(path, cli_password(), cli_load_options())
}

/// Parse a document from bytes, supplying the CLI password if one was given.
///
/// The `from_bytes` counterpart of [`open_document`], for the subcommands that
/// have already read the file (usually because they also need the raw bytes).
pub(crate) fn open_document_bytes(
    bytes: Vec<u8>,
) -> Result<pdfcer_core::document::Document, pdfcer_core::document::DocError> {
    pdfcer_core::document::Document::from_bytes_with_options(
        bytes,
        cli_password(),
        cli_load_options(),
    )
}

/// The planned subcommand surface. Only [`Command::Inspect`] is implemented
/// at Pass 0; the rest are stubs (see the module docs). Each variant's doc
/// comment is what `pdfcer --help` and `pdfcer <cmd> --help` show.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Inspect a PDF: confirm the %PDF- header and print its declared version.
    ///
    /// With `--text-blocks`, instead recognise and dump the page's
    /// **editable text-block** structure (ISO 32000-1 §14.8):
    /// the derived Run→Line→Column→Block hierarchy, every inference counted
    /// and disclosed. Strictly READ-ONLY — nothing is written. Because an
    /// untagged content stream defines no word/line/paragraph/column/reading
    /// order (§14.8, S1-S9), the whole structure is a reviewable HINT, and
    /// the sourced-only text is reported unchanged alongside it.
    Inspect {
        /// Path to the PDF file to inspect.
        file: PathBuf,
        /// Recognise and dump the editable text-block structure instead of
        /// printing the version line (read-only; §14.8).
        #[arg(long)]
        text_blocks: bool,
        /// With `--text-blocks`: 1-based pages to analyse: `all`, `3`,
        /// `1-4`, `5,1-2`. Order is honoured. Ignored without
        /// `--text-blocks`.
        #[arg(long, default_value = "all")]
        pages: String,
        /// With `--text-blocks`: emit a JSON document (full structure,
        /// per-line provenance, every diagnostic counter) instead of the
        /// human-readable report. Ignored without `--text-blocks`. Also
        /// selects JSON output for `--reflow-preview`.
        #[arg(long)]
        json: bool,
        /// Compute and dump a READ-ONLY within-block reflow PREVIEW for one
        /// recognised block (ISO 32000-1 §14.8; decision 015):
        /// the auto-detected alignment, the greedy re-wrap's new break
        /// points and per-line origins, the new block box, and every
        /// disclosure. Strictly READ-ONLY — nothing is written, no content
        /// stream is mutated (`reflow` does that). Select the block with
        /// `--block` (and the page via `--pages`, first page used); tune the
        /// preview with `--width`/`--align`/`--leading`.
        #[arg(long)]
        reflow_preview: bool,
        /// With `--reflow-preview`: 0-based index of the block to preview on
        /// the selected page. Default `0`.
        #[arg(long, default_value_t = 0)]
        block: usize,
        /// With `--reflow-preview`: wrap width in points. Default = the
        /// recognised block's own box width.
        #[arg(long)]
        width: Option<f64>,
        /// With `--reflow-preview`: alignment override — `left`, `right`,
        /// `center`, or `justified` (aliases `l`/`r`/`c`/`j`/`justify`/
        /// `centre`). Default = auto-detected from glyph x-positions.
        #[arg(long)]
        align: Option<String>,
        /// With `--reflow-preview`: leading (baseline-to-baseline) in points.
        /// Default = the block's measured baseline gap.
        #[arg(long)]
        leading: Option<f64>,

        /// List the form XObjects each page paints, with the object number
        /// `edit-text --target form:N` takes, the nesting depth, where its
        /// resources resolve, and HOW MANY PLACES IN THE DOCUMENT paint it
        /// (Pass 119.0).
        ///
        /// That last column is the one to read before a batch edit: a form
        /// XObject may legally be painted from several pages, so editing text
        /// inside one changes every place it appears. Honours `--pages`.
        #[arg(long)]
        forms: bool,
    },

    /// Merge several PDFs into one, in argument order.
    ///
    /// Produces a brand-new document; every input is read and left
    /// untouched. Form fields whose fully-qualified names collide across
    /// inputs are auto-renamed with a `Doc<N>_` prefix, which is what
    /// stops same-named fields from becoming ONE logical field that
    /// fills every copy at once.
    ///
    /// A BOOKMARK THAT OPENS ANOTHER OF THESE FILES IS RE-POINTED at that
    /// file's pages inside the merged document, instead of being dropped.
    ///
    /// This is the table-of-contents case: one PDF whose bookmarks open the
    /// other PDFs in a folder, through a `/Launch` or `/GoToR` action
    /// (§12.6.4.5, §12.6.4.3). Merging destroys every one of those targets
    /// — the bookmark still says *open `chapter1.pdf`* and there is no
    /// longer a `chapter1.pdf` — so a naive merge discards every one of
    /// them, and the operator's own bookmark titles go with them.
    ///
    /// Matching is by file NAME, case-insensitively, ignoring directories.
    /// A bookmark naming a file that is NOT one of the inputs is still
    /// dropped: it is genuinely dead, and inventing a destination for it
    /// would be worse. Both counts are on the metrics line
    /// (`outline_relinked=`, `outline_dropped=`) and the re-pointing is
    /// also stated in prose, because matching a filename is an INFERENCE.
    ///
    /// PDF inputs only. Converting Word/Excel/images to PDF as part of a
    /// merge is a separate capability, not a flag on this one.
    Merge {
        /// Input PDFs, concatenated in the order given. At least two.
        inputs: Vec<PathBuf>,
        /// Output path for the merged PDF.
        #[arg(short, long)]
        output: PathBuf,
        /// Do not generate one top-level bookmark per source file.
        ///
        /// Generation is ON by default, matching Acrobat's documented
        /// Combine-Files default. The bookmark is named after the input
        /// file's stem.
        #[arg(long)]
        no_bookmarks: bool,
    },

    /// Split a PDF into several standalone files.
    ///
    /// Exactly one criterion may be given; `--every` is the default when
    /// none is. Nothing is written until every output name is known to be
    /// distinct and (unless `--force`) free.
    Split {
        /// Input PDF to split.
        input: PathBuf,
        /// Directory to write the parts into. Created if absent.
        #[arg(long)]
        out_dir: PathBuf,
        /// Fixed number of pages per output file.
        #[arg(long, default_value_t = 1, group = "criterion")]
        every: usize,
        /// Split AFTER these 1-based pages, e.g. `3,7,12`.
        #[arg(long, group = "criterion")]
        after: Option<String>,
        /// One output per top-level bookmark, breaking at the page each
        /// one targets. Nested bookmarks do not create boundaries.
        #[arg(long, group = "criterion")]
        bookmarks: bool,
        /// Output naming template. Placeholders: `{stem}` `{n}`
        /// `{start}` `{end}`; `{n}` is zero-padded to the part count so
        /// the files sort correctly.
        #[arg(long, default_value = pdfcer_core::pageops::split::DEFAULT_NAME_TEMPLATE)]
        name_template: String,
        /// Overwrite existing files in the output directory.
        #[arg(long)]
        force: bool,
    },

    /// Extract pages into a new standalone PDF.
    ///
    /// The source is read and left untouched — use `delete-pages` on it
    /// afterwards for Acrobat's "extract and delete from original".
    /// Pages appear in the order given, so `--pages 5,1-2` is both a
    /// selection and an ordering.
    ExtractPages {
        /// Input PDF.
        input: PathBuf,
        /// Pages to extract, 1-based and inclusive, e.g. `3-7,9`.
        #[arg(long)]
        pages: String,
        /// Output path for the extracted pages.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Insert pages from another PDF into a target, producing a new file.
    InsertPages {
        /// The document being added to. Read, never modified.
        input: PathBuf,
        /// The PDF to take pages from.
        #[arg(long)]
        source: PathBuf,
        /// Which of the source's pages to insert, 1-based. Defaults to
        /// all of them.
        #[arg(long, default_value = "all")]
        source_pages: String,
        /// Insert BEFORE this 1-based target page. `0` means "at the
        /// start"; a value past the end means "at the end".
        #[arg(long)]
        before: Option<usize>,
        /// Insert AFTER this 1-based target page. Mutually exclusive
        /// with `--before`; the default is to append at the end.
        #[arg(long, conflicts_with = "before")]
        after: Option<usize>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// **Place a custom stamp's artwork on a page** — the placing half of
    /// stamp collections (§12.5.6.12).
    ///
    /// The stamp's page is imported as a form XObject and becomes a
    /// `/Stamp` annotation's appearance, which is what Acrobat writes: the
    /// artwork stays VECTOR, the page's own content stream is never
    /// touched, and the result is selectable, movable and deletable like
    /// any other annotation.
    ///
    /// Address the stamp either by its page in the collection
    /// (`--stamp-page`) or by the internal name the collection's name tree
    /// gives it (`--stamp`), which is what `stamp-list` prints.
    ///
    /// ⚠ A DYNAMIC stamp places its design-time text: its words come from
    /// AcroForm JavaScript that pdfcer does not author. `stamp-list` marks
    /// those, and this command says so when it places one.
    PlaceStamp {
        /// The document being stamped. Read, never modified.
        input: PathBuf,
        /// The stamp collection PDF to take the artwork from.
        #[arg(long)]
        from: PathBuf,
        /// Which page of the collection holds the artwork, 1-based.
        #[arg(long, conflicts_with = "stamp")]
        stamp_page: Option<usize>,
        /// The stamp's internal name, as `stamp-list` prints it (with or
        /// without the leading `#` a dynamic stamp carries).
        #[arg(long)]
        stamp: Option<String>,
        /// Which page of the input to stamp, 1-based.
        #[arg(long)]
        page: usize,
        /// Where to put it: `x0,y0,x1,y1` in points, default user space.
        ///
        /// The artwork is scaled to fill this rectangle (§12.5.5), so a
        /// rectangle whose proportions differ from the stamp's squashes it
        /// — which the command reports rather than leaving you to notice.
        /// §12.5.5's mapping is anisotropic by definition, so that is the
        /// standard's behaviour rather than a pdfcer limit.
        #[arg(long, conflicts_with = "at")]
        rect: Option<String>,
        /// Place at the stamp's OWN size, lower-left corner at `x,y`.
        ///
        /// Acrobat's click-to-place behaviour: the artwork arrives at the
        /// size its author drew it, undistorted. Prefer this unless you have
        /// a box the stamp must fill.
        #[arg(long, value_name = "X,Y")]
        at: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Find content drawn outside the page** — off-canvas objects, per
    /// file and per page (`Pass 294.0`).
    ///
    /// A page box (`/CropBox`, or `/MediaBox` when there is none) is what a
    /// reader displays; a content stream may draw anywhere. Marks outside it
    /// are still IN THE FILE: they print on a larger sheet, they survive a
    /// page-box change, and text among them is still extractable and
    /// searchable. Moving something off the sheet is not deleting it.
    ///
    /// Takes files, folders, or both. A folder is scanned one level deep
    /// unless `--recursive`.
    ///
    /// **Every file and every page is scanned, always.** Finding something
    /// does not stop the scan, and neither does a file that will not open —
    /// that one is reported and the walk continues.
    ///
    /// The EXIT CODE is `0` when nothing was found and `1` when something
    /// was, so a script can branch on it (`if pdfcer scan-offpage … ; then`).
    /// That is a verdict delivered at the END of the run, not an early stop.
    ///
    /// For one log of everything in one pass:
    ///
    ///     pdfcer scan-offpage <folder> --recursive --detail -o report.txt
    ScanOffpage {
        /// PDFs and/or folders to scan.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Descend into subfolders.
        #[arg(long, short)]
        recursive: bool,
        /// Ignored fringe in points. A border stroked exactly on the page
        /// edge overhangs by half its line width; reporting that would bury
        /// the real finding. Raising this does not find less — it looks less.
        #[arg(long, default_value_t = pdfcer_core::offpage::DEFAULT_TOLERANCE_PT)]
        tolerance: f64,
        /// Write the report here instead of to the screen.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// List every off-page object, not just the per-page counts.
        #[arg(long)]
        detail: bool,
        /// Print one line per affected FILE and nothing else — the form to
        /// pipe into a copy or a batch.
        #[arg(long, conflicts_with = "detail")]
        files_only: bool,
    },

    /// **Remove content drawn outside the page**, keeping the part that is on
    /// it (`Pass 294.0`, §12.5.6.23).
    ///
    /// Objects wholly off the page are cut away; objects crossing the edge are
    /// CUT AT THE EDGE, so what was on the sheet stays. That is redaction
    /// machinery, not a clip: the off-page bytes are removed from the content
    /// stream rather than hidden, which is the whole point — a clipped path is
    /// a path whose data survives.
    ///
    /// ⚠️ **Destructive and deliberate.** The removed content cannot be
    /// recovered from the output. Keep the input.
    RedactOffpage {
        /// PDFs and/or folders to clean.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// How far beyond the marked bands the residual sweep may act. See
        /// `redact-apply --help`; the same sweep runs here.
        #[arg(long, value_enum, default_value_t = ResidualScopeArg::HiddenCarriers)]
        residual_scope: ResidualScopeArg,
        /// Descend into subfolders.
        #[arg(long, short)]
        recursive: bool,
        /// Output path, for a SINGLE input file.
        #[arg(short, long, conflicts_with = "out_dir")]
        output: Option<PathBuf>,
        /// Output folder, for a batch. The input tree's shape is preserved
        /// under it, so two drawings with the same file name in different
        /// product folders cannot overwrite each other.
        #[arg(long, conflicts_with = "output")]
        out_dir: Option<PathBuf>,
        /// Appended to each output's stem in a batch, before `.pdf`.
        #[arg(long, default_value = "-offpage")]
        suffix: String,
        /// Overwrite an output that already exists. Without this, an existing
        /// output is left alone and reported -- a re-run after a partial batch
        /// resumes rather than redoing.
        #[arg(long)]
        force: bool,
        /// Ignored fringe in points, as `scan-offpage`.
        #[arg(long, default_value_t = pdfcer_core::offpage::DEFAULT_TOLERANCE_PT)]
        tolerance: f64,
        /// Report what would be removed and write nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove pages from a document.
    ///
    /// A page-tree splice: the pages leave the tree, ancestors' counts
    /// drop, and every object the removed pages owned exclusively is
    /// freed. Bookmarks and links that pointed at a removed page are
    /// reported, never silently repaired.
    ///
    /// NOT redaction. Under the default incremental save the removed
    /// pages' bytes remain in the file; deletion removes them from the
    /// document, not from the bytes.
    DeletePages {
        /// Input PDF.
        input: PathBuf,
        /// Pages to remove, 1-based and inclusive, e.g. `2,5-7`.
        #[arg(long)]
        pages: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte (ARCHITECTURE.md §11.1).
        #[arg(long)]
        verify_undo: bool,
    },

    /// Put a document's pages in a new order.
    ///
    /// `--order` is the complete new sequence of 1-based page numbers —
    /// every page exactly once. A list that drops or repeats a page is
    /// refused, because that would be a delete or a duplicate wearing a
    /// reorder's name.
    ReorderPages {
        /// Input PDF.
        input: PathBuf,
        /// The new page order, e.g. `3,1,2` or `5-8,1-4`.
        #[arg(long)]
        order: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Rotate every page, or a selection, by a multiple of 90°.
    ///
    /// `--degrees` is a turn RELATIVE to each page's current rotation,
    /// which is how a rotate-right button behaves and what ISO 32000-1
    /// Table 30 ends up storing (an absolute `/Rotate`, computed as
    /// existing + increment). Pages at different rotations therefore stay
    /// different.
    Rotate {
        /// Input PDF.
        input: PathBuf,
        /// Rotation in degrees; a multiple of 90. Negative turns left.
        #[arg(long, allow_hyphen_values = true)]
        degrees: i32,
        /// Which pages to turn, 1-based. Defaults to all of them.
        #[arg(long, default_value = "all")]
        pages: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Set one page's rotation (ISO 32000-1 Table 30 `/Rotate`).
    ///
    /// Writes the entry on the page object itself, which overrides any
    /// value inherited from an ancestor page-tree node — so rotating one
    /// page never disturbs its siblings. By default the document is
    /// saved as an incremental update, leaving every prior byte (and
    /// therefore every existing signature) intact.
    RotatePage {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to rotate.
        #[arg(long)]
        page: u32,
        /// Rotation in degrees; must be a multiple of 90. Negative and
        /// ≥360 values are accepted and normalized (Table 30 constrains
        /// only "a multiple of 90").
        #[arg(long, allow_hyphen_values = true)]
        degrees: i32,
        /// Treat `--degrees` as a turn relative to the page's current
        /// effective rotation rather than an absolute value.
        #[arg(long)]
        relative: bool,
        /// Output path. The input is never modified.
        ///
        /// This said "Never the input path by default — see `--in-place`"
        /// until 2026-08-27, and **there was no `--in-place` flag on this
        /// subcommand or on any other**. Operator-facing `--help` text
        /// pointed at an option that had never existed. Corrected rather than
        /// silently deleted, because the operator was right to want it: `ocr`
        /// now has `--in-place`, and extending it to the other editing
        /// subcommands is filed rather than done here.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte (ARCHITECTURE.md §11.1). Costs one extra save.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Set one or more pages' sheet size (ISO 32000-1 §7.7.3.3
    /// `/MediaBox`).
    ///
    /// Writes the entry on each page object itself, which overrides any
    /// value inherited from an ancestor page-tree node — so resizing one
    /// page never disturbs its siblings (§7.7.3.4). Content is neither
    /// moved nor scaled: only the sheet boundary changes.
    ///
    /// Either `--size` (with optional `--landscape`) or an explicit
    /// `--width`/`--height` pair in points. `--size` accepts:
    /// `a0`…`a6`, `letter`, `legal`, `tabloid`, `executive`,
    /// `ansi-a`…`ansi-e`.
    SetPageSize {
        /// Input PDF.
        input: PathBuf,
        /// Which pages, 1-based: `3`, `1,4,7`, `2-5`, or `all`.
        #[arg(long, default_value = "1", value_name = "SPEC")]
        pages: String,
        /// A standard sheet size by name (`a1`, `ansi-d`, `letter`, …).
        #[arg(
            long,
            value_name = "NAME",
            required_unless_present = "width",
            conflicts_with_all = ["width", "height"]
        )]
        size: Option<String>,
        /// Use `--size` rotated to landscape (wider than tall) — the
        /// normal orientation for a drawing sheet.
        #[arg(long, requires = "size")]
        landscape: bool,
        /// Custom sheet width in points (1/72 inch). Requires `--height`.
        #[arg(long, requires = "height", value_name = "PT")]
        width: Option<f64>,
        /// Custom sheet height in points (1/72 inch). Requires `--width`.
        #[arg(long, requires = "width", value_name = "PT")]
        height: Option<f64>,
        /// Output path. The input is never modified.
        ///
        /// This said "Never the input path by default — see `--in-place`"
        /// until 2026-08-27, and **there was no `--in-place` flag on this
        /// subcommand or on any other**. Operator-facing `--help` text
        /// pointed at an option that had never existed. Corrected rather than
        /// silently deleted, because the operator was right to want it: `ocr`
        /// now has `--in-place`, and extending it to the other editing
        /// subcommands is filed rather than done here.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte (ARCHITECTURE.md §11.1). Costs one extra save.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Set or clear document information dictionary fields (§14.3.3).
    ///
    /// Creates an `/Info` dictionary if the file has none — the operator
    /// asked for the metadata by name, which is what distinguishes this
    /// from pdfcer stamping its own producer id (see `--producer`).
    SetInfo {
        /// Input PDF.
        input: PathBuf,
        /// New `/Title`.
        #[arg(long)]
        title: Option<String>,
        /// New `/Author`.
        #[arg(long)]
        author: Option<String>,
        /// New `/Subject`.
        #[arg(long)]
        subject: Option<String>,
        /// New `/Keywords`.
        #[arg(long)]
        keywords: Option<String>,
        /// Remove a field entirely. Repeatable. Removal is a distinct
        /// flag rather than "pass an empty string", because an empty
        /// title and an absent title are different things in the file
        /// and a script must be able to ask for either.
        #[arg(long = "clear", value_enum)]
        clear: Vec<InfoFieldArg>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// `/Producer` handling for `--mode full` (ignored otherwise).
        #[arg(long, value_enum, default_value_t = ProducerArg::Preserve)]
        producer: ProducerArg,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte (ARCHITECTURE.md §11.1). Costs one extra save.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Add a geometric-markup annotation to a page (Pass 6.1, §12.5.6).
    ///
    /// Authors a fully-baked `/AP` appearance (R44) and patches the page's
    /// `/Annots` **without touching the page content stream** (R47), saved
    /// incrementally by default so every prior byte (and signature) stays
    /// intact. The subtype selects which geometry flag is read:
    ///
    /// | `--type` | geometry flag | example |
    /// |---|---|---|
    /// | `square`, `circle` | `--rect x0,y0,x1,y1` | `--rect 72,72,300,200` |
    /// | `line` | `--line x0,y0,x1,y1` | `--line 72,100,300,100` |
    /// | `polygon`, `polyline` | `--points "x,y x,y …"` | `--points "72,72 200,72 140,180"` |
    /// | `ink` | `--strokes "x,y x,y \| x,y …"` | `--strokes "72,72 90,120 \| 200,80 230,140"` |
    /// | `highlight`, `underline`, `strikeout`, `squiggly` | `--quads "x1,y1,…,x8,y8 ; …"` or `--rect` (one marquee quad) | `--rect 72,90,300,110` |
    ///
    /// Refusals (all exit `9`, EDIT_REFUSED): an encrypted document, an
    /// enforced certification signature (DocMDP), a page out of range,
    /// empty geometry, or a malformed `/Annots`. A file pdfcer cannot open
    /// exits with the load error's own code (3/4).
    Annotate {
        /// Input PDF.
        input: PathBuf,
        /// The markup subtype to author.
        #[arg(long = "type", value_enum)]
        kind: AnnotKindArg,
        /// 1-based page number to annotate.
        #[arg(long)]
        page: u32,
        /// Rectangle `x0,y0,x1,y1` in default user space — for
        /// `square`/`circle`, and for a marquee text-markup (one quad).
        #[arg(long)]
        rect: Option<String>,
        /// Line `x0,y0,x1,y1` — for `line`.
        #[arg(long)]
        line: Option<String>,
        /// Vertices `x,y x,y …` — for `polygon`/`polyline`.
        #[arg(long)]
        points: Option<String>,
        /// Ink strokes `x,y x,y | x,y x,y` (`|` separates strokes) — for
        /// `ink`.
        #[arg(long)]
        strokes: Option<String>,
        /// Text-markup quads `x1,y1,…,x8,y8 ; …` (Z-order UL,UR,LL,LR) —
        /// overrides `--rect` for the text-markup subtypes.
        #[arg(long)]
        quads: Option<String>,
        /// The text to show — required for `freetext`, `text` (the note's
        /// popup body); optional for `stamp` (defaults to the stamp name).
        #[arg(long)]
        text: Option<String>,
        /// Standard-14 font `BaseFont` name for `freetext`/`stamp`
        /// (`Helvetica`, `Times-Roman`, `Courier`, …). Default `Helvetica`.
        #[arg(long, default_value = "Helvetica")]
        font: String,
        /// Font size in points for `freetext`. `0` = auto-size to the box
        /// height (a reviewable pdfcer heuristic — §12.7.3.3 mandates no
        /// formula). Default `12`.
        #[arg(long, default_value_t = 12.0)]
        size: f64,
        /// Justification for `freetext`: `left`, `center`, or `right`
        /// (`/Q` 0/1/2). Default `left`.
        #[arg(long, value_enum, default_value_t = QuadArg::Left)]
        quad: QuadArg,
        /// Wrap `freetext` to multiple lines within the box.
        #[arg(long)]
        multiline: bool,
        /// Sticky-note icon for `text`: `note` (default), `comment`,
        /// `key`, `help`, `newparagraph`, `paragraph`, `insert`.
        #[arg(long, value_enum, default_value_t = IconArg::Note)]
        icon: IconArg,
        /// Standard stamp name for `stamp` (`draft` default, `approved`,
        /// `confidential`, `final`, `experimental`, `expired`, …).
        #[arg(long, value_enum, default_value_t = StampArg::Draft)]
        stamp_name: StampArg,
        /// Stamp label size in points (default 12). Omit `--stamp-fit` and a
        /// label wider than `--rect` WIDENS the stamp rather than being cut.
        #[arg(long)]
        stamp_font_size: Option<f64>,
        /// What a stamp does when its label does not fit `--rect`:
        /// `grow` (default — widen the stamp), `shrink` (smaller text, same
        /// box), `clip` (cut the label).
        #[arg(long, value_enum, default_value_t = StampFitArg::Grow)]
        stamp_fit: StampFitArg,
        /// Stroke/mark colour as `RRGGBB` hex. Default is per-subtype
        /// (yellow for highlight, red otherwise).
        #[arg(long)]
        color: Option<String>,
        /// Interior fill colour as `RRGGBB` hex (`square`/`circle`/
        /// `polygon`). Absent ⇒ transparent interior.
        #[arg(long)]
        fill: Option<String>,
        /// Border/stroke width in points.
        #[arg(long, default_value_t = 1.0)]
        width: f64,
        /// Draw the border CLOUDY (`/BE << /S /C /I n >>`, ISO 32000-1
        /// §12.5.4 Table 167) at intensity `n` — a revision cloud.
        ///
        /// Valid on `square` and `polygon` only. **`n` is a continuous
        /// value in `0..=2`, not a choice of three**: Table 167 types it
        /// `number` and constrains it "in the range 0 to 2", so `1.5` is
        /// conformant and accepted.
        #[arg(long, value_name = "INTENSITY")]
        cloud: Option<f64>,
        /// Whole-annotation opacity `/CA`, `0.0`-`1.0` (ISO 32000-1
        /// §12.5.2 Table 164). Absent omits the key, which is the
        /// standard's default of fully opaque.
        ///
        /// Applied to the ANNOTATION as composited onto the page, not
        /// inside its appearance stream, so it never compounds with the
        /// appearance's own alpha. Valid for every subtype this
        /// subcommand authors -- Table 164 is the markup-annotation
        /// entry list, and a sticky note is a markup annotation exactly
        /// as a square is.
        ///
        /// Out of range is REFUSED, not clamped: authoring an opaque
        /// annotation and reporting success would hide the one thing the
        /// operator asked for.
        #[arg(long, value_name = "ALPHA")]
        opacity: Option<f64>,
        /// Draw the border DASHED: comma-separated on/off point lengths
        /// (`4,2` = 4 on, 2 off; `3` = the Table 166 default).
        ///
        /// A dashed revision cloud and a dashed leader are ordinary AEC
        /// markup. Omit for a solid border.
        ///
        /// Has no effect on a text markup (highlight, underline,
        /// strike-out, squiggly), which draws no `/BS` border.
        #[arg(long, value_name = "ON,OFF,...")]
        dash: Option<String>,
        /// The note text this annotation carries (`/Contents`, §12.5.2
        /// Table 164) — what a reviewer's comment panel shows as the comment
        /// (`Pass 150.0`).
        ///
        /// Geometric markup could be authored with a shape and a colour but
        /// no words. Every reviewer UI lists such an annotation with an empty
        /// body, which is a comment nobody can read.
        ///
        /// An empty string is accepted and is not the same as omitting this:
        /// it authors an annotation whose author and date are set and whose
        /// text is deliberately blank.
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// The note's author (`/T`) — §12.5.6.4 Table 170 defines it as
        /// *"the name of the person who created the annotation"*.
        ///
        /// Worth setting whenever `--note` is: an annotation with text and no
        /// author renders in every reviewer UI as a note from nobody, which
        /// reads as a broken panel rather than as an anonymous comment.
        /// `list-annotations` prints `author=none` beside it.
        #[arg(long, value_name = "NAME")]
        note_author: Option<String>,
        /// The note's modification date (`/M`), as a **PDF date string**
        /// (§7.9.4): `D:YYYYMMDDHHmmSS` with an optional `Z`/`+`/`-` offset.
        ///
        /// **pdfcer does not read a clock for you, and that is deliberate.**
        /// A wall-clock timestamp would make every authored annotation
        /// unreproducible — byte-identical output for identical input is an
        /// acceptance criterion across this project — and it would be a value
        /// pdfcer invented and wrote silently into your document. You know what
        /// "now" is; pass it.
        ///
        /// Malformed is REFUSED by name rather than written: the read side
        /// hands `/M` straight back as an opaque string, so a garbage date
        /// there looks authoritative and nothing downstream would report it.
        #[arg(long, value_name = "D:YYYYMMDDHHMMSS")]
        note_date: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Mark content for redaction (ISO 32000-1 §12.5.6.23, MARK phase).
    ///
    /// This is the non-destructive first phase: it authors reviewable
    /// `/Redact` annotations, saved into the document, that a later
    /// `redact-apply` turns into true removal. Nothing is removed here —
    /// the marks can be reviewed, moved, or deleted first. Give exactly
    /// one of `--rect`, `--search`, or `--pattern`.
    ///
    /// The saved marks are drawn as a RED OUTLINE, never a filled box, so
    /// a marked-but-unapplied document can never be mistaken for a redacted
    /// one. Verify with `list-redactions`, then run `redact-apply`.
    RedactMark {
        /// Input PDF.
        input: PathBuf,
        /// Mark a single rectangle `x0,y0,x1,y1` (default user space) on
        /// `--page`.
        #[arg(long, group = "how")]
        rect: Option<String>,
        /// Mark every occurrence of this exact text (search-and-redact).
        #[arg(long, group = "how")]
        search: Option<String>,
        /// Mark every match of a simple pattern: literal text where `#`
        /// matches any digit and `?` matches any single character (e.g.
        /// `###-##-####` for a US SSN).
        #[arg(long, group = "how")]
        pattern: Option<String>,
        /// Case-insensitive matching for `--search`/`--pattern` (ASCII).
        #[arg(long)]
        ignore_case: bool,
        /// 1-based page for `--rect` (ignored by search/pattern, which
        /// scan every page).
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Fill colour (`/IC`) applied to the region ON APPLY, as `RRGGBB`
        /// hex. Applies to `--rect`, `--search` and `--pattern` alike.
        ///
        /// OMITTING IT LEAVES THE REGION TRANSPARENT, not black: ISO
        /// 32000-1 Table 192 says an absent `/IC` leaves the interior
        /// transparent. Pass `--fill 000000` for a black box.
        #[arg(long)]
        fill: Option<String>,
        /// Overlay text (`/OverlayText`) drawn over the fill on apply,
        /// wrapped and clipped to the region. Applies to `--rect`,
        /// `--search` and `--pattern` alike.
        ///
        /// Base-14 Latin this build: a character with no WinAnsi code is
        /// drawn as `?` and counted in the `redact-apply` report.
        #[arg(long)]
        overlay_text: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// **Remove embedded font programs** — a DRY RUN unless `--apply`.
    ///
    /// The first destructive font operation. It strikes `/FontFile`,
    /// `/FontFile2` or `/FontFile3` from the `/FontDescriptor` (§9.9
    /// Table 126), leaving a font reference the reader satisfies by
    /// substitution, and frees the program's object.
    ///
    /// ONLY a font whose `list-fonts` verdict is `removable` may go.
    /// Every other font is refused **by name, with its reason printed** —
    /// never silently, never merely missing from the output. That is a
    /// deliberate divergence from Acrobat, which refuses the same fonts by
    /// leaving them out of its list with no explanation anywhere. Measured
    /// over 4,023 real files, refusal is the majority case by a wide margin:
    /// of 1,560 embedded fonts, 28.8 % removable, 53.6 % symbolic with a
    /// built-in encoding, 12.9 % glyph-index encoded, 4.4 % embedded CMap.
    ///
    /// APPEARANCE CHANGES. `/Widths` is preserved, so every glyph keeps
    /// its exact advance, but the substituted face's own shapes and widths
    /// are not those numbers. Text sits in the same places and looks
    /// different. This is a certainty, not a risk.
    ///
    /// BYTES ARE RECLAIMED BY `--mode full`, NOT by the default
    /// incremental save. An incremental update appends a revision; the
    /// freed program's bytes stay in the prior revision and the file gets
    /// LARGER. Both numbers are printed so the difference cannot be missed.
    ///
    /// By default the six-letter §9.6.4 subset tag is stripped from
    /// `/BaseFont` and `/FontName` together (Table 122 makes them equal by
    /// `shall`), because `ABCDEF+Arial` matches no installed font once the
    /// program is gone. `--keep-subset-tag` leaves both alone.
    ///
    /// A PDF/A-identified document is refused unless `--acknowledge-pdfa`:
    /// every part of ISO 19005 requires embedded fonts, so unembedding
    /// breaks the conformance the file claims about itself.
    ///
    /// WHICH FONTS IS REQUIRED — pass `--all-removable` to take every font
    /// whose verdict is `removable`, or name them individually with
    /// `--font`. There is no default: pdfcer does not guess at the scope of
    /// an edit, least of all a destructive one.
    // Same `required(true)` hazard as `embed-font` above, shipped in the same
    // Pass and found by checking whether that bug was a one-off. It was not:
    // `unembed-font <file>` parsed cleanly, selected nothing, and printed
    // `fonts=0 refused=0 unmatched=0` followed by "Every refusal is printed
    // above with its reason" — with nothing printed.
    #[command(group = clap::ArgGroup::new("which").required(true))]
    UnembedFont {
        /// Input PDF.
        input: PathBuf,
        /// A font to unembed, by `/BaseFont` or by its family name — both
        /// `ABCDEF+Arial` and `Arial` work. Repeatable. A name that matches
        /// nothing is reported and exits non-zero.
        #[arg(long, group = "which")]
        font: Vec<String>,
        /// Unembed every font whose verdict is `removable`.
        #[arg(long, group = "which")]
        all_removable: bool,
        /// Actually write the output. Without it this is a DRY RUN: the
        /// full report is printed and no file is written.
        ///
        /// Inverted from most tools on purpose, the same way `print`
        /// requires `--send`: this removes something the file cannot get
        /// back, so the default has to be the one that cannot surprise
        /// anybody.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Which save path to use. `full` is the one that actually
        /// reclaims the bytes — see the command description.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Verify that undoing the operation reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
        /// Leave the §9.6.4 subset tag on `/BaseFont` and `/FontName`.
        #[arg(long)]
        keep_subset_tag: bool,
        /// Proceed on a document that identifies itself as PDF/A.
        /// Unembedding breaks that conformance; this says you know.
        #[arg(long)]
        acknowledge_pdfa: bool,
    },

    /// **Embed the programs for fonts the document ALREADY NAMES** — a DRY
    /// RUN unless `--apply`.
    ///
    /// **It does not add a new font to the document.** This fills in the
    /// missing *program* for a face the PDF already references; it cannot
    /// introduce a typeface the file does not use. Nothing currently can —
    /// `format-text --set-font` selects only among the fonts a page already
    /// carries. The heading previously read *"add the font programs a
    /// document is missing"*, which is true and reads like the other thing.
    ///
    /// The constructive mirror of `unembed-font`, and the fix for the one
    /// thing every print-on-demand service rejects a book for: a font the
    /// PDF names but does not carry. `list-fonts` reports it as
    /// `not-embedded=N`; this drives that number down and prints what is
    /// left.
    ///
    /// THE SOURCE FONTS COME FROM `--font-dir`. pdfcer never goes looking
    /// on its own. Point it at a folder holding the faces — on Windows,
    /// `--font-dir C:\Windows\Fonts` — and every face there is matched
    /// against the document's font names. A font nothing answers to is
    /// reported BY NAME, with what would satisfy it.
    ///
    /// CHARACTER POSITIONS DO NOT MOVE. A PDF spaces text from its own
    /// `/Widths` array, never from the font program (§9.6.2.1 Table 111),
    /// and this command either leaves that array untouched or writes it from
    /// the Adobe Core-14 metrics a reader was already applying. What changes
    /// is the letterforms. That is a certainty in both directions: the
    /// layout is safe, and the shapes WILL differ where the face is not the
    /// original.
    ///
    /// EXACT vs SUBSTITUTE is printed per font. `exact` means the folder
    /// held the face the document names. `alias` means a metric-compatible
    /// stand-in was used (`Helvetica` → `Arial`). `bundled` means one of
    /// pdfcer's own substitute faces, which is off unless
    /// `--use-bundled-fonts` is passed.
    ///
    /// A font whose own licensing field says it may not be embedded is
    /// refused by name (§9.9). So are composite (CID) fonts, whose character
    /// codes are positions inside the specific program that is missing —
    /// no other face can stand in for one without drawing the wrong
    /// characters.
    ///
    /// The file gets BIGGER. Programs are compressed on the way in, and both
    /// save modes keep them.
    ///
    /// WHICH FONTS IS REQUIRED — pass `--all-missing` to take every font
    /// the document is missing, or name them individually with `--font`.
    /// There is no default: pdfcer does not guess at the scope of an edit.
    // `required(true)` IS LOAD-BEARING, not tidiness. `#[arg(group = "x")]`
    // makes a group that enforces mutual exclusion but is NOT required by
    // default, so "exactly one of these" silently means "zero or one of
    // these". With neither flag the selection parsed as an empty NAME LIST,
    // and the resulting no-op was invisible in every direction: an empty
    // name list selects no fonts, produces no `unmatched` rows (no name
    // failed to match), and under a non-`AllMissing` selection an unselected
    // font is deliberately not reported as a refusal. The operator got
    // `fonts=0 refused=0 unmatched=0` over a document with missing fonts and
    // a font folder that could resolve them — a shipped feature that read as
    // completely broken. Every pre-existing test passed `--all-missing` and
    // so could not see it (R151's shape). `unembed-font` had it too.
    #[command(group = clap::ArgGroup::new("which-embed").required(true))]
    EmbedFont {
        /// Input PDF.
        input: PathBuf,
        /// A font to embed into, by `/BaseFont` or family name — both
        /// `ABCDEF+Arial` and `Arial` work. Repeatable. A name that matches
        /// nothing is reported and exits non-zero.
        #[arg(long, group = "which-embed")]
        font: Vec<String>,
        /// Embed into every font the document does not carry a program for.
        #[arg(long, group = "which-embed")]
        all_missing: bool,
        /// A folder of font files to resolve the document's font names
        /// against. Repeatable; later folders win a duplicate name.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Also offer pdfcer's own bundled standard-14 substitute faces when
        /// no supplied folder answers to a name.
        ///
        /// OFF by default, and not for a technical reason: the bundled
        /// faces are BSD-3-Clause (see `THIRD_PARTY_LICENSES.md`), and
        /// embedding one puts it inside a document you then distribute —
        /// which carries that licence's attribution condition with it.
        /// That is your decision to make, so pdfcer does not make it for you.
        #[arg(long)]
        use_bundled_fonts: bool,
        /// Actually write the output. Without it this is a DRY RUN: the full
        /// report is printed and no file is written.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Which save path to use. Both keep the embedded programs;
        /// `incremental` leaves the input revision byte-identical.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Verify that undoing the operation reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Apply redactions: TRULY REMOVE the marked content (§12.5.6.23).
    ///
    /// The one destructive, irreversible operation in pdfcer (R35). It
    /// removes the covered glyphs from the content stream (advance-
    /// preserving, so surviving text stays put), CUTS vector paths at the
    /// region boundary (strokes against the region widened by their stroke
    /// width, fills to the region's complement; a path wholly inside is
    /// deleted), DESTROYS the covered
    /// samples of any image a region touches (decoded, cleared, re-encoded
    /// losslessly; an image a region contains entirely is removed outright;
    /// an image also painted elsewhere is copied so the other placements
    /// keep it), scrubs duplicating carriers (`/Info`, XMP), decomposes
    /// object streams so no removed object survives compressed, removes the
    /// marks, and writes a FORCED FULL REWRITE with no prior revision — so
    /// nothing is recoverable.
    ///
    /// It prints a REDACTION REPORT of exactly what was removed and which
    /// carriers were scrubbed or left. If any carrier could not be scrubbed
    /// (XFA, a tagged ActualText copy, an attachment, a malformed vector
    /// path that could not be cut), apply exits non-zero UNLESS you pass
    /// `--acknowledge-residuals` — there is no path
    /// where a partial redaction reads as complete. A mark over an image
    /// whose samples pdfcer cannot decode is RETAINED in the output (left
    /// unapplied, named in the report with its reason) while every other
    /// mark applies; only when no mark at all can be applied is the run
    /// refused.
    ///
    /// AFTER the surgery it runs a RESIDUAL SWEEP over every object in the
    /// file, looking for the redacted text surviving somewhere the marks did
    /// not cover -- a document-information entry, an XMP packet, a superseded
    /// content stream. `--residual-scope` governs how far that sweep may ACT;
    /// it never governs what it REPORTS. The default scrubs carriers you
    /// cannot see and leaves drawn page content alone, because removing an
    /// unmarked occurrence of a common word is destruction, not diligence.
    RedactApply {
        /// Input PDF carrying `/Redact` marks.
        input: PathBuf,
        /// Output path for the redacted document.
        #[arg(short, long)]
        output: PathBuf,
        /// How far beyond the marked regions the residual sweep may act.
        /// Every match a narrower scope declines is still counted and named.
        #[arg(long, value_enum, default_value_t = ResidualScopeArg::HiddenCarriers)]
        residual_scope: ResidualScopeArg,
        /// Acknowledge disclosed, un-scrubbed carrier residuals and exit 0
        /// anyway (the removal itself always happens; this only governs the
        /// exit code for the disclosed residuals).
        #[arg(long)]
        acknowledge_residuals: bool,
    },

    /// Encrypt a document with **AES-256 (`/R` 6)** and a user and/or owner
    /// password (`Pass 5.4`).
    ///
    /// Only AES-256 `/R` 6 is written — no RC4, no `/R` 2–5. An empty user
    /// password (omit `--user-password`) makes a permissions-only document that
    /// opens with no prompt but still carries the `/P` bits.
    ///
    /// Permissions default to ALL granted. Restrict with `--allow` (grant only
    /// the listed bits) and/or `--deny` (remove bits from the granted set).
    /// Bit names: `print`, `print-high-quality`, `modify-contents`, `copy`,
    /// `annotate`, `fill-forms`, `accessibility-extract`, `assemble`.
    ///
    /// PDF permissions are a request, not a lock — see the notice this prints.
    Encrypt {
        /// The document to encrypt.
        input: PathBuf,
        /// Where to write the encrypted document.
        output: PathBuf,
        /// The user password (opens the document; `/P`-limited). Omit for a
        /// permissions-only document. Prefer `--user-password-file`.
        #[arg(long)]
        user_password: Option<String>,
        /// Read the user password from a file (first line; `-` for stdin).
        #[arg(long, conflicts_with = "user_password")]
        user_password_file: Option<PathBuf>,
        /// The owner password (full access regardless of `/P`). Prefer
        /// `--owner-password-file`.
        #[arg(long)]
        owner_password: Option<String>,
        /// Read the owner password from a file (first line; `-` for stdin).
        #[arg(long, conflicts_with = "owner_password")]
        owner_password_file: Option<PathBuf>,
        /// Grant ONLY these permission bits (comma-separated or repeated).
        /// Default: all granted.
        #[arg(long, value_delimiter = ',')]
        allow: Vec<String>,
        /// Remove these permission bits from the granted set (comma-separated
        /// or repeated).
        #[arg(long, value_delimiter = ',')]
        deny: Vec<String>,
        /// Leave the `/Metadata` stream in clear (default: encrypt it too).
        #[arg(long)]
        no_encrypt_metadata: bool,
    },
    /// Re-key an ENCRYPTED document with a new permission set (and optionally
    /// new passwords), keeping AES-256 `/R` 6 (`Pass 5.4`).
    ///
    /// Owner-only: open the document with the OWNER password via
    /// `--open-password`/`--open-password-file`. `/P` is bound into the
    /// encryption at the byte level, so setting it re-derives the whole
    /// `/Encrypt` dictionary under a fresh key.
    SetPermissions {
        /// The encrypted document to re-key.
        input: PathBuf,
        /// Where to write the re-keyed document.
        output: PathBuf,
        /// The new user password (default: reuse via `--user-password`).
        #[arg(long)]
        user_password: Option<String>,
        /// Read the new user password from a file (first line; `-` for stdin).
        #[arg(long, conflicts_with = "user_password")]
        user_password_file: Option<PathBuf>,
        /// The new owner password.
        #[arg(long)]
        owner_password: Option<String>,
        /// Read the new owner password from a file (first line; `-` for stdin).
        #[arg(long, conflicts_with = "owner_password")]
        owner_password_file: Option<PathBuf>,
        /// Grant ONLY these permission bits. Default: all granted.
        #[arg(long, value_delimiter = ',')]
        allow: Vec<String>,
        /// Remove these permission bits from the granted set.
        #[arg(long, value_delimiter = ',')]
        deny: Vec<String>,
        /// Leave the `/Metadata` stream in clear (default: encrypt it too).
        #[arg(long)]
        no_encrypt_metadata: bool,
    },
    /// Remove encryption from a document (`Pass 5.4`).
    ///
    /// Owner-only: open the document with the OWNER password via
    /// `--open-password`/`--open-password-file`. The output is a plaintext
    /// document that opens with no password.
    RemoveEncryption {
        /// The encrypted document.
        input: PathBuf,
        /// Where to write the plaintext document.
        output: PathBuf,
    },
    /// List the trust anchors in an installed Acrobat/Reader trust store
    /// (`Pass 10.2`) — READ-ONLY.
    ///
    /// Adobe's downloaded AATL and EU Trusted Lists (EUTL) certificates live in
    /// `addressbook.acrodata` (a `%PPKLITE-` COS file). This reads that store
    /// and reports what it trusts, so you can see the anchor set BEFORE anything
    /// relies on it. It changes nothing and contacts no network.
    ///
    /// By default it auto-locates `%APPDATA%\Adobe\Acrobat\<track>\Security\
    /// addressbook.acrodata`; pass `--file` to point at a specific one.
    ///
    /// NOTE: pdfcer does not yet EVALUATE signatures against these anchors
    /// (that is a later, opt-in step); this subcommand only shows the store.
    /// Reading Adobe's own already-downloaded file is a local read of your
    /// file; whether relying on it fits Adobe's Reader licence is your call.
    TrustStoreList {
        /// Read a specific `addressbook.acrodata` instead of auto-locating.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Show only anchors from this list: `aatl`, `eutl`, `adbe`, or `all`
        /// (default `all`).
        #[arg(long, default_value = "all")]
        source: String,
        /// List every anchor's subject/issuer, not just the per-source counts.
        #[arg(long)]
        identities: bool,
    },
    /// List the `/Redact` marks awaiting apply in a document.
    ///
    /// Reports the count and per-page inventory computed from the
    /// document's own annotations (never a session counter), so a script
    /// can detect a marked-but-not-applied file before shipping it.
    ListRedactions {
        /// Input PDF.
        input: PathBuf,
    },

    /// Stamp Bates numbers across a batch of PDFs. [not yet implemented]
    BatesStamp {
        /// Input PDFs to stamp.
        inputs: Vec<PathBuf>,
        /// Starting number.
        #[arg(long, default_value_t = 1)]
        start: u64,
        /// Format string, e.g. `DOC-{:06}`.
        #[arg(long, default_value = "{:06}")]
        format: String,
    },

    /// Convert a PDF to a PDF/A conformance level. [not yet implemented]
    ToPdfa {
        /// Input PDF.
        input: PathBuf,
        /// PDF/A level, e.g. `2b`.
        #[arg(long)]
        level: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Validate a PDF against a PDF/A profile and print a report.
    /// [not yet implemented]
    ValidatePdfa {
        /// Input PDF to validate.
        input: PathBuf,
    },

    #[cfg(feature = "signing")]
    /// **Sign a PDF** with a PKCS#12 digital ID — PAdES B-B by default
    /// (`Pass 10.9`; ISO 32000-1 §12.8, ETSI EN 319 142-1).
    ///
    /// The signature is appended as an incremental update, so every byte of
    /// the input — and every signature already in it — is preserved; a second
    /// `sign` on the output adds a second signature and the first still
    /// verifies. The `.pfx`'s integrity MAC is checked first (a wrong
    /// `--password` fails there, by name), both PKCS#12 encryption eras are
    /// read, and the signer's certificate chain is embedded in the CMS. Before
    /// the file is written pdfcer re-reads it and verifies its own signature
    /// with `verify-signatures`' engine; a signature pdfcer cannot verify is
    /// never written.
    ///
    /// `--format`: `cades` (default) writes `/SubFilter /ETSI.CAdES.detached`
    /// (PAdES); `pkcs7` writes `adbe.pkcs7.detached` (the widest legacy
    /// reader support). Both are CMS, both detached, both carry the
    /// `signing-certificate-v2` attribute. `--algorithm` defaults to the key's
    /// own: RSA PKCS#1 v1.5 for an RSA key, ECDSA for an EC key; `rsa-pss` is
    /// the PAdES-preferred RSA scheme.
    ///
    /// `--signing-time` is the `/M` the signature claims, as a PDF date
    /// (`D:YYYYMMDDHHmmSSZ`). pdfcer's engine reads no clock; when the flag
    /// is absent THIS COMMAND derives the time from the system clock and
    /// PRINTS that it did (project rule 4 — a derived value is disclosed,
    /// never silent). `--reason`, `--location`, `--contact`, `--name` are the
    /// signer's own words, written verbatim.
    ///
    /// Invisible by default (`/Rect [0 0 0 0]`, nothing drawn). `--visible
    /// x0,y0,x1,y1` places a widget on `--page` showing a frame and, in
    /// Helvetica shrunk to fit, the signer's name, the date and
    /// `--reason`/`--location` when given; the lines are printed on success,
    /// and a box too small for them at 4 pt is refused by name.
    ///
    /// `--certify` (with `--mdp-level none|form-fill|annotate`) makes it a
    /// CERTIFICATION signature: the document's author signature, carrying
    /// the DocMDP permission every conforming reader enforces on later
    /// changes. It must be the first signature and there is one per
    /// document; both are refused by name. The level and its meaning are
    /// printed; `verify-signatures` reports them back.
    ///
    /// Refused by name, nothing written: a signing time that is not a PDF
    /// date; an encrypted document (the incremental writer cannot append to
    /// one yet); a document opened through cross-reference recovery; a
    /// certification signature whose `/DocMDP` permission is 1 (no changes);
    /// a colliding `--field-name`; a `--page` out of range; a CMS larger than
    /// `--reserve` bytes (default 12288 — raise it for a long chain).
    ///
    /// The level printed is always `B-B`: pdfcer embeds no timestamp and no
    /// revocation data yet, and never claims a level the material does not
    /// support. Exit 12 when the signature was refused or failed
    /// self-verification; 9 for a refused request; 3 for a file that cannot
    /// be read or written.
    Sign {
        /// Input PDF.
        input: PathBuf,
        /// PKCS#12 (`.p12`/`.pfx`) digital ID: private key + certificate chain.
        #[arg(long)]
        cert: PathBuf,
        /// The container's password. Empty if omitted.
        #[arg(long, default_value = "")]
        password: String,
        /// Output path for the signed PDF.
        #[arg(short, long)]
        output: PathBuf,
        /// `cades` (PAdES, default) or `pkcs7` (`adbe.pkcs7.detached`).
        #[arg(long, value_enum, default_value_t = SignFormatArg::Cades)]
        format: SignFormatArg,
        /// Signature algorithm; default is the key's own (RSA PKCS#1 v1.5 or
        /// ECDSA).
        #[arg(long, value_enum)]
        algorithm: Option<SignAlgorithmArg>,
        /// The claimed signing time as a PDF date (`D:20260905120000Z`).
        /// Derived from the system clock, and disclosed, when absent.
        #[arg(long)]
        signing_time: Option<String>,
        /// `/Name` — the signer's display name.
        #[arg(long)]
        name: Option<String>,
        /// `/Reason` — why the document is being signed.
        #[arg(long)]
        reason: Option<String>,
        /// `/Location` — where, in the signer's words.
        #[arg(long)]
        location: Option<String>,
        /// `/ContactInfo`.
        #[arg(long)]
        contact: Option<String>,
        /// Make this a CERTIFICATION (author) signature (`Pass 10.12`, ISO
        /// 32000-1 §12.8.2.2): writes the `/DocMDP` transform and the
        /// catalog's `/Perms`, which every conforming reader enforces on
        /// later changes. Must be the document's FIRST signature, and a
        /// document carries only one — both refused by name. Level from
        /// `--mdp-level`; without it, the standard's default (`form-fill`,
        /// P=2), printed so the choice is never silent.
        #[arg(long)]
        certify: bool,
        /// What a certified document still permits (Table 254): `none`
        /// (P=1, any change invalidates), `form-fill` (P=2, form fill-in
        /// and signing — the default), `annotate` (P=3, also annotations).
        /// Implies `--certify`.
        #[arg(long, value_enum)]
        mdp_level: Option<MdpLevelArg>,
        /// The signature field's name; `Signature1`, `Signature2`, … when
        /// absent. Naming an EXISTING, empty `/FT /Sig` field signs INTO it
        /// (`Pass 10.13`): its own rectangle and page place the signature,
        /// a `/Lock` on it becomes a `/FieldMDP` lock, and its seed-value
        /// constraints (`/SV`) are enforced in full — a required one this
        /// request does not meet, or one pdfcer cannot evaluate, is refused
        /// by name. An already-signed field, a non-signature field, or
        /// `--visible` alongside an existing field are refused.
        #[arg(long)]
        field_name: Option<String>,
        /// Make the signature visible: the widget rectangle `x0,y0,x1,y1` in
        /// points on `--page`. The box shows a frame and, in Helvetica shrunk
        /// to fit, the signer's name, the date, and `--reason`/`--location`
        /// when given; a box too small for that text at 4 pt is refused by
        /// name (never clipped). The lines are printed on success.
        #[arg(long, allow_hyphen_values = true)]
        visible: Option<String>,
        /// 1-based page for `--visible`.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Bytes reserved for the signature blob.
        #[arg(long, default_value_t = 12 * 1024)]
        reserve: usize,
    },

    /// **List the document's bookmarks** (ISO 32000-1 §12.3.3).
    ///
    /// Reports the outline tree with each item's nesting level, its
    /// open/closed state, and where it points. Read-only.
    ListOutline {
        /// Input PDF.
        input: PathBuf,
        /// Print one line per bookmark with no indentation, for scripts
        /// that parse rather than read. `level=` is present either way.
        #[arg(long)]
        flat: bool,
        /// Emit JSON instead: one array of bookmark objects, each with
        /// `title`, `level`, and — when the bookmark opens another file —
        /// the `file` it names.
        ///
        /// For the case this was built for: a table-of-contents PDF whose
        /// bookmarks launch the other PDFs in a folder. `title` + `file`
        /// is the pair to read; everything else is context.
        #[arg(long, conflicts_with = "flat")]
        json: bool,
    },

    /// **Rename a bookmark** — its `/Title` (`Pass 157.0`).
    ///
    /// The commonest bookmark edit. Identify the item by its `n=` number from
    /// `list-outline`, which numbers every item in reading order.
    RenameBookmark {
        /// Input PDF.
        input: PathBuf,
        /// Which bookmark — the `n=` value `list-outline` prints, 1-based.
        #[arg(long)]
        n: usize,
        /// The new title.
        #[arg(long)]
        title: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Delete a bookmark and everything under it** (`Pass 157.0`).
    ///
    /// The subtree goes too, as Acrobat does — promoting orphaned children
    /// would silently reorganise a document's navigation, splicing a deleted
    /// chapter's sections into the top level.
    ///
    /// Relinks the sibling chain and fixes `/Count` on every open ancestor and
    /// on the root, which counts a different quantity (§12.3.3 Tables
    /// 152–153). The outline ROOT itself is refused by name.
    DeleteBookmark {
        /// Input PDF.
        input: PathBuf,
        /// Which bookmark — the `n=` value `list-outline` prints, 1-based.
        #[arg(long)]
        n: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Move a bookmark** — reorder it among its siblings, or nest it under a
    /// different one (`Pass 161.0`).
    ///
    /// The bookmark's whole subtree travels with it, as Acrobat does: a
    /// chapter moved under a different part takes its sections along. Its
    /// destination is untouched — a move changes where a bookmark *sits*, not
    /// where it *goes*.
    ///
    /// Every bookmark is named by the `n=` number `list-outline` prints, so
    /// the two commands compose: read the tree, pick a row, pick a target.
    ///
    /// # Choosing the destination
    ///
    /// Exactly one of these:
    ///
    /// * `--before N` / `--after N` — land next to bookmark `N`, under
    ///   whatever parent `N` has. This is how you reorder.
    /// * `--under N` — become the LAST child of bookmark `N`, which is where
    ///   `add-bookmark` puts a new one. Add `--first` for the first child
    ///   instead.
    /// * `--to-top-level` — become a top-level bookmark, last; `--first`
    ///   makes it the first.
    ///
    /// # What it refuses, and why each one matters
    ///
    /// Moving a bookmark under one of its own descendants is **refused**: it
    /// would make the outline's `/Parent` chain a cycle, producing a file that
    /// still opens and that a reader without a depth guard walks forever.
    ///
    /// Moving a bookmark to where it already is is **not** an error — it
    /// reports `moved=0` and writes nothing, because a script rebuilding an
    /// outline issues redundant moves by construction.
    ///
    /// # `/Count` is maintained for you
    ///
    /// Every ancestor carries a count of the items visible beneath it, and the
    /// two branches of a move must be adjusted in opposite directions
    /// (§12.3.3 Tables 152–153). Moving a bookmark into a COLLAPSED parent
    /// hides it, so the counts above go DOWN even though nothing was deleted —
    /// that is correct, and `visible=` reports how many items moved.
    MoveBookmark {
        /// Input PDF.
        input: PathBuf,
        /// Which bookmark to move — the `n=` value `list-outline` prints,
        /// 1-based.
        #[arg(long)]
        n: usize,
        /// Land immediately before bookmark N, under N's parent.
        #[arg(long, group = "destination")]
        before: Option<usize>,
        /// Land immediately after bookmark N, under N's parent.
        #[arg(long, group = "destination")]
        after: Option<usize>,
        /// Become a child of bookmark N — last by default, first with
        /// `--first`.
        #[arg(long, group = "destination")]
        under: Option<usize>,
        /// Become a top-level bookmark — last by default, first with
        /// `--first`.
        #[arg(long, group = "destination")]
        to_top_level: bool,
        /// With `--under` or `--to-top-level`, land FIRST among the children
        /// rather than last.
        #[arg(long)]
        first: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Expand or collapse a bookmark** (`Pass 161.0`).
    ///
    /// A bookmark with children is stored open or closed, and the state
    /// survives a save — it is the sign of `/Count` (§12.3.3 Table 153), not a
    /// viewer preference. This sets it.
    ///
    /// # Why this is separate from `move-bookmark`
    ///
    /// Moving a bookmark into a collapsed parent leaves it hidden. Whether a
    /// tool should then expand that parent is a real two-answer question —
    /// revealing respects "I just put it there", preserving respects "I
    /// collapsed that on purpose" — and Acrobat's own behaviour could not be
    /// sourced either way. pdfcer ships both: the move preserves, and this
    /// expands. Run them together for reveal-on-move.
    ///
    /// A bookmark with no children has no expansion state and reports
    /// `changed=0` rather than failing, so a sweep over every row does not
    /// have to filter first.
    SetBookmarkOpen {
        /// Input PDF.
        input: PathBuf,
        /// Which bookmark — the `n=` value `list-outline` prints, 1-based.
        #[arg(long)]
        n: usize,
        /// Collapse it instead of expanding it.
        #[arg(long)]
        collapse: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Add a bookmark to the document outline** (ISO 32000-1 §12.3.3).
    ///
    /// Appends one item as the LAST child of its parent — the top level by
    /// default, or under an existing bookmark with `--under`.
    ///
    /// `--under` takes the `n=` number `list-outline` prints, so the two
    /// commands compose directly: read the tree, pick a row, nest under it.
    /// Depth-first document order, 1-based.
    ///
    /// # `/Count` is maintained for you, and that is the whole difficulty
    ///
    /// A bookmark is not just a dictionary: every ancestor carries a count
    /// of the items visible beneath it, and a viewer's panel disagrees with
    /// the file if those are wrong. pdfcer propagates them per §12.3.3,
    /// stopping at the first CLOSED ancestor because nothing below one is
    /// visible. Adding under a collapsed bookmark therefore leaves the
    /// document's total unchanged — correct, and worth knowing before you
    /// read the `root_count=` field and think it failed.
    AddBookmark {
        /// Input PDF.
        input: PathBuf,
        /// The bookmark's text.
        #[arg(long)]
        title: String,
        /// Destination page, 1-based. Omit for a heading — a bookmark with
        /// no destination is legal and common for a container row.
        #[arg(long)]
        page: Option<u32>,
        /// Scroll so this user-space Y coordinate is at the top of the
        /// window, instead of fitting the whole page. Requires `--page`.
        #[arg(long)]
        top: Option<f64>,
        /// Nest under the bookmark with this `n=` number from
        /// `list-outline`. Omit for a top-level bookmark.
        #[arg(long)]
        under: Option<usize>,
        /// Point at a NAMED destination instead of a page (§12.3.2.3).
        /// Mutually exclusive with `--page`. Define it first with
        /// `add-named-dest`; an undefined name is refused, because a
        /// bookmark that scrolls nowhere still looks like a working one.
        #[arg(long, conflicts_with_all = ["page", "top"])]
        dest_name: Option<String>,
        /// Output PDF.
        #[arg(long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Round-trip the undo stack before saving and report whether the
        /// document returned to its original bytes.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Register an existing widget annotation as a form field**
    /// (ISO 32000-1 §12.7.3).
    ///
    /// For controls that are drawn on a page but belong to no field — the
    /// state `insert-pages` leaves behind, because it copies the page and
    /// its annotations without merging the document-level `/AcroForm`.
    /// The widget keeps its geometry, appearance and value; only the
    /// registration is written.
    ///
    /// Addressed by the `page=`/`index=` pair `list-annotations` prints,
    /// because an unregistered widget has no name for `list-fields` to
    /// show.
    ///
    /// # Two widgets that look identical and are not
    ///
    /// §12.7.3.1 allows a field and its single widget to be ONE dictionary,
    /// and most producers take that option — such a widget carries its own
    /// `/T`, `/FT` and `/V`, and adopts losslessly. A widget that was a KID
    /// of a field (how a radio group is represented) carries none of those,
    /// and its `/Parent` did not survive the copy. It is refused unless you
    /// supply `--name`, because anything pdfcer chose would be a name the
    /// source never used.
    AdoptWidget {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=`
        /// value `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// Field name. Omit to keep the widget's own `/T`. Required for a
        /// widget that has none, and the way to resolve a name collision.
        #[arg(long)]
        name: Option<String>,
        /// Report what would happen and write nothing — including the name
        /// the widget already carries, which is in the file rather than on
        /// screen, and whether the field will have a usable `/FT`. Skips
        /// `--output` entirely.
        #[arg(long, conflicts_with_all = ["output", "mode", "verify_undo"])]
        dry_run: bool,
        /// Output PDF. Required unless `--dry-run`.
        #[arg(long, required_unless_present = "dry_run")]
        output: Option<PathBuf>,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Round-trip the undo stack before saving and report whether the
        /// document returned to its original bytes.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Define a named destination** (ISO 32000-1 §12.3.2.3, §7.9.6).
    ///
    /// A named destination is a level of indirection: bookmarks and links
    /// refer to the NAME, and the name resolves to a page and view. That is
    /// what lets a document's internal links survive a page reorder — the
    /// name moves with the destination and nothing pointing at it has to be
    /// rewritten.
    ///
    /// Written into the PDF 1.2 `/Names` → `/Dests` name tree. A collision
    /// is checked against **both** namespaces (the tree and the legacy PDF
    /// 1.1 catalog `/Dests` dictionary) and refused, because the two have no
    /// defined precedence and a key in both is an anomaly.
    AddNamedDest {
        /// Input PDF.
        input: PathBuf,
        /// The destination name. Interpreted as bytes; §7.9.6 imposes no
        /// encoding on name-tree keys.
        #[arg(long)]
        name: String,
        /// Destination page, 1-based.
        #[arg(long)]
        page: u32,
        /// Scroll so this user-space Y coordinate is at the top of the
        /// window, instead of fitting the whole page.
        #[arg(long)]
        top: Option<f64>,
        /// Output PDF.
        #[arg(long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Round-trip the undo stack before saving and report whether the
        /// document returned to its original bytes.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Merge a whole document into this one**, preserving the session
    /// (ISO 32000-1 §12.7.3 for the form half).
    ///
    /// Unlike `insert-pages`, which builds a NEW document, this edits the
    /// input incrementally and carries the source's form fields across so
    /// they arrive **fillable** rather than as boxes nothing can fill.
    ///
    /// Field-name collisions are RENAMED (`Address` -> `Address_2`) and
    /// counted. §12.7.3.1 makes the fully qualified name a field's identity,
    /// so leaving a duplicate would make one field with two widgets, where
    /// filling either fills both.
    MergeDocument {
        /// The document to merge INTO. Edited incrementally.
        input: PathBuf,
        /// The document to merge in. Every page of it comes across.
        #[arg(long)]
        source: PathBuf,
        /// Put the merged pages before this 1-based target page.
        #[arg(long, conflicts_with_all = ["after", "at_start"])]
        before: Option<usize>,
        /// Put the merged pages after this 1-based target page.
        #[arg(long, conflicts_with_all = ["before", "at_start"])]
        after: Option<usize>,
        /// Put the merged pages at the very front.
        #[arg(long, conflicts_with_all = ["before", "after"])]
        at_start: bool,
        /// Output PDF.
        #[arg(long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Round-trip the undo stack before saving and report whether the
        /// document returned to its original bytes.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Report what each signature COVERS** — not whether it is valid.
    ///
    /// This measures each signature's `/ByteRange` (§12.8.1) against the
    /// file's real length and reports what it protects, which answers a
    /// question a validity badge does not: was anything added beyond the
    /// signed range? It computes no digest; `verify-signatures` does.
    ///
    /// A signature can be cryptographically perfect over the first 40 KB
    /// of a 900 KB file.
    ListSignatures {
        /// Input PDF.
        input: PathBuf,
    },

    /// **Verify each signature's INTEGRITY and COVERAGE** — and say, in so
    /// many words, that trust is not checked (ISO 32000-1 §12.8, RFC 5652).
    ///
    /// For every signature field: the digest over the `/ByteRange` is
    /// recomputed and compared with what the signer signed, and the
    /// signature value is checked against the signer's own embedded
    /// certificate (RSA PKCS#1 v1.5 / RSASSA-PSS, ECDSA P-256 / P-384;
    /// SHA-1 / 256 / 384 / 512; `adbe.pkcs7.detached`,
    /// `ETSI.CAdES.detached`, `adbe.pkcs7.sha1`). Coverage says whether
    /// anything was appended after signing. Anything pdfcer cannot verify is
    /// reported BY NAME as unverifiable, never as valid or invalid.
    ///
    /// **`verified` is not `valid`.** No trust store, no chain, no
    /// revocation, no clock: the signer's name and dates are printed as the
    /// CLAIMS the certificate makes. Exit 0 only when every signature
    /// verified; 12 when any failed integrity; 13 when none failed but one
    /// or more could not be verified.
    VerifySignatures {
        /// Input PDF.
        input: PathBuf,
        /// Evaluate signer TRUST against the trust store an installed
        /// Acrobat/Reader has downloaded (AATL + EU Trusted Lists), read from
        /// `%APPDATA%\\Adobe\\Acrobat\\<track>\\Security\\addressbook.acrodata`
        /// (`Pass 10.3`). OFF by default. ⚠ AT YOUR OWN RISK: it reads Adobe's
        /// own downloaded file (a local read), and whether relying on it fits
        /// the Adobe Reader licence is your call. A trusted result checks the
        /// signature chain, RFC 5280 CA/key-usage constraints, and certificate
        /// validity dates at the signing time — but NOT revocation (CRL/OCSP),
        /// which needs the network pdfcer-core never uses.
        #[arg(long = "trust-from-acrobat")]
        trust_from_acrobat: bool,
    },

    /// **List a document's optional-content groups** — layers (§8.11).
    ///
    /// Reports each layer's name and whether a reader would DRAW it with
    /// no interaction, which is the fact a name cannot carry: a
    /// "Confidential" watermark layer that is off by default is a
    /// different document from one where it is on.
    ///
    /// Read-only. Toggling a layer is session state in a viewer with no
    /// file-format footprint unless explicitly saved, and pdfcer has no
    /// save path for it — so there is no toggle to offer here.
    ListLayers {
        /// Input PDF.
        input: PathBuf,
    },

    /// **List a document's fonts** — what they are, what they cost, and
    /// which of them could safely have their embedded program removed
    /// (§9.5–9.10).
    ///
    /// One stable line per DISTINCT font object — a font referenced from
    /// forty pages is one row naming forty pages, not forty rows —
    /// followed by a document summary. Read-only; nothing is modified.
    ///
    /// Reports `/BaseFont` (and the family name when it carries a §9.6.4
    /// subset tag), the `/Subtype` and a composite font's descendant
    /// subtype, `/Encoding`, whether a program is embedded and under which
    /// descriptor key, **the program's byte size in this file**, whether
    /// `/ToUnicode` is present, the OpenType `fsType` permission bits where
    /// they can be read, and a removability verdict.
    ///
    /// The verdict is the point. For a `Type0` font on `Identity-H` the
    /// character codes in the content stream are glyph indices into that
    /// exact embedded program (§9.9 directs conforming writers to do
    /// this), so deleting the program leaves text no substitute font can
    /// draw. Each such font is named, with its reason, rather than being
    /// quietly left off a list.
    ///
    /// The summary line also states which font-bearing surfaces were
    /// searched and which were not.
    ListFonts {
        /// Input PDF.
        input: PathBuf,
        /// After each font, print the sentence explaining its verdict.
        ///
        /// Off by default so the one-line-per-font listing stays easy to
        /// parse and to scan. The distinct reasons present in the document
        /// are written to stderr regardless, so nothing is hidden by
        /// leaving this off — this flag only puts them next to the row
        /// they belong to.
        #[arg(long)]
        reasons: bool,
        /// Sort by embedded program size, largest first.
        ///
        /// The default order is first discovery, which is stable and
        /// diff-friendly. This is the order an operator asking "what is
        /// costing me the most" wants, and it is a separate question.
        #[arg(long)]
        by_size: bool,
    },

    /// **Which fonts would `format-text --set-font` ACCEPT for one run?**
    /// A read-only pre-flight (Pass 142.1). Writes nothing.
    ///
    /// `list-fonts` answers *"what font dictionaries does this document
    /// contain"*, keyed on the dictionary. This answers a different question
    /// that a shell offering a font control actually needs: **which strings
    /// will `--set-font` accept, for THIS run, on THIS page** — keyed the way
    /// `--set-font` matches. The two keys disagree whenever a page carries
    /// two `/Font` resources with the same `/BaseFont` (two independent
    /// subsets of one face), which is most pages that embed anything.
    ///
    /// Three things it reports that could not be computed from `list-fonts`:
    ///
    /// - **The selector that reaches each resource.** Normally the
    ///   `/BaseFont` with its §9.6.4 subset tag stripped; the resource key
    ///   instead when the `/BaseFont` is ambiguous on the page, because
    ///   `--set-font`'s name match reaches only one of the twins. The
    ///   `ambiguous` marker says which case it is.
    /// - **Whether the face can show THIS RUN's characters.** Acceptance is
    ///   per run, never per page: an `/Encoding /Differences` array that
    ///   reassigns one code makes a face unusable for text containing that
    ///   character and perfectly usable for text that does not. The refusal
    ///   printed is the one `--set-font` itself would print, verbatim.
    /// - **Whether a real Bold or Italic of the family would be ACCEPTED** —
    ///   accepted, not merely named `Bold`. A `real_bold=-` does NOT mean a
    ///   style control should be disabled — but the reason is broader than
    ///   this line used to give. It said "it means synthesis is the route";
    ///   synthesis is ONE route, and `--set-font Helvetica-Bold` is another
    ///   that needs no embedding and that this survey does not look for.
    ///
    /// Read-only: it opens the file, answers, and writes nothing.
    FontPreflight {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number the run is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Text to find within a single run on the page.
        ///
        /// Acceptance is computed against exactly these characters, because
        /// that is what `--set-font` re-encodes. Asking about a different
        /// substring can give a different answer, and legitimately so.
        ///
        /// May be **empty** when `--pin-span` is given, which means *"the
        /// whole pinned show operator"* — the same meaning `format-text` gives
        /// it, and by the same code, so a preview and a commit cannot describe
        /// different characters. Empty WITHOUT a pin is refused.
        #[arg(long, default_value = "")]
        find: String,
        /// Pin the operator by its **byte span**, as `START:LEN`, from
        /// `extract-text --json --spans`.
        ///
        /// With an empty `--find` this asks the question a shell actually has:
        /// *"for the operator I already located, which faces would
        /// `--set-font` accept?"* — without having to describe it.
        #[arg(long = "pin-span", value_name = "START:LEN")]
        pin_span: Option<String>,
        /// The text you are ABOUT TO WRITE (`Pass 142.2`). Every face is
        /// then tested against THESE characters instead of the located text
        /// — the question a chooser actually has when a character is missing
        /// from the run's own font. The standard-14 block is tested for the
        /// same text either way.
        #[arg(long, value_name = "TEXT")]
        candidate: Option<String>,
        /// Emit machine-readable JSON instead of the aligned listing.
        #[arg(long)]
        json: bool,
    },

    /// **List a document's embedded files** (§7.11.4, §12.5.6.15).
    ///
    /// Reports BOTH kinds — document-level `/Names /EmbeddedFiles` and
    /// page-level `/FileAttachment` annotations — in one list, each
    /// labelled by kind, because they behave differently on save and on
    /// page deletion but an operator asking what is in a file should not
    /// need to know that to get a complete answer.
    ///
    /// Names are reported RAW. An attachment name is attacker-controlled
    /// and may contain path separators or a right-to-left override that
    /// makes `gnp.exe` render as `exe.png`; a sanitised alternative is
    /// printed alongside when the two differ.
    ///
    /// Read-only: it never writes a file out.
    ListAttachments {
        /// Input PDF.
        input: PathBuf,
    },

    /// **Extract an embedded file** out of a PDF (§7.11.4).
    ///
    /// The counterpart to `list-attachments`, which could name an
    /// attachment but never get it out — `pdfcer_core` has been able to do
    /// this since attachments were first read; there was simply no way to
    /// ask for it from a shell.
    ///
    /// THE OUTPUT PATH IS YOURS, NOT THE DOCUMENT'S. An attachment's name
    /// is attacker-controlled and unconstrained by ISO 32000-1: it may be
    /// `..\..\Windows\System32\evil.exe`, may contain a NUL, or may use a
    /// right-to-left override so `gnp.exe` renders as `exe.png`. This command
    /// therefore takes an explicit `--output` and NEVER derives a path from
    /// the name in the file.
    ExtractAttachment {
        /// Input PDF.
        input: PathBuf,
        /// Which attachment, by the name `list-attachments` reports.
        #[arg(long)]
        name: String,
        /// Where to write the extracted bytes. Required, deliberately — see
        /// the command's own help for why the name in the document is not
        /// used.
        #[arg(long, short)]
        output: PathBuf,
        /// Also DETACH the file from the document, writing the result here —
        /// CUT (`Pass 173.0`).
        ///
        /// The extraction runs first, so an attachment whose bytes cannot be
        /// decoded is refused with nothing detached. That ordering matters
        /// more here than anywhere else: the embedded file is the ONLY copy
        /// of that data in the document, so a cut that carried nothing would
        /// destroy it with nothing on any page to hint it was ever there.
        #[arg(long, value_name = "OUTPUT.pdf")]
        cut: Option<PathBuf>,
        /// Save mode for `--cut`.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Attach a file to a PDF** as a document-level embedded file
    /// (§7.11.4.1, `/Names /EmbeddedFiles`).
    ///
    /// A DRY RUN unless `--apply`, matching every other mutating command
    /// here.
    ///
    /// ⚠️ Attaching does not encrypt or protect the file. It travels with
    /// the PDF and anyone who can open the PDF can extract it.
    AttachFile {
        /// Input PDF.
        input: PathBuf,
        /// The file to embed.
        #[arg(long)]
        file: PathBuf,
        /// The name to file it under. Defaults to the source file's own
        /// name.
        #[arg(long)]
        name: Option<String>,
        /// Optional description, shown beside the name in a reader's
        /// attachments pane (`/Desc`, Table 44).
        #[arg(long)]
        desc: Option<String>,
        /// Actually write the output. Without it this is a DRY RUN.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Remove a document-level attachment** from a PDF (§7.11.4.1).
    ///
    /// Removes the name-tree entry, the file specification AND the embedded
    /// stream — not merely the entry, which would hide the attachment from
    /// every reader while leaving its bytes fully present.
    ///
    /// ⚠️ NOT a redaction. Under the default incremental save every prior
    /// revision remains in the file by design (§7.5.6) — that is what keeps
    /// existing signatures valid — so the bytes stay recoverable from the
    /// earlier revision. Use `--mode full` when the point of removing it was
    /// that it should not be in the file at all.
    DetachFile {
        /// Input PDF.
        input: PathBuf,
        /// Which attachment, by the name `list-attachments` reports.
        #[arg(long)]
        name: String,
        /// Actually write the output. Without it this is a DRY RUN.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Which save path to use. `full` is the one that does not leave the
        /// removed bytes in a prior revision.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Send pages to a printer.** Does a DRY RUN unless `--send` is
    /// given.
    ///
    /// Without `--send` it reports what printing this document WOULD do,
    /// without printing: it resolves the printer, reads its resolution and
    /// printable area, and places every selected page onto the sheet —
    /// reporting the scale, the offset, and whether content would fall off
    /// the edge.
    ///
    /// Acrobat clips an oversized page silently. This names the pages
    /// that would lose content, so a scripted caller can refuse before
    /// paper is consumed rather than discover it afterwards.
    ///
    /// Every step runs either way — the device is opened, its resolution
    /// and printable area are read, placement is computed and the pages
    /// are rasterised. `--send` is the only thing that starts a job.
    ///
    /// That default is inverted from most tools on purpose: printing is
    /// irreversible, consumes paper, and occupies a device other people
    /// may share.
    Print {
        /// Input PDF.
        input: PathBuf,
        /// Printer name, as `list-printers` reports it. Defaults to the
        /// system default printer.
        #[arg(long)]
        printer: Option<String>,
        /// How the page is sized onto the sheet.
        #[arg(long, value_enum, default_value_t = PrintScaleArg::Fit)]
        scale: PrintScaleArg,
        /// An explicit percentage, where 100 is actual size. OVERRIDES
        /// `--scale` when given.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=1000))]
        scale_percent: Option<u32>,
        /// 1-based pages: `all`, `3`, `1-4`, `5,1-2`.
        #[arg(long, default_value = "all")]
        pages: String,
        /// **Actually print.** Without this the command stops before
        /// starting the job and reports what it would have done.
        #[arg(long)]
        send: bool,
        /// Cap the rendering resolution, in DPI.
        ///
        /// A memory decision, not a quality one: an A4 page at 600 DPI is
        /// 4960x7016 px, about 139 MB at RGBA for a single page. The cap
        /// is disclosed on stderr whenever it binds, because it is pdfcer
        /// choosing a number the operator did not.
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u32).range(36..=2400))]
        max_dpi: u32,
        /// Write the job to a FILE instead of the printer's own port.
        ///
        /// What GDI's `lpszOutput` does. Most PDF writers sit on a
        /// `PORTPROMPT:` port and pop a Save dialog; this bypasses it,
        /// which makes them scriptable — and is a real capability rather
        /// than only a testing device.
        #[arg(long, value_name = "PATH")]
        to_file: Option<PathBuf>,
        /// How many copies.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=999))]
        copies: u16,
        /// Print each page's copies together (1,1,2,2) rather than whole
        /// documents in order (1,2,1,2).
        #[arg(long)]
        uncollated: bool,
        /// Print only odd or only even DOCUMENT page numbers, within
        /// whatever `--pages` selected.
        #[arg(long, value_enum, default_value_t = SubsetArg::All)]
        subset: SubsetArg,
        /// Print the sequence back to front.
        #[arg(long)]
        reverse: bool,
        /// Sheet orientation. `auto` decides from the page's own shape.
        #[arg(long, value_enum, default_value_t = OrientationArg::Auto)]
        orientation: OrientationArg,
        /// Two-sided printing, if the device supports it. Never
        /// simulated: a printer that cannot duplex will print
        /// single-sided and `list-printers` says which can.
        #[arg(long, value_enum, default_value_t = DuplexArg::Simplex)]
        duplex: DuplexArg,
        /// Ask the driver to choose the input tray from each page's
        /// size rather than using its default tray.
        #[arg(long)]
        pick_tray: bool,
        /// Which sheet to print on: a form ID or a form NAME, as
        /// `list-paper-sizes` reports them.
        ///
        /// A name is matched case-insensitively — exactly first, then as
        /// a unique prefix — and the ID pdfcer settled on is printed,
        /// because choosing a form from a name is pdfcer inferring
        /// something the operator did not type.
        #[arg(long, value_name = "ID-OR-NAME", conflicts_with = "paper_size")]
        paper: Option<String>,
        /// Print on a custom sheet, `WIDTHxHEIGHT` in PDF points.
        ///
        /// The driver's own unit is tenths of a millimetre, so the
        /// request is rounded; the sheet that will actually be fed is
        /// printed. Sizes past the DEVMODE ceiling of about 3.28 m are
        /// refused rather than clamped.
        #[arg(long, value_name = "WxH")]
        paper_size: Option<String>,
        /// Print with driver settings saved by `printer-properties`.
        ///
        /// Everything in the file applies except the members pdfcer sets
        /// itself — orientation, duplex, tray and paper still come from
        /// the flags above, so a saved configuration adds the settings
        /// pdfcer has no flag for rather than overriding the ones it has.
        #[arg(long, value_name = "PATH")]
        printer_config: Option<PathBuf>,
        /// Which annotation classes print.
        #[arg(long, value_enum, default_value_t = CommentsArg::Document)]
        comments: CommentsArg,
        /// Print several pages per sheet (2, 4, 6, 9, 16, …).
        ///
        /// The grid is chosen to place the first page as large as
        /// possible, rotation included — so 2-up on a portrait page
        /// turns the pages and stacks them, which is what fits.
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(2..=1024))]
        n_up: Option<u32>,
        /// Draw a border around each page's cell when using `--n-up`.
        #[arg(long)]
        n_up_border: bool,
        /// Impose as a folded booklet: two page-halves per sheet face,
        /// remapped so the fold reads in order.
        ///
        /// Pages are padded to a multiple of four with blanks, and the
        /// blanks SCATTER across sheets rather than grouping at the end
        /// — that is what a fold requires, and grouping them produces a
        /// booklet with a blank leaf in the middle.
        #[arg(long)]
        booklet: bool,
        /// Tile ONE oversized page across MANY sheets, to be taped
        /// together. The inverse of N-up.
        ///
        /// Mutually exclusive with `--n-up` and `--booklet`: all three
        /// change the shape of the job, and no two of them compose.
        #[arg(long)]
        poster: bool,
        /// Magnification applied before tiling, where 1.0 is 100%. This
        /// decides how big the assembled poster is, and therefore how many
        /// sheets it takes.
        #[arg(long, default_value_t = 1.0)]
        poster_scale: f64,
        /// Shared border in POINTS, duplicated onto adjacent tiles so the
        /// sheets can be aligned and taped without a gap at the seam.
        ///
        /// No default is invented: no source gives Acrobat's, so pdfcer
        /// leaves it at zero and lets the operator choose.
        #[arg(long, default_value_t = 0.0)]
        poster_overlap: f64,
        /// Tile only pages larger than the printable area; pages that
        /// already fit print normally in the same job.
        #[arg(long)]
        poster_large_only: bool,
        /// Refuse a poster needing more sheets than this.
        #[arg(long, default_value_t = pdfcer_print::imposition::DEFAULT_MAX_TILES)]
        poster_max_tiles: u32,
        /// Which edge the booklet is bound on.
        #[arg(long, value_enum, default_value_t = BindingArg::Left)]
        binding: BindingArg,
        /// Print one face of each sheet, for a printer without duplex.
        #[arg(long, value_enum, default_value_t = BookletSubsetArg::BothSides)]
        booklet_subset: BookletSubsetArg,
        /// Print every line at one width, in millimetres.
        ///
        /// Thin lines are thickened and thick lines thinned, so a drawing
        /// prints with one pen. Fills and text are unchanged. Without this
        /// flag every line prints at the width the file declares.
        #[arg(long, value_name = "MM", value_parser = parse_line_width_mm)]
        line_width: Option<f64>,
        /// Draw cut marks on each poster sheet, in a strip along the top and
        /// left edges.
        ///
        /// The strip costs printable area, so a poster can need more sheets.
        #[arg(long, requires = "poster")]
        poster_cut_marks: bool,
        /// Print a label on each poster sheet naming the file and the
        /// tile's row and column, in a strip along the top edge.
        #[arg(long, requires = "poster")]
        poster_labels: bool,
    },

    /// **Report what a print WOULD do**, without printing anything
    /// (`Pass 138.0`).
    ///
    /// Resolves the printer, reads its resolution and printable area, and
    /// places every selected page onto the sheet — exactly as a real print
    /// would — then reports the result instead of spooling it. There is
    /// deliberately no flag here that starts a job.
    ///
    /// # The clip report is the point
    ///
    /// Acrobat clips an oversized page SILENTLY. pdfcer names the pages that
    /// would lose content and reflects it in the exit code, so a scripted
    /// caller can refuse to print rather than discover the loss on paper.
    PrintPreview {
        /// Input PDF.
        input: PathBuf,
        /// Printer name, as `list-printers` reports it. Defaults to the
        /// system default printer.
        #[arg(long)]
        printer: Option<String>,
        /// How the page is sized onto the sheet.
        #[arg(long, value_enum, default_value_t = PrintScaleArg::Fit)]
        scale: PrintScaleArg,
        /// An explicit percentage, where 100 is actual size. OVERRIDES
        /// `--scale` when given.
        ///
        /// Reader accepts a free-form 1–1000% rather than a set of
        /// presets, so this is a number and not another enum value.
        /// Overriding rather than conflicting: `--scale` has a default,
        /// so making the two mutually exclusive would force every
        /// percentage caller to also pass a scale word they do not mean.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=1000))]
        scale_percent: Option<u32>,
        /// 1-based pages: `all`, `3`, `1-4`, `5,1-2`.
        #[arg(long, default_value = "all")]
        pages: String,
        /// Sheet orientation, exactly as `print` takes it. `auto`
        /// decides from the first page's own shape.
        ///
        /// It is here because orientation TURNS THE SHEET: the printable
        /// area a landscape job is placed on is the device's own,
        /// transposed. A preview that ignored it would report a scale the
        /// real print would not use, which is the one thing a preview
        /// must not do.
        #[arg(long, value_enum, default_value_t = OrientationArg::Auto)]
        orientation: OrientationArg,
        /// Which sheet to plan against: a form ID or NAME, exactly as
        /// `print` takes it.
        ///
        /// Here for the same reason `--orientation` is: paper CHANGES
        /// THE SHEET, so the printable area a page is placed on is a
        /// different rectangle. A preview that ignored it would report a
        /// scale the real print would not use.
        #[arg(long, value_name = "ID-OR-NAME", conflicts_with = "paper_size")]
        paper: Option<String>,
        /// Plan against a custom sheet, `WIDTHxHEIGHT` in PDF points.
        #[arg(long, value_name = "WxH")]
        paper_size: Option<String>,
    },

    /// **List the paper sizes a printer offers** (Windows only).
    ///
    /// The forms the DRIVER enumerates, with the ID `print --paper`
    /// takes and the sheet size in PDF points. Read-only; it starts no
    /// job and opens no document.
    ///
    /// The size is the PHYSICAL sheet, not the printable area — the
    /// hardware's unprintable margins are smaller than the sheet and are
    /// reported by `print-preview` instead, because they depend on the
    /// job's orientation and this list does not.
    ListPaperSizes {
        /// Printer name, as `list-printers` reports it. Defaults to the
        /// system default printer.
        #[arg(long)]
        printer: Option<String>,
    },

    /// **Open the printer driver's own properties dialog** (Windows only).
    ///
    /// The settings a generic API cannot model — media type, output bin,
    /// stapling, quality, the whole vendor-specific half — live behind
    /// the driver's own dialog, and this is the route to it. What the
    /// dialog returns is a driver `DEVMODE`, which `--save` writes to a
    /// file for `print --printer-config` to use.
    ///
    /// # It opens a window
    ///
    /// The one subcommand in `pdfcer` that does. That is deliberate
    /// rather than accidental: the dialog is the driver's, pdfcer cannot
    /// reproduce it, and a capability the GUI could reach and the CLI
    /// could not would be the same boundary error in the other
    /// direction. Pressing Cancel is not a failure — it reports
    /// `changed=0` and exits 0.
    PrinterProperties {
        /// Printer name, as `list-printers` reports it. Defaults to the
        /// system default printer.
        #[arg(long)]
        printer: Option<String>,
        /// Write the resulting configuration here, for
        /// `print --printer-config`.
        ///
        /// Without it the dialog still opens and the result is still
        /// summarised, but nothing is kept — which is a legitimate way
        /// to read what a device is currently set to.
        #[arg(long, value_name = "PATH")]
        save: Option<PathBuf>,
        /// Open the dialog on a configuration saved earlier rather than
        /// on the device's current settings.
        #[arg(long, value_name = "PATH")]
        from: Option<PathBuf>,
        /// **Do not open the dialog** — read the device's current
        /// settings and report or save them as they stand.
        ///
        /// The scriptable half. A command that always opens a modal
        /// window cannot run from a batch file, cannot run over a remote
        /// session with no desktop, and cannot be tested without a
        /// person to click it — so the capture and the editing are
        /// separable, and only the editing needs the operator.
        #[arg(long, conflicts_with = "from")]
        no_dialog: bool,
    },

    /// **List the printers this machine can reach** (Windows only).
    ///
    /// Read-only. It queries the print spooler and reports nothing else;
    /// it does not open a document and cannot start a print job.
    ///
    /// The first slice of pdfcer's printing support, which does not spool
    /// yet: printing consumes paper and occupies a shared device, so the
    /// half that can be built and checked without side effects is built
    /// first.
    ListPrinters,

    /// **Find text in a document's pages**, reporting where each hit is.
    ///
    /// Reports the page and the on-page bounding box of every occurrence,
    /// so a hit can be pointed at rather than merely counted. The
    /// geometry is the SAME scan `mark-redaction --search` uses, so what
    /// this finds and what that would cover cannot disagree.
    ///
    /// Searches **page content text only** — not form-field values,
    /// annotation contents, bookmarks or attachments. And matching is per
    /// text run, so a phrase the producer split across runs (at a kerning
    /// pair or a style change) is not found. Both limits are real and
    /// stated rather than left to be discovered.
    ///
    /// Changes nothing and gates on nothing: an encrypted or certified
    /// document is still searchable, because reading is not what a
    /// signature restricts.
    FindText {
        /// Input PDF.
        input: PathBuf,
        /// The text to find.
        #[arg(long)]
        needle: String,
        /// Match regardless of case.
        #[arg(long)]
        ignore_case: bool,
    },

    /// Extract a document's text content (ISO 32000-1 §9.10).
    ///
    /// Prints what the file actually says, plus the word spaces and line
    /// breaks pdfcer had to DERIVE from glyph geometry — because outside
    /// a Tagged PDF the standard guarantees neither (§14.8.2.5, and the
    /// negative results S1–S9). `--json` splits the two apart run by
    /// run, so a caller that wants only the sourced characters can have
    /// exactly those.
    ///
    /// Diagnostics are never optional and never silent: how many
    /// character codes came from each rung of the §9.10.2 ladder, how
    /// many fell through it to U+FFFD, which fonts carry no recoverable
    /// Unicode at all, and how many spaces and line breaks pdfcer
    /// invented.
    ExtractText {
        /// Input PDF.
        input: PathBuf,
        /// 1-based pages to extract: `all`, `3`, `1-4`, `5,1-2`.
        ///
        /// Order is honoured, so `--pages 3,1` extracts page 3 first.
        #[arg(long, default_value = "all")]
        pages: String,
        /// Write the text here instead of to stdout.
        ///
        /// When this is given, stdout carries the machine-readable
        /// result line instead of the text — so a script can capture the
        /// counters without parsing them out of the document's prose.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Emit a JSON document instead of plain text.
        ///
        /// The JSON exposes the sourced/derived split per run, the
        /// §9.10.2 ladder rung per glyph, and every diagnostic counter.
        #[arg(long)]
        json: bool,
        /// Include artifact content (running heads, folios, watermarks)
        /// in the extracted text.
        ///
        /// Off by default, which is a POLICY choice and not a
        /// conformance one: §14.8.2.2 states no `shall` requiring a
        /// reader to exclude artifacts — every reader-side verb there is
        /// `may`/`can`/`probably should`. Artifact runs appear in
        /// `--json` output either way, flagged.
        #[arg(long)]
        include_artifacts: bool,
        /// Also emit each glyph's **show-operator byte span** (`--json`
        /// only): `op_start`, `op_len`, and the `stream` those index.
        ///
        /// This is the pin `format-text --pin-span` takes, and without it
        /// there is no way to obtain one from outside the library — which is
        /// why a consuming project had to reconstruct a `find` string from a
        /// run's text instead, and got it wrong three times (`Pass 145.0`).
        ///
        /// Off by default because it turns on provenance capture, which
        /// costs memory per glyph and changes nothing else. An absent field
        /// means "not captured", which a zero could not.
        #[arg(long, requires = "json")]
        spans: bool,
    },

    /// **Download the OCR model weights**, verified against a pinned
    /// SHA-256 before anything is written.
    ///
    /// # When you need this, which is rarely
    ///
    /// The weights normally ship **inside the portable folder** at
    /// `models/ocrs`, so OCR works with no network at all. This exists for a
    /// build that does not carry them — a `cargo install`, a stripped
    /// package, or a machine where the folder was not copied.
    ///
    /// # What it will not do
    ///
    /// It refuses to write a file whose SHA-256 does not match the pinned
    /// one, and refuses **without leaving a partial file behind**. That is a
    /// supply-chain control, not a corruption check: a truncated download and
    /// a substituted file are indistinguishable to a caller, so both are
    /// refused identically. There is no mirror fallback and no retry against
    /// a different URL — a pinned artefact has one source, and silently
    /// reaching for a second is how you end up running the copy nobody
    /// measured.
    ///
    /// Nothing here runs on a timer, at startup, or in the background.
    FetchOcrModels {
        /// Where to write them. Defaults to `models/ocrs` beside this
        /// executable, which is where `ocr` looks first.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// **List the rendering presets** pdfcer holds for the PDF subset
    /// standards (PDF/X, PDF/A, PDF/UA), and what each one would set.
    ///
    /// Prints, per standard, every setting it states, the value, and **where
    /// that value comes from** — `sourced`, `implied`, `best-effort` or
    /// `not applicable`. That last column is the point: a preset labelled
    /// with an ISO number is a claim about a standard, and a claim without a
    /// provenance is an opinion wearing a committee's name.
    ListStandards {
        /// Show only this standard.
        #[arg(long)]
        standard: Option<String>,
    },
    /// **Recognise the text in a scanned page** and add it as an
    /// invisible, selectable layer (ISO 32000-1 §9.3.6, Table 106 mode 3).
    ///
    /// # What this does to the page, which is nothing
    ///
    /// The recognised words are drawn in text rendering mode **3** —
    /// *"neither fill nor stroke text (invisible)"* — on top of the scan,
    /// which is left **byte-identical**. The saved file therefore looks
    /// EXACTLY like the input at every zoom and on every printer, and
    /// `find-text`, `extract-text` and any viewer's copy/search now work on
    /// it. This is the "sandwich" OCRmyPDF popularised and the one Acrobat
    /// produces.
    ///
    /// **So there is nothing to LOOK at afterwards, and that is success,
    /// not failure.** An OCR layer you can see is a defect. To check it
    /// worked, search the output rather than looking at it:
    /// `pdfcer find-text out.pdf --needle <a word on the page>`.
    ///
    /// # It is a guess, and it says so
    ///
    /// Every word is an inference (project rule 4). The word count, the
    /// engine's confidence support, where the models came from and every
    /// word that had to be substituted or dropped are all reported on
    /// stderr. The `ocrs` engine reports **no per-word confidence at all**,
    /// and that is stated rather than presented as a clean bill of health.
    Ocr {
        /// The PDF to read. Never modified.
        input: PathBuf,
        /// 1-based page number to recognise.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Where to write the result. The input is left untouched.
        ///
        /// Mutually exclusive with `--in-place`, and exactly one of the two
        /// is required.
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// **Write the recognised layer back over the input file.**
        ///
        /// # Why this exists
        ///
        /// The operator's question, relayed from the GUI project: *"Why do I
        /// have to save a copy instead of just go back into my pdf and save
        /// over it?"* Six OCR tools were surveyed and **zero of six** force a
        /// Save-As on the open-document path. Requiring `--output` made
        /// recognition the one pdfcer operation that could not touch the file
        /// you pointed it at.
        ///
        /// # What is different about it, beyond the destination
        ///
        /// This routes through `EditSession::add_ocr_layer` — the layer is an
        /// **undoable edit** planned against the session, not a one-shot
        /// rewrite of an immutable document. On this one-shot CLI invocation
        /// there is nothing to undo it with, so the visible difference is
        /// only the destination; the reason to prefer it is that the *same*
        /// verb is what a GUI holding an open document uses, so the two
        /// shells cannot drift into producing different files from the same
        /// input.
        ///
        /// The write is **incremental** (`ARCHITECTURE.md` §5): every byte
        /// of the original stays where it was and a new revision is appended,
        /// so the scan itself is not re-encoded and the original content is
        /// still in the file. That is round-trip fidelity, and it is **not**
        /// redaction — if you needed the original page gone, this is not the
        /// verb.
        #[arg(long, conflicts_with = "output")]
        in_place: bool,
        /// Rasterisation resolution, dots per inch.
        ///
        /// DPI rather than `render-page`'s `--scale`, deliberately: OCR is
        /// the one place in this CLI where the operator's own unit is the
        /// right one, because scanner output is described in DPI and
        /// recognisers are tuned against it.
        ///
        /// # MORE RESOLUTION IS WORSE, WHICH IS THE OPPOSITE OF THE
        /// OBVIOUS EXPECTATION
        ///
        /// This defaulted to **300** on the reasoning quoted here until
        /// 2026-08-26 — *"300 is what document scanners produce and what
        /// `ocrs` was trained near; below about 150 recognition degrades
        /// sharply"*. **Every clause of that was an assumption, and the
        /// measured direction is backwards.**
        ///
        /// `pdfce-gui` scored a real CAD sheet (130 ground-truth tokens,
        /// against the page's own vector text) with the current detection
        /// model:
        ///
        /// ```text
        ///   72 dpi  56.5 %     200 dpi  53.9 %
        ///  100 dpi  56.7 %     300 dpi  35.1 %   <- the old default
        ///  150 dpi  54.5 %
        /// ```
        ///
        /// A **plateau from 72 to 200** — a spread under three points, inside
        /// the noise of that sample — and then a **cliff**. 300 was the worst
        /// of the five by twenty points.
        ///
        /// The mechanism is a property of the crate rather than of the
        /// weights, which is why it is trustworthy beyond one document:
        /// `ocrs` resizes its input to a **fixed model input size**, so past
        /// that size more pixels means the text is *smaller* relative to the
        /// model's window, not larger. Corroborated twice, by two different
        /// detection models — one of which was the broken one, which is a
        /// peculiar but real form of independent confirmation.
        ///
        /// 150 sits inside the plateau and is what a fitted-pixel-budget
        /// approach independently lands on for a Letter sheet. There is **no
        /// measured optimum** to pick: the plateau is flat within sampling
        /// noise, and reading a maximum out of it would repeat the mistake
        /// this correction is undoing.
        ///
        /// # What pdfcer's OWN corpus can and cannot say about this
        ///
        /// It **cannot** corroborate the cliff, and that is a limitation of
        /// the fixture rather than a disagreement. Swept over
        /// `fixtures/synthetic/ocr/scan.pdf` at 72/100/150/200/300/400 dpi,
        /// content recall is **100 % at every value up to 300** and 97.9 % at
        /// 400. It saturates, so it cannot rank anything.
        ///
        /// That is by design — the fixture's own docs say its degradation is
        /// "deliberately mild… not a stress test of the recogniser's
        /// tolerance". It proves the *pipeline*, not the *difficulty*. So the
        /// DPI evidence here is entirely `pdfce-gui`'s, and pdfcer's fixture
        /// is not being cited as agreement.
        ///
        /// Raise it for genuinely small text; lower it for speed. Both are
        /// cheap to try — the flag exists because no single value is right.
        #[arg(long, default_value_t = 150.0)]
        dpi: f32,
        /// Which recogniser reads the page: `ocrs` (the default), `ocrcer`
        /// or `tesseract`.
        ///
        /// `ocrcer` is the OCRcer engine (MIT, pure Rust), in every standard
        /// build; one compiled with `--no-default-features` and without the
        /// `ocrcer` feature refuses it by name. It reports a per-word confidence;
        /// `ocrs` reports none. Its model is one file, `ocrcer.ocrw`, which
        /// pdfcer does not ship or download: build or copy it from the
        /// OCRcer project (`model/out/ocrcer.ocrw`) into `models/ocrcer`
        /// beside this executable, or name its folder with `--model-dir`.
        ///
        /// `tesseract` runs the Tesseract program (Apache-2.0) in
        /// `models/tesseract`, which holds `tesseract.exe` and a `tessdata`
        /// folder of language files. It reads 100+ languages (see
        /// `--ocr-lang`) and reports a per-word confidence. A stock
        /// Tesseract install has the same layout, so `--model-dir` can name
        /// its folder directly.
        #[arg(long, value_enum, default_value_t = OcrEngineArg::Ocrs)]
        ocr_engine: OcrEngineArg,
        /// Directory holding the selected engine's model files.
        ///
        /// When omitted, `models/<engine>` beside this executable is used —
        /// `models/ocrs` (two `.rten` files, shipped in the portable
        /// package), `models/ocrcer` (`ocrcer.ocrw`, not shipped) or
        /// `models/tesseract` (`tesseract.exe` plus `tessdata`). A
        /// path given here that lacks the engine's files is REPORTED,
        /// never quietly replaced by the bundled copy — running a different
        /// model from the one you named is the sneaky half of rule 4.
        #[arg(long)]
        model_dir: Option<PathBuf>,
        /// Languages for `--ocr-engine tesseract`, joined by `+`: `eng`,
        /// `deu`, `eng+fra`, `jpn`.
        ///
        /// Each needs `<code>.traineddata` in the engine's `tessdata` folder;
        /// a missing one is refused by name. Ignored by the other engines,
        /// which have no language choice.
        #[arg(long, default_value = "eng")]
        ocr_lang: String,
        /// Print each recognised word and its page-space rectangle.
        ///
        /// The way to check POSITION rather than content: a layer can be
        /// perfectly recognised and land in the wrong place, and no word
        /// count can tell you so.
        #[arg(long)]
        words: bool,
        /// Write the exact greyscale image handed to the recogniser, as a PNG.
        ///
        /// # Why this is a shipped flag and not a debug print
        ///
        /// When OCR returns nonsense there are two suspects and they need
        /// completely different fixes: the recogniser cannot read the page,
        /// or the page it was given is not the page you think. Nothing in
        /// the output distinguishes them - garbage words look identical
        /// either way - and every other diagnostic here describes what came
        /// OUT.
        ///
        /// This is the only way to see what went IN. It is the buffer
        /// itself, after rasterisation and after the RGBA-to-luma
        /// conversion, not a re-render that might differ.
        #[arg(long)]
        dump_image: Option<PathBuf>,
        /// What to do when the page already carries an OCR layer pdfcer wrote.
        ///
        /// Layers are recognised by the marked-content tag pdfcer wraps them
        /// in. Invisible text written by other software carries no such tag
        /// and is never touched by any of these.
        #[arg(long, value_enum, default_value_t = ExistingOcrArg::Replace)]
        existing: ExistingOcrArg,
    },
    /// **List the stamps in an Acrobat-compatible stamp collection**
    /// (`Pass 288.0`).
    ///
    /// A stamp collection is an ordinary PDF: one file per category, one page
    /// per stamp, names in the catalog's `/Names` -> `/Pages` tree as
    /// `internal=display`. Reads Acrobat's own shipped collections and
    /// pdfcer's alike.
    StampList {
        /// The stamp collection PDF.
        input: PathBuf,
    },
    /// **Name a PDF's pages as stamps, making it a collection Acrobat can
    /// read** (`Pass 288.0`).
    ///
    /// Page 1 becomes the first `--stamp`, page 2 the second, and so on. A
    /// `--stamp` past the last page is skipped and reported rather than
    /// written, because a name pointing at no page is a stamp that appears in
    /// a picker and then draws nothing.
    StampPack {
        /// The PDF whose pages are the stamp artwork.
        input: PathBuf,
        /// The category name, written to `/Info` `/Title` — the heading a
        /// picker groups these stamps under.
        #[arg(long)]
        category: String,
        /// One per page, in page order, as `Internal=Display` or just
        /// `Display`. Repeat the flag.
        #[arg(long = "stamp", required_unless_present = "stamps_from")]
        stamps: Vec<String>,
        /// Read the names from a text file instead — one per line, in page
        /// order, same `Internal=Display` or `Display` form. Blank lines and
        /// lines beginning `#` are skipped.
        ///
        /// For a downloaded artwork sheet this is the only practical route:
        /// they run to a hundred pages or more, and repeating `--stamp` that
        /// many times is not a command anybody types twice.
        #[arg(long, conflicts_with = "stamps")]
        stamps_from: Option<PathBuf>,
        /// Where to write the collection.
        #[arg(long, short)]
        output: PathBuf,
    },
    /// **Rasterise one page to a PNG** (ISO 32000-1 §8, §9).
    ///
    /// Interprets the page's content stream and writes the result at the
    /// requested scale. `--page` is 1-based, matching how every reader and
    /// every human numbers pages.
    ///
    /// # Fidelity is reported, never assumed
    ///
    /// One machine-readable line on stdout always; a human-readable
    /// expansion on stderr ONLY when the render was less than fully
    /// faithful — substituted fonts, unsupported constructs, clamped
    /// geometry. A clean render writes nothing to stderr, so a non-empty
    /// stderr is a real signal and `2>/dev/null` is never needed.
    ///
    /// # No system fonts are discovered
    ///
    /// The default render is deterministic (`R19`): a batch job whose output
    /// depends on which fonts the runner happens to have installed is not one
    /// anyone can trust. `--font-dir` is the explicit, disclosed opt-in, and
    /// glyphs drawn from a supplied face are counted separately from
    /// substituted ones.
    RenderPage {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to render.
        ///
        /// A flag rather than a positional (the Pass 0 stub had it
        /// positional): rendering page 1 is overwhelmingly the common
        /// case, so it gets a default, and a defaulted positional in
        /// front of `-o` reads badly at a shell prompt.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Device pixels per PDF user-space unit. 1.0 ≈ 72 DPI; for a
        /// target resolution use `scale = dpi / 72` (150 DPI ≈ 2.0833).
        ///
        /// Scale, not DPI, is the knob `pdfcer-render` actually takes
        /// (`render_page(doc, page, scale)`), and passing the engine's
        /// own unit keeps the CLI from inventing a second one that has
        /// to be kept in sync.
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
        /// Render with the **preset for a PDF subset standard** —
        /// `pdf-x4`, `pdf-a2`, `pdf-ua`… (`list-standards` shows them all).
        ///
        /// # What this does, and the much larger thing it does not
        ///
        /// It applies a named bundle of render settings on top of your saved
        /// ones, for this invocation only. Nothing is written to your
        /// settings file.
        ///
        /// **It does not make the output conformant and does not check
        /// whether the input is.** A control carrying an ISO number invites
        /// exactly that reading, so the preset says otherwise on stderr every
        /// time it is used, along with which of its values are sourced to the
        /// standard and which are pdfcer's own judgement where the standard is
        /// silent — which, for most of these axes, is most of them.
        #[arg(long)]
        standard: Option<String>,
        /// Override `overprint_zero_tint_scope` for this render only:
        /// `device_cmyk_only` (the default), `grey_as_k_only` (the default
        /// up to v0.24.0) or `all_process_spaces`.
        ///
        /// # What this exposes, and it is a DIVERGENCE, not an ambiguity
        ///
        /// ISO 32000-1 §8.6.7 scopes `OPM 1`'s zero-tint rule to a
        /// `DeviceCMYK` source. A `DeviceGray` fill under `/OP true` either
        /// replaces a PROCESS backdrop (the literal reading, the default) or
        /// preserves its C, M and Y (`grey_as_k_only`: convert grey to K-only
        /// CMYK first, then apply the rule). A SPOT backdrop survives under
        /// every value: Table 149 puts any process space × spot colorant ×
        /// `OP true` at `c_b`, and pdfcer keeps spot inks on their own plane.
        ///
        /// The literal reading is the default because it measures better:
        /// 0 FAIL / 43 pass over the whole print-conformance sweep, against
        /// 2 FAIL under `grey_as_k_only`, the two being grey-over-process
        /// cells whose reference render the literal reading matches exactly.
        /// Choose `grey_as_k_only` to reproduce a pre-v0.25.0 render.
        ///
        /// CALLING THIS AN AMBIGUITY WOULD BE WRONG under **ISO 32000-1**:
        /// §8.6.7's next sentence excludes *"conversions from some other
        /// colour space"* by name, and Tables 148/149 tabulate *"any process
        /// colour space"* and give it `OPM 0` behaviour. ISO 32000-**2**
        /// deletes both of those supports, so the question really is open
        /// there — the edition matters. The default is ISO 32000-1 to the
        /// letter.
        ///
        /// Like `--standard`, this is applied OVER the saved settings and is
        /// **never written back**: one diagnostic render must not silently
        /// change how every later render behaves.
        #[arg(long)]
        overprint_zero_tint_scope: Option<String>,
        /// Override `spot_colorant_device_model` for this render only
        /// (`OP-A7`): `simulate_separations` (default) or
        /// `alternate_space_substitution`.
        ///
        /// **Which output device the page is rendered FOR**, and the
        /// standard genuinely offers both.
        ///
        /// ISO 32000-1 §8.6.6.4 *requires* a reader to substitute a
        /// `Separation`'s alternate colour space when the device has no
        /// colorant of that name — which a screen never does. ISO 32000-2
        /// §10.8.3 *permits* simulating a device that does have it. The two
        /// render a spot backdrop under overprint differently: the composite
        /// model knocks it out, the simulated one preserves it. Both are
        /// conformant.
        ///
        /// Use `alternate_space_substitution` to reproduce what a composite
        /// viewer shows, including Adobe Acrobat's default view — which is
        /// the setting to reach for when comparing pdfcer against another
        /// engine's screenshot.
        ///
        /// Applied OVER the saved settings and **never written back**, like
        /// `--overprint-zero-tint-scope` above.
        #[arg(long)]
        spot_colorant_device_model: Option<String>,
        /// Render only a **region** of the page, as
        /// `llx,lly,urx,ury` in PDF user-space points.
        ///
        /// # Why this exists, and why it is the flag that makes deep zoom
        /// possible
        ///
        /// Without it, magnifying a page means rasterising the WHOLE page
        /// at that scale, and the raster grows with the SQUARE of the
        /// zoom: a US-Letter page at 1600 % is 9,792 x 12,672 px, which is
        /// 124 M pixels and roughly half a gigabyte of RGBA before any
        /// compositing buffer is allocated on top of it. That is what
        /// makes a whole-page renderer feel like it has a zoom ceiling,
        /// and the ceiling is spatial rather than numerical.
        ///
        /// With it, the cost is the size of what you are LOOKING at, not
        /// the size of the page it sits on. Measured on a suite page at a
        /// 401x301 pt region: **95 ms at 1x and 87 ms at 32x** -- flat,
        /// because the pixel count never changes.
        ///
        /// # Where the real ceiling is, and WHICH ceiling
        ///
        /// There are **two**, they differ by four orders of magnitude,
        /// and this block used to name only the higher one. Standing rule
        /// `R213`: a magnitude claim is a claim about ONE quantity, and
        /// the quantity has to be in the sentence.
        ///
        /// **The VIEWPORT's ceiling — one trillion percent.** A region's
        /// device geometry is derived in `f64` with the region's origin
        /// subtracted BEFORE narrowing (`Pass 74.2`), so a requested
        /// 800x600 viewport comes back at 800x600 out to a scale of
        /// `1e10`. That is a claim about the RECTANGLE: its size, its
        /// shape, and the arithmetic that computes it.
        ///
        /// **The CONTENT's ceiling — about a hundred times lower**, and
        /// it is the one a viewer meets first when a page carries small
        /// geometry. Two `f32` limits sit under the drawing itself, both
        /// measured on `tools/gen-scale-demo`:
        ///
        /// - **Path coordinates.** A point near `x = 540` has an `f32`
        ///   spacing of `6.1e-5 pt`, i.e. **21.5 um**. Anything smaller
        ///   than that written as an absolute page coordinate is
        ///   quantised away, whatever the scale.
        /// - **The placement matrix.** A `cm` carrying a page coordinate
        ///   leaves the CTM's translation as the difference of two large
        ///   nearly-equal `f32` values, so content DRIFTS by roughly
        ///   `page_x * scale / 16,700,000` pixels — about 5 px at a scale
        ///   of `1.6e5`, ~400 px at `8e6`, and past the viewport entirely
        ///   above `5e6`. Equivalently, the content's device position is
        ///   quantised: at `8.1e6` it moves in ~500 px steps, so nudging
        ///   `--region` by less than that does nothing at all.
        ///
        /// ⇒ `--region` will hand you a correctly-sized viewport far past
        /// the point where what is inside it stops being correctly placed.
        /// The second half is addressed by the `f64` trick above, carried
        /// through content-stream `cm` concatenation.
        ///
        /// The older `examples/zoom_ceiling.rs` measurement — `f32`
        /// transform error against a bar 2,999.7373 pt from the origin,
        /// under one device pixel out past **20,000x** — is a THIRD
        /// quantity again, the rasteriser's own error at a fixed
        /// coordinate, and is kept for what it is rather than as "the"
        /// ceiling.
        ///
        /// # Coordinates
        ///
        /// PDF user space, so the same numbers a `/MediaBox` or a
        /// `/BBox` uses: origin bottom-left, y increasing upward. The
        /// region is intersected with the page box; a region entirely
        /// outside it is an error rather than a blank image, because a
        /// blank image is indistinguishable from a page that is genuinely
        /// blank there.
        ///
        /// # `allow_hyphen_values`, and it is not cosmetic
        ///
        /// A `/MediaBox` may legitimately have a negative origin, and a
        /// viewer scrolled past the left or bottom edge asks for a region
        /// with negative coordinates as a matter of course. Without this,
        /// `clap` reads `--region -760,-437,840,562` as a flag named
        /// `-760,...` and rejects the command with a usage message that
        /// says nothing about coordinates — which reads as "the region
        /// flag is broken" rather than "the minus sign was eaten".
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        region: Option<String>,
        /// Output PNG path.
        #[arg(short, long)]
        output: PathBuf,
        /// Do not paint annotation appearances (markup, stamps, form-field
        /// widgets — ISO 32000-1 §12.5). Annotations are painted by
        /// default, matching what a reader shows; this flag reproduces the
        /// pre-6.0 content-only raster (for A/B comparison, or a document
        /// whose annotations you want excluded). The annotation *counters*
        /// on the result line are reported either way, so a suppressed
        /// render still discloses how many annotations the page carries.
        #[arg(long)]
        no_annotations: bool,
        /// Skip geometry smaller than half a device pixel, trading a
        /// little fidelity for speed at low zoom.
        ///
        /// OFF by default, because it is LOSSY: a form that small is not
        /// invisible, it contributes anti-aliased coverage, and a page
        /// carrying hundreds of them looks measurably lighter without
        /// them. pdfcer does not trade fidelity for speed on its own
        /// account (decision 082) -- the exact `/BBox` cull is always on
        /// and needs no flag; this is the inexact one, so it is yours to
        /// ask for.
        ///
        /// Worth it when a page holds a great deal of detail far below
        /// the resolution you are viewing it at -- a CAD sheet or a map at
        /// page-fit zoom. Measured on the gen-scale-demo banana, whole
        /// page: 1 468 ms -> 108 ms, with ZERO pixels different, because
        /// at that zoom the dropped objects are 1/70th of a pixel each.
        ///
        /// The loss shows up in between, not at the bottom: on the same
        /// page in a 1 pt window it changes 18 of 400 pixels at 20x and 82
        /// of 3 600 at 60x, by up to a quarter of a channel. Largest
        /// exactly where the speed-up is smallest.
        ///
        /// `subpixel_culled` on the result line reports how many objects
        /// were dropped, and is printed whether or not the flag is set --
        /// so a raster always carries the count of what it left out.
        #[arg(long)]
        fast_subpixel: bool,
        /// Override the memory ceiling on the PRINT-COLOUR blending
        /// buffer for this render, e.g. `1gib`, `512mb`, `268435456`,
        /// `default`, or `0` to refuse it entirely.
        ///
        /// A page whose group declares a CMYK blending space is composited
        /// in a four-colorant buffer at 20 bytes per pixel, so the cost
        /// grows with the SQUARE of `--scale`. Above the ceiling pdfcer
        /// composites on screen instead and says so
        /// (`cmyk_buffer_refused=1`), which is why the same page can come
        /// out with slightly different colours at different scales.
        ///
        /// Without this flag the operator's `max_cmyk_buffer_bytes`
        /// setting applies, and without that, pdfcer's built-in ceiling.
        /// There is deliberately NO UPPER LIMIT -- a ceiling this machine
        /// cannot honour falls back and discloses it rather than crashing.
        #[arg(long, value_name = "SIZE")]
        max_cmyk_buffer_bytes: Option<String>,
        /// **Report the INK at one device pixel**, as `X,Y` — origin
        /// top-left, the same numbers an image editor shows. Prints one
        /// extra `ink-probe:` line; changes no pixel of the output.
        ///
        /// # What it answers that the PNG cannot
        ///
        /// The PNG is sRGB. It is the OUTPUT of pdfcer's colour pipeline,
        /// so every question about what happened inside that pipeline is
        /// unanswerable from it — and two very different ink states can
        /// flatten to the same sRGB triple.
        ///
        /// A page destined for ink is composited in a four-colorant
        /// buffer and converted to sRGB at the very end. This probe reads
        /// that buffer IMMEDIATELY BEFORE the conversion, which splits a
        /// colour error into the half that happened while compositing and
        /// the half that happened while converting. For a single opaque
        /// paint on an empty page a correct composite is the identity on
        /// its operand, so an operand that arrives unchanged and a colour
        /// that is still wrong convicts the conversion.
        ///
        /// # When there is no ink to report
        ///
        /// Most pages are not composited in ink — only those whose
        /// blending colour space is subtractive, and not those where the
        /// buffer exceeded `--max-cmyk-buffer-bytes`. Both cases report
        /// `source=screen-srgb` and NO colorant values, rather than
        /// manufacturing four numbers by running the sRGB result
        /// backwards. That reconstruction is a different quantity and
        /// would be indistinguishable from a measurement.
        ///
        /// # Out of range is a report, not a refusal
        ///
        /// The raster's size is not known until `--scale`, `--region` and
        /// the page's own box have been resolved, so a coordinate cannot
        /// be validated when it is parsed. One outside the raster prints
        /// `source=out-of-range` and the page still renders: a diagnostic
        /// must not destroy the output it was asked about.
        #[arg(long, value_name = "X,Y")]
        probe_ink: Option<String>,
        /// Directory of font files to supply for the document's
        /// NON-embedded fonts (decision 012). Repeatable. pdfcer walks each
        /// directory, registers every readable `.ttf`/`.otf`/`.ttc`/`.cff`/
        /// `.pfb` face under its advertised name(s) AND its filename stem,
        /// and draws a non-embedded font from a supplied face whose name
        /// matches the PDF's `/BaseFont` (e.g. `Calibri.ttf` covers a
        /// document that references `Calibri` or `ABCDEF+Calibri` without
        /// embedding it). Without this flag pdfcer uses its bundled Base-14
        /// substitutes — the deterministic default (R19).
        ///
        /// Supplied fonts improve glyph SHAPES only: positions still come
        /// from the PDF's own `/Widths` (decision 004 §3.6), so layout is
        /// identical with or without `--font-dir`. Renders that use a
        /// supplied face are machine-dependent by definition and are
        /// disclosed separately (`supplied=` on the result line); they are
        /// outside pdfcer's same-input-same-pixels guarantee (R63).
        /// Unreadable, oversized, or unparseable files are skipped and
        /// noted on stderr, never fatal.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Force an optional-content layer (OCG) VISIBLE, by its `/Name`
        /// (ISO 32000-1 §8.11). Repeatable.
        ///
        /// Overrides the document's own default configuration
        /// (`/OCProperties /D`) for this render only — nothing is
        /// written. Use `list-layers` to see the names and which state
        /// the document itself asks for.
        ///
        /// A name that matches no layer is a NOTE on stderr, not a
        /// failure: a batch that renders a hundred drawings must not
        /// abort because one of them lacks a "Grid" layer, and silently
        /// ignoring it would let a typo produce a hundred wrong rasters
        /// with no sign anything was wrong.
        ///
        /// If a name is given to BOTH flags, that is refused rather than
        /// resolved by flag order — the operator asked for two things
        /// and pdfcer cannot know which was meant.
        #[arg(long = "show-layer", value_name = "NAME")]
        show_layers: Vec<String>,
        /// Force an optional-content layer HIDDEN, by its `/Name`.
        /// Repeatable. See `--show-layer`.
        #[arg(long = "hide-layer", value_name = "NAME")]
        hide_layers: Vec<String>,
        /// Render the state a PRINTING or aggregating application would
        /// use: the `/D` default configuration alone, with `/AS` usage
        /// application dictionaries **not** applied (ISO 32000-1
        /// §8.11.4.5).
        ///
        /// Without this flag `render-page` behaves as a viewer and
        /// applies `View`-event usage at `--scale`, so a layer banded to
        /// a magnification range appears or disappears with it. With it,
        /// magnification is irrelevant and you get the state the
        /// document opens in.
        ///
        /// §8.11.4.5 NOTE 2 names this exact affordance: viewers "may
        /// also provide users with an option to view documents in this
        /// state … [permitting] an accurate preview of the content as it
        /// will appear when placed into an aggregating application or
        /// sent to a stand-alone printing system."
        #[arg(long)]
        print_state: bool,
    },

    /// Export page(s) as PNG, JPEG or SVG files — with REAL transparency
    /// for PNG and SVG (`Pass 248.0`, `Pass 248.1`).
    ///
    /// One file per page. `--dpi` sets the pixel density (150 by default,
    /// Acrobat's own export default) and is written INTO the file (PNG
    /// `pHYs`, JFIF density), so Word, PowerPoint and LibreOffice place the
    /// image at the page's physical size rather than at 96 DPI — four times
    /// too large for a 300 DPI export, which is the first thing anyone
    /// pasting a page into a slide would otherwise have to fix.
    ///
    /// `--transparent` keeps the page group's own alpha (ISO 32000-1
    /// §11.4.7: the page IS an isolated transparency group, and the white
    /// paper is a final composite this flag declines). A pixel nothing
    /// painted is see-through; a `/ca 0.5` fill is half so. **Acrobat cannot
    /// do this** — its Export-To-Image flattens onto an opaque background in
    /// every format with no transparency option, so this is a parity-plus.
    ///
    /// JPEG has no alpha channel, so `--transparent` with `--format jpeg` is
    /// REFUSED by name rather than silently flattened: a white-backed JPEG
    /// looks exactly like the export succeeding. `--background #rrggbb`
    /// chooses the colour a JPEG (or a non-transparent PNG) is flattened
    /// onto; the default is white.
    ///
    /// Everything `render-page` honours — `--font-dir`, `--show-layer`/
    /// `--hide-layer`, `--standard`, `--no-annotations`, the overprint and
    /// spot-colorant settings, the colorant-buffer ceiling — is honoured here
    /// through the SAME resolver, so the two verbs cannot disagree about how
    /// a page looks. The stable stdout line carries `render-page`'s complete
    /// counter set after its own prefix, for the same reason.
    ExportImage {
        /// Input PDF.
        input: PathBuf,
        /// 1-based pages to export: `all`, `3`, `1-4`, `5,1-2`. Default `1`.
        /// Selecting more than one page needs `--output-dir`.
        #[arg(long, default_value = "1")]
        pages: String,
        /// Output format: `png`, `jpeg` (`jpg`), `svg` (vector —
        /// transparent by default, with `--dpi` governing only what must be
        /// embedded as raster inside it) or `emf` (Windows metafile).
        #[arg(long, value_enum, default_value_t = ImageFormatArg::Png)]
        format: ImageFormatArg,
        /// Pixel density. The render scale is `dpi / 72`; the value is also
        /// written into the file so it opens at physical size elsewhere.
        #[arg(long, default_value_t = 150.0)]
        dpi: f32,
        /// Keep the page's transparency instead of compositing it onto
        /// white (PNG only — refused for JPEG, which has no alpha).
        #[arg(long)]
        transparent: bool,
        /// JPEG quality, 1–100 (default 90; at 90 and above the encoder
        /// stops subsampling chroma, which keeps coloured line art crisp).
        /// Ignored for PNG, which is lossless.
        #[arg(long, default_value_t = 90)]
        quality: u8,
        /// Opaque background colour (`#rrggbb`) that transparency is
        /// flattened onto. Default white. Contradicts `--transparent`, and
        /// clap refuses the pair.
        #[arg(long, value_name = "#RRGGBB", conflicts_with = "transparent")]
        background: Option<String>,
        /// Output file, single-page mode. The extension is yours; the
        /// format comes from `--format`.
        #[arg(short, long, conflicts_with = "output_dir")]
        output: Option<PathBuf>,
        /// Existing directory to write one file per page into, named
        /// `<stem>_p<n>.<png|jpg>` with `<n>` zero-padded to the widest page
        /// number in the run so the files sort in page order — the same
        /// naming `export-dxf --pages` uses.
        #[arg(long, conflicts_with = "output")]
        output_dir: Option<PathBuf>,
        /// Render with the preset for a PDF subset standard (`pdf-x4`,
        /// `pdf-a2`, `pdf-ua`…; `list-standards` lists them). Applied over
        /// your saved settings for this run only; see `render-page
        /// --standard` for what it does and does not claim.
        #[arg(long)]
        standard: Option<String>,
        /// Override `overprint_zero_tint_scope` for this run only. See
        /// `render-page --overprint-zero-tint-scope`.
        #[arg(long)]
        overprint_zero_tint_scope: Option<String>,
        /// Override `spot_colorant_device_model` for this run only. See
        /// `render-page --spot-colorant-device-model`.
        #[arg(long)]
        spot_colorant_device_model: Option<String>,
        /// Do not paint annotation appearances (markup, stamps, form-field
        /// widgets — ISO 32000-1 §12.5). Painted by default, matching what
        /// a reader shows. The annotation counters on the result line are
        /// reported either way.
        #[arg(long)]
        no_annotations: bool,
        /// Drop sub-pixel geometry (LOSSY; see `render-page --fast-subpixel`).
        /// `subpixel_culled=` on the result line says how much was dropped.
        #[arg(long)]
        fast_subpixel: bool,
        /// Override the `max_cmyk_buffer_bytes` setting for this run only
        /// (`64MiB`, `1.5G`…). See `render-page --max-cmyk-buffer-bytes`.
        #[arg(long, value_name = "SIZE")]
        max_cmyk_buffer_bytes: Option<String>,
        /// Directory of font files to supply for the document's NON-embedded
        /// fonts (decision 012). Repeatable. See `render-page --font-dir`.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Force an optional-content layer VISIBLE, by its `/Name`
        /// (ISO 32000-1 §8.11). Repeatable. See `render-page --show-layer`.
        #[arg(long = "show-layer", value_name = "NAME")]
        show_layers: Vec<String>,
        /// Force an optional-content layer HIDDEN, by its `/Name`.
        /// Repeatable. See `render-page --hide-layer`.
        #[arg(long = "hide-layer", value_name = "NAME")]
        hide_layers: Vec<String>,
        /// Export the state a PRINTING application would use: `/AS` usage
        /// dictionaries NOT applied (ISO 32000-1 §8.11.4.5). See
        /// `render-page --print-state`.
        #[arg(long)]
        print_state: bool,
        /// SVG only: how text is written. `outlines` (default) draws every
        /// glyph as a path, which looks the same in every program. `keep`
        /// writes each line it can as real, selectable text in a subset
        /// copy of the PDF's own font embedded in the SVG; lines it cannot
        /// keep stay outlines and are counted on an `svg-text:` line.
        /// Browsers show kept text exactly; Word and Inkscape show it in a
        /// substitute font.
        #[arg(long, value_enum, default_value_t = SvgTextArg::Outlines)]
        svg_text: SvgTextArg,
        /// EMF only: how text is written. `outlines` (default) draws every
        /// glyph as a path. `keep` writes each line it can as real text in
        /// the INSTALLED font of the same name, each character pinned to
        /// its PDF position; an EMF cannot carry the font, so a machine
        /// without it shows a substitute. Lines it cannot keep stay
        /// outlines and are counted on an `emf-text:` line.
        #[arg(long, value_enum, default_value_t = SvgTextArg::Outlines)]
        emf_text: SvgTextArg,
    },

    /// Copy a page to the OS clipboard so it pastes as EDITABLE VECTORS into
    /// Word, PowerPoint, Excel and Inkscape, and as an alpha raster
    /// everywhere else (`Pass 248.2`). Windows only in this build.
    ///
    /// One transaction places, in this order: `image/svg+xml` (the SVG
    /// `export-image --format svg` writes, which Microsoft 365 and Inkscape
    /// read as vectors), `PNG` (with real transparency, at `--dpi`),
    /// `CF_DIBV5` (for readers older than the PNG convention), and
    /// `application/pdf` (the page as a one-page PDF, which Inkscape can
    /// import). A reader takes the first format it knows, so a paste into
    /// Word is vectors, a paste into Paint is pixels, and neither needs a
    /// switch. See docs/clipboard-interop-survey.md for who reads what.
    ///
    /// Everything `export-image` discloses is printed here too: the counter
    /// line, the `svg:` line, and the notes naming what is raster or
    /// approximated inside the vector payload.
    CopyPage {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Resolution of the raster payloads and of anything embedded as
        /// raster inside the SVG.
        #[arg(long, default_value_t = 150.0)]
        dpi: f32,
        /// Do not place the SVG (vector) payload.
        #[arg(long)]
        no_svg: bool,
        /// Do not place the EMF (Windows metafile) payload -- the vector
        /// form LibreOffice 24.x and legacy Win32 applications read.
        #[arg(long)]
        no_emf: bool,
        /// Do not place the PNG / DIB (raster) payloads.
        #[arg(long)]
        no_raster: bool,
        /// Do not place the one-page PDF payload.
        #[arg(long)]
        no_pdf: bool,
        /// Opaque background colour (`#rrggbb`) for every payload; the
        /// default keeps the page's transparency.
        #[arg(long, value_name = "#RRGGBB")]
        background: Option<String>,
        /// Do not paint annotation appearances (ISO 32000-1 §12.5).
        #[arg(long)]
        no_annotations: bool,
        /// Directory of font files for NON-embedded fonts (decision 012).
        /// Repeatable. See `render-page --font-dir`.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Force a layer VISIBLE by `/Name`. Repeatable.
        #[arg(long = "show-layer", value_name = "NAME")]
        show_layers: Vec<String>,
        /// Force a layer HIDDEN by `/Name`. Repeatable.
        #[arg(long = "hide-layer", value_name = "NAME")]
        hide_layers: Vec<String>,
    },

    /// List a document's annotations per page (ISO 32000-1 §12.5).
    ///
    /// Read-only inventory: for each page, every `/Annots` entry with its
    /// subtype, rectangle, flags, and whether pdfcer would paint its
    /// appearance, refuse it by name (no `/AP`, unresolved `/AS`,
    /// degenerate placement), or suppress it (Hidden/NoView/Popup). This
    /// is the windowless companion to `render-page`'s annotation counters
    /// — it says *which* annotations are which, where the counters say
    /// *how many*. Emits the locale-invariant stable-line format; nothing
    /// is modified.
    ListAnnotations {
        /// Input PDF.
        input: PathBuf,
        /// 1-based pages to inventory: `all`, `3`, `1-4`, `5,1-2`.
        #[arg(long, default_value = "all")]
        pages: String,
    },

    /// **Print the order a reader tabs through a page's annotations**
    /// (ISO 32000-1 §12.5.1; ISO 32000-2 §12.5.1 for `/A` and `/W`).
    ///
    /// `list-annotations` says what is on the page and where. This says
    /// **what order a reader visits it in** — which is a different
    /// question with a different answer, because a page can declare that
    /// its tab order comes from geometry or from its structure tree, in
    /// which case the order `/Annots` lists is not the order anything is
    /// visited in.
    ///
    /// One `page-order` line per page, then one `tab` line per annotation
    /// in visit order, then a `skipped` line for each annotation a reader
    /// does not tab to, then a `note` line for everything pdfcer had to
    /// infer. Read-only; nothing is modified.
    ///
    /// # The `basis` column is the one to read
    ///
    /// It says where the order came from, and the six answers are not
    /// interchangeable:
    ///
    /// - `stated-array` — `/Tabs /A`: the file states this order outright.
    ///   Nothing was inferred, and there are no `note` lines.
    /// - `stated-widget` — `/Tabs /W`: form fields in array order first.
    ///   What follows them is **contested inside ISO 32000-2 itself**; the
    ///   `note` line names which reading was applied and the
    ///   `widget_tab_tail` setting chooses it.
    /// - `row` / `column` — `/Tabs /R` or `/C`: **computed from where the
    ///   annotations sit on the page**, because the file says to derive it
    ///   and does not list it. The standard defines no grouping rule, so
    ///   the `note` line states pdfcer's.
    /// - `array-convention` — `/Tabs` absent, or a value outside the
    ///   standard's closed set. The file states no order at all; this is
    ///   the array order used as a convention, and the `note` line says so.
    /// - `not-derived` — `/Tabs /S`: the order lives in the document's
    ///   structure tree, which pdfcer does not read. **No `tab` lines are
    ///   printed for that page**, deliberately: a fall back to array order
    ///   would be indistinguishable from an answer.
    ///
    /// `derived=1` marks the three bases pdfcer computed rather than read.
    /// It cannot tell `stated-array` from `not-derived` — both are `0` —
    /// which is what `basis` is for.
    ///
    /// # What is skipped, and why it is printed rather than dropped
    ///
    /// `why=hidden` and `why=no-view` are read off §12.5.3, which says such
    /// an annotation shall not *"interact with the user"*; `why=trap-net`
    /// and `why=popup` are pdfcer's reading, argued in the core docs. A
    /// `NoView` annotation that also sets `ToggleNoView` stays in the
    /// sequence — under ISO 32000-2 that bit makes it appear *when
    /// selected*, and tabbing to it is what selects it.
    ///
    /// # Exit codes
    ///
    /// `0` success; `3`/`4` unreadable / not-a-PDF; `1` for a structural
    /// failure or an out-of-range `--pages` selection.
    TabOrder {
        /// Input PDF.
        input: PathBuf,
        /// 1-based pages to report: `all`, `3`, `1-4`, `5,1-2`.
        #[arg(long, default_value = "all")]
        pages: String,
        /// Row/column grouping tolerance in points, overriding the
        /// `tab_row_tolerance` setting for this run. The standard defines
        /// none; see the setting's own comment in `settings.txt`.
        #[arg(long)]
        row_tolerance: Option<f64>,
    },

    /// **List every clickable link and where it goes** (ISO 32000-1
    /// §12.5.6.5, Table 173).
    ///
    /// `list-annotations` says a `/Link` is *there* and prints
    /// `action=GoTo`; it deliberately stops at the action's type. This
    /// resolves the destination behind it — through a `/GoTo` action's
    /// `/D`, a direct `/Dest`, either §12.3.2.3 named-destination
    /// namespace, and any `<< /D … >>` wrapper — so a script can dump a
    /// document's table of contents, or find the links a page delete left
    /// pointing at nothing.
    ///
    /// One line per link. `dest=` is the resolution, and only
    /// `dest=page` is a jump this document can perform:
    ///
    /// - `dest=page target=<1-based> view=<Fit|FitH|FitR|XYZ|…>` — resolved.
    /// - `dest=unmapped target=<obj> …` — named an object that is not a
    ///   page in this document's tree; the usual residue of a page delete.
    /// - `dest=named name="…"` — a name neither namespace defines.
    /// - `dest=remote file="…"` — `/GoToR`, a page of ANOTHER file.
    ///   Never resolved against this document's names, by design.
    /// - `dest=action action=<URI|Launch|JavaScript|…>` — not a
    ///   navigation at all. **Recognised and disclosed, never executed.**
    ///
    /// A trailing `links-without-destination=<n>` summary line counts
    /// `/Link` annotations carrying neither `/Dest` nor `/A` — links the
    /// operator can see and can never follow. It is printed even when
    /// zero, so that "no broken links" and "this tool did not check" stay
    /// distinguishable.
    ///
    /// `/Widget` pushbuttons that carry a `/GoTo` are **not** listed: a
    /// widget is a form control first, and activating one has form-side
    /// consequences a link has none of. Read-only; nothing is modified.
    ListLinks {
        /// Input PDF.
        input: PathBuf,
        /// 1-based pages to inventory: `all`, `3`, `1-4`, `5,1-2`.
        #[arg(long, default_value = "all")]
        pages: String,
    },

    /// **Print one object's internal structure** (ISO 32000-1 §7.3).
    ///
    /// Shows the object exactly as pdfcer parsed it, whether it sits at a byte
    /// offset in the file or compressed inside an object stream (§7.5.7) —
    /// which is the case `grep` cannot reach and which is why this exists.
    ///
    /// Indirect references are expanded to `--depth` levels. Cycles are
    /// detected and marked rather than followed: a page's `/Parent` points back
    /// at its `/Pages` node, so a realistic dump revisits objects immediately
    /// and that is normal, not malformed.
    ///
    /// Read-only. Writes nothing, and the output is a REPORT, not PDF syntax —
    /// do not feed it back to a parser.
    DumpObject {
        /// Input PDF.
        input: PathBuf,
        /// Object number to print.
        #[arg(long)]
        id: u32,
        /// Generation number (§7.3.10). Almost always 0.
        #[arg(long, default_value_t = 0)]
        generation: u16,
        /// How many levels of indirect reference to expand.
        ///
        /// `0` prints references as `N G R` without following them. This is
        /// REFERENCE depth, not container nesting: a deeply nested direct
        /// dictionary prints in full at 0, because it is all one object.
        #[arg(long, default_value_t = 1)]
        depth: usize,
        /// Whether to include stream data, and in what form.
        ///
        /// `decoded` runs every filter in `/Filter`; `raw` shows the bytes as
        /// stored, which is what you want when the FILTER is under suspicion.
        /// A stream that will not decode reports its filter error in place and
        /// the dump continues.
        #[arg(long, value_enum, default_value_t = StreamDump::Omit)]
        streams: StreamDump,
        /// Ceiling on bytes shown per stream. Truncation is always disclosed.
        ///
        /// Applied AFTER decoding, because the decoded size is the one that can
        /// explode: a small stream can inflate enormously, and a ceiling on the
        /// encoded size would not catch it.
        #[arg(long, default_value_t = 4096)]
        max_stream_bytes: usize,
    },

    /// **Walk and print a document's object graph** from a chosen root.
    ///
    /// Breadth of the walk is bounded by `--max-objects`, and hitting that
    /// ceiling is REPORTED rather than left to look like completeness.
    ///
    /// Read-only.
    DumpStructure {
        /// Input PDF.
        input: PathBuf,
        /// Where to start: `catalog`, `page:<n>` (1-based), or `<num>[,<gen>]`.
        #[arg(long, default_value = "catalog")]
        root: String,
        /// Ceiling on how many distinct objects the walk will print.
        #[arg(long, default_value_t = 256)]
        max_objects: usize,
        /// Whether to include stream data, and in what form.
        #[arg(long, value_enum, default_value_t = StreamDump::Omit)]
        streams: StreamDump,
        /// Ceiling on bytes shown per stream. Truncation is disclosed.
        #[arg(long, default_value_t = 1024)]
        max_stream_bytes: usize,
    },

    /// **Export a PDF's internals to an editable form** (`Pass 194.0`).
    ///
    /// Writes a **valid PDF** with object streams expanded, stream data decoded
    /// and `/Filter` dropped, and a classic cross-reference table — the form
    /// qpdf calls QDF. Open it in a text editor, change what you like, then
    /// compile it back with `import-structure`.
    ///
    /// It is deliberately NOT byte-identical to the input: it is a full
    /// rewrite. Only the compile-back is minimal-diff.
    ///
    /// Refuses an encrypted document rather than writing its decrypted
    /// contents to disk.
    ExportStructure {
        /// Input PDF.
        input: PathBuf,
        /// Where to write the editable PDF.
        #[arg(long, short)]
        output: PathBuf,
    },

    /// **Compile an edited export back**, appending only what changed
    /// (`Pass 194.0`).
    ///
    /// Diffs the edited export against the ORIGINAL and, by default, writes a
    /// §7.5.6 **incremental update**: the original bytes are left as an
    /// untouched prefix and only the objects you actually changed are appended.
    ///
    /// That is the half qpdf does not have — its own issue tracker lists
    /// incremental updates and digital-signature support as unimplemented, so
    /// every qpdf round trip rewrites the file and invalidates every signature
    /// in it. Here, a signature over a byte range you did not edit stays valid;
    /// only editing an object a signature covers breaks it, which no
    /// implementation can avoid.
    ///
    /// Streams are compared SEMANTICALLY — same decoded payload, ignoring
    /// `/Filter`, `/DecodeParms` and `/Length` — because the export decoded
    /// them and the original is compressed. Without that, every stream in the
    /// document would look edited.
    ImportStructure {
        /// The ORIGINAL PDF the export came from.
        input: PathBuf,
        /// The edited export.
        #[arg(long)]
        edited: PathBuf,
        /// Where to write the result.
        #[arg(long, short)]
        output: PathBuf,
        /// Write a full rewrite instead of an incremental update.
        ///
        /// Destroys every existing signature (§12.8.1) and renumbers nothing,
        /// but produces a single-revision file. The default is incremental
        /// precisely because it does not.
        #[arg(long)]
        full: bool,
        /// Report what would change and write nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// **Inventory every object, and report the file's physical layout.**
    ///
    /// Two things a page-level view cannot show. Per object: where it actually
    /// lives (a byte offset, or a slot inside a named object stream), its
    /// `/Type` and `/Subtype`, and **what references it**. Per file: the
    /// cross-reference style (table, stream, or hybrid), which objects are
    /// compressed inside which container, linearization, encryption, and
    /// whether the cross-reference table had to be rebuilt by scanning.
    ///
    /// The reverse-reference column is the one Acrobat has no equivalent for,
    /// and it answers the questions operators actually ask — *what still points
    /// at this?* An object nothing references is listed under `unreferenced`;
    /// that is **not** by itself a defect, because an incremental update leaves
    /// superseded objects behind by design (§7.5.6).
    ///
    /// Read-only.
    ListObjects {
        /// Input PDF.
        input: PathBuf,
        /// Only list objects whose `/Type` matches this name (e.g. `Page`,
        /// `Font`, `ExtGState`). Case-sensitive, as PDF names are.
        #[arg(long)]
        filter_type: Option<String>,
        /// Print only the file-layout summary, not the per-object rows.
        #[arg(long)]
        layout_only: bool,
        /// Also list objects nothing references.
        #[arg(long)]
        show_unreferenced: bool,
    },

    /// List a PDF's interactive-form (AcroForm) fields (Pass 7).
    ///
    /// Prints one stable, locale-invariant line per terminal field —
    /// fully-qualified name, type, flags, value, widget count, and whether
    /// a baked `/AP` is present — followed by a document-level summary line
    /// with the form disclosures (`/NeedAppearances`, `/SigFlags`, `/CO`
    /// calculation-order length, XFA presence, fields carrying `/AA`
    /// JavaScript). Read-only; authors nothing.
    ListFields {
        /// Input PDF.
        input: PathBuf,
        /// Only list fillable fields (skip read-only, pushbuttons,
        /// signatures).
        #[arg(long)]
        fillable_only: bool,
        /// For each RICH-TEXT field, also print its formatting run by run.
        ///
        /// A rich-text field's row shows `rich=<n>runs`, which says the
        /// field HAS formatting without saying what it is. This prints the
        /// text of each run and the style resolved for it from `/RV` and
        /// `/DS` together (§12.7.3.4) — which is the question an operator
        /// asks before deciding whether a downgrade is acceptable.
        ///
        /// Off by default: it is several lines per field, and on a form of
        /// any size that would bury the one-line-per-field listing the rest
        /// of this command exists to give.
        #[arg(long)]
        rich_text: bool,
        /// Print one line per WIDGET under each field: its rectangle, border,
        /// visibility, raw annotation flags and appearance state.
        ///
        /// Separate from the field row because these are properties of the
        /// **box**, not of the field, and a field may carry several widgets
        /// with different ones. A single field-level `border=` column would be
        /// a lie the moment a field has two widgets — which is the normal case
        /// for a radio group and common for a field repeated across pages.
        ///
        /// `border=-` means **the file states no border**, not "solid 1 pt".
        /// The distinction is the whole point: a control seeded from a default
        /// would show a border the document does not contain, and the
        /// operator's first press would write that invention in.
        ///
        /// `visibility=other` means the widget's `/F` flags are legal but are
        /// not one of the four combinations pdfcer can set. `flags=` carries the
        /// raw word either way, so nothing is hidden by the mapping.
        #[arg(long)]
        widgets: bool,
    },

    /// **Create a new text form field** (§12.7.2 + §12.5.6.19).
    ///
    /// Writes a merged field/widget dictionary, registers it in the
    /// document's `/AcroForm` `/Fields` (creating the `/AcroForm` if the
    /// document has none), adds it to the page's `/Annots`, and bakes an
    /// appearance — all additively, leaving every existing byte in place.
    ///
    /// Defaults match Acrobat's documented creation floor: Helvetica at size
    /// 0 (auto-size), black, thin solid border. The field is immediately
    /// fillable with `fill-field`.
    ///
    /// Refused by name on a document carrying an XFA layer (pdfcer can write
    /// the AcroForm half but not the XFA half, and one-sided is worse than
    /// neither), when the name is already used by a field of a DIFFERENT
    /// type, and when the name belongs to a group that contains other fields.
    /// The same name and the SAME type is not a refusal — it merges.
    AddTextField {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name — also how `fill-field` and
        /// `list-fields` refer to it.
        ///
        /// A PERIOD SEPARATES LEVELS (§12.7.3.2): `Personal.Address.Zip`
        /// creates the group `Personal`, the group `Personal.Address`, and
        /// the field `Zip` inside it — reusing any of those that already
        /// exist. A name segment may not itself contain a period, so a
        /// leading, trailing or doubled one is refused rather than guessed at.
        ///
        /// REUSING AN EXISTING NAME OF THE SAME TYPE MERGES: a second widget
        /// is attached to the same field rather than a second field created.
        /// One value, two places to see and edit it — which is how a check box
        /// appears on every page of a form. A different type under the same
        /// name is refused, and so is a name that belongs to a group.
        #[arg(long)]
        name: String,
        /// 1-based page number to place the field on.
        #[arg(long)]
        page: usize,
        /// The field rectangle in PDF user space, `llx,lly,urx,ury`.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// Initial value. Omitted leaves the field empty.
        #[arg(long)]
        value: Option<String>,
        /// `/MaxLen` — maximum character count.
        #[arg(long)]
        max_len: Option<i64>,
        /// `/TU`, the accessibility name a screen reader announces.
        #[arg(long)]
        tooltip: Option<String>,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--no-tooltip` is required. Omitting
        /// both is an error, never a silent default: for a form field, `/TU`
        /// — not the tag tree — is what a screen reader announces, so a
        /// missing one is invisible to the person creating the field and
        /// load-bearing for the person who cannot see the form. Declining is
        /// a legitimate answer; it just has to be an ANSWER, and it is
        /// reported back in the operation's disclosures.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// Accept multiple lines (`/Ff` bit 13).
        #[arg(long)]
        multiline: bool,
        /// Mark the field read-only (`/Ff` bit 1).
        #[arg(long)]
        read_only: bool,
        /// Mark the field required at submit time (`/Ff` bit 2).
        #[arg(long)]
        required: bool,
        /// Echo the value as bullets (`/Ff` bit 14).
        ///
        /// pdfcer writes the flag; the obscuring is a viewer behaviour.
        #[arg(long)]
        password: bool,
        /// Lay the value out in equally-spaced cells (`/Ff` bit 25).
        ///
        /// REFUSED unless `--max-len` is given and neither `--multiline` nor
        /// `--password` is set. Table 228 bit 25 permits comb "only if" those
        /// hold, and a file that breaks the rule has no defined rendering —
        /// two viewers may legitimately draw it differently.
        #[arg(long)]
        comb: bool,
        /// Border line style (§12.5.4 Table 166).
        #[arg(long, value_enum, default_value_t = BorderArg::Solid)]
        border: BorderArg,
        /// Border width in points. Zero means no border.
        #[arg(long, default_value_t = 1.0)]
        border_width: f64,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, PAINTED
        /// into the appearance at creation, not merely recorded.
        ///
        /// Accepts `none` (Table 189's empty array, which STATES no colour
        /// and is not the same as the key being absent), one number for
        /// DeviceGray, three for DeviceRGB or four for DeviceCMYK, comma
        /// separated, each 0-1. CMYK is written as CMYK, never converted.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR, painted into the
        /// appearance at creation. Same spelling as `--background`; `none`
        /// leaves a text or choice field with no frame.
        ///
        /// Not `--border` or `--border-width`, which are `/BS` (Table 166) --
        /// the border's style and width. Different dictionaries.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,
        /// Where the widget is visible (§12.5.3 Table 165).
        #[arg(long, value_enum, default_value_t = VisibilityArg::Visible)]
        visibility: VisibilityArg,
        /// Pre-fill this field's properties from an existing field.
        ///
        /// Copies only NON-BOOLEAN, TYPE-MATCHED data — `--max-len` for a
        /// text field, the option list for a choice field, the on-state for
        /// a check box. A radio template copies nothing.
        ///
        /// Yes/no properties are never copied, and that is deliberate: these
        /// are presence flags, so a copied `--multiline` could be added but
        /// never turned off, and a single-line field could not be made from
        /// a multiline template. The accessibility name is never copied
        /// either — deciding it is the whole point of requiring
        /// `--tooltip`/`--no-tooltip`, and inheriting someone else's answer
        /// is not deciding.
        ///
        /// Anything given explicitly wins; this only fills gaps. When the
        /// template contributes nothing, it says so rather than silently
        /// doing nothing.
        #[arg(long, value_name = "FIELD")]
        defaults_from: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the add reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Copy a form field onto a portable clip file (`Pass 167.0`).
    ///
    /// The clip carries everything the field IS — type, flags, value, default
    /// value, appearance string, quadding, options, length limit, actions,
    /// accessibility name, every widget's rectangle, `/MK` colours, border
    /// style, appearance streams, and the `/AcroForm` `/DR` font its `/DA`
    /// names — minus its identity (`/T`, `/Parent`, `/Kids`), which
    /// `paste-field` supplies.
    ///
    /// This is the batch half of the gesture Acrobat has no command-line form
    /// of at all: copy one field from a template drawing, then stamp it onto
    /// two hundred others in a loop.
    ///
    /// REFUSED for a field with no widget annotation (a value-only field has
    /// nothing to place), and for a SIGNED signature field — a signature
    /// covers a byte range of the document it was made in (§12.7.4.5), so
    /// only its "signed by" artwork could travel, into a file nobody signed.
    /// An UNSIGNED signature field copies normally.
    CopyField {
        /// Input PDF.
        input: PathBuf,
        /// The fully-qualified name of the field to copy, as `list-fields`
        /// prints it.
        #[arg(long)]
        name: String,
        /// Where to write the clip.
        ///
        /// A private, versioned binary format — not a PDF. `paste-field`
        /// reads it, and `inspect-field-clip` says what is in it.
        #[arg(short, long)]
        output: PathBuf,
        /// Also remove the field from the document, writing the result here —
        /// CUT (`Pass 168.0`).
        ///
        /// Deletes every widget, the field dictionary, its `/AcroForm`
        /// registration and any grouping node it leaves empty, as ONE undo
        /// entry with the copy. The clip is written first, so a field that
        /// cannot be carried is refused with nothing deleted — which matters
        /// most for a SIGNED signature field, where a cut that carried
        /// nothing would have deleted a signature and left you holding an
        /// empty clipboard.
        #[arg(long, value_name = "OUTPUT.pdf")]
        cut: Option<PathBuf>,
        /// Save mode for `--cut`.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Say what a clip file carries, without pasting it (`Pass 167.0`).
    ///
    /// The question a script wants answered BEFORE it stamps two hundred
    /// files: does this clip bring a calculation with it, does it bring a
    /// value, does it bring a font, and how many widgets will land.
    InspectFieldClip {
        /// The clip file written by `copy-field`.
        clip: PathBuf,
    },

    /// Paste a copied form field onto a page (`Pass 167.0`).
    ///
    /// TWO PASTES, and they are different on purpose — the operator's own
    /// ruling, on two different keys in the GUI and two different flags here:
    ///
    /// * `--as-new <NAME>` — a NEW, INDEPENDENT field. Its own name, its own
    ///   value. Refused when the name is already taken; never auto-suffixed,
    ///   because an engine-invented `Name_2` is a name nobody chose.
    ///
    /// * `--as-widget-of <NAME>` — ANOTHER WIDGET of a field that already
    ///   exists here. One field, two places to see and edit it, one shared
    ///   value (§12.7.3.2). Refused when that field is not in this document;
    ///   it never falls back to `--as-new`, because the difference is
    ///   invisible on the page and shows up only when somebody types in one
    ///   and the other does not follow.
    ///
    /// `--as-widget-of` is the HIGHER-FIDELITY route, which is the
    /// counter-intuitive part: it does not touch the field object at all, so
    /// the font, colour, alignment, default value and actions are the
    /// original's exactly.
    ///
    /// Everything the paste dropped, renamed, translated or reused is printed
    /// to stderr. Read it: an inert calculation and a reused accessibility
    /// name are both invisible in the file.
    PasteField {
        /// Input PDF — the document the field is pasted INTO.
        input: PathBuf,
        /// The clip file written by `copy-field`.
        #[arg(long)]
        clip: PathBuf,
        /// 1-based page number to place the field on.
        #[arg(long)]
        page: usize,
        /// Where to place it, in PDF user space, `llx,lly,urx,ury`.
        ///
        /// For a single-widget field this rectangle is used verbatim. For a
        /// MULTI-widget field (a radio group) the group is moved as a unit so
        /// its first widget's lower-left corner lands on this rectangle's —
        /// each button keeps its own size and its distance from the others,
        /// because that spacing is part of what the group means. The
        /// rectangle's size is then ignored, and the command says so.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// Paste as a NEW, INDEPENDENT field with this fully-qualified name.
        ///
        /// A period separates levels (§12.7.3.2) and any missing grouping
        /// ancestors are created, exactly as `add-text-field` does.
        #[arg(long, value_name = "NAME", conflicts_with = "as_widget_of")]
        as_new: Option<String>,
        /// Paste as ANOTHER WIDGET of the field already named this.
        ///
        /// One field, one value, two places on the page. Refused when no
        /// field here bears the name, and refused on a type mismatch.
        #[arg(long, value_name = "NAME", conflicts_with = "as_new")]
        as_widget_of: Option<String>,
        /// Also carry the copied field's VALUE (`--as-new` only).
        ///
        /// Off by default: a value is CONTENT. A "Revision" field arriving
        /// pre-filled with the source drawing's revision is a wrong answer
        /// that looks like a right one. The DEFAULT value (`/DV`) travels
        /// either way, so Reset Form still restores the right thing.
        #[arg(long)]
        copy_value: bool,
        /// Also carry the copied field's ACTIONS (`--as-new` only).
        ///
        /// Off by default: an action is BEHAVIOUR, and a calculation that
        /// refers to fields this document does not have arrives inert with
        /// nothing on screen to show it. When carried, a calculate action is
        /// appended to `/AcroForm /CO` (§12.7.2 Table 218 requires that array
        /// once any field has one). pdfcer never EXECUTES a script either way.
        #[arg(long)]
        copy_actions: bool,
        /// `/TU`, the accessibility name a screen reader announces
        /// (`--as-new` only).
        #[arg(long)]
        tooltip: Option<String>,
        /// Reuse the COPIED field's accessibility name (`--as-new` only).
        ///
        /// A legitimate explicit answer — you are copying your own field. It
        /// is reported, because two fields announcing themselves identically
        /// to a screen reader is invisible to a sighted operator.
        #[arg(long, conflicts_with_all = ["tooltip", "no_tooltip"])]
        carry_tooltip: bool,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--carry-tooltip` / `--no-tooltip` is
        /// required with `--as-new`. Omitting all three is an error, never a
        /// silent default: for a form field, `/TU` — not the tag tree — is
        /// what a screen reader announces.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the paste reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Copy whole pages** to a clipboard file, and with `--cut` remove them
    /// too (`Pass 171.0`).
    ///
    /// THE CLIP IS A PDF. Not a private payload — a real, openable document
    /// containing exactly the copied pages. Open it, mail it, or hand it to
    /// `page-paste`.
    ///
    /// Everything the pages reach travels: content, resources, fonts, images,
    /// annotations, and the form fields whose widgets are ENTIRELY on the
    /// copied pages. A field straddling a copied and an uncopied page is
    /// dropped and counted, because half a field is not a field.
    ///
    /// Document-level structures do not: the outline, named destinations,
    /// page labels, optional-content groups. `page-paste` reports what that
    /// cost on arrival, because it is the destination that decides the damage.
    PageCopy {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page numbers to copy, comma-separated (`1,3,5`).
        #[arg(long, value_name = "N,N,N")]
        pages: String,
        /// Where to write the clip — a PDF.
        #[arg(long)]
        clip: PathBuf,
        /// Also remove the copied pages, writing the result here — CUT.
        ///
        /// The copy runs first, so a page set that cannot be carried is
        /// refused with nothing deleted. REFUSED when it would remove every
        /// page: a document with no pages is not a document, and the failure
        /// would not be an error but a file that opens to nothing.
        #[arg(long, value_name = "OUTPUT.pdf")]
        cut: Option<PathBuf>,
        /// Save mode for `--cut`.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Paste whole pages** from a clipboard file into a document
    /// (`Pass 171.0`).
    ///
    /// READ THE COUNTERS. Two of them are invisible in the result.
    ///
    /// `orphaned_widgets` is the one that bites: a page's `/Annots` reaches
    /// its widgets, so form-field boxes ARRIVE even though the `/AcroForm`
    /// that owns them does not. They draw like fields and nothing can fill
    /// them. `orphaned_unrecoverable` is the subset that cannot even be
    /// adopted into a field afterwards, because the widget carries no name or
    /// type of its own.
    PagePaste {
        /// Input PDF — the document the pages are pasted INTO.
        input: PathBuf,
        /// The clip file written by `page-copy`.
        #[arg(long)]
        clip: PathBuf,
        /// Where to put them: `start`, `end`, `before:N` or `after:N`
        /// (1-based).
        #[arg(long, default_value = "end")]
        at: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Copy a bookmark and everything under it** to a clipboard file, and
    /// with `--cut` remove it too (`Pass 172.0`).
    ///
    /// ACROBAT CANNOT DO THIS BETWEEN TWO FILES AT ALL. Adobe's own
    /// documentation says bookmarks "can't be copied directly … from one file
    /// to another"; it offers cut and paste within a document and nothing
    /// between two.
    ///
    /// The clip carries the whole subtree — titles, destinations INCLUDING
    /// the zoom and scroll position, open state, colour and style flags.
    ///
    /// Use `list-outline` to find the object number of the bookmark to copy.
    BookmarkCopy {
        /// Input PDF.
        input: PathBuf,
        /// The bookmark's object number, as `list-outline` prints it.
        #[arg(long, value_name = "N")]
        item: u32,
        /// Where to write the clip.
        #[arg(long)]
        clip: PathBuf,
        /// Also remove the bookmark and its descendants, writing the result
        /// here — CUT. The copy runs first, so a subtree that cannot be
        /// carried is refused with nothing deleted.
        #[arg(long, value_name = "OUTPUT.pdf")]
        cut: Option<PathBuf>,
        /// Save mode for `--cut`.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Paste a bookmark subtree** from a clipboard file (`Pass 172.0`).
    ///
    /// A DESTINATION NAMING A PAGE THIS DOCUMENT DOES NOT HAVE IS DROPPED,
    /// not clamped to the last page. A bookmark that navigates confidently to
    /// the wrong place is worse than one that plainly does not navigate, and
    /// §12.3.3 permits an item with no destination. The count is printed.
    BookmarkPaste {
        /// Input PDF — the document the bookmarks are pasted INTO.
        input: PathBuf,
        /// The clip file written by `bookmark-copy`.
        #[arg(long)]
        clip: PathBuf,
        /// Become the last child of this bookmark's object number. Omit for a
        /// top-level bookmark.
        #[arg(long, value_name = "N")]
        under: Option<u32>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Author a new check box (ISO 32000-1 §12.7.4.2).
    ///
    /// Both appearance states are written at creation, so the box is
    /// immediately usable by `set-button-state` and immediately correct in a
    /// viewer — there is no `/NeedAppearances` fallback involved.
    AddCheckBox {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name — also how `fill-field` and
        /// `list-fields` refer to it.
        ///
        /// A PERIOD SEPARATES LEVELS (§12.7.3.2): `Personal.Address.Zip`
        /// creates the group `Personal`, the group `Personal.Address`, and
        /// the field `Zip` inside it — reusing any of those that already
        /// exist. A name segment may not itself contain a period, so a
        /// leading, trailing or doubled one is refused rather than guessed at.
        ///
        /// REUSING AN EXISTING NAME OF THE SAME TYPE MERGES: a second widget
        /// is attached to the same field rather than a second field created.
        /// One value, two places to see and edit it — which is how a check box
        /// appears on every page of a form. A different type under the same
        /// name is refused, and so is a name that belongs to a group.
        #[arg(long)]
        name: String,
        /// 1-based page number to place the box on.
        #[arg(long)]
        page: usize,
        /// The field rectangle in PDF user space, `llx,lly,urx,ury`.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// The ON state's name — the value this box exports when ticked.
        ///
        /// `Off` is reserved for the unticked state (§12.7.4.2.3) and is
        /// refused here. Override it when the form's submitted data needs a
        /// particular value, e.g. `--on-state Red`.
        #[arg(long, default_value = "Yes")]
        on_state: String,
        /// Create the box already ticked.
        #[arg(long)]
        checked: bool,
        /// `/TU`, the accessibility name a screen reader announces.
        #[arg(long)]
        tooltip: Option<String>,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--no-tooltip` is required. Omitting
        /// both is an error, never a silent default: for a form field, `/TU`
        /// — not the tag tree — is what a screen reader announces, so a
        /// missing one is invisible to the person creating the field and
        /// load-bearing for the person who cannot see the form. Declining is
        /// a legitimate answer; it just has to be an ANSWER, and it is
        /// reported back in the operation's disclosures.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// Mark the field read-only (`/Ff` bit 1).
        #[arg(long)]
        read_only: bool,
        /// Mark the field required at submit time (`/Ff` bit 2).
        #[arg(long)]
        required: bool,
        /// Pre-fill this field's properties from an existing field.
        ///
        /// Copies only NON-BOOLEAN, TYPE-MATCHED data — `--max-len` for a
        /// text field, the option list for a choice field, the on-state for
        /// a check box. A radio template copies nothing.
        ///
        /// Yes/no properties are never copied, and that is deliberate: these
        /// are presence flags, so a copied `--multiline` could be added but
        /// never turned off, and a single-line field could not be made from
        /// a multiline template. The accessibility name is never copied
        /// either — deciding it is the whole point of requiring
        /// `--tooltip`/`--no-tooltip`, and inheriting someone else's answer
        /// is not deciding.
        ///
        /// Anything given explicitly wins; this only fills gaps. When the
        /// template contributes nothing, it says so rather than silently
        /// doing nothing.
        #[arg(long, value_name = "FIELD")]
        defaults_from: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the add reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
        /// Border line style (§12.5.4 Table 166).
        #[arg(long, value_enum, default_value_t = BorderArg::Solid)]
        border: BorderArg,
        /// Border width in points. Zero means no border.
        #[arg(long, default_value_t = 1.0)]
        border_width: f64,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, PAINTED
        /// into the appearance at creation, not merely recorded.
        ///
        /// Accepts `none` (Table 189's empty array, which STATES no colour
        /// and is not the same as the key being absent), one number for
        /// DeviceGray, three for DeviceRGB or four for DeviceCMYK, comma
        /// separated, each 0-1. CMYK is written as CMYK, never converted.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR, painted into the
        /// appearance at creation. Same spelling as `--background`; `none`
        /// leaves a text or choice field with no frame.
        ///
        /// Not `--border` or `--border-width`, which are `/BS` (Table 166) --
        /// the border's style and width. Different dictionaries.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,
        /// Where the widget is visible (§12.5.3 Table 165).
        #[arg(long, value_enum, default_value_t = VisibilityArg::Visible)]
        visibility: VisibilityArg,
        /// Which glyph the box draws when ON: check, cross, star, circle,
        /// square or diamond. Default `check`.
        ///
        /// These are Acrobat's six. pdfcer draws each as VECTOR ARTWORK and
        /// also records the choice in `/MK /CA` as the ZapfDingbats character
        /// Acrobat stores, so another editor reads back which style you
        /// picked.
        ///
        /// Drawing rather than setting a ZapfDingbats font is deliberate:
        /// Acrobat's own appearance depends on resolving that font at display
        /// time and has a long-standing bug failing to, leaving the box
        /// blank. Paths need no font and no substitution.
        #[arg(long, value_name = "check|cross|star|circle|square|diamond")]
        check_style: Option<String>,
    },

    /// Author one member of a radio group (ISO 32000-1 §12.7.4.2.1).
    ///
    /// ONE CALL PER MEMBER, not per group. Repeat with the same `--name` and
    /// a different `--export-value` to build the group up; the second call
    /// merges a widget into the field the first created, exactly as a check
    /// box repeated across pages does. There is no `add-radio-group` verb,
    /// because there is no moment at which pdfcer could know you were
    /// finished — a one-member group is a legitimate intermediate state.
    ///
    /// Both appearance states are written per member, so the group is
    /// immediately usable by `set-button-state` and correct in a viewer.
    AddRadioButton {
        /// Input PDF.
        input: PathBuf,
        /// The GROUP's fully-qualified name — shared by every member, and how
        /// `set-button-state` and `list-fields` refer to it.
        ///
        /// A PERIOD SEPARATES LEVELS (§12.7.3.2): `Personal.Contact.Method`
        /// creates the groups `Personal` and `Personal.Contact` and the field
        /// `Method` inside it — reusing any that already exist.
        ///
        /// REUSING THIS NAME IS HOW A GROUP IS BUILT: each call adds a member.
        /// A check box or a text field under the same name is refused — a
        /// check box and a radio are both `/FT /Btn` and would otherwise
        /// merge into one field whose widgets disagree about whether they
        /// toggle independently or exclusively.
        #[arg(long)]
        name: String,
        /// 1-based page number to place this member on.
        ///
        /// Members may sit on DIFFERENT pages; the group is one field
        /// regardless.
        #[arg(long)]
        page: usize,
        /// This member's rectangle in PDF user space, `llx,lly,urx,ury`.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// This member's export value — its identity within the group.
        ///
        /// It is simultaneously the `/AP /N` key, the `/AS` when this member
        /// is chosen, and the `/V` the group takes (§12.7.4.2.1). Members are
        /// told apart by it and nothing else, so two members may not share
        /// one unless `--radios-in-unison` says they select together.
        ///
        /// `Off` is reserved for the unselected state (§12.7.4.2.3).
        #[arg(long)]
        export_value: String,
        /// Make this member the group's initial selection.
        #[arg(long)]
        selected: bool,
        /// `/TU`, the accessibility name a screen reader announces.
        #[arg(long)]
        tooltip: Option<String>,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--no-tooltip` is required.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// `/Ff` bit 15 — once a member is chosen, clicking it again does not
        /// clear the group.
        ///
        /// Only the call that CREATES the group decides this; a later member
        /// passing a different value is told its flag was ignored rather than
        /// silently rewriting how the existing members behave.
        #[arg(long)]
        no_toggle_to_off: bool,
        /// `/Ff` bit 26 — members sharing an export value turn on together.
        ///
        /// This is also what permits a duplicate `--export-value`, which is
        /// otherwise refused. Only the creating call decides it.
        #[arg(long)]
        radios_in_unison: bool,
        /// Mark the field read-only (`/Ff` bit 1).
        #[arg(long)]
        read_only: bool,
        /// Mark the field required at submit time (`/Ff` bit 2).
        #[arg(long)]
        required: bool,
        /// Pre-fill this field's properties from an existing field.
        ///
        /// Copies only NON-BOOLEAN, TYPE-MATCHED data — `--max-len` for a
        /// text field, the option list for a choice field, the on-state for
        /// a check box. A radio template copies nothing.
        ///
        /// Yes/no properties are never copied, and that is deliberate: these
        /// are presence flags, so a copied `--multiline` could be added but
        /// never turned off, and a single-line field could not be made from
        /// a multiline template. The accessibility name is never copied
        /// either — deciding it is the whole point of requiring
        /// `--tooltip`/`--no-tooltip`, and inheriting someone else's answer
        /// is not deciding.
        ///
        /// Anything given explicitly wins; this only fills gaps. When the
        /// template contributes nothing, it says so rather than silently
        /// doing nothing.
        #[arg(long, value_name = "FIELD")]
        defaults_from: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the add reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
        /// Border line style (§12.5.4 Table 166).
        #[arg(long, value_enum, default_value_t = BorderArg::Solid)]
        border: BorderArg,
        /// Border width in points. Zero means no border.
        #[arg(long, default_value_t = 1.0)]
        border_width: f64,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, PAINTED
        /// into the appearance at creation, not merely recorded.
        ///
        /// Accepts `none` (Table 189's empty array, which STATES no colour
        /// and is not the same as the key being absent), one number for
        /// DeviceGray, three for DeviceRGB or four for DeviceCMYK, comma
        /// separated, each 0-1. CMYK is written as CMYK, never converted.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR, painted into the
        /// appearance at creation. Same spelling as `--background`; `none`
        /// leaves a text or choice field with no frame.
        ///
        /// Not `--border` or `--border-width`, which are `/BS` (Table 166) --
        /// the border's style and width. Different dictionaries.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,
        /// Where the widget is visible (§12.5.3 Table 165).
        #[arg(long, value_enum, default_value_t = VisibilityArg::Visible)]
        visibility: VisibilityArg,
    },

    /// Delete a form field entirely (ISO 32000-1 §12.7.3).
    ///
    /// Removes every widget from its page, the field dictionary, its
    /// `/AcroForm /Fields` registration, and any grouping node left childless
    /// — a named node owning nothing still occupies its slot in the
    /// fully-qualified-name space and would refuse a later field wanting the
    /// name.
    ///
    /// To remove ONE member of a radio group rather than the group, use
    /// `delete-widget`.
    DeleteField {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the deletion reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Delete a grouping node and every field beneath it** (§12.7.3.2).
    ///
    /// `delete-field` names ONE terminal. This names an interior node of the
    /// field tree — `Personal`, not `Personal.Name` — and removes the whole
    /// subtree: every terminal under it however deep, all their widgets, and
    /// the intermediate nodes themselves.
    ///
    /// **It removes fields you did not name**, which is why it will not run
    /// without `--yes`. Run it without that flag first: it prints exactly
    /// which terminals would go and exits without writing. That listing is
    /// the point of the command — a subtree is precisely the thing an
    /// operator cannot see the inside of before deleting it.
    ///
    /// A terminal field's name is REFUSED here rather than quietly treated
    /// as a one-field delete: the two commands remove different amounts, and
    /// guessing which you meant is how a mistyped name becomes silent data
    /// loss. Use `delete-field` for a terminal.
    DeleteFieldGroup {
        /// Input PDF.
        input: PathBuf,
        /// The grouping node's fully-qualified name — the dotted prefix
        /// `list-fields` shows on the terminals beneath it.
        #[arg(long)]
        name: String,
        /// Output path. Not written unless `--yes` is given.
        #[arg(short, long)]
        output: PathBuf,
        /// Actually perform the deletion. Without it, the affected fields
        /// are listed and nothing is written.
        #[arg(long)]
        yes: bool,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the deletion reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Rename a form field** (ISO 32000-1 §12.7.3.2).
    ///
    /// `--to` is a **partial** name — the one path segment this field
    /// contributes — not a fully-qualified one. Renaming `Address.City` to
    /// `Town` gives `Address.Town`; the field keeps its place in the tree,
    /// and this verb deliberately cannot re-parent it.
    ///
    /// RENAMING A GROUP RENAMES EVERYTHING UNDER IT. §12.7.3.2 builds a
    /// fully-qualified name by appending each node's partial name walking
    /// down, so renaming `Address` re-derives `Address.City` as
    /// `Location.City` — without writing to `City` at all. The output line
    /// reports `descendants_renamed` for exactly this reason: a one-field
    /// request can rename six. Button actions naming any of them are
    /// REPAIRED and counted (`action_targets_retargeted`); an FDF or a
    /// JavaScript naming them is not, and stops matching.
    ///
    /// A rename onto a name something else already holds is REFUSED, not
    /// merged — unlike `add-*`, which merges a same-type name because the
    /// caller asked for a field of that name. Here they asked for an
    /// existing field to take a new one, and fusing two identities is not
    /// something the request describes.
    RenameField {
        /// Input PDF.
        input: PathBuf,
        /// The field's current fully-qualified name, as `list-fields`
        /// reports it.
        #[arg(long)]
        name: String,
        /// The new PARTIAL name — one segment, no periods.
        #[arg(long = "to")]
        to: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the rename reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Write a note onto an annotation that already exists** —
    /// `/Contents`, and optionally `/T` (author) and `/M` (date).
    ///
    /// The counterpart of `set-markup-style`, which does this shape for
    /// colour, width and opacity.
    ///
    /// # Why this exists
    ///
    /// A note could previously only be written at the moment an annotation
    /// was CREATED (`annotate --note`), and a geometric markup has no
    /// text-entry moment: a cloud or a highlight is authored from geometry
    /// alone. Commenting a shape you already drew, or fixing a typo in a
    /// comment, had no route at all.
    ///
    /// # What it leaves alone
    ///
    /// Omitting `--note-author` or `--note-date` leaves any existing author
    /// and date UNTOUCHED. Correcting a typo does not un-sign a comment.
    /// Use `--clear` to remove all three.
    ///
    /// The previous note text is PRINTED when this replaces one — those
    /// words leave no trace on the page, unlike a restyle where the shape
    /// still shows.
    SetMarkupNote {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=`
        /// value `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// The note text (`/Contents`). Required unless `--clear`.
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// Author (`/T`). Omit to leave any existing author alone.
        #[arg(long, value_name = "NAME")]
        note_author: Option<String>,
        /// Modification date (`/M`) as a §7.9.4 string, e.g.
        /// `D:20260828120000Z`. Omit to leave any existing date alone —
        /// pdfcer does not read a clock, deliberately.
        #[arg(long, value_name = "D:YYYYMMDDHHmmSSZ")]
        note_date: Option<String>,
        /// Remove the note entirely — `/Contents`, `/T` and `/M`.
        ///
        /// A distinct act from an empty `--note ""`: an empty comment is a
        /// comment, and the saved bytes tell the two apart.
        #[arg(long, conflicts_with_all = ["note", "note_author", "note_date"])]
        clear: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Restyle a sticky note, stamp or text box** — its icon and/or its
    /// colour (ISO 32000-1 §12.5.6.4, §12.5.2 Table 164).
    ///
    /// THE VERB `set-markup-style` CANNOT REACH THESE SUBTYPES. That one
    /// reads through the geometric spec model, which has no `/Text` arm and
    /// says so by name. Without this verb the only route to a different
    /// icon would be to delete the note and place another, losing its `/M`,
    /// its object identity and any reply hung off it.
    ///
    /// The appearance is REGENERATED, because pdfcer paints from `/AP` or
    /// not at all — writing `/Name` alone would leave the note drawing its
    /// old icon while the dictionary claimed otherwise.
    ///
    /// `--icon` is refused by name on anything but a `/Text`: a stamp's face
    /// comes from its own vocabulary and a text box has no icon.
    SetTextAnnotStyle {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED.
        #[arg(long)]
        index: usize,
        /// Sticky-note icon. `/Text` only.
        #[arg(long, value_name = "NAME")]
        icon: Option<StickyIconArg>,
        /// Colour as `RRGGBB` hex.
        ///
        /// There is no `none`: the authoring model gives a note's icon, a
        /// stamp's face and a text box's frame a REQUIRED colour, so "no
        /// colour" is not a state it can express and offering the word
        /// would mean inventing a fallback.
        #[arg(long, value_name = "RRGGBB")]
        color: Option<String>,
        /// **Label size in points** for a `/Stamp` or `/FreeText`
        /// (`Pass 292.0`).
        ///
        /// Refused by name on a `/Text` sticky note, which draws an icon and
        /// has no label to size. A stamp's new size is written to `/DA` and
        /// its appearance re-baked, so the size survives a later resize.
        #[arg(long, value_name = "POINTS")]
        font_size: Option<f64>,
        /// What a stamp does when the RESIZED label no longer fits its box:
        /// `grow` (default — widen the box), `shrink` (smaller text, same
        /// box), `clip` (cut the label).
        ///
        /// Only meaningful with `--font-size`, and refused without it. The
        /// author's original fit intent is NOT recorded anywhere in a PDF,
        /// so this is your choice rather than a recovered one.
        #[arg(long, value_enum)]
        stamp_fit: Option<StampFitArg>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Set a review status on a comment** — `/State` + `/StateModel`
    /// (ISO 32000-1 §12.5.6.3, Table 171).
    ///
    /// THE STATUS IS NOT WRITTEN ONTO THE COMMENT. §12.5.6.3 puts it on a
    /// SEPARATE text annotation that points back through `/IRT`, and says
    /// so with a `shall` — so this creates an annotation and leaves the one
    /// you named untouched.
    ///
    /// A SECOND STATUS BY THE SAME AUTHOR CHAINS ONTO THEIR FIRST, not onto
    /// the comment: "Additional state changes shall be made by adding text
    /// annotations in reply to the previous reply for a given user." The
    /// printed `attached_to=` says which annotation this one replied to,
    /// because the wrong shape is invisible in every viewer.
    ///
    /// pdfcer does NOT decide which of several statuses is "current". The
    /// standard says nothing about ordering or currency, `/M` is optional
    /// and empirically ties, so any resolver would be guessing.
    SetReviewState {
        /// Input PDF.
        input: PathBuf,
        /// Page of the annotation being reviewed, 1-BASED.
        #[arg(long)]
        page: usize,
        /// Index of the annotation being reviewed, 0-BASED.
        #[arg(long)]
        index: usize,
        /// The status. `marked`/`unmarked` are the Marked model; the rest
        /// are the Review model. `/StateModel` follows automatically.
        #[arg(long, value_enum)]
        state: ReviewStateArg,
        /// The reviewer's name (`/T`). Required: §12.5.6.3 makes the
        /// per-user chain the structure, so a status with no owner has
        /// nowhere to chain.
        #[arg(long, value_name = "NAME")]
        author: String,
        /// The status's `/M` date, verbatim.
        #[arg(long, value_name = "D:YYYYMMDDHHMMSS")]
        note_date: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Reply to a comment** — a `/Text` annotation carrying `/IRT` and
    /// `/RT /R` (ISO 32000-1 §12.5.6.2, Table 170).
    ///
    /// A thread can be CONTINUED, not merely read: this verb is what writes
    /// `/IRT` and `/RT`.
    ///
    /// The reply is placed at its parent's own rectangle, on its parent's
    /// page, and takes its parent's colour so a thread reads as one
    /// conversation. It arrives CLOSED; use `set-annotation-open` to change
    /// that.
    ///
    /// `--note-date` is yours to supply, as everywhere else — pdfcer reads
    /// no clock, so an omitted date means the key is not written rather
    /// than that "now" is invented.
    AddReply {
        /// Input PDF.
        input: PathBuf,
        /// Page of the annotation being replied to, 1-BASED.
        #[arg(long)]
        page: usize,
        /// Index of the annotation being replied to, 0-BASED.
        #[arg(long)]
        index: usize,
        /// The reply's text (`/Contents`).
        #[arg(long)]
        note: String,
        /// The reply's author (`/T`).
        #[arg(long, value_name = "NAME")]
        note_author: Option<String>,
        /// The reply's `/M` date, verbatim (`D:YYYYMMDDHHMMSS`).
        #[arg(long, value_name = "D:YYYYMMDDHHMMSS")]
        note_date: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Open or close an annotation's pop-up window** — `/Open`
    /// (ISO 32000-1 §12.5.6.4 Table 172, §12.5.6.14 Table 183).
    ///
    /// WRITES BOTH THE ANNOTATION AND ITS `/Popup` COMPANION, as one undo
    /// entry. Table 170 gives geometric markup no `/Open` of its own, so a
    /// square's window state lives only on the companion, and a verb that
    /// wrote one of the two would leave them disagreeing.
    ///
    /// `list-annotations` prints this key as
    /// `open=1|0|none`, where `none` means the file carries no such key —
    /// a different fact from `0`, and the reason the reported value has
    /// three states.
    ///
    /// IT DOES NOT CREATE A `/Popup`. An annotation without one has no
    /// window to open, and choosing that companion's rectangle would be
    /// authoring rather than a state change. Such a call is a reported
    /// no-op, not a refusal, so a script can pass a whole page's
    /// annotations without first filtering by subtype.
    SetAnnotationOpen {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=`
        /// value `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// Open the window. Pass `--open false` to close it.
        #[arg(long, action = clap::ArgAction::Set)]
        open: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Set an annotation's display flags** (`/F`, ISO 32000-1 §12.5.3
    /// Table 165) — hide it, stop it printing, or LOCK it.
    ///
    /// # The whole word, not one bit at a time
    ///
    /// Table 165's bits interact: `--no-view` with `--print` means *prints
    /// but is not on screen*, which an operator reaches deliberately. So this
    /// takes the complete set and writes it, rather than toggling one bit and
    /// leaving the rest — a sequence of individually-sensible toggles can
    /// build a state nobody intended. `list-annotations` prints the current
    /// word as `flags=0x…`; pass the flags you want the annotation to END
    /// with.
    ///
    /// # Locked, and why you can still unlock
    ///
    /// `--locked` is the flag Table 165 bit 8 defines: pdfcer's own move,
    /// resize, rotate and restyle verbs refuse a Locked annotation. Clearing
    /// it here is deliberately allowed — a lock you could only undo in
    /// another application would be a one-way door.
    ///
    /// **`--locked-contents` is a DIFFERENT flag** (bit 10) and guards the
    /// annotation's text, not its geometry. It does not stop a move.
    ///
    /// # Refuses a form widget by name
    ///
    /// A widget's visibility belongs to `edit-widget --visibility`, whose
    /// four combinations cannot express a contradictory pair. Two writers of
    /// one key with different vocabularies is how a field ends up in a state
    /// its own editor cannot describe.
    SetAnnotationFlags {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=` value
        /// `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// Bit 1 — the annotation is not displayed or printed at all.
        #[arg(long)]
        invisible: bool,
        /// Bit 2 — hidden: not displayed, not printed, not interactive.
        #[arg(long)]
        hidden: bool,
        /// Bit 3 — print the annotation. Most authored markup wants this.
        #[arg(long)]
        print: bool,
        /// Bit 4 — do not scale the annotation with the page zoom.
        #[arg(long)]
        no_zoom: bool,
        /// Bit 5 — do not rotate the annotation with the page.
        #[arg(long)]
        no_rotate: bool,
        /// Bit 6 — do not display on screen (may still print).
        #[arg(long)]
        no_view: bool,
        /// Bit 8 — LOCKED: pdfcer's move, resize, rotate and restyle verbs
        /// refuse it. Omit the flag to clear the lock.
        #[arg(long)]
        locked: bool,
        /// Bit 10 — LockedContents: guards the text, NOT the geometry. Does
        /// not stop a move.
        #[arg(long)]
        locked_contents: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Restyle an EXISTING markup annotation in place (ISO 32000-1
    /// §12.5.6), keeping its object identity.
    ///
    /// ADDRESSED BY `--page` + `--index`, the same pair
    /// `list-annotations` prints and `delete-annotation` takes.
    ///
    /// The appearance stream is **regenerated** from the annotation's own
    /// declared geometry, not just its `/C` — pdfcer paints from `/AP` or
    /// not at all, so setting the colour without redrawing would leave the
    /// change invisible. Anything the original appearance expressed that
    /// pdfcer does not model (a cloudy `/BE` border, a dashed `/BS`, an
    /// exotic arrowhead) is reported on stderr as it is dropped.
    ///
    /// Refuses, by name: a ce dimension (use `set-dimension-style`), a
    /// subtype pdfcer cannot author an appearance for (`FreeText`,
    /// `Stamp`, `Widget`, `Link`), an annotation whose geometry keys are
    /// missing, and a `Locked` annotation (§12.5.3 Table 165 bit 8).
    SetMarkupStyle {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=`
        /// value `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// Stroke/border colour as `RRGGBB` hex, or `none` to remove it
        /// (§12.5.6 spells "no border" as an ABSENT `/C`, not as a
        /// colour). On a `/Line`, `/Ink`, `/PolyLine` or text markup the
        /// stroke is unconditional, so `none` there means black.
        #[arg(long, value_name = "RRGGBB|none")]
        color: Option<String>,
        /// Interior (fill) colour as `RRGGBB` hex, or `none` for a
        /// transparent interior. `Square`, `Circle` and `Polygon` only.
        #[arg(long, value_name = "RRGGBB|none")]
        interior: Option<String>,
        /// Border width in points. On every subtype except `Square` and
        /// `Circle` this also moves `/Rect`, because the rectangle is
        /// derived from the geometry plus a margin containing the stroke.
        #[arg(long, value_name = "PT")]
        width: Option<f64>,
        /// Constant opacity `/CA`, 0.0–1.0 (§12.5.2), or `none` to
        /// remove the entry — which is fully opaque, and is a different
        /// fact about the file from an explicit `1.0`.
        #[arg(long, value_name = "0.0-1.0|none")]
        opacity: Option<String>,
        /// Border line style: a dash pattern as comma-separated point
        /// lengths (`4,2` = 4 on, 2 off; `3` = the Table 166 default), or
        /// `solid` to remove the dash.
        ///
        /// OMITTING THIS PRESERVES AN EXISTING DASH — a restyle that
        /// regenerated the appearance without one would silently solidify a
        /// dashed border, because `/AP` is what gets painted (R43).
        ///
        /// Refused by name on a text markup, which has no border to dash.
        #[arg(long, value_name = "ON,OFF,...|solid")]
        dash: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the restyle reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Delete an annotation** — any subtype, addressed as `list-annotations`
    /// reports it (ISO 32000-1 §12.5.2).
    ///
    /// The general deletion verb. Before it, pdfcer could delete only the three
    /// annotation kinds that had a verb of their own (a redaction mark, a ce
    /// dimension, a form-field widget) — a highlight, a square, a stamp or a
    /// FreeText note, including ones pdfcer itself authored, could not be
    /// removed at all.
    ///
    /// ADDRESSED BY `--page` + `--index`, the exact pair `list-annotations`
    /// prints, so the two commands compose: list, read the index, delete it.
    /// `--page` is 1-based, `--index` is 0-based within that page's `/Annots`
    /// array — the same convention `list-annotations` uses on its own output,
    /// not a second one invented here.
    ///
    /// FOUR THINGS CAN HAPPEN BESIDES THE OBVIOUS ONE, and the output line
    /// reports each:
    ///
    /// - Its `/Popup` window goes with it. §12.5.6.14 says a pop-up "shall not
    ///   appear alone", so this is the spec's requirement, not tidying —
    ///   `popup_removed=1`.
    /// - Replies to it (`/IRT`) SURVIVE, with their now-dangling link removed
    ///   — `replies_orphaned=N`. They are somebody's text and you asked to
    ///   delete one annotation; deleting a thread is N deletions.
    /// - `/RT /Group` subordinates of it are counted separately
    ///   (`group_promoted=N`) because the consequence is worse: while the
    ///   primary existed a reader was instructed to IGNORE their own author
    ///   and note text in favour of its, so removing it makes several other
    ///   comments start displaying text that was previously suppressed.
    /// - Appearance streams go only if nothing else uses them
    ///   (`ap_removed=N`) — forty stamps sharing one "DRAFT" stream keep it.
    ///
    /// A `/Widget` is REFUSED, not deleted: use `delete-widget` for that one
    /// widget or `delete-field` for the whole field. Deleting it here would
    /// leave the field registered in `/AcroForm /Fields` with nothing on the
    /// page, and which of the two you meant is not something this verb may
    /// guess. A `/Redact` mark and a ce dimension ARE accepted and are routed
    /// to their own verbs, so their sidecar/review semantics still apply —
    /// `route=` says which ran.
    DeleteAnnotation {
        /// Input PDF.
        input: PathBuf,
        /// Page, 1-BASED — the `page=` value `list-annotations` prints.
        #[arg(long)]
        page: usize,
        /// Index within that page's `/Annots`, 0-BASED — the `index=` value
        /// `list-annotations` prints.
        #[arg(long)]
        index: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the deletion reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Delete ONE widget of a form field (ISO 32000-1 §12.5.6.19).
    ///
    /// The usual case is dropping a member from a radio group, or one of the
    /// several places a check box appears across a form's pages.
    ///
    /// THREE THINGS CAN HAPPEN, and the output line says which:
    ///
    /// - Normally the widget goes and the field stays.
    /// - If the deleted widget held the field's VALUE, that value would name
    ///   a state no remaining widget can display, so it is cleared to `Off`
    ///   along with every survivor's appearance state — and
    ///   `selection_cleared=1` reports it.
    /// - If it was the LAST widget, the field goes too, exactly as
    ///   `delete-field` would.
    ///
    /// A group reduced to one member keeps its `/Kids` structure rather than
    /// collapsing back into a single merged dictionary: both shapes are
    /// legal, so the deletion does not rewrite object identities nobody asked
    /// it to change.
    DeleteWidget {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Which widget to remove, numbered from 0 in the order
        /// `list-fields` reports the field's widgets.
        #[arg(long)]
        index: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the deletion reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Set or clear a field's **format**, **validate** or **calculate**
    /// script -- `/AA` `/F`, `/V`, `/C` (`Pass 308.6`).
    ///
    /// ONE of `--format-*`, `--validate-range` or `--calculate` per run, or
    /// `--clear` with `--trigger` to remove one.
    ///
    /// There is NO way to pass arbitrary JavaScript, and that is the point:
    /// pdfcer authors only the helper calls it can also read back and
    /// describe. A script it cannot classify is a script it will not write.
    ///
    /// Only TEXT fields and COMBO (drop-down) choice fields carry these. A
    /// list box carries none of the three -- it is a `/Ch` like a combo box
    /// and Acrobat treats it differently.
    SetFieldScript {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Number format: decimals,separator,negative,currency-style,symbol,prepend.
        ///
        /// Six values, comma separated, matching `AFNumber_Format`'s own
        /// argument order -- e.g. `2,0,0,0,$,true` for two decimals with a
        /// leading dollar sign. Writes the paired keystroke filter too.
        #[arg(long, value_name = "N,S,N,C,SYM,BOOL")]
        format_number: Option<String>,
        /// Percent format: decimals,separator.
        #[arg(long, value_name = "N,S")]
        format_percent: Option<String>,
        /// Date format by Acrobat's predefined index.
        #[arg(long, value_name = "INDEX")]
        format_date: Option<i64>,
        /// Date format by an explicit format string, e.g. `yyyy-mm-dd`.
        #[arg(long, value_name = "FORMAT")]
        format_date_string: Option<String>,
        /// Time format by Acrobat's predefined index.
        #[arg(long, value_name = "INDEX")]
        format_time: Option<i64>,
        /// Special format selector -- zip, zip+4, phone, social-security.
        #[arg(long, value_name = "SELECTOR")]
        format_special: Option<i64>,
        /// Validate against a numeric range: `MIN..MAX`, `MIN..` or `..MAX`.
        ///
        /// pdfcer DISCLOSES a range and never enforces it -- its fills are
        /// operator-reviewed (decision 009 §6). Writing one authors a
        /// constraint for other readers, which is what building a form for
        /// distribution means; it is not a promise pdfcer starts keeping.
        #[arg(long, value_name = "MIN..MAX")]
        validate_range: Option<String>,
        /// Calculate: `OP:field,field,...` where OP is SUM, AVG, PRD, MIN or
        /// MAX -- case sensitive, as Acrobat writes them.
        ///
        /// Also registers the field in the form's `/CO` calculation order,
        /// appended at the end. A calculation absent from `/CO` is one Acrobat
        /// will not run.
        #[arg(long, value_name = "OP:FIELDS")]
        calculate: Option<String>,
        /// REMOVE the script named by `--trigger`.
        #[arg(long, requires = "trigger")]
        clear: bool,
        /// Which script `--clear` removes: format, validate or calculate.
        ///
        /// Clearing a format removes its paired keystroke filter too.
        #[arg(long, value_name = "format|validate|calculate")]
        trigger: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Re-open the saved file and verify the undo entry round-trips.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Move a markup, link, redaction-mark, stamp or note annotation** by a
    /// delta in points (`Pass 149.0`).
    ///
    /// The move verb the annotation family did not have. `move-widget` covers
    /// form widgets and `dimension-*` covers ce dimensions; everything else —
    /// Ink, Square, Circle, Line, Polygon, PolyLine, the four text markups,
    /// FreeText, Text notes, Stamp, Link and unapplied Redact marks — had no
    /// way to move at all.
    ///
    /// # What moves, and the half that is easy to get wrong
    ///
    /// `/Rect`, and **every geometry key the annotation carries** — `/L`,
    /// `/Vertices`, `/InkList`, `/QuadPoints`, `/CL`. Those hold absolute
    /// page coordinates and are what any OTHER tool regenerates an appearance
    /// from, so moving `/Rect` alone would render in the new place and be
    /// reconstructed in the old one by the next viewer that rebuilt it.
    ///
    /// The appearance stream is **not rewritten**. ISO 32000-1 §12.5.5
    /// recomputes the placement matrix from the appearance `BBox` and the new
    /// `/Rect`, so a pure translation moves the artwork 1:1 for free — and an
    /// appearance pdfcer did not author survives the move intact. A move is
    /// not a restyle.
    ///
    /// # What is deliberately left, and reported
    ///
    /// `/RD` (rect differences) are inset DISTANCES, not coordinates;
    /// translating them would deform the annotation while claiming to move
    /// it. A `/Popup` is a separate annotation with its own placement
    /// (§12.5.6.14 leaves it to the reader) and is named in the report so you
    /// can move it too if you want it to follow.
    MoveAnnotation {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the annotation is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Which annotation, numbered from 0 in `list-annotations` order.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Horizontal shift in points, positive to the right.
        #[arg(long, allow_negative_numbers = true)]
        dx: f64,
        /// Vertical shift in points, positive UP — PDF user space has its
        /// origin at the bottom-left corner (§8.3.2.3), not the top.
        #[arg(long, allow_negative_numbers = true)]
        dy: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Put a page's annotations in a new order** — its `/Annots` array,
    /// which is the paint order and, on nearly every page, the tab order.
    ///
    /// # What this is for
    ///
    /// A page with no `/Tabs` entry (ISO 32000-1 §7.7.3.3 Table 30, values
    /// in §12.5.1) states no tab order at all, and readers in practice fall
    /// back to the order of `/Annots`; under PDF 2.0's `/Tabs /A` (Table 31)
    /// that order is the stated one. So "tab through these fields in this
    /// order" is, at the file level, "arrange these references in this
    /// order" — and that is what this command does. No annotation is
    /// rewritten, recreated or re-registered: every widget keeps its object
    /// id, its field, its `/Parent` chain and its triggers. Cut-and-paste
    /// would have rebuilt them; this moves them.
    ///
    /// # Three things the standard makes follow the array, and do
    ///
    /// A `/TrapNet` annotation shall stay the last entry (§12.5.6.21) — it
    /// is held in place, and may be listed last or left out. A trap
    /// network's `/AnnotStates` shall stay index-parallel to `/Annots`
    /// (Table 366) — it is permuted alongside. A `/GoToE` target's integer
    /// `/A` is an index into this array (Table 202) — every one aimed at
    /// this page is re-indexed to the annotation's new position. Each is
    /// reported when it happens.
    ///
    /// # `--order`
    ///
    /// A comma-separated list of the page's annotation indices **in the
    /// order you want them**, numbered from 0 as `list-annotations` prints
    /// them — every index exactly once. `2,0,1` puts the third annotation
    /// first. A list that drops or repeats an index is refused, because that
    /// would be a delete or a duplicate wearing a reorder's name.
    ///
    /// # What is reported, and why
    ///
    /// The page's `/Tabs` value, because under `/R`, `/C` or `/S` a reader
    /// does not tab by array order at all and the order you arranged will
    /// not be the order it uses — the array is still reordered (paint order
    /// changed as asked), and the mismatch is said out loud rather than
    /// discovered by tabbing. Any non-widget that moved, because `/Annots`
    /// order is also which annotation draws on top where two overlap. And
    /// any entry that is a direct dictionary rather than an indirect object,
    /// which has no identity to name and stays where it was.
    ///
    /// `/Tabs` itself is **not** written. Stating the order in the file is a
    /// change to the page dictionary with its own standards consequences
    /// and is a separate act.
    ReorderAnnotations {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page whose annotations are being reordered.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// The new order as `list-annotations` indices, e.g. `2,0,1` — every
        /// annotation on the page exactly once.
        #[arg(long)]
        order: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the edit reproduces the input file
        /// byte for byte.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Rotate an annotation** about a point (`Pass 155.0`) — the third
    /// transform, after `move-annotation` and `resize-annotation`.
    ///
    /// # Why this one cannot distort anything
    ///
    /// A rotation is an isometry: every length is preserved, including the
    /// drawn stroke width. So unlike `resize-annotation` there are no options
    /// and no refusal for foreign artwork — the rotation is written into the
    /// appearance's own `/Matrix` (§12.5.5 step a), composed with whatever
    /// the producer already had, so nothing is redrawn.
    ///
    /// # `/Rect` gets BIGGER, and that is correct
    ///
    /// §12.5.2 requires `/Rect` to be upright, and the upright box bounding a
    /// rotated shape is larger unless the angle is a multiple of 90°. The
    /// artwork does not grow; only the rectangle around it does.
    ///
    /// **`/Rect` IS DERIVED FROM THE ARTWORK**, never from the previous
    /// rectangle, and `rect_derived=` on the second output line says from
    /// which of three sources. Deriving it from the previous rectangle
    /// compounds: each turn bounds an already-grown box while the
    /// appearance's `/Matrix` only accumulates the angle, and §12.5.5 then
    /// scales the artwork **up** to fill the surplus — four 15° turns draw a
    /// square 1.93× wider than one 60° turn.
    ///
    /// `/RD` is left alone and reported: at an angle that is not a quarter
    /// turn, no axis-aligned inset expresses the rotated result.
    RotateAnnotation {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the annotation is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Which annotation, numbered from 0 in `list-annotations` order.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Rotation in degrees, ANTICLOCKWISE — PDF user space has its origin
        /// at the bottom-left (§8.3.2.3), so positive turns the way a
        /// mathematician expects and not the way a screen does.
        ///
        /// A DELTA from wherever the annotation is now, unless `--absolute`.
        #[arg(long, allow_negative_numbers = true)]
        degrees: f64,
        /// Treat `--degrees` as an ABSOLUTE angle measured from the
        /// annotation's authored orientation, rather than as a delta
        /// (`Pass 155.2`).
        ///
        /// pdfcer reads the annotation's current angle out of its appearance
        /// `/Matrix` and applies the difference — so `--absolute --degrees 45`
        /// leaves it at 45° whatever it was at before, and running it twice
        /// changes nothing the second time.
        ///
        /// This is what a typed properties field needs. It REFUSES rather
        /// than guesses when the current angle cannot be read: an annotation
        /// with no appearance stream has nowhere to record an orientation
        /// (§12.5.2 requires `/Rect` upright), and a `/Matrix` carrying a
        /// shear or a mirror is not an angle at all. Without this flag the
        /// delta form works on both, because a delta needs no starting angle.
        #[arg(long)]
        absolute: bool,
        /// Pivot x in points — the point that does NOT move.
        #[arg(long, allow_negative_numbers = true)]
        anchor_x: f64,
        /// Pivot y in points.
        #[arg(long, allow_negative_numbers = true)]
        anchor_y: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Resize a markup, link, stamp or note annotation** by scaling it about
    /// an anchor point (`Pass 151.0`).
    ///
    /// The other half of `move-annotation`. `/Rect` and every geometry key
    /// scale together about `--anchor-x`/`--anchor-y`, so the annotation
    /// cannot render at one size and be reconstructed at another by a tool
    /// that regenerates from `/Vertices` or `/InkList`.
    ///
    /// # The two toggles, and why their defaults point opposite ways
    ///
    /// `--scale-stroke-width` is **off**: a border width is a drafting
    /// convention, and on a CAD drawing a line weight means something to
    /// whoever reads the print. `/RD` (rect differences) **does** scale, and
    /// `--keep-rect-differences` opts out.
    ///
    /// That looks inconsistent and is not. The test is: **is the property a
    /// length in the space being transformed?** An inset is; a line weight is
    /// not. It is also why `move-annotation` scales neither — a translation
    /// changes no length at all.
    ///
    /// # The appearance, which is where a resize stops resembling a move
    ///
    /// §12.5.5 maps the appearance's `BBox` onto `/Rect`, which under a
    /// translation is free and under a scale is a matrix applied AFTER
    /// stroking — so the drawn stroke scales whatever `/BS /W` says, and under
    /// a non-uniform scale it is anisotropic, which no single stroke width can
    /// express.
    ///
    /// pdfcer re-authors the appearance where it drew it (established by
    /// comparing BYTES, not by asking whether the dictionary parses), so both
    /// toggle states come out exact. A foreign appearance it will not redraw:
    /// a uniform scale with `--scale-stroke-width` proceeds because the matrix
    /// then does exactly what was asked, and anything else is REFUSED unless
    /// `--allow-appearance-distortion` takes the distortion knowingly.
    ResizeAnnotation {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the annotation is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Which annotation, numbered from 0 in `list-annotations` order.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Horizontal scale factor. Negative mirrors; zero is refused.
        #[arg(long, allow_negative_numbers = true)]
        sx: f64,
        /// Vertical scale factor. Negative mirrors; zero is refused.
        #[arg(long, allow_negative_numbers = true)]
        sy: f64,
        /// Anchor x in points — the point that does NOT move. Typically the
        /// corner opposite the grip being dragged.
        #[arg(long, allow_negative_numbers = true)]
        anchor_x: f64,
        /// Anchor y in points. PDF user space has its origin at the
        /// bottom-left corner (§8.3.2.3), not the top.
        #[arg(long, allow_negative_numbers = true)]
        anchor_y: f64,
        /// Scale `/BS /W` with the geometry. Off by default — a line weight
        /// is a drafting convention, not a length in the scaled space.
        #[arg(long)]
        scale_stroke_width: bool,
        /// Leave `/RD` unscaled. By default rect differences DO scale.
        #[arg(long)]
        keep_rect_differences: bool,
        /// Proceed when carrying a foreign appearance would contradict the
        /// other options, accepting the distortion.
        #[arg(long)]
        allow_appearance_distortion: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Rotate a form-field widget** (Pass 177.0): write `/MK /R` and redraw
    /// the field's appearance in the rotated frame.
    ///
    /// COUNTERCLOCKWISE. The page's `/Rotate` is the CLOCKWISE one -- the two
    /// entries are word-for-word parallel in the standard and the direction
    /// word is the only difference between them (ISO 32000-1 12.5.6.19
    /// Table 189 against 7.7.3.3 Table 30).
    ///
    /// `--degrees` must be a multiple of 90. The standard sets no range, so
    /// `-90` and `450` are accepted and reduced into 0..360, and the result
    /// line says when that happened.
    ///
    /// The `/Rect` does NOT move. A rotated field turns its content inside the
    /// box you placed, so the appearance is redrawn into a width/height
    /// swapped box and stood upright by the appearance stream's own `/Matrix`.
    ///
    /// pdfcer can only redraw appearances it authored -- text and choice
    /// fields. For a push button's caption artwork, a signature, or a form
    /// built elsewhere, `/MK /R` is written, the picture is left alone, and
    /// the result line SAYS SO: a conforming PDF 2.0 reader ignores `/MK`
    /// entirely when an appearance stream is present, so the field will still
    /// look upright there.
    RotateWidget {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Which widget to rotate, numbered from 0 in the order `list-fields`
        /// reports the field's widgets. Rotation is a WIDGET property, so a
        /// field with several widgets can have each rotated differently.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Degrees counterclockwise. Must be a multiple of 90.
        #[arg(long, allow_negative_numbers = true)]
        degrees: i64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Make a push button actually do something** (Pass 182.0/183.0,
    /// ISO 32000-1 12.6.4 and 12.7.5): reset, submit, navigate, open a URL,
    /// or take the action away again.
    ///
    /// pdfcer used to author push buttons that were valid and INERT, and said
    /// so on every creation, because writing `/A` reaches launch actions,
    /// network submits and JavaScript. The operator moved that boundary
    /// twice on 2026-08-30: first "a reset button should actually reset",
    /// then "make the submit and other options that don't need javascript
    /// available for buttons with the safeguards like we had planned".
    ///
    /// WHAT A BUTTON CAN BE GIVEN
    ///
    /// `--reset` resets every field; `--reset-only A,B` just those;
    /// `--reset-except A,B` everything else. `--submit URL` sends form data
    /// to a URL the document names. `--goto-page N` jumps to a page in this
    /// document. `--named next-page` and friends are the four
    /// reader-predefined navigation actions. `--uri URL` opens a link.
    /// `--clear` removes whatever action was there, which is what you want
    /// when you open somebody else's form and want the button inert; the
    /// result line names what it removed, including a script pdfcer would
    /// never write back.
    ///
    /// AUTHORING A SUBMIT SENDS NOTHING. pdfcer has no network code and fires
    /// no trigger; this writes a declaration another program may honour. What
    /// you get instead is a computed statement of what that button WOULD send
    /// -- the full URL including port and path, the format, the method, the
    /// field count, and specifically the things you cannot see: hidden
    /// fields, password fields, file-select fields that carry a local file
    /// off the machine, whether the whole document goes, and whether the
    /// baseline FDF payload carries this document's own path. Acrobat's
    /// equivalent warning names the host only and says nothing at all about
    /// the payload.
    ///
    /// REFUSALS, ALL BEFORE ANYTHING IS WRITTEN. A field that is not a push
    /// button; a reset or submit target that does not exist; a relative or
    /// non-ASCII destination, because a relative one resolves differently in
    /// different readers and pdfcer will not author a button whose target it
    /// cannot state; and a flag combination the standard forbids with a
    /// `shall`. No host is ever refused -- destination policy is open, by
    /// operator ruling.
    ///
    /// JAVASCRIPT AND LAUNCH ARE NOT OFFERED, permanently. pdfcer recognises
    /// scripts and never runs or writes them; a launch action starts a
    /// program.
    SetButtonAction {
        /// Input PDF.
        input: PathBuf,
        /// The push button's fully-qualified name, as `list-fields` reports.
        #[arg(long)]
        name: String,
        /// Reset every field in the form.
        #[arg(long, conflicts_with_all = ["reset_only", "reset_except", "clear", "submit", "goto_page", "named", "uri", "hide", "show"])]
        reset: bool,
        /// Reset ONLY these fields (comma-separated fully-qualified names).
        #[arg(long, value_delimiter = ',', conflicts_with_all = ["reset", "reset_except", "clear", "submit", "goto_page", "named", "uri", "hide", "show"])]
        reset_only: Vec<String>,
        /// Reset everything EXCEPT these fields.
        #[arg(long, value_delimiter = ',', conflicts_with_all = ["reset", "reset_only", "clear", "submit", "goto_page", "named", "uri", "hide", "show"])]
        reset_except: Vec<String>,
        /// Send the form to this URL when the button is pressed.
        ///
        /// Must be absolute and ASCII. Any host, any scheme -- the operator
        /// set destination policy open. The result line states in full what
        /// this button would send.
        #[arg(long, conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "goto_page", "named", "uri", "hide", "show"])]
        submit: Option<String>,
        /// The submission format (ISO 32000-1 Table 237 bits 3, 6, 9).
        ///
        /// `fdf` is the baseline and is what a zero flag word means. `pdf`
        /// sends the ENTIRE document file and ignores field selection
        /// entirely -- there is no partial-PDF submission.
        #[arg(long, value_enum, default_value_t = SubmitFormatArg::Fdf)]
        submit_format: SubmitFormatArg,
        /// Submit ONLY these fields, and their descendants.
        #[arg(long, value_delimiter = ',', conflicts_with = "submit_except")]
        submit_only: Vec<String>,
        /// Submit everything EXCEPT these fields.
        #[arg(long, value_delimiter = ',')]
        submit_except: Vec<String>,
        /// Use HTTP GET instead of POST. HTML format only -- Table 237 bit 4
        /// `shall` be clear otherwise.
        #[arg(long)]
        submit_get: bool,
        /// Also send where the mouse was clicked. HTML format only.
        #[arg(long)]
        submit_coordinates: bool,
        /// Send empty fields too, by name only -- form structure, not data.
        #[arg(long)]
        include_no_value_fields: bool,
        /// Normalise date-looking values to the standard date format.
        #[arg(long)]
        canonical_dates: bool,
        /// Include every markup annotation in the document, whoever wrote
        /// them. FDF format only.
        #[arg(long)]
        include_annotations: bool,
        /// Narrow --include-annotations to the current user's, as judged by
        /// the receiving server. FDF format only, and requires it.
        #[arg(long)]
        only_current_user_annotations: bool,
        /// Also send every incremental update since the document was opened.
        /// THIS PERFORMS A SAVE FIRST, and ships signatures with it. FDF
        /// format only.
        #[arg(long)]
        include_incremental_updates: bool,
        /// Suppress this document's own file path from the payload. The one
        /// privacy-narrowing flag in the whole word. FDF format only.
        #[arg(long)]
        exclude_document_path: bool,
        /// Embed a copy of this whole PDF inside the submitted FDF. FDF
        /// format only.
        #[arg(long)]
        embed_form: bool,
        /// Jump to this page (0-based) in THIS document when pressed.
        #[arg(long, conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "submit", "named", "uri", "hide", "show"])]
        goto_page: Option<usize>,
        /// How the page is positioned on arrival.
        #[arg(long, value_enum, default_value_t = GotoViewArg::WholePage)]
        goto_view: GotoViewArg,
        /// Hide these fields (comma-separated fully-qualified names).
        ///
        /// Every widget of each named field, including ones on other pages.
        /// A grouping name is refused: the standard states no descendant rule
        /// for a hide action, so pdfcer will not guess which of the two
        /// readings you meant.
        #[arg(long, value_delimiter = ',', conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "submit", "goto_page", "named", "uri", "show"])]
        hide: Vec<String>,
        /// Show these fields — the same action with its flag cleared.
        ///
        /// A SEPARATE FLAG, not a modifier, because `/H`'s default is "hide":
        /// an action that omits the flag hides, so "show" has to be written
        /// out and is easy to lose.
        #[arg(long, value_delimiter = ',', conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "submit", "goto_page", "named", "uri", "hide"])]
        show: Vec<String>,
        /// One of the four reader-predefined navigation actions.
        #[arg(long, value_enum, conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "submit", "goto_page", "uri", "hide", "show"])]
        named: Option<NamedActionArg>,
        /// Open this URI. Authored as data -- pdfcer never follows one.
        #[arg(long, conflicts_with_all = ["reset", "reset_only", "reset_except", "clear", "submit", "goto_page", "named", "hide", "show"])]
        uri: Option<String>,
        /// Remove the button's action, leaving it inert.
        #[arg(long, conflicts_with_all = ["reset", "reset_only", "reset_except", "submit", "goto_page", "named", "uri", "hide", "show"])]
        clear: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Move a form field's widget** — translate its `/Rect` (§12.5.2).
    ///
    /// MOVES ONE APPEARANCE, NOT THE FIELD. A field can own widgets on
    /// several pages; this shifts the one you name and leaves its siblings
    /// where they are, reporting how many it left behind. That is the same
    /// widget-versus-field distinction `delete-widget` draws against
    /// `delete-field`.
    ///
    /// The artwork is NOT regenerated, and does not need to be: §12.5.5
    /// scales the appearance onto `/Rect` by the ratio of their extents, so
    /// a translation leaves both ratios at 1 and the existing picture is
    /// simply carried along at its original size.
    ///
    /// RESIZING IS A DIFFERENT OPERATION and is deliberately not this verb.
    /// Changing the extent makes those ratios ≠ 1, which §12.5.5 defines as
    /// a non-uniform stretch — a resized check box would get a distorted
    /// tick. A resize has to regenerate the appearance instead.
    MoveWidget {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Which widget to move, numbered from 0 in the order `list-fields`
        /// reports the field's widgets.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// Horizontal shift in points, positive to the right.
        #[arg(long, allow_negative_numbers = true)]
        dx: f64,
        /// Vertical shift in points, positive UP — PDF user space has its
        /// origin at the bottom-left corner (§8.3.2.3), not the top.
        #[arg(long, allow_negative_numbers = true)]
        dy: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Change an existing field's FIELD-SCOPE properties (`Pass 134.0`).
    ///
    /// Every property here lives on the field and is therefore shared by
    /// every widget the field owns -- change `--required` on a radio group
    /// and the whole group is required. The per-placement properties
    /// (position, border, visibility, caption) are `edit-widget`.
    ///
    /// A flag not passed is LEFT ALONE. There is no way to say "reset this
    /// to the default", because a default is not a thing a file records.
    ///
    /// The standard's gates are checked against the RESULT, not against
    /// what you typed: clearing `--max-len` on a comb field is refused even
    /// though the request never mentions comb, because Table 228 permits
    /// Comb only when /MaxLen is present.
    EditField {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// `/Ff` bit 2 -- the field must have a value when the form is
        /// submitted.
        #[arg(long)]
        required: Option<bool>,
        /// `/Ff` bit 1 -- the value may not be changed by the operator.
        #[arg(long)]
        read_only: Option<bool>,
        /// `/TU`, the accessibility name a screen reader announces INSTEAD
        /// of the field name. Pass an empty string to remove it.
        #[arg(long)]
        tooltip: Option<String>,
        /// `/Ff` bit 13 (text fields) -- accept multiple lines.
        #[arg(long)]
        multiline: Option<bool>,
        /// `/Ff` bit 14 (text fields) -- echo the value as bullets.
        #[arg(long)]
        password: Option<bool>,
        /// `/Ff` bit 25 (text fields) -- lay the value out in equal cells.
        /// Requires a `--max-len`, and refuses multiline or password.
        #[arg(long)]
        comb: Option<bool>,
        /// `/MaxLen` (text fields) -- the maximum character count. Pass 0 to
        /// REMOVE the limit.
        #[arg(long)]
        max_len: Option<i64>,
        /// `/Ff` bit 15 (radio groups) -- the selected button cannot be
        /// clicked off.
        #[arg(long)]
        no_toggle_to_off: Option<bool>,
        /// `/Ff` bit 26 (radio groups) -- radios sharing an on-state move
        /// together.
        #[arg(long)]
        radios_in_unison: Option<bool>,
        /// `/Ff` bit 18 (choice fields) -- a drop-down rather than a list.
        #[arg(long)]
        combo: Option<bool>,
        /// `/Ff` bit 19 (choice fields) -- the operator may type a value
        /// that is not in the list. Permitted only on a `--combo` field.
        #[arg(long)]
        editable: Option<bool>,
        /// `/Ff` bit 22 (choice fields) -- more than one option may be
        /// selected.
        #[arg(long)]
        multi_select: Option<bool>,
        /// `/Ff` bit 20 (choice fields) -- RECORD that the options were
        /// sorted. Sorts nothing: Table 230 makes this a claim about the
        /// writer, and says conforming readers "shall display the options in
        /// the order in which they occur".
        #[arg(long)]
        sort: Option<bool>,
        /// `/Q` — how the field's text is justified: 0 left, 1 centred,
        /// 2 right (ISO 32000-1 §12.7.3.3 Table 222).
        ///
        /// A value outside 0-2 is REFUSED, not clamped: Table 222 defines
        /// exactly three, and clamping 7 to 2 would silently right-align a
        /// field you meant to do something else with.
        ///
        /// The appearance is REDRAWN, not merely recorded -- justification is
        /// painted into the stream, so writing the key alone would change a
        /// number and no pixels.
        #[arg(long, value_name = "0|1|2")]
        quadding: Option<i64>,
        /// REMOVE `/Q`, so the field INHERITS a justification again.
        ///
        /// Not the same as `--quadding 0`: that STATES left, this says the
        /// field is silent and takes whatever its parent or the form's
        /// /AcroForm says (§12.7.3.2), falling back to left only when nothing
        /// above it states one. Under a parent carrying `/Q 1` this gives you
        /// CENTRED, not left.
        #[arg(long, conflicts_with = "quadding")]
        clear_quadding: bool,
        /// `/DV` — the default value `reset-form` restores.
        ///
        /// Until now `/DV` was readable and unwritable, so a reset could only
        /// restore defaults some OTHER application had authored: a form
        /// pdfcer built reset every field to empty whatever the author
        /// intended.
        #[arg(long)]
        default_value: Option<String>,
        /// REMOVE `/DV`, so a reset CLEARS this field instead of restoring a
        /// value.
        #[arg(long, conflicts_with = "default_value")]
        clear_default_value: bool,
        /// `Ff` bit 3 — NoExport: this field's value is NOT submitted by a
        /// SubmitForm action (§12.7.4.1 Table 226).
        ///
        /// Changes what a submit SENDS, not what the operator sees, so it is
        /// the kind of property you set once and cannot verify by looking at
        /// the page. `list-fields` prints the whole Ff word.
        #[arg(long)]
        no_export: Option<bool>,
        /// `Ff` bit 21 — FileSelect: the value is a FILE PATH to submit, not
        /// literal text.
        ///
        /// ⚠️ A file-select field is a submit hazard — a form carrying one can
        /// send a local file when activated.
        #[arg(long)]
        file_select: Option<bool>,
        /// `Ff` bit 23 — DoNotSpellCheck. Advisory to the reader; pdfcer
        /// neither spell-checks nor draws differently for it.
        #[arg(long)]
        no_spell_check: Option<bool>,
        /// `Ff` bit 24 — DoNotScroll: text overflowing the box is CLIPPED
        /// rather than scrolled. Worth setting on a box sized deliberately.
        #[arg(long)]
        no_scroll: Option<bool>,
        /// `Ff` bit 27 — CommitOnSelChange: a choice field commits the moment
        /// the selection changes, not on losing focus.
        #[arg(long)]
        commit_on_sel_change: Option<bool>,
        /// `/TM` — the mapping name used when EXPORTING form data, in place
        /// of the field's own name.
        ///
        /// Changes the exported payload without changing anything visible on
        /// the page, which is what makes it worth stating in the report.
        #[arg(long)]
        mapping_name: Option<String>,
        /// REMOVE `/TM`, so an export reverts to the field's own name.
        #[arg(long, conflicts_with = "mapping_name")]
        clear_mapping_name: bool,
        /// `/DA` font — the face the field's VALUE is drawn in. One of the
        /// standard 14: helvetica, helvetica-bold, helvetica-oblique,
        /// helvetica-bold-oblique, times, times-bold, times-italic,
        /// times-bold-italic, courier, courier-bold, courier-oblique,
        /// courier-bold-oblique, symbol, zapf-dingbats.
        ///
        /// pdfcer adds the font to the form's default resources, so the name
        /// always resolves. To use a face the document already embeds, pass
        /// its resource key with `--font-resource` instead.
        ///
        /// Requires `--font-size`. Changing any of the three redraws the
        /// field, so the text you see matches what the file says.
        #[arg(long, value_name = "NAME")]
        font: Option<String>,
        /// `/DA` font, named by a resource key ALREADY in the form's `/DR`
        /// `/Font` — an embedded face the document's own author put there.
        ///
        /// REFUSED if the key is not there, listing what is. pdfcer will not
        /// write a `/DA` naming a font that does not resolve: the field would
        /// draw in a substituted face, look normal, and be wrong.
        #[arg(long, value_name = "KEY", conflicts_with = "font")]
        font_resource: Option<String>,
        /// `/DA` size in points. `0` means AUTO — the reader fits the text to
        /// the box, which is what Acrobat calls Auto and what a new text
        /// field defaults to.
        #[arg(long, value_name = "PT")]
        font_size: Option<f64>,
        /// `/DA` text colour: one number for gray, three for RGB, four for
        /// CMYK, comma separated, each 0-1. Defaults to black.
        ///
        /// This is the colour of the VALUE's glyphs. The box's own fill and
        /// border are `edit-widget --background` and `--border-color`, a
        /// different dictionary entirely.
        #[arg(long, value_name = "G|R,G,B|C,M,Y,K")]
        font_color: Option<String>,
        /// Replace a choice field's option list. Repeatable, in order.
        /// `Label` for a plain option, or `export=Label` when the submitted
        /// value differs from what the operator sees.
        #[arg(long = "option", value_name = "OPTION")]
        options: Vec<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Change ONE widget's position, size, border, visibility or caption
    /// (`Pass 134.0`).
    ///
    /// A field may own widgets on several pages -- every radio group does,
    /// and so does any field placed twice. This changes one of them and
    /// reports how many it left alone.
    ///
    /// `--rect` REPLACES the rectangle, so it both moves and resizes.
    /// `move-widget` shifts by a delta and is the cheaper path when you only
    /// want to move: a translation keeps the baked appearance exact, whereas
    /// a changed width or height means the appearance must be rebuilt or a
    /// viewer will stretch it (§12.5.5).
    EditWidget {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name, as `list-fields` reports it.
        #[arg(long)]
        name: String,
        /// Which widget, numbered from 0 in the order `list-fields` reports
        /// the field's widgets.
        #[arg(long, default_value_t = 0)]
        index: usize,
        /// The new rectangle in points, `llx,lly,urx,ury`. PDF user space
        /// has its origin at the BOTTOM-left (§8.3.2.3).
        #[arg(long, value_name = "LLX,LLY,URX,URY")]
        rect: Option<String>,
        /// Border style: solid, dashed, beveled, inset or underline.
        #[arg(long)]
        border_style: Option<String>,
        /// Border width in points. Zero means no border, which Table 166
        /// states explicitly.
        #[arg(long)]
        border_width: Option<f64>,
        /// Where the widget is visible: screen-and-print, screen-only,
        /// print-only or hidden.
        #[arg(long)]
        visibility: Option<String>,
        /// `/MK` `/CA` -- the widget's caption. On a push button this is the
        /// only human-readable thing distinguishing Submit from Reset, since
        /// a push button has no value at all. Pass an empty string to remove.
        ///
        /// On a push button pdfcer drew, this also REDRAWS the plate: the
        /// caption is painted into the artwork, so writing the key alone
        /// would leave the button showing its previous word.
        #[arg(long)]
        caption: Option<String>,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, painted into
        /// the appearance. On a check box or radio button this is the fill
        /// behind the tick.
        ///
        /// Accepts one number for DeviceGray, three for DeviceRGB or four for
        /// DeviceCMYK, comma separated, each 0-1 -- CMYK is written as CMYK
        /// and never converted -- plus two words that are NOT the same thing:
        ///
        /// `none` writes Table 189's EMPTY ARRAY, which states *no colour*.
        /// `unset` REMOVES the key, so the file states nothing and the
        /// builder's own default stands. On a push button that difference is
        /// visible: `none` means no plate, `unset` brings the plate grey back.
        #[arg(long, value_name = "none|unset|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR. Same spelling as
        /// `--background`, `unset` included.
        ///
        /// Not `--border-style` or `--border-width`, which are `/BS`
        /// (Table 166) -- the border's style and width. Different
        /// dictionaries; a widget may carry either without the other.
        #[arg(long, value_name = "none|unset|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,

        /// Scale `/BS /W` with the geometry. Off by default -- a line weight
        /// is a drafting convention, not a length in the scaled space.
        ///
        /// Same three options `resize-annotation` takes, spelled identically.
        #[arg(long)]
        scale_stroke_width: bool,
        /// Leave `/RD` unscaled. By default rect differences DO scale. A
        /// widget rarely has an `/RD`; accepted so both verbs take the same
        /// answers.
        #[arg(long)]
        keep_rect_differences: bool,
        /// Proceed when the appearance cannot be rebuilt, accepting that a
        /// viewer will stretch it.
        ///
        /// Needed only for artwork pdfcer did not draw -- a foreign `/AP` on a
        /// button, or a signature field. A check box, radio button or push
        /// button pdfcer authored is REDRAWN at the new size and needs none of
        /// this.
        #[arg(long)]
        allow_appearance_distortion: bool,

        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Author a new push button (ISO 32000-1 §12.7.4.2.2).
    ///
    /// The button is created WITH NO ACTION and does nothing when clicked.
    /// What this makes is a valid, inert control, and that is stated on every
    /// run rather than left to be discovered.
    ///
    /// GIVING IT ONE IS A SEPARATE COMMAND -- `set-button-action`, which
    /// attaches a reset, a submit, page navigation or a URL. Creation
    /// deliberately does not: a button that gained behaviour as a side effect
    /// of being drawn is exactly what the inert default protects against.
    ///
    /// A push button has no value in any state (§12.7.4.2.2 — it "shall not
    /// use the V and DV entries"), so `fill-field` cannot target it and
    /// there is no `--required` flag: a field that can never hold a value
    /// cannot be required to hold one.
    AddPushButton {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name — also how `list-fields` refers
        /// to it. This is the SCRIPT-FACING identifier, not the label; the
        /// label is `--caption`.
        ///
        /// A PERIOD SEPARATES LEVELS (§12.7.3.2): `Form.Actions.Submit`
        /// creates the group `Form`, the group `Form.Actions`, and the field
        /// `Submit` inside it — reusing any of those that already exist. A
        /// name segment may not itself contain a period, so a leading,
        /// trailing or doubled one is refused rather than guessed at.
        ///
        /// REUSING AN EXISTING PUSH BUTTON'S NAME MERGES: a second widget is
        /// attached to the same field rather than a second field created —
        /// one button, two places to press it. Each widget keeps its OWN
        /// caption, because the caption is a widget property (/MK /CA); the
        /// second add therefore does not relabel the first. A different type
        /// under the same name is refused, and so is a name that belongs to
        /// a group.
        #[arg(long)]
        name: String,
        /// 1-based page number to place the button on.
        #[arg(long)]
        page: usize,
        /// The button rectangle in PDF user space, `llx,lly,urx,ury`.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// The text printed on the button (`/MK` `/CA`).
        ///
        /// Distinct from `--name` (the script identifier) and `--tooltip`
        /// (what a screen reader announces). Defaulting any of the three
        /// from another would put an identifier on a control a person reads,
        /// so none of them is derived from the others.
        ///
        /// An empty caption is allowed and produces a blank plate; it is
        /// reported, because a blank button and a forgotten `--caption` are
        /// the same bytes.
        #[arg(long, default_value = "")]
        caption: String,
        /// `/TU`, the accessibility name a screen reader announces.
        #[arg(long)]
        tooltip: Option<String>,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--no-tooltip` is required. Omitting
        /// both is an error, never a silent default. This bites harder on a
        /// push button than on any other type: its `/T` is usually a script
        /// identifier and its caption is usually a bare verb, so a
        /// screen-reader user with neither a tooltip nor a meaningful name
        /// has nothing at all to go on.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// Mark the button read-only (`/Ff` bit 1) — it renders but cannot
        /// be activated.
        #[arg(long)]
        read_only: bool,
        /// Pre-fill this button's properties from an existing push button.
        ///
        /// Copies the CAPTION and nothing else — it is the only non-boolean
        /// property a push button has. A template of any other type (or a
        /// captionless push button) contributes nothing and says so.
        ///
        /// An explicit `--caption` wins; this only fills a gap.
        #[arg(long, value_name = "FIELD")]
        defaults_from: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the add reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
        /// Border line style (§12.5.4 Table 166).
        #[arg(long, value_enum, default_value_t = BorderArg::Solid)]
        border: BorderArg,
        /// Border width in points. Zero means no border.
        #[arg(long, default_value_t = 1.0)]
        border_width: f64,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, PAINTED
        /// into the appearance at creation, not merely recorded.
        ///
        /// Accepts `none` (Table 189's empty array, which STATES no colour
        /// and is not the same as the key being absent), one number for
        /// DeviceGray, three for DeviceRGB or four for DeviceCMYK, comma
        /// separated, each 0-1. CMYK is written as CMYK, never converted.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR, painted into the
        /// appearance at creation. Same spelling as `--background`; `none`
        /// leaves a text or choice field with no frame.
        ///
        /// Not `--border` or `--border-width`, which are `/BS` (Table 166) --
        /// the border's style and width. Different dictionaries.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,
        /// Where the widget is visible (§12.5.3 Table 165).
        #[arg(long, value_enum, default_value_t = VisibilityArg::Visible)]
        visibility: VisibilityArg,
    },

    /// Author a new list box or drop-down (ISO 32000-1 §12.7.4.4).
    ///
    /// The field is created with its options and NO selection; `fill-field`
    /// puts the first value in.
    AddChoiceField {
        /// Input PDF.
        input: PathBuf,
        /// The field's fully-qualified name — also how `fill-field` and
        /// `list-fields` refer to it.
        ///
        /// A PERIOD SEPARATES LEVELS (§12.7.3.2): `Personal.Address.Zip`
        /// creates the group `Personal`, the group `Personal.Address`, and
        /// the field `Zip` inside it — reusing any of those that already
        /// exist. A name segment may not itself contain a period, so a
        /// leading, trailing or doubled one is refused rather than guessed at.
        ///
        /// REUSING AN EXISTING NAME OF THE SAME TYPE MERGES: a second widget
        /// is attached to the same field rather than a second field created.
        /// One value, two places to see and edit it — which is how a check box
        /// appears on every page of a form. A different type under the same
        /// name is refused, and so is a name that belongs to a group.
        #[arg(long)]
        name: String,
        /// 1-based page number to place the field on.
        #[arg(long)]
        page: usize,
        /// The field rectangle in PDF user space, `llx,lly,urx,ury`.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// One selectable option. Repeat for each.
        ///
        /// `LABEL` alone makes the exported value and the displayed label the
        /// same. `EXPORT=LABEL` splits them — the form submits `EXPORT` and
        /// the operator sees `LABEL`. That split is the whole point of a
        /// choice field's option list: `--option CA=Canada` submits `CA`.
        ///
        /// May be omitted: a choice field with no options is legal and
        /// saves, but cannot be filled until options are added, so creating
        /// one prints a warning rather than failing.
        #[arg(long = "option", value_name = "[EXPORT=]LABEL")]
        options: Vec<String>,
        /// Make this a drop-down (combo box) rather than a scrolling list.
        #[arg(long)]
        combo: bool,
        /// Allow typing a value that is not in the list. Combo boxes only
        /// (§12.7.4.4 Table 230).
        #[arg(long)]
        editable: bool,
        /// Allow more than one selection at a time (`/Ff` bit 22).
        #[arg(long)]
        multi_select: bool,
        /// Sort the options alphabetically by label.
        ///
        /// This REORDERS the written array, because §12.7.4.4 makes readers
        /// display `/Opt` order regardless of the sort flag.
        #[arg(long)]
        sort: bool,
        /// `/TU`, the accessibility name a screen reader announces.
        #[arg(long)]
        tooltip: Option<String>,
        /// Explicitly DECLINE an accessibility name (R105).
        ///
        /// Exactly one of `--tooltip` / `--no-tooltip` is required. Omitting
        /// both is an error, never a silent default: for a form field, `/TU`
        /// — not the tag tree — is what a screen reader announces, so a
        /// missing one is invisible to the person creating the field and
        /// load-bearing for the person who cannot see the form. Declining is
        /// a legitimate answer; it just has to be an ANSWER, and it is
        /// reported back in the operation's disclosures.
        #[arg(long, conflicts_with = "tooltip")]
        no_tooltip: bool,
        /// Mark the field read-only (`/Ff` bit 1).
        #[arg(long)]
        read_only: bool,
        /// Mark the field required at submit time (`/Ff` bit 2).
        #[arg(long)]
        required: bool,
        /// Pre-fill this field's properties from an existing field.
        ///
        /// Copies only NON-BOOLEAN, TYPE-MATCHED data — `--max-len` for a
        /// text field, the option list for a choice field, the on-state for
        /// a check box. A radio template copies nothing.
        ///
        /// Yes/no properties are never copied, and that is deliberate: these
        /// are presence flags, so a copied `--multiline` could be added but
        /// never turned off, and a single-line field could not be made from
        /// a multiline template. The accessibility name is never copied
        /// either — deciding it is the whole point of requiring
        /// `--tooltip`/`--no-tooltip`, and inheriting someone else's answer
        /// is not deciding.
        ///
        /// Anything given explicitly wins; this only fills gaps. When the
        /// template contributes nothing, it says so rather than silently
        /// doing nothing.
        #[arg(long, value_name = "FIELD")]
        defaults_from: Option<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the add reproduces the input byte for
        /// byte.
        #[arg(long)]
        verify_undo: bool,
        /// Border line style (§12.5.4 Table 166).
        #[arg(long, value_enum, default_value_t = BorderArg::Solid)]
        border: BorderArg,
        /// Border width in points. Zero means no border.
        #[arg(long, default_value_t = 1.0)]
        border_width: f64,
        /// `/MK` `/BG` -- the widget's BACKGROUND (fill) colour, PAINTED
        /// into the appearance at creation, not merely recorded.
        ///
        /// Accepts `none` (Table 189's empty array, which STATES no colour
        /// and is not the same as the key being absent), one number for
        /// DeviceGray, three for DeviceRGB or four for DeviceCMYK, comma
        /// separated, each 0-1. CMYK is written as CMYK, never converted.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        background: Option<String>,
        /// `/MK` `/BC` -- the widget's BORDER COLOUR, painted into the
        /// appearance at creation. Same spelling as `--background`; `none`
        /// leaves a text or choice field with no frame.
        ///
        /// Not `--border` or `--border-width`, which are `/BS` (Table 166) --
        /// the border's style and width. Different dictionaries.
        #[arg(long, value_name = "none|G|R,G,B|C,M,Y,K")]
        border_color: Option<String>,
        /// Where the widget is visible (§12.5.3 Table 165).
        #[arg(long, value_enum, default_value_t = VisibilityArg::Visible)]
        visibility: VisibilityArg,
    },

    /// Recompute recognised Acrobat calculation scripts natively, without
    /// executing any JavaScript (decision 009 posture B).
    ///
    /// **Shows the plan and changes nothing unless `--apply` is given.** A
    /// recomputed total is something pdfcer inferred from a script it did not
    /// run, so it is visible before it becomes document state (rule 4).
    ///
    /// Only exact-shape `AFSimple_Calculate` calls are recomputed. Anything
    /// else — author code, an edited built-in, a calculation naming a field
    /// this document does not contain — is left alone and reported.
    Recompute {
        /// Input PDF.
        input: PathBuf,
        /// Apply the plan and write `--output`. Without this, nothing is
        /// written and the plan is printed for review.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// How to read a comma in a stored value. A comma is ambiguous
        /// between a decimal point and a thousands separator, and pdfcer
        /// refuses to guess by default.
        #[arg(long, value_enum, default_value_t = CommaArg::NotNumeric)]
        comma: CommaArg,
        /// Also verify that undoing the recompute reproduces the input file
        /// byte for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Reset form fields to their default values and save (§12.7.5.3).
    ///
    /// Sets each field's `/V` to its `/DV`, and **removes `/V` entirely**
    /// where there is no `/DV` — both halves are `shall` in the clause, and
    /// removal is not the same as blanking.
    ///
    /// Pushbuttons, signature fields and read-only fields are left alone and
    /// counted. **Destructive**: this discards typed answers, so it prints
    /// what it will clear unless `--apply` is given.
    ResetForm {
        /// Input PDF.
        input: PathBuf,
        /// Reset only this field, by fully-qualified name. Repeatable.
        /// Omit to reset every eligible field.
        #[arg(long = "field", value_name = "NAME")]
        fields: Vec<String>,
        /// Perform the reset and write `--output`. Without this, nothing is
        /// written and the fields that would be cleared are listed.
        #[arg(long)]
        apply: bool,
        /// Output path. Required with `--apply`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the reset reproduces the input file byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },

    /// List every form-field script, classified (decision 009 posture B).
    ///
    /// One stable line per script: which field, which `/AA` trigger, what
    /// pdfcer recognised it as, and whether pdfcer can natively reproduce its
    /// effect. **pdfcer never executes any of it** — a recognised built-in is
    /// *read*, never run, and everything else is disclosed as unrun.
    ///
    /// Lines are locale-invariant and ordered by the field tree, so the
    /// output diffs cleanly between two revisions of the same form.
    ListScripts {
        /// Input PDF.
        input: PathBuf,
        /// Show only the scripts pdfcer can natively reproduce.
        #[arg(long)]
        reproducible_only: bool,
    },

    /// Fill one or more interactive-form fields and save (Pass 7).
    ///
    /// Each `--set NAME=VALUE` sets a field by fully-qualified name: a text
    /// or choice field's value is set and its appearance regenerated
    /// (§12.7.3.3); a check-box/radio field's state is selected (VALUE is
    /// the on-state name, e.g. `Yes`, or `Off`/`on`/`true`/`1`). Saves
    /// incrementally by default (the minimal-diff path). Never flattens —
    /// the fields stay interactive.
    FillField {
        /// Input PDF.
        input: PathBuf,
        /// A field assignment `NAME=VALUE`. Repeatable.
        #[arg(long = "set", value_name = "NAME=VALUE", required = true)]
        sets: Vec<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the fill reproduces the input file byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
        /// Allow filling a RICH-TEXT field by converting it to a plain text
        /// field, discarding the stored formatting.
        ///
        /// Without this, a rich-text field is refused: writing /V while a
        /// live /RV remains would make conforming readers regenerate the
        /// appearance from the OLD text (§12.7.3.4), so the file would
        /// display something the operator never typed. Refusing is right,
        /// but it leaves the field unfillable, and this flag is the
        /// deliberate way through.
        ///
        /// It is LOSSY and irreversible within the fill: the /RV is removed
        /// and the RichText flag cleared, so bold, colour and every other
        /// span property in the stored rich text is gone. Every converted
        /// field is named individually on stderr — a count would not tell
        /// the operator WHICH field lost its formatting.
        #[arg(long)]
        downgrade_rich_text: bool,
    },

    /// Regenerate widget appearances and clear /NeedAppearances (Pass 7.1).
    ///
    /// For every text/choice field that has no baked appearance — or, when
    /// the document sets /NeedAppearances, every such field — the widget
    /// appearance is rebuilt from the field's stored value (§12.7.3.3), and
    /// the /NeedAppearances flag is removed so pdfcer never emits a stale
    /// "appearances need regenerating" assertion on a file it just fixed
    /// (R51). Buttons are untouched (state selections, not generated).
    RegenerateAppearances {
        /// Input PDF.
        input: PathBuf,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Flatten interactive form fields into page content (Pass 7.1).
    ///
    /// DESTRUCTIVE. Each field's appearance is burned into its page's
    /// content stream and the field is removed from /AcroForm and /Annots —
    /// the fields stop being interactive. Under the default incremental save
    /// the pre-flatten values remain recoverable in the prior revision;
    /// `--full-rewrite` writes a single revision that removes even that
    /// (R48). Refused on a certified document (flatten is structural).
    Flatten {
        /// Input PDF.
        input: PathBuf,
        /// Only flatten these fully-qualified field names (repeatable).
        /// Omit to flatten every field.
        #[arg(long = "field", value_name = "NAME")]
        fields: Vec<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Write a single-revision full rewrite that physically removes the
        /// pre-flatten field data (R48), instead of the default incremental
        /// save which leaves it recoverable in the prior revision.
        #[arg(long)]
        full_rewrite: bool,
    },

    /// Export a filled form's field data to FDF or XFDF (Pass 7.1).
    ///
    /// Read-only: writes the document's present field values to a standalone
    /// data file. FDF is a PDF-like data file; XFDF is its XML form. The
    /// source PDF path is embedded as a hint so a reader knows the data's
    /// origin.
    ExportData {
        /// Input PDF (read, never modified).
        input: PathBuf,
        /// Output data file.
        #[arg(short, long)]
        output: PathBuf,
        /// Data format to write.
        #[arg(long, value_enum, default_value_t = DataFormat::Fdf)]
        format: DataFormat,
    },

    /// Import form-field data from an FDF or XFDF file (Pass 7.1).
    ///
    /// Sets each named field's value (dispatched by the target field's type)
    /// and regenerates its appearance, then saves. A named field the
    /// document does not have is counted and skipped, never an error. The
    /// data format is detected from the file's content.
    ImportData {
        /// Input PDF.
        input: PathBuf,
        /// The FDF/XFDF data file to import.
        #[arg(long)]
        data: PathBuf,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// Save a PDF and verify the round-trip invariant (ARCHITECTURE.md §5).
    ///
    /// Loads the document, saves it in the chosen mode, and checks the
    /// result: byte identity where the mode promises it, reloadability
    /// always, and — unless `--no-raster` is given — that page 1
    /// re-renders to an identical raster. Exits 0 only if every check
    /// the mode promises passed; see the exit-code table in
    /// `pdfcer --help` and the module documentation.
    RoundTrip {
        /// Input PDF.
        input: PathBuf,
        /// Which save path to exercise.
        #[arg(long, value_enum, default_value_t = RoundTripMode::Incremental)]
        mode: RoundTripMode,
        /// Write the produced file here. Omit to verify in memory only —
        /// which is what a corpus sweep wants, since it never needs the
        /// bytes it just checked.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// `/Producer` handling for `--mode full` (ignored otherwise:
        /// incremental save never touches `/Info`).
        ///
        /// `preserve` is the default here, unlike the pdfcer-core API
        /// default, because this subcommand's job is verification and a
        /// stamped `/Producer` is a deliberate byte change that would
        /// make the per-object identity check fail for one object by
        /// design. Ask for `set` explicitly when you want authorship.
        #[arg(long, value_enum, default_value_t = ProducerArg::Preserve)]
        producer: ProducerArg,
        /// Device pixels per PDF user-space unit for the raster oracle
        /// (`scale = dpi / 72`), matching `render-page --scale`.
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
        /// Skip the raster comparison. Faster, and the only option for
        /// a document whose page 1 does not render at all.
        #[arg(long)]
        no_raster: bool,
    },

    /// Edit a page's own text in place (Pass 14.1): re-encode a run + relayout.
    ///
    /// Locates `--find` on `--page` — inside one show operator, or (`Pass 256.0`) across CONSECUTIVE show operators that share font resource, size and baseline, the shape a producer writes when it emits one glyph per operator, re-encodes
    /// `--replace` in that run's OWN font encoding (inverting /Encoding, never
    /// /ToUnicode which is one-way and lossy, ISO 32000-1 §9.6.6), preserves
    /// the §9.4.4 advance so un-edited text stays put, relayouts the edited
    /// line (reflow by default; the line may overflow the original margin,
    /// which is disclosed), and saves INCREMENTALLY. The prior text survives
    /// in the document's revision history by design (disclosed) -- to truly
    /// remove text, use `redact-apply` (a distinct, security operation). A
    /// character the run's font cannot provide is REFUSED by name (the
    /// font-on-edit gate); an embedded SUBSET refuses a glyph it does not
    /// already carry. `--font-dir` supplies non-embedded faces (decision 012).
    EditText {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to edit.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Text to find within a single run on the page.
        ///
        /// May be **empty** when `--pin-span` is given, which means *"the
        /// whole pinned show operator"* — see that flag. Empty WITHOUT a pin
        /// is refused, so forgetting the pin cannot silently restyle an
        /// operator pdfcer chose.
        #[arg(long, default_value = "")]
        find: String,
        /// Pin the target show operator by its **byte span**, as
        /// `START:LEN`, instead of (or as well as) searching for text.
        ///
        /// Get the numbers from `extract-text --json --spans`, whose glyphs
        /// carry `op_start` / `op_len` / `stream`. The span indexes the
        /// **decoded** content buffer named by `stream` — a page's
        /// `/Contents` are concatenated into one buffer and every form
        /// XObject is a separate one, so a form's span pinned against the
        /// page finds nothing.
        ///
        /// # Why pin at all
        ///
        /// Because `--find` describes a target that may already be known.
        /// A caller holding an operator had to hand back a string for pdfcer
        /// to search for inside that very operator, and rebuilding that
        /// string is where it goes wrong: `/ToUnicode` may map ONE glyph to
        /// SEVERAL characters (§9.10.3 — an `ffl` ligature is one glyph and
        /// three characters), so a string rebuilt from extracted text need
        /// not match the operator's own decoded text. That failure is
        /// invisible on unligatured test text and routine on real typeset
        /// copy.
        ///
        /// With a pin and an empty `--find`, the whole operator is the
        /// target and nothing has to be described. The report discloses the
        /// extent it took.
        #[arg(long = "pin-span", value_name = "START:LEN")]
        pin_span: Option<String>,
        /// Let `--find` BEGIN at `--pin-span` and run on across the following
        /// operators, instead of having to lie inside the pinned one.
        ///
        /// This is the flag for text that **repeats on the page**. `--find`
        /// alone lets pdfcer pick an occurrence — and not necessarily the
        /// first: it prefers a match inside a SINGLE operator anywhere on the
        /// page over one that spans operators above it, so a spanning run can
        /// be unreachable by `--find` alone. `--pin-span` alone confines the
        /// match to one operator, and a producer that emits one glyph per
        /// operator will not have the whole run in any single one. Together,
        /// with this flag, they say *"this occurrence, and keep going"*.
        ///
        /// Refused unless `--pin-span` is given, since it has nothing to
        /// start from otherwise.
        #[arg(long = "span-from-pin", requires = "pin_span")]
        span_from_pin: bool,
        /// Replacement text (re-encoded into the run's font).
        #[arg(long)]
        replace: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Pin survivors with a compensating TJ instead of reflowing the line.
        #[arg(long)]
        pin: bool,
        /// Operator-supplied font folder for non-embedded runs (decision 012).
        /// Repeatable.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Which content stream to edit (Pass 119.0): `auto` (default -- the
        /// page's own content first, then each form XObject it paints, in
        /// paint order), `page` (the page's own content ONLY), or `form:N`
        /// (that form XObject's stream, by object number).
        ///
        /// On a CAD-exported drawing nearly every label lives inside a form
        /// XObject, so `auto` is what finds them; `page` reproduces the
        /// pre-119.0 reach for a batch script that wants a hard failure rather
        /// than a widened search.
        #[arg(
            long = "target",
            value_name = "auto|page|form:N",
            default_value = "auto"
        )]
        target: String,
    },

    /// Format a page's own text in place (Pass 14.2): size, colour, font family.
    ///
    /// Locates `--find` on `--page` — inside one show operator, or (`Pass 256.0`) across CONSECUTIVE show operators that share font resource, size and baseline, the shape a producer writes when it emits one glyph per operator and applies any
    /// combination of three formatting changes to that run, reusing
    /// `edit-text`'s advance-preserving surgery (only the changed text-state
    /// operators differ), then saves INCREMENTALLY (the prior state survives in history
    /// by design; to truly remove content use `redact-apply`):
    ///
    /// - `--set-size N` changes ONLY the `Tf` size operand (never the colour
    ///   operator). Size never needs new glyphs, so it always works on
    ///   existing text; the line is relaid out (reflow by default, `--pin` to
    ///   pin the tail). Arbitrary point values and multi-size flattening are
    ///   pdfcer's own documented choices (Acrobat behaviour unconfirmed).
    /// - `--set-color MODEL:C,…` sets the fill colour and STORES THE CHOSEN
    ///   SPACE (`rgb:` -> `rg`, `cmyk:` -> `k`, `gray:` -> `g`) — pdfcer does
    ///   NOT force-convert to DeviceRGB the way Acrobat does. A run originally
    ///   painted in a non-device space is DISCLOSED as a narrowing conversion.
    /// - `--set-font NAME` swaps to an existing Bold/Italic (or any) font
    ///   RESOURCE (by resource key or `/BaseFont`), re-encoding the run into
    ///   that face. It is gated on COVERAGE: a target that cannot show every
    ///   character in the run is REFUSED by name with nothing applied (never
    ///   `.notdef`, never a silent substitution). A successful change never
    ///   embeds a font. An outlined/vector run has no font to swap and is
    ///   refused. `--font-dir` supplies non-embedded faces (decision 012).
    ///
    /// Three direct text-state controls follow, each emitted for the
    /// matched run ONLY and explicitly restored to the run's ambient value
    /// immediately after it (text state persists for the whole content stream
    /// per ISO 32000-1 §9.3, and `q`/`Q` are illegal inside a text object per
    /// §8.2 Table 51, so the scope is closed by restoring BY VALUE):
    ///
    /// - `--char-spacing V` sets character spacing `Tc` (§9.3.2). Accepts
    ///   `0.5` or `0.5pt` (ABSOLUTE — unscaled text-space units, written as
    ///   typed at any size) and `20em` (RELATIVE — 20 THOUSANDTHS of an em,
    ///   the typographic tracking unit, NOT 20 ems), which is re-derived
    ///   against the run's size so a later resize stays correct.
    /// - `--h-scale PCT` sets horizontal scaling `Tz` (§9.3.4) as a percentage
    ///   of normal width; 100 is normal. It stretches the glyphs themselves,
    ///   not just the gaps, and also scales the spacing parameters.
    /// - `--superscript` / `--subscript` / `--no-script` set the baseline via
    ///   `Ts` (§9.3.7) plus a reduced `Tf` size. The size and rise ratios are
    ///   pdfcer's OWN documented defaults, NOT a parity claim (Acrobat's are
    ///   undocumented), and are printed by value in the report.
    ///
    /// Word spacing completes the family, and behaves differently from every
    /// flag above in two ways worth knowing BEFORE reaching for it:
    ///
    /// - `--word-spacing V` sets word spacing `Tw` (§9.3.3), same
    ///   `pt`/`em` unit grammar as `--char-spacing`. It applies to EVERY
    ///   occurrence of the single-byte character code 32 in the matched run —
    ///   leading spaces, trailing spaces and both halves of a doubled space
    ///   included. PDF has no per-gap word spacing; per-gap control is what
    ///   `TJ` numeric adjustments do, which is why `reflow --align justified`
    ///   distributes slack as `TJ` and not as `Tw`. The report prints how many
    ///   spaces were affected, including zero.
    /// - It is REFUSED, by name and with nothing applied, on a COMPOSITE
    ///   (Type 0 / CIDFont) run: §9.3.3 states word spacing "shall not apply
    ///   to occurrences of the byte value 32 in multiple-byte codes", so a
    ///   `Tw` there would be written into the file and do nothing. Use
    ///   `reflow` to redistribute inter-word space on a composite run.
    /// - `Tw` is multiplied by horizontal scaling (§9.4.4), so under a
    ///   `--h-scale 50` the visible gap is half the number given; the
    ///   disclosure quotes the effective value.
    ///
    /// If the run's ambient value for a parameter cannot be restored — it was
    /// inherited from outside the edited content stream — the edit is REFUSED
    /// by name with nothing applied, rather than guessing a default that would
    /// silently change content pdfcer did not touch. Changing a run's width
    /// inside a JUSTIFIED line invalidates that line's slack; pdfcer discloses
    /// that and offers re-justification instead of leaving it wrong.
    ///
    /// A formatting change inside a tagged (accessible) run PRESERVES its
    /// BDC/EMC+MCID wrapper and discloses that the structure tree went stale —
    /// pdfcer does not reproduce Acrobat's tag-corruption defect (R72).
    FormatText {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to format.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Text to find within a single run on the page.
        ///
        /// May be **empty** when `--pin-span` is given, which means *"the
        /// whole pinned show operator"* — see that flag. Empty WITHOUT a pin
        /// is refused, so forgetting the pin cannot silently restyle an
        /// operator pdfcer chose.
        #[arg(long, default_value = "")]
        find: String,
        /// Pin the target show operator by its **byte span**, as
        /// `START:LEN`, instead of (or as well as) searching for text.
        ///
        /// Get the numbers from `extract-text --json --spans`, whose glyphs
        /// carry `op_start` / `op_len` / `stream`. The span indexes the
        /// **decoded** content buffer named by `stream` — a page's
        /// `/Contents` are concatenated into one buffer and every form
        /// XObject is a separate one, so a form's span pinned against the
        /// page finds nothing.
        ///
        /// # Why pin at all
        ///
        /// Because `--find` describes a target that may already be known.
        /// A caller holding an operator had to hand back a string for pdfcer
        /// to search for inside that very operator, and rebuilding that
        /// string is where it goes wrong: `/ToUnicode` may map ONE glyph to
        /// SEVERAL characters (§9.10.3 — an `ffl` ligature is one glyph and
        /// three characters), so a string rebuilt from extracted text need
        /// not match the operator's own decoded text. That failure is
        /// invisible on unligatured test text and routine on real typeset
        /// copy.
        ///
        /// With a pin and an empty `--find`, the whole operator is the
        /// target and nothing has to be described. The report discloses the
        /// extent it took.
        #[arg(long = "pin-span", value_name = "START:LEN")]
        pin_span: Option<String>,
        /// New font size in points (changes only the `Tf` size operand).
        #[arg(long)]
        set_size: Option<f64>,
        /// New fill colour as `MODEL:comps`, comma-separated components in
        /// `0..=1`: `rgb:1,0,0` (red), `cmyk:0,1,1,0`, `gray:0.5`. The chosen
        /// device space is STORED (never force-converted to DeviceRGB).
        #[arg(long, value_name = "MODEL:C,..")]
        set_color: Option<String>,
        /// New font family/style: an existing page font resource, named by
        /// its resource key (`F2`) or its `/BaseFont` (`Times-Bold`).
        #[arg(long, value_name = "NAME")]
        set_font: Option<String>,
        /// Character spacing `Tc` (§9.3.2) for the matched run. `0.5` or
        /// `0.5pt` is ABSOLUTE (unscaled text-space units); `20em` is
        /// RELATIVE and means 20 THOUSANDTHS of an em (the tracking unit) —
        /// not 20 ems — and is re-derived if the run is later resized.
        #[arg(long = "char-spacing", value_name = "V[pt|em]")]
        char_spacing: Option<String>,
        /// Word spacing `Tw` (§9.3.3) for the matched run — the final FF-H
        /// control. Same unit grammar as `--char-spacing`: `2` or `2pt` is
        /// ABSOLUTE (unscaled text-space units); `200em` is RELATIVE and
        /// means 200 THOUSANDTHS of an em, re-derived if the run is later
        /// resized. Applies to EVERY single-byte code 32 in the run —
        /// leading, trailing and doubled spaces included; there is no
        /// per-gap word spacing in PDF. REFUSED by name on a composite
        /// (Type 0 / CIDFont) run, where §9.3.3 makes it void.
        #[arg(long = "word-spacing", value_name = "V[pt|em]")]
        word_spacing: Option<String>,
        /// Horizontal scaling `Tz` (§9.3.4) for the matched run, as a
        /// percentage of normal glyph width. 100 is normal; must be > 0.
        #[arg(long = "h-scale", value_name = "PCT")]
        h_scale: Option<f64>,
        /// Text rendering mode `Tr` (§9.3.6) for the matched run, 0 to 7:
        /// 0 fill, 1 stroke, 2 fill then stroke, 3 invisible, 4 to 7 the
        /// same plus clip. 3 keeps an OCR correction invisible over the
        /// scan. Refused with --bold-synthetic, which is itself mode 2.
        #[arg(long = "render-mode", value_name = "0-7")]
        render_mode: Option<u8>,
        /// Raise the matched run to superscript (`Ts` rise + reduced size).
        #[arg(long, conflicts_with_all = ["subscript", "no_script"])]
        superscript: bool,
        /// Lower the matched run to subscript (`Ts` drop + reduced size).
        #[arg(long, conflicts_with = "no_script")]
        subscript: bool,
        /// Reset the matched run to the baseline (`0 Ts`, size unchanged) —
        /// how an inherited non-zero rise is flattened for one run.
        #[arg(long = "no-script")]
        no_script: bool,
        /// Free-form baseline rise `Ts` (§9.3.7) for the matched run — pdfcer's
        /// deliberate EXCEED over Acrobat, which exposes only the coarse
        /// superscript/subscript toggle. `3.25` or `3.25pt` is ABSOLUTE
        /// (unscaled text-space units, written exactly as typed); `280em` is
        /// RELATIVE and means 280 THOUSANDTHS of an em, re-derived if the run
        /// is later resized. Positive raises the baseline. A rise moves the
        /// run WITHOUT changing its advance, so nothing after it shifts.
        /// Conflicts with the script toggles: both write `Ts`.
        #[arg(
            long,
            value_name = "V[pt|em]",
            conflicts_with_all = ["superscript", "subscript", "no_script"]
        )]
        rise: Option<String>,
        /// Apply SYNTHETIC bold: text rendering mode 2 (fill-then-stroke)
        /// with a user-space stroke width and the stroking colour matched to
        /// the fill (§9.3.6). A synthesised weight is the regular letterforms
        /// thickened, not a real typeface. Nothing is synthesised without
        /// this flag.
        ///
        /// WHAT HAPPENS IF A REAL BOLD FACE IS AVAILABLE depends on
        /// `--style-policy` (or the stored `style_policy` setting): `auto`
        /// applies the synthesis and NAMES the face it passed over, `warn`
        /// does the same and warns, `refuse` stops and points at that face.
        /// `refuse` is what this command always did before that became a
        /// choice.
        ///
        /// "Available" means a face already present as a resource ON THIS
        /// PAGE that `--set-font` WOULD ACCEPT for THIS RUN's characters —
        /// pdfcer checks before it recommends and quotes the exact
        /// `--set-font` argument. If no face of the run's own family can show
        /// the run, a usable face from ANOTHER family is offered and the
        /// message says so outright.
        ///
        /// It does NOT include the standard-14 bold names
        /// (`Helvetica-Bold`, `Times-Bold`, `Courier-Bold`), which
        /// `--set-font` can bind with no embedding at all. So "no real bold
        /// face is available" is a statement about this page's resources, not
        /// about what pdfcer can do. Ask `font-preflight` first.
        #[arg(long = "bold-synthetic")]
        bold_synthetic: bool,
        /// Make the run BOLD and let pdfcer choose how (`Pass 179.0`): a real
        /// bold face already on the page if one can show the text, else the
        /// standard-14 bold sibling of the run's own family (`Helvetica` →
        /// `Helvetica-Bold`, nothing embedded), else the synthetic stroke —
        /// the last step subject to `--style-policy` (`refuse` stops there
        /// and names `--bold-synthetic` as the explicit override). The rung
        /// taken is printed. Not with `--set-font` (name the styled face
        /// directly) or `--bold-synthetic`.
        #[arg(long, conflicts_with_all = ["set_font", "bold_synthetic"])]
        bold: bool,
        /// Make the run ITALIC the same automatic way (`--bold` describes
        /// the ladder); the two combine, per axis — a real Bold may bind
        /// while Italic is synthesised in the same operation.
        #[arg(long, conflicts_with_all = ["set_font", "italic_synthetic"])]
        italic: bool,
        /// Apply SYNTHETIC italic: a 12-degree oblique shear premultiplied
        /// into the run's text matrix. Same `--style-policy` handling as
        /// `--bold-synthetic`. REFUSED when a Td/TD/T* next-line operator
        /// follows the run inside the same text object (the injected matrix
        /// would displace that line), and refused with `--pin`.
        #[arg(long = "italic-synthetic")]
        italic_synthetic: bool,
        /// What to do when a bold/italic request needs a FALLBACK, for this
        /// run only. Omit to use the stored `style_policy` setting.
        ///
        /// Bold is not a switch in a PDF -- it is a different typeface -- so
        /// pdfcer prefers a real face and thickens the letters only when it
        /// cannot find one. That preference never changes; this decides only
        /// what pdfcer does about having fallen back.
        ///
        /// `auto` just does it and reports which it used, and is the default.
        /// `warn` says so loudly when the weight was faked. `refuse` stops an
        /// explicit `--bold-synthetic` when a real face was available and
        /// names that face, which is what pdfcer always did before this became
        /// a choice.
        ///
        /// Naming a font with `--set-font`, or forcing the fake one with
        /// `--bold-synthetic`, always works and is not affected by this.
        #[arg(long, value_enum)]
        style_policy: Option<StylePolicyArg>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Pin survivors with a compensating TJ instead of reflowing the line
        /// (only affects a size/font change; colour never shifts the line).
        #[arg(long)]
        pin: bool,
        /// Operator-supplied font folder for non-embedded runs (decision 012).
        /// Repeatable.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// Which content stream to format (Pass 119.2): `auto` (default -- the
        /// page's own content first, then each form XObject it paints, in paint
        /// order), `page` (the page's own content ONLY), or `form:N`.
        ///
        /// Same selector `edit-text --target` takes, and for the same reason:
        /// on a CAD-exported drawing the text worth restyling lives inside a
        /// form XObject. `pdfcer inspect --forms` lists them.
        #[arg(
            long = "target",
            value_name = "auto|page|form:N",
            default_value = "auto"
        )]
        target: String,
    },

    /// Re-wrap (reflow) a recognized paragraph in place (Pass 15.1).
    ///
    /// Applies an EXPLICIT within-block reflow: the recognized paragraph
    /// `--block` on `--page` is greedily re-wrapped to `--width` (default: the
    /// block's own detected box width) at `--align` (default: auto-detected
    /// from the block's glyph x-positions and preserved — left/right/center/
    /// justify) and `--leading` (default: the block's measured baseline gap),
    /// and ONLY that block's own content-stream object is re-emitted at the
    /// new per-line origins/breaks. JUSTIFIED full lines distribute their
    /// slack as per-gap `TJ` numbers (ISO 32000-1 §9.4.3); the last line of a
    /// paragraph is never stretched. The save is INCREMENTAL — the prior text
    /// survives in the document's revision history by design (disclosed); to
    /// truly remove text use `redact-apply` (a distinct, security operation).
    ///
    /// Preview first with `inspect --reflow-preview` (Pass 15.0, read-only).
    /// A reflow that grows the block past the page bottom EMITS the off-page
    /// content at its true position and DISCLOSES the overflow — it never
    /// silently clips or drops content (R76). A composite (Type0/CJK) block, a
    /// rotated/skewed block, or a block sharing a text object with other
    /// content is REFUSED by name (a clean, named non-zero exit — never a
    /// crash). A tagged block's BDC/EMC+MCID wrapper is preserved and its
    /// stale /ActualText disclosed (R72).
    Reflow {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number holding the block to reflow.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// 0-based index of the recognized block (paragraph) to reflow.
        #[arg(long, default_value_t = 0)]
        block: usize,
        /// Wrap width in points (default: the block's detected box width).
        #[arg(long)]
        width: Option<f64>,
        /// Alignment override: `left`, `right`, `center`, or `justified`
        /// (default: auto-detected from glyph x-positions and preserved).
        #[arg(long)]
        align: Option<String>,
        /// Leading (baseline-to-baseline) in points (default: the block's
        /// measured baseline gap).
        #[arg(long)]
        leading: Option<f64>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Add NEW text as real page content (FF-D).
    ///
    /// Two modes: **point** (`--at "x,y"`) shows the whole `--text` as one
    /// un-wrapped line; **boxed** (`--box "x,y,w,h"`) wraps `--text` to the
    /// box width via the same greedy breaker `reflow` uses, laid out top-anchored from
    /// the box top with `--align` (left|center|right|justify). Exactly one of
    /// `--at`/`--box` is required. Either mode APPENDS a fresh `BT…ET` run
    /// (default user space, §9.4.4) as a new content stream in the page
    /// `/Contents` array (ISO 32000-1 §7.7.3.3) — every ORIGINAL
    /// content stream stays byte-identical (R32/R46); only the page dict's
    /// `/Contents` reference, one new stream, and one new `/Font` entry change.
    /// The run defaults to a bundled Standard-14 face (`--font`, default
    /// Helvetica), written by name+code with NO embedding (R79 / §9.6.2.2) — so
    /// it is decision 014's most-editable font case and never hits the
    /// embedded-subset wall. A character the chosen face cannot represent is
    /// REFUSED by name (the F-refuse gate, R71) — a clean, named non-zero exit,
    /// never a crash or a faked glyph.
    ///
    /// This is genuine page content, NOT a `/FreeText` annotation (R78): the
    /// added run is thereafter editable with `edit-text`, formattable with
    /// `format-text`, and reflowable with `reflow`, exactly like the page's own
    /// text. The save is INCREMENTAL. On a TAGGED page the new run is untagged
    /// and that is disclosed (R73 — no structure element is fabricated). If the
    /// page inherited its `/Resources`, pdfcer gives it its own (referencing the
    /// same shared sub-dictionaries) rather than mutating the shared ancestor
    /// (§7.7.3.4) — also disclosed. `--font-dir` registers an operator-supplied
    /// face so the disclosed provenance is `Supplied` (shapes only; the written
    /// dict is identical — decision 012).
    AddText {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to add text to.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// POINT mode (16.0): origin `x,y` in points (default user space) — the
        /// run's absolute text-matrix translation (§9.4.2), e.g. `72,700`. The
        /// whole `--text` is one un-wrapped line. Mutually exclusive with
        /// `--box`; exactly one of `--at`/`--box` is required.
        #[arg(long, value_name = "X,Y")]
        at: Option<String>,
        /// BOXED mode (16.1): wrap `--text` to the rectangle `x,y,w,h` (points;
        /// `x,y` = lower-left corner), laid out top-anchored from the box top
        /// via the shipped 15.x greedy breaker. Multi-line, honours `--align`.
        /// Mutually exclusive with `--at`. Text taller than the box, or growing
        /// past the page, is DISCLOSED and still emitted in full (R76).
        #[arg(long = "box", value_name = "X,Y,W,H")]
        wrap_box: Option<String>,
        /// BOXED mode alignment: `left` (default) | `center` | `right` |
        /// `justify`. A fresh box has no glyphs to auto-detect from, so
        /// alignment is an explicit choice (justify distributes inter-word
        /// slack; the last line of each paragraph is left un-stretched).
        #[arg(long, value_name = "MODE")]
        align: Option<String>,
        /// BOXED mode leading (baseline-to-baseline, points). Omitted = the
        /// derived default `1.2 x size` (disclosed).
        #[arg(long, value_name = "PT")]
        leading: Option<f64>,
        /// The text to add. In BOXED mode a `\n` (literal newline in the
        /// argument) forces a hard line break; runs of spaces collapse.
        #[arg(long)]
        text: String,
        /// Standard-14 `BaseFont` name (`Helvetica`, `Times-Roman`, `Courier`,
        /// `Symbol`, …) or `auto` (= Helvetica). Exact §9.6.2.2 spelling.
        #[arg(long, default_value = "auto")]
        font: String,
        /// Font size in points.
        #[arg(long, default_value_t = 12.0)]
        size: f64,
        /// Fill colour as `r,g,b` components in `0..=1` (e.g. `1,0,0` red).
        /// Omitted = black.
        #[arg(long, value_name = "R,G,B")]
        color: Option<String>,
        /// Text rendering mode `Tr` (§9.3.6), 0 to 7; default 0 (fill).
        /// 3 writes the text INVISIBLE: searchable and selectable but not
        /// painted, which is how a word the OCR missed joins an OCR layer.
        #[arg(long = "render-mode", value_name = "0-7", default_value_t = 0)]
        render_mode: u8,
        /// Operator-supplied font folder (decision 012): registering a face for
        /// the chosen `--font` name discloses provenance `Supplied`. Repeatable.
        #[arg(long = "font-dir", value_name = "DIR")]
        font_dirs: Vec<PathBuf>,
        /// SUBSET AND EMBED this font file, so the saved PDF carries its own
        /// glyphs for the added text (FF-C, decision 021).
        ///
        /// Without this, `add-text` writes a Standard-14 face by name with no
        /// embedding (R79), which means the text is limited to that face's
        /// repertoire — in practice WinAnsi, so no Greek, Cyrillic, CJK or
        /// Hebrew at all. With it, pdfcer reads the given face, keeps only the
        /// glyphs this text needs, and adds them to the document as a new
        /// `/Type0` resource. Nothing already in the file is rewritten.
        ///
        /// This is ALWAYS explicit and never inferred — not from
        /// `--font-dir`, not from the text needing it (R108). Embedding
        /// changes the file size and redistributes someone else's font, so
        /// pdfcer will refuse rather than decide for you.
        ///
        /// TrueType (`.ttf`) only in this first cut; a CFF/PostScript
        /// (`.otf`) face is refused by name. `--box` is not yet supported
        /// with an embedded face.
        #[arg(long = "embed-font", value_name = "FONT-FILE")]
        embed_font: Option<PathBuf>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// **Import a plain-text file as PDF pages** — the round trip back from
    /// `extract-text`.
    ///
    /// Reads `TEXT_FILE` (UTF-8), wraps it to a column of your page template,
    /// and creates **as many pages as the text needs**. This is the import half
    /// of `extract-text`: text could come out of a document and there was no
    /// route back in. `add-text` is the neighbouring verb and is deliberately
    /// different — it places a run on ONE page and emits whatever will not fit
    /// past the paper edge, which is right for a note and would silently lose
    /// most of a text file.
    ///
    /// With `--input` the pages are inserted into that document at
    /// `--position`. Without it, pdfcer creates the document.
    ///
    /// Everything pdfcer decides on the way is printed: how many pages, how
    /// many lines per page, characters placed, tabs collapsed (indentation is
    /// LOST — PDF has no tab stops), form feeds honoured as page breaks,
    /// control characters removed, and blank pages kept. A character the
    /// Standard-14 face cannot represent — anything outside WinAnsi, so Greek,
    /// Cyrillic, CJK — **refuses the whole import and names every one of them**
    /// rather than dropping it; `--drop-unmappable` places the rest instead and
    /// reports exactly which characters were lost.
    ///
    /// A form feed (U+000C) is honoured as an explicit page break, which is the
    /// separator `extract-text --page-separator formfeed` writes — so an
    /// exported, edited, re-imported file keeps its pagination.
    PlaceText {
        /// The plain-text file to import (UTF-8; a leading byte-order mark is
        /// stripped, CRLF and lone CR both read as one line break).
        text_file: PathBuf,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Insert the created pages into this PDF. Omit to have pdfcer create
        /// the document (a fresh file of exactly the pages the text needs).
        #[arg(long)]
        input: Option<PathBuf>,
        /// Where the created pages go in `--input`: `end` (default), `start`,
        /// `before:N` or `after:N` with N a 1-based page number. Ignored
        /// without `--input`.
        #[arg(long, value_name = "WHERE", default_value = "end")]
        position: String,
        /// Sheet size: `letter` (default), `legal`, `a4`, `a3`, `tabloid`,
        /// `ansi-d`, … Superseded by `--page-size`.
        #[arg(long, default_value = "letter")]
        paper: String,
        /// Turn the sheet on its side.
        #[arg(long)]
        landscape: bool,
        /// Explicit sheet size in points, `W,H` — overrides `--paper` and
        /// `--landscape` (e.g. `612,792`).
        #[arg(long = "page-size", value_name = "W,H")]
        page_size: Option<String>,
        /// Margin on all four sides, points. 72 (one inch) by default.
        #[arg(long, default_value_t = 72.0)]
        margin: f64,
        /// Left margin, points — overrides `--margin` on that side alone.
        #[arg(long = "margin-left", value_name = "PT")]
        margin_left: Option<f64>,
        /// Right margin, points — overrides `--margin` on that side alone.
        #[arg(long = "margin-right", value_name = "PT")]
        margin_right: Option<f64>,
        /// Top margin, points — overrides `--margin` on that side alone.
        #[arg(long = "margin-top", value_name = "PT")]
        margin_top: Option<f64>,
        /// Bottom margin, points — overrides `--margin` on that side alone.
        #[arg(long = "margin-bottom", value_name = "PT")]
        margin_bottom: Option<f64>,
        /// Standard-14 `BaseFont` name (`Helvetica`, `Times-Roman`, `Courier`,
        /// …) or `auto` (= Helvetica). Exact ISO 32000-1 §9.6.2.2 spelling.
        #[arg(long, default_value = "auto")]
        font: String,
        /// Font size in points.
        #[arg(long, default_value_t = 12.0)]
        size: f64,
        /// Leading (baseline-to-baseline) in points. Omitted = `1.2 x size`,
        /// which is reported as a derived default.
        #[arg(long, value_name = "PT")]
        leading: Option<f64>,
        /// Column alignment: `left` (default) | `center` | `right` | `justify`.
        ///
        /// With `justify`, a paragraph cut across a page break has the line
        /// before the break set flush left — each page is wrapped as its own
        /// text and a paragraph's last line is never stretched. pdfcer counts
        /// and reports how many paragraphs that affected.
        #[arg(long, value_name = "MODE")]
        align: Option<String>,
        /// Text colour as `r,g,b` components in `0..=1` (e.g. `0,0,0.5`).
        /// Omitted = black.
        #[arg(long, value_name = "R,G,B")]
        color: Option<String>,
        /// Place the text even when the font cannot represent some of it,
        /// dropping those characters and reporting exactly which were lost.
        ///
        /// Off by default, and never inferred. Without it pdfcer refuses the
        /// whole import and names every offending character, because an import
        /// is bulk content nobody has read character by character and a silent
        /// hole in it is unrecoverable.
        #[arg(long = "drop-unmappable")]
        drop_unmappable: bool,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// `/Producer` handling for `--mode full` (ignored otherwise).
        #[arg(long, value_enum, default_value_t = ProducerArg::Preserve)]
        producer: ProducerArg,
    },
    /// Author a dimension: a scaled measurement `/Line`
    /// `/IT /LineDimension` annotation with a baked appearance, on its group's
    /// optional-content layer, with the scale mirrored into a portable
    /// `/Measure` dict and the authoritative `/PieceInfo` sidecar updated.
    /// Purely additive — existing page content is left byte-verbatim.
    DimensionAdd {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Dimension kind.
        #[arg(long, value_enum, default_value_t = DimKindArg::Linear)]
        kind: DimKindArg,
        /// Points as `x,y x,y ...` (space- or `;`-separated, in points).
        /// Linear uses the first two; radius/diameter fits a Taubin circle to
        /// all of them (needs at least 3 non-collinear points).
        #[arg(long)]
        points: String,
        /// Target group id (0 = the always-present default group).
        #[arg(long, default_value_t = 0)]
        group: u32,
        /// Linear alignment constraint.
        #[arg(long, value_enum, default_value_t = ConstraintArg::Aligned)]
        constraint: ConstraintArg,
        /// Standoff of the dimension line from the first point, in points,
        /// perpendicular to the measured axis (Pass 27.1).
        ///
        /// Positive is up for a horizontal dimension, right for a vertical
        /// one, and the sign does not depend on which point you gave first.
        /// 0 (the default) draws the dimension line through the first point,
        /// which is rarely what a drawing wants — a real drawing stands its
        /// dimensions off the geometry so the extension lines are visible.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        offset: f64,
        /// **Treat the two lines as parallel**, whatever the measured angle.
        ///
        /// Only meaningful with `--kind two-lines`. The CLI form of the
        /// checkbox the operator asked for: for a pair that is nominally
        /// parallel and arrived a fraction off from an exporter's rounding,
        /// a scan, or a slightly-off original.
        ///
        /// The measured angle is still REPORTED — this overrides the
        /// decision, not the measurement.
        #[arg(long)]
        treat_as_parallel: bool,
        /// Where the value text sits along the dimension line, in points from
        /// its midpoint (Pass 27.1). 0 is centred.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        text_along: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// List the dimension groups and dimensions stored in a document —
    /// reads the authoritative `/PieceInfo` sidecar.
    DimensionList {
        /// Input PDF.
        input: PathBuf,
        /// Also print each group's style defaults and each ce dimension's
        /// RESOLVED style with the tier every property came from
        /// (`factory` / `group` / `dimension`) - the inheritance disclosure.
        /// Without it, each ce dimension still reports how many properties it
        /// overrides.
        #[arg(long)]
        style: bool,
    },
    /// **Rename a ce dimension group**.
    ///
    /// Metadata only — **no appearance is regenerated**, because a group's
    /// name is not drawn on the page. Nothing about what any member measures
    /// or prints changes.
    ///
    /// The name matters beyond the group list: a ce dimension copied to
    /// another document is matched to a destination group **by name**, so
    /// renaming here changes where a future paste lands. Two groups may not
    /// share a name.
    ///
    /// List the ids and current names with `dimension-list`.
    GroupRename {
        /// Input PDF.
        input: PathBuf,
        /// The group id, as printed by `dimension-list`.
        #[arg(long)]
        group: u32,
        /// The new name.
        #[arg(long)]
        name: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Delete a ce dimension group**, answering the what-about-the-members
    /// question explicitly.
    ///
    /// `--members refuse` (the default) **refuses** a group that still has
    /// members and reports how many, so a script never destroys measurements
    /// it did not know were there. `--members reassign --to N` moves them to
    /// group N first and **re-measures** every one of them against that
    /// group's scale, unit and precision — the label derives from the group,
    /// so a re-parented ce dimension genuinely reads differently afterwards.
    ///
    /// There is deliberately **no delete-the-members policy.** It would be a
    /// second `/Annots` removal path and would give one undo entry per
    /// member; `dimension-delete` already removes a ce dimension outright.
    ///
    /// One undo entry covering the group, its members and every regenerated
    /// appearance. The default group cannot be deleted.
    GroupDelete {
        /// Input PDF.
        input: PathBuf,
        /// The group id, as printed by `dimension-list`.
        #[arg(long)]
        group: u32,
        /// What happens to member ce dimensions.
        #[arg(long, value_enum, default_value_t = GroupDeletionArg::Refuse)]
        members: GroupDeletionArg,
        /// Destination group for `--members reassign`. Required with it, and
        /// refused without it.
        #[arg(long)]
        to: Option<u32>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Move one placed ce dimension into another group — RE-MEASURING
    /// it**.
    ///
    /// This is not a field assignment. A ce dimension's scale, unit,
    /// precision and drafting standard all live on its GROUP, so re-parenting
    /// changes what the dimension **reads**, not merely which list it appears
    /// in. A 200 pt line reading `5.000 m` in a 1:50 group reads something
    /// else entirely in a 1:100 one.
    ///
    /// The printed value before and after is reported on the result line for
    /// exactly that reason — it is the fact most likely to be mistaken for a
    /// defect.
    ///
    /// `/Rect`, `/Contents`, `/Measure` and `/L` are all regenerated together
    /// through the one shared path, as a single undo entry.
    DimensionGroup {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// The destination group id.
        #[arg(long)]
        group: u32,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set a ce dimension group's drafting standard** (Pass 27.2) and
    /// regenerate every member to it.
    ///
    /// ANSI (the default) breaks the dimension line and centres the value in
    /// the gap, with all text horizontal. ISO runs the line unbroken with the
    /// value above it, aligned to the line, and uses a comma decimal marker —
    /// mandated by ISO 129-1:2018 cl. 4.1.1 — which is also mirrored into the
    /// portable `/Measure` dict so a conforming reader computes the same
    /// string pdfcer drew.
    ///
    /// pdfcer draws **ISO-style**, not "ISO 129-1 conformant": that standard's
    /// normative Annex A is paywalled and was not obtained, so the claim would
    /// be broader than the evidence.
    GroupSetStandard {
        /// Input PDF.
        input: PathBuf,
        /// Group id, as printed by `dimension-list`.
        #[arg(long, default_value_t = 0)]
        group: u32,
        /// The drafting standard.
        #[arg(long, value_enum)]
        standard: StandardArg,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set a ce dimension GROUP's style defaults** (Pass 69.0) and
    /// regenerate every member.
    ///
    /// This is the middle tier of the style cascade: factory default -> GROUP
    /// default -> per-ce-dimension override. A member that does not override a
    /// property follows what is set here; a member that overrides it does not.
    /// Read the current state, and which tier each value came from, with
    /// `dimension-list --style`.
    ///
    /// Flags not given are LEFT ALONE (read-modify-write), so setting one
    /// property never silently clears another. To clear a property back to the
    /// factory default use `--clear <property>`; to clear them all, `--reset`.
    GroupStyle {
        /// Input PDF.
        input: PathBuf,
        /// Group id, as printed by `dimension-list`.
        #[arg(long, default_value_t = 0)]
        group: u32,
        /// Label point size.
        #[arg(long)]
        text_height: Option<f64>,
        /// Dimension/extension-line stroke width, in points.
        #[arg(long)]
        line_width: Option<f64>,
        /// Arrowhead length, in points.
        #[arg(long)]
        arrow_length: Option<f64>,
        /// Terminator form drawn at each end of the dimension line.
        #[arg(long, value_enum)]
        arrow_form: Option<ArrowFormArg>,
        /// Colour, as `r,g,b` components in 0.0-1.0 or as `#rrggbb`.
        #[arg(long)]
        color: Option<String>,
        /// Tolerance, as `none` | `basic` | `min` | `max` | `sym:<v>` |
        /// `dev:<plus>/<minus>` | `limit:<upper>/<lower>`. Values are in the
        /// DISPLAYED unit, not page points.
        #[arg(long, allow_hyphen_values = true)]
        tolerance: Option<String>,
        /// The tolerance's own decimal precision. Omit to follow the nominal's.
        #[arg(long)]
        tolerance_places: Option<u32>,
        /// Clear a property back to the factory default (repeatable, or
        /// comma-separated). Property names are the ones
        /// `dimension-list --style` prints.
        #[arg(long, value_enum, value_delimiter = ',')]
        clear: Vec<StylePropArg>,
        /// Clear every group-tier style property.
        #[arg(long)]
        reset: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set ONE ce dimension's style overrides** (Pass 69.0) - the operator's
    /// per-ce-dimension "override and set differently" checkbox, in batch form.
    ///
    /// Each property set here detaches THAT property from the group for THIS
    /// ce dimension only; everything else keeps following the group, so a
    /// later group edit still moves it. `--clear`/`--reset` re-attach.
    ///
    /// The scale is deliberately absent: it is a group property and stays one.
    /// A ce dimension quietly measuring at a different scale from its group
    /// would print a number nothing on the page discloses.
    DimensionStyle {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Display unit for this ce dimension only.
        #[arg(long)]
        unit: Option<String>,
        /// Fixed decimal places for this ce dimension only.
        #[arg(long)]
        places: Option<u32>,
        /// Show the value as a fraction with this denominator (inches).
        #[arg(long)]
        denominator: Option<u32>,
        /// Reduce the fraction (only with `--denominator`).
        #[arg(long)]
        reduce: bool,
        /// Decimal marker for this ce dimension only.
        #[arg(long, value_enum)]
        decimal_marker: Option<DecimalMarkerArg>,
        /// Drafting standard for this ce dimension only.
        #[arg(long, value_enum)]
        standard: Option<StandardArg>,
        /// Label point size.
        #[arg(long)]
        text_height: Option<f64>,
        /// Dimension/extension-line stroke width, in points.
        #[arg(long)]
        line_width: Option<f64>,
        /// Arrowhead length, in points.
        #[arg(long)]
        arrow_length: Option<f64>,
        /// Terminator form.
        #[arg(long, value_enum)]
        arrow_form: Option<ArrowFormArg>,
        /// Colour, as `r,g,b` components in 0.0-1.0 or as `#rrggbb`.
        #[arg(long)]
        color: Option<String>,
        /// Tolerance, as `none` | `basic` | `min` | `max` | `sym:<v>` |
        /// `dev:<plus>/<minus>` | `limit:<upper>/<lower>`. Values are in the
        /// DISPLAYED unit, not page points.
        #[arg(long, allow_hyphen_values = true)]
        tolerance: Option<String>,
        /// The tolerance's own decimal precision. Omit to follow the nominal's.
        #[arg(long)]
        tolerance_places: Option<u32>,
        /// Clear an override, returning that property to inheritance
        /// (repeatable, or comma-separated).
        #[arg(long, value_enum, value_delimiter = ',')]
        clear: Vec<StylePropArg>,
        /// Clear every override on this ce dimension.
        #[arg(long)]
        reset: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Rotate a ce dimension** about a point (`Pass 159.0`).
    ///
    /// # The measured value does NOT change
    ///
    /// A rotation preserves every distance, so the number is identical either
    /// side of it — not because pdfcer holds it, but because there is nothing
    /// to change. That is what makes rotating a ce dimension a legitimate
    /// drafting operation.
    ///
    /// # Scaling one is deliberately not offered
    ///
    /// It has no honest reading: either the value stays fixed while the
    /// geometry grows, so the dimension lies about the drawing, or both
    /// change, so nothing was measured. Use `group-scale` to change the
    /// measurement RATIO — which is the operation actually wanted.
    ///
    /// # What it may relax, and reports
    ///
    /// A `Linear` dimension locked to horizontal or vertical cannot stay
    /// locked through a rotation. pdfcer relaxes it to *aligned* — which is
    /// what a line following its own picked points actually is — and says so.
    DimensionRotate {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Rotation in degrees, ANTICLOCKWISE.
        #[arg(long, allow_negative_numbers = true)]
        degrees: f64,
        /// Pivot x in points — the point that does not move.
        #[arg(long, allow_negative_numbers = true)]
        pivot_x: f64,
        /// Pivot y in points.
        #[arg(long, allow_negative_numbers = true)]
        pivot_y: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },
    /// **Edit one vertex of a ce dimension** (`Pass 107.0`) — move it, insert
    /// a new one after it, or remove it.
    ///
    /// This is the only ce-dimension verb that deliberately RE-MEASURES.
    /// `dimension-move` translates the whole thing and preserves every
    /// distance; `dimension-offset` writes only placement fields the value
    /// function never reads. Here the number changing is the point, so the
    /// command prints the value **before and after** — the CLI has no session
    /// and no undo, so the invocation IS the commit and the disclosure has to
    /// ride out with it (project rule 4, rule 11).
    ///
    /// Read the current vertex count from `dimension-list`, which prints
    /// `vertices=` for a perimeter. Indices are 0-based and in pick order.
    ///
    /// `--dry-run` answers exactly what the real invocation would answer,
    /// through the same guards, and writes nothing — the scriptable form of
    /// the preflight a GUI uses to grey a menu item.
    ///
    /// Refusals, all evaluated before anything is written: an index that names
    /// nothing; a removal that would leave fewer than two vertices on an open
    /// path or three on a closed one; a non-finite coordinate; and
    /// insert/remove aimed at a linear ce dimension, whose two picked points
    /// are structural (moving one of them IS supported).
    DimensionVertex {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Which edit to perform.
        #[arg(long, value_enum)]
        op: VertexOpArg,
        /// The 0-based vertex index. For `insert`, the FIRST vertex of the
        /// segment being split — so the last index splits a closed shape's
        /// closing segment, and extends an open path.
        #[arg(long)]
        index: usize,
        /// `move` only: page-space x displacement, in points.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dx: f64,
        /// `move` only: page-space y displacement, in points.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dy: f64,
        /// `insert` only: where the new vertex goes, as `x,y` in points.
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
        /// Report what the edit would do and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Output path. Not used with `--dry-run`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Edit one vertex of a markup annotation** (`Pass 255.0`) — move it,
    /// insert a new one after it, or remove it — on a `/Polygon` (plain or
    /// cloudy), a `/PolyLine`, or (move only) a `/Line`.
    ///
    /// The `/Vertices` (or `/L`) array, the `/Rect` and the appearance
    /// stream are all rebuilt from the new geometry by the same bake
    /// `annotate` authored the shape with, so a reshaped revision cloud
    /// scallops exactly as a redrawn one would. Read the current vertices
    /// from `list-annotations`, which prints `vertices=` / `line=` /
    /// `ink=` per annotation. Indices are 0-based in array order.
    ///
    /// Insert and remove on a polygon or polyline EXCEED current Acrobat
    /// (whose GUI only drags existing points) on purpose. The floor is 3
    /// vertices for a Polygon and 2 for a PolyLine — a removal that would
    /// go below it is refused; delete the annotation instead.
    ///
    /// Refused by name, never silently: any vertex edit on `/Ink` (a pen
    /// trace is moved, resized or redrawn whole — Acrobat has never offered
    /// per-point ink editing either), insert/remove on a `/Line` (two
    /// endpoints by definition), any edit on a `/Square`, `/Circle` or text
    /// markup (no vertices), a ce dimension (use `dimension-vertex`, which
    /// re-measures), and an annotation with the Locked flag. The
    /// LockedContents flag does NOT block a reshape — it guards the note
    /// text, not the geometry.
    ///
    /// If the annotation carries a `/Measure` dictionary its stated
    /// measurement is NOT recomputed and the command says so on stderr —
    /// the number may now be stale.
    ///
    /// `--dry-run` answers exactly what the real invocation would, through
    /// the same guards, and writes nothing.
    AnnotationVertex {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the annotation is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Which annotation, numbered from 0 in `list-annotations` order.
        #[arg(long, default_value_t = 0)]
        annot: usize,
        /// What to do: `move`, `insert` or `remove`.
        #[arg(long, value_enum)]
        op: VertexOpArg,
        /// The vertex, 0-based. For `insert`, the vertex the new one goes
        /// AFTER.
        #[arg(long)]
        index: usize,
        /// Horizontal shift in points (move only), positive to the right.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dx: f64,
        /// Vertical shift in points (move only), positive UP.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dy: f64,
        /// Where the new vertex goes (insert only), as `x,y` in points.
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
        /// A `/M` modification date to stamp, verbatim (e.g.
        /// `D:20260905120000Z`). pdfcer reads no clock; without this the
        /// annotation's `/M` is left exactly as it was.
        #[arg(long)]
        modified: Option<String>,
        /// Report what would happen and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Output path (required unless `--dry-run`).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// After saving, undo in memory and check the base bytes are
        /// untouched.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Edit a freehand `/Ink` stroke** (`Pass 278.0`) — move, insert or
    /// remove one point, or replace, move or remove a whole stroke.
    ///
    /// An `/InkList` (§12.5.6.13) is a list **of** strokes, so a point in it
    /// is addressed by `--stroke` and `--point`, not by one index — which is
    /// why `annotation-vertex` refuses `/Ink` and sends you here. Read the
    /// current strokes from `list-annotations`, which prints `ink=` per
    /// annotation. Both indices are 0-based.
    ///
    /// The `/InkList`, the `/Rect` and the appearance stream are all rebuilt
    /// from the new geometry by the same bake `annotate` authors ink with.
    /// pdfcer draws a stroke as a POLYLINE — §12.5.6.13 leaves the join
    /// "implementation-dependent" — so a point move changes where two
    /// segments go and nothing else. On a stroke pdfcer did not draw, the
    /// re-bake REPLACES the other producer's artwork with pdfcer's rendering,
    /// which straightens a smoothed curve; the command says so on stderr
    /// before it saves.
    ///
    /// Refused by name, never silently: a stroke or point index that names
    /// nothing (reported separately, because the two index spaces are
    /// different questions); a point removal that would leave a stroke with
    /// fewer than two points — one point is not a path, so use
    /// `--op remove-stroke`; a stroke removal that would leave the annotation
    /// with nothing drawable — `delete-annotation` removes the annotation
    /// itself, along with its comment and reply thread; a non-finite result;
    /// an annotation that is not an `/Ink`; and the Locked flag. The
    /// LockedContents flag does NOT block a reshape — it guards the note
    /// text, not the geometry.
    ///
    /// `--dry-run` answers exactly what the real invocation would, through
    /// the same guards, and writes nothing.
    InkEdit {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the annotation is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Which annotation, numbered from 0 in `list-annotations` order.
        #[arg(long, default_value_t = 0)]
        annot: usize,
        /// What to do.
        #[arg(long, value_enum)]
        op: InkOpArg,
        /// Which stroke of the `/InkList`, 0-based.
        #[arg(long, default_value_t = 0)]
        stroke: usize,
        /// Which point within that stroke, 0-based. For `insert-point`, the
        /// point the new one goes AFTER — the last index extends the stroke.
        #[arg(long, default_value_t = 0)]
        point: usize,
        /// Horizontal shift in points, positive to the right
        /// (`move-point` / `move-stroke`).
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dx: f64,
        /// Vertical shift in points, positive UP
        /// (`move-point` / `move-stroke`).
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        dy: f64,
        /// Where the new point goes (`insert-point` only), as `x,y` in points.
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
        /// The stroke's new points (`replace-stroke` only), as
        /// `x,y;x,y;…` in points. At least two.
        #[arg(long, allow_hyphen_values = true)]
        points: Option<String>,
        /// A `/M` modification date to stamp, verbatim (e.g.
        /// `D:20260909120000Z`). pdfcer reads no clock; without this the
        /// annotation's `/M` is left exactly as it was.
        #[arg(long)]
        modified: Option<String>,
        /// Report what would happen and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Output path (required unless `--dry-run`).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// How to save: incremental (default) or full rewrite.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// After saving, undo in memory and check the base bytes are
        /// untouched.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Which characters can this run accept?** (`Pass 280.0`) — asked
    /// before an edit is attempted, not after it is refused.
    ///
    /// Locates one text run the same way `edit-text` does (`--find`, or
    /// `--pin-span` for a specific show operator) and prints the set of
    /// characters that run will take. **A character in the set is one
    /// `edit-text` will not refuse for that run** — the query asks the same
    /// accepting code the refusal does, so the two cannot disagree.
    ///
    /// It is computed from the run's OWN font resource, so an embedded subset
    /// is narrowed to the codes this page already carries (`R-INV-1`) rather
    /// than widened to whatever the face could draw. That distinction is the
    /// difference between *"this font cannot draw that character"* and *"this
    /// FILE cannot, yet"* — and only the second has `format-text --set-font`
    /// as a remedy.
    ///
    /// A run whose font has **no usable encoding** is not an error: the set is
    /// empty and `reason=` says why, so a script can skip the run rather than
    /// attempt an edit that cannot succeed.
    RunRepertoire {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page the run is on.
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Text identifying the run, as `edit-text --find` takes it. Empty
        /// with `--pin-span` means the whole pinned operator.
        #[arg(long, default_value = "")]
        find: String,
        /// Pin the run to one show operator by content-stream byte span,
        /// `START:LEN` — the same spelling `edit-text --pin-span` takes.
        #[arg(long)]
        pin_span: Option<String>,
        /// Print every accepted character rather than a summary count.
        #[arg(long)]
        list: bool,
    },
    /// **Place a ce dimension** (Pass 27.1): set how far its dimension line
    /// stands off the geometry and where its value sits along that line.
    ///
    /// This is the batch form of dragging a dimension in the GUI, and like the
    /// drag it does NOT re-measure: the measured points stay where they were,
    /// the extension lines stretch, and the printed value is unchanged. Read
    /// the current values from `dimension-list`.
    ///
    /// Refused by name for a circular dimension, which has no axis to stand
    /// off from or slide along.
    ///
    /// For a PERIMETER (`Pass 107.0`) the same two numbers displace the label
    /// from the shape's vertex centroid, in PAGE axes: `--offset` is +y and
    /// `--text-along` is +x. A polyline has no single axis for a standoff to
    /// be perpendicular to, and anchoring on the centroid is what keeps the
    /// label from teleporting when a vertex is edited.
    DimensionOffset {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Standoff from the first measured point, in points, perpendicular to
        /// the measured axis. Positive is up for a horizontal dimension, right
        /// for a vertical one.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        offset: f64,
        /// Where the value sits along the dimension line, in points from its
        /// midpoint. 0 is centred.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        text_along: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set a placed ce dimension's radius/diameter display** (Pass 34.2).
    ///
    /// Radius-versus-diameter used to be a draw-time choice only: whatever was
    /// picked when the ce dimension was authored was permanent, and the only
    /// way to change it was to delete and redraw — which also loses the
    /// dimension's id, its group and its placement. This changes the reading
    /// on an already-placed ce dimension.
    ///
    /// It does NOT re-measure: the fitted circle's centre, radius and fit
    /// residual are untouched, and only the flag deciding whether the label
    /// prints `r` or `2r` moves. Read the current reading from
    /// `dimension-list`.
    ///
    /// Refused by name for a LINEAR ce dimension, which has no circle and so
    /// no radius or diameter to choose between.
    DimensionDisplay {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Which reading the label should print.
        #[arg(long, value_enum)]
        show: DisplayReading,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set or clear a ce dimension's TEXT OVERRIDE** (Pass 175.0,
    /// decision 097): make it print what you type instead of what it measured
    /// -- or clear the override and get the measurement back, exactly, with no
    /// re-measurement.
    ///
    /// The override SHADOWS the measurement; it never replaces it. The
    /// measured geometry, the group's scale and the annotation's `/Measure`
    /// dict are all untouched, and the measured value stays in the
    /// `/PieceInfo` sidecar beside the override, so `--clear` a week and three
    /// saves later restores exactly what was there.
    ///
    /// `<DIM>` in the text is replaced by the measured caption, so
    /// `--text "2X <DIM> TYP"` keeps TRACKING the geometry: re-scale the group
    /// and the printed number follows. A bare `--text "55 5/8"` tracks nothing,
    /// which is you saying so.
    ///
    /// Find the id, and see which dimensions are already overridden, with
    /// `dimension-list` -- it prints the measured `value=` and the overriding
    /// `label=` side by side.
    ///
    /// Refused for text that is empty, longer than 128 characters, or contains
    /// a character the caption cannot draw (the label font is Helvetica with
    /// WinAnsiEncoding, so a glyph outside it would print as a question mark).
    DimensionLabel {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// The caption to print instead of the measurement. May contain
        /// `<DIM>`, which is replaced by the measured caption.
        #[arg(long, conflicts_with = "clear", required_unless_present = "clear")]
        text: Option<String>,
        /// Clear the override and restore the measured caption.
        #[arg(long, conflicts_with = "text")]
        clear: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Delete a ce dimension** (Pass 25.6): remove its `/Annots` reference,
    /// its annotation dictionary, its `/AP` appearance stream and its
    /// `/PieceInfo` sidecar record, together, as one undoable command.
    ///
    /// Find the id with `dimension-list`. The dimension's GROUP is left alone
    /// even when this was its last member — a group carries a calibrated scale
    /// that is not cheap to redo.
    DimensionDelete {
        /// Input PDF.
        input: PathBuf,
        /// The ce dimension id, as printed by `dimension-list`.
        #[arg(long)]
        dimension: u32,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// Create a named dimension group. Prints the new group id.
    GroupAdd {
        /// Input PDF.
        input: PathBuf,
        /// The group name.
        #[arg(long)]
        name: String,
        /// The display unit: `mm|cm|m|km|in|ft|ft-in|yd|mi`.
        #[arg(long, default_value = "mm")]
        unit: String,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// Set a dimension group's scale + units, regenerating every member's
    /// baked appearance.
    GroupSetScale {
        /// Input PDF.
        input: PathBuf,
        /// Target group id (0 = default group).
        #[arg(long, default_value_t = 0)]
        group: u32,
        /// Real-length path: the real-world length of a drawn reference line,
        /// written the way a drawing writes it.
        ///
        /// Accepts `55 5/8"`, `4'-7 1/2"`, `12'`, `1200mm`, `1.2m`, or a plain
        /// number (which uses `--unit`). A notation that names a unit sets the
        /// group's unit too, so `--unit` is only needed for a bare number —
        /// the same rule the GUI field follows, so a command and a click
        /// produce the same result from the same text.
        #[arg(long)]
        real_length: Option<String>,
        /// Real-length path: the drawn reference line's length in points.
        #[arg(long)]
        drawn: Option<f64>,
        /// Direct-ratio path, e.g. `1:100` (paper:real; inch paper-unit basis).
        #[arg(long)]
        ratio: Option<String>,
        /// Display unit: `mm|cm|m|km|in|ft|ft-in|yd|mi`.
        #[arg(long, default_value = "mm")]
        unit: String,
        /// Set an explicit 1:1 (full-size) scale instead of calibrating.
        #[arg(long)]
        one_to_one: bool,
        /// Decimal precision (decimal units) or fraction denominator (ft-in).
        #[arg(long)]
        precision: Option<u32>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// Toggle a dimension group's optional-content layer visibility
    /// (§8.11 `/D` config).
    LayerToggle {
        /// Input PDF.
        input: PathBuf,
        /// Target group id.
        #[arg(long, default_value_t = 0)]
        group: u32,
        /// Hide the layer (default is to show it).
        #[arg(long)]
        hide: bool,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **List** a page's vector objects in paint order — the index discovery
    /// path for `object-move`, `object-delete` and `node-move`.
    ///
    /// Read-only; nothing is written. One `object …` line per selectable
    /// object, then an `object-list …` summary line. The `index=` on each
    /// line IS the value those three editing subcommands take as `--object`:
    /// both come from the same `pdfcer_core::vector::decompose_page` walk, in
    /// the same paint order, so the correspondence is exact and not a
    /// convention this subcommand invents.
    ///
    /// Every geometry figure is in **PDF user space** (page space): origin at
    /// the page's lower-left, Y increasing upward, units of points (1/72 in).
    /// That is the same frame `object-move --dx/--dy`, `node-move --x/--y`
    /// and `dimension-add --points` use.
    ///
    /// `--hit X,Y` additionally answers "which object would a click here
    /// select?" by calling the SAME `pdfcer_core::vector::hit_test_point_deep`
    /// the GUI's object-edit tool calls, so the answer is authoritative for
    /// the GUI's behaviour rather than a second implementation of it.
    ///
    /// FORMS ARE NOT CANDIDATES, which a script has to plan for. A
    /// form's `/BBox` is a clipping extent (ISO 32000-1 8.10.1), not ink, so
    /// a page-sized form is not a page-sized hit target — what is drawn
    /// INSIDE it is reported instead, on rows carrying `leaf=N
    /// containment=… paint_order=N editable=false` and `kind=leaf:…` in place
    /// of `index=N`. The key differs deliberately: `index=` is what the
    /// editing subcommands take, and they write to the PAGE's stream, so a
    /// leaf ordinal under that key would be a number that is in range and
    /// corrupts the page.
    ///
    /// `--hit-scope page` gives a shallow, page-only query if a script needs
    /// one. It is not the GUI's behaviour, and the `scope=` field on every
    /// `hit` line says which one you asked for.
    ///
    /// One difference from the GUI, deliberately: the GUI receives a
    /// *canvas-space* pointer (Y-down device coordinates, page rotation
    /// applied) and converts it to PDF space before hit-testing, whereas
    /// `--hit` takes PDF space directly — so on a rotated or non-zero-origin
    /// page the number you would read off a screen ruler is NOT the number to
    /// pass here. Use the `bbox=` values this subcommand prints.
    ///
    /// `--all-hits` adds one `hit-candidate …` line per target under the
    /// point, front-most first — the same list the GUI's Alt+click cycling
    /// steps through, interleaved on paint order so an object inside a form
    /// is a first-class stop on that walk. Without it the `hit …` line names
    /// only the winner, which cannot answer "why did my click select THAT?"
    /// when two objects overlap.
    ///
    /// A `--hit` MISS is a valid answer, not an error: the exit code stays 0
    /// and the `hit …` line reports `index=none`. Scripts branch on that
    /// field, not on the exit status.
    ///
    /// Example — inventory page 1:
    ///
    ///     pdfcer object-list drawing.pdf --page 1
    ///
    /// Example — ask what a click at (200, 200) would select:
    ///
    ///     pdfcer object-list drawing.pdf --page 1 --hit 200,200
    ///
    /// Example — ask what ELSE is under that point, in cycling order:
    ///
    ///     pdfcer object-list drawing.pdf --page 1 --hit 200,200 --all-hits
    ///
    /// Example — move whatever object index 2 turned out to be:
    ///
    ///     pdfcer object-move drawing.pdf --object 2 --dx=10 --dy=0 -o out.pdf
    ObjectList {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Report which object a click at this page-space point would select,
        /// as `X,Y` in PDF user space (points).
        #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
        hit: Option<String>,
        /// List EVERY object under `--hit`, front-most first, as
        /// `hit-candidate …` lines with an `ordinal=` field — the order the
        /// GUI's Alt+click click-through cycling visits them in. The `hit …`
        /// line is still printed and still names the topmost. Ignored
        /// without `--hit`.
        #[arg(long)]
        all_hits: bool,
        /// How deep `--hit` looks: `deep` descends into form XObjects and
        /// never names a form itself (the GUI's behaviour, and the default);
        /// `page` is a shallow query over the page's own object list only.
        ///
        /// Under `page` a page-sized form wins every click at every point,
        /// which is what the operator originally reported as "all I get is
        /// the page selected".
        #[arg(long, value_enum, default_value_t = HitScope::Deep)]
        hit_scope: HitScope,
        /// Report which straight LINE a click at this page-space point would
        /// resolve to, as `X,Y` in PDF user space — the headless twin of the
        /// two-line measure gesture.
        ///
        /// Different from `--hit`, and the difference matters on a CAD sheet:
        /// `--hit` names an OBJECT, and one measured export holds an entire
        /// orthographic view as a single object with 1194 subpaths. "Which
        /// object" does not answer "which line did I click".
        ///
        /// Searches page objects AND the contents of form XObjects, so it
        /// works on a wrapped drawing. A curve is never reported: pdfcer does
        /// not chord a Bezier into a line, because dimensioning "the line" of
        /// a curve would measure something the drawing does not contain.
        ///
        /// A miss prints `index=none` with a `reason=` and exits 0.
        #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
        line_pick: Option<String>,
        /// Descend INTO this object index and report which of its subpaths the
        /// `--hit` point lands on, nearest first, as `subpath-hit …` lines.
        ///
        /// A CAD producer routinely emits an entire drawing view as ONE path
        /// object with hundreds of subpaths — one measured SolidWorks export
        /// has a single object holding 1194 subpaths and 6681 anchors for a
        /// whole isometric view. Per-object hit testing correctly names that
        /// object, which is useless if what you meant was one line of it. This
        /// is the level below: the same query the GUI runs after a double-click
        /// enters an object.
        ///
        /// Combine with `--hit`; ignored without it. Prints nothing for a
        /// non-path object or an out-of-range index.
        #[arg(long, value_name = "INDEX")]
        enter: Option<usize>,
        /// Page-space slack, in points, a `--hit` point may miss an object's
        /// edge by and still select it. Default 3.0 — the GUI's
        /// `FALLBACK_SELECT_TOLERANCE`, i.e. the catch radius a click gets at
        /// 100% zoom. Ignored without `--hit`.
        ///
        /// `allow_hyphen_values` matches the other numeric operands in this
        /// CLI (`--dx`, `--dy`, `--x`, `--y`): a leading `-` must reach the
        /// f64 parser as a value, so a negative reaches the handler's
        /// named refusal instead of dying as a clap usage error (exit 2)
        /// that tells the operator nothing about why it is wrong.
        #[arg(long, default_value_t = HIT_TOLERANCE_PT, allow_hyphen_values = true)]
        tolerance: f64,
    },
    /// **Move** a path or text object by a page-space `(dx, dy)` via
    /// content-stream surgery: a path's construction operands are translated;
    /// a text object's `Tm` and first `Td` are (a `Td` is added, and said so,
    /// when the text has none to adjust). Only the edited content stream
    /// changes; every other object stays byte-verbatim. Images are refused —
    /// use `object-transform`. `--object` is the object's 0-based paint-order
    /// index — run `object-list` on the page to discover it.
    ObjectMove {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// Page-space x displacement, in points.
        #[arg(long, allow_hyphen_values = true)]
        dx: f64,
        /// Page-space y displacement, in points.
        #[arg(long, allow_hyphen_values = true)]
        dy: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Transform** a selection of vector objects (Pass 113.0): scale, rotate,
    /// shear or move them by one page-space matrix, by wrapping each object's
    /// operator run in `q <cm> ... Q`.
    ///
    /// Works on ANY object kind — path, text, image XObject, form XObject,
    /// inline image — because wrapping never looks at an operand. That is what
    /// `object-move` cannot do: operand rewriting can express translation and
    /// nothing else, and an image has no operand to rewrite at all.
    ///
    /// The transform is built from `--scale`, `--rotate` and `--translate`,
    /// composed in that order, and applied about `--pivot` (default: the
    /// selection's own bounding-box centre, so a scale or rotation stays where
    /// the objects are instead of flying toward the page origin).
    ///
    /// `--objects` takes 0-based paint-order indices, the numbering
    /// `object-list` prints. `--preview` plans and reports WITHOUT writing
    /// anything, which is the same body the real verb runs.
    ObjectTransform {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object indices, comma-separated (`3,4,7`).
        #[arg(long, value_name = "N,N,...")]
        objects: String,
        /// Uniform or `SX,SY` scale factor. A NEGATIVE factor is a mirror and
        /// is perfectly legal; exactly zero is refused (see `--on-singular`).
        #[arg(long, value_name = "S|SX,SY", allow_hyphen_values = true)]
        scale: Option<String>,
        /// Rotation in DEGREES, counter-clockwise.
        #[arg(long, value_name = "DEG", allow_hyphen_values = true)]
        rotate: Option<f64>,
        /// Page-space translation `DX,DY` in points, applied last.
        #[arg(long, value_name = "DX,DY", allow_hyphen_values = true)]
        translate: Option<String>,
        /// Pivot `X,Y` in page points for the scale/rotation. Defaults to the
        /// selection's bounding-box centre.
        #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
        pivot: Option<String>,
        /// What to do when the selection holds more than one object kind:
        /// `whole` (default) or `refuse`.
        #[arg(long, value_name = "whole|refuse", default_value = "whole")]
        on_mixed: String,
        /// What to do when the transform is singular (maps area to zero):
        /// `refuse` (default) or `clamp:MIN`.
        #[arg(long, value_name = "refuse|clamp:MIN", default_value = "refuse")]
        on_singular: String,
        /// Plan and report WITHOUT writing anything (Pass 113.1). `--output`
        /// is then optional and ignored.
        #[arg(long)]
        preview: bool,
        /// Output path. Required unless `--preview`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Copy** a selection of page objects to a clipboard file.
    ///
    /// Writes a self-contained `.pdfceclip` payload carrying the copied bytes
    /// AND every resource they reference — the font, the image, the graphics
    /// state — so `object-paste` needs neither the source file nor this
    /// process. That is what makes cross-document paste work.
    ///
    /// Reads only; the input PDF is not modified and no output PDF is written.
    ///
    /// `--objects` takes 0-based paint-order indices, the numbering
    /// `object-list` prints. Add `--cut OUTPUT.pdf` to also remove them from a
    /// copy of the input, which is cut in the one-shot shape a CLI can offer.
    ObjectCopy {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object indices, comma-separated (`3,4,7`).
        ///
        /// May be empty when `--annotations` is given: content objects and
        /// annotations are two different address spaces, and a selection may
        /// be entirely one or the other.
        #[arg(long, value_name = "N,N,...", default_value = "")]
        objects: String,
        /// 0-based ANNOTATION indices, comma-separated (Pass 120.4) — markup
        /// and ce dimensions, in the page's /Annots order.
        ///
        /// A different numbering from `--objects`: an annotation is not page
        /// content, so it has no paint-order index. A widget is carried but
        /// NOT pasted — see the paste refusal.
        #[arg(long, value_name = "N,N,...", default_value = "")]
        annotations: String,
        /// Where to write the clipboard payload.
        #[arg(long, value_name = "FILE")]
        clip: PathBuf,
        /// Also write the selection as a standalone one-page PDF here
        /// (Pass 120.2) — the interchange format, for applications that are
        /// not pdfcer.
        ///
        /// The page IS the selection: its MediaBox is the selection's own
        /// bounding box, so a consumer placing the file gets the objects and
        /// no surrounding whitespace. Deliberately NOT the same thing as
        /// `--clip`: a PDF cannot carry which byte range was which object,
        /// each item's CTM, or which resource names its operators consumed, so
        /// a pdfcer-to-pdfce paste through it would be worse than the private
        /// format. Write both; use each with its own consumer.
        #[arg(long, value_name = "FILE.pdf")]
        pdf: Option<PathBuf>,
        /// Also remove what was copied, writing the result here — CUT.
        ///
        /// Removes the annotations too, not only the objects — a cut that
        /// took the content objects and left every `--annotations` entry on
        /// the page would still report `cut=1`, which is the shape of a
        /// silent partial deletion.
        ///
        /// ONE undo entry, however many things were removed — so undoing a
        /// cut in a shell puts back exactly what one cut took.
        ///
        /// REFUSED when the selection holds an annotation the clipboard
        /// cannot carry back (a `/Link`, a `/Popup`, a sticky note). Copy
        /// leaves such a thing in place and says so; a cut would delete it
        /// with nothing to paste, which is a deletion wearing a clipboard's
        /// clothes.
        #[arg(long, value_name = "OUTPUT.pdf")]
        cut: Option<PathBuf>,
        /// Save mode for `--cut`.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
    },

    /// **Paste** a clipboard file onto a page (Pass 120.0/120.1).
    ///
    /// The payload's resources are imported at fresh object numbers and bound
    /// to fresh names, and the copied content is rewritten to use them — so a
    /// text run copied from a Helvetica document stays Helvetica even when the
    /// destination's own `/F1` is Courier. Pasting the bytes verbatim would
    /// bind to the destination's font and silently render the wrong typeface.
    ///
    /// Placement is a page-space transform built from `--translate`,
    /// `--scale` and `--rotate`, the same flags `object-transform` takes. With
    /// none of them the paste lands exactly where it was copied from.
    ///
    /// `--preview` reports what would happen and writes nothing.
    ObjectPaste {
        /// Input PDF — the destination.
        input: PathBuf,
        /// 1-based page number to paste onto.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// The clipboard payload written by `object-copy`.
        #[arg(long, value_name = "FILE")]
        clip: PathBuf,
        /// Page-space translation `DX,DY` in points.
        #[arg(long, value_name = "DX,DY", allow_hyphen_values = true)]
        translate: Option<String>,
        /// Uniform or `SX,SY` scale, about the clip's own centre.
        #[arg(long, value_name = "S|SX,SY", allow_hyphen_values = true)]
        scale: Option<String>,
        /// Rotation in DEGREES counter-clockwise, about the clip's own centre.
        #[arg(long, value_name = "DEG", allow_hyphen_values = true)]
        rotate: Option<f64>,
        /// Plan and report WITHOUT writing anything.
        #[arg(long)]
        preview: bool,
        /// Output path. Required unless `--preview`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Give one page its own private copy of a shared form XObject**
    /// (`unshare_form`), so a later edit to it changes that page and no other.
    ///
    /// # Why you would want this
    ///
    /// A form XObject may legally be invoked from more than one page and more
    /// than once from one page — ISO 32000-1 §8.10.1 names CAD output as its
    /// own illustration, and a shared title block is the everyday case. So
    /// editing content inside one **necessarily changes every sheet that
    /// invokes it**: there is exactly one stream object to write, and pdfcer
    /// cannot prevent that structurally.
    ///
    /// That is the documented default (edit-in-place, disclosed). This is the
    /// explicit, separate act of **breaking the sharing first**, so that the
    /// edit which follows lands on one page only.
    ///
    /// # What changes in the file
    ///
    /// The form is cloned to a new object and this page's reference(s) are
    /// re-pointed at the copy. Every other invocation site keeps naming the
    /// original and is left byte-identical. If the page's `/Resources` or its
    /// `/XObject` subdictionary was inherited or shared, it is privatised in
    /// the same write — otherwise the re-point would leak onto every page that
    /// shares the dictionary, producing a "private" copy that is still shared.
    ///
    /// # Refused for a form invoked from INSIDE another form
    ///
    /// Re-binding a nested invocation means editing the parent form, which may
    /// itself be shared, so the blast radius would depend on the document's
    /// nesting structure. Unshare the outer form instead, or edit in place and
    /// accept that every invocation changes.
    ///
    /// # Finding the object number
    ///
    /// `object-list --page N` prints `kind=form` rows. The form's object number
    /// is what `--form` takes.
    UnshareForm {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number to give the private copy to.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// The form XObject stream's **object number**.
        #[arg(long)]
        form: u32,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },

    /// **Delete** a vector object (Pass 9c-min, decision 011 §2.5): remove an
    /// object's construction + painting operators from the content stream via
    /// surgery (R46/§5.7). Works on any object kind (path/text/image). NOT
    /// redaction — it removes a drawing object from a page, not covered
    /// content for security.
    ObjectDelete {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Move one subpath** of a path object (Pass 28.0): translate a single
    /// subpath's construction operands, leaving the object's other subpaths
    /// byte-verbatim.
    ///
    /// The companion to `subpath-delete`, for the same CAD-export case: when
    /// one path object holds a whole drawing view, moving "this line" means
    /// moving one of its subpaths.
    ///
    /// Refused for a subpath that starts implicitly (a segment after `h`,
    /// whose start point is inherited rather than written) — translating the
    /// operands that exist would tear it away from a start that stayed put.
    SubpathMove {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index, as `object-list` prints it on an
        /// `object index=` row.
        ///
        /// Mutually exclusive with `--leaf`; pass exactly one.
        #[arg(long)]
        object: Option<usize>,
        /// 0-based FORM LEAF index -- an object drawn INSIDE a form XObject,
        /// as `object-list` prints it on a `leaf index=` row.
        ///
        /// A form has one set of bytes and may be drawn many times, so this
        /// edit changes every place that form appears. How many is printed on
        /// stderr; `unshare-form` gives one page its own copy first.
        #[arg(long)]
        leaf: Option<usize>,
        /// 0-based subpath index within that object.
        #[arg(long)]
        subpath: usize,
        /// Page-space x displacement, in points.
        #[arg(long, allow_hyphen_values = true)]
        dx: f64,
        /// Page-space y displacement, in points.
        #[arg(long, allow_hyphen_values = true)]
        dy: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Export a page's vector geometry as DXF** — the format SOLIDWORKS,
    /// AutoCAD and plasma-table controllers import natively.
    ///
    /// WHY THIS EXISTS. SOLIDWORKS gates its own PDF import on Adobe Acrobat
    /// or Illustrator being installed and licensed. It imports DXF with no
    /// Adobe dependency at all — so this does not work around that gate, it
    /// makes it irrelevant.
    ///
    /// SCALE IS THE THING TO GET RIGHT. A PDF drawing is at PAPER scale, so a
    /// 1:2 detail view exports at half size and looks entirely plausible.
    /// Pass `--scale 2` for a 1:2 view. Every generic PDF-to-DXF converter
    /// skips this and says nothing.
    ///
    /// Circles and arcs are RECOGNISED, not flattened: PDF has no arc
    /// primitive, so a hole arrives as four Bezier curves, and emitting those
    /// as fine polylines is what turns forty washers into a 767 KB file.
    /// `--no-fit-arcs` disables it if you want the curves verbatim.
    ///
    /// The output carries no `MATERIAL` object and no group code 94, so it
    /// loads in AutoCAD LT 2004 and older CAM controllers that reject both.
    ///
    /// NOT A ROUND TRIP TO A MODEL. A PDF of a CAD drawing is printed output
    /// — derived geometry. You get sketch entities, never features and never
    /// dimensions as constraints.
    ExportDxf {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number. One page, one `--output` file.
        #[arg(long, default_value_t = 1, conflicts_with = "pages")]
        page: u32,
        /// 1-based pages to export, one DXF each into `--output-dir`:
        /// `all`, `3`, `1-4`, `5,1-2`.
        ///
        /// Files are named `<stem>_p<n>.dxf`, zero-padded to the widest
        /// page number in the run so they sort in page order — the same
        /// naming the GUI's multi-page export uses, deliberately, so a
        /// batch script and an operator produce interchangeable output.
        ///
        /// The drawing scale is derived from the ce dimensions on **all**
        /// the selected pages together, because one `--scale` serves the
        /// whole run. Pages at different scales are therefore a REFUSAL,
        /// exactly as two disagreeing groups on one page are: export them
        /// in separate runs, or pass `--scale`.
        #[arg(long, conflicts_with = "page")]
        pages: Option<String>,
        /// Output `.dxf` path (single-page mode).
        #[arg(short, long, conflicts_with = "output_dir")]
        output: Option<PathBuf>,
        /// Existing directory to write one DXF per page into
        /// (multi-page mode; requires `--pages`).
        #[arg(long, conflicts_with = "output", requires = "pages")]
        output_dir: Option<PathBuf>,
        /// Output units.
        #[arg(long, value_enum, default_value_t = DxfUnitArg::In)]
        units: DxfUnitArg,
        /// Drawing scale — real-world units per paper unit. `2` for a 1:2
        /// view, `0.5` for a 2:1 view.
        ///
        /// OMIT IT and pdfcer derives the scale from the ce dimensions
        /// already on the page: if the drawing has been calibrated with the
        /// measure tool's "scale by known dimension", that answer is exactly
        /// what this needs, and the derived figure is printed before the
        /// file is written. With nothing calibrated, the export falls back
        /// to paper scale and says so loudly. If two dimension groups
        /// disagree — a 1:1 plan and a 1:5 detail on one sheet — the export
        /// is REFUSED and both are listed, because DXF carries one scale and
        /// picking either silently exports half the sheet wrong.
        #[arg(long)]
        scale: Option<f64>,
        /// Emit Bezier curves verbatim as SPLINEs instead of recognising
        /// circles and arcs.
        #[arg(long)]
        no_fit_arcs: bool,
        /// Leave the page's text out of the DXF entirely.
        ///
        /// By default each text run becomes a TEXT entity on its own layer
        /// (`PDFCER_TEXT`), so the drawing's dimensions and notes are
        /// readable and can be switched off in one click without touching
        /// the geometry. Pass this when the destination is a cutting table
        /// and any stray entity is a hazard.
        #[arg(long)]
        no_text: bool,
    },
    /// **Delete ONE text run** — one show operator — out of a text object
    /// (`Pass 32.0`, ISO 32000-1 §9.4).
    ///
    /// The text-side twin of `subpath-delete`. Deletion is otherwise
    /// object-granular, and a CAD exporter puts every label on a sheet inside
    /// ONE `BT`...`ET` — measured on a real drawing, one text object holding
    /// all 237 dimension labels — so deleting "a label" deleted all of them.
    ///
    /// `--run` is 0-based in content order, the same numbering `object-list`
    /// reports as `runs=`.
    ///
    /// REFUSED when the FOLLOWING run has no position of its own. §9.4.2
    /// leaves the pen advanced past the string just drawn, so such a run
    /// starts wherever this one ends; removing this one would slide it, in an
    /// edit that round-trips and passes `--verify-undo` and is still wrong.
    /// The remedy is in the message and always works: delete the later run
    /// first.
    ///
    /// Deleting the only run deletes the text object.
    TextRunDelete {
        /// Input PDF.
        input: PathBuf,
        /// 0-based paint-order object index on the page.
        ///
        /// Pass exactly one of this and `--leaf`.
        #[arg(long)]
        object: Option<usize>,
        /// 0-based index into this page's form leaves, to delete a run inside
        /// a form XObject (`G017`).
        ///
        /// On a SolidWorks set the title block IS a form, drawn on every
        /// sheet — so this is where most of a drawing's text actually lives,
        /// and until now nothing inside one could be deleted at all. The
        /// form's stream is SHARED: one delete removes the run from every
        /// page the form is drawn on, and the reach is printed.
        #[arg(long)]
        leaf: Option<usize>,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based show-operator index within that text object, content order.
        #[arg(long)]
        run: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Move a text run** — one show operator, or several as one edit —
    /// inside a text object (`G017`, `G030`, ISO 32000-1 §9.4).
    ///
    /// The twin `text-run-delete` has long implied this verb, and a text run
    /// was the last part kind in pdfcer to be missing one: a subpath and an
    /// anchor could each already be moved and deleted.
    ///
    /// The operator's case: one `BT`...`ET` on a SolidWorks title block holds
    /// every string in it — the sibling case measures at 237 dimension labels
    /// in one text object — so nudging one line of a title block meant moving
    /// the whole block or nothing.
    ///
    /// `--dx`/`--dy` are a PAGE-space displacement in points, exactly as for
    /// `subpath-move`. `--run` is 0-based in content order, the same numbering
    /// `object-list` reports as `runs=`.
    ///
    /// REFUSED, before any mutation, in two cases, both §9.4.2: when the run
    /// itself has no position of its own (it starts where the previous string
    /// left the pen, and that position is written nowhere in the file), and
    /// when the run AFTER it does not (moving this one would drag that one
    /// along). Both name the same remedies: list the neighbouring run too, or
    /// move the whole text object. With several runs listed, only a run
    /// whose neighbour is NOT listed is refused — so a whole line moves.
    ///
    /// Where the producer wrote no operand that could be adjusted — a `TD`,
    /// whose second operand IS the leading, or a bare `T*` — a positioning
    /// operator is ADDED and the addition is disclosed on stderr. The text
    /// goes exactly where asked either way; what differs is that the file now
    /// records the position differently from the way its producer did.
    TextRunMove {
        /// Input PDF.
        input: PathBuf,
        /// 0-based paint-order object index on the page.
        ///
        /// Pass exactly one of this and `--leaf`.
        #[arg(long)]
        object: Option<usize>,
        /// 0-based index into this page's form leaves, to move a run inside a
        /// form XObject. The form's stream is SHARED — the run moves on every
        /// page the form is drawn on, and the reach is printed.
        #[arg(long)]
        leaf: Option<usize>,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based show-operator index within that text object, content order.
        ///
        /// Repeat it, or separate indices with commas and no spaces, to move
        /// several runs together as one edit — the way to move a whole line.
        /// A run that inherits its position is then accepted when the run
        /// before it is listed too.
        #[arg(long, required = true, num_args = 1.., value_delimiter = ',')]
        run: Vec<usize>,
        /// Page-space horizontal displacement, in points.
        #[arg(long, allow_negative_numbers = true)]
        dx: f64,
        /// Page-space vertical displacement, in points.
        #[arg(long, allow_negative_numbers = true)]
        dy: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Set a text run's width** — stretch or squeeze one show operator to
    /// an exact width on the page by setting its horizontal scaling (`G038`,
    /// ISO 32000-1 §9.3.4, `Tz`).
    ///
    /// The case this is for: an OCR word whose recognised box is wider or
    /// narrower than the glyphs laid into it, so a selection highlight or a
    /// search hit does not line up with the scanned word underneath.
    ///
    /// `--width` is in PAGE points, measured along the run's own baseline, so
    /// a rotated or scaled run gets the width asked for on the page. The
    /// scale is absolute: a run that already has a horizontal scaling gets
    /// the same result as one that has none, and running this twice with the
    /// same width changes nothing the second time.
    ///
    /// The run keeps its origin, its font, its size and its render mode (an
    /// invisible OCR word stays invisible). Nothing after it moves: a run that
    /// starts where this one ends is held in place.
    ///
    /// REFUSED, before any mutation: a width that is zero, negative or not a
    /// number; a run whose `TJ` carries kerning; a run whose font has no
    /// advance widths to measure; and a run whose text or page transform is
    /// singular, so it has no baseline to measure along.
    TextRunWidth {
        /// Input PDF.
        input: PathBuf,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based show-operator index within that text object, content order,
        /// the same numbering `object-list` reports as `runs=`.
        #[arg(long)]
        run: usize,
        /// The width to fit the run to, in page points.
        #[arg(long, allow_negative_numbers = true)]
        width: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Merge consecutive text runs into one** — join the show operators an
    /// OCR engine or a producer split a word or line into, so it selects,
    /// searches and edits as one run (`G035`, ISO 32000-1 §9.4.3).
    ///
    /// The first run keeps its origin, font, size and render mode (an
    /// invisible OCR word stays invisible). The text of every listed run is
    /// joined, with `--separator` between each pair, and the merged run is
    /// scaled with `Tz` so it spans from the first run's start to the last
    /// run's end (`--fit span`, the default). `--fit natural` keeps the first
    /// run's own scaling instead, so the merged run may be shorter or longer
    /// than the runs it replaces.
    ///
    /// Runs after the merge renumber down by one less than the number merged.
    ///
    /// REFUSED, before any mutation: fewer than two runs; runs that are not
    /// consecutive and ascending; a run after the merge that inherits its
    /// position (it would move); runs that differ in font, size, spacing,
    /// rise, render mode or colour; a marked-content boundary between them; a
    /// `'` or `"` show; and a composite (Type0) font.
    TextRunMerge {
        /// Input PDF.
        input: PathBuf,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based show-operator indices to merge, consecutive and ascending,
        /// comma-separated or repeated — the numbering `object-list` reports
        /// as `runs=`.
        #[arg(long, required = true, num_args = 1.., value_delimiter = ',')]
        run: Vec<usize>,
        /// Text written between each pair of runs: `none` (the default),
        /// `space`, or any other literal text.
        #[arg(long, default_value = "none")]
        separator: String,
        /// How wide the merged run is.
        #[arg(long, value_enum, default_value_t = MergeFitArg::Span)]
        fit: MergeFitArg,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Cut one text object into several** so each line can be moved and
    /// styled on its own (`Pass 306.0`, ISO 32000-1 §9.4).
    ///
    /// A CAD exporter may put every string on a sheet inside ONE `BT`...`ET`,
    /// and SolidWorks does — a measured drawing has one absolute `Tm` followed
    /// by a chain of relative `Td` steps covering the whole page. Two things
    /// follow. A *line* is not addressable at all: the object is the whole
    /// sheet and a run is one show operator, with no handle between them. And
    /// neighbours are coupled, because `Td` is relative — which is why
    /// `text-run-move` has to compensate the run after the one it moves, and
    /// refuses outright when that one has no position of its own.
    ///
    /// This removes the coupling instead of compensating for it. After a split
    /// each piece is an ordinary text object: move it with `object-move`,
    /// recolour it, delete it, or reflow it.
    ///
    /// Choose the cuts one of two ways:
    ///
    /// - `--granularity run` — one new text object per show operator.
    /// - `--granularity line` — a new object wherever the baseline changes
    ///   between consecutive show operators, or clear space (or a jump
    ///   backwards) separates them, so a table row splits into its cells.
    ///   This is an INFERENCE (the file
    ///   does not record where its lines are, §14.8) and is disclosed on
    ///   stderr with the count.
    /// - `--before N` (repeatable) — cut before exactly these runs, 0-based in
    ///   content order, the same numbering `object-list` reports as `runs=`.
    ///   Overrides `--granularity`.
    ///
    /// The mechanism inserts `ET BT <the run's own Tm>` before each named run
    /// and changes nothing else — every original operator keeps its bytes, its
    /// order and its paint position, so the page renders identically.
    ///
    /// REFUSED, before any mutation: a cut before run 0 (it divides nothing);
    /// a cut before a run with no position of its own (§9.4.2 — it starts
    /// where the previous string left the pen, so there is no origin to
    /// re-state); a cut before a run shown by `'` or `"` (those move the line
    /// and then show, and the injected `Tm` would apply that move twice); and
    /// a cut inside a marked-content sequence opened within the same text
    /// object (§14.6 requires the two nest). The whole split is refused rather
    /// than part of it.
    ///
    /// Object indices after the split target SHIFT by the number of cuts.
    TextObjectSplit {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// Where to cut, when `--before` is not given.
        #[arg(long, value_enum, default_value_t = SplitGranularityArg::Line)]
        granularity: SplitGranularityArg,
        /// Cut before this 0-based run; repeatable. Overrides `--granularity`.
        #[arg(long)]
        before: Vec<usize>,
        /// Print the cut points and the disclosure, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Output path. Required unless `--dry-run`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Delete one subpath** of a path object (Pass 25.2): remove a single
    /// subpath's construction operators via surgery (R46/§5.7), leaving the
    /// object's other subpaths byte-verbatim.
    ///
    /// This is the operation for CAD output. A producer routinely emits a whole
    /// drawing view as ONE path object — a measured SolidWorks export has a
    /// single stroked path holding 1194 subpaths for one isometric view — so
    /// `object-delete` there removes the entire view. This removes one line of
    /// it. Find the index with
    /// `object-list --hit X,Y --enter OBJECT`, which prints `subpath-hit` lines
    /// nearest-first.
    ///
    /// NOT redaction: it removes a drawing element from a page, not covered
    /// content for security.
    ///
    /// Refused, by name and before any mutation, when the path defines a
    /// clipping region (deleting part of it would change what OTHER content is
    /// visible), and when the subpaths found in the operators disagree in count
    /// with the geometry (the index could then name a different line from the
    /// one intended). Deleting the only subpath deletes the object.
    SubpathDelete {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// 0-based subpath index within that object, in decomposition order —
        /// the same order `object-list --enter` reports.
        #[arg(long)]
        subpath: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Delete a node** of a path object (Pass 36.1): remove ONE anchor via
    /// surgery (R46/§5.7), joining its neighbours directly. `--node` is the
    /// anchor's 0-based index in decomposition order — the same numbering
    /// `node-move` takes.
    ///
    /// The segment operator that produced the anchor is excised. When the
    /// anchor is its subpath's FIRST, the following operator is rewritten into
    /// the new `m` instead; if that follower was a curve, its control points
    /// go with it and the loss is disclosed on stderr.
    ///
    /// Refused, by name and before any mutation, when the removal would leave
    /// a part with fewer than two points (delete the part instead), when the
    /// anchor is a corner of an `re` rectangle (no operand names it, and the
    /// result would be a triangle), when it is the inherited start of an
    /// `h`-reopened subpath (its coordinates belong to the part before it),
    /// and when the path defines a clipping region.
    NodeDelete {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// 0-based anchor node index (decomposition order).
        #[arg(long)]
        node: usize,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Drag a node** of a path object (Pass 9c-min, decision 011 §2.5):
    /// move ONE anchor to a page-space point via surgery (R46/§5.7).
    /// `--node` is the anchor's 0-based index in decomposition order (start,
    /// then each segment endpoint, across subpaths). Every anchor is
    /// draggable, including an `re` rectangle corner and the inherited start
    /// of a subpath reopened after `h` — each of which has no operand of its
    /// own, so one is materialized and the change of form is disclosed on
    /// stderr (Pass 30.0).
    NodeMove {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page, as `object-list`
        /// prints it on an `object index=` row.
        ///
        /// Mutually exclusive with `--leaf`; pass exactly one.
        #[arg(long)]
        object: Option<usize>,
        /// 0-based FORM LEAF index -- an anchor of a path drawn INSIDE a form
        /// XObject, as `object-list` prints it on a `leaf index=` row.
        ///
        /// `--x`/`--y` stay in PAGE space; the conversion into the form's own
        /// coordinates uses the placement matrix `object-list` prints beside
        /// the leaf. A form drawn many times is edited in every one of them,
        /// and the count is printed on stderr.
        #[arg(long)]
        leaf: Option<usize>,
        /// 0-based anchor node index (decomposition order).
        #[arg(long)]
        node: usize,
        /// New anchor x, page space (points).
        #[arg(long, allow_hyphen_values = true)]
        x: f64,
        /// New anchor y, page space (points).
        #[arg(long, allow_hyphen_values = true)]
        y: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Move SEVERAL anchors of one path object at once**, as a single
    /// undoable surgery (`Pass 23.3`).
    ///
    /// The batch form of `node-move`. Repeat `--move NODE,X,Y` once per
    /// anchor; every anchor named goes to its own absolute page-space point,
    /// so this expresses a rigid translation and an arbitrary re-shaping
    /// equally well — nothing here requires the targets be a uniform offset.
    ///
    /// WHY THIS IS NOT JUST A LOOP OVER `node-move`. Two reasons, and only
    /// the first is about convenience:
    ///
    /// - One command, so ONE undo entry. A loop leaves N of them, and undoing
    ///   a batch then means pressing undo N times and knowing what N was.
    /// - Anchors that share an OPERATOR are rewritten together. All four
    ///   corners of a rectangle are the same four operands of one `re`, and an
    ///   `h`-reopened subpath's implicit start shares its byte range with the
    ///   segment that inherits it — cases where two independent edits would
    ///   overlap, and an overlapping edit is silently dropped rather than
    ///   applied.
    ///
    /// REFUSED, all before any byte changes: naming no anchors at all; naming
    /// the same anchor twice (last-wins and first-wins are equally defensible
    /// and give different geometry, so pdfcer will not pick one for you); and
    /// any index the object does not have — which refuses the WHOLE batch,
    /// never a partial application.
    ///
    /// Disclosures go to stderr, as `node-move`'s do, so the stdout record
    /// stays machine-parseable. They are DE-DUPLICATED: rewriting three
    /// rectangles says so once, not three times.
    NodesMove {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// One anchor's destination, as `NODE,X,Y` — a 0-based anchor index
        /// (decomposition order, as `object-list` counts them) and an
        /// absolute page-space point. Repeat once per anchor.
        // NOT `num_args = 1..`: that makes the flag GREEDY, so
        // `--move 0,1,2 -o out.pdf` swallows `-o` and `out.pdf` as further
        // move tokens and the command dies reporting `--output` missing.
        // A `Vec` field already appends on repeat, which is the behaviour
        // wanted — one value per occurrence, occur as often as you like.
        #[arg(
            long = "move",
            value_name = "NODE,X,Y",
            required = true,
            allow_hyphen_values = true
        )]
        moves: Vec<String>,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },
    /// **Drag a curve handle** of a path object (Pass 30.1): move one Bézier
    /// control point, leaving the on-curve node itself where it is. This is
    /// the operation that changes a curve's SHAPE — `node-move` can only move
    /// the points a curve passes through.
    ///
    /// `--side incoming` is the control point governing the curve as it
    /// ARRIVES at the node, `--side outgoing` as it LEAVES. A straight
    /// segment has no handle and is refused rather than silently turned into
    /// a curve. `v` and `y` operators leave one control point implied by
    /// another point (§8.5.2.1 Table 59); dragging that one re-spells the
    /// segment as `c` and discloses it on stderr.
    HandleMove {
        /// Input PDF.
        input: PathBuf,
        /// 1-based page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// 0-based paint-order object index on the page.
        #[arg(long)]
        object: usize,
        /// 0-based anchor node index (decomposition order).
        #[arg(long)]
        node: usize,
        /// Which of the node's two handles to move.
        #[arg(long, value_enum)]
        side: HandleArg,
        /// New control-point x, page space (points).
        #[arg(long, allow_hyphen_values = true)]
        x: f64,
        /// New control-point y, page space (points).
        #[arg(long, allow_hyphen_values = true)]
        y: f64,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Save mode.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Reload and verify the edit undoes byte-identically.
        #[arg(long)]
        verify_undo: bool,
    },

    /// Place a raster image on a page as an image XObject (ISO 32000-1 §8.9.5).
    ///
    /// PNG, JPEG and BMP are placed. Anything else is refused BY NAME, with a
    /// message saying which formats do work — never a silent failure and
    /// never a wrong-looking placement.
    ///
    /// NOTHING IS RE-ENCODED THAT DOES NOT HAVE TO BE. A JPEG's codestream is
    /// embedded byte for byte behind `/DCTDecode`, so placing a scan or a
    /// photograph costs no quality; a non-interlaced PNG's compressed image
    /// data is reused verbatim behind `/FlateDecode` with `/Predictor 15`.
    /// pdfcer decodes and re-compresses only where PDF cannot express the
    /// source's layout — a PNG with an interleaved alpha channel (which
    /// becomes a base image plus a separate `/SMask`), or a BMP (which has no
    /// compressed form at all). Every such case is reported.
    ///
    /// `--compression jpeg` is the one way to ask for a re-encode anyway. It
    /// is lossy by definition and is never chosen for you.
    AddImage {
        /// Input PDF.
        input: PathBuf,
        /// The image file to place: PNG, JPEG or BMP.
        #[arg(long, value_name = "FILE")]
        image: PathBuf,
        /// 1-based page number to place the image on.
        #[arg(long)]
        page: usize,
        /// The rectangle to place the image in, `llx,lly,urx,ury`, in PDF
        /// user space (points, origin at the page's lower-left).
        ///
        /// By default the image keeps its shape and is CENTRED inside this
        /// rectangle, so one axis may end up smaller than asked — PDF itself
        /// preserves no aspect ratio (§8.9.4), so pdfcer has to choose, and a
        /// distorted picture is a defect nobody asked for. Pass `--stretch`
        /// to fill the rectangle exactly instead.
        #[arg(long, value_name = "LLX,LLY,URX,URY", allow_hyphen_values = true)]
        rect: String,
        /// Fill `--rect` exactly, distorting the aspect ratio if it differs.
        ///
        /// The right answer when the rectangle came from a measurement —
        /// fitting a scan to a known paper size, replacing a stamp of fixed
        /// extent — rather than from a freehand drag.
        #[arg(long)]
        stretch: bool,
        /// Replace `--rect`'s SIZE with the image's natural size, keeping its
        /// lower-left corner.
        ///
        /// Natural size comes from the resolution the image file declares (a
        /// PNG `pHYs` chunk, a JFIF density, EXIF `XResolution`, a BMP's
        /// pixels-per-metre). When the file declares none, one pixel becomes
        /// one point (72 dpi) — and the reported `dpi_source=` says which of
        /// the two happened, so an assumed resolution is never mistaken for a
        /// declared one.
        ///
        /// Not the default: applying an embedded resolution silently would
        /// make the same picture land at wildly different sizes depending on
        /// metadata the operator never saw.
        #[arg(long, conflicts_with = "stretch")]
        natural: bool,
        /// How the image's pixels are stored in the PDF.
        ///
        /// `passthrough` (the DEFAULT) embeds the source's own compressed
        /// bytes unchanged — a JPEG's codestream verbatim, a PNG's compressed
        /// image data verbatim. Nothing is re-encoded, so nothing is
        /// degraded. Where a source has no compressed form to keep (a BMP) or
        /// its layout cannot be expressed in PDF (a PNG with an interleaved
        /// alpha channel), the result is lossless compression instead, and the
        /// reported `compression_applied=` says so.
        ///
        /// `lossless` stores the decoded samples with lossless compression.
        /// On a PNG or BMP this changes nothing. ON A JPEG IT RECOVERS
        /// NOTHING — it preserves exactly the pixels the JPEG decodes to,
        /// artefacts included, while typically multiplying the stored size
        /// several-fold. Useful before further editing; not a quality
        /// improvement.
        ///
        /// `jpeg` RE-ENCODES the image lossily at `--quality`. This is the
        /// only policy that degrades the picture, and it does so on purpose.
        /// ON A SOURCE THAT WAS ALREADY A JPEG IT IS A SECOND LOSSY PASS:
        /// the DCT runs again over the artefacts the first one left, which
        /// COMPOUNDS them rather than adding one predictable generation of
        /// loss, and no quality setting undoes that. The reported
        /// `jpeg_from_lossy=1` says when this happened; the honest fix is
        /// usually to place the original file instead. A transparent colour
        /// (a PNG `tRNS` on a truecolour image) is refused by name rather
        /// than re-encoded, because lossy encoding moves the exact sample
        /// values that transparency is matched against.
        ///
        /// Resolution capping ("downsample to N dpi") is still absent, but no
        /// longer for want of an encoder: a resampler is a visible quality
        /// decision (box vs. Lanczos) that deserves its own flag and its own
        /// disclosure, not a silent choice hidden inside this one.
        #[arg(long, value_enum, default_value_t = CompressionArg::Passthrough)]
        compression: CompressionArg,
        /// Encoder quality for `--compression jpeg`, 1-100. Ignored by every
        /// other policy.
        ///
        /// Larger is better and bigger. 100 is NOT lossless — it is the
        /// finest quantisation the scale defines, and on synthetic content
        /// (a screenshot, a CAD export) it routinely produces a file LARGER
        /// than `--compression lossless` would, while still losing detail.
        /// Values outside 1-100 are rejected here at parse time and, for
        /// library callers, refused by name rather than clamped.
        #[arg(long, default_value_t = 85, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
        /// Output path.
        #[arg(short, long)]
        output: PathBuf,
        /// Which save path to use.
        #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
        mode: SaveMode,
        /// Also verify that undoing the placement reproduces the input byte
        /// for byte.
        #[arg(long)]
        verify_undo: bool,
    },
}
