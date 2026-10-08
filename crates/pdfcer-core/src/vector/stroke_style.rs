//! Line width, dash pattern and constant alpha: the model's [`Dash`], the
//! [`StrokeStyle`] request [`crate::edit::EditSession::set_object_stroke_style`]
//! takes, and the prefix it wraps each object in.
//!
//! ISO 32000-2 §8.4.3.2 (`w`), §8.4.3.6 (`d`, the dash array and phase, both
//! in user space), §8.4.5 Table 58 (`/D`, `/CA`, `/ca`) and §11.6.4.4 (the
//! alpha constants).

use crate::content::{ContentToken, ContentTokenKind};
use crate::object::Object;

use super::ImageSource;

use super::VectorEditError;
use crate::writer::content::emit_number;

/// A line dash pattern (§8.4.3.6): alternating dash and gap lengths, and how
/// far into the pattern stroking starts. Both in the object's user space.
///
/// The default — an empty array, phase 0 — is a solid line (Table 52).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dash {
    /// Dash and gap lengths, alternating, starting with a dash. An odd-length
    /// array cycles with the roles swapping each pass.
    pub array: Vec<f64>,
    /// Distance into the pattern at which stroking begins.
    pub phase: f64,
}

impl Dash {
    /// A dash pattern from its array and phase.
    ///
    /// ```
    /// use pdfcer_core::vector::Dash;
    /// let d = Dash::new(vec![3.0, 2.0], 0.0);
    /// assert!(!d.is_solid());
    /// assert!(Dash::default().is_solid());
    /// ```
    #[must_use]
    pub const fn new(array: Vec<f64>, phase: f64) -> Self {
        Self { array, phase }
    }

    /// Whether this pattern strokes a solid line: an empty array.
    #[must_use]
    pub fn is_solid(&self) -> bool {
        self.array.is_empty()
    }

    /// The §8.4.3.6 rule: finite, nonnegative and, unless empty, not all zero.
    fn check(&self) -> Result<(), VectorEditError> {
        let bad = |reason| Err(VectorEditError::InvalidStrokeStyle { reason });
        if !self.phase.is_finite() {
            return bad("the dash phase must be a finite number");
        }
        if self.array.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return bad("dash lengths must be finite and nonnegative");
        }
        if !self.array.is_empty() && self.array.iter().all(|v| *v == 0.0) {
            return bad("dash lengths must not all be zero (use an empty array for solid)");
        }
        Ok(())
    }

    /// `[a b …] phase d `.
    fn emit(&self, out: &mut Vec<u8>) {
        out.push(b'[');
        for (i, v) in self.array.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            emit_number(out, *v);
        }
        out.extend_from_slice(b"] ");
        emit_number(out, self.phase);
        out.extend_from_slice(b" d ");
    }
}

/// The `d` operator's operands: an array of numbers and a number.
pub(crate) fn dash_from_operands(operands: &[ContentToken]) -> Option<Dash> {
    let [array, phase] = operands else {
        return None;
    };
    let ContentTokenKind::Operand(Object::Array(items)) = &array.kind else {
        return None;
    };
    let ContentTokenKind::Operand(phase) = &phase.kind else {
        return None;
    };
    dash_from_parts(items, phase)
}

/// An `/ExtGState /D` value, `[[array] phase]`, its members already resolved.
pub(crate) fn dash_from_parts(items: &[Object], phase: &Object) -> Option<Dash> {
    let array = items
        .iter()
        .map(Object::as_number)
        .collect::<Option<Vec<f64>>>()?;
    Some(Dash::new(array, phase.as_number()?))
}

/// What [`crate::edit::EditSession::set_object_stroke_style`] sets. `None`
/// leaves that parameter as the object already paints it.
///
/// `width` and `dash` are in the object's user space, as
/// [`super::PathObject::line_width`] and [`super::PathObject::dash`] report
/// them. The alphas are constant opacities in `0.0..=1.0`, replacing (not
/// multiplying) the ones in force.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StrokeStyle {
    /// Line width, `>= 0`; `0` is the thinnest line the device can draw.
    pub width: Option<f64>,
    /// Dash pattern; [`Dash::default`] makes the stroke solid.
    pub dash: Option<Dash>,
    /// Stroking alpha (`/CA`).
    pub stroke_alpha: Option<f64>,
    /// Non-stroking (fill) alpha (`/ca`).
    pub fill_alpha: Option<f64>,
}

