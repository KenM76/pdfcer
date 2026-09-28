//! `SoundData::from_wav` (§13.3) and `add_sound_annotation` (`Pass 261.2`,
//! §12.5.6.16): every WAV branch, then the annotation read back from the
//! saved bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::{SoundIcon, SoundSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::object::{Name, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::sound::{
    SoundConversion, SoundData, SoundEncoding, SoundRatePolicy, WavError, WavImportOptions,
};
use pdfcer_core::writer::SaveOptions;

/// A WAV file: `fmt ` (16-byte, or 40-byte extensible when `sub` is set),
/// an optional odd-sized `LIST` chunk before `data`, then `data`.
fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8], odd_chunk: bool) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&tag.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    let align = channels * bits.div_ceil(8);
    fmt.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    fmt.extend_from_slice(&align.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    let mut body = b"WAVE".to_vec();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&u32::try_from(fmt.len()).unwrap().to_le_bytes());
    body.extend_from_slice(&fmt);
    if odd_chunk {
        body.extend_from_slice(b"LIST\x03\0\0\0abc\0");
    }
    body.extend_from_slice(b"data");
    body.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    body.extend_from_slice(data);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn extensible(sub_tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    let mut w = wav(0xFFFE, channels, rate, bits, data, false);
    // Grow `fmt ` from 16 to 40 bytes: cbSize, valid bits, mask, GUID.
    let mut ext = Vec::new();
    ext.extend_from_slice(&22u16.to_le_bytes());
    ext.extend_from_slice(&bits.to_le_bytes());
    ext.extend_from_slice(&0u32.to_le_bytes());
    ext.extend_from_slice(&sub_tag.to_le_bytes());
    ext.extend_from_slice(&[0; 14]);
    let fmt_end = 12 + 8 + 16;
    w.splice(fmt_end..fmt_end, ext);
    w[16..20].copy_from_slice(&40u32.to_le_bytes());
    let riff = u32::try_from(w.len() - 8).unwrap();
    w[4..8].copy_from_slice(&riff.to_le_bytes());
    w
}

fn import(bytes: &[u8]) -> pdfcer_core::sound::WavImport {
    SoundData::from_wav(bytes, &WavImportOptions::default()).unwrap()
}

#[test]
fn sixteen_bit_stereo_is_byte_swapped_and_nothing_else() {
    let data = [0x01, 0x00, 0x02, 0x00, 0xff, 0x7f, 0x00, 0x80];
    let got = import(&wav(1, 2, 44_100, 16, &data, true));
    assert!(got.conversions.is_empty());
    let s = &got.sound;
    assert_eq!(
        (s.rate, s.channels, s.bits, s.encoding),
        (44_100, 2, 16, SoundEncoding::Signed)
    );
    assert_eq!(s.samples, [0x00, 0x01, 0x00, 0x02, 0x7f, 0xff, 0x80, 0x00]);
}

#[test]
fn eight_bit_is_raw_and_a_partial_frame_is_dropped() {
    let got = import(&wav(1, 2, 11_025, 8, &[10, 20, 30], false));
    assert_eq!(got.sound.encoding, SoundEncoding::Raw);
    assert_eq!(got.sound.samples, [10, 20]);
}

#[test]
fn twenty_four_and_thirty_two_bit_are_reversed_per_sample() {
    let got = import(&wav(1, 1, 8_000, 24, &[1, 2, 3, 4, 5, 6], false));
    assert_eq!(got.sound.bits, 24);
    assert_eq!(got.sound.samples, [3, 2, 1, 6, 5, 4]);
    let got = import(&extensible(1, 1, 8_000, 32, &[1, 2, 3, 4]));
    assert_eq!(got.sound.bits, 32);
    assert_eq!(got.sound.samples, [4, 3, 2, 1]);
}

