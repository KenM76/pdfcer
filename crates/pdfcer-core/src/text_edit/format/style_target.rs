//! Per-axis style targets for [`FormatRequest::style`]: bold or italic ON,
//! OFF, or kept, and the ladder that takes an axis off (`G086`).
//!
//! Taking an axis off walks the same three real-face rungs as putting one on
//! — a page face, the standard-14 sibling, a supplied face — but only within
//! the run's OWN family: dropping bold is not licence to change typeface. An
//! axis that was synthesised (`Tr 2` stroke, `Tm` shear, re-detected from the
//! bytes by [`detect`]) is removed by undoing the synthesis instead.

use super::{
    FontCandidate, FontPlan, FontSelector, FormatError, FormatRequest, PassedOver, StyleLadder,
    StyleRung, axes_label, family_stem, plan_font, survey_page_fonts,
};
use crate::object::Dict;
use crate::text_edit::edit::{OpRec, ShowData};
use crate::text_edit::synth::{
    StyleSynthesis, detect, matrix_scale, name_claims_bold, name_claims_italic,
};
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;

/// What [`FormatRequest::style`] asks of each axis: `Some(true)` on,
/// `Some(false)` off, `None` keep as it is.
///
/// A [`StyleSynthesis`] converts to the target that turns its axes on and
/// keeps the rest, so `req.style(StyleSynthesis::Bold)` still reads as
/// "make it bold".
///
/// ```
/// use pdfcer_core::text_edit::{FormatRequest, StyleSynthesis, StyleTarget};
///
/// let off = FormatRequest::new(0, "Total").style(StyleTarget::new(Some(false), None));
/// assert_eq!(off.set_style.unwrap().removes(), StyleSynthesis::Bold);
///
/// let on: StyleTarget = StyleSynthesis::Italic.into();
/// assert_eq!(on, StyleTarget::new(None, Some(true)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct StyleTarget {
    /// The bold axis: on, off, or `None` to keep.
    pub bold: Option<bool>,
    /// The italic axis: on, off, or `None` to keep.
    pub italic: Option<bool>,
}

impl StyleTarget {
    /// Keep both axes — the target that asks for nothing.
    pub const KEEP: Self = Self::new(None, None);

    /// A target from its two axes.
    #[must_use]
    pub const fn new(bold: Option<bool>, italic: Option<bool>) -> Self {
        Self { bold, italic }
    }

    /// The axes asked ON.
    #[must_use]
    pub const fn adds(self) -> StyleSynthesis {
        StyleSynthesis::new(
            matches!(self.bold, Some(true)),
            matches!(self.italic, Some(true)),
        )
    }

    /// The axes asked OFF.
    #[must_use]
    pub const fn removes(self) -> StyleSynthesis {
        StyleSynthesis::new(
            matches!(self.bold, Some(false)),
            matches!(self.italic, Some(false)),
        )
    }

    /// Whether both axes are kept, i.e. nothing is asked.
    #[must_use]
    pub const fn is_keep(self) -> bool {
        self.bold.is_none() && self.italic.is_none()
    }
}

impl From<StyleSynthesis> for StyleTarget {
    fn from(s: StyleSynthesis) -> Self {
        Self::new(s.bold().then_some(true), s.italic().then_some(true))
    }
}

/// The inputs every rung binds through.
pub(super) struct LadderCtx<'a> {
    pub(super) doc: &'a DocumentView<'a>,
    pub(super) resources: &'a Dict,
    pub(super) recs: &'a [OpRec],
    pub(super) req: &'a FormatRequest,
    pub(super) find: &'a str,
}

