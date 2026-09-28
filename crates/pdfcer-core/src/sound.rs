//! Sound objects (ISO 32000-1 §13.3, Table 294; 2.0 Table 305) and the
//! WAV (RIFF) importer that produces them.
//!
//! A [`SoundData`] holds samples already in the PDF layout: big-endian,
//! interleaved left-first, described by `/R /C /B /E`. [`SoundData::from_wav`]
//! is the only constructor; it reports every conversion it made
//! ([`SoundConversion`]) so a shell can disclose it.
//!
//! Sound objects are deprecated in PDF 2.0 (§13.3 opening paragraph: "should
//! not be written"). They remain valid and pdfcer writes them on request.
//!
//! Spec ambiguities, each an option on [`WavImportOptions`]:
//! - §13.3's portability paragraph says Raw/Signed sound "shall" be 11,025 or
//!   22,050 Hz while the same paragraph asks readers to support 8000 and to
//!   resample "as necessary". [`SoundRatePolicy::KeepNative`] (default) keeps
//!   the recorded rate; [`SoundRatePolicy::SpecRate`] resamples.
//! - Channel order beyond stereo is undefined, so more than two channels are
//!   refused unless [`WavImportOptions::downmix`] averages them to mono.
//! - There is no float `/E`; float WAV is converted to Signed 16-bit.

use crate::object::{Dict, Name, Object};

/// Largest sample buffer [`SoundData::from_wav`] will produce, in bytes
/// (`ARCHITECTURE.md` §10: every decoder has an output ceiling).
pub const MAX_SOUND_BYTES: usize = 512 * 1024 * 1024;

/// The sample encoding, `/E` (Table 294).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEncoding {
    /// `/Raw` — unsigned, 0 to 2^B − 1 (the default when `/E` is absent).
    Raw,
    /// `/Signed` — two's complement.
    Signed,
    /// `/muLaw` — µ-law.
    MuLaw,
    /// `/ALaw` — A-law.
    ALaw,
}

impl SoundEncoding {
    /// The `/E` name bytes.
    #[must_use]
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Raw => b"Raw",
            Self::Signed => b"Signed",
            Self::MuLaw => b"muLaw",
            Self::ALaw => b"ALaw",
        }
    }
}

/// How to treat a sample rate other than 11,025 or 22,050 Hz for Raw/Signed
/// sound, or other than 8000 Hz for µ-law (§13.3 portability paragraph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoundRatePolicy {
    /// Keep the recorded rate. Every reader §13.3 describes resamples on
    /// playback; the file stays exactly as recorded.
    #[default]
    KeepNative,
    /// Resample PCM to 11,025 Hz (sources at or below 16,537 Hz) or
    /// 22,050 Hz, by linear interpolation. µ-law not at 8000 Hz mono is
    /// refused rather than re-encoded.
    SpecRate,
}

/// Options for [`SoundData::from_wav`]. Fields are public and the type is
/// exhaustive so a caller can write `WavImportOptions { downmix: true,
/// ..Default::default() }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WavImportOptions {
    /// The sample-rate reading to apply.
    pub rate_policy: SoundRatePolicy,
    /// Average more than two channels to mono instead of refusing.
    pub downmix: bool,
}

/// A conversion [`SoundData::from_wav`] applied — disclosed, never silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SoundConversion {
    /// Float samples of `from_bits` converted to Signed 16-bit.
    FloatToSigned16 {
        /// The source float width, 32 or 64.
        from_bits: u16,
    },
    /// Resampled by linear interpolation.
    Resampled {
        /// The source rate, Hz.
        from: u32,
        /// The written rate, Hz.
        to: u32,
    },
    /// Averaged to one channel.
    Downmixed {
        /// The source channel count.
        from_channels: u16,
    },
}

