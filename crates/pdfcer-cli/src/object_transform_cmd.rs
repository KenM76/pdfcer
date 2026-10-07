//! `object-transform`: scale, rotate, mirror or move a selection by one
//! page-space matrix — page objects, or with `--leaf` objects inside one form
//! XObject.

use super::*;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{Matrix, MixedSelection, Point, SingularPolicy, TransformOptions};

/// Arguments for [`cmd_object_transform`], grouped so the handler stays under
/// the clippy `too_many_arguments` bound (the `EditTextArgs` pattern).
pub(crate) struct ObjectTransformArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// `--objects`, unparsed.
    pub(crate) objects: &'a str,
    /// `--objects` are form-leaf indices (`transform_objects_in_form`).
    pub(crate) leaf: bool,
    pub(crate) scale: Option<&'a str>,
    /// Degrees, counter-clockwise.
    pub(crate) rotate: Option<f64>,
    pub(crate) translate: Option<&'a str>,
    pub(crate) pivot: Option<&'a str>,
    pub(crate) on_mixed: &'a str,
    pub(crate) on_singular: &'a str,
    pub(crate) preview: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// The parsed, validated flags: everything but the pivot, which needs the
/// opened document.
struct TransformPlan {
    indices: Vec<usize>,
    options: TransformOptions,
    scale: Option<(f64, f64)>,
    translate: Option<(f64, f64)>,
    pivot: Option<(f64, f64)>,
}

impl TransformPlan {
    /// Scale, then rotate, both about `pivot`, then translate.
    fn matrix(&self, rotate: Option<f64>, pivot: Point) -> Matrix {
        let mut m = Matrix::IDENTITY;
        if let Some((sx, sy)) = self.scale {
            m = m.post_concat(Matrix::scale(sx, sy).about(pivot));
        }
        if let Some(degrees) = rotate {
            m = m.post_concat(Matrix::rotate(degrees.to_radians()).about(pivot));
        }
        if let Some((dx, dy)) = self.translate {
            m = m.post_concat(Matrix::translate(dx, dy));
        }
        m
    }
}

/// What the verb reported, in the shape both routes print.
struct Transformed {
    count: u64,
    clamped: bool,
    /// ` invocations=N pages=M` under `--leaf`, else empty.
    reach: String,
}

fn refuse(message: &str) -> u8 {
    eprintln!("pdfcer: object-transform refused: {message}");
    exit::EDIT_REFUSED
}

