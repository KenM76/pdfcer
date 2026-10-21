//! Image facts and placements: where page content draws each image, at what
//! size, and so at what effective resolution (ISO 32000-2 §8.3.4, §8.9.4:
//! an image fills the unit square of the CTM in force at its `Do`).

use std::collections::BTreeMap;

use super::{ImagePlacement, ImageSpace, dict_of, name_of};
use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree;
use crate::view::DocumentView;

/// Form-XObject nesting past which a form is not entered.
const MAX_FORM_DEPTH: usize = 32;

/// An affine matrix `[a b c d e f]` (§8.3.3).
#[derive(Clone, Copy)]
struct Mat([f64; 6]);

impl Mat {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self × other`: apply `self`, then `other` (the `cm` rule, §8.4.4).
    fn then(self, other: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [na, nb, nc, nd, ne, nf] = other.0;
        Self([
            a * na + b * nc,
            a * nb + b * nd,
            c * na + d * nc,
            c * nb + d * nd,
            e * na + f * nc + ne,
            e * nb + f * nd + nf,
        ])
    }

    fn from_numbers(values: &[Object]) -> Option<Self> {
        let nums: Vec<f64> = values.iter().filter_map(Object::as_number).collect();
        <[f64; 6]>::try_from(nums).ok().map(Self)
    }
}

/// Facts and page placements for every image XObject in the file.
pub(super) fn image_facts(doc: &Document) -> BTreeMap<ObjId, ImageSpace> {
    let mut images: BTreeMap<ObjId, ImageSpace> = doc
        .objects()
        .filter(|o| matches!(o.value, Object::Stream(_)))
        .filter_map(|o| {
            let dict = dict_of(&o.value)?;
            (name_of(dict, b"Subtype").as_deref() == Some("Image"))
                .then(|| (o.id, facts(doc, dict)))
        })
        .collect();
    let view = doc.view();
    if let Ok(pages) = page_tree::pages(doc) {
        for (i, page) in pages.iter().enumerate() {
            let Ok(stream) = ContentStream::from_page(&view, page) else {
                continue;
            };
            let mut placer = Placer {
                doc,
                view: &view,
                page: i + 1,
                images: &mut images,
                active: Vec::new(),
            };
            placer.walk(&stream, &page.resources, Mat::IDENTITY, 0);
        }
    }
    inherit_mask_placements(doc, &mut images);
    images
}

fn facts(doc: &Document, dict: &Dict) -> ImageSpace {
    let int = |key: &[u8]| {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let color_space = dict
        .get(b"ColorSpace")
        .map(|o| doc.resolve(o))
        .and_then(|cs| match cs {
            Object::Name(n) => Some(n.as_bytes().to_vec()),
            Object::Array(a) => a
                .first()
                .and_then(Object::as_name)
                .map(|n| n.as_bytes().to_vec()),
            _ => None,
        });
    ImageSpace {
        width: int(b"Width")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
        height: int(b"Height")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
        bits_per_component: int(b"BitsPerComponent").and_then(|v| u8::try_from(v).ok()),
        color_space: color_space.map(|b| String::from_utf8_lossy(&b).into_owned()),
        components: dict
            .get(b"ColorSpace")
            .and_then(|cs| components(doc, doc.resolve(cs))),
        image_mask: matches!(
            dict.get(b"ImageMask").map(|o| doc.resolve(o)),
            Some(Object::Boolean(true))
        ),
        placements: Vec::new(),
    }
}

/// Components per sample of colour space `cs` (ISO 32000-2 §8.6), `/N`
/// read for `/ICCBased`.
fn components(doc: &Document, cs: &Object) -> Option<u8> {
    let (family, params) = match cs {
        Object::Name(n) => (n.as_bytes(), None),
        Object::Array(a) => (a.first()?.as_name()?.as_bytes(), a.get(1)),
        _ => return None,
    };
    match family {
        b"DeviceGray" | b"G" | b"CalGray" | b"Indexed" | b"I" | b"Separation" => Some(1),
        b"DeviceRGB" | b"RGB" | b"CalRGB" | b"Lab" => Some(3),
        b"DeviceCMYK" | b"CMYK" => Some(4),
        b"ICCBased" => match doc.resolve(params?) {
            Object::Stream(s) => s
                .dict
                .get(b"N")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_int)
                .and_then(|n| u8::try_from(n).ok()),
            _ => None,
        },
        b"DeviceN" => match params.map(|o| doc.resolve(o)) {
            Some(Object::Array(names)) => u8::try_from(names.len()).ok(),
            _ => None,
        },
        _ => None,
    }
}

/// A soft mask or stencil mask is drawn wherever its parent image is.
fn inherit_mask_placements(doc: &Document, images: &mut BTreeMap<ObjId, ImageSpace>) {
    let mut copies: Vec<(ObjId, Vec<ImagePlacement>)> = Vec::new();
    for (id, image) in images.iter() {
        let Some(dict) = doc.get(*id).and_then(|o| dict_of(&o.value)) else {
            continue;
        };
        for key in [&b"SMask"[..], b"Mask"] {
            if let Some(mask) = dict.get(key).and_then(Object::as_reference) {
                copies.push((mask, image.placements.clone()));
            }
        }
    }
    for (mask, placements) in copies {
        if let Some(target) = images.get_mut(&mask) {
            target.placements.extend(placements);
        }
    }
}

struct Placer<'d, 'v> {
    doc: &'d Document,
    view: &'v DocumentView<'d>,
    page: usize,
    images: &'v mut BTreeMap<ObjId, ImageSpace>,
    active: Vec<u32>,
}

impl Placer<'_, '_> {
    fn walk(&mut self, stream: &ContentStream, resources: &Dict, base: Mat, depth: usize) {
        let mut ctm = base;
        let mut stack: Vec<Mat> = Vec::new();
        for op in stream.operations() {
            match op.operator_name(&stream.buf) {
                Some(b"q") => stack.push(ctm),
                Some(b"Q") => ctm = stack.pop().unwrap_or(base),
                Some(b"cm") => {
                    if let Some(m) = Mat::from_numbers(&operand_values(&op)) {
                        ctm = m.then(ctm);
                    }
                }
                Some(b"Do") => self.draw(&op, resources, ctm, depth),
                _ => {}
            }
        }
    }

    fn draw(&mut self, op: &Operation<'_>, resources: &Dict, ctm: Mat, depth: usize) {
        let Some(Object::Name(name)) = operand_values(op).into_iter().next() else {
            return;
        };
        let Some(id) = self
            .doc
            .resolve(resources.get(b"XObject").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(name.as_bytes()))
            .and_then(Object::as_reference)
        else {
            return;
        };
        let Some(Object::Stream(xobj)) = self.view.graph().value(id) else {
            return;
        };
        match name_of(&xobj.dict, b"Subtype").as_deref() {
            Some("Image") => self.place(id, ctm),
            Some("Form") if depth < MAX_FORM_DEPTH && !self.active.contains(&id.num) => {
                let matrix = xobj
                    .dict
                    .get(b"Matrix")
                    .map(|o| self.doc.resolve(o))
                    .and_then(Object::as_array)
                    .and_then(Mat::from_numbers)
                    .unwrap_or(Mat::IDENTITY);
                let own = self
                    .doc
                    .resolve(xobj.dict.get(b"Resources").unwrap_or(&Object::Null))
                    .as_dict()
                    .filter(|d| !d.is_empty())
                    .cloned();
                let inner = self
                    .view
                    .slice(xobj.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&xobj.dict, raw).ok())
                    .and_then(|decoded| ContentStream::parse(decoded).ok());
                if let Some(inner) = inner {
                    self.active.push(id.num);
                    let res = own.unwrap_or_else(|| resources.clone());
                    self.walk(&inner, &res, matrix.then(ctm), depth + 1);
                    self.active.pop();
                }
            }
            _ => {}
        }
    }

    fn place(&mut self, id: ObjId, ctm: Mat) {
        let Some(image) = self.images.get_mut(&id) else {
            return;
        };
        let [a, b, c, d, _, _] = ctm.0;
        let width_pt = a.hypot(b);
        let height_pt = c.hypot(d);
        if width_pt < 1e-9 || height_pt < 1e-9 {
            return;
        }
        image.placements.push(ImagePlacement {
            page: self.page,
            width_pt,
            height_pt,
            ppi_x: f64::from(image.width) * 72.0 / width_pt,
            ppi_y: f64::from(image.height) * 72.0 / height_pt,
        });
    }
}

fn operand_values(op: &Operation<'_>) -> Vec<Object> {
    op.operands
        .iter()
        .filter_map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => Some(o.clone()),
            _ => None,
        })
        .collect()
}
