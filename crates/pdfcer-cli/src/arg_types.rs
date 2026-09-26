use super::*;

/// `--compression` for [`Command::AddImage`].
///
/// A `clap`-side mirror of [`pdfcer_core::image_import::ImageCompression`]
/// rather than a re-export, for the reason [`HandleArg`] gives: the core type
/// is not a `ValueEnum`, and making it one would put a CLI-parsing concern
/// into the GUI-free core crate. It also lets the CLI carry `--quality` as a
/// separate flag while the core type carries it inside the variant.
/// How deep `object-list --hit` looks.
///
/// # Two answers exist and both are shipped, per standing rule `R206`
///
/// The consuming shell asked for either a changed `--hit` or a separate
/// `--hit-deep` and said it had no preference it could justify. Making it a
/// mode rather than a second flag keeps ONE query path with a switch on it,
/// instead of two flags whose implementations can drift apart — which is the
/// exact failure this whole area has produced twice.
///
/// The DEFAULT is the one the GUI does, because this flag's documented job is
/// to be authoritative about the GUI's behaviour, and a default that is not
/// that makes the documentation false again the moment anyone reads it.
/// Whether a structure dump includes stream data, and in what form.
///
/// The shell mirror of `pdfcer_core::structure::StreamMode`. Deliberately a
/// separate type rather than a `ValueEnum` derive on the core enum: `clap` is a
/// GUI-adjacent concern and `pdfcer-core` does not depend on it, which is the
/// crate-separation invariant rather than a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StreamDump {
    /// Report each stream's dictionary and its length; omit the data. Default.
    Omit,
    /// The bytes as stored, still encoded — what you want when the filter
    /// itself is under suspicion.
    Raw,
    /// The bytes after every filter in /Filter has run.
    Decoded,
}

impl From<StreamDump> for pdfcer_core::structure::StreamMode {
    fn from(v: StreamDump) -> Self {
        match v {
            StreamDump::Omit => Self::Omit,
            StreamDump::Raw => Self::Raw,
            StreamDump::Decoded => Self::Decoded,
        }
    }
}

/// The shell mirror of `pdfcer_core::ocr::layer::ExistingLayers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ExistingOcrArg {
    /// Take the earlier layer off and write the new one. The default.
    Replace,
    /// Leave the file alone and exit with an error.
    Refuse,
    /// Keep the earlier layer and add the new one beside it.
    Stack,
}

impl From<ExistingOcrArg> for pdfcer_core::ocr::layer::ExistingLayers {
    fn from(v: ExistingOcrArg) -> Self {
        match v {
            ExistingOcrArg::Replace => Self::Replace,
            ExistingOcrArg::Refuse => Self::Refuse,
            ExistingOcrArg::Stack => Self::Stack,
        }
    }
}

/// `ocr --ocr-engine`. Both variants exist in every build, so a build
/// without the `ocrcer` feature refuses the choice by name instead of
/// rejecting an unknown value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum OcrEngineArg {
    /// The ocrs engine. The default; its models ship in the portable package.
    Ocrs,
    /// The OCRcer engine. Needs a build with the ocrcer feature and the model file ocrcer.ocrw.
    Ocrcer,
    /// The Tesseract program in models/tesseract. Choose languages with --ocr-lang.
    Tesseract,
}