/// Why a WAV file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WavError {
    /// Not a `RIFF`/`WAVE` file.
    #[error("not a WAV file (no RIFF/WAVE header)")]
    NotWav,
    /// No `fmt ` chunk, or one too short to read.
    #[error("the WAV file has no usable fmt chunk")]
    MissingFormat,
    /// No `data` chunk.
    #[error("the WAV file has no data chunk")]
    MissingData,
    /// A format tag or sample width pdfcer cannot express as a sound object.
    #[error("unsupported WAV format: tag {tag:#06x}, {bits} bits per sample")]
    UnsupportedFormat {
        /// The `wFormatTag` (or extensible sub-format).
        tag: u16,
        /// `wBitsPerSample`.
        bits: u16,
    },
    /// A field that makes the file internally inconsistent.
    #[error("malformed WAV file: {0}")]
    Malformed(&'static str),
    /// More than two channels without [`WavImportOptions::downmix`].
    #[error(
        "{channels} channels: a PDF sound object defines at most stereo (§13.3); \
         allow downmixing to mono"
    )]
    TooManyChannels {
        /// The channel count.
        channels: u16,
    },
    /// Under [`SoundRatePolicy::SpecRate`], µ-law not at 8000 Hz mono.
    #[error(
        "µ-law sound must be 8000 Hz mono under the spec-rate policy; got {rate} Hz, {channels} channel(s)"
    )]
    MuLawNotConformant {
        /// The rate, Hz.
        rate: u32,
        /// The channel count.
        channels: u16,
    },
    /// The result would exceed [`MAX_SOUND_BYTES`].
    #[error("the converted sound would exceed {MAX_SOUND_BYTES} bytes")]
    TooLarge,
}

/// Sound samples in the PDF layout (§13.3): big-endian, interleaved with
/// the left channel first.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SoundData {
    /// `/R`, samples per second.
    pub rate: u32,
    /// `/C`, 1 or 2.
    pub channels: u8,
    /// `/B`, bits per sample per channel.
    pub bits: u8,
    /// `/E`.
    pub encoding: SoundEncoding,
    /// The stream body, before any `/Filter`.
    pub samples: Vec<u8>,
}

/// What [`SoundData::from_wav`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WavImport {
    /// The sound, ready to embed.
    pub sound: SoundData,
    /// Every conversion applied, in order. Empty = the samples are the
    /// recording's, byte-reordered only.
    pub conversions: Vec<SoundConversion>,
}

struct Format {
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
}

const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_ALAW: u16 = 6;
const TAG_MULAW: u16 = 7;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Walk the RIFF chunks and return the format and the `data` body.
fn parse_riff(bytes: &[u8]) -> Result<(Format, &[u8]), WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotWav);
    }
    let mut fmt = None;
    let mut data = None;
    let mut at = 12usize;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let size = size as usize;
        let body_at = at + 8;
        // A truncated final chunk yields what is present (a common producer
        // defect for `data` written before its length was known).
        let end = body_at.saturating_add(size).min(bytes.len());
        let body = bytes.get(body_at..end).unwrap_or(&[]);
        match id {
            b"fmt " if fmt.is_none() => fmt = Some(body),
            b"data" if data.is_none() => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned; the pad byte is not counted in `size`.
        at = match body_at
            .checked_add(size)
            .and_then(|e| e.checked_add(size & 1))
        {
            Some(next) => next,
            None => break,
        };
    }
    let fmt = fmt.ok_or(WavError::MissingFormat)?;
    let data = data.ok_or(WavError::MissingData)?;
    let mut tag = u16_at(fmt, 0).ok_or(WavError::MissingFormat)?;
    let channels = u16_at(fmt, 2).ok_or(WavError::MissingFormat)?;
    let rate = u32_at(fmt, 4).ok_or(WavError::MissingFormat)?;
    let bits = u16_at(fmt, 14).ok_or(WavError::MissingFormat)?;
    if tag == TAG_EXTENSIBLE {
        // WAVEFORMATEXTENSIBLE: the SubFormat GUID begins at 24; its first
        // two bytes are the effective format tag.
        tag = u16_at(fmt, 24).ok_or(WavError::MissingFormat)?;
    }
    if channels == 0 {
        return Err(WavError::Malformed("zero channels"));
    }
    if rate == 0 {
        return Err(WavError::Malformed("zero sample rate"));
    }
    Ok((
        Format {
            tag,
            channels,
            rate,
            bits,
        },
        data,
    ))
}

