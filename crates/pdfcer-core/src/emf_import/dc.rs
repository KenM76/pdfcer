//! The playback device context: transforms, map modes and the selected
//! objects ([MS-EMF] §3.1, §2.2.28 XForm, §2.1.21 MapMode).

use super::reader::{Header, Rec};

/// `x' = a·x + c·y + e`, `y' = b·x + d·y + f` — XForm's M11, M12, M21, M22,
/// Dx, Dy in that order, which is also a PDF `cm` operand order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Affine {
    pub(super) a: f64,
    pub(super) b: f64,
    pub(super) c: f64,
    pub(super) d: f64,
    pub(super) e: f64,
    pub(super) f: f64,
}

impl Affine {
    pub(super) const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Map a point through this matrix.
    pub(super) fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The linear part only (a direction, not a position).
    pub(super) fn vector(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    /// `self` applied first, then `next`.
    pub(super) fn then(&self, n: &Self) -> Self {
        Self {
            a: n.a * self.a + n.c * self.b,
            b: n.b * self.a + n.d * self.b,
            c: n.a * self.c + n.c * self.d,
            d: n.b * self.c + n.d * self.d,
            e: n.a * self.e + n.c * self.f + n.e,
            f: n.b * self.e + n.d * self.f + n.f,
        }
    }

    /// The determinant (negative when the transform mirrors).
    pub(super) fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// The uniform scale a pen width or dash length goes through.
    pub(super) fn length_scale(&self) -> f64 {
        let s = self.det().abs().sqrt();
        if s.is_finite() { s } else { 0.0 }
    }

    /// An XForm read at `off` ([MS-EMF] §2.2.28: six f32).
    pub(super) fn read(rec: &Rec<'_>, off: usize) -> Option<Self> {
        let v = |i: usize| rec.f32(off + i * 4).map(f64::from);
        let m = Self {
            a: v(0)?,
            b: v(1)?,
            c: v(2)?,
            d: v(3)?,
            e: v(4)?,
            f: v(5)?,
        };
        [m.a, m.b, m.c, m.d, m.e, m.f]
            .iter()
            .all(|x| x.is_finite())
            .then_some(m)
    }
}

/// Map mode plus window and viewport ([MS-EMF] §2.1.21, §3.1.1.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Mapping {
    pub(super) mode: u32,
    pub(super) win_org: (f64, f64),
    pub(super) win_ext: (f64, f64),
    pub(super) vp_org: (f64, f64),
    pub(super) vp_ext: (f64, f64),
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            mode: 1,
            win_org: (0.0, 0.0),
            win_ext: (1.0, 1.0),
            vp_org: (0.0, 0.0),
            vp_ext: (1.0, 1.0),
        }
    }
}

impl Mapping {
    /// Page space → device pixels. Metric modes are y-up; MM_TEXT is one
    /// pixel per unit; MM_ISOTROPIC forces equal-magnitude axes.
    pub(super) fn page_to_device(&self, mm_per_px: (f64, f64)) -> Affine {
        let metric = |unit_mm: f64| (unit_mm / mm_per_px.0, -unit_mm / mm_per_px.1);
        let (mut sx, mut sy) = match self.mode {
            2 => metric(0.1),
            3 => metric(0.01),
            4 => metric(0.254),
            5 => metric(0.0254),
            6 => metric(25.4 / 1440.0),
            7 | 8 => (
                ratio(self.vp_ext.0, self.win_ext.0),
                ratio(self.vp_ext.1, self.win_ext.1),
            ),
            _ => (1.0, 1.0),
        };
        if self.mode == 7 {
            let m = sx.abs().min(sy.abs());
            sx = m.copysign(sx);
            sy = m.copysign(sy);
        }
        Affine {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: self.vp_org.0 - self.win_org.0 * sx,
            f: self.vp_org.1 - self.win_org.1 * sy,
        }
    }
}

fn ratio(n: f64, d: f64) -> f64 {
    if d == 0.0 || n == 0.0 { 1.0 } else { n / d }
}

