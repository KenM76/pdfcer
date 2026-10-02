//! Why a text-edit verb will not do an edit, as data a shell can switch on.
//!
//! One enum serves [`EditError::Unsupported`](super::EditError::Unsupported),
//! [`ReflowApplyError::Unsupported`](super::ReflowApplyError::Unsupported) and
//! [`AddTextError::Unsupported`](super::AddTextError::Unsupported), so a shell
//! maps every cause to an operator sentence in one place. `Display` is the
//! sentence the CLI prints.
//!
//! Font-coverage refusals (R-INV-n) are not here: they stay
//! [`Refusal`](super::Refusal), which already carries its trigger as data.

/// The cause of an `Unsupported` text-edit refusal.
///
/// Every variant is a property of the page or of the request, never of the
/// session's history, so the same request refuses the same way until the
/// document changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnsupportedCause {
    /// The page has no `/Contents` stream to edit.
    NoContents,
    /// The `find` text was empty on a request that names no operator.
    EmptyFind,
    /// The run is shown with the `'` or `"` operator (only `Tj`/`TJ` are
    /// edited).
    QuoteOperator,
    /// The match crosses two string elements of one `TJ` array, on a path
    /// that edits one element only.
    CrossElementTj,
    /// The run's `Tf` names a font that the stream's resources do not
    /// resolve.
    FontUnresolvable,
    /// The run's simple font has no encoding that can be inverted.
    EncodingNotInvertible,
    /// The run's composite font has no `/ToUnicode`.
    CompositeWithoutToUnicode,
    /// The run's composite font map cannot be inverted.
    FontMapNotInvertible {
        /// The inversion failure, in words.
        detail: String,
    },
    /// The run's font writes vertically (`/WMode 1`, e.g. `Identity-V`,
    /// ISO 32000-2 §9.7.4.3). Vertical layout is not supported, and laying
    /// the run out with horizontal advances would write it wrong.
    VerticalWriting,
    /// The request targeted a form XObject the page does not paint.
    FormNotOnPage {
        /// The form's object number.
        object: u32,
    },
    /// The request targeted a form XObject whose content cannot be decoded.
    FormUndecodable {
        /// The form's object number.
        object: u32,
    },
    /// The text is drawn through a form XObject, on a path that edits page
    /// content only.
    InsideFormXObject {
        /// The form's object number, when known.
        object: Option<u32>,
    },
    /// The form XObject is a reference XObject (`/Ref`, ISO 32000-1 §8.10.4).
    ReferenceXObject,
    /// The form XObject is an OPI proxy (`/OPI`, ISO 32000-1 §14.11.7).
    OpiProxy,
    /// No object number is left to allocate.
    ObjectNumbersExhausted,
    /// The page object is not a dictionary.
    PageNotDictionary,
    /// Justifying a block whose producer already set non-zero `Tc`/`Tw`.
    /// Not produced: reflow justifies with `TJ` over the measured advances.
    JustifyWithSpacing,
    /// The block mixes more than one font resource. Not produced: reflow
    /// carries each glyph's own font.
    MixedFonts,
    /// A block glyph was shown with no font selected (malformed content).
    ShowWithoutFont,
    /// The block carries no font resource. Not produced: a glyph shown with
    /// no font is [`Self::ShowWithoutFont`].
    NoFont,
    /// The block has no show operators that can be located.
    NoShowOperators,
    /// The block's text or current transformation matrix is rotated or
    /// skewed.
    RotatedOrSkewed {
        /// Which matrix: `"text matrix"` or `"CTM"`.
        matrix: &'static str,
    },
    /// The block spans more than one scale of the named matrix.
    MixedScale {
        /// Which matrix: `"text matrix"` or `"CTM"`.
        matrix: &'static str,
    },
    /// The block's CTM has a zero scale.
    DegenerateCtm,
    /// A block show operator lies outside `BT … ET` (malformed content).
    ShowOutsideTextObject,
    /// The block shares a text object with other content.
    SharedTextObject,
    /// The block's text objects are not contiguous in the content stream.
    NonContiguousTextObjects,
    /// The block's show operators were not found in the content stream.
    ShowOperatorsNotFound,
    /// The text state in force after the block cannot be restored.
    StateNotRestorable {
        /// Which state, in words.
        detail: String,
    },
    /// The block's content region holds an operator a re-emitted block
    /// cannot carry (a path, an image, a nested form, unbalanced `q`/`Q`):
    /// reflowing would drop it.
    OperatorInBlock {
        /// The operator, as written (`"inline image"` for `BI … EI`).
        operator: String,
    },
    /// A word break on a block shown in this font needs a space glyph, and
    /// neither the block nor a single-byte font supplies one.
    NoSpaceGlyph {
        /// The font's `/BaseFont`.
        font: String,
    },
    /// Committing the planned edit to the session failed.
    CommitFailed {
        /// The session's error, in words.
        detail: String,
    },
}

