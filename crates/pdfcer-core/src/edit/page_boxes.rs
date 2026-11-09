//! Set or reset any of a page's four visible-region boxes — crop, bleed,
//! trim, art — with a rectangle per page, as one undoable command
//! (ISO 32000-2 §14.11.2, Table 30).

use super::{
    Command, CommandKind, CropBoxChange, CropBoxEdit, CropBoxEntry, EditError, EditSession,
    ObjectWrite, rect_array,
};
use crate::crypto::PermissionBit;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{self, Rect};

/// Which page boundary a [`PageBoxEdit`] changes (§14.11.2).
///
/// `/CropBox` is inheritable (§7.7.3.4); the other three are not, and each
/// defaults to the page's crop box when absent (Table 30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PageBox {
    /// `/CropBox`: the region a reader shows and prints.
    Crop,
    /// `/BleedBox`: the region production output is clipped to.
    Bleed,
    /// `/TrimBox`: the intended finished page after trimming.
    Trim,
    /// `/ArtBox`: the extent of the page's meaningful content.
    Art,
}

impl PageBox {
    /// The page-dictionary key, e.g. `"TrimBox"`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Crop => "CropBox",
            Self::Bleed => "BleedBox",
            Self::Trim => "TrimBox",
            Self::Art => "ArtBox",
        }
    }
}

/// One box change on one page, for [`EditSession::set_page_boxes`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageBoxEdit {
    /// The 0-based page index.
    pub page_index: usize,
    /// The box to change.
    pub target: PageBox,
    /// Set a rectangle (default user space) or reset to the default.
    pub edit: CropBoxEdit,
}

impl PageBoxEdit {
    /// A change of `target` on page `page_index`.
    pub const fn new(page_index: usize, target: PageBox, edit: CropBoxEdit) -> Self {
        Self {
            page_index,
            target,
            edit,
        }
    }
}

/// What [`EditSession::set_page_boxes`] did to one box on one page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageBoxChange {
    /// The 0-based page index.
    pub page_index: usize,
    /// The box changed.
    pub target: PageBox,
    /// The effective box before the call: the written box ∩ the media box,
    /// or its default (the media box for crop, the crop box otherwise).
    pub before: Rect,
    /// The effective box after the call.
    pub after: Rect,
    /// How the page's own entry ended up expressed. Bleed, trim and art
    /// are not inheritable, so they never report
    /// [`CropBoxEntry::InheritedSoOwnEntryRemoved`].
    pub entry: CropBoxEntry,
    /// The requested rectangle overhangs the media box, so [`Self::after`]
    /// is its intersection with the sheet (§14.11.2.1). Written as asked.
    pub overhangs_media_box: bool,
}

impl PageBoxChange {
    fn from_crop(change: CropBoxChange) -> Self {
        Self {
            page_index: change.page_index,
            target: PageBox::Crop,
            before: change.before,
            after: change.after,
            entry: change.entry,
            overhangs_media_box: change.overhangs_media_box,
        }
    }
}

impl EditSession {
    /// Apply box changes to any set of pages, each with its own rectangle,
    /// as **one** undoable command (ISO 32000-2 §14.11.2, Table 30).
    ///
    /// A crop change follows [`EditSession::set_crop_boxes`] exactly. A
    /// bleed, trim or art change writes the page's own entry (they are not
    /// inheritable), keeps the base file's spelling when it already
    /// denotes the result, and on reset removes the entry so the box
    /// defaults to the crop box. All changes to one page land in one
    /// write. Cropping hides content; it removes nothing.
    ///
    /// Returns one [`PageBoxChange`] per edit, ordered by page, then crop
    /// before bleed, trim and art; a bleed/trim/art default is judged
    /// against the crop box this same call leaves.
    ///
    /// # Errors
    ///
    /// Raised before anything is committed:
    /// - [`EditError::PageBoxRepeated`] — two edits name the same box on the
    ///   same page.
    /// - [`EditError::CropBoxEmpty`] / [`EditError::PageBoxEmpty`] — a
    ///   rectangle with no area, a non-finite coordinate, or no overlap
    ///   with the media box.
    /// - [`EditError::PageOutOfRange`], [`EditError::NotADictionary`],
    ///   [`EditError::DocumentEncrypted`], and the certification gate.
    ///
    /// # Example
    ///
    /// ```
    /// # use pdfcer_core::document::Document;
    /// # use pdfcer_core::edit::{CropBoxEdit, EditSession, PageBox, PageBoxEdit};
    /// # use pdfcer_core::page_tree::Rect;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let bytes: Vec<u8> =
    ///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec();
    /// let mut session = EditSession::new(Document::from_bytes(bytes)?);
    /// let trim = Rect::from_corners(18.0, 18.0, 100.0, 100.0);
    /// let changes = session.set_page_boxes(&[
    ///     PageBoxEdit::new(0, PageBox::Trim, CropBoxEdit::Set(trim)),
    /// ])?;
    /// assert_eq!(changes[0].after, trim);
    /// session.set_page_boxes(&[PageBoxEdit::new(0, PageBox::Trim, CropBoxEdit::Reset)])?;
    /// assert!(!session.is_modified(), "set then reset is net zero");
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_page_boxes(
        &mut self,
        edits: &[PageBoxEdit],
    ) -> Result<Vec<PageBoxChange>, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let mut order: Vec<PageBoxEdit> = edits.to_vec();
        order.sort_by_key(|e| (e.page_index, e.target));
        if let Some((a, _)) = order
            .iter()
            .zip(order.iter().skip(1))
            .find(|(a, b)| (a.page_index, a.target) == (b.page_index, b.target))
        {
            return Err(EditError::PageBoxRepeated {
                page_index: a.page_index,
                target: a.target,
            });
        }

