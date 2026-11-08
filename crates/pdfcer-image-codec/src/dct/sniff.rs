//! Marker pre-sniff (ITU-T T.81 B.1): the frame facts the adapter needs
//! before decoding, and the by-name refusal of frame types zune-jpeg lacks.

use super::corrupt_detail;
use crate::ImageCodecError;

/// What the marker walk learned about the codestream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Frame {
    /// Components per sample from the SOF header.
    pub(super) components: u8,
    /// The Adobe APP14 transform byte, if the marker is present.
    pub(super) adobe_transform: Option<u8>,
}

/// Walk the codestream's marker chain to SOS, refusing unsupported
/// frame types **by name**.
///
/// This is the same walk decision 005 §3.2 used to measure the corpus's
/// 70 JPEG codestreams, and it exists for three reasons that a
/// "just call the decoder and map its error" approach cannot serve:
///
/// 1. `zune-jpeg` reports arithmetic-coded, lossless and 12-bit JPEG
///    through a single `UnsupportedSchemes` error whose text is not a
///    stable key; rule R27 requires a stable, countable name per
///    feature.
/// 2. An Adobe transform byte outside 0–2 is a **hard error** in
///    `zune-jpeg`'s header parser, so the value would never reach
///    pdfcer's Table 13 logic at all.
/// 3. Table 13's own default rule needs the component count *before*
///    deciding what to ask the decoder for.
///
/// T.81 §B.1.1.3: markers are `0xFF` followed by a non-zero, non-`0xFF`
/// byte; `0xFF00` is a stuffed data byte and repeated `0xFF`s are fill.
/// Standalone markers (SOI, EOI, TEM, RST0–7) carry no length; every
/// other marker segment starts with a 2-byte big-endian length that
/// *includes* those two bytes.
pub(super) fn sniff(data: &[u8]) -> Result<Frame, ImageCodecError> {
    let mut frame = Frame::default();
    let mut i = 0usize;
    let mut seen_sof = false;

    // T.81 §B.2.1: the codestream begins with SOI.
    if data.first() != Some(&0xFF) || data.get(1) != Some(&0xD8) {
        return Err(corrupt_detail("codestream does not begin with SOI"));
    }
    i += 2;

    loop {
        // Skip fill bytes; find the next marker prefix.
        while data.get(i) == Some(&0xFF) && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        let (Some(&0xFF), Some(&marker)) = (data.get(i), data.get(i + 1)) else {
            // Ran out of markers without ever reaching a scan. If a SOF
            // was seen we already know everything this walk is for;
            // otherwise the decoder will reject it in a moment anyway.
            return if seen_sof {
                Ok(frame)
            } else {
                Err(corrupt_detail("no SOF marker before end of data"))
            };
        };
        i += 2;

        match marker {
            // Standalone markers: no length field (T.81 §B.1.1.3).
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // Start of scan — everything this walk needs precedes it.
            0xDA => {
                return if seen_sof {
                    Ok(frame)
                } else {
                    Err(corrupt_detail("SOS before any SOF marker"))
                };
            }
            0xD9 => {
                return if seen_sof {
                    Ok(frame)
                } else {
                    Err(corrupt_detail("EOI before any SOF marker"))
                };
            }
            _ => {}
        }

        // Every other marker introduces a segment: a 2-byte length that
        // counts itself, then `length - 2` payload bytes.
        let (Some(&hi), Some(&lo)) = (data.get(i), data.get(i + 1)) else {
            return Err(corrupt_detail("truncated marker segment length"));
        };
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let Some(payload_len) = length.checked_sub(2) else {
            return Err(corrupt_detail("marker segment length below 2"));
        };
        let Some(payload) = data.get(i + 2..i + 2 + payload_len) else {
            return Err(corrupt_detail("marker segment runs past end of data"));
        };
        i += 2 + payload_len;

        match marker {
            // SOF markers. The gaps are real: 0xC4 is DHT, 0xC8 is a
            // reserved JPG extension, 0xCC is DAC — none of them frame
            // headers, and treating them as such would read a component
            // count out of a Huffman table.
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if seen_sof {
                    // A second frame header means a hierarchical or
                    // multi-frame stream; the first one is what the
                    // decoder will use.
                    continue;
                }
                seen_sof = true;
                if let Some(feature) = unsupported_sof(marker) {
                    return Err(ImageCodecError::FeatureUnsupported { feature });
                }
                // SOF payload: precision(1), height(2), width(2),
                // component count(1), then per-component triples.
                let (Some(&precision), Some(&components)) = (payload.first(), payload.get(5))
                else {
                    return Err(corrupt_detail("truncated SOF header"));
                };
                if precision != 8 {
                    return Err(ImageCodecError::FeatureUnsupported {
                        feature: match precision {
                            12 => "DCT/12-bit",
                            16 => "DCT/16-bit",
                            _ => "DCT/precision-unsupported",
                        },
                    });
                }
                frame.components = components;
            }
            // APP14. Only an "Adobe"-identified segment carries a
            // transform byte; any other APP14 is a different vendor's
            // and is ignored (T.81 leaves APPn contents to the
            // application).
            0xEE => {
                // Adobe layout: "Adobe"(5), version(2), flags0(2),
                // flags1(2), transform(1) = 12 payload bytes.
                if payload.starts_with(b"Adobe")
                    && let Some(&transform) = payload.get(11)
                {
                    frame.adobe_transform = Some(transform);
                }
            }
            _ => {}
        }
    }
}

/// Map an unsupported SOF marker to its stable diagnostic key (R27).
///
/// Returns `None` for the three frame types `zune-jpeg` implements:
/// SOF0 (baseline sequential), SOF1 (extended sequential) and SOF2
/// (progressive) — which between them cover 100% of the 70 codestreams
/// measured in decision 005 §3.2 (77% SOF0, 14% SOF2, 9% SOF0
/// grayscale).
const fn unsupported_sof(marker: u8) -> Option<&'static str> {
    match marker {
        // SOF0, SOF1, SOF2.
        0xC0..=0xC2 => None,
        // Lossless: SOF3 (huffman), SOF7 (differential), SOF11
        // (arithmetic), SOF15 (differential arithmetic). Named
        // "lossless" first because that is the property that makes them
        // a different codec, not merely a different entropy coder.
        0xC3 | 0xC7 | 0xCB | 0xCF => Some("DCT/lossless"),
        // Arithmetic entropy coding: SOF9, SOF10, SOF13, SOF14.
        0xC9 | 0xCA | 0xCD | 0xCE => Some("DCT/arithmetic"),
        // Differential (hierarchical) huffman: SOF5, SOF6.
        0xC5 | 0xC6 => Some("DCT/differential"),
        _ => Some("DCT/unsupported-frame"),
    }
}