impl OcrEngineArg {
    /// The engine's name: its `models/<name>` folder and the layer marker.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Ocrs => "ocrs",
            Self::Ocrcer => "ocrcer",
            Self::Tesseract => tesseract::MODEL_DIR,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum HitScope {
    /// Descend into form XObjects; never name a form itself. The GUI's
    /// behaviour, and the default.
    Deep,
    /// The page's own object list only — a shallow query, in which a
    /// page-sized form wins every click.
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CompressionArg {
    /// Embed the source's own compressed bytes unchanged. The default.
    Passthrough,
    /// Store the decoded samples with lossless compression.
    Lossless,
    /// Re-encode as JPEG at --quality — lossy, on purpose, and a SECOND
    /// lossy pass if the source was already a JPEG.
    Jpeg,
}

impl CompressionArg {
    /// The core policy this argument selects.
    pub(crate) fn policy(self, quality: u8) -> pdfcer_core::image_import::ImageCompression {
        use pdfcer_core::image_import::ImageCompression;
        match self {
            Self::Passthrough => ImageCompression::Passthrough,
            Self::Lossless => ImageCompression::Lossless,
            Self::Jpeg => ImageCompression::Jpeg { quality },
        }
    }
}

/// Which of a node's two Bézier handles [`Command::HandleMove`] moves.
///
/// A `clap`-side mirror of [`pdfcer_core::vector::Handle`] rather than a
/// re-export: the core type is not a `ValueEnum`, and making it one would put
/// a CLI-parsing concern into the GUI-free core crate.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum HandleArg {
    /// The handle shaping the curve as it ARRIVES at the node.
    Incoming,
    /// The handle shaping the curve as it LEAVES the node.
    Outgoing,
}

impl HandleArg {
    /// The core enum this stands for.
    pub(crate) const fn to_core(self) -> pdfcer_core::vector::Handle {
        match self {
            HandleArg::Incoming => pdfcer_core::vector::Handle::Incoming,
            HandleArg::Outgoing => pdfcer_core::vector::Handle::Outgoing,
        }
    }

    /// A stable token for CLI output.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            HandleArg::Incoming => "incoming", // ui-text-exempt: stable output token
            HandleArg::Outgoing => "outgoing", // ui-text-exempt: stable output token
        }
    }
}

/// Which dimension kind [`Command::DimensionAdd`] authors. Radius and diameter
/// share one Taubin fit and differ only in DISPLAY (decision 011 §2.3).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum DimKindArg {
    /// A linear (distance) dimension between the first two points.
    Linear,
    /// A radius dimension over a best-fit circle.
    Radius,
    /// A diameter dimension over a best-fit circle (2×radius).
    Diameter,
    /// TWO LINES — pdfcer decides which dimension they call for.
    ///
    /// Takes FOUR points: the first two are one line's endpoints, the second
    /// two are the other's. What gets authored depends on the geometry, which
    /// is the whole point of the mode:
    ///
    /// - parallel (within the parallel_epsilon_degrees setting) → a
    ///   LINEAR ce dimension of the perpendicular distance between them.
    /// - at an angle → an ANGULAR ce dimension of the angle between them.
    /// - collinear → refused by name, because a zero-distance dimension
    ///   is not a drawing anyone wanted.
    ///
    /// --treat-as-parallel forces the first reading regardless of the
    /// measured angle — the CLI form of the checkbox the operator asked for,
    /// for a pair he knows is nominally parallel and that arrived a fraction
    /// off from an exporter's rounding.
    TwoLines,
    /// A closed perimeter over every supplied point: the sum of all its
    /// segments including the one from the last point back to the first,
    /// printed as one number.
    ///
    /// Needs at least three points. Use --offset/--text-along to displace
    /// the label from the shape's vertex centroid, in page axes.
    Perimeter,
    /// An open path length over every supplied point — the same
    /// measurement without the closing segment: a pipe run, a cable
    /// route, a kerb line that does not come back on itself.
    ///
    /// Needs at least two points. This is one kind with
    /// [DimKindArg::Perimeter], not a different one; they differ by exactly
    /// the closing segment, which is why they share every other option.
    Path,
}

impl DimKindArg {
    /// A stable token for CLI output.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            DimKindArg::Linear => "linear",
            DimKindArg::Radius => "radius",
            DimKindArg::Diameter => "diameter",
            // The token reports what was ASKED for. What was AUTHORED is
            // reported separately by the handler, because for this mode they
            // legitimately differ — that is the feature, not a discrepancy,
            // and a report that showed only one of them would hide the
            // decision pdfcer made.
            DimKindArg::TwoLines => "two-lines",
            DimKindArg::Perimeter => "perimeter",
            DimKindArg::Path => "path",
        }
    }
}

/// Which reading [`Command::DimensionDisplay`] switches a placed circular ce
/// dimension to (Pass 34.2).
///
/// # Why this is a SECOND enum rather than a reuse of [`DimKindArg`]
///
/// [`DimKindArg`] answers "what kind of ce dimension am I authoring", and its
/// `Linear` variant is a legitimate answer to that question. Here `Linear` is
/// precisely the case being refused — the verb only applies to a circular ce
/// dimension. Reusing the wider enum would make `--show linear` parse cleanly
/// and then fail at runtime, which is a worse experience than clap refusing it
/// at the argument boundary with the two valid values listed.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum DisplayReading {
    /// Print the fitted radius.
    Radius,
    /// Print the diameter (2×radius) of the same fitted circle.
    Diameter,
}