/// Device pixels → form points: the header's Frame (0.01 mm) becomes
/// `[0 0 w h]`, y up.
pub(super) fn device_to_form(h: &Header, frame: [f64; 4], height_pt: f64) -> Affine {
    let k = 72.0 / 2540.0;
    Affine {
        a: h.mm_per_px.0 * 100.0 * k,
        b: 0.0,
        c: 0.0,
        d: -h.mm_per_px.1 * 100.0 * k,
        e: -frame[0] * k,
        f: height_pt + frame[1] * k,
    }
}

pub(super) type Rgb = [u8; 3];

/// A ColorRef: R, G, B, reserved on disk ([MS-WMF] §2.2.2.8).
pub(super) fn colorref(rec: &Rec<'_>, off: usize) -> Option<Rgb> {
    rec.array(off)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Pen {
    /// PenStyle bits ([MS-WMF] §2.1.1.23).
    pub(super) style: u32,
    /// Logical units; ignored for a cosmetic pen.
    pub(super) width: f64,
    pub(super) color: Option<Rgb>,
    pub(super) geometric: bool,
    /// PS_USERSTYLE entries, logical units.
    pub(super) dashes: Vec<f64>,
}

impl Pen {
    /// A one-unit solid pen of `color`.
    pub(super) fn solid(color: Rgb) -> Self {
        Self {
            style: 0,
            width: 0.0,
            color: Some(color),
            geometric: false,
            dashes: Vec::new(),
        }
    }

    /// PS_NULL: draws nothing.
    pub(super) fn null() -> Self {
        Self {
            color: None,
            ..Self::solid([0, 0, 0])
        }
    }

    /// `None` when the pen draws nothing (PS_NULL or a null brush).
    pub(super) fn paint(&self) -> Option<Rgb> {
        if self.style & 0x0F == 5 {
            None
        } else {
            self.color
        }
    }
}

/// A brush: its fill colour, `None` for BS_NULL or an unreadable pattern.
pub(super) type Brush = Option<Rgb>;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Font {
    /// lfHeight: negative = em height, positive = cell height.
    pub(super) height: f64,
    /// lfEscapement, tenths of a degree counter-clockwise.
    pub(super) escapement: f64,
    pub(super) weight: i32,
    pub(super) italic: bool,
    pub(super) pitch_family: u8,
    pub(super) face: String,
    /// A stock font: the face is not known.
    pub(super) stock: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            height: -12.0,
            escapement: 0.0,
            weight: 400,
            italic: false,
            pitch_family: 0,
            face: String::new(),
            stock: true,
        }
    }
}

/// One clip shape in form space: path operators without the painting
/// operator, and its rule.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ClipOp {
    pub(super) path: Vec<u8>,
    pub(super) even_odd: bool,
}

/// The state EMR_SAVEDC saves ([MS-EMF] §3.1.1.1).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Dc {
    pub(super) world: Affine,
    pub(super) map: Mapping,
    pub(super) pen: Pen,
    pub(super) brush: Brush,
    pub(super) font: Font,
    pub(super) text_color: Rgb,
    pub(super) bk_color: Rgb,
    pub(super) polyfill_winding: bool,
    pub(super) text_align: u32,
    pub(super) miter: f64,
    /// Current position, logical units.
    pub(super) cur: (f64, f64),
    /// The clip, as an intersection of shapes (empty = no clip).
    pub(super) clip: Vec<ClipOp>,
}

impl Default for Dc {
    fn default() -> Self {
        Self {
            world: Affine::IDENTITY,
            map: Mapping::default(),
            pen: Pen::solid([0, 0, 0]),
            brush: Some([255, 255, 255]),
            font: Font::default(),
            text_color: [0, 0, 0],
            bk_color: [255, 255, 255],
            polyfill_winding: false,
            text_align: 0,
            miter: 10.0,
            cur: (0.0, 0.0),
            clip: Vec::new(),
        }
    }
}
