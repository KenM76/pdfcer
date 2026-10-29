//! [`EditSession::add_printer_marks`]: grow each page past its trim box and
//! draw printer's marks in the new band.

use super::running_text::selected_pages;
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, rect_array};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{BoxResolution, Page};
use crate::printer_marks::{
    MarksLayout, PrinterMarks, PrinterMarksError, PrinterMarksOutcome, clip_content, marks_content,
};

/// The objects every marked page shares.
struct Shared {
    font: ObjId,
    all: ObjId,
}

impl EditSession {
    /// **Add printer's marks** to the selected pages: each page's media and
    /// crop box grow past its trim box by the marks band, its content is
    /// clipped to its bleed box, and the marks are drawn in the band
    /// (ISO 32000-1 §14.11.2).
    ///
    /// The `/TrimBox`, `/BleedBox` and `/ArtBox` are written as they
    /// resolved, so the finished page still trims where it did. A page with
    /// no `/TrimBox` takes its crop box as the trim, reported in
    /// [`PrinterMarksOutcome::trim_assumed`]. The page's own content streams
    /// are byte-identical; a clip stream goes before them and the marks
    /// stream after. One [`CommandKind::AddPrinterMarks`] undo entry.
    ///
    /// # Errors
    ///
    /// Checked before anything is written: the [`Self::stamp_bates`] session
    /// refusals; [`EditError::PrinterMarks`] for no marks, a bad size, an
    /// unencodable label or an empty selection; [`EditError::PageOutOfRange`].
    pub fn add_printer_marks(
        &mut self,
        marks: &PrinterMarks,
    ) -> Result<PrinterMarksOutcome, EditError> {
        self.check_page_stamp()?;
        marks.check()?;
        let pages = self.pages()?;
        let selected = selected_pages(marks.pages.as_deref(), pages.len())?;
        if selected.is_empty() {
            return Err(PrinterMarksError::NoPages.into());
        }
        let mut objects = Vec::new();
        let shared = self.marks_shared(&mut objects)?;
        let mut outcome = PrinterMarksOutcome {
            pages: selected.clone(),
            trim_assumed: Vec::new(),
            grown_by: 0.0,
        };
        for &index in &selected {
            let Some(page) = pages.get(index) else {
                continue;
            };
            if page.trim_box_resolution == BoxResolution::Defaulted {
                outcome.trim_assumed.push(index);
            }
            let info = marks.info_line(index + 1, pages.len());
            let grown = self.mark_page(page, marks, &shared, &info, &mut objects)?;
            outcome.grown_by = outcome.grown_by.max(grown);
        }
        self.commit(Command {
            kind: CommandKind::AddPrinterMarks,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(outcome)
    }

    /// Helvetica and the `/Separation /All` colour space array.
    fn marks_shared(&mut self, objects: &mut Vec<ObjectWrite>) -> Result<Shared, EditError> {
        let font = ObjId::new(self.alloc_number()?, 0);
        let mut f = Dict::new();
        for (k, v) in [
            (&b"Type"[..], &b"Font"[..]),
            (b"Subtype", b"Type1"),
            (b"BaseFont", b"Helvetica"),
            (b"Encoding", b"WinAnsiEncoding"),
        ] {
            f.insert(Name::from(k), Object::Name(Name::from(v)));
        }
        objects.push(ObjectWrite {
            id: font,
            before: None,
            after: Some(Object::Dict(f)),
        });
        // §8.6.6.4: `/All` still needs a valid alternate and tint transform;
        // full tint maps to all four process inks.
        let mut tint = Dict::new();
        tint.insert(Name::from(b"FunctionType"), Object::Integer(2));
        tint.insert(Name::from(b"N"), Object::Integer(1));
        for (k, v) in [
            (&b"Domain"[..], vec![0.0, 1.0]),
            (b"C0", vec![0.0; 4]),
            (b"C1", vec![1.0; 4]),
        ] {
            tint.insert(
                Name::from(k),
                Object::Array(v.into_iter().map(Object::Real).collect()),
            );
        }
        let all = ObjId::new(self.alloc_number()?, 0);
        objects.push(ObjectWrite {
            id: all,
            before: None,
            after: Some(Object::Array(vec![
                Object::Name(Name::from(b"Separation")),
                Object::Name(Name::from(b"All")),
                Object::Name(Name::from(b"DeviceCMYK")),
                Object::Dict(tint),
            ])),
        });
        Ok(Shared { font, all })
    }

    /// Write one page's boxes, resources and content; returns the band
    /// width it grew by.
    fn mark_page(
        &mut self,
        page: &Page,
        marks: &PrinterMarks,
        shared: &Shared,
        info: &str,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<f64, EditError> {
        let layout = MarksLayout::new(marks, page.trim_box, page.bleed_box);
        let mut resources = page.resources.clone();
        let font = self.free_name_in(&resources, b"Font", "Mk");
        self.put_resource(
            &mut resources,
            b"Font",
            &font,
            Object::Reference(shared.font),
        );
        let all = self.free_name_in(&resources, b"ColorSpace", "MkAll");
        self.put_resource(
            &mut resources,
            b"ColorSpace",
            &all,
            Object::Reference(shared.all),
        );
        let clip = self.new_stream(Dict::new(), &clip_content(layout.bleed), objects)?;
        let content = marks_content(marks, &layout, &all, &font, info);
        let drawn = self.new_stream(Dict::new(), &content, objects)?;
        let before = self.value(page.id).cloned();
        let mut updated = self
            .graph()
            .resolved(page.id)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let mut contents = vec![Object::Reference(clip)];
        contents.extend(self.content_refs(updated.get(b"Contents")));
        contents.push(Object::Reference(drawn));
        updated.insert(Name::from(b"Contents"), Object::Array(contents));
        updated.insert(Name::from(b"Resources"), Object::Dict(resources));
        for (key, rect) in [
            (&b"MediaBox"[..], layout.sheet),
            (b"CropBox", layout.sheet),
            (b"BleedBox", layout.bleed),
            (b"TrimBox", layout.trim),
            (b"ArtBox", page.art_box),
        ] {
            updated.insert(Name::from(key), rect_array(rect));
        }
        objects.push(ObjectWrite {
            id: page.id,
            before,
            after: Some(Object::Dict(updated)),
        });
        Ok(layout.band())
    }

    /// The page's `/Contents` as a list of entries, an indirect array
    /// opened (§7.7.3.3: a stream or an array of streams).
    fn content_refs(&self, contents: Option<&Object>) -> Vec<Object> {
        match contents {
            None => Vec::new(),
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Reference(r)) => match self.graph().resolved(*r) {
                Object::Array(items) => items.clone(),
                _ => vec![Object::Reference(*r)],
            },
            Some(other) => vec![other.clone()],
        }
    }
}
