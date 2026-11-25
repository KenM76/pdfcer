//! `centre-line` and `centre-mark` — author centre geometry as ce dimensions.

use super::*;

use pdfcer_core::dimension::{CentreMarkSpec, DimensionKind, FitCircle, GroupId};
use pdfcer_core::vector::Point;

/// Whether `centre-mark` also authors the circle's measurement.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CentreMeasureArg {
    /// The mark alone.
    None,
    /// The mark plus a radius ce dimension, as one object.
    Radius,
    /// The mark plus a diameter ce dimension, as one object.
    Diameter,
}

impl CentreMeasureArg {
    const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Radius => "radius",
            Self::Diameter => "diameter",
        }
    }
}

/// Where the authored object goes and how it is saved.
pub(crate) struct CentreTarget<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) group: u32,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `centre-line` arguments.
pub(crate) struct CentreLineArgs<'a> {
    pub(crate) target: CentreTarget<'a>,
    pub(crate) points: &'a str,
    pub(crate) picks: Option<&'a str>,
    pub(crate) overshoot: f64,
    pub(crate) treat_as_parallel: bool,
}

/// `centre-mark` arguments.
pub(crate) struct CentreMarkArgs<'a> {
    pub(crate) target: CentreTarget<'a>,
    pub(crate) points: Option<&'a str>,
    pub(crate) circle: Option<&'a str>,
    pub(crate) measure: CentreMeasureArg,
    pub(crate) extension_lines: bool,
    pub(crate) angle: f64,
}

