//! `dimension-anchor`, `dimension-add --anchor`, and the anchor lines other
//! ce-dimension commands print (G332).

use super::*;
use pdfcer_core::dimension::{
    Anchor, AnchorRole, AnchorState, ArcCondition, DimensionEnd, DimensionId, Resolved,
};
use pdfcer_core::edit::EditSession;

/// The role an anchored point takes on its target; the names are the
/// sidecar tokens.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum AnchorRoleArg {
    /// The centre of a centre mark or a radius/diameter ce dimension.
    MarkCentre,
    /// A centre line's first end.
    LineEndA,
    /// A centre line's second end.
    LineEndB,
    /// A centre line's midpoint.
    LineMidpoint,
    /// The foot of the perpendicular from the other end onto the centre
    /// line, taken as an infinite line (linear ce dimensions only).
    LineAxis,
    /// A circle's centre, reached as a rim anchor.
    RimCentre,
    /// The rim point nearest the other end (linear ce dimensions only).
    RimMin,
    /// The rim point farthest from the other end (linear ce dimensions only).
    RimMax,
}

impl AnchorRoleArg {
    /// The engine's role.
    pub(crate) const fn role(self) -> AnchorRole {
        match self {
            Self::MarkCentre => AnchorRole::MarkCentre,
            Self::LineEndA => AnchorRole::LineEnd(DimensionEnd::A),
            Self::LineEndB => AnchorRole::LineEnd(DimensionEnd::B),
            Self::LineMidpoint => AnchorRole::LineMidpoint,
            Self::LineAxis => AnchorRole::LineAxis,
            Self::RimCentre => AnchorRole::Rim(ArcCondition::Centre),
            Self::RimMin => AnchorRole::Rim(ArcCondition::Min),
            Self::RimMax => AnchorRole::Rim(ArcCondition::Max),
        }
    }
}

/// Parse one `--anchor POINT:TARGET:ROLE`, ROLE as a sidecar token.
pub(crate) fn parse_anchor_spec(text: &str) -> Option<(usize, Anchor)> {
    let mut parts = text.splitn(3, ':');
    let point = parts.next()?.trim().parse().ok()?;
    let target = DimensionId(parts.next()?.trim().parse().ok()?);
    let role = AnchorRole::parse(parts.next()?.trim())?;
    Some((point, Anchor::new(target, role)))
}

/// Every `--anchor`, or the message naming the first malformed one.
pub(crate) fn parse_anchor_specs(specs: &[String]) -> Result<Vec<(usize, Anchor)>, String> {
    specs
        .iter()
        .map(|s| {
            parse_anchor_spec(s).ok_or_else(|| {
                let roles: Vec<&str> = AnchorRole::ALL.iter().map(|r| r.token()).collect();
                format!(
                    "--anchor {s:?} must be POINT:TARGET:ROLE, ROLE one of {}",
                    roles.join(", ")
                )
            })
        })
        .collect()
}

/// `solved`, or `unsolvable reason="..."`.
fn state_token(state: AnchorState) -> String {
    match state {
        AnchorState::Unsolvable(why) => format!("unsolvable reason=\"{why}\""),
        _ => "solved".to_owned(),
    }
}

/// One `    anchor point= target= role= state=` line per anchor of `id`.
pub(crate) fn print_anchor_lines(session: &EditSession, id: DimensionId) {
    for (point, anchor, state) in session.dimension_anchors(id) {
        println!(
            "    anchor point={point} target={} role={} state={}",
            anchor.target.0,
            anchor.role.token(),
            state_token(state)
        );
    }
}

/// The ce dimensions a command re-solved or detached
/// ([`EditSession::last_anchor_resolutions`], taken before saving), one line each
/// (`followed` or `detached`), so a value that changed on a ce dimension the
/// operator did not name is never silent.
pub(crate) fn print_anchor_report(report: &[Resolved]) {
    for r in report {
        if r.detached {
            println!(
                "  detached dimension={} value={:?} (its target was deleted or not pasted with it; it no longer follows it)",
                r.id.0, r.after_label
            );
        } else {
            println!(
                "  followed dimension={} before={:?} after={:?}",
                r.id.0, r.before_label, r.after_label
            );
        }
    }
}

/// The arguments of `dimension-anchor`; `anchor: None` is `--clear`.
pub(crate) struct AnchorEdit<'a> {
    pub(crate) input: &'a Path,
    pub(crate) dimension: u32,
    pub(crate) point: usize,
    pub(crate) anchor: Option<Anchor>,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `dimension-anchor` — attach one measured point of a ce dimension to a
/// centre mark, centre line or circle, or detach it, re-solving at once.
pub(crate) fn cmd_dimension_anchor(args: &AnchorEdit<'_>) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let id = DimensionId(args.dimension);
    let out = match session.set_dimension_anchor(id, args.point, args.anchor) {
        Ok(out) => out,
        Err(err) => return report_edit_error(input, &err),
    };
    let saved = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let anchor = args.anchor.map_or_else(
        || "none".to_owned(),
        |a| format!("target={} role={}", a.target.0, a.role.token()),
    );
    let state = out.state.map_or_else(|| "none".to_owned(), state_token);
    let r = &saved.report;
    println!(
        "dimension-anchor {} dimension={} point={} {anchor} state={state} before={:?} after={:?} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        args.dimension,
        args.point,
        out.before_label,
        out.after_label,
        args.mode.name(),
        args.output.display(),
        saved.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(saved.undo_verified),
        u32::from(saved.undo_identical),
    );
    if let Some(AnchorState::Unsolvable(why)) = out.state {
        eprintln!(
            "pdfcer: {}: the anchor is stored but could not be solved ({why}); the point stays where it was.",
            input.display()
        );
    }
    finish_edit(input, &saved)
}
