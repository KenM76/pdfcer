//! Text lines stamped along a page edge: Bates labels and headers/footers
//! share one writer, [`EditSession::write_page_labels`].

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::bates::{BatesPosition, helvetica_width, label_content_as, label_matrix, winansi_bytes};
use crate::crypto::PermissionBit;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Page;
use crate::running_text::{RunningText, RunningTextError, RunningTextOutcome};

/// How a set of edge labels is drawn.
pub(super) struct LabelStyle<'a> {
    pub position: BatesPosition,
    pub margin: f64,
    pub font_size: f64,
    /// The artifact `/Subtype` (`Bates`, `Header`, `Footer`).
    pub subtype: &'a [u8],
    /// Prefix of the page font resource name.
    pub font_prefix: &'a str,
}

impl EditSession {
    /// The refusals every page-stamping verb shares: encryption forbidding
    /// content changes, a certification forbidding page changes, hidden
    /// objects a new object number could expose.
    pub(super) fn check_page_stamp(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        Ok(())
    }

    /// Write `labels` (`(page index, WinAnsi bytes)`) onto `pages`: one shared
    /// Helvetica font object, one self-contained label stream per page,
    /// existing content wrapped not rewritten. Returns the writes for one
    /// command.
    pub(super) fn write_page_labels(
        &mut self,
        pages: &[Page],
        labels: &[(usize, Vec<u8>)],
        style: &LabelStyle<'_>,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let mut objects = Vec::new();
        let mut wrap = None;
        let font_id = ObjId::new(self.alloc_number()?, 0);
        let mut font = Dict::new();
        for (k, v) in [
            (&b"Type"[..], &b"Font"[..]),
            (b"Subtype", b"Type1"),
            (b"BaseFont", b"Helvetica"),
            (b"Encoding", b"WinAnsiEncoding"),
        ] {
            font.insert(Name::from(k), Object::Name(Name::from(v)));
        }
        objects.push(ObjectWrite {
            id: font_id,
            before: None,
            after: Some(Object::Dict(font)),
        });
        for (index, label) in labels {
            let Some(page) = pages.get(*index) else {
                continue;
            };
            let width = helvetica_width(label) * style.font_size;
            let m = label_matrix(
                page.crop_box,
                page.rotate,
                style.position,
                style.margin,
                style.font_size,
                width,
            );
            let mut resources = page.resources.clone();
            let font_name = self.free_name_in(&resources, b"Font", style.font_prefix);
            self.put_resource(
                &mut resources,
                b"Font",
                &font_name,
                Object::Reference(font_id),
            );
            let content = label_content_as(style.subtype, &font_name, m, label);
            let label_id = self.new_stream(Dict::new(), &content, &mut objects)?;
            let before = self.value(page.id).cloned();
            let mut updated = self
                .graph()
                .resolved(page.id)
                .as_dict()
                .cloned()
                .unwrap_or_default();
            updated.insert(Name::from(b"Resources"), Object::Dict(resources));
            self.append_page_content(&mut updated, label_id, &mut wrap, &mut objects)?;
            objects.push(ObjectWrite {
                id: page.id,
                before,
                after: Some(Object::Dict(updated)),
            });
        }
        Ok(objects)
    }

    /// **Stamp a header or footer line** on the selected pages, as page
    /// content marked `/Artifact << /Type /Pagination /Subtype /Header >>`
    /// (or `/Footer`; ISO 32000-1 §14.8.2.2 Table 330).
    ///
    /// `{page}` and `{pages}` in the text are filled per page (1-based page
    /// number, page count). Drawn as [`Self::stamp_bates`] draws a label:
    /// Helvetica, black, upright on the displayed page, the page's own
    /// content streams byte-identical. One [`CommandKind::StampRunningText`]
    /// undo entry.
    ///
    /// # Errors
    ///
    /// Checked before anything is written: the [`Self::stamp_bates`] session
    /// refusals; [`EditError::RunningText`] for empty or unencodable text,
    /// bad geometry or an empty selection; [`EditError::PageOutOfRange`].
    pub fn stamp_running_text(
        &mut self,
        stamp: &RunningText,
    ) -> Result<RunningTextOutcome, EditError> {
        self.check_page_stamp()?;
        stamp.check()?;
        let pages = self.pages()?;
        let selected = selected_pages(stamp.pages.as_deref(), pages.len())?;
        if selected.is_empty() {
            return Err(RunningTextError::NoPages.into());
        }
        let texts: Vec<String> = selected
            .iter()
            .map(|&i| stamp.label(i + 1, pages.len()))
            .collect();
        let labels: Vec<(usize, Vec<u8>)> = selected
            .iter()
            .zip(&texts)
            .map(|(&i, t)| (i, winansi_bytes(t)))
            .collect();
        let style = LabelStyle {
            position: stamp.position(),
            margin: stamp.margin,
            font_size: stamp.font_size,
            subtype: stamp.kind.subtype().as_bytes(),
            font_prefix: "Hdr",
        };
        let objects = self.write_page_labels(&pages, &labels, &style)?;
        self.commit(Command {
            kind: CommandKind::StampRunningText,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(RunningTextOutcome {
            pages: selected,
            first_label: texts.into_iter().next().unwrap_or_default(),
        })
    }
}

/// `selection` (`None` = every page) sorted and deduplicated, refusing an
/// index at or past `count`. May be empty.
pub(super) fn selected_pages(
    selection: Option<&[usize]>,
    count: usize,
) -> Result<Vec<usize>, EditError> {
    let mut selected = selection.map_or_else(|| (0..count).collect(), <[usize]>::to_vec);
    selected.sort_unstable();
    selected.dedup();
    if let Some(&index) = selected.iter().find(|&&i| i >= count) {
        return Err(EditError::PageOutOfRange { index, count });
    }
    Ok(selected)
}