/// `centre-line`: the midline or bisector of two lines, each given by its two
/// ends. Prints the reading pdfcer made (rule 4) before saving.
pub(crate) fn cmd_centre_line(args: &CentreLineArgs<'_>) -> u8 {
    use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine, TwoLineRelation};
    let input = args.target.input;
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let Some([a1, a2, b1, b2]) =
        parse_dim_points(args.points).and_then(|v| <[Point; 4]>::try_from(v).ok())
    else {
        eprintln!(
            "pdfcer: {}: --points must be exactly four points, two per line: `x,y x,y  x,y x,y`",
            input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let picks = match args.picks.map(parse_dim_points) {
        None => [a1.midpoint(a2), b1.midpoint(b2)],
        Some(Some(v)) if v.len() == 2 => [v[0], v[1]],
        Some(_) => {
            eprintln!(
                "pdfcer: {}: --picks must be exactly two points, one on each line: `x,y x,y`",
                input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };
    let mk = |start: Point, end: Point, pick: Point| PickedLine {
        target: pdfcer_core::vector::HitTarget::Object(0),
        subpath: 0,
        segment: 0,
        start,
        end,
        pick,
    };
    let mut policy = ParallelPolicy::from_setting(settings.parallel_epsilon_degrees);
    if args.treat_as_parallel {
        policy = policy.forcing_parallel();
    }
    let authored = match pdfcer_core::dimension::author_centre_line(
        &mk(a1, a2, picks[0]),
        &mk(b1, b2, picks[1]),
        policy,
        args.overshoot,
    ) {
        Ok(authored) => authored,
        Err(refusal) => {
            eprintln!(
                "pdfcer: {}: {refusal}. Nothing was authored.",
                input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };
    let relation = match authored.relation {
        TwoLineRelation::Parallel { distance } => format!("parallel distance={distance:.4}"),
        TwoLineRelation::Angled { degrees, apex, .. } => {
            format!(
                "angled degrees={degrees:.3} apex={:.2},{:.2}",
                apex.x, apex.y
            )
        }
        TwoLineRelation::Collinear => "collinear".to_owned(),
    };
    let measured = authored
        .measured_angle_degrees
        .map_or_else(|| "-".to_owned(), |d| format!("{d:.3}"));
    println!(
        "  centre_line relation={relation} measured_angle={measured} epsilon={} forced={} overlap={:.4} overshoot={}",
        settings.parallel_epsilon_degrees,
        u32::from(authored.forced_parallel),
        authored.overlap,
        args.overshoot
    );
    author_and_report("centre-line", &args.target, authored.kind)
}

/// `centre-mark`: a centre cross on a circle or arc, from three or more rim
/// points or from an explicit centre and radius, optionally with its radius
/// or diameter measurement.
pub(crate) fn cmd_centre_mark(args: &CentreMarkArgs<'_>) -> u8 {
    let input = args.target.input;
    let fit = match (args.points, args.circle) {
        (Some(points), None) => {
            let fitted = parse_dim_points(points)
                .and_then(|p| pdfcer_core::dimension::fit_circle_taubin(&p));
            let Some(fit) = fitted else {
                eprintln!(
                    "pdfcer: {}: --points needs at least 3 non-collinear rim points to fit a circle",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            };
            println!(
                "  centre_mark fitted centre={:.4},{:.4} radius={:.4} residual={:.4}",
                fit.center.x, fit.center.y, fit.radius, fit.residual
            );
            fit
        }
        (None, Some(circle)) => match parse_circle(circle) {
            Some(fit) => fit,
            None => {
                eprintln!(
                    "pdfcer: {}: --circle must be `cx,cy,r` with a positive radius",
                    input.display()
                );
                return exit::EDIT_REFUSED;
            }
        },
        _ => {
            eprintln!(
                "pdfcer: {}: give exactly one of --points (rim points to fit) or --circle (cx,cy,r)",
                input.display()
            );
            return exit::EDIT_REFUSED;
        }
    };
    let mark = CentreMarkSpec {
        extension_lines: args.extension_lines,
        angle: args.angle,
    };
    let kind = match args.measure {
        CentreMeasureArg::None => DimensionKind::CentreMark { fit, mark },
        m => DimensionKind::Circular {
            fit,
            show_diameter: m == CentreMeasureArg::Diameter,
            show_circle: true,
            leader_angle: 0.0,
            text_distance: None,
            centre_mark: Some(mark),
        },
    };
    println!(
        "  centre_mark measure={} extension_lines={} angle={}",
        args.measure.token(),
        if args.extension_lines { "on" } else { "off" },
        args.angle
    );
    author_and_report("centre-mark", &args.target, kind)
}

/// `cx,cy,r` with a positive finite radius, as a zero-residual fit.
fn parse_circle(spec: &str) -> Option<FitCircle> {
    let v: Vec<f64> = spec
        .split(',')
        .map(|t| t.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [x, y, r] = v.as_slice() else {
        return None;
    };
    (x.is_finite() && y.is_finite() && r.is_finite() && *r > 0.0).then(|| FitCircle {
        center: Point::new(*x, *y),
        radius: *r,
        residual: 0.0,
    })
}

/// Add `kind` to the target page and group, save, and print the save line.
fn author_and_report(verb: &str, t: &CentreTarget<'_>, kind: DimensionKind) -> u8 {
    let Some(page_index) = t.page.checked_sub(1).map(|i| i as usize) else {
        eprintln!(
            "pdfcer: {}: --page is 1-based; 0 is not a page",
            t.input.display()
        );
        return exit::EDIT_REFUSED;
    };
    let (source, mut session) = match open_for_edit(t.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let (annot_id, dim_id) = match session.add_dimension(page_index, GroupId(t.group), kind) {
        Ok(v) => v,
        Err(err) => return report_edit_error(t.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        t.output,
        t.mode,
        ProducerArg::Preserve,
        t.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "{verb} {} page {} group={} mode={} -> {}; annot={annot_id} dim={} changed={} objects={} verbatim={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        t.input.display(),
        t.page,
        t.group,
        t.mode.name(),
        t.output.display(),
        dim_id.0,
        outcome.changed,
        r.objects_written,
        r.objects_verbatim,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(t.input, &outcome)
}
