//! Drawing records → path operators: lines, polys, Béziers, rectangles,
//! ellipses, path brackets and clips ([MS-EMF] §2.3.5 drawing, §2.3.10
//! path bracket, §2.3.2 clipping).

use super::dc::{ClipOp, Rgb};
use super::draw::{Bracket, Player, num};
use super::reader::Rec;

pub(super) type P = (f64, f64);

/// A path segment in logical units.
#[derive(Debug, Clone, Copy)]
pub(super) enum Seg {
    M(P),
    L(P),
    C(P, P, P),
    H,
}

/// The cubic-Bézier circle constant, 4(√2 − 1)/3.
const KAPPA: f64 = 0.552_284_749_830_793_4;

impl Player {
    /// Segments → form-space path operators under the current transform.
    pub(super) fn ops(&self, segs: &[Seg]) -> Vec<u8> {
        let m = self.xf();
        let mut out = Vec::with_capacity(segs.len() * 24);
        let pt = |out: &mut Vec<u8>, p: P| {
            let (x, y) = m.apply(p);
            num(out, x);
            num(out, y);
        };
        for s in segs {
            match *s {
                Seg::M(p) => {
                    pt(&mut out, p);
                    out.extend_from_slice(b"m\n");
                }
                Seg::L(p) => {
                    pt(&mut out, p);
                    out.extend_from_slice(b"l\n");
                }
                Seg::C(a, b, c) => {
                    pt(&mut out, a);
                    pt(&mut out, b);
                    pt(&mut out, c);
                    out.extend_from_slice(b"c\n");
                }
                Seg::H => out.extend_from_slice(b"h\n"),
            }
        }
        out
    }

    /// Drawing records; `false` when `rec` is not one.
    pub(super) fn geometry(&mut self, rec: &Rec<'_>) -> bool {
        let wide = rec.kind < 0x55;
        let ok = match rec.kind {
            0x1B => rec.point(8, 0, true).map(|p| self.move_to(p)),
            0x36 => rec
                .point(8, 0, true)
                .map(|p| self.continue_figure(vec![Seg::L(p)], p)),
            0x05 | 0x58 => poly(rec, wide).map(|pts| {
                let end = pts.as_chunks::<3>().0.last().map_or(self.dc.cur, |c| c[2]);
                self.continue_figure(beziers(&pts), end);
            }),
            0x06 | 0x59 => poly(rec, wide).map(|pts| {
                let end = pts.last().copied().unwrap_or(self.dc.cur);
                self.continue_figure(pts.into_iter().map(Seg::L).collect(), end);
            }),
            0x02 | 0x55 => poly(rec, wide).map(|pts| {
                let mut segs = Vec::with_capacity(pts.len());
                if let Some((&first, rest)) = pts.split_first() {
                    segs.push(Seg::M(first));
                    segs.extend(beziers(rest));
                }
                self.figure(&segs, false);
            }),
            0x03 | 0x56 => poly(rec, wide).map(|pts| self.figure(&polyline(&pts, true), true)),
            0x04 | 0x57 => poly(rec, wide).map(|pts| self.figure(&polyline(&pts, false), false)),
            0x07 | 0x5A | 0x08 | 0x5B => polypoly(rec, wide).map(|polys| {
                let closed = matches!(rec.kind, 0x08 | 0x5B);
                let segs: Vec<Seg> = polys.iter().flat_map(|p| polyline(p, closed)).collect();
                self.figure(&segs, closed);
            }),
            0x2B => rec
                .rectl(8)
                .map(|b| self.figure(&rect(b.map(f64::from)), true)),
            0x2A => rec
                .rectl(8)
                .map(|b| self.figure(&ellipse(b.map(f64::from)), true)),
            0x2C => rec.rectl(8).and_then(|b| {
                let corner = (f64::from(rec.i32(24)?), f64::from(rec.i32(28)?));
                self.figure(&round_rect(b.map(f64::from), corner), true);
                Some(())
            }),
            _ => return false,
        };
        if ok.is_none() {
            self.notes
                .skip(&format!("{} (malformed)", record_label(rec.kind)));
        }
        true
    }

    fn move_to(&mut self, p: P) {
        self.dc.cur = p;
        if let Some(b) = &mut self.bracket {
            b.figure_open = false;
        }
    }

