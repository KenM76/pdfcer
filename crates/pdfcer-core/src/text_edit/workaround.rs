//! Pass 436.0: the opt-in workaround policy for a text edit the exact
//! surgery refuses (decision 175).
//!
//! By default every refusal stays a refusal and names the workaround on
//! offer. With [`WorkaroundPolicy::Apply`] the planner tries it: an exact
//! fix when one exists (joining text objects; rewriting `'`/`"` as the `T*`
//! and `Tj` ISO 32000-1 §9.4.3 Table 109 defines them to be), otherwise a
//! retype ([`super::retype`]). The workaround used is disclosed in
//! [`EditReport::workaround`](super::EditReport::workaround) and its
//! disclosures (rule 4); nothing is marked on the page.

use crate::content::ContentStream;
use crate::span::ByteSpan;
use crate::text_edit::cause::{NotFoundReason, UnsupportedCause};
use crate::text_edit::edit::{
    EditError, EditOptions, EditPlan, EditPlanTarget, EditRequest, OpRec, PlanMode, Rec, ShowOp,
    emit_show, find_anchor_span, plan_exact, splice, walk_records,
};
use crate::text_edit::encoding::{RInvTrigger, Refusal};
use crate::text_edit::retype;
use crate::view::DocumentView;
use crate::writer::content::emit_number;

/// Whether a refused edit is worked around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum WorkaroundPolicy {
    /// Refuse exactly as the exact surgery does; the error names the
    /// workaround on offer ([`EditError::workaround`]).
    #[default]
    Refuse,
    /// Apply the workaround on offer and disclose it.
    Apply,
}

/// A way round a refused text edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Workaround {
    /// Join show operators drawn in separate text objects on one line into
    /// one run (Pass 428.0's crossing edit). Exact.
    JoinTextObjects,
    /// Rewrite a `'` or `"` show operator as the `T*` (and `Tw`/`Tc`) plus
    /// `Tj` §9.4.3 Table 109 defines it to be, then edit the `Tj`. Exact.
    RewriteQuoteOperator,
    /// Remove the run's show operators from the content stream and set the
    /// new run text at the first operator's origin, size, colour and
    /// spacing: in the run's own font when it can encode the text, else in
    /// the fallback face. Approximate: the original kerning and per-glyph
    /// positioning are not kept, and the text is laid horizontally.
    Retype,
}

impl Workaround {
    /// Whether the result draws exactly what the original would have with
    /// the text replaced ([`Self::Retype`] is the one approximation).
    #[must_use]
    pub const fn is_exact(self) -> bool {
        !matches!(self, Self::Retype)
    }

    /// A short stable label: `join-text-objects`, `rewrite-quote-operator`
    /// or `retype`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::JoinTextObjects => "join-text-objects",
            Self::RewriteQuoteOperator => "rewrite-quote-operator",
            Self::Retype => "retype",
        }
    }
}

impl std::fmt::Display for Workaround {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::JoinTextObjects => "join the text objects into one run (exact)",
            Self::RewriteQuoteOperator => {
                "rewrite the ' or \" operator as the T* and Tj it is defined to be (exact)"
            }
            Self::Retype => {
                "retype the run: remove its show operators and set the new text at the run's \
                 origin, size, colour and spacing, in its own font or a fallback face \
                 (approximate: the original kerning is not kept)"
            }
        })
    }
}

/// The workaround an edit used, and the refusal it worked around.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkaroundUse {
    /// The workaround applied.
    pub workaround: Workaround,
    /// The exact surgery's refusal, as its sentence without the offer.
    pub refusal: String,
}

impl UnsupportedCause {
    /// The workaround on offer for this cause, or `None`.
    ///
    /// `None` for a cause no rewrite of the run can answer: a proxy form
    /// (`ReferenceXObject`, `OpiProxy`), no object numbers left, a failed
    /// commit, unrestorable state, and every cause about the request or
    /// the page rather than the run.
    #[must_use]
    pub const fn workaround(&self) -> Option<Workaround> {
        match self {
            Self::QuoteOperator => Some(Workaround::RewriteQuoteOperator),
            Self::CrossElementTj
            | Self::FontUnresolvable
            | Self::EncodingNotInvertible
            | Self::CompositeWithoutToUnicode
            | Self::FontMapNotInvertible { .. }
            | Self::VerticalWriting => Some(Workaround::Retype),
            _ => None,
        }
    }
}

impl NotFoundReason {
    /// The workaround on offer when the text is on the page but not in one
    /// run, or `None` when it is absent.
    #[must_use]
    pub const fn workaround(&self) -> Option<Workaround> {
        match self {
            Self::NoSuchText => None,
            Self::SpansTextObjects { .. } => Some(Workaround::JoinTextObjects),
            Self::SplitRun { .. } => Some(Workaround::Retype),
        }
    }
}

