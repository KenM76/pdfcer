//! Every saved view of a 3D artwork (ISO 32000-1 §13.6.3 Table 300 `/VA`,
//! `/DV`; §13.6.2 Table 298 `/3DV`).

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::textstring::decode_text_string;

use super::view::{MAX_VIEWS_SEARCHED, internal_name, read_view, view_box, views_of};
use super::{ThreeDArtwork, ThreeDSavedView};

/// An artwork's `/VA` views and which one the file names as its default.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SavedViews {
    /// Every `/VA` entry in file order, read as [`super::default_3d_view`]
    /// reads one (with the annotation's view box). An entry that is not a
    /// dictionary is skipped; at most 4,096 are read (a pdfcer guard).
    pub views: Vec<ThreeDSavedView>,
    /// The index into [`Self::views`] of the view the file names: the
    /// annotation's `/3DV`, which outranks the stream's `/DV`. `None` when
    /// neither names a `/VA` entry (a reader then opens on the first view),
    /// or when the selector is an inline view dictionary not in `/VA`.
    pub default: Option<usize>,
}

/// Every saved view of `artwork`, with the index of its named default.
///
/// Empty for a RichMedia asset (no 3D stream dictionary) or a stream
/// without `/VA`. [`crate::edit::EditSession::set_3d_views`] writes the
/// list back; reading, editing one entry and writing keeps the others.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::threed::{list_3d, saved_3d_views};
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/minimal.pdf").to_vec(),
/// )?;
/// assert!(list_3d(&doc).iter().all(|a| saved_3d_views(&doc, a).views.is_empty()));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn saved_3d_views<G: ObjectGraph + ?Sized>(graph: &G, artwork: &ThreeDArtwork) -> SavedViews {
    let (stream, entries) = views_of(graph, artwork);
    let annot = artwork.annot_id.and_then(|id| graph.resolved(id).as_dict());
    let view_box = annot.and_then(|a| view_box(graph, a));
    let mut views = Vec::new();
    let mut slots = Vec::new();
    for (i, entry) in entries.iter().take(MAX_VIEWS_SEARCHED).enumerate() {
        if let Some(d) = graph.resolve(entry).as_dict() {
            let mut v = read_view(graph, d);
            v.view_box = view_box;
            views.push(v);
            slots.push(i);
        }
    }
    let choice = annot
        .and_then(|a| a.get(b"3DV"))
        .filter(|c| graph.resolve(c).as_name().map(|n| n.as_bytes()) != Some(b"D"))
        .or_else(|| stream.and_then(|s| s.get(b"DV")));
    let default = choice
        .and_then(|c| entry_index(graph, c, entries))
        .and_then(|i| slots.iter().position(|s| *s == i));
    SavedViews { views, default }
}

/// The `/VA` index a view selector names, if it names one.
fn entry_index<G: ObjectGraph + ?Sized>(
    graph: &G,
    choice: &Object,
    entries: &[Object],
) -> Option<usize> {
    let last = entries.len().checked_sub(1)?;
    match graph.resolve(choice) {
        Object::Integer(i) => usize::try_from(*i).ok().filter(|i| *i <= last),
        Object::Name(n) => match n.as_bytes() {
            b"F" => Some(0),
            b"L" => Some(last),
            _ => None,
        },
        Object::String(wanted) => {
            let wanted = decode_text_string(wanted).text;
            entries.iter().take(MAX_VIEWS_SEARCHED).position(|v| {
                graph
                    .resolve(v)
                    .as_dict()
                    .is_some_and(|d: &Dict| internal_name(graph, d).as_deref() == Some(&wanted))
            })
        }
        _ => None,
    }
}
