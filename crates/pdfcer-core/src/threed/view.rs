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
    /// `/P /OS` (Table 305): the factor scaling the near plane's x and y
    /// onto the annotation's target coordinate system. Default 1; a
    /// non-positive or non-finite value reads as 1.
    pub ortho_scale: f64,
    /// `/P /OB` (Table 305, PDF 1.7): how the near plane is additionally
    /// scaled to fit the annotation. Default [`OrthoBinding::Absolute`].
    pub ortho_binding: OrthoBinding,
    /// Width and height, in default user space units, of the annotation's
    /// 3D view box (`/3DB`, else its `/Rect`; Table 298), which the
    /// projection's target coordinate system is centred on. `None` without
    /// an annotation or a well-formed rectangle.
    pub view_box: Option<[f64; 2]>,
}

impl Default for ThreeDSavedView {
    /// No name and no camera matrix; perspective, `OS` 1, `/Absolute`.
    fn default() -> Self {
        Self {
            name: String::new(),
            camera_to_world: None,
            orbit_distance: None,
            orthographic: false,
            ortho_scale: 1.0,
            ortho_binding: OrthoBinding::Absolute,
            view_box: None,
        }
    }
}

/// `/OB`, the orthographic binding (ISO 32000-1 Table 305).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OrthoBinding {
    /// `/Absolute`: no scaling due to binding.
    #[default]
    Absolute,
    /// `/W`: scale to fit the annotation's width.
    Width,
    /// `/H`: scale to fit its height.
    Height,
    /// `/Min`: scale to fit the lesser of width and height.
    Min,
    /// `/Max`: scale to fit the greater of width and height.
    Max,
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
    let annot = artwork.annot_id.and_then(|id| graph.resolved(id).as_dict());
    let annot_choice = annot.and_then(|a| a.get(b"3DV"));
    let dict = match annot_choice {
        Some(choice) if graph.resolve(choice).as_name().map(|n| n.as_bytes()) != Some(b"D") => {
            pick(graph, choice, views)?
        }
        _ => stream_default(graph, stream?, views)?,
    };
    let mut view = read_view(graph, dict);
    view.view_box = annot.and_then(|a| view_box(graph, a));
    Some(view)
}

/// Table 298 `/3DB`, else the annotation's `/Rect`: width and height.
fn view_box<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> Option<[f64; 2]> {
    [b"3DB".as_slice(), b"Rect"].iter().find_map(|k| {
        let r = graph.resolve(annot.get(k)?).as_array()?;
        let n: Vec<f64> = r
            .iter()
            .filter_map(|o| graph.resolve(o).as_number())
            .collect();
        let [x0, y0, x1, y1] = <[f64; 4]>::try_from(n).ok()?;
        let size = [(x1 - x0).abs(), (y1 - y0).abs()];
        size.iter()
            .all(|v| v.is_finite() && *v > 0.0)
            .then_some(size)
    })
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
    let projection = view.get(b"P").and_then(|o| graph.resolve(o).as_dict());
    let orthographic = projection.is_some_and(|p| name_of(graph, p, b"Subtype") == Some(b"O"));
    let ortho_scale = projection
        .and_then(|p| p.get(b"OS"))
        .and_then(|o| graph.resolve(o).as_number())
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);
    let ortho_binding = match projection.and_then(|p| name_of(graph, p, b"OB")) {
        Some(b"W") => OrthoBinding::Width,
        Some(b"H") => OrthoBinding::Height,
        Some(b"Min") => OrthoBinding::Min,
        Some(b"Max") => OrthoBinding::Max,
        _ => OrthoBinding::Absolute,
    };
    ThreeDSavedView {
        name,
        camera_to_world,
        orbit_distance,
        orthographic,
        ortho_scale,
        ortho_binding,
        view_box: None,
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
