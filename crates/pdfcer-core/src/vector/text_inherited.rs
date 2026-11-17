//! The graphics state a text object inherits at its `BT`.
//!
//! Colour and the text-state parameters are graphics state (ISO 32000-2
//! §8.4.1 Table 52, §9.3.1 Table 105), so a producer may set them once and
//! let many `BT`…`ET` blocks inherit them. A text object's bytes are its
//! `BT`…`ET`, so what it inherits is not in them.

use super::decompose::{DevicePaintSpace, PathPaint};
use crate::text_state::TextStateParams;
use crate::writer::content::emit_number;

/// The colour and text state in force at a text object's `BT`.
///
/// This is what the object **starts** with, not necessarily what every
/// glyph is painted with: an operator inside the object may replace any of
/// it part-way through.
#[derive(Debug, Clone, PartialEq)]
pub struct TextInherited {
    /// The non-stroking colour: what fills glyphs in rendering modes 0, 2,
    /// 4 and 6 (§9.3.6).
    pub fill: PathPaint,
    /// The stroking colour: what outlines glyphs in modes 1, 2, 5 and 6.
    pub stroke: PathPaint,
    /// `Tc`, `Tw`, `Tz`, `TL`, `Ts` and `Tr` (§9.3).
    pub text_state: TextStateParams,
}

impl Default for TextInherited {
    /// A page's initial state: black, and Table 105's initial values.
    fn default() -> Self {
        Self {
            fill: PathPaint::Default,
            stroke: PathPaint::Default,
            text_state: TextStateParams::INITIAL,
        }
    }
}

impl TextInherited {
    /// Appends the operators that re-establish this state ahead of the
    /// object's own bytes.
    ///
    /// Both colours are always written, because the destination's ambient
    /// colour is unknown; a text-state parameter is written only when it
    /// differs from its initial value. A colour in a named space is written
    /// as `/Name cs … scn`, whose name the caller must bind like any other
    /// resource. A pattern colour is not written: its pattern name is not in
    /// the model.
    pub(crate) fn emit(&self, out: &mut Vec<u8>) {
        emit_paint(out, &self.fill, false);
        emit_paint(out, &self.stroke, true);
        let t = &self.text_state;
        let i = TextStateParams::INITIAL;
        #[allow(clippy::float_cmp)] // "Was it ever set" is an exact question.
        let params: [(f64, f64, &[u8]); 5] = [
            (t.char_spacing, i.char_spacing, b"Tc"),
            (t.word_spacing, i.word_spacing, b"Tw"),
            (t.h_scale * 100.0, i.h_scale * 100.0, b"Tz"),
            (t.leading, i.leading, b"TL"),
            (t.rise, i.rise, b"Ts"),
        ];
        for (value, initial, op) in params {
            #[allow(clippy::float_cmp)] // As above.
            if value != initial && value.is_finite() {
                operator(out, &[value], op);
            }
        }
        if t.render_mode != i.render_mode && (0..=7).contains(&t.render_mode) {
            sep(out);
            out.extend_from_slice(t.render_mode.to_string().as_bytes());
            out.extend_from_slice(b" Tr");
        }
    }
}

/// A space between operators, when something is already written.
fn sep(out: &mut Vec<u8>) {
    if out.last().is_some_and(|b| !b.is_ascii_whitespace()) {
        out.push(b' ');
    }
}

fn operator(out: &mut Vec<u8>, operands: &[f64], op: &[u8]) {
    sep(out);
    for &v in operands {
        emit_number(out, v);
        out.push(b' ');
    }
    out.extend_from_slice(op);
}

/// Writes the operator(s) that set `paint`, or nothing when it cannot be
/// re-established from the model.
fn emit_paint(out: &mut Vec<u8>, paint: &PathPaint, stroking: bool) {
    let pick = |fill: &'static [u8], stroke: &'static [u8]| if stroking { stroke } else { fill };
    match paint {
        PathPaint::Default => operator(out, &[0.0], pick(b"g", b"G")),
        PathPaint::Device { space, comps, .. } => {
            let (count, op) = match space {
                DevicePaintSpace::Gray => (1, pick(b"g", b"G")),
                DevicePaintSpace::Rgb => (3, pick(b"rg", b"RG")),
                DevicePaintSpace::Cmyk => (4, pick(b"k", b"K")),
            };
            if comps.len() == count && comps.iter().all(|c| c.is_finite()) {
                operator(out, comps, op);
            }
        }
        PathPaint::Other {
            space: Some(name),
            comps,
            pattern: false,
        } if comps.iter().all(|c| c.is_finite()) => {
            sep(out);
            out.push(b'/');
            out.extend_from_slice(name);
            out.push(b' ');
            out.extend_from_slice(pick(b"cs", b"CS"));
            if !comps.is_empty() {
                operator(out, comps, pick(b"scn", b"SCN"));
            }
        }
        PathPaint::Other { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::geometry::Rgb;

    fn emitted(state: &TextInherited) -> String {
        let mut out = Vec::new();
        state.emit(&mut out);
        String::from_utf8(out).unwrap_or_default()
    }

    #[test]
    fn the_initial_state_writes_black_and_no_text_state() {
        assert_eq!(emitted(&TextInherited::default()), "0 g 0 G");
    }

    #[test]
    fn a_device_colour_is_written_in_its_own_space() {
        let fill = PathPaint::Device {
            space: DevicePaintSpace::Cmyk,
            comps: vec![0.0, 1.0, 1.0, 0.0],
            rgb: Rgb::BLACK,
        };
        let stroke = PathPaint::Device {
            space: DevicePaintSpace::Rgb,
            comps: vec![0.0, 0.0, 1.0],
            rgb: Rgb::BLACK,
        };
        let state = TextInherited {
            fill,
            stroke,
            ..TextInherited::default()
        };
        assert_eq!(emitted(&state), "0 1 1 0 k 0 0 1 RG");
    }

    #[test]
    fn a_named_space_is_written_by_name_and_a_pattern_is_not() {
        let fill = PathPaint::Other {
            space: Some(b"CS0".to_vec()),
            comps: vec![0.5],
            pattern: false,
        };
        let stroke = PathPaint::Other {
            space: Some(b"Pattern".to_vec()),
            comps: Vec::new(),
            pattern: true,
        };
        let state = TextInherited {
            fill,
            stroke,
            ..TextInherited::default()
        };
        assert_eq!(emitted(&state), "/CS0 cs 0.5 scn");
    }

    #[test]
    fn only_the_text_state_that_was_set_is_written() {
        let mut state = TextInherited::default();
        state.text_state.char_spacing = 2.0;
        state.text_state.h_scale = 0.5;
        state.text_state.render_mode = 1;
        assert_eq!(emitted(&state), "0 g 0 G 2 Tc 50 Tz 1 Tr");
    }
}