impl SoundData {
    /// Import a WAV (RIFF `WAVE`) file as a PDF sound object.
    ///
    /// PCM of 8, 16, 24 or 32 bits keeps its width (8-bit as `/Raw`, wider as
    /// `/Signed`, byte-reversed to big-endian); µ-law and A-law are copied;
    /// 32/64-bit float becomes Signed 16-bit. A trailing partial frame is
    /// dropped. Rate and channel handling follow `options`; every change is
    /// listed in [`WavImport::conversions`].
    ///
    /// # Errors
    ///
    /// [`WavError`] for a file that is not WAV, is malformed, uses a format
    /// with no sound-object equivalent, has more than two channels without
    /// `downmix`, breaks the µ-law rule under [`SoundRatePolicy::SpecRate`],
    /// or would exceed [`MAX_SOUND_BYTES`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::sound::{SoundData, SoundEncoding, WavImportOptions};
    ///
    /// // A 16-bit mono WAV holding two samples, 1 and -2.
    /// let mut wav = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x22\x56\0\0\x44\xac\0\0\x02\0\x10\0data\x04\0\0\0".to_vec();
    /// wav.extend_from_slice(&[0x01, 0x00, 0xfe, 0xff]);
    /// let import = SoundData::from_wav(&wav, &WavImportOptions::default())?;
    /// assert_eq!(import.sound.encoding, SoundEncoding::Signed);
    /// assert_eq!(import.sound.samples, [0x00, 0x01, 0xff, 0xfe]);
    /// assert!(import.conversions.is_empty());
    /// # Ok::<(), pdfcer_core::sound::WavError>(())
    /// ```
    pub fn from_wav(bytes: &[u8], options: &WavImportOptions) -> Result<WavImport, WavError> {
        let (fmt, data) = parse_riff(bytes)?;
        let unsupported = WavError::UnsupportedFormat {
            tag: fmt.tag,
            bits: fmt.bits,
        };
        let mut conversions = Vec::new();

        if matches!(fmt.tag, TAG_MULAW | TAG_ALAW) {
            if fmt.bits != 8 {
                return Err(unsupported);
            }
            if fmt.channels > 2 {
                return Err(WavError::TooManyChannels {
                    channels: fmt.channels,
                });
            }
            let encoding = if fmt.tag == TAG_MULAW {
                SoundEncoding::MuLaw
            } else {
                SoundEncoding::ALaw
            };
            if encoding == SoundEncoding::MuLaw
                && options.rate_policy == SoundRatePolicy::SpecRate
                && (fmt.rate != 8000 || fmt.channels != 1)
            {
                return Err(WavError::MuLawNotConformant {
                    rate: fmt.rate,
                    channels: fmt.channels,
                });
            }
            let frame = usize::from(fmt.channels);
            let whole = data.len() - data.len() % frame;
            return Ok(WavImport {
                sound: SoundData {
                    rate: fmt.rate,
                    channels: u8::try_from(fmt.channels).unwrap_or(2),
                    bits: 8,
                    encoding,
                    samples: data.get(..whole).unwrap_or(&[]).to_vec(),
                },
                conversions,
            });
        }

        // Linear PCM (integer or float) from here on.
        let (float, in_bytes) = match (fmt.tag, fmt.bits) {
            (TAG_PCM, 8 | 16 | 24 | 32) => (false, usize::from(fmt.bits / 8)),
            (TAG_FLOAT, 32 | 64) => (true, usize::from(fmt.bits / 8)),
            _ => return Err(unsupported),
        };
        if fmt.channels > 2 && !options.downmix {
            return Err(WavError::TooManyChannels {
                channels: fmt.channels,
            });
        }
        let out_bits: u8 = if float {
            conversions.push(SoundConversion::FloatToSigned16 {
                from_bits: fmt.bits,
            });
            16
        } else {
            u8::try_from(fmt.bits).map_err(|_| unsupported.clone())?
        };
        let target_rate = match options.rate_policy {
            SoundRatePolicy::KeepNative => fmt.rate,
            SoundRatePolicy::SpecRate if fmt.rate <= 16_537 => 11_025,
            SoundRatePolicy::SpecRate => 22_050,
        };
        let out_channels: u16 = if fmt.channels > 2 { 1 } else { fmt.channels };
        let frame_in = in_bytes * usize::from(fmt.channels);
        let frames = data.len() / frame_in;

        // Fast path: nothing but byte order changes.
        if !float && out_channels == fmt.channels && target_rate == fmt.rate {
            let whole = frames * frame_in;
            let mut samples = data.get(..whole).unwrap_or(&[]).to_vec();
            if in_bytes > 1 {
                for s in samples.chunks_exact_mut(in_bytes) {
                    s.reverse();
                }
            }
            return Ok(WavImport {
                sound: SoundData {
                    rate: fmt.rate,
                    channels: u8::try_from(fmt.channels).unwrap_or(2),
                    bits: out_bits,
                    encoding: pcm_encoding(out_bits),
                    samples,
                },
                conversions,
            });
        }

        let out_frames = if target_rate == fmt.rate {
            frames
        } else {
            conversions.push(SoundConversion::Resampled {
                from: fmt.rate,
                to: target_rate,
            });
            let n = (frames as u128 * u128::from(target_rate)) / u128::from(fmt.rate);
            usize::try_from(n).map_err(|_| WavError::TooLarge)?
        };
        if out_channels != fmt.channels {
            conversions.push(SoundConversion::Downmixed {
                from_channels: fmt.channels,
            });
        }
        let out_len = out_frames
            .checked_mul(usize::from(out_channels))
            .and_then(|n| n.checked_mul(usize::from(out_bits / 8)))
            .ok_or(WavError::TooLarge)?;
        if out_len > MAX_SOUND_BYTES {
            return Err(WavError::TooLarge);
        }

        // One normalised value in -1.0..=1.0 per (frame, output channel).
        let read = |frame: usize, ch: usize| -> f64 {
            let at = frame * frame_in + ch * in_bytes;
            let s = data.get(at..at + in_bytes).unwrap_or(&[]);
            decode_sample(s, float)
        };
        let value = |frame: usize, ch: usize| -> f64 {
            if out_channels == fmt.channels {
                read(frame, ch)
            } else {
                let n = usize::from(fmt.channels);
                (0..n).map(|c| read(frame, c)).sum::<f64>() / n as f64
            }
        };
        let mut samples = Vec::with_capacity(out_len);
        let step = f64::from(fmt.rate) / f64::from(target_rate);
        for i in 0..out_frames {
            let pos = i as f64 * step;
            let f0 = (pos.floor() as usize).min(frames.saturating_sub(1));
            let f1 = (f0 + 1).min(frames.saturating_sub(1));
            let t = pos - f0 as f64;
            for ch in 0..usize::from(out_channels) {
                let v = value(f0, ch) * (1.0 - t) + value(f1, ch) * t;
                encode_sample(v, out_bits, &mut samples);
            }
        }
        Ok(WavImport {
            sound: SoundData {
                rate: target_rate,
                channels: u8::try_from(out_channels).unwrap_or(1),
                bits: out_bits,
                encoding: pcm_encoding(out_bits),
                samples,
            },
            conversions,
        })
    }

