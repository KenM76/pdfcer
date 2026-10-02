//! Push-button icons: the `/MK /TP` caption position and `/IF` icon fit
//! (ISO 32000-1 §12.5.6.19 Table 189 and §12.7.7.3.2 Table 247; ISO
//! 32000-2 Tables 192 and 250), and the `/AP /N` that lays an icon and its
//! caption out by them.

use super::{
    FieldAppearance, Rect, WidgetChrome, push_button_caption, push_button_caption_font,
    push_button_plate,
};
use crate::object::{Dict, Name, ObjId, Object};
use crate::vartext::{self, FontResource, VarTextError};
use crate::writer::content::{ContentBuilder, Paint};

/// The resource name a push button's `/AP /N` draws its icon form under.
pub(crate) const ICON_RESOURCE: &[u8] = b"Icon";

/// Horizontal room either side of a caption beside an icon; the same pad
/// the variable-text generator keeps from a box edge.
const CAPTION_PAD: f64 = 2.0;

/// Where a push button's caption goes relative to its icon: `/MK /TP`
/// (ISO 32000-1 §12.5.6.19 Table 189).
///
/// # Examples
///
/// ```
/// use pdfcer_core::annot_author::CaptionPosition;
/// assert_eq!(CaptionPosition::from_tp(1), Some(CaptionPosition::IconOnly));
/// assert_eq!(CaptionPosition::CaptionBelow.to_tp(), 2);
/// assert_eq!(CaptionPosition::from_tp(7), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CaptionPosition {
    /// `0` — caption only, no icon. The default when `/TP` is absent.
    #[default]
    CaptionOnly,
    /// `1` — icon only, no caption.
    IconOnly,
    /// `2` — caption below the icon.
    CaptionBelow,
    /// `3` — caption above the icon.
    CaptionAbove,
    /// `4` — caption to the right of the icon.
    CaptionRight,
    /// `5` — caption to the left of the icon.
    CaptionLeft,
    /// `6` — caption overlaid directly on the icon.
    Overlaid,
}

impl CaptionPosition {
    /// The position a `/TP` integer names, or `None` outside `0..=6`.
    #[must_use]
    pub const fn from_tp(tp: i64) -> Option<Self> {
        Some(match tp {
            0 => Self::CaptionOnly,
            1 => Self::IconOnly,
            2 => Self::CaptionBelow,
            3 => Self::CaptionAbove,
            4 => Self::CaptionRight,
            5 => Self::CaptionLeft,
            6 => Self::Overlaid,
            _ => return None,
        })
    }

    /// The `/TP` integer.
    #[must_use]
    pub const fn to_tp(self) -> i64 {
        match self {
            Self::CaptionOnly => 0,
            Self::IconOnly => 1,
            Self::CaptionBelow => 2,
            Self::CaptionAbove => 3,
            Self::CaptionRight => 4,
            Self::CaptionLeft => 5,
            Self::Overlaid => 6,
        }
    }

    /// Whether this position draws the icon (everything but
    /// [`Self::CaptionOnly`]).
    #[must_use]
    pub const fn shows_icon(self) -> bool {
        !matches!(self, Self::CaptionOnly)
    }
}

/// When an icon is scaled to its box: `/IF /SW` (§12.7.7.3.2 Table 247).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IconScaleWhen {
    /// `/A` — always. The default.
    #[default]
    Always,
    /// `/B` — only when the icon is bigger than its box (shrink only).
    Bigger,
    /// `/S` — only when the icon is smaller than its box (grow only).
    Smaller,
    /// `/N` — never.
    Never,
}

/// How an icon is scaled: `/IF /S` (§12.7.7.3.2 Table 247).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IconScaling {
    /// `/A` — anamorphic: each axis to the box, ignoring the aspect ratio.
    Anamorphic,
    /// `/P` — proportional: the smaller factor on both axes. The default.
    #[default]
    Proportional,
}

/// A push button's icon fit dictionary, `/MK /IF` (ISO 32000-1
/// §12.7.7.3.2 Table 247). `Default` is the table's defaults: always
/// scale, proportionally, centred, inside the border.
///
/// # Examples
///
/// ```
/// use pdfcer_core::annot_author::{IconFit, IconScaling};
/// let fit = IconFit::default();
/// assert_eq!(fit.scaling, IconScaling::Proportional);
/// assert_eq!(IconFit::from_dict(&fit.to_dict()), fit);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconFit {
    /// `/SW`.
    pub scale_when: IconScaleWhen,
    /// `/S`.
    pub scaling: IconScaling,
    /// `/A`: the fraction of leftover space put left of and below the icon,
    /// each `0.0..=1.0`.
    pub align: [f64; 2],
    /// `/FB` (PDF 1.5): fit within the whole widget, ignoring the border
    /// width.
    pub full_bounds: bool,
}

