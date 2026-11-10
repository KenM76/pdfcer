#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::print_output::PrintOutput;

const SWAP_RG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/icc-rgb/swap-rg.icc"
);
const CMYK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/icc-rgb/dest-cmyk.icc"
);

fn sheet(rgba: Vec<u8>) -> pdfcer_print::PageBitmap {
    pdfcer_print::PageBitmap {
        width: u32::try_from(rgba.len() / 4).unwrap(),
        height: 1,
        rgba,
        placement: pdfcer_print::Placement {
            scale: 1.0,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            clipped: false,
        },
        page_pt: (1.0, 1.0),
    }
}

#[test]
fn a_profile_file_converts_the_printed_sheets_after_color() {
    let colour = resolve(SWAP_RG, PrinterIntentArg::Relative, false).unwrap();
    let out = PrintOutput {
        printer_colour: Some((colour, PrinterIntentArg::Relative)),
        ..PrintOutput::default()
    };
    let mut sheets = [sheet(vec![255, 0, 0, 255])];
    let notes = out.recolor(&mut sheets).unwrap();
    let px = &sheets[0].rgba;
    assert!(px[0] < 40 && px[1] > 215 && px[2] < 40, "{px:?}");
    assert_eq!(px[3], 255);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].contains("relative colorimetric intent"),
        "{notes:?}"
    );
    assert!(
        notes[0].contains("black point compensation on"),
        "{notes:?}"
    );
}

#[test]
fn grayscale_runs_before_the_printer_profile() {
    let colour = resolve(SWAP_RG, PrinterIntentArg::Perceptual, true).unwrap();
    let out = PrintOutput {
        color: crate::print_output::ColorArg::Grayscale,
        printer_colour: Some((colour, PrinterIntentArg::Perceptual)),
        ..PrintOutput::default()
    };
    let mut sheets = [sheet(vec![255, 0, 0, 255])];
    let notes = out.recolor(&mut sheets).unwrap();
    let px = &sheets[0].rgba;
    // Grey in, so swapping red and green leaves it grey.
    assert!(px[0].abs_diff(px[1]) <= 2, "{px:?}");
    assert_eq!(notes.len(), 2);
    assert!(
        notes[1].contains("black point compensation off"),
        "{notes:?}"
    );
}

#[test]
fn absolute_intent_says_bpc_does_not_apply() {
    let colour = resolve(SWAP_RG, PrinterIntentArg::Absolute, false).unwrap();
    let line = disclosure(&colour, PrinterIntentArg::Absolute);
    assert!(line.contains("absolute colorimetric intent"), "{line}");
    assert!(line.contains("does not apply"), "{line}");
}

#[test]
fn a_cmyk_profile_and_an_unknown_name_are_refused_by_name() {
    let err = resolve(CMYK, PrinterIntentArg::Relative, false).unwrap_err();
    assert!(err.starts_with("--printer-profile "), "{err}");
    assert!(err.contains("CMYK"), "{err}");
    let err = resolve(
        "no such profile pdfcer-x063",
        PrinterIntentArg::Relative,
        false,
    )
    .unwrap_err();
    assert!(err.contains("pdfcer list-icc-profiles"), "{err}");
}

#[test]
fn a_listing_line_names_space_class_and_printability() {
    let p = InstalledProfile {
        path: std::path::PathBuf::from("a/b.icc"),
        name: "Glossy".to_owned(),
        color_space: ProfileSpace::Rgb,
        class: *b"prtr",
    };
    assert_eq!(
        profile_line(&p),
        r#"icc-profile name="Glossy" space=rgb class=prtr printable=1 path="a/b.icc""#
    );
    let p = InstalledProfile {
        color_space: ProfileSpace::Other(*b"Lab "),
        class: *b"abst",
        ..p
    };
    assert!(profile_line(&p).contains("space=lab class=abst printable=0"));
}

#[test]
fn each_intent_flag_asks_for_its_own_intent() {
    for (arg, intent) in [
        (PrinterIntentArg::Perceptual, RenderingIntent::Perceptual),
        (
            PrinterIntentArg::Relative,
            RenderingIntent::RelativeColorimetric,
        ),
        (PrinterIntentArg::Saturation, RenderingIntent::Saturation),
        (
            PrinterIntentArg::Absolute,
            RenderingIntent::AbsoluteColorimetric,
        ),
    ] {
        assert_eq!(resolve(SWAP_RG, arg, false).unwrap().intent, intent);
    }
}
