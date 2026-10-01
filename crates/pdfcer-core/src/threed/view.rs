//! The 3D view a reader opens an artwork on (ISO 32000-1 §13.6.2 Table 298
//! `/3DV`, §13.6.3 Table 300 `/VA` and `/DV`, §13.6.4 Table 304).

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::textstring::decode_text_string;

use super::{ThreeDArtwork, name_of, stream_at};

/// Ceiling on `/VA` entries searched for a view named by string (a pdfcer
/// guard; the spec sets none).
const MAX_VIEWS_SEARCHED: usize = 4096;

/// A 3D view dictionary's camera, as far as a renderer needs it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ThreeDSavedView {
    /// The view's `/XN` external name, the label a reader shows; empty when
    /// the dictionary has none (it is required, so the file is malformed).
    pub name: String,
    /// `/C2W` when `/MS` is `/M`: camera-to-world `[a b c d e f g h i tx
    /// ty tz]`. Columns `a b c`, `d e f`, `g h i` are the camera's x, y and
    /// z axes in world space, `tx ty tz` its position; the camera looks
    /// along its +z (§13.6.5). `None` when the view defers to the
    /// artwork's own camera (`/MS` absent, or `/U3D`), or the matrix is not
    /// twelve finite numbers.
    pub camera_to_world: Option<[f64; 12]>,
    /// `/CO`: distance from the camera to the centre of orbit along its z.
    pub orbit_distance: Option<f64>,
    /// `/P /Subtype /O` (Table 305). `false` when perspective or absent
    /// (absent means perspective).
    pub orthographic: bool,
}

/// The view `artwork` opens on: the annotation's `/3DV`, else the 3D
/// stream's `/DV`, else the stream's first `/VA` entry.
///
/// `/3DV` may be a view dictionary, an index into `/VA`, a view's `/IN`
/// name (which defaults to its `/XN`), or `/F`, `/L` or `/D` (first, last,
/// or the stream's default). `/DV` takes the same forms except `/D`.
/// `None` when nothing names a view dictionary, so the artwork's own
/// default camera applies; also for a RichMedia asset, which has no 3D
/// stream dictionary, and when `/3DV` is malformed.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::threed::{default_3d_view, list_3d};
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/minimal.pdf").to_vec(),
/// )?;
/// assert!(list_3d(&doc).iter().all(|a| default_3d_view(&doc, a).is_none()));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn default_3d_view<G: ObjectGraph + ?Sized>(
    graph: &G,
    artwork: &ThreeDArtwork,
) -> Option<ThreeDSavedView> {
    let stream = artwork
        .stream_id
        .and_then(|id| stream_at(graph, &Object::Reference(id)))
        .map(|(_, dict)| dict);
    let views = stream.map_or(&[][..], |s| {
        s.get(b"VA")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .unwrap_or_default()
    });
    let annot_choice = artwork
        .annot_id
        .and_then(|id| graph.resolved(id).as_dict())
        .and_then(|a| a.get(b"3DV"));
    let dict = match annot_choice {
        Some(choice) if graph.resolve(choice).as_name().map(|n| n.as_bytes()) != Some(b"D") => {
            pick(graph, choice, views)?
        }
        _ => stream_default(graph, stream?, views)?,
    };
    Some(read_view(graph, dict))
}

/// Table 300 `/DV`, else `/VA[0]`.
fn stream_default<'a, G: ObjectGraph + ?Sized>(
    graph: &'a G,
    stream: &'a Dict,
    views: &'a [Object],
) -> Option<&'a Dict> {
    match stream.get(b"DV") {
        Some(choice) => pick(graph, choice, views),
        None => views.first().and_then(|v| graph.resolve(v).as_dict()),
    }
}

/// Resolve one view selector against `/VA`.
fn pick<'a, G: ObjectGraph + ?Sized>(
    graph: &'a G,
    choice: &'a Object,
    views: &'a [Object],
) -> Option<&'a Dict> {
    let entry = match graph.resolve(choice) {
        Object::Dict(d) => return Some(d),
        Object::Integer(i) => views.get(usize::try_from(*i).ok()?)?,
        Object::Name(n) => match n.as_bytes() {
            b"F" => views.first()?,
            b"L" => views.last()?,
            _ => return None,
        },
        Object::String(wanted) => {
            let wanted = decode_text_string(wanted).text;
            return views
                .iter()
                .take(MAX_VIEWS_SEARCHED)
                .filter_map(|v| graph.resolve(v).as_dict())
                .find(|v| internal_name(graph, v).as_deref() == Some(wanted.as_str()));
        }
        _ => return None,
    };
    graph.resolve(entry).as_dict()
}

/// Table 304: `/IN`, defaulting to `/XN`.
fn internal_name<G: ObjectGraph + ?Sized>(graph: &G, view: &Dict) -> Option<String> {
    [b"IN".as_slice(), b"XN"]
        .iter()
        .find_map(|k| match graph.resolve(view.get(k)?) {
            Object::String(s) => Some(decode_text_string(s).text),
            _ => None,
        })
}

fn read_view<G: ObjectGraph + ?Sized>(graph: &G, view: &Dict) -> ThreeDSavedView {
    let name = match view.get(b"XN").map(|o| graph.resolve(o)) {
        Some(Object::String(s)) => decode_text_string(s).text,
        _ => String::new(),
    };
    let camera_to_world = (name_of(graph, view, b"MS") == Some(b"M"))
        .then(|| view.get(b"C2W").map(|o| graph.resolve(o)))
        .flatten()
        .and_then(Object::as_array)
        .and_then(twelve_finite);
    let orbit_distance = view
        .get(b"CO")
        .and_then(|o| graph.resolve(o).as_number())
        .filter(|d| d.is_finite());
    let orthographic = view
        .get(b"P")
        .and_then(|o| graph.resolve(o).as_dict())
        .is_some_and(|p| name_of(graph, p, b"Subtype") == Some(b"O"));
    ThreeDSavedView {
        name,
        camera_to_world,
        orbit_distance,
        orthographic,
    }
}

fn twelve_finite(items: &[Object]) -> Option<[f64; 12]> {
    let mut out = [0.0; 12];
    if items.len() != out.len() {
        return None;
    }
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = item.as_number().filter(|v| v.is_finite())?;
    }
    Some(out)
}
