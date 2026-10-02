//! The importer's local object table and the per-form content writer.
//!
//! Objects are numbered locally (`ObjId::new(index + 1, 0)`) so an import is
//! a pure value; [`crate::edit::EditSession::add_svg`] renumbers them into
//! the document at commit.

use std::collections::HashMap;

use usvg::Transform;
use usvg::tiny_skia_path::{Path, PathSegment, Point};

use super::{MAX_CONTENT_BYTES, SvgImportError};
use crate::image_import::ImportedImage;
use crate::object::{Dict, Name, ObjId, Object};
use crate::writer::content::emit_number;

/// Most objects one import may create.
const MAX_OBJECTS: usize = 200_000;

/// One object of an import, before it has a document number.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SvgObject {
    /// A dictionary object.
    Dict(Dict),
    /// A stream: dictionary (without `/Length`) and the encoded payload.
    Stream { dict: Dict, data: Vec<u8> },
    /// A raster image, staged through the image-import path at commit.
    Image(Box<ImportedImage>),
}

/// The local object table plus the running output budget.
#[derive(Debug, Default)]
pub(super) struct ObjectTable {
    pub(super) objects: Vec<SvgObject>,
    bytes: usize,
}

impl ObjectTable {
    /// Add `obj`, charging its size against [`MAX_CONTENT_BYTES`]; refuses
    /// past that or past the object-count ceiling.
    pub(super) fn push(&mut self, obj: SvgObject) -> Result<ObjId, SvgImportError> {
        let size = match &obj {
            SvgObject::Dict(_) => 64,
            SvgObject::Stream { data, .. } => data.len() + 64,
            SvgObject::Image(img) => img.data.len() + 64,
        };
        self.charge(size)?;
        if self.objects.len() >= MAX_OBJECTS {
            return Err(SvgImportError::TooComplex {
                limit: MAX_CONTENT_BYTES,
            });
        }
        self.objects.push(obj);
        let num = u32::try_from(self.objects.len()).map_err(|_| SvgImportError::TooComplex {
            limit: MAX_CONTENT_BYTES,
        })?;
        Ok(ObjId::new(num, 0))
    }

    /// Count `bytes` of output against [`MAX_CONTENT_BYTES`].
    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), SvgImportError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_CONTENT_BYTES {
            return Err(SvgImportError::TooComplex {
                limit: MAX_CONTENT_BYTES,
            });
        }
        Ok(())
    }
}

/// Which resource category a name lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Res {
    ExtGState,
    Pattern,
    XObject,
    Shading,
}

impl Res {
    const fn key(self) -> &'static [u8] {
        match self {
            Self::ExtGState => b"ExtGState",
            Self::Pattern => b"Pattern",
            Self::XObject => b"XObject",
            Self::Shading => b"Shading",
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::ExtGState => "G",
            Self::Pattern => "P",
            Self::XObject => "X",
            Self::Shading => "S",
        }
    }
}

/// A content stream under construction with its resource names.
///
/// `base` maps SVG user space of this canvas onto the form's space: the
/// y-flip for page-facing forms, identity inside a tiling pattern. No `cm`
/// is ever left active at the canvas's top level, so every nested form,
/// pattern matrix and soft mask shares that one space.
#[derive(Debug)]
pub(super) struct Canvas {
    pub(super) out: Vec<u8>,
    pub(super) base: Transform,
    pub(super) bbox: [f64; 4],
    names: HashMap<(Res, ObjId), Vec<u8>>,
    order: Vec<(Res, Vec<u8>, ObjId)>,
}