impl DisplayReading {
    /// `true` when the label should print the diameter — the shape
    /// [`pdfcer_core::edit::EditSession::set_dimension_display`] takes.
    pub(crate) const fn show_diameter(self) -> bool {
        matches!(self, DisplayReading::Diameter)
    }

    /// A stable token for CLI output.
    pub(crate) const fn token(self) -> &'static str {
        match self {
            DisplayReading::Radius => "radius",
            DisplayReading::Diameter => "diameter",
        }
    }
}

/// The linear alignment constraint for [`Command::DimensionAdd`].
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum ConstraintArg {
    /// Free Euclidean direction.
    Aligned,
    /// Project onto the page X axis (measured length |Δx|).
    Horizontal,
    /// Project onto the page Y axis (measured length |Δy|).
    Vertical,
}

impl ConstraintArg {
    /// The `pdfcer_core` constraint this maps to.
    pub(crate) const fn to_core(self) -> pdfcer_core::vector::AxisConstraint {
        match self {
            ConstraintArg::Aligned => pdfcer_core::vector::AxisConstraint::Aligned,
            ConstraintArg::Horizontal => pdfcer_core::vector::AxisConstraint::Horizontal,
            ConstraintArg::Vertical => pdfcer_core::vector::AxisConstraint::Vertical,
        }
    }
}

/// Which save path `round-trip` exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum RoundTripMode {
    /// §7.5.6 incremental save with an empty dirty set. Promises
    /// whole-file byte identity: zero edits means zero bytes.
    Incremental,
    /// Full rewrite. Promises per-object-definition byte identity, a
    /// reloadable file, and an identical raster — never whole-file
    /// identity, because object offsets legitimately move.
    Full,
    /// §7.5.6 incremental save that re-emits every object of the
    /// base revision unchanged, exercising the real append writer.
    ///
    /// This is a verification mode, not an editing feature: no object's
    /// value changes, so the result is semantically identical to the
    /// input by construction. It exists because the incremental mode's
    /// empty-dirty-set path is a memcpy — without this, the §7.5.6
    /// append machinery (object re-emission, update-section
    /// construction, /Prev chaining, trailer copying) would ship with
    /// no corpus coverage at all.
    AppendIdentity,
}

/// Which image file `export-image` writes (`Pass 248.0`).
///
/// Two formats, one transparency story: PNG carries alpha and JPEG
/// cannot, so `--transparent` is legal for exactly one of them and the
/// refusal for the other is spelled out at the call site rather than
/// buried in an encoder that would happily flatten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ImageFormatArg {
    /// PNG — lossless RGBA8; keeps transparency with --transparent.
    Png,
    /// JPEG — lossy, always opaque. jpg is accepted as a spelling.
    #[value(alias = "jpg")]
    Jpeg,
    /// EMF — a Windows Enhanced Metafile for LibreOffice 24.x and legacy
    /// Win32 consumers: vectors where EMF has them, alpha bitmaps where it
    /// does not, every substitution counted.
    Emf,
    /// SVG — vector, resolution-free, transparent unless --background is
    /// given. --dpi governs only what has to be embedded as raster inside
    /// it.
    Svg,
}

impl ImageFormatArg {
    /// The file extension a batch export names its files with.
    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
            Self::Emf => "emf",
        }
    }

    /// The token printed on the stable line (`format=<png|jpeg>`).
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Svg => "svg",
            Self::Emf => "emf",
        }
    }
}

/// `export-image --svg-text` and `--emf-text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SvgTextArg {
    /// Every glyph a filled path.
    Outlines,
    /// Real text in an embedded subset font where possible.
    Keep,
}

impl From<SvgTextArg> for pdfcer_render::emf::EmfText {
    fn from(arg: SvgTextArg) -> Self {
        match arg {
            SvgTextArg::Outlines => Self::Outlines,
            SvgTextArg::Keep => Self::KeepText,
        }
    }
}