impl Default for IconFit {
    fn default() -> Self {
        Self {
            scale_when: IconScaleWhen::Always,
            scaling: IconScaling::Proportional,
            align: [0.5, 0.5],
            full_bounds: false,
        }
    }
}

impl IconFit {
    /// Read an `/IF` dictionary. Direct values only; an absent, unknown or
    /// out-of-range entry takes Table 247's default (an `/A` component is
    /// clamped to `0.0..=1.0`).
    #[must_use]
    pub fn from_dict(d: &Dict) -> Self {
        let name = |k: &[u8]| d.get(k).and_then(Object::as_name).map(|n| n.0.clone());
        let mut fit = Self {
            scale_when: match name(b"SW").as_deref() {
                Some(b"B") => IconScaleWhen::Bigger,
                Some(b"S") => IconScaleWhen::Smaller,
                Some(b"N") => IconScaleWhen::Never,
                _ => IconScaleWhen::Always,
            },
            ..Self::default()
        };
        if name(b"S").as_deref() == Some(b"A") {
            fit.scaling = IconScaling::Anamorphic;
        }
        if let Some(Object::Array(a)) = d.get(b"A") {
            for (slot, v) in fit.align.iter_mut().zip(a) {
                if let Some(n) = v.as_number().filter(|n| n.is_finite()) {
                    *slot = n.clamp(0.0, 1.0);
                }
            }
        }
        fit.full_bounds = matches!(d.get(b"FB"), Some(Object::Boolean(true)));
        fit
    }

    /// The `/IF` dictionary: `/SW`, `/S` and `/A` always, `/FB` only when
    /// set.
    #[must_use]
    pub fn to_dict(&self) -> Dict {
        let sw: &[u8] = match self.scale_when {
            IconScaleWhen::Always => b"A",
            IconScaleWhen::Bigger => b"B",
            IconScaleWhen::Smaller => b"S",
            IconScaleWhen::Never => b"N",
        };
        let s: &[u8] = match self.scaling {
            IconScaling::Anamorphic => b"A",
            IconScaling::Proportional => b"P",
        };
        let mut d = Dict::new();
        d.insert(Name::from(b"SW"), Object::Name(Name(sw.to_vec())));
        d.insert(Name::from(b"S"), Object::Name(Name(s.to_vec())));
        d.insert(
            Name::from(b"A"),
            Object::Array(self.align.iter().map(|v| Object::Real(*v)).collect()),
        );
        if self.full_bounds {
            d.insert(Name::from(b"FB"), Object::Boolean(true));
        }
        d
    }

    /// The matrix placing an icon whose drawn extent is `icon` inside
    /// `area`.
    pub(crate) fn place(&self, icon: Rect, area: Rect) -> [f64; 6] {
        let (bw, bh) = (
            icon.width().max(f64::EPSILON),
            icon.height().max(f64::EPSILON),
        );
        let (fx, fy) = (area.width() / bw, area.height() / bh);
        let (fx, fy) = match self.scaling {
            IconScaling::Proportional => {
                let s = fx.min(fy);
                (s, s)
            }
            IconScaling::Anamorphic => (fx, fy),
        };
        let when = |f: f64| match self.scale_when {
            IconScaleWhen::Always => f,
            IconScaleWhen::Bigger => f.min(1.0),
            IconScaleWhen::Smaller => f.max(1.0),
            IconScaleWhen::Never => 1.0,
        };
        let (sx, sy) = (when(fx), when(fy));
        let x0 = area.llx + (area.width() - bw * sx) * self.align[0];
        let y0 = area.lly + (area.height() - bh * sy) * self.align[1];
        [sx, 0.0, 0.0, sy, x0 - icon.llx * sx, y0 - icon.lly * sy]
    }
}

/// A push button's icon as its appearance draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ButtonIcon {
    /// The icon form's drawn extent: its `/BBox` through its `/Matrix`.
    pub(crate) bounds: Rect,
    /// `/MK /TP`; [`CaptionPosition::CaptionOnly`] draws no icon.
    pub(crate) position: CaptionPosition,
    /// `/MK /IF`.
    pub(crate) fit: IconFit,
}

