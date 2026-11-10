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
        printer_colour: Some(IccPrint::Printer(
            Box::new(colour),
            PrinterIntentArg::Relative,
        )),
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
        printer_colour: Some(IccPrint::Printer(
            Box::new(colour),
            PrinterIntentArg::Perceptual,
        )),
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
    assert!(err.ends_with("with --proof-profile"), "{err}");
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
        r#"icc-profile name="Glossy" space=rgb class=prtr printable=1 proof=1 path="a/b.icc""#
    );
    let p = InstalledProfile {
        color_space: ProfileSpace::Other(*b"Lab "),
        class: *b"abst",
        ..p
    };
    assert!(profile_line(&p).contains("space=lab class=abst printable=0 proof=0"));
    let p = InstalledProfile {
        color_space: ProfileSpace::Cmyk,
        class: *b"prtr",
        ..p
    };
    assert!(profile_line(&p).contains("printable=0 proof=1"));
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

fn proof_args(printer: Option<&'static str>) -> IccArgs<'static> {
    IccArgs {
        proof_profile: Some(CMYK),
        printer_profile: printer,
        ..IccArgs::default()
    }
}

#[test]
fn a_cmyk_proof_converts_the_sheets_and_says_what_it_simulated() {
    let icc = IccPrint::from_args(&proof_args(None)).unwrap().unwrap();
    let out = PrintOutput {
        printer_colour: Some(icc),
        ..PrintOutput::default()
    };
    let mut sheets = [sheet(vec![0, 255, 0, 255])];
    let notes = out.recolor(&mut sheets).unwrap();
    assert_ne!(&sheets[0].rgba[..3], &[0, 255, 0], "the proof did nothing");
    assert_eq!(notes.len(), 1);
    let n = &notes[0];
    assert!(n.contains("proof of") && n.contains("(cmyk)"), "{n}");
    assert!(n.contains("printed as sRGB"), "{n}");
    assert!(n.contains("map to this printer's"), "{n}");
    // dest-cmyk.icc's black point cannot be estimated.
    assert!(n.contains("cannot be estimated"), "{n}");
}

#[test]
fn a_proof_goes_through_the_printer_profile_and_can_simulate_paper() {
    let mut args = proof_args(Some(SWAP_RG));
    args.simulate_paper = true;
    let Some(IccPrint::Proof(colour, _)) = IccPrint::from_args(&args).unwrap() else {
        panic!("not a proof");
    };
    assert!(colour.simulate_paper);
    let line = proof_disclosure(&colour, PrinterIntentArg::Relative);
    assert!(line.contains("printed through printer profile"), "{line}");
    assert!(
        line.contains("paper tint and black are simulated"),
        "{line}"
    );
}

#[test]
fn proof_flags_reach_the_proof() {
    let mut args = proof_args(None);
    args.proof_intent = PrinterIntentArg::Perceptual;
    args.no_proof_bpc = true;
    let Some(IccPrint::Proof(colour, intent)) = IccPrint::from_args(&args).unwrap() else {
        panic!("not a proof");
    };
    assert_eq!(colour.intent, RenderingIntent::Perceptual);
    assert_eq!(intent, PrinterIntentArg::Perceptual);
    assert!(!colour.black_point_compensation);
    assert!(!colour.simulate_paper);
    assert!(colour.printer.is_none());
}