impl From<SvgTextArg> for pdfcer_render::svg::SvgText {
    fn from(arg: SvgTextArg) -> Self {
        match arg {
            SvgTextArg::Outlines => Self::Outlines,
            SvgTextArg::Keep => Self::KeepText,
        }
    }
}

/// `--units` for `export-dxf`, mapped to the DXF header's `$INSUNITS`.
///
/// Short names because they are typed: `--units mm` reads better than
/// `--units millimetres` and is what a drawing office would say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum DxfUnitArg {
    /// Inches ($INSUNITS 1).
    In,
    /// Millimetres ($INSUNITS 4).
    Mm,
}

/// `/Producer` policy, as a CLI value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ProducerArg {
    /// Write /Producer (pdfcer <version>) into an existing /Info.
    Set,
    /// Leave /Info byte-untouched (R41's no-fingerprint posture).
    Preserve,
}

/// How the CLI reads a comma in a stored field value.
///
/// Mirrors [`pdfcer_core::form_script::calc::CommaPolicy`]. Kept as its own
/// clap enum rather than deriving `ValueEnum` on the core type, so the core
/// crate gains no CLI dependency — the GUI-core separation applies to
/// argument parsing too.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CommaArg {
    /// A comma makes the value non-numeric, so it counts as a disclosed
    /// zero. The default: refusing to guess cannot turn 1,234 into 1.234.
    NotNumeric,
    /// A comma is the decimal separator (1,5 is 1.5).
    Decimal,
    /// A comma is the thousands separator (1,234 is 1234).
    Grouping,
}

impl From<CommaArg> for pdfcer_core::form_script::calc::CommaPolicy {
    fn from(arg: CommaArg) -> Self {
        match arg {
            CommaArg::NotNumeric => Self::NotNumeric,
            CommaArg::Decimal => Self::DecimalSeparator,
            CommaArg::Grouping => Self::GroupingSeparator,
        }
    }
}

/// `--residual-scope` on `redact-apply` and `redact-offpage`.
///
/// Mirrors [`pdfcer_core::redact::ResidualScope`]. Its own clap enum rather
/// than a `ValueEnum` derive on the core type, so `pdfcer-core` gains no CLI
/// dependency — the GUI-core separation applies to argument parsing too.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ResidualScopeArg {
    /// Act only on the marked regions. The residual sweep reports what it
    /// finds elsewhere and changes none of it.
    MarkedOnly,
    /// The default. Also scrub carriers the operator cannot see: /Info, XMP
    /// packets and string entries in arbitrary dictionaries. Text drawn on
    /// pages outside the marks is reported, not edited.
    HiddenCarriers,
    /// Also blank matching text wherever it is drawn, including on pages the
    /// operator never marked. The strongest absence guarantee and the most
    /// destructive.
    WholeDocument,
}

impl From<ResidualScopeArg> for pdfcer_core::redact::ResidualScope {
    fn from(arg: ResidualScopeArg) -> Self {
        match arg {
            ResidualScopeArg::MarkedOnly => Self::MarkedOnly,
            ResidualScopeArg::HiddenCarriers => Self::HiddenCarriers,
            ResidualScopeArg::WholeDocument => Self::WholeDocument,
        }
    }
}

/// `--border` on `add-text-field` (§12.5.4 Table 166).
///
/// Its own clap enum rather than a `ValueEnum` derive on the core type, so
/// `pdfcer-core` gains no CLI dependency — the GUI-core separation applies to
/// argument parsing too.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BorderArg {
    /// Solid rectangle. Table 166's default and pdfcer's.
    Solid,
    /// Dashed.
    Dashed,
    /// Beveled — solid with an embossed highlight.
    Beveled,
    /// Inset — solid with an engraved lowlight.
    Inset,
    /// Underline — a line along the bottom edge only.
    Underline,
}

impl From<BorderArg> for pdfcer_core::edit::BorderStyle {
    fn from(arg: BorderArg) -> Self {
        match arg {
            BorderArg::Solid => Self::Solid,
            BorderArg::Dashed => Self::Dashed,
            BorderArg::Beveled => Self::Beveled,
            BorderArg::Inset => Self::Inset,
            BorderArg::Underline => Self::Underline,
        }
    }
}

