//! The length units a measurement can be read out in.

use std::fmt;
use std::str::FromStr;

/// A length unit for reading out a measurement, as a measure tool's unit
/// choice offers. Convert a model length to it with
/// `value * model_units.mm_per_unit / unit.mm_per_unit()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LengthUnit {
    /// Micrometres.
    Micrometre,
    /// Millimetres.
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Metres.
    Metre,
    /// Inches.
    Inch,
    /// Feet.
    Foot,
}

impl LengthUnit {
    /// Every unit, smallest first.
    pub const ALL: [LengthUnit; 6] = [
        LengthUnit::Micrometre,
        LengthUnit::Millimetre,
        LengthUnit::Centimetre,
        LengthUnit::Inch,
        LengthUnit::Foot,
        LengthUnit::Metre,
    ];

    /// Millimetres in one of this unit (an inch is exactly 25.4).
    #[must_use]
    pub fn mm_per_unit(self) -> f64 {
        match self {
            LengthUnit::Micrometre => 0.001,
            LengthUnit::Millimetre => 1.0,
            LengthUnit::Centimetre => 10.0,
            LengthUnit::Metre => 1000.0,
            LengthUnit::Inch => 25.4,
            LengthUnit::Foot => 304.8,
        }
    }

    /// The short name: `um`, `mm`, `cm`, `m`, `in`, `ft`. [`FromStr`]
    /// reads it back.
    ///
    /// ```
    /// use pdfcer_3d::LengthUnit;
    /// assert_eq!("in".parse::<LengthUnit>(), Ok(LengthUnit::Inch));
    /// assert_eq!(LengthUnit::Inch.symbol(), "in");
    /// assert_eq!(127.0 / LengthUnit::Inch.mm_per_unit(), 5.0);
    /// ```
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            LengthUnit::Micrometre => "um",
            LengthUnit::Millimetre => "mm",
            LengthUnit::Centimetre => "cm",
            LengthUnit::Metre => "m",
            LengthUnit::Inch => "in",
            LengthUnit::Foot => "ft",
        }
    }
}

impl fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// A unit name [`LengthUnit::symbol`] does not give.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a length unit (um, mm, cm, m, in or ft)")]
pub struct UnknownLengthUnit(pub String);

impl FromStr for LengthUnit {
    type Err = UnknownLengthUnit;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LengthUnit::ALL
            .into_iter()
            .find(|u| u.symbol() == s)
            .ok_or_else(|| UnknownLengthUnit(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_reads_back_from_its_symbol_and_converts() {
        for u in LengthUnit::ALL {
            assert_eq!(u.symbol().parse::<LengthUnit>(), Ok(u));
            assert_eq!(u.to_string(), u.symbol());
        }
        let ratio = |a: LengthUnit, b: LengthUnit| a.mm_per_unit() / b.mm_per_unit();
        assert!((ratio(LengthUnit::Foot, LengthUnit::Inch) - 12.0).abs() < 1e-12);
        assert!((ratio(LengthUnit::Metre, LengthUnit::Micrometre) - 1e6).abs() < 1e-6);
        assert!("yd".parse::<LengthUnit>().is_err() && "MM".parse::<LengthUnit>().is_err());
    }
}