        let slots = self.page_slots()?;
        let pages = self.pages()?;
        let mut changes = Vec::with_capacity(order.len());
        let mut writes: Vec<ObjectWrite> = Vec::new();
        for group in order.chunk_by(|a, b| a.page_index == b.page_index) {
            let Some(index) = group.first().map(|e| e.page_index) else {
                continue;
            };
            let (Some(slot), Some(page)) = (slots.get(index), pages.get(index)) else {
                return Err(EditError::PageOutOfRange {
                    index,
                    count: slots.len(),
                });
            };
            let id = slot.id;
            let Some(Object::Dict(current)) = self.value(id) else {
                return Err(EditError::NotADictionary { id, key: "CropBox" });
            };
            let current = current.clone();
            let mut updated = current.clone();
            let media = page.media_box;
            let mut crop = page.crop_box;
            for e in group {
                let change = if e.target == PageBox::Crop {
                    let c = self.crop_box_update(
                        index,
                        slot,
                        &current,
                        &mut updated,
                        media,
                        media,
                        e.edit,
                    )?;
                    crop = c.after;
                    PageBoxChange::from_crop(c)
                } else {
                    let defaults = (page.crop_box, crop);
                    self.print_box_update(*e, id, &mut updated, media, defaults)?
                };
                changes.push(change);
            }
            if updated != current {
                writes.push(ObjectWrite {
                    id,
                    before: self.state.get(&id).cloned(),
                    after: Some(Object::Dict(updated)),
                });
            }
        }
        if !writes.is_empty() {
            self.commit(Command {
                kind: CommandKind::SetPageBoxes {
                    count: writes.len(),
                },
                objects: writes,
                removals: Vec::new(),
                trailer: None,
            });
        }
        Ok(changes)
    }

    /// Apply a bleed, trim or art edit to `updated`. `defaults` is the
    /// effective crop box before and after this call.
    fn print_box_update(
        &self,
        e: PageBoxEdit,
        id: ObjId,
        updated: &mut Dict,
        media: Rect,
        defaults: (Rect, Rect),
    ) -> Result<PageBoxChange, EditError> {
        let key = e.target.key();
        let parse = |value: &Object| page_tree::parse_rect(&self.graph(), value, key).ok();
        let effective = |own: Option<Rect>, default: Rect| {
            own.and_then(|r| media.intersection(&r)).unwrap_or(default)
        };
        let before = effective(updated.get(key.as_bytes()).and_then(parse), defaults.0);
        let (entry, after, overhangs) = match e.edit {
            CropBoxEdit::Reset => {
                updated.remove(key.as_bytes());
                (CropBoxEntry::Absent, defaults.1, false)
            }
            CropBoxEdit::Set(rect) => {
                let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
                let finite = [rect.llx, rect.lly, rect.urx, rect.ury]
                    .iter()
                    .all(|v| v.is_finite());
                let Some(after) = media.intersection(&rect).filter(|_| finite) else {
                    return Err(EditError::PageBoxEmpty {
                        page_index: e.page_index,
                        target: e.target,
                    });
                };
                let base_own = self
                    .base
                    .get(id)
                    .and_then(|io| io.value.as_dict())
                    .and_then(|d| d.get(key.as_bytes()))
                    .cloned();
                let entry = match base_own {
                    Some(original) if parse(&original) == Some(rect) => {
                        updated.insert(Name::from(key.as_bytes()), original);
                        CropBoxEntry::BaseSpellingKept
                    }
                    _ => {
                        updated.insert(Name::from(key.as_bytes()), rect_array(rect));
                        CropBoxEntry::ExplicitWritten
                    }
                };
                (entry, after, !media.contains(&rect))
            }
        };
        Ok(PageBoxChange {
            page_index: e.page_index,
            target: e.target,
            before,
            after,
            entry,
            overhangs_media_box: overhangs,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::*;
    use crate::document::Document;

    const PAGES: usize = 3;

    /// Three letter-size pages, the media box on the `Pages` node.
    fn session() -> EditSession {
        let page = "<< /Type /Page /Parent 2 0 R /Resources << >> >>";
        let pdf = crate::edit::tests::build(
            &[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] >>",
                page,
                page,
                page,
            ],
            "",
        );
        EditSession::new(Document::from_bytes(pdf).unwrap())
    }

    fn rect(a: f64, b: f64, c: f64, d: f64) -> Rect {
        Rect::from_corners(a, b, c, d)
    }

    #[test]
    fn several_boxes_on_several_pages_are_one_undo_entry() {
        let mut s = session();
        let edits = [
            PageBoxEdit::new(
                2,
                PageBox::Trim,
                CropBoxEdit::Set(rect(9.0, 9.0, 603.0, 783.0)),
            ),
            PageBoxEdit::new(
                0,
                PageBox::Crop,
                CropBoxEdit::Set(rect(36.0, 36.0, 576.0, 756.0)),
            ),
            PageBoxEdit::new(
                0,
                PageBox::Art,
                CropBoxEdit::Set(rect(50.0, 50.0, 300.0, 300.0)),
            ),
        ];
        let changes = s.set_page_boxes(&edits).unwrap();
        let order: Vec<_> = changes.iter().map(|c| (c.page_index, c.target)).collect();
        assert_eq!(
            order,
            [(0, PageBox::Crop), (0, PageBox::Art), (2, PageBox::Trim)]
        );
        assert_eq!(s.undo_kind(), Some(CommandKind::SetPageBoxes { count: 2 }));
        assert_eq!(
            s.pages().unwrap()[0].crop_box,
            rect(36.0, 36.0, 576.0, 756.0)
        );
        s.undo().unwrap();
        assert!(!s.is_modified());
    }

    #[test]
    fn a_print_box_defaults_to_the_crop_this_call_leaves() {
        let mut s = session();
        let crop = rect(100.0, 100.0, 500.0, 700.0);
        let changes = s
            .set_page_boxes(&[
                PageBoxEdit::new(1, PageBox::Bleed, CropBoxEdit::Reset),
                PageBoxEdit::new(1, PageBox::Crop, CropBoxEdit::Set(crop)),
            ])
            .unwrap();
        assert_eq!(changes[1].target, PageBox::Bleed);
        assert_eq!(changes[1].before, rect(0.0, 0.0, 612.0, 792.0));
        assert_eq!(changes[1].after, crop);
        assert_eq!(changes[1].entry, CropBoxEntry::Absent);
    }

    #[test]
    fn an_overhanging_trim_is_written_as_asked_and_reported() {
        let mut s = session();
        let wide = rect(-10.0, -10.0, 700.0, 800.0);
        let c = s
            .set_page_boxes(&[PageBoxEdit::new(0, PageBox::Trim, CropBoxEdit::Set(wide))])
            .unwrap()[0];
        assert!(c.overhangs_media_box);
        assert_eq!(c.after, rect(0.0, 0.0, 612.0, 792.0));
        assert_eq!(c.entry, CropBoxEntry::ExplicitWritten);
    }

    #[test]
    fn refusals_commit_nothing() {
        let mut s = session();
        let ok = PageBoxEdit::new(0, PageBox::Art, CropBoxEdit::Set(rect(1.0, 1.0, 9.0, 9.0)));
        let off = PageBoxEdit::new(
            1,
            PageBox::Art,
            CropBoxEdit::Set(rect(900.0, 900.0, 950.0, 950.0)),
        );
        assert!(matches!(
            s.set_page_boxes(&[ok, off]),
            Err(EditError::PageBoxEmpty {
                page_index: 1,
                target: PageBox::Art
            })
        ));
        assert!(matches!(
            s.set_page_boxes(&[ok, ok]),
            Err(EditError::PageBoxRepeated {
                page_index: 0,
                target: PageBox::Art
            })
        ));
        let far = PageBoxEdit::new(PAGES, PageBox::Trim, CropBoxEdit::Reset);
        assert!(matches!(
            s.set_page_boxes(&[ok, far]),
            Err(EditError::PageOutOfRange { .. })
        ));
        assert!(!s.is_modified());
    }

    #[test]
    fn resetting_an_absent_print_box_writes_nothing() {
        let mut s = session();
        let c = s
            .set_page_boxes(&[PageBoxEdit::new(0, PageBox::Trim, CropBoxEdit::Reset)])
            .unwrap();
        assert_eq!(c[0].entry, CropBoxEntry::Absent);
        assert!(!s.is_modified());
    }
}
