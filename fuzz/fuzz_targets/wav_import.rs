//! Fuzz target: WAV import (`pdfcer_core::sound::SoundData::from_wav`).
//!
//! The WAV is an operator-supplied file of unknown origin. Invariant: under
//! every option combination `from_wav` returns `Ok` or `WavError` without
//! panicking, and an `Ok` sound is mono or stereo, holds whole frames, and
//! stays under `MAX_SOUND_BYTES`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::sound::{MAX_SOUND_BYTES, SoundData, SoundRatePolicy, WavImportOptions};

fuzz_target!(|data: &[u8]| {
    for rate_policy in [SoundRatePolicy::KeepNative, SoundRatePolicy::SpecRate] {
        for downmix in [false, true] {
            let options = WavImportOptions {
                rate_policy,
                downmix,
            };
            if let Ok(got) = SoundData::from_wav(data, &options) {
                let s = got.sound;
                assert!(matches!(s.channels, 1 | 2));
                let frame = usize::from(s.channels) * usize::from(s.bits / 8);
                assert_eq!(s.samples.len() % frame, 0);
                assert!(s.samples.len() <= MAX_SOUND_BYTES);
            }
        }
    }
});