/// `--visibility` on `add-text-field` (§12.5.3 Table 165).
///
/// Four combinations, not eight bits. `hidden` and `print-only` are kept
/// distinct because Table 165 makes them different: `Hidden` suppresses
/// screen AND print "regardless of its annotation type", while `NoView`
/// suppresses only the screen and leaves printing to the `Print` flag.
/// Collapsing them would silently stop a field printing that the operator
/// asked to print.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum VisibilityArg {
    /// On screen and printed (/F 4). The default.
    Visible,
    /// On screen, never printed (/F 0).
    ScreenOnly,
    /// Printed, not shown on screen (/F 36).
    PrintOnly,
    /// Suppressed everywhere (/F 2).
    Hidden,
}

impl From<VisibilityArg> for pdfcer_core::edit::Visibility {
    fn from(arg: VisibilityArg) -> Self {
        match arg {
            VisibilityArg::Visible => Self::VisibleAndPrints,
            VisibilityArg::ScreenOnly => Self::ScreenOnly,
            VisibilityArg::PrintOnly => Self::PrintOnly,
            VisibilityArg::Hidden => Self::Hidden,
        }
    }
}

/// `--submit-format` on `set-button-action` (ISO 32000-1 Table 237 bits 3,
/// 6, 9).
///
/// The standard selects the format with a strict precedence chain rather than
/// independent bits — `SubmitPDF` ≻ `XFDF` ≻ `ExportFormat` — so this is one
/// choice, not three switches. `pdfcer-core`'s `SubmitFormat` carries the
/// format-specific flags with the format; the CLI's per-flag options are
/// folded into whichever variant this names, and options belonging to a
/// different format are **refused by name** rather than silently dropped.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SubmitFormatArg {
    /// Forms Data Format, by POST. The baseline, and what a zero flag word
    /// means. Carries this document's own path and identity fingerprint
    /// unless --exclude-document-path.
    Fdf,
    /// HTML form encoding. The only format that may use GET or send click
    /// coordinates.
    Html,
    /// XFDF — FDF expressed as XML.
    Xfdf,
    /// The ENTIRE document file. Ignores field selection completely; there is
    /// no partial-PDF submission.
    Pdf,
}

/// `--goto-view` on `set-button-action` (ISO 32000-1 Table 151).
///
/// Every coordinate is computed from the target page's own crop box, so none
/// of these takes a number: a caller who had to supply `top` in user space
/// would have to read the page box first, and would get it wrong on a
/// cropped page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum GotoViewArg {
    /// Fit the whole page in the window (/Fit).
    WholePage,
    /// Fit the page's full width, top edge at the top (/FitH).
    FullWidth,
    /// The page's top-left corner, current zoom retained (/XYZ).
    TopLeft,
}

/// `--named` on `set-button-action` (ISO 32000-1 Table 211).
///
/// The registry is open — *"further names may be added"* — but exactly these
/// four are defined in both editions, and an unrecognised name is the one
/// place the standard tells a reader to *"take no action"*. Authoring a fifth
/// would author a button that does nothing.
// The shared `Page` postfix is not repetition to be factored out: these four
// spellings are the standard's own (Table 211 `/NextPage`, `/PrevPage`,
// `/FirstPage`, `/LastPage`), and clap derives the operator-facing values
// `next-page` … `last-page` from them. Shortening to `Next`/`Prev` would put
// names in `--help` that appear nowhere in the format.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum NamedActionArg {
    /// Go to the next page.
    NextPage,
    /// Go to the previous page.
    PrevPage,
    /// Go to the first page.
    FirstPage,
    /// Go to the last page.
    LastPage,
}

impl From<NamedActionArg> for pdfcer_core::edit::NamedAction {
    fn from(arg: NamedActionArg) -> Self {
        match arg {
            NamedActionArg::NextPage => Self::NextPage,
            NamedActionArg::PrevPage => Self::PrevPage,
            NamedActionArg::FirstPage => Self::FirstPage,
            NamedActionArg::LastPage => Self::LastPage,
        }
    }
}