impl EditError {
    /// The workaround [`WorkaroundPolicy::Apply`] would try for this
    /// refusal, or `None`.
    ///
    /// A run-level font refusal (a composite font with no usable map, a
    /// symbolic font with no encoding, a `/ToUnicode`-only font) offers
    /// [`Workaround::Retype`]. A refusal of one character does not: the
    /// fallback face ([`EditOptions::with_fallback`]) is its route.
    #[must_use]
    pub fn workaround(&self) -> Option<Workaround> {
        match self {
            Self::Unsupported(cause) => cause.workaround(),
            Self::NoMatch { reason, .. } => reason.workaround(),
            Self::Refused(r) => refusal_workaround(r),
            _ => None,
        }
    }
}

fn refusal_workaround(r: &Refusal) -> Option<Workaround> {
    let run_level = matches!(
        r.trigger,
        RInvTrigger::Composite | RInvTrigger::SymbolicNoEncoding | RInvTrigger::ToUnicodeOnly
    );
    (run_level && r.character.is_none()).then_some(Workaround::Retype)
}

/// The suffix a refusal's sentence carries when a workaround is on offer.
pub(crate) fn offer(w: Option<Workaround>) -> String {
    w.map_or_else(String::new, |w| {
        format!(" -- a workaround is on offer when workarounds are enabled: {w}")
    })
}

/// [`offer`] for a font refusal.
pub(crate) fn refusal_offer(r: &Refusal) -> String {
    offer(refusal_workaround(r))
}

/// `e`'s sentence without the offer suffix.
pub(crate) fn bare(e: &EditError) -> String {
    let full = e.to_string();
    let suffix = offer(e.workaround());
    full.strip_suffix(suffix.as_str())
        .map_or_else(|| full.clone(), str::to_owned)
}

/// Everything a workaround route plans against.
pub(crate) struct Planning<'a> {
    pub(crate) doc: &'a DocumentView<'a>,
    pub(crate) target: &'a EditPlanTarget,
    pub(crate) stream: &'a ContentStream,
    pub(crate) recs: &'a [OpRec],
    pub(crate) opts: &'a EditOptions,
    pub(crate) mode: PlanMode,
}

/// Plan `req`: the exact surgery, then, when it refuses with a workaround
/// on offer and the policy is [`WorkaroundPolicy::Apply`], that workaround.
///
/// # Errors
///
/// The exact surgery's refusal; or [`EditError::WorkaroundRefused`] when
/// the workaround was tried and could not be applied.
pub(crate) fn plan(p: &Planning<'_>, req: &EditRequest) -> Result<EditPlan, EditError> {
    let refused = match plan_exact(p.doc, p.target, p.stream, p.recs, req, p.opts, p.mode) {
        Ok(plan) => return Ok(plan),
        Err(e) => e,
    };
    let Some(offered) = refused.workaround() else {
        return Err(refused);
    };
    if p.opts.workarounds != WorkaroundPolicy::Apply {
        return Err(refused);
    }
    let tried = match offered {
        Workaround::JoinTextObjects => join(p, req).or_else(|_| retype_route(p, req)),
        Workaround::RewriteQuoteOperator => match rewrite_quote(p, req) {
            Ok(plan) => Ok((plan, offered)),
            Err(Route::Inner(e)) if e.workaround() == Some(Workaround::Retype) => {
                retype_route(p, req)
            }
            Err(Route::Inner(e)) => return Err(e),
            Err(Route::Failed(why)) => Err(why),
        },
        Workaround::Retype => retype_route(p, req),
    };
    match tried {
        Ok((mut plan, used)) => {
            let refusal = bare(&refused);
            plan.report
                .disclosures
                .insert(0, disclosure(used, &refusal));
            plan.report.workaround = Some(WorkaroundUse {
                workaround: used,
                refusal,
            });
            Ok(plan)
        }
        Err(why) => Err(EditError::WorkaroundRefused {
            refused: Box::new(refused),
            workaround: offered,
            why,
        }),
    }
}

/// Rule 4: which workaround was applied, and to what.
fn disclosure(used: Workaround, refusal: &str) -> String {
    let kind = if used.is_exact() {
        "exact"
    } else {
        "approximate"
    };
    format!(
        "workaround ({}, {kind}): the exact edit was refused ({refusal}), so pdfcer applied: {used}",
        used.label()
    )
}

fn retype_route(p: &Planning<'_>, req: &EditRequest) -> Result<(EditPlan, Workaround), String> {
    retype::plan(p, req).map(|plan| (plan, Workaround::Retype))
}

