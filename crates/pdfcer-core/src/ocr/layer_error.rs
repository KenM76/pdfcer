//! [`OcrLayerError`]: why an OCR-layer write or removal refused.

use crate::object::ObjId;
use crate::page_tree::PageTreeError;
use crate::writer::WriteError;

/// A failure to write an OCR layer. Every variant is a named, clean outcome.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OcrLayerError {
    /// The page index is past the end of the document.
    #[error("page index {0} is out of range")]
    PageIndex(usize),
    /// The document is encrypted and this edit is not permitted: see
    /// [`EditError::DocumentEncrypted`](crate::edit::EditError::DocumentEncrypted).
    #[error("{}", crate::edit::ENCRYPTED_EDIT_REFUSED)]
    Encrypted,
    /// The recognised page contained no word that could be written.
    ///
    /// A named refusal rather than a zero-word success, because writing an
    /// empty content stream and a font nobody uses would grow the file and
    /// change its bytes to accomplish nothing — and would report "done" for a
    /// page where OCR in fact found nothing.
    #[error("no recognised words could be written for this page")]
    NothingToWrite,
    /// Two entries in one session run named the same page.
    ///
    /// # Why this is a refusal and not a silent merge
    ///
    /// [`crate::edit::EditSession::add_ocr_layer`] plans every page against
    /// the graph as it stands **before** the command is committed — that is
    /// what lets a multi-page run be one undo entry. Two entries for one page
    /// would therefore both append to that page's *original* `/Contents`, and
    /// the second page-dictionary write would clobber the first. One layer
    /// written, one layer paid for and lost, and a report claiming both.
    ///
    /// Merging them instead would mean deciding what "both layers on one page"
    /// means, which is a question the caller is better placed to answer by
    /// merging the word lists before it calls.
    #[error("page {page_index} appears more than once in one OCR run")]
    DuplicatePage {
        /// The page index that appeared twice.
        page_index: usize,
    },
    /// The document's `/Size` is smaller than the highest object number in
    /// use, so objects exist that a writer cannot see.
    ///
    /// The [`crate::text_edit::AddTextError::HiddenObjects`] sibling, refused
    /// for the same reason and in the same position: allocating a new object
    /// number in a document whose numbering already lies is how a write lands
    /// on top of something.
    #[error("the document hides {count} object(s) behind an undersized /Size")]
    HiddenObjects {
        /// How many objects are unreachable through `/Size`.
        count: usize,
    },
    /// The page object is not a dictionary.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The page tree could not be walked.
    #[error(transparent)]
    PageTree(#[from] PageTreeError),
    /// The document has no free object numbers left.
    #[error("no free object numbers remain")]
    ObjectNumbersExhausted,
    /// The document is certified and its enforced DocMDP forbids the change.
    ///
    /// The [`crate::text_edit::add_text`] sibling of this refusal, for the
    /// same reason and by the same machinery: writing an OCR layer creates a
    /// content stream and a font and rewrites the page dict, and §12.8.4
    /// Table 258 requires a consumer to enforce `/Perms` -> `/DocMDP`.
    /// Deliberately conservative -- every enforced certification is treated
    /// as forbidding -- because over-refusal is fail-clean and the
    /// alternative is a silently-invalidated signature.
    #[error(
        "the document is certified with DocMDP permission {permission}, which forbids adding an OCR layer"
    )]
    CertificationForbidsChange {
        /// The `/P` value from the DocMDP transform parameters (Table 254
        /// default 2 when absent).
        permission: u8,
    },
    /// The page already carries a pdfcer OCR layer and the options say
    /// [`ExistingLayers::Refuse`](super::layer::ExistingLayers::Refuse).
    #[error("page {page_index} already carries {count} OCR layer(s) written by pdfcer")]
    LayerPresent {
        /// The page index.
        page_index: usize,
        /// How many layers were found.
        count: usize,
    },
    /// The page carries an editable-text layer
    /// ([`super::editable::OcrOutput::EditableText`]): its words are no
    /// longer in the scan, so replacing or removing the layer would delete
    /// them. Refused, as Acrobat refuses re-recognising such a page.
    #[error("page {page_index} carries editable OCR text; its words are no longer in the image")]
    EditableLayerPresent {
        /// The page index.
        page_index: usize,
    },
    /// Editable output: an image whose glyphs were erased could not be
    /// re-encoded, so nothing was written (visible text over glyphs still in
    /// the image would show every word twice).
    #[error("an image could not be re-encoded after erasing its text")]
    ImageEncodeFailed,
    /// [`OcrLayerOptions::on_layer`](super::layer::OcrLayerOptions::on_layer) named an object that is not a group
    /// listed in `/OCProperties /OCGs` (§8.11.4.2 Table 100).
    #[error("object {id} is not a layer listed in /OCProperties /OCGs")]
    NotALayerGroup {
        /// The object named.
        id: ObjId,
    },
    /// The layer to remove is not on that page (any more).
    ///
    /// An [`super::marker::OcrLayerRef`] names one revision; find again after
    /// an edit rather than reuse an old one.
    #[error("no pdfcer OCR layer in object {content} on page {page_index}")]
    LayerNotFound {
        /// The page index the reference named.
        page_index: usize,
        /// The content stream the reference named.
        content: ObjId,
    },
    /// The incremental save failed.
    #[error(transparent)]
    Write(#[from] WriteError),
}