impl From<GotoViewArg> for pdfcer_core::edit::PageView {
    fn from(arg: GotoViewArg) -> Self {
        match arg {
            GotoViewArg::WholePage => Self::WholePage,
            GotoViewArg::FullWidth => Self::FullWidth,
            GotoViewArg::TopLeft => Self::TopLeft,
        }
    }
}

/// Which save path an **editing** subcommand uses.
///
/// Deliberately a separate enum from [`RoundTripMode`], which carries a
/// verification-only `append-identity` variant that has no meaning for
/// an edit — merging them would put a mode in `--help` that cannot do
/// what its name suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SaveMode {
    /// Append a §7.5.6 revision, leaving every prior byte intact. The
    /// default, and the only mode that preserves existing digital
    /// signatures (§12.8.1 NOTE 1).
    Incremental,
    /// Rewrite the file as a single revision. Smaller output, and it
    /// drops superseded revisions — but it destroys every existing
    /// signature, and it is refused outright for a hybrid-reference
    /// file (§7.5.8.4).
    Full,
}

/// `text-object-split`'s `--granularity`, the CLI face of
/// [`pdfcer_core::vector::SplitGranularity`].
///
/// A separate enum rather than a re-export because clap needs `ValueEnum`, and
/// deriving it on the core type would put a shell concern in `pdfcer-core` —
/// the one thing `ARCHITECTURE.md` §3 does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SplitGranularityArg {
    /// One new text object per show operator.
    Run,
    /// A new text object wherever the baseline changes between consecutive
    /// show operators, or clear space separates them (a table row splits into
    /// its cells). An inference (ISO 32000-1 §14.8), disclosed on stderr.
    Line,
}

impl SplitGranularityArg {
    /// The core enum this names.
    pub(crate) const fn to_core(self) -> pdfcer_core::vector::SplitGranularity {
        match self {
            Self::Run => pdfcer_core::vector::SplitGranularity::Run,
            Self::Line => pdfcer_core::vector::SplitGranularity::Line,
        }
    }

    /// The `granularity=` token on the stdout line — part of the stable output
    /// contract.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Line => "line",
        }
    }
}

impl SaveMode {
    /// The `mode=` token on the stdout line. Part of the stable output
    /// contract, so it is pinned by a test.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::Full => "full",
        }
    }
}

/// The form-data interchange format for `export-data` / `import-data`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum DataFormat {
    /// Forms Data Format (ISO 32000-1 §12.7.7) — a PDF-like data file.
    Fdf,
    /// XML Forms Data Format — the XML companion format.
    Xfdf,
    /// Two-column name,value CSV — the format a spreadsheet opens.
    ///
    /// Not a PDF-world format: FDF and XFDF interchange between PDF
    /// programs, and this one leaves that world. Values a spreadsheet would
    /// read as formulae are prefixed with an apostrophe and the change is
    /// reported.
    Csv,
}

/// A document-information field, as a CLI value for `--clear`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum InfoFieldArg {
    /// /Title.
    Title,
    /// /Author.
    Author,
    /// /Subject.
    Subject,
    /// /Keywords.
    Keywords,
}

impl From<InfoFieldArg> for pdfcer_core::edit::InfoField {
    fn from(arg: InfoFieldArg) -> Self {
        match arg {
            InfoFieldArg::Title => Self::Title,
            InfoFieldArg::Author => Self::Author,
            InfoFieldArg::Subject => Self::Subject,
            InfoFieldArg::Keywords => Self::Keywords,
        }
    }
}

/// The geometric-markup subtype selected by `pdfcer annotate --type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum AnnotKindArg {
    /// /Square — an axis-aligned rectangle (--rect).
    Square,
    /// /Circle — an ellipse inscribed in a rectangle (--rect).
    Circle,
    /// /Line — a single segment, arrow-headed by default (--line).
    Line,
    /// /Ink — freehand strokes (--strokes).
    Ink,
    /// /Polygon — a closed multi-segment shape (--points).
    Polygon,
    /// /PolyLine — an open multi-segment path (--points).
    Polyline,
    /// /Highlight — a translucent wash over quads (--quads/--rect).
    Highlight,
    /// /Underline — a baseline line over quads.
    Underline,
    /// /StrikeOut — a strike-through line over quads.
    Strikeout,
    /// /Squiggly — a wavy line over quads.
    Squiggly,
    /// /FreeText — text drawn on the page (--text, --rect).
    Freetext,
    /// /Text — a sticky note whose body opens in a popup (--text).
    Text,
    /// /Stamp — a rubber stamp with a framed label (--stamp-name).
    Stamp,
}