    /// Segments that start at the current position (no leading `M`).
    fn continue_figure(&mut self, segs: Vec<Seg>, end: P) {
        let open = self.bracket.as_ref().is_some_and(|b| b.figure_open);
        let mut all = Vec::with_capacity(segs.len() + 1);
        if !open {
            all.push(Seg::M(self.dc.cur));
        }
        all.extend(segs);
        let ops = self.ops(&all);
        match &mut self.bracket {
            Some(b) => {
                b.ops.extend_from_slice(&ops);
                b.figure_open = true;
            }
            None => self.paint(&ops, false, true),
        }
        self.dc.cur = end;
    }

    /// A self-contained figure; `closed` ones are filled as well as stroked.
    fn figure(&mut self, segs: &[Seg], closed: bool) {
        let ops = self.ops(segs);
        match &mut self.bracket {
            Some(b) => {
                b.ops.extend_from_slice(&ops);
                b.figure_open = false;
            }
            None => self.paint(&ops, closed, true),
        }
    }

    /// Path-bracket records; `false` when `rec` is not one.
    pub(super) fn path_record(&mut self, rec: &Rec<'_>) -> bool {
        match rec.kind {
            0x3B => {
                self.bracket = Some(Bracket::default());
                self.path = None;
                self.widened = false;
            }
            0x3C => self.path = self.bracket.take().map(|b| b.ops),
            0x3D => {
                if let Some(b) = &mut self.bracket
                    && b.figure_open
                {
                    b.ops.extend_from_slice(b"h\n");
                    b.figure_open = false;
                }
            }
            0x44 => {
                self.bracket = None;
                self.path = None;
            }
            0x42 => self.widened = true,
            0x3E..=0x40 => {
                let Some(ops) = self.take_path() else {
                    return true;
                };
                let fill = rec.kind != 0x40;
                let stroke = rec.kind != 0x3E;
                if std::mem::take(&mut self.widened) {
                    self.paint_widened(&ops, fill);
                } else {
                    self.paint(&ops, fill, stroke);
                }
            }
            _ => return false,
        }
        true
    }

    /// The finished path, or else the open bracket's operators.
    pub(super) fn take_path(&mut self) -> Option<Vec<u8>> {
        self.path
            .take()
            .or_else(|| self.bracket.take().map(|b| b.ops))
    }

    /// A widened path is the outline of a stroke; filling it is drawn as
    /// that stroke in the brush colour.
    fn paint_widened(&mut self, ops: &[u8], fill: bool) {
        self.notes
            .approximate("EMR_WIDENPATH (outline drawn as a stroke)");
        let pen = self.dc.pen.clone();
        if fill {
            self.dc.pen.color = self.dc.brush;
            self.dc.pen.style &= !0x0F;
        }
        self.paint(ops, false, true);
        self.dc.pen = pen;
    }

    /// Clip records; `false` when `rec` is not one.
    pub(super) fn clip_record(&mut self, rec: &Rec<'_>) -> bool {
        match rec.kind {
            0x1E | 0x1D => {
                let Some(b) = rec.rectl(8) else {
                    return true;
                };
                let path = self.ops(&rect(b.map(f64::from)));
                self.clip_with(path, false, if rec.kind == 0x1E { 1 } else { 4 });
            }
            0x43 => {
                let mode = rec.u32(8).unwrap_or(5);
                if let Some(path) = self.take_path() {
                    let eo = !self.dc.polyfill_winding;
                    self.clip_with(path, eo, mode);
                }
            }
            0x4B => self.select_clip_region(rec),
            _ => return false,
        }
        true
    }

    /// Apply a clip shape with a RegionMode ([MS-EMF] §2.1.29): RGN_AND 1,
    /// RGN_OR 2, RGN_XOR 3, RGN_DIFF 4, RGN_COPY 5.
    fn clip_with(&mut self, path: Vec<u8>, even_odd: bool, mode: u32) {
        match mode {
            1 => self.push_clip(ClipOp { path, even_odd }),
            5 => {
                self.reset_clip();
                self.push_clip(ClipOp { path, even_odd });
            }
            4 => {
                let mut outer = self.outer_box();
                outer.extend_from_slice(&path);
                self.push_clip(ClipOp {
                    path: outer,
                    even_odd: true,
                });
            }
            2 => self.notes.skip("clip RGN_OR (clip not narrowed)"),
            3 => self.notes.skip("clip RGN_XOR (clip not narrowed)"),
            _ => self.notes.skip("clip with an unknown RegionMode"),
        }
    }