/// Split `area` into the icon's box and the caption's for `position`.
/// `band_h` is one caption line's height and `caption_w` the caption's
/// width; an empty caption (`caption_w` of `None`) gives the icon all of
/// `area`.
fn split(
    position: CaptionPosition,
    area: Rect,
    band_h: f64,
    caption_w: Option<f64>,
) -> (Option<Rect>, Option<Rect>) {
    let Some(cw) = caption_w else {
        return (position.shows_icon().then_some(area), None);
    };
    let band_h = band_h.min(area.height());
    let cw = (cw + 2.0 * CAPTION_PAD).min(area.width());
    let r = |llx, lly, urx, ury| Rect { llx, lly, urx, ury };
    let Rect { llx, lly, urx, ury } = area;
    match position {
        CaptionPosition::CaptionOnly => (None, Some(area)),
        CaptionPosition::IconOnly => (Some(area), None),
        CaptionPosition::Overlaid => (Some(area), Some(area)),
        CaptionPosition::CaptionBelow => (
            Some(r(llx, lly + band_h, urx, ury)),
            Some(r(llx, lly, urx, lly + band_h)),
        ),
        CaptionPosition::CaptionAbove => (
            Some(r(llx, lly, urx, ury - band_h)),
            Some(r(llx, ury - band_h, urx, ury)),
        ),
        CaptionPosition::CaptionRight => (
            Some(r(llx, lly, urx - cw, ury)),
            Some(r(urx - cw, lly, urx, ury)),
        ),
        CaptionPosition::CaptionLeft => (
            Some(r(llx + cw, lly, urx, ury)),
            Some(r(llx, lly, llx + cw, ury)),
        ),
    }
}

/// A push button's `/AP /N` with an icon: the plate and frame of
/// [`super::build_push_button_appearance`], the icon form drawn as
/// [`ICON_RESOURCE`] fitted by `icon.fit` and clipped to its box, and the
/// caption placed by `icon.position`.
///
/// The returned `/Resources` lacks the `/XObject` entry naming the icon form;
/// the caller, which knows its object, adds it.
fn build_push_button_icon_appearance(
    (width, height): (f64, f64),
    caption: &str,
    da: &[u8],
    resources: &[FontResource],
    chrome: &WidgetChrome,
    icon: &ButtonIcon,
) -> Result<FieldAppearance, VarTextError> {
    let (w, h) = (width.max(1.0), height.max(1.0));
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };
    let mut content = push_button_plate(chrome, w, h);
    let inset = if icon.fit.full_bounds {
        0.0
    } else {
        chrome
            .border_width()
            .unwrap_or(0.0)
            .min(w / 2.0)
            .min(h / 2.0)
    };
    let area = Rect {
        llx: inset,
        lly: inset,
        urx: w - inset,
        ury: h - inset,
    };
    let (font, size, _) = push_button_caption_font(h, da, resources)?;
    let caption_w = (!caption.is_empty()).then(|| vartext::text_width(font, size, caption));
    let band_h = vartext::text_band_height(font, size);
    let (icon_box, caption_box) = split(icon.position, area, band_h, caption_w);
    if let Some(b) = icon_box.filter(|b| b.width() > 0.0 && b.height() > 0.0) {
        let [a, bb, c, d, e, f] = icon.fit.place(icon.bounds, b);
        let mut cb = ContentBuilder::new();
        cb.save_state();
        cb.rect(b.llx, b.lly, b.width(), b.height());
        cb.clip_nonzero();
        cb.paint(Paint::NoPaint);
        cb.concat_matrix(a, bb, c, d, e, f);
        cb.invoke_xobject(ICON_RESOURCE);
        cb.restore_state();
        content.extend_from_slice(&cb.into_bytes());
    }
    match caption_box {
        Some(b) => {
            let laid = push_button_caption(h, b, caption, da, resources)?;
            content.extend_from_slice(&laid.content);
            Ok(laid.into_appearance(bbox, content))
        }
        None => Ok(FieldAppearance {
            ap_dict: super::form_dict(bbox, Dict::new()),
            content,
            applied_autosize: None,
            applied_autosize_bound: None,
            da_colour_unmodelled: false,
            unencodable_chars: 0,
        }),
    }
}