/// Justification for a FreeText annotation (`/Q`, §12.7.3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum QuadArg {
    /// /Q 0 — left-justified.
    Left,
    /// /Q 1 — centred.
    Center,
    /// /Q 2 — right-justified.
    Right,
}

impl QuadArg {
    /// The core quadding (text alignment) this word names.
    pub(crate) fn to_quadding(self) -> pdfcer_core::vartext::Quadding {
        use pdfcer_core::vartext::Quadding;
        match self {
            Self::Left => Quadding::Left,
            Self::Center => Quadding::Center,
            Self::Right => Quadding::Right,
        }
    }
}

/// Sticky-note icon name (§12.5.6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum IconArg {
    /// /Comment.
    Comment,
    /// /Key.
    Key,
    /// /Note (default).
    Note,
    /// /Help.
    Help,
    /// /NewParagraph.
    NewParagraph,
    /// /Paragraph.
    Paragraph,
    /// /Insert.
    Insert,
}

impl IconArg {
    /// The core sticky-note icon this word names.
    pub(crate) fn to_icon(self) -> pdfcer_core::annot_author::StickyIcon {
        use pdfcer_core::annot_author::StickyIcon;
        match self {
            Self::Comment => StickyIcon::Comment,
            Self::Key => StickyIcon::Key,
            Self::Note => StickyIcon::Note,
            Self::Help => StickyIcon::Help,
            Self::NewParagraph => StickyIcon::NewParagraph,
            Self::Paragraph => StickyIcon::Paragraph,
            Self::Insert => StickyIcon::Insert,
        }
    }
}

/// Standard rubber-stamp name (§12.5.6.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StampArg {
    /// /Approved.
    Approved,
    /// /Experimental.
    Experimental,
    /// /NotApproved.
    NotApproved,
    /// /AsIs.
    AsIs,
    /// /Expired.
    Expired,
    /// /NotForPublicRelease.
    NotForPublicRelease,
    /// /Confidential.
    Confidential,
    /// /Final.
    Final,
    /// /Sold.
    Sold,
    /// /Departmental.
    Departmental,
    /// /ForComment.
    ForComment,
    /// /TopSecret.
    TopSecret,
    /// /Draft (default).
    Draft,
    /// /ForPublicRelease.
    ForPublicRelease,
}

/// `--stamp-fit`: what a stamp does when its label will not fit `--rect`
/// (`Pass 287.0`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StampFitArg {
    /// Widen the stamp so the whole label fits. The default.
    Grow,
    /// Keep the drawn box and shrink the text into it.
    Shrink,
    /// Keep both and let the label be cut, so an existing document's look
    /// can be reproduced.
    Clip,
}

impl StampFitArg {
    /// The core fit policy this word names.
    pub(crate) fn to_fit(self) -> pdfcer_core::annot_author::StampFit {
        use pdfcer_core::annot_author::StampFit;
        match self {
            Self::Grow => StampFit::GrowToText,
            Self::Shrink => StampFit::ShrinkToBox,
            Self::Clip => StampFit::ClipToBox,
        }
    }
}

impl StampArg {
    /// The core standard stamp name this word names.
    pub(crate) fn to_stamp_name(self) -> pdfcer_core::annot_author::StampName {
        use pdfcer_core::annot_author::StampName as S;
        match self {
            Self::Approved => S::Approved,
            Self::Experimental => S::Experimental,
            Self::NotApproved => S::NotApproved,
            Self::AsIs => S::AsIs,
            Self::Expired => S::Expired,
            Self::NotForPublicRelease => S::NotForPublicRelease,
            Self::Confidential => S::Confidential,
            Self::Final => S::Final,
            Self::Sold => S::Sold,
            Self::Departmental => S::Departmental,
            Self::ForComment => S::ForComment,
            Self::TopSecret => S::TopSecret,
            Self::Draft => S::Draft,
            Self::ForPublicRelease => S::ForPublicRelease,
        }
    }
}