/// Bind `selector` through `set_font`'s one coverage gate (`R221`); a
/// coverage refusal is a rung miss recorded on `passed_over`, not an error.
pub(super) fn bind_style_face(
    ctx: &LadderCtx<'_>,
    selector: &str,
    embed: Option<&crate::font_embed::FontEmbedPlan>,
    passed_over: &mut Vec<PassedOver>,
) -> Result<Option<FontPlan>, FormatError> {
    let mut probe = ctx.req.clone();
    probe.set_font = Some(FontSelector::new(selector));
    probe.style_donors = Vec::new();
    probe.embed_font = embed.map(|p| Box::new(p.clone()));
    match plan_font(ctx.doc, ctx.resources, ctx.recs, &probe, ctx.find) {
        Ok(plan) => Ok(plan),
        Err(FormatError::CoverageFailure(r)) => {
            passed_over.push(PassedOver {
                base_font: r.base_font.clone(),
                reason: r.message.clone(),
                refusal: Some(r.clone()),
            });
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// The run's style as it stands: what its face's name claims, and what was
/// synthesised on top of it (re-detected from `Tr`, `w` and `Tm`).
fn current_style(anchor: &ShowData, base_font: &str) -> (StyleSynthesis, StyleSynthesis) {
    let face = StyleSynthesis::new(name_claims_bold(base_font), name_claims_italic(base_font));
    let tm = anchor.text_matrix;
    let synth = detect(
        base_font,
        anchor.text_state.params().render_mode,
        anchor.line_width.value(),
        anchor.tf_size * matrix_scale(tm),
        tm,
    );
    // An untracked matrix is not evidence of a shear.
    let synth = StyleSynthesis::new(synth.bold(), synth.italic() && anchor.matrix_known);
    (face, synth)
}

/// The face one axis needs, and whether that need is soft (an ON axis the
/// face lacks, which synthesis may supply if no face has it).
const fn face_need(target: Option<bool>, face: bool, synth: bool) -> (bool, bool) {
    match target {
        Some(false) => (false, false),
        Some(true) if !face && !synth => (true, true),
        _ => (face, false),
    }
}

type LadderOutcome = (Option<FontPlan>, Option<StyleLadder>, StyleSynthesis);

/// The ladder for a target that takes at least one axis OFF.
///
/// # Errors
///
/// [`FormatError::NoFaceWithoutStyle`] when the face itself carries an axis
/// asked off and no face of its family without it can show the run; anything
/// [`plan_font`] returns other than a coverage refusal.
pub(super) fn plan_style_off(
    ctx: &LadderCtx<'_>,
    anchor: &ShowData,
    orig_font: &ExtractFont,
    target: StyleTarget,
) -> Result<LadderOutcome, FormatError> {
    let (face, synth) = current_style(anchor, &orig_font.base_font);
    let (nb, soft_b) = face_need(target.bold, face.bold(), synth.bold());
    let (ni, soft_i) = face_need(target.italic, face.italic(), synth.italic());
    let mut ladder = StyleLadder {
        requested: target.adds(),
        removed: target.removes(),
        unsynthesised: StyleSynthesis::new(
            target.bold == Some(false) && synth.bold(),
            target.italic == Some(false) && synth.italic(),
        ),
        bound: None,
        same_family: None,
        rung: StyleRung::AlreadyStyled,
        synthesised: StyleSynthesis::None,
        passed_over: Vec::new(),
    };
    let mut want = StyleSynthesis::new(nb, ni);
    let mut plan = None;
    if want != face {
        let candidates = survey_page_fonts(ctx.doc, ctx.resources, ctx.recs, ctx.find);
        let mut po = Vec::new();
        plan = find_exact_face(ctx, &candidates, anchor, orig_font, want, &mut po)?;
        if plan.is_none() && (soft_b || soft_i) {
            ladder.synthesised = StyleSynthesis::new(soft_b, soft_i);
            want = StyleSynthesis::new(nb && !soft_b, ni && !soft_i);
            if want != face {
                plan = find_exact_face(ctx, &candidates, anchor, orig_font, want, &mut po)?;
            }
        }
        ladder.passed_over = po;
        match &plan {
            Some((p, rung)) => {
                let bound = p.font.base_font.clone();
                ladder.same_family = Some(family_stem(&bound) == family_stem(&orig_font.base_font));
                ladder.bound = Some(bound);
                ladder.rung = *rung;
            }
            None if want != face => {
                return Err(FormatError::NoFaceWithoutStyle {
                    run_font: orig_font.base_font.clone(),
                    style: axes_label(StyleSynthesis::new(
                        face.bold() && !want.bold(),
                        face.italic() && !want.italic(),
                    )),
                });
            }
            None => {}
        }
    }
    if ladder.bound.is_none() {
        ladder.rung = if !ladder.synthesised.is_none() {
            StyleRung::Synthetic
        } else if !ladder.unsynthesised.is_none() {
            StyleRung::SynthesisRemoved
        } else {
            StyleRung::AlreadyStyled
        };
    }
    let synthesised = ladder.synthesised;
    Ok((plan.map(|(p, _)| p), Some(ladder), synthesised))
}

/// The first face of the run's own family whose name claims exactly `want`
/// and that binds: a page face (rung 1), the standard-14 sibling (rung 2), a
/// supplied face (rung 3).
fn find_exact_face(
    ctx: &LadderCtx<'_>,
    candidates: &[FontCandidate],
    anchor: &ShowData,
    orig_font: &ExtractFont,
    want: StyleSynthesis,
    passed_over: &mut Vec<PassedOver>,
) -> Result<Option<(FontPlan, StyleRung)>, FormatError> {
    let family = family_stem(&orig_font.base_font);
    let exact = |name: &str| {
        family_stem(name) == family
            && name_claims_bold(name) == want.bold()
            && name_claims_italic(name) == want.italic()
    };
    for c in candidates
        .iter()
        .filter(|c| c.resource != anchor.font_name && exact(&c.base_font))
    {
        if let Err(e) = &c.accepted {
            passed_over.push(PassedOver {
                base_font: c.base_font.clone(),
                reason: e.to_string(),
                refusal: match e {
                    FormatError::CoverageFailure(r) => Some(r.clone()),
                    _ => None,
                },
            });
        } else if let Some(p) = bind_style_face(ctx, &c.selector, None, passed_over)? {
            return Ok(Some((p, StyleRung::RealFaceOnPage)));
        }
    }
    if let Some(own) = crate::fontdata::basefont_to_std14(orig_font.base_font.as_bytes())
        && let Some(sib) = crate::fontdata::std14_styled(own, want.bold(), want.italic())
        && sib != own
        && let Some(p) = bind_style_face(
            ctx,
            crate::fontdata::std14_base_font_name(sib),
            None,
            passed_over,
        )?
    {
        return Ok(Some((p, StyleRung::StandardFourteenSibling)));
    }
    for donor in ctx.req.style_donors.iter().filter(|d| exact(&d.base_name)) {
        if let Some(p) = bind_style_face(ctx, &donor.base_name, Some(donor), passed_over)? {
            return Ok(Some((p, StyleRung::SuppliedFaceEmbedded)));
        }
    }
    Ok(None)
}

/// Undo a synthetic bold: back to fill-only `0 Tr`, or `4 Tr` when the
/// ambient mode was `6` so the clip survives (§9.3.6 Table 106). The stroke
/// width and colour are left alone; no stroking mode reads them.
///
/// # Errors
///
/// [`FormatError::ConflictingRenderMode`] when the request also sets a mode;
/// [`FormatError::AmbientUnrestorable`] as for any text-state change.
pub(super) fn plan_unbold(
    set_ops: &mut Vec<u8>,
    restore_ops: &mut Vec<u8>,
    anchor: &ShowData,
    req: &FormatRequest,
    restore_narrowed: &mut Vec<crate::text_state::TextStateParam>,
    emitted_state: &mut Vec<crate::text_state::TextStateParam>,
) -> Result<(), FormatError> {
    if req.set_render_mode.is_some() {
        return Err(FormatError::ConflictingRenderMode);
    }
    let mode = if anchor.text_state.params().render_mode == 6 {
        4.0
    } else {
        0.0
    };
    super::push_state_param(
        set_ops,
        restore_ops,
        &anchor.text_state,
        crate::text_state::TextStateParam::RenderMode,
        mode,
        restore_narrowed,
        emitted_state,
    )
}

/// The rule-4 sentence for a ladder that took an axis off.
pub(super) fn disclosure(l: &StyleLadder, passed: &str) -> String {
    let off = axes_label(l.removed);
    let mut s = match (&l.bound, l.rung) {
        (Some(face), rung) => format!("style: {off} off via {rung} — bound '{face}'"),
        (None, StyleRung::AlreadyStyled) => {
            format!("style: the run is already not {off}; nothing to change")
        }
        (None, _) => format!("style: {off} off — the face is unchanged"),
    };
    if !l.unsynthesised.is_none() {
        s.push_str(&format!(
            "; the synthetic {} is removed ({})",
            axes_label(l.unsynthesised),
            match (l.unsynthesised.bold(), l.unsynthesised.italic()) {
                (true, true) => "Tr 2 stroke and Tm shear",
                (true, false) => "Tr 2 stroke",
                _ => "Tm shear",
            }
        ));
    }
    if !l.synthesised.is_none() {
        s.push_str(&format!(
            "; {} is synthesised (rung 4)",
            axes_label(l.synthesised)
        ));
    }
    s.push('.');
    s.push_str(passed);
    s
}