    /// EMR_EXTSELECTCLIPRGN (§2.3.2.2): RgnDataSize @8, RegionMode @12,
    /// RegionData @16 (§2.2.24: 32-byte header, CountRects @8, RectL[]).
    fn select_clip_region(&mut self, rec: &Rec<'_>) {
        let size = rec.u32(8).unwrap_or(0) as usize;
        let mode = rec.u32(12).unwrap_or(5);
        if mode == 5 && size == 0 {
            self.reset_clip();
            return;
        }
        let count = rec.u32(24).unwrap_or(0) as usize;
        if count.saturating_mul(16).saturating_add(32) > size {
            self.notes.skip("EMR_EXTSELECTCLIPRGN (malformed)");
            return;
        }
        let mut segs = Vec::with_capacity(count * 5);
        for i in 0..count {
            let Some(r) = rec.rectl(48 + i * 16) else {
                self.notes.skip("EMR_EXTSELECTCLIPRGN (malformed)");
                return;
            };
            segs.extend(rect(r.map(f64::from)));
        }
        let path = self.ops(&segs);
        self.clip_with(path, false, mode);
    }

    /// The stroke state operators for the selected pen in colour `c`.
    pub(super) fn stroke_ops(&mut self, c: Rgb) -> Vec<u8> {
        let pen = self.dc.pen.clone();
        let unit = if pen.geometric {
            self.xf().length_scale()
        } else {
            self.px_pt()
        };
        let mut s = Vec::new();
        for v in c {
            num(&mut s, f64::from(v) / 255.0);
        }
        s.extend_from_slice(b"RG\n");
        let width = if pen.geometric { pen.width * unit } else { 0.0 };
        num(&mut s, width);
        let (cap, join) = if pen.geometric {
            cap_join(pen.style)
        } else {
            (0, 0)
        };
        s.extend_from_slice(format!("w\n{cap} J {join} j\n").as_bytes());
        num(&mut s, self.dc.miter);
        s.extend_from_slice(b"M\n[");
        for d in self.dash(&pen.dashes, pen.style, width, unit) {
            num(&mut s, d);
        }
        s.extend_from_slice(b"] 0 d\n");
        s
    }

    /// The dash array: exact for PS_USERSTYLE, approximated for the
    /// predefined styles (GDI draws those device-dependent).
    fn dash(&mut self, user: &[f64], style: u32, width: f64, unit: f64) -> Vec<f64> {
        let pattern: &[f64] = match style & 0x0F {
            7 => {
                let d: Vec<f64> = user.iter().map(|v| v * unit).collect();
                return if d.iter().sum::<f64>() > 0.0 {
                    d
                } else {
                    Vec::new()
                };
            }
            1 => &[3.0, 1.0],
            2 => &[1.0, 1.0],
            3 => &[3.0, 1.0, 1.0, 1.0],
            4 => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            _ => return Vec::new(),
        };
        self.notes
            .approximate("styled pen dash pattern (approximated)");
        // Cosmetic pens: GDI's 1-pixel dash is about 6 pixels.
        let k = if width > 0.0 {
            width
        } else {
            6.0 * self.px_pt()
        };
        pattern.iter().map(|v| v * k).collect()
    }
}

/// PenStyle end cap (0xF00) and join (0xF000) → PDF `J` and `j`
/// ([MS-WMF] §2.1.1.23: ROUND 0, SQUARE 0x100, FLAT 0x200; ROUND 0,
/// BEVEL 0x1000, MITER 0x2000).
fn cap_join(style: u32) -> (u8, u8) {
    let cap = match style & 0x0F00 {
        0x0100 => 2,
        0x0200 => 0,
        _ => 1,
    };
    let join = match style & 0xF000 {
        0x1000 => 2,
        0x2000 => 0,
        _ => 1,
    };
    (cap, join)
}

fn record_label(kind: u32) -> &'static str {
    match kind {
        0x1B => "EMR_MOVETOEX",
        0x36 => "EMR_LINETO",
        0x02 | 0x55 => "EMR_POLYBEZIER",
        0x03 | 0x56 => "EMR_POLYGON",
        0x04 | 0x57 => "EMR_POLYLINE",
        0x05 | 0x58 => "EMR_POLYBEZIERTO",
        0x06 | 0x59 => "EMR_POLYLINETO",
        0x07 | 0x5A => "EMR_POLYPOLYLINE",
        0x08 | 0x5B => "EMR_POLYPOLYGON",
        0x2B => "EMR_RECTANGLE",
        0x2A => "EMR_ELLIPSE",
        _ => "EMR_ROUNDRECT",
    }
}