/// The join: the same request pinned to the operator the joined match
/// starts in, as a spanning request, which may cross text objects.
fn join(p: &Planning<'_>, req: &EditRequest) -> Result<(EditPlan, Workaround), String> {
    let joined = retype::Joined::of(p.recs);
    let (pos, _) = joined
        .find(&req.find, None)
        .ok_or_else(|| "the joined text does not contain the find text".to_owned())?;
    let first = joined
        .op_at(pos)
        .and_then(|i| p.recs.get(i))
        .ok_or_else(|| "the match starts in no show operator".to_owned())?;
    let span = ByteSpan::from_range(first.start..first.end);
    let pinned = EditRequest::spanning_from(req.page_index, span, &req.find, &req.replace)
        .with_target(req.target);
    plan_exact(p.doc, p.target, p.stream, p.recs, &pinned, p.opts, p.mode)
        .map(|plan| (plan, Workaround::JoinTextObjects))
        .map_err(|e| e.to_string())
}

/// Why a route did not produce a plan.
enum Route {
    /// The route itself could not be applied.
    Failed(String),
    /// The rewrite succeeded and the exact surgery refused the result.
    Inner(EditError),
}

/// Rewrite the quote operator the request names as `T*` (with `Tw`/`Tc`
/// for `"`) plus `Tj`, and plan the request against the rewritten stream.
fn rewrite_quote(p: &Planning<'_>, req: &EditRequest) -> Result<EditPlan, Route> {
    let failed = |e: EditError| Route::Failed(e.to_string());
    let span = find_anchor_span(p.recs, req).map_err(failed)?;
    let Some(OpRec {
        start,
        end,
        rec: Rec::Show(s),
    }) = p.recs.get(span.first)
    else {
        return Err(Route::Failed("the match is not a show operator".to_owned()));
    };
    let mut bytes = Vec::new();
    if s.op == ShowOp::DoubleQuote {
        emit_number(&mut bytes, s.tw());
        bytes.extend_from_slice(b" Tw ");
        emit_number(&mut bytes, s.tc());
        bytes.extend_from_slice(b" Tc ");
    }
    bytes.extend_from_slice(b"T* ");
    let tj_at = start + bytes.len();
    bytes.extend(emit_show(&s.elems));
    let new_end = start + bytes.len();
    let buf = splice(&p.stream.buf, &mut [(*start, *end, bytes)]);
    let stream = ContentStream::parse(buf).map_err(|e| Route::Failed(e.to_string()))?;
    let recs = walk_records(p.doc, &p.target.resources, &stream);
    let mut moved = req.clone();
    moved.pinned_span = req.pinned_span.map(|pin| {
        let names_quote = pin.end() == *end && pin.start >= *start;
        if names_quote {
            ByteSpan::from_range(tj_at..new_end)
        } else if pin.start >= *end {
            ByteSpan::new(pin.start + new_end - *end, pin.len)
        } else {
            pin
        }
    });
    plan_exact(p.doc, p.target, &stream, &recs, &moved, p.opts, p.mode).map_err(Route::Inner)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test assertions
mod tests {
    use super::*;

    #[test]
    fn the_five_named_causes_offer_nothing() {
        for cause in [
            UnsupportedCause::ReferenceXObject,
            UnsupportedCause::OpiProxy,
            UnsupportedCause::ObjectNumbersExhausted,
            UnsupportedCause::CommitFailed {
                detail: String::new(),
            },
            UnsupportedCause::StateNotRestorable {
                detail: String::new(),
            },
        ] {
            assert_eq!(cause.workaround(), None, "{cause:?}");
            let e = EditError::Unsupported(cause);
            assert_eq!(e.to_string(), bare(&e));
        }
    }

    #[test]
    fn a_quote_offers_the_exact_rewrite_and_names_it() {
        let e = EditError::Unsupported(UnsupportedCause::QuoteOperator);
        assert_eq!(e.workaround(), Some(Workaround::RewriteQuoteOperator));
        assert!(e.to_string().contains("workaround is on offer"));
        assert!(!bare(&e).contains("workaround"));
        assert!(Workaround::RewriteQuoteOperator.is_exact());
        assert!(!Workaround::Retype.is_exact());
    }

    #[test]
    fn a_cross_object_match_offers_the_join() {
        let e = EditError::NoMatch {
            find: "x".to_owned(),
            reason: NotFoundReason::SpansTextObjects { objects: 2 },
        };
        assert_eq!(e.workaround(), Some(Workaround::JoinTextObjects));
        assert_eq!(EditError::no_match("x").workaround(), None);
    }
}
