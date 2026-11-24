//! `convert-colours`: move every colour drawn in RGB, Lab, calibrated or
//! grey spaces to one destination profile (`EditSession::convert_colours`,
//! converted by icccer through pdfcer-render's `IcccerConverter`).

use super::*;

use pdfcer_core::color::RenderingIntent;
use pdfcer_core::prepress::{
    ColourConversion, ColourDestination, ConvertColoursReport, ConvertScope, IntentChoice,
    OutputIntentChange,
};

/// `--intent` on `convert-colours`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ConvertIntentArg {
    /// Compress the whole gamut to fit. For photographs.
    Perceptual,
    /// Match in-gamut colours exactly, relative to the paper white.
    #[default]
    Relative,
    /// Keep colours vivid rather than accurate. For charts.
    Saturation,
    /// Match colours exactly, including the paper tint.
    Absolute,
    /// Each object's own intent: ri, an ExtGState RI or an image's Intent.
    Own,
}

impl ConvertIntentArg {
    const fn choice(self) -> IntentChoice {
        match self {
            Self::Perceptual => IntentChoice::Fixed(RenderingIntent::Perceptual),
            Self::Relative => IntentChoice::Fixed(RenderingIntent::RelativeColorimetric),
            Self::Saturation => IntentChoice::Fixed(RenderingIntent::Saturation),
            Self::Absolute => IntentChoice::Fixed(RenderingIntent::AbsoluteColorimetric),
            Self::Own => IntentChoice::ObjectsOwn,
        }
    }
}

/// A source family on `--from`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ConvertSourceArg {
    /// DeviceRGB, read as sRGB.
    Rgb,
    /// ICCBased RGB.
    IccRgb,
    /// Lab.
    Lab,
    /// CalRGB.
    CalRgb,
    /// DeviceGray, ICCBased grey and CalGray.
    Grey,
}

/// What `convert-colours` converts and how.
pub(crate) struct ConvertArgs<'a> {
    /// The destination ICC profile; `None` takes the output intent's.
    pub profile: Option<&'a Path>,
    /// Source families; empty keeps the defaults (RGB and Lab).
    pub from: &'a [ConvertSourceArg],
    /// The rendering intent.
    pub intent: ConvertIntentArg,
    /// Convert pure black to the destination's black too.
    pub convert_black: bool,
    /// Turn black point compensation off.
    pub no_bpc: bool,
    /// Embed the destination as an output intent of this subtype.
    pub embed_intent: Option<&'a str>,
    /// `all` or a 1-based page list.
    pub pages: &'a str,
}

fn conversion(args: &ConvertArgs<'_>) -> Result<ColourConversion, u8> {
    let destination = match args.profile {
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => ColourDestination::Profile(bytes),
            Err(err) => {
                eprintln!("pdfcer: {}: --profile: {err}", path.display());
                return Err(exit::EDIT_REFUSED);
            }
        },
        None => ColourDestination::OutputIntent,
    };
    let mut c = ColourConversion::new(destination);
    if !args.from.is_empty() {
        let has = |s| args.from.contains(&s);
        c.device_rgb = has(ConvertSourceArg::Rgb);
        c.icc_rgb = has(ConvertSourceArg::IccRgb);
        c.lab = has(ConvertSourceArg::Lab);
        c.cal_rgb = has(ConvertSourceArg::CalRgb);
        c.grey = has(ConvertSourceArg::Grey);
    }
    c.intent = args.intent.choice();
    c.keep_pure_black = !args.convert_black;
    c.black_point_compensation = !args.no_bpc;
    c.embed_output_intent = args.embed_intent.map(str::to_owned);
    Ok(c)
}

/// `convert-colours` — one undo step.
///
/// ## Contract
///
/// - One `source=<space> paints=<n> images=<n> lookups=<n> shadings=<n>
///   out_of_gamut=<n>` line per source space converted, one
///   `image object=<id> source=<space> bits=<n> filter=<f|-> bytes=<a>-><b>`
///   line per re-encoded image, one `unconverted object=<id|-> "<why>"`
///   line per item left, an `output-intent added|replaced …` line when
///   one was embedded, `note: …` lines, then the save line and
///   [`finish_edit`]'s exit.
/// - No usable destination, no output intent to take one from, no source
///   selected or a page past the end is refused before anything is written
///   (exit `EDIT_REFUSED`).
pub(crate) fn cmd_convert_colours(
    input: &Path,
    args: &ConvertArgs<'_>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let scope = if args.pages.trim().eq_ignore_ascii_case("all") {
        ConvertScope::All
    } else {
        let count = match session.pages() {
            Ok(p) => p.len(),
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::EDIT_REFUSED;
            }
        };
        match parse_pages(args.pages, count) {
            Ok(p) => ConvertScope::Pages(p),
            Err(err) => {
                eprintln!("pdfcer: {}: --pages: {err}", input.display());
                return exit::EDIT_REFUSED;
            }
        }
    };
    let conversion = match conversion(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let converter = pdfcer_render::convert_colours::IcccerConverter::new();
    let report = match session.convert_colours(&scope, &conversion, &converter) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };
    print_report(&report);
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "convert-colours {} out_of_gamut={} images={} unconverted={} mode={} -> {}; changed={} \
objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        report.out_of_gamut,
        report.images.len(),
        report.unconverted.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

fn print_report(report: &ConvertColoursReport) {
    for c in &report.by_source {
        println!(
            "source=\"{}\" paints={} images={} lookups={} shadings={} out_of_gamut={}",
            c.source.name(),
            c.paints,
            c.images,
            c.lookups,
            c.shadings,
            c.out_of_gamut
        );
    }
    for i in &report.images {
        println!(
            "image object={} source=\"{}\" bits={} filter={} bytes={}->{}",
            i.object,
            i.source.name(),
            i.bits_before,
            i.filter_before.as_deref().unwrap_or("-"),
            i.bytes_before,
            i.bytes_after
        );
    }
    for u in &report.unconverted {
        let at = u.object.map_or_else(|| "-".to_owned(), |id| id.to_string());
        println!("unconverted object={at} \"{}\"", u.reason);
    }
    match &report.output_intent {
        OutputIntentChange::Added => println!("output-intent added"),
        OutputIntentChange::Replaced { previous } => {
            println!("output-intent replaced previous=\"{previous}\"");
        }
        _ => {}
    }
    for d in &report.disclosures {
        println!("note: {d}");
    }
}