impl UnsupportedCause {
    /// The message of a variant that carries no data.
    fn fixed_text(&self) -> Option<&'static str> {
        Some(match self {
            Self::NoContents => "the page has no /Contents to edit",
            Self::EmptyFind => "empty find text",
            Self::QuoteOperator => {
                "editing a run shown with the ' or \" operator is deferred (only Tj/TJ are edited)"
            }
            Self::CrossElementTj => {
                "the match spans more than one TJ string element (cross-element edit deferred)"
            }
            Self::FontUnresolvable => {
                "the run's font resource is unresolvable in the target stream's resources"
            }
            Self::EncodingNotInvertible => "the run's font has no invertible encoding",
            Self::CompositeWithoutToUnicode => "the run's composite font has no /ToUnicode",
            Self::VerticalWriting => {
                "the run's font writes vertically (/WMode 1); vertical text is not edited, because \
                 laying it out with horizontal advances would write it wrong"
            }
            Self::ReferenceXObject => {
                "this form XObject is a REFERENCE XObject (/Ref, ISO 32000-1 8.10.4) -- its visible content is a proxy for content in another file, which a conforming reader may substitute wholesale, so an edit here could silently fail to reach what is actually printed"
            }
            Self::OpiProxy => {
                "this form XObject is an OPI proxy (/OPI, ISO 32000-1 14.11.7) -- a prepress system substitutes the real high-resolution artwork at print time, so an edit here could silently fail to reach what is actually printed"
            }
            Self::ObjectNumbersExhausted => "no object number is left to allocate",
            Self::PageNotDictionary => "the page object is not a dictionary",
            Self::JustifyWithSpacing => {
                "justify of a block with non-zero Tc/Tw is deferred (the slack arithmetic assumes the kept spaces carry only their own w0); reflow with left/right/centre instead"
            }
            Self::MixedFonts => {
                "the block mixes more than one font resource; reflow-apply of a multi-font block is deferred"
            }
            Self::ShowWithoutFont => {
                "a block glyph was shown with no font selected (malformed); refusing"
            }
            Self::NoFont => "the block carries no font resource",
            Self::NoShowOperators => "the block has no locatable show operators",
            Self::DegenerateCtm => "the block's CTM has a degenerate (zero) scale; refusing",
            Self::ShowOutsideTextObject => {
                "a block show operator appears outside a BT … ET text object (malformed); refusing"
            }
            Self::SharedTextObject => {
                "the block shares a BT … ET text object with other content; reflow-apply of an interleaved block is deferred"
            }
            Self::NonContiguousTextObjects => {
                "the block's text objects are not contiguous in the content stream; reflow-apply of a split block is deferred"
            }
            Self::ShowOperatorsNotFound => {
                "the block's show operators were not found in the content stream; refusing"
            }
            _ => return None,
        })
    }
}

impl std::fmt::Display for UnsupportedCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(text) = self.fixed_text() {
            return f.write_str(text);
        }
        match self {
            Self::FontMapNotInvertible { detail } => {
                write!(f, "the run's font map cannot be inverted: {detail}")
            }
            Self::FormNotOnPage { object } => write!(
                f,
                "form XObject {object} is not painted by this page, so there is nothing to edit inside it here"
            ),
            Self::FormUndecodable { object } => write!(
                f,
                "form XObject {object} is painted by this page but its content stream could not be decoded"
            ),
            Self::InsideFormXObject { object: Some(o) } => write!(
                f,
                "the text is drawn through form XObject {o}, which this operation does not reach"
            ),
            Self::InsideFormXObject { object: None } => f.write_str(
                "the text is drawn through a form XObject, which this operation does not reach",
            ),
            Self::RotatedOrSkewed { matrix } => write!(
                f,
                "the block's {matrix} is rotated or skewed (off-diagonal terms non-zero); reflow-apply of rotated/skewed text is deferred"
            ),
            Self::MixedScale { matrix } => write!(
                f,
                "the block spans more than one {matrix} scale; reflow-apply of a multi-transform block is deferred"
            ),
            Self::OperatorInBlock { operator } => write!(
                f,
                "the block's text is interleaved with a `{operator}` operator that re-emitting the block would drop; refusing rather than lose it"
            ),
            Self::NoSpaceGlyph { font } => write!(
                f,
                "a re-wrapped line needs a word space in font {font}, which shows no space glyph in this block and is not a single-byte font; refusing rather than guess a code"
            ),
            Self::StateNotRestorable { detail } | Self::CommitFailed { detail } => {
                f.write_str(detail)
            }
            _ => Ok(()),
        }
    }
}

/// Why an edit's find text matched nothing: the payload of
/// [`EditError::NoMatch`](super::EditError::NoMatch).
///
/// A pin that names no operator is not a `NoMatch`: it is
/// [`EditError::PinnedSpanNotFound`](super::EditError::PinnedSpanNotFound).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotFoundReason {
    /// The text is not shown on this page in the searched stream.
    NoSuchText,
    /// The text is on the page, but only by joining show operators across
    /// `objects` separate text objects (`BT … ET`), which the matcher does
    /// not join.
    SpansTextObjects {
        /// How many text objects the joined match touches (at least 2).
        objects: usize,
    },
    /// The text is in one text object, on one line, but across `operators`
    /// show operators the matcher does not join into one run (a font, size
    /// or spacing change between them).
    SplitRun {
        /// How many show operators the match touches (at least 2).
        operators: usize,
    },
}

/// Renders as a suffix to the `NoMatch` sentence: empty for
/// [`NotFoundReason::NoSuchText`], a clause for the others.
impl std::fmt::Display for NotFoundReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchText => Ok(()),
            Self::SpansTextObjects { objects } => write!(
                f,
                " -- it is drawn across {objects} separate text objects, which an edit does not join"
            ),
            Self::SplitRun { operators } => write!(
                f,
                " -- it is drawn across {operators} show operators that do not continue one \
                 run (a font, size or spacing change between them)"
            ),
        }
    }
}