impl Canvas {
    /// An empty content stream whose drawing starts from `base`.
    pub(super) fn new(base: Transform, bbox: [f64; 4]) -> Self {
        Self {
            out: Vec::new(),
            base,
            bbox,
            names: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// A fresh canvas in the same space as `self`.
    pub(super) fn sibling(&self) -> Self {
        Self::new(self.base, self.bbox)
    }

    /// The resource name for `id`, allocating one on first use.
    pub(super) fn name(&mut self, res: Res, id: ObjId) -> Vec<u8> {
        if let Some(n) = self.names.get(&(res, id)) {
            return n.clone();
        }
        let count = self.order.iter().filter(|(r, _, _)| *r == res).count();
        let name = format!("{}{}", res.prefix(), count + 1).into_bytes();
        self.names.insert((res, id), name.clone());
        self.order.push((res, name.clone(), id));
        name
    }

    /// The `/Resources` dictionary this content needs.
    pub(super) fn resources(&self) -> Dict {
        let mut res = Dict::new();
        for kind in [Res::ExtGState, Res::Pattern, Res::XObject, Res::Shading] {
            let mut sub = Dict::new();
            for (r, name, id) in &self.order {
                if *r == kind {
                    sub.insert(Name::from(name.as_slice()), Object::Reference(*id));
                }
            }
            if !sub.is_empty() {
                res.insert(Name::from(kind.key()), Object::Dict(sub));
            }
        }
        res
    }

    /// Append an operand.
    pub(super) fn num(&mut self, v: f64) {
        emit_number(&mut self.out, v);
        self.out.push(b' ');
    }

    /// Append an operator, ending its line.
    pub(super) fn op(&mut self, op: &str) {
        self.out.extend_from_slice(op.as_bytes());
        self.out.push(b'\n');
    }

    /// Append `/name op`.
    pub(super) fn name_op(&mut self, name: &[u8], op: &str) {
        self.out.push(b'/');
        self.out.extend_from_slice(name);
        self.out.push(b' ');
        self.op(op);
    }

    /// `a b c d e f cm`.
    pub(super) fn cm(&mut self, t: Transform) {
        for v in matrix(t) {
            self.num(v);
        }
        self.op("cm");
    }

    /// The path-construction operators for `path`, in its own coordinates.
    pub(super) fn path(&mut self, path: &Path) {
        let mut start = Point::zero();
        let mut last = Point::zero();
        for seg in path.segments() {
            match seg {
                PathSegment::MoveTo(p) => {
                    self.point(p);
                    self.op("m");
                    start = p;
                    last = p;
                }
                PathSegment::LineTo(p) => {
                    self.point(p);
                    self.op("l");
                    last = p;
                }
                PathSegment::QuadTo(c, p) => {
                    // Degree elevation: c1 = p0 + 2/3 (c - p0), c2 = p + 2/3 (c - p).
                    let c1 = Point::from_xy(
                        last.x + 2.0 / 3.0 * (c.x - last.x),
                        last.y + 2.0 / 3.0 * (c.y - last.y),
                    );
                    let c2 = Point::from_xy(
                        p.x + 2.0 / 3.0 * (c.x - p.x),
                        p.y + 2.0 / 3.0 * (c.y - p.y),
                    );
                    self.point(c1);
                    self.point(c2);
                    self.point(p);
                    self.op("c");
                    last = p;
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    self.point(c1);
                    self.point(c2);
                    self.point(p);
                    self.op("c");
                    last = p;
                }
                PathSegment::Close => {
                    self.op("h");
                    last = start;
                }
            }
        }
    }

    fn point(&mut self, p: Point) {
        self.num(f64::from(p.x));
        self.num(f64::from(p.y));
    }
}

/// A tiny-skia transform as the six PDF matrix operands `[a b c d e f]`.
pub(super) fn matrix(t: Transform) -> [f64; 6] {
    [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from)
}

/// `[a b c d e f]` as a PDF array.
pub(super) fn matrix_obj(t: Transform) -> Object {
    Object::Array(matrix(t).into_iter().map(real).collect())
}

/// A number object: integer when integral, real otherwise.
pub(super) fn real(v: f64) -> Object {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        #[allow(clippy::cast_possible_truncation)] // integral and in range, checked above
        Object::Integer(v as i64)
    } else if v.is_finite() {
        Object::Real(v)
    } else {
        Object::Integer(0)
    }
}

/// A PDF name object.
pub(super) fn name(n: &[u8]) -> Object {
    Object::Name(Name::from(n))
}

/// A PDF array of reals.
pub(super) fn reals(vs: &[f64]) -> Object {
    Object::Array(vs.iter().copied().map(real).collect())
}

/// What kind of transparency group a form is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupKind {
    /// Not a group: a plain form.
    None,
    /// An isolated transparency group (a layer).
    Isolated,
    /// The group of a luminosity soft mask (`/CS /DeviceRGB`).
    Luminosity,
}

/// Close `canvas` into a Form XObject in `table` (§8.10, Table 95).
pub(super) fn finish_form(
    table: &mut ObjectTable,
    canvas: Canvas,
    group: GroupKind,
) -> Result<ObjId, SvgImportError> {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), name(b"XObject"));
    d.insert(Name::from(b"Subtype"), name(b"Form"));
    d.insert(Name::from(b"BBox"), reals(&canvas.bbox));
    d.insert(Name::from(b"Resources"), Object::Dict(canvas.resources()));
    if group != GroupKind::None {
        // §11.6.6 Table 147: /S /Transparency; /I isolates the layer.
        let mut g = Dict::new();
        g.insert(Name::from(b"S"), name(b"Transparency"));
        if group == GroupKind::Luminosity {
            g.insert(Name::from(b"CS"), name(b"DeviceRGB"));
        } else {
            g.insert(Name::from(b"I"), Object::Boolean(true));
        }
        d.insert(Name::from(b"Group"), Object::Dict(g));
    }
    let data = crate::filters::flate::encode(&canvas.out);
    d.insert(Name::from(b"Filter"), name(b"FlateDecode"));
    table.push(SvgObject::Stream { dict: d, data })
}