/// `object-transform` — scale/rotate/shear/move a selection by one page-space
/// matrix (Pass 113.0/113.1/113.2; `--leaf` Pass 536.0).
///
/// # Why the CLI composes the matrix rather than taking six numbers
///
/// A `--matrix a,b,c,d,e,f` flag would be a faithful mirror of the API and a
/// poor command line: nobody types a rotation matrix, and the pivot — which is
/// what makes a scale or a rotation land where the objects are rather than
/// flying toward the page origin — would have to be pre-composed by the
/// caller. So the flags name the gesture and the composition happens here,
/// through the same `Matrix::about` the shell uses.
///
/// # `--preview` is the same body, not a dry run
///
/// It calls `EditSession::transform_preview`, which shares one planner with
/// the verb. A preview that said yes where the verb refuses is not a reachable
/// state. There is no in-form preview verb, so `--preview --leaf` is refused.
pub(crate) fn cmd_object_transform(args: &ObjectTransformArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let plan = match transform_plan(args) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // The pivot defaults to the SELECTION's own centre. A default of the page
    // origin would send a scaled object flying off the sheet.
    let pivot = match plan.pivot {
        Some((x, y)) => Point::new(x, y),
        None => match selection_centre(&mut session, page_index, &plan.indices, args.leaf) {
            Ok(p) => p,
            Err(code) => return code,
        },
    };
    let matrix = plan.matrix(args.rotate, pivot);
    if args.preview {
        return report_preview(args, &mut session, &plan, matrix);
    }
    let transformed = match apply(args, &mut session, &plan, matrix) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let Some(output) = args.output else {
        return refuse("--output is required unless --preview");
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "object-transform {} page {} objects={} leaf={}{} mode={} -> {}; transformed={} clamped={} \
changed={} objects_written={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page,
        plan.indices.len(),
        u32::from(args.leaf),
        transformed.reach,
        args.mode.name(),
        output.display(),
        transformed.count,
        u32::from(transformed.clamped),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Parse and validate every flag that does not need the document.
fn transform_plan(args: &ObjectTransformArgs<'_>) -> Result<TransformPlan, u8> {
    let indices = match parse_object_indices(args.objects) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => return Err(refuse("--objects named no objects")),
        Err(message) => return Err(refuse(&message)),
    };
    if !args.preview && args.output.is_none() {
        return Err(refuse("--output is required unless --preview"));
    }
    if args.preview && args.leaf {
        return Err(refuse(
            "--preview cannot be combined with --leaf: there is no in-form preview",
        ));
    }
    let options = transform_options(args)?;
    let pair = |flag: &str, raw: Option<&str>| match raw.map(|r| parse_pair(flag, r)) {
        Some(Ok(p)) => Ok(Some(p)),
        Some(Err(message)) => Err(refuse(&message)),
        None => Ok(None),
    };
    let scale = pair("--scale", args.scale)?;
    let translate = pair("--translate", args.translate)?;
    let pivot = pair("--pivot", args.pivot)?;
    if scale.is_none() && args.rotate.is_none() && translate.is_none() {
        return Err(refuse(
            "nothing to do -- give at least one of --scale, --rotate, --translate",
        ));
    }
    Ok(TransformPlan {
        indices,
        options,
        scale,
        translate,
        pivot,
    })
}

/// `--on-mixed` and `--on-singular`.
fn transform_options(args: &ObjectTransformArgs<'_>) -> Result<TransformOptions, u8> {
    let mixed = match args.on_mixed {
        "whole" => MixedSelection::TransformWhole,
        "refuse" => MixedSelection::RefuseHeterogeneous,
        other => {
            return Err(refuse(&format!(
                "--on-mixed {other:?} is not whole or refuse"
            )));
        }
    };
    let singular = if args.on_singular == "refuse" {
        SingularPolicy::Refuse
    } else if let Some(min) = args.on_singular.strip_prefix("clamp:") {
        match min.parse::<f64>() {
            Ok(min) if min > 0.0 => SingularPolicy::Clamp { min },
            _ => {
                return Err(refuse(&format!(
                    "--on-singular clamp:MIN needs a positive MIN, got {min:?}"
                )));
            }
        }
    } else {
        return Err(refuse(&format!(
            "--on-singular {:?} is not refuse or clamp:MIN",
            args.on_singular
        )));
    };
    Ok(TransformOptions::default()
        .with_mixed(mixed)
        .with_singular(singular))
}

fn report_preview(
    args: &ObjectTransformArgs<'_>,
    session: &mut EditSession,
    plan: &TransformPlan,
    matrix: Matrix,
) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    match session.transform_preview(page_index, &plan.indices, matrix, plan.options) {
        Err(err) => report_edit_error(args.input, &err),
        Ok(outcome) => {
            report_disclosures(&outcome.disclosures);
            println!(
                "object-transform {} page {} objects={} PREVIEW; would_transform={} clamped={}",
                args.input.display(),
                args.page,
                plan.indices.len(),
                outcome.objects_transformed,
                u32::from(outcome.clamped),
            );
            exit::SUCCESS
        }
    }
}

/// Run the page verb, or the in-form verb under `--leaf`.
fn apply(
    args: &ObjectTransformArgs<'_>,
    session: &mut EditSession,
    plan: &TransformPlan,
    matrix: Matrix,
) -> Result<Transformed, u8> {
    let page_index = (args.page.max(1) - 1) as usize;
    if args.leaf {
        let out = session
            .transform_objects_in_form(page_index, &plan.indices, matrix, plan.options)
            .map_err(|err| report_edit_error(args.input, &err))?;
        report_disclosures(&out.disclosures);
        // The in-form verb reports no clamp flag; a clamp is disclosed above.
        return Ok(Transformed {
            count: plan.indices.len() as u64,
            clamped: false,
            reach: format!(" invocations={} pages={}", out.invocations, out.pages),
        });
    }
    let out = session
        .transform_objects(page_index, &plan.indices, matrix, plan.options)
        .map_err(|err| report_edit_error(args.input, &err))?;
    report_disclosures(&out.disclosures);
    Ok(Transformed {
        count: out.objects_transformed,
        clamped: out.clamped,
        reach: String::new(),
    })
}

/// The page-space bounding-box centre of a selection — `object-transform`'s
/// default pivot. Indices out of range fall back to the origin: the verb
/// raises that refusal, with the real count, in one place.
///
/// # Errors
///
/// An exit code, already reported to stderr.
fn selection_centre(
    session: &mut EditSession,
    page_index: usize,
    indices: &[usize],
    leaf: bool,
) -> Result<Point, u8> {
    use pdfcer_core::vector::Bounds;
    let model = match session.page_objects(page_index) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("pdfcer: object-transform: {err}");
            return Err(exit::EDIT_REFUSED);
        }
    };
    let mut bounds = Bounds::EMPTY;
    for &i in indices {
        let bbox = if leaf {
            model.leaves.get(i).map(|l| l.object.page_bbox())
        } else {
            model.objects.get(i).map(|o| o.page_bbox())
        };
        let Some(bbox) = bbox else {
            return Ok(Point::new(0.0, 0.0));
        };
        bounds = bounds.union(bbox);
    }
    if bounds.min.x > bounds.max.x {
        return Ok(Point::new(0.0, 0.0));
    }
    Ok(Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    ))
}