impl StrokeStyle {
    /// Whether nothing is set.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width.is_none()
            && self.dash.is_none()
            && self.stroke_alpha.is_none()
            && self.fill_alpha.is_none()
    }

    /// Whether an `/ExtGState` is needed to express this.
    #[must_use]
    pub const fn sets_alpha(&self) -> bool {
        self.stroke_alpha.is_some() || self.fill_alpha.is_some()
    }

    /// Refuse a value the operators cannot carry.
    ///
    /// # Errors
    ///
    /// [`VectorEditError::InvalidStrokeStyle`] naming the first bad value.
    pub fn validate(&self) -> Result<(), VectorEditError> {
        let bad = |reason| Err(VectorEditError::InvalidStrokeStyle { reason });
        if self.width.is_some_and(|w| !w.is_finite() || w < 0.0) {
            return bad("the line width must be finite and nonnegative");
        }
        let alpha_ok = |a: Option<f64>| a.is_none_or(|a| (0.0..=1.0).contains(&a));
        if !alpha_ok(self.stroke_alpha) || !alpha_ok(self.fill_alpha) {
            return bad("an alpha must be between 0 and 1");
        }
        self.dash.as_ref().map_or(Ok(()), Dash::check)
    }

    /// Whether this style changes how an image or form object paints. An
    /// image takes only the non-stroking alpha (§11.6.4.4: `Do` on an image
    /// is a non-stroking operation); a form takes either alpha as the
    /// starting state of its content. Width and dash never reach either.
    #[must_use]
    pub const fn fades(&self, source: ImageSource) -> bool {
        match source {
            ImageSource::Form => self.sets_alpha(),
            ImageSource::Inline | ImageSource::XObject => self.fill_alpha.is_some(),
        }
    }

    /// The `/ExtGState` dictionary for the alphas (§8.4.5 Table 58).
    pub(crate) fn ext_gstate(&self) -> crate::object::Dict {
        use crate::object::{Dict, Name};
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"ExtGState")));
        if let Some(a) = self.stroke_alpha {
            d.insert(Name::from(b"CA"), Object::Real(a));
        }
        if let Some(a) = self.fill_alpha {
            d.insert(Name::from(b"ca"), Object::Real(a));
        }
        d
    }

    /// The prefix for an image or form object: only the alphas' `gs`, so a
    /// form's own content never inherits the width or dash.
    pub(crate) fn alpha_prefix(gs_name: Option<&[u8]>) -> Vec<u8> {
        Self::default().prefix(gs_name)
    }

    /// The operators to put inside the object's `q`, with `gs_name` the
    /// resource name the alphas were bound under.
    pub(crate) fn prefix(&self, gs_name: Option<&[u8]>) -> Vec<u8> {
        let mut out = b"q ".to_vec();
        if let Some(w) = self.width {
            emit_number(&mut out, w);
            out.extend_from_slice(b" w ");
        }
        if let Some(d) = &self.dash {
            d.emit(&mut out);
        }
        if let Some(name) = gs_name {
            out.push(b'/');
            out.extend_from_slice(name);
            out.extend_from_slice(b" gs ");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefix_spells_each_operator() {
        let s = StrokeStyle {
            width: Some(2.5),
            dash: Some(Dash::new(vec![3.0, 1.0], 0.5)),
            ..StrokeStyle::default()
        };
        assert_eq!(s.prefix(Some(b"GS1")), b"q 2.5 w [3 1] 0.5 d /GS1 gs ");
        assert_eq!(StrokeStyle::default().prefix(None), b"q ");
    }

    #[test]
    fn bad_values_are_refused() {
        let all_zero = StrokeStyle {
            dash: Some(Dash::new(vec![0.0, 0.0], 0.0)),
            ..StrokeStyle::default()
        };
        assert!(all_zero.validate().is_err());
        let alpha = StrokeStyle {
            fill_alpha: Some(1.5),
            ..StrokeStyle::default()
        };
        assert!(alpha.validate().is_err());
        let width = StrokeStyle {
            width: Some(f64::NAN),
            ..StrokeStyle::default()
        };
        assert!(width.validate().is_err());
        let solid = StrokeStyle {
            dash: Some(Dash::default()),
            ..StrokeStyle::default()
        };
        assert!(solid.validate().is_ok());
    }
}