/// Count @24, points @28 (§2.3.5.16 and siblings).
fn poly(rec: &Rec<'_>, wide: bool) -> Option<Vec<P>> {
    let n = rec.u32(24)? as usize;
    points(rec, 28, n, wide)
}

fn points(rec: &Rec<'_>, off: usize, n: usize, wide: bool) -> Option<Vec<P>> {
    let stride = if wide { 8 } else { 4 };
    if off.checked_add(n.checked_mul(stride)?)? > rec.data.len() {
        return None;
    }
    (0..n).map(|i| rec.point(off, i, wide)).collect()
}

/// NumberOfPolygons @24, Count @28, PolygonPointCount[N] @32, points after
/// (§2.3.5.28).
fn polypoly(rec: &Rec<'_>, wide: bool) -> Option<Vec<Vec<P>>> {
    let n = rec.u32(24)? as usize;
    let total = rec.u32(28)? as usize;
    if 32usize.checked_add(n.checked_mul(4)?)? > rec.data.len() {
        return None;
    }
    let counts: Vec<usize> = (0..n)
        .map(|i| rec.u32(32 + i * 4).map(|c| c as usize))
        .collect::<Option<_>>()?;
    if counts.iter().try_fold(0usize, |a, &c| a.checked_add(c))? != total {
        return None;
    }
    let all = points(rec, 32 + n * 4, total, wide)?;
    let mut rest = all.as_slice();
    let mut out = Vec::with_capacity(n);
    for c in counts {
        let (head, tail) = rest.split_at(c);
        out.push(head.to_vec());
        rest = tail;
    }
    Some(out)
}

fn polyline(pts: &[P], closed: bool) -> Vec<Seg> {
    let mut segs = Vec::with_capacity(pts.len() + 1);
    for (i, &p) in pts.iter().enumerate() {
        segs.push(if i == 0 { Seg::M(p) } else { Seg::L(p) });
    }
    if closed && !segs.is_empty() {
        segs.push(Seg::H);
    }
    segs
}

/// Groups of three points (c1, c2, end); a partial trailing group is
/// ignored.
fn beziers(pts: &[P]) -> Vec<Seg> {
    pts.as_chunks::<3>()
        .0
        .iter()
        .map(|c| Seg::C(c[0], c[1], c[2]))
        .collect()
}

/// A closed rectangle `[left, top, right, bottom]` as path segments.
pub(super) fn rect([l, t, r, b]: [f64; 4]) -> Vec<Seg> {
    vec![
        Seg::M((l, t)),
        Seg::L((r, t)),
        Seg::L((r, b)),
        Seg::L((l, b)),
        Seg::H,
    ]
}

fn ellipse([l, t, r, b]: [f64; 4]) -> Vec<Seg> {
    let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0, (b - t) / 2.0);
    let (kx, ky) = (KAPPA * rx, KAPPA * ry);
    vec![
        Seg::M((cx + rx, cy)),
        Seg::C((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry)),
        Seg::C((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy)),
        Seg::C((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry)),
        Seg::C((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy)),
        Seg::H,
    ]
}

/// EMR_ROUNDRECT's Corner is the corner ellipse's width and height
/// (§2.3.5.35).
fn round_rect([l, t, r, b]: [f64; 4], corner: P) -> Vec<Seg> {
    let (x0, x1, y0, y1) = (l.min(r), l.max(r), t.min(b), t.max(b));
    let rx = (corner.0.abs() / 2.0).min((x1 - x0) / 2.0);
    let ry = (corner.1.abs() / 2.0).min((y1 - y0) / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        return rect([l, t, r, b]);
    }
    let (kx, ky) = ((1.0 - KAPPA) * rx, (1.0 - KAPPA) * ry);
    vec![
        Seg::M((x0 + rx, y0)),
        Seg::L((x1 - rx, y0)),
        Seg::C((x1 - kx, y0), (x1, y0 + ky), (x1, y0 + ry)),
        Seg::L((x1, y1 - ry)),
        Seg::C((x1, y1 - ky), (x1 - kx, y1), (x1 - rx, y1)),
        Seg::L((x0 + rx, y1)),
        Seg::C((x0 + kx, y1), (x0, y1 - ky), (x0, y1 - ry)),
        Seg::L((x0, y0 + ry)),
        Seg::C((x0, y0 + ky), (x0 + kx, y0), (x0 + rx, y0)),
        Seg::H,
    ]
}
