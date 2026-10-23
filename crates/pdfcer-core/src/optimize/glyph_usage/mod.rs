//! Which codes each font dictionary shows, across everything a document
//! draws: page content, annotation appearances (`/AP` N, R, D and their
//! states), form XObjects, tiling patterns, soft-mask groups and Type 3
//! glyph procedures. Re-subsetting (Pass 616.6) keeps only these glyphs, so
//! the walk refuses rather than guesses: any stream it cannot read, any
//! show with no font in force and any limit reached is a fault, and a
//! fault makes the whole result unusable.
//!
//! The same walk answers whether anything drawn asks for overprint
//! (`gs` with `/OP` or `/op` true, §8.6.7, Table 57), for
//! [`crate::prepress::overprint_use`].
//!
//! [`GlyphUsage::holders`] lists every object whose resources the walk
//! looked through; a font dictionary referenced from anywhere else may be
//! drawn by something the walk did not see.

mod interp;

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::view::DocumentView;
use interp::{Font, Walker};

/// Page-tree ancestors followed above one page.
const MAX_ANCESTORS: usize = 64;
/// Fault messages kept per list.
const MAX_FAULTS: usize = 16;

/// How a font's shown strings split into codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CodeWidth {
    /// One byte per code: every simple font.
    One,
    /// Two bytes per code: a Type 0 font under `Identity-H` or `-V`.
    Two,
    /// A Type 0 font under another CMap; its codes are not split.
    Unknown,
}

/// The result of [`enumerate`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GlyphUsage {
    /// The codes shown with each indirect font dictionary.
    pub(crate) codes: BTreeMap<ObjId, BTreeSet<u32>>,
    /// Fonts shown under a CMap the walk does not follow.
    pub(crate) unsplit: BTreeSet<ObjId>,
    /// Pages, page-tree nodes, walked streams, Type 3 fonts and the
    /// indirect `/Resources` and `/Font` dictionaries they hold.
    pub(crate) holders: BTreeSet<ObjId>,
    /// Why the result cannot be trusted; empty when it can.
    pub(crate) faults: Vec<String>,
    /// The faults that left something undrawn by the walk (a stream it
    /// could not read, a limit reached), as opposed to a show it could
    /// not attribute to a font.
    pub(crate) unwalked: Vec<String>,
    /// The page or stream whose `gs` first asked for overprint.
    pub(crate) overprint: Option<ObjId>,
}

impl GlyphUsage {
    /// A fault that left part of the drawing unwalked.
    fn fault(&mut self, why: String) {
        if self.unwalked.len() < MAX_FAULTS {
            self.unwalked.push(why.clone());
        }
        self.glyph_fault(why);
    }

    /// A fault about attributing shown text to a font only.
    fn glyph_fault(&mut self, why: String) {
        if self.faults.len() < MAX_FAULTS {
            self.faults.push(why);
        }
    }

    fn record(&mut self, font: ObjId, width: CodeWidth, strings: &[&[u8]]) {
        let codes = self.codes.entry(font).or_default();
        for s in strings {
            match width {
                CodeWidth::One => codes.extend(s.iter().map(|&b| u32::from(b))),
                CodeWidth::Two => {
                    codes.extend(s.chunks(2).map(|c| match c {
                        [hi, lo] => u32::from(*hi) << 8 | u32::from(*lo),
                        [hi] => u32::from(*hi) << 8,
                        _ => 0,
                    }));
                }
                CodeWidth::Unknown => {
                    self.unsplit.insert(font);
                }
            }
        }
    }

    /// Note `resources` (an entry's value) and its `/Font` as holders
    /// when they are indirect.
    fn note_resources(&mut self, view: &DocumentView<'_>, resources: &Object) {
        if let Some(id) = resources.as_reference() {
            self.holders.insert(id);
        }
        let font = view
            .resolve(resources)
            .as_dict()
            .and_then(|d| d.get(b"Font"));
        if let Some(id) = font.and_then(Object::as_reference) {
            self.holders.insert(id);
        }
    }
}

/// Walk everything `view` draws.
pub(crate) fn enumerate(view: &DocumentView<'_>) -> GlyphUsage {
    walk(view, false)
}

/// Walk until something asks for overprint, or everything is walked.
pub(crate) fn find_overprint(view: &DocumentView<'_>) -> GlyphUsage {
    walk(view, true)
}

fn walk(view: &DocumentView<'_>, stop_at_overprint: bool) -> GlyphUsage {
    let mut walker = Walker::new(view);
    walker.stop_at_overprint = stop_at_overprint;
    let pages = match crate::page_tree::pages_in(view) {
        Ok(p) => p,
        Err(e) => {
            walker
                .usage
                .fault(format!("the page tree cannot be read: {e}"));
            return walker.usage;
        }
    };
    for page in &pages {
        if walker.stopped() {
            break;
        }
        walker.usage.holders.insert(page.id);
        walker.page = page.id;
        note_ancestors(&mut walker.usage, view, page.id);
        match crate::content::ContentStream::from_page(view, page) {
            Ok(content) => walker.content(&content, &page.resources, Font::Unset),
            Err(e) => walker.usage.fault(format!(
                "the content of page {} cannot be read: {e}",
                page.id
            )),
        }
        for stream in appearance_streams(view, page.id) {
            walker.stream(stream, &crate::object::Dict::default(), Font::Unset);
        }
    }
    walker.usage
}

/// `page` and its page-tree ancestors, with the resources each holds.
fn note_ancestors(usage: &mut GlyphUsage, view: &DocumentView<'_>, page: ObjId) {
    let mut node = Some(page);
    for _ in 0..MAX_ANCESTORS {
        let Some(id) = node else { return };
        usage.holders.insert(id);
        let Some(dict) = view.value(id).and_then(Object::as_dict) else {
            return;
        };
        if let Some(r) = dict.get(b"Resources") {
            usage.note_resources(view, r);
        }
        node = dict.get(b"Parent").and_then(Object::as_reference);
    }
}

/// Every appearance stream of `page`'s annotations (§12.5.5).
fn appearance_streams(view: &DocumentView<'_>, page: ObjId) -> Vec<ObjId> {
    let dict = |o: &Object| view.resolve(o).as_dict().cloned();
    let annots = view
        .value(page)
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Annots"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .map(<[Object]>::to_vec)
        .unwrap_or_default();
    let mut out = Vec::new();
    for annot in annots.iter().filter_map(dict) {
        let Some(ap) = annot.get(b"AP").and_then(dict) else {
            continue;
        };
        for key in [b"N".as_slice(), b"R", b"D"] {
            let Some(entry) = ap.get(key) else { continue };
            match entry.as_reference() {
                Some(id) if matches!(view.value(id), Some(Object::Stream(_))) => out.push(id),
                _ => {
                    if let Some(states) = dict(entry) {
                        out.extend(states.iter().filter_map(|(_, v)| v.as_reference()));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