    /// The sound stream dictionary's Table 294 entries (`/Type /R /C /B /E`).
    /// The caller adds `/Filter` and `/Length`.
    pub(crate) fn stream_dict(&self) -> Dict {
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Sound")));
        d.insert(Name::from(b"R"), Object::Integer(i64::from(self.rate)));
        d.insert(Name::from(b"C"), Object::Integer(i64::from(self.channels)));
        d.insert(Name::from(b"B"), Object::Integer(i64::from(self.bits)));
        d.insert(
            Name::from(b"E"),
            Object::Name(Name(self.encoding.name().to_vec())),
        );
        d
    }
}

fn pcm_encoding(bits: u8) -> SoundEncoding {
    if bits == 8 {
        SoundEncoding::Raw
    } else {
        SoundEncoding::Signed
    }
}

/// A little-endian WAV sample as a value in -1.0..=1.0.
fn decode_sample(s: &[u8], float: bool) -> f64 {
    let v = match (s, float) {
        (&[a], false) => (f64::from(a) - 128.0) / 128.0,
        (&[a, b], false) => f64::from(i16::from_le_bytes([a, b])) / 32_768.0,
        (&[a, b, c], false) => f64::from(i32::from_le_bytes([0, a, b, c]) >> 8) / 8_388_608.0,
        (&[a, b, c, d], false) => f64::from(i32::from_le_bytes([a, b, c, d])) / 2_147_483_648.0,
        (&[a, b, c, d], true) => f64::from(f32::from_le_bytes([a, b, c, d])),
        (&[a, b, c, d, e, f, g, h], true) => f64::from_le_bytes([a, b, c, d, e, f, g, h]),
        _ => 0.0,
    };
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Append `v` (-1.0..=1.0) as one big-endian sample of `bits`.
#[allow(clippy::cast_possible_truncation)] // clamped into range first
fn encode_sample(v: f64, bits: u8, out: &mut Vec<u8>) {
    match bits {
        8 => out.push((v * 127.0 + 128.0).round().clamp(0.0, 255.0) as u8),
        16 => out.extend_from_slice(
            &((v * 32_767.0).round().clamp(-32_768.0, 32_767.0) as i16).to_be_bytes(),
        ),
        24 => {
            let n = (v * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
            out.extend_from_slice(&n.to_be_bytes()[1..]);
        }
        _ => out.extend_from_slice(
            &((v * 2_147_483_647.0)
                .round()
                .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32)
                .to_be_bytes(),
        ),
    }
}
