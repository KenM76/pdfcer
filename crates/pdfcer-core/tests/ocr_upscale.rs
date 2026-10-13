//! `ocr::upscale`: PIL-exact resampling, the refusals, and mapping words
//! found on the enlarged image back.

use pdfcer_core::ocr::RecognizedWord;
use pdfcer_core::ocr::upscale::{MAX_FACTOR, Upscale, UpscaleError, UpscaleFilter, upscale_grey};
use pdfcer_core::page_tree::Rect;

const PX: [u8; 15] = [
    0, 50, 100, 200, 255, 10, 20, 30, 40, 50, 255, 0, 255, 0, 128,
];

fn up(factor: f32, filter: UpscaleFilter) -> Upscale {
    Upscale::new(factor, filter).unwrap()
}

#[test]
fn lanczos_matches_pil() {
    // Pillow `Image.resize((10, 6), Image.LANCZOS)` on the 5x3 mode-"L" image.
    let out = upscale_grey(5, 3, &PX, up(2.0, UpscaleFilter::Lanczos)).unwrap();
    assert_eq!((out.width, out.height), (10, 6));
    assert_eq!(
        out.pixels,
        [
            8, 13, 38, 68, 98, 139, 191, 244, 255, 255, 0, 0, 31, 54, 62, 92, 147, 191, 210, 211,
            0, 0, 20, 31, 18, 30, 70, 96, 97, 91, 75, 59, 19, 22, 70, 72, 24, 9, 33, 52, 210, 158,
            29, 32, 178, 186, 44, 0, 68, 122, 255, 214, 36, 40, 243, 255, 64, 2, 101, 175
        ]
    );
}

#[test]
fn bicubic_matches_pil() {
    // Pillow `Image.resize((10, 6), Image.BICUBIC)`.
    let out = upscale_grey(5, 3, &PX, up(2.0, UpscaleFilter::Bicubic)).unwrap();
    assert_eq!(
        out.pixels,
        [
            0, 8, 37, 64, 89, 131, 189, 235, 255, 255, 0, 5, 31, 53, 69, 102, 150, 186, 208, 214,
            0, 0, 19, 30, 28, 40, 68, 84, 89, 90, 64, 54, 21, 24, 66, 68, 31, 24, 45, 56, 209, 165,
            34, 36, 178, 181, 44, 13, 84, 118, 255, 218, 41, 42, 232, 234, 50, 8, 102, 147
        ]
    );
}

#[test]
fn flat_stays_flat_and_identity_is_a_copy() {
    for filter in [UpscaleFilter::Bicubic, UpscaleFilter::Lanczos] {
        let out = upscale_grey(3, 3, &[77; 9], up(3.0, filter)).unwrap();
        assert!(out.pixels.iter().all(|&p| p == 77), "{filter}");
        let same = upscale_grey(5, 3, &PX, up(1.0, filter)).unwrap();
        assert_eq!(same.pixels, PX);
        assert!((same.scale_x - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn sizes_round_per_axis_and_ratios_are_exact() {
    let out = upscale_grey(3, 7, &[0; 21], up(1.5, UpscaleFilter::Bicubic)).unwrap();
    // 4.5 rounds away from zero; 10.5 likewise.
    assert_eq!((out.width, out.height), (5, 11));
    assert!((out.scale_x - 5.0 / 3.0).abs() < 1e-12);
    assert!((out.scale_y - 11.0 / 7.0).abs() < 1e-12);
}

#[test]
fn refusals_are_named() {
    for f in [0.5, 0.999, MAX_FACTOR + 0.01, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            Upscale::new(f, UpscaleFilter::Bicubic),
            Err(UpscaleError::Factor(_))
        ));
    }
    assert!(matches!(
        upscale_grey(4, 4, &[0; 15], up(2.0, UpscaleFilter::Bicubic)),
        Err(UpscaleError::Buffer { got: 15, .. })
    ));
    assert!(matches!(
        upscale_grey(0, 4, &[], up(2.0, UpscaleFilter::Bicubic)),
        Err(UpscaleError::Buffer { .. })
    ));
    // 20000 x 2000 at x4 is 640 Mpx: refused before any allocation.
    let big = vec![0u8; 20_000 * 2_000];
    assert_eq!(
        upscale_grey(20_000, 2_000, &big, up(4.0, UpscaleFilter::Bicubic)),
        Err(UpscaleError::TooLarge {
            width: 80_000,
            height: 8_000
        })
    );
    assert_eq!(
        "Lanczos".parse::<UpscaleFilter>(),
        Ok(UpscaleFilter::Lanczos)
    );
    assert!(matches!(
        "nearest".parse::<UpscaleFilter>(),
        Err(UpscaleError::UnknownFilter(_))
    ));
}

#[test]
fn words_map_back_by_the_actual_ratio() {
    let out = upscale_grey(3, 7, &[0; 21], up(1.5, UpscaleFilter::Lanczos)).unwrap();
    let mut words = vec![RecognizedWord {
        text: "x".into(),
        rect: Rect {
            llx: 5.0,
            lly: 11.0,
            urx: 10.0,
            ury: 22.0,
        },
        confidence: None,
    }];
    out.map_back(&mut words);
    let r = words[0].rect;
    assert!((r.llx - 3.0).abs() < 1e-9 && (r.urx - 6.0).abs() < 1e-9);
    assert!((r.lly - 7.0).abs() < 1e-9 && (r.ury - 14.0).abs() < 1e-9);
}
