//! PNG prediction for image samples written as `/FlateDecode`
//! (ISO 32000-2 §7.4.4.4, Table 10 value 15: a per-row filter tag).

use std::fmt;

use crate::filters::flate;
use crate::filters::predictor::{self, Params};
use crate::object::{Dict, Name, Object};

/// Whether Flate-written image samples are PNG-predicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Predictor {
    /// Plain Flate.
    Off,
    /// `/Predictor 15`, each row filtered by whichever PNG filter suits it.
    Png,
    /// Both, keeping the smaller (plain on a tie).
    #[default]
    Auto,
}

impl Predictor {
    /// Every choice, in spelling order.
    pub const ALL: [Self; 3] = [Self::Off, Self::Png, Self::Auto];

    /// The profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Png => "png",
            Self::Auto => "auto",
        }
    }

    /// The choice spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == name)
    }
}

impl fmt::Display for Predictor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Samples compressed for `/FlateDecode`.
pub(crate) struct Flated {
    pub(crate) data: Vec<u8>,
    /// The `/DecodeParms` a predicted result needs.
    pub(crate) parms: Option<Dict>,
}

/// Flate `samples` (rows of `columns` samples of `colors` components at
/// `bpc` bits, byte-padded) at `level`, predicted as `predictor` says.
pub(crate) fn flate(
    samples: &[u8],
    (colors, bpc, columns): (u32, u32, u32),
    level: u8,
    predictor: Predictor,
) -> Flated {
    let plain = || Flated {
        data: flate::encode_with_level(samples, u32::from(level)),
        parms: None,
    };
    if predictor == Predictor::Off {
        return plain();
    }
    let params = Params {
        predictor: 15,
        colors,
        bits_per_component: bpc,
        columns,
    };
    let Ok(rows) = predictor::predict_png(samples, &params) else {
        return plain();
    };
    let predicted = Flated {
        data: flate::encode_with_level(&rows, u32::from(level)),
        parms: Some(parms(&params)),
    };
    if predictor == Predictor::Auto {
        let base = plain();
        if base.data.len() <= predicted.data.len() {
            return base;
        }
    }
    predicted
}

fn parms(p: &Params) -> Dict {
    let mut d = Dict::new();
    for (k, v) in [
        (&b"Predictor"[..], u32::from(p.predictor)),
        (b"Colors", p.colors),
        (b"BitsPerComponent", p.bits_per_component),
        (b"Columns", p.columns),
    ] {
        d.insert(Name::from(k), Object::Integer(i64::from(v)));
    }
    d
}
