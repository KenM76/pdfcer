//! PNG prediction for image samples written as `/FlateDecode` or
//! `/LZWDecode` (ISO 32000-2 §7.4.4.4, Table 10 value 15: a per-row filter
//! tag; Table 8 gives LZW the same predictor parameters).

use std::fmt;

use crate::filters::predictor::{self, Params};
use crate::filters::{FilterError, brotli, flate, lzw};
use crate::object::{Dict, Name, Object};

/// Whether Flate- or LZW-written image samples are PNG-predicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Predictor {
    /// No prediction.
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

/// The compressor behind a predicted filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Codec {
    /// zlib at this level.
    Flate(u8),
    /// LZW, `/EarlyChange 1`.
    Lzw,
    /// Brotli at this quality.
    Brotli(u8),
}

impl Codec {
    fn compress(self, data: &[u8]) -> Result<Vec<u8>, FilterError> {
        match self {
            Self::Flate(level) => Ok(flate::encode_with_level(data, u32::from(level))),
            Self::Lzw => lzw::encode(data),
            Self::Brotli(quality) => {
                brotli::encode(data, u32::from(quality)).map_err(|e| FilterError::Corrupt {
                    filter: "BrotliDecode",
                    detail: e.to_string(),
                })
            }
        }
    }
}

/// Samples compressed for `/FlateDecode`, `/LZWDecode` or `/BrotliDecode`.
pub(crate) struct Flated {
    pub(crate) data: Vec<u8>,
    /// The `/DecodeParms` a predicted result needs.
    pub(crate) parms: Option<Dict>,
}

/// Compress `samples` (rows of `columns` samples of `colors` components at
/// `bpc` bits, byte-padded) with `codec`, predicted as `predictor` says.
pub(crate) fn compress(
    samples: &[u8],
    (colors, bpc, columns): (u32, u32, u32),
    codec: Codec,
    predictor: Predictor,
) -> Result<Flated, FilterError> {
    let plain = || {
        codec
            .compress(samples)
            .map(|data| Flated { data, parms: None })
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
        data: codec.compress(&rows)?,
        parms: Some(parms(&params)),
    };
    if predictor == Predictor::Auto {
        let base = plain()?;
        if base.data.len() <= predicted.data.len() {
            return Ok(base);
        }
    }
    Ok(predicted)
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