/// [`build_push_button_icon_appearance`] with `/Resources /XObject` naming
/// the icon form `form` as [`ICON_RESOURCE`].
pub(crate) fn push_button_with_icon(
    size: (f64, f64),
    caption: &str,
    da: &[u8],
    resources: &[FontResource],
    chrome: &WidgetChrome,
    icon: &ButtonIcon,
    form: ObjId,
) -> Result<FieldAppearance, VarTextError> {
    let mut built = build_push_button_icon_appearance(size, caption, da, resources, chrome, icon)?;
    let mut res = match built.ap_dict.get(b"Resources") {
        Some(Object::Dict(d)) => d.clone(),
        _ => Dict::new(),
    };
    let mut xobjects = Dict::new();
    xobjects.insert(Name::from(ICON_RESOURCE), Object::Reference(form));
    res.insert(Name::from(b"XObject"), Object::Dict(xobjects));
    built
        .ap_dict
        .insert(Name::from(b"Resources"), Object::Dict(res));
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
        Rect { llx, lly, urx, ury }
    }

    #[test]
    fn tp_round_trips_and_refuses_out_of_range() {
        for tp in 0..=6 {
            assert_eq!(
                CaptionPosition::from_tp(tp).map(CaptionPosition::to_tp),
                Some(tp)
            );
        }
        assert_eq!(CaptionPosition::from_tp(-1), None);
        assert_eq!(CaptionPosition::from_tp(7), None);
    }

    #[test]
    fn proportional_fit_centres_in_the_leftover_dimension() {
        let m = IconFit::default().place(r(0.0, 0.0, 32.0, 32.0), r(0.0, 0.0, 100.0, 50.0));
        assert_eq!(m, [50.0 / 32.0, 0.0, 0.0, 50.0 / 32.0, 25.0, 0.0]);
    }

    #[test]
    fn anamorphic_fit_fills_both_axes() {
        let fit = IconFit {
            scaling: IconScaling::Anamorphic,
            ..IconFit::default()
        };
        let m = fit.place(r(0.0, 0.0, 10.0, 20.0), r(5.0, 5.0, 25.0, 15.0));
        assert_eq!(m, [2.0, 0.0, 0.0, 0.5, 5.0, 5.0]);
    }

    #[test]
    fn scale_when_bigger_never_grows_and_smaller_never_shrinks() {
        let small = r(0.0, 0.0, 10.0, 10.0);
        let big_box = r(0.0, 0.0, 40.0, 40.0);
        let bigger = IconFit {
            scale_when: IconScaleWhen::Bigger,
            ..IconFit::default()
        };
        assert_eq!(bigger.place(small, big_box)[0], 1.0);
        assert_eq!(bigger.place(big_box, small)[0], 0.25);
        let smaller = IconFit {
            scale_when: IconScaleWhen::Smaller,
            ..IconFit::default()
        };
        assert_eq!(smaller.place(small, big_box)[0], 4.0);
        assert_eq!(smaller.place(big_box, small)[0], 1.0);
        let never = IconFit {
            scale_when: IconScaleWhen::Never,
            align: [0.0, 1.0],
            ..IconFit::default()
        };
        assert_eq!(never.place(small, big_box), [1.0, 0.0, 0.0, 1.0, 0.0, 30.0]);
    }

    #[test]
    fn place_offsets_a_non_zero_bbox_origin() {
        let m = IconFit::default().place(r(10.0, 20.0, 20.0, 30.0), r(0.0, 0.0, 10.0, 10.0));
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, -10.0, -20.0]);
    }

    #[test]
    fn if_dictionary_round_trips_and_defaults_unknowns() {
        let fit = IconFit {
            scale_when: IconScaleWhen::Never,
            scaling: IconScaling::Anamorphic,
            align: [0.0, 1.0],
            full_bounds: true,
        };
        assert_eq!(IconFit::from_dict(&fit.to_dict()), fit);
        let mut odd = Dict::new();
        odd.insert(Name::from(b"SW"), Object::Name(Name::from(b"Q")));
        odd.insert(
            Name::from(b"A"),
            Object::Array(vec![Object::Real(4.0), Object::Real(-1.0)]),
        );
        let read = IconFit::from_dict(&odd);
        assert_eq!(read.scale_when, IconScaleWhen::Always);
        assert_eq!(read.align, [1.0, 0.0]);
    }

    #[test]
    fn split_places_the_caption_band_per_position() {
        let area = r(0.0, 0.0, 100.0, 60.0);
        let (icon, cap) = split(CaptionPosition::CaptionBelow, area, 10.0, Some(20.0));
        assert_eq!(
            (icon, cap),
            (
                Some(r(0.0, 10.0, 100.0, 60.0)),
                Some(r(0.0, 0.0, 100.0, 10.0))
            )
        );
        let (icon, cap) = split(CaptionPosition::CaptionAbove, area, 10.0, Some(20.0));
        assert_eq!(
            (icon, cap),
            (
                Some(r(0.0, 0.0, 100.0, 50.0)),
                Some(r(0.0, 50.0, 100.0, 60.0))
            )
        );
        let (icon, cap) = split(CaptionPosition::CaptionRight, area, 10.0, Some(20.0));
        assert_eq!(
            (icon, cap),
            (
                Some(r(0.0, 0.0, 76.0, 60.0)),
                Some(r(76.0, 0.0, 100.0, 60.0))
            )
        );
        let (icon, cap) = split(CaptionPosition::CaptionLeft, area, 10.0, Some(20.0));
        assert_eq!(
            (icon, cap),
            (
                Some(r(24.0, 0.0, 100.0, 60.0)),
                Some(r(0.0, 0.0, 24.0, 60.0))
            )
        );
        assert_eq!(
            split(CaptionPosition::Overlaid, area, 10.0, Some(20.0)),
            (Some(area), Some(area))
        );
        assert_eq!(
            split(CaptionPosition::IconOnly, area, 10.0, Some(20.0)),
            (Some(area), None)
        );
        assert_eq!(
            split(CaptionPosition::CaptionBelow, area, 10.0, None),
            (Some(area), None)
        );
    }
}