#[test]
fn float_becomes_signed_sixteen_and_says_so() {
    let mut data = Vec::new();
    for v in [0.5f32, -1.0, 2.0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    let got = import(&wav(3, 1, 22_050, 32, &data, false));
    assert_eq!(
        got.conversions,
        [SoundConversion::FloatToSigned16 { from_bits: 32 }]
    );
    assert_eq!(
        (got.sound.bits, got.sound.encoding),
        (16, SoundEncoding::Signed)
    );
    let v: Vec<i16> = got
        .sound
        .samples
        .chunks(2)
        .map(|c| i16::from_be_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(v, [16_384, -32_767, 32_767], "out-of-range floats clamp");
}

#[test]
fn companded_sound_is_copied_and_the_spec_rate_policy_polices_mu_law() {
    let got = import(&wav(7, 1, 8_000, 8, &[0x12, 0x34], false));
    assert_eq!(got.sound.encoding, SoundEncoding::MuLaw);
    assert_eq!(got.sound.samples, [0x12, 0x34]);
    let got = import(&extensible(6, 2, 8_000, 8, &[1, 2, 3, 4]));
    assert_eq!(
        (got.sound.encoding, got.sound.channels),
        (SoundEncoding::ALaw, 2)
    );

    let strict = WavImportOptions {
        rate_policy: SoundRatePolicy::SpecRate,
        ..Default::default()
    };
    let err = SoundData::from_wav(&wav(7, 1, 16_000, 8, &[0; 4], false), &strict).unwrap_err();
    assert_eq!(
        err,
        WavError::MuLawNotConformant {
            rate: 16_000,
            channels: 1
        }
    );
}

#[test]
fn the_spec_rate_policy_resamples_and_discloses() {
    let data: Vec<u8> = (0..100i16).flat_map(|v| (v * 100).to_le_bytes()).collect();
    let options = WavImportOptions {
        rate_policy: SoundRatePolicy::SpecRate,
        ..Default::default()
    };
    let got = SoundData::from_wav(&wav(1, 1, 44_100, 16, &data, false), &options).unwrap();
    assert_eq!(
        got.conversions,
        [SoundConversion::Resampled {
            from: 44_100,
            to: 22_050
        }]
    );
    assert_eq!(got.sound.rate, 22_050);
    assert_eq!(got.sound.samples.len(), 100, "50 frames of 2 bytes");
    // Already at a spec rate: untouched.
    let got = SoundData::from_wav(&wav(1, 1, 11_025, 16, &data, false), &options).unwrap();
    assert!(got.conversions.is_empty());
}

#[test]
fn more_than_two_channels_is_refused_unless_downmixed() {
    let data = [100u8, 200, 150, 50, 60, 70];
    let bytes = wav(1, 3, 8_000, 8, &data, false);
    assert_eq!(
        SoundData::from_wav(&bytes, &WavImportOptions::default()).unwrap_err(),
        WavError::TooManyChannels { channels: 3 }
    );
    let options = WavImportOptions {
        downmix: true,
        ..Default::default()
    };
    let got = SoundData::from_wav(&bytes, &options).unwrap();
    assert_eq!(
        got.conversions,
        [SoundConversion::Downmixed { from_channels: 3 }]
    );
    assert_eq!(got.sound.channels, 1);
    assert_eq!(got.sound.samples.len(), 2);
    assert_eq!(got.sound.samples[0], 150, "mean of 100, 200, 150");
}

#[test]
fn malformed_input_is_refused_by_name() {
    let none = WavImportOptions::default();
    assert_eq!(
        SoundData::from_wav(b"not a wav", &none).unwrap_err(),
        WavError::NotWav
    );
    let mut no_data = wav(1, 1, 8_000, 8, &[], false);
    no_data.truncate(no_data.len() - 8);
    assert_eq!(
        SoundData::from_wav(&no_data, &none).unwrap_err(),
        WavError::MissingData
    );
    assert_eq!(
        SoundData::from_wav(&wav(2, 1, 8_000, 4, &[0], false), &none).unwrap_err(),
        WavError::UnsupportedFormat { tag: 2, bits: 4 }
    );
    assert!(matches!(
        SoundData::from_wav(&wav(1, 0, 8_000, 8, &[0], false), &none).unwrap_err(),
        WavError::Malformed(_)
    ));
}

fn one_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

const RECT: Rect = Rect {
    llx: 72.0,
    lly: 300.0,
    urx: 92.0,
    ury: 320.0,
};

fn int(d: &pdfcer_core::object::Dict, key: &[u8]) -> Option<i64> {
    match d.get(key) {
        Some(Object::Integer(i)) => Some(*i),
        _ => None,
    }
}

#[test]
fn a_sound_annotation_embeds_its_samples_as_a_sound_object() {
    let sound = import(&wav(1, 2, 44_100, 16, &[1, 0, 2, 0, 3, 0, 4, 0], false)).sound;
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let mut spec = SoundSpec::new(RECT, sound.clone());
    spec.icon = SoundIcon::Mic;
    let options = MarkupOptions {
        note: Some(MarkupNote::new("Site walk narration").by("Ken")),
        ..Default::default()
    };
    let id = s.add_sound_annotation(0, &spec, &options).unwrap();
    let doc =
        Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap();

    let Object::Dict(annot) = &doc.get(id).unwrap().value else {
        panic!("annotation is not a dictionary")
    };
    assert_eq!(
        annot.get(b"Subtype").and_then(Object::as_name),
        Some(&Name::from(b"Sound"))
    );
    assert_eq!(
        annot.get(b"Name").and_then(Object::as_name),
        Some(&Name::from(b"Mic"))
    );
    assert!(
        matches!(annot.get(b"Contents"), Some(Object::String(t)) if t == b"Site walk narration")
    );
    assert!(matches!(annot.get(b"AP"), Some(Object::Dict(_))));
    let Some(Object::Reference(sound_id)) = annot.get(b"Sound") else {
        panic!("no indirect /Sound")
    };
    let Object::Stream(st) = &doc.get(*sound_id).unwrap().value else {
        panic!("/Sound is not a stream")
    };
    assert_eq!(
        st.dict.get(b"Type").and_then(Object::as_name),
        Some(&Name::from(b"Sound"))
    );
    assert_eq!(int(&st.dict, b"R"), Some(44_100));
    assert_eq!(int(&st.dict, b"C"), Some(2));
    assert_eq!(int(&st.dict, b"B"), Some(16));
    assert_eq!(
        st.dict.get(b"E").and_then(Object::as_name),
        Some(&Name::from(b"Signed"))
    );
    assert!(!st.dict.contains_key(b"CO") && !st.dict.contains_key(b"CP"));
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    let decoded = pdfcer_core::filters::flate::decode(raw, None).unwrap();
    assert_eq!(decoded, sound.samples);
}

#[test]
fn one_undo_removes_a_sound_annotation() {
    let sound = import(&wav(1, 1, 8_000, 8, &[128; 16], false)).sound;
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    s.add_sound_annotation(0, &SoundSpec::new(RECT, sound), &MarkupOptions::default())
        .unwrap();
    assert!(s.undo().is_some());
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        before
    );
}
