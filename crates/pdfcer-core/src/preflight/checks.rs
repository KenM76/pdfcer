//! The checks: document-level ones once, page-level ones per page.

use crate::fontinfo::{self, FontSubtype, Program};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::page_tree::{BoxResolution, Page, Rect};
use crate::space_audit::axis_ppi;
use crate::vector::{self, Bounds, ImageSource, Matrix, VectorObject};
use crate::view::DocumentView;

use super::walk::{self, ColourUse, Event, Origin};
use super::{
    Check, Finding, FindingKind, FindingTarget, Measure, MeasureUnit, Profile, ProfileRule,
    Severity,
};

/// Collects findings up to a cap and counts the rest.
pub(super) struct Sink {
    max: usize,
    findings: Vec<Finding>,
    dropped: usize,
}

impl Sink {
    /// A sink keeping at most `max` findings.
    pub(super) const fn new(max: usize) -> Self {
        Self {
            max,
            findings: Vec::new(),
            dropped: 0,
        }
    }

    /// Records `finding`, counting it as dropped once the cap is reached.
    pub(super) fn push(&mut self, finding: Finding) {
        if self.findings.len() < self.max {
            self.findings.push(finding);
        } else {
            self.dropped += 1;
        }
    }

    /// One `Incomplete` finding for every page-level rule.
    pub(super) fn incomplete_all(&mut self, profile: &Profile, page: Option<usize>, why: &str) {
        for rule in profile.rules.iter().filter(|r| is_page_level(&r.check)) {
            self.push(incomplete(rule, page, why));
        }
    }

    /// The kept findings and the dropped count.
    pub(super) fn finish(self) -> (Vec<Finding>, usize) {
        (self.findings, self.dropped)
    }
}

fn is_page_level(check: &Check) -> bool {
    matches!(
        check,
        Check::ImageBelowPpi { .. }
            | Check::ImageAbovePpi { .. }
            | Check::NoTrimBox
            | Check::ColourSpaceUsed { .. }
            | Check::SpotNotAllowed { .. }
            | Check::OverprintUsed
            | Check::StrokeBelow { .. }
            | Check::TransparencyUsed
            | Check::TotalAreaCoverage { .. }
    ) || matches!(check, Check::PdfxRequirement { level, row } if super::pdfx::is_page_row(row, *level))
        || matches!(check, Check::PdfaRequirement { level, row } if super::pdfa::is_page_row(row, *level))
}

fn uses_walk(check: &Check) -> bool {
    matches!(
        check,
        Check::ColourSpaceUsed { .. } | Check::SpotNotAllowed { .. } | Check::OverprintUsed
    ) || matches!(check, Check::PdfxRequirement { row, .. } if *row == "PXQ-CS-1")
        || matches!(check, Check::PdfaRequirement { row, .. } if super::pdfa::uses_walk(row))
}

/// A finding of `rule`'s severity, `Violation` kind, with no rect or measure.
pub(super) fn finding(
    rule: &ProfileRule,
    page: Option<usize>,
    target: FindingTarget,
    message: String,
) -> Finding {
    Finding {
        rule_id: rule.id.clone(),
        severity: rule.severity,
        kind: FindingKind::Violation,
        page,
        target,
        rect: None,
        message,
        measured: None,
    }
}

/// An `Incomplete` finding of `rule`: the part it needed could not be measured.
pub(super) fn incomplete(rule: &ProfileRule, page: Option<usize>, why: &str) -> Finding {
    Finding {
        kind: FindingKind::Incomplete,
        ..finding(
            rule,
            page,
            FindingTarget::Page,
            format!("Not checked: {why}."),
        )
    }
}

/// Document-level rules.
pub(super) fn document(view: &DocumentView<'_>, profile: &Profile, sink: &mut Sink) {
    let catalog = view
        .trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    for rule in &profile.rules {
        let doc = |message: &str| finding(rule, None, FindingTarget::Document, message.to_owned());
        match &rule.check {
            Check::Encrypted if view.trailer_entry(b"Encrypt").is_some() => {
                sink.push(doc("The file is encrypted."));
            }
            Check::NoOutputIntent if !has_output_intent(view, catalog) => {
                sink.push(doc("The document has no output intent."));
            }
            Check::VersionAbove { max } if view.version() > *max => {
                let version = view.version();
                sink.push(Finding {
                    measured: Some(Measure {
                        value: version_number(version),
                        limit: version_number(*max),
                        unit: MeasureUnit::Version,
                    }),
                    ..doc(&format!(
                        "The file is PDF {version}; the profile allows {max} at most."
                    ))
                });
            }
            Check::FontNotEmbedded => fonts(view, rule, sink),
            Check::PdfUa { part } => pdfua(view, rule, *part, sink),
            Check::PdfxRequirement { level, row } => {
                super::pdfx::document_row(view, rule, *level, row, sink);
            }
            Check::PdfaRequirement { level, row } => {
                super::pdfa::document_row(view, rule, *level, row, sink);
            }
            _ => {}
        }
    }
}

/// Run PDF/UA validation as one profile rule. The rule's id prefixes
/// each requirement id (`rule/PUI-…`).
fn pdfua(view: &DocumentView<'_>, rule: &ProfileRule, part: super::PdfuaPart, sink: &mut Sink) {
    for f in super::validate_pdfua(view, part).findings {
        let severity = if f.kind == FindingKind::NeedsPerson {
            Severity::Info
        } else {
            rule.severity
        };
        sink.push(Finding {
            rule_id: format!("{}/{}", rule.id, f.rule_id),
            severity,
            ..f
        });
    }
}

fn version_number(v: crate::PdfVersion) -> f64 {
    f64::from(v.major) + f64::from(v.minor) / 10.0
}

fn has_output_intent(view: &DocumentView<'_>, catalog: Option<&Dict>) -> bool {
    catalog
        .and_then(|c| c.get(b"OutputIntents"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .is_some_and(|a| !a.is_empty())
}

/// One finding per non-Type 3 font that is not embedded (shared with PDF/X FT-1).
pub(super) fn fonts(view: &DocumentView<'_>, rule: &ProfileRule, sink: &mut Sink) {
    for font in fontinfo::inventory(view).fonts {
        if font.subtype == FontSubtype::Type3 {
            continue;
        }
        let name = font.base_font.as_deref().unwrap_or("an unnamed font");
        let message = match &font.program {
            Program::Embedded(_) => continue,
            Program::NotEmbedded => format!("The font {name} is not embedded."),
            Program::Unreadable { why, .. } => {
                format!("The font {name} claims a program that cannot be read: {why}.")
            }
            _ => format!("The font {name} has a program this version cannot classify."),
        };
        let target = font.id.map_or(FindingTarget::Document, FindingTarget::Font);
        let page = font.pages.first().map(|&p| p as usize);
        sink.push(finding(rule, page, target, message));
    }
}

/// Page-level rules for page `number` (1-based).
pub(super) fn page(
    view: &DocumentView<'_>,
    profile: &Profile,
    page: &Page,
    number: usize,
    sink: &mut Sink,
) {
    let events = if profile.rules.iter().any(|r| uses_walk(&r.check)) {
        walk::page_events(view, page)
    } else {
        Vec::new()
    };
    let mut objects = None;
    for rule in &profile.rules {
        match &rule.check {
            Check::NoTrimBox => no_trim_box(rule, page, number, sink),
            Check::TransparencyUsed => transparency_used(rule, view, page, number, sink),
            Check::ImageBelowPpi { min_ppi: limit } | Check::ImageAbovePpi { max_ppi: limit } => {
                let below = matches!(rule.check, Check::ImageBelowPpi { .. });
                let decomposed = objects
                    .get_or_insert_with(|| vector::decompose_page(view, page, Matrix::IDENTITY));
                match decomposed {
                    Ok(objs) => image_ppi(rule, *limit, below, objs, number, sink),
                    Err(e) => sink.push(incomplete(
                        rule,
                        Some(number),
                        &format!("the page content could not be read ({e})"),
                    )),
                }
            }
            Check::StrokeBelow { min_width_pt } => {
                let decomposed = objects
                    .get_or_insert_with(|| vector::decompose_page(view, page, Matrix::IDENTITY));
                match decomposed {
                    Ok(objs) => thin_strokes(rule, *min_width_pt, objs, number, sink),
                    Err(e) => sink.push(incomplete(
                        rule,
                        Some(number),
                        &format!("the page content could not be read ({e})"),
                    )),
                }
            }
            Check::PdfxRequirement { level, row } => {
                let at = super::pdfx::PageCtx {
                    view,
                    page,
                    number,
                    events: &events,
                };
                super::pdfx::page_row(rule, *level, row, &at, sink);
            }
            Check::PdfaRequirement { level, row } => {
                let at = super::pdfa::PageCtx {
                    view,
                    page,
                    number,
                    events: &events,
                };
                super::pdfa::page_row(rule, *level, row, &at, sink);
            }
            check if uses_walk(check) => colour(rule, &events, number, sink),
            _ => {}
        }
    }
}

/// One finding per place [`crate::transparency::scan_page`] reports,
/// pointing at the annotation, graphics state or image that carries it.
pub(super) fn transparency_used(
    rule: &ProfileRule,
    view: &DocumentView<'_>,
    page: &Page,
    number: usize,
    sink: &mut Sink,
) {
    use crate::transparency::{TransparencyHolder as H, TransparencyKind as K};
    let found = crate::transparency::scan_page(view.graph(), page);
    if found.too_deep > 0 {
        sink.push(incomplete(
            rule,
            Some(number),
            "resources nested deeper than the scan follows were not checked",
        ));
    }
    for site in found.sites {
        let what = match &site.kind {
            K::StrokeAlpha(a) => format!("stroke alpha {a}"),
            K::FillAlpha(a) => format!("fill alpha {a}"),
            K::BlendMode(n) => format!("blend mode {}", String::from_utf8_lossy(n)),
            K::SoftMask => "a soft mask".to_owned(),
            K::Group => "a transparency group".to_owned(),
            K::ImageSoftMask | K::ImageSoftMaskInData => "an image soft mask".to_owned(),
        };
        let (target, holder) = match site.holder {
            H::Annotation(id) => (FindingTarget::Annotation(id), "An annotation"),
            H::GraphicsState(Some(id)) => (FindingTarget::GraphicsState(id), "A graphics state"),
            H::Image(id) => (FindingTarget::Image(id), "An image"),
            H::Form(_) => (FindingTarget::Page, "A form XObject"),
            _ => (FindingTarget::Page, "The page"),
        };
        let name = site
            .resource_name
            .map(|n| format!(" (/{})", String::from_utf8_lossy(&n)))
            .unwrap_or_default();
        let message = format!("{holder}{name} uses {what}.");
        sink.push(finding(rule, Some(number), target, message));
    }
}

/// A finding when the page has neither a TrimBox nor an ArtBox (shared with PDF/X BX-2).
pub(super) fn no_trim_box(rule: &ProfileRule, page: &Page, number: usize, sink: &mut Sink) {
    let defaulted = |r: BoxResolution| r == BoxResolution::Defaulted;
    if defaulted(page.trim_box_resolution) && defaulted(page.art_box_resolution) {
        sink.push(Finding {
            rect: Some(page.media_box),
            ..finding(
                rule,
                Some(number),
                FindingTarget::Page,
                "The page has no trim box or art box.".to_owned(),
            )
        });
    }
}

/// Every stroke on the page printing below `min_width_pt`, or of width 0.
fn thin_strokes(
    rule: &ProfileRule,
    min_width_pt: f64,
    objs: &vector::PageObjects,
    number: usize,
    sink: &mut Sink,
) {
    for h in super::hairlines_in(objs, min_width_pt) {
        let message = if h.zero_width {
            "A stroke has width 0, the thinnest line the device can draw.".to_owned()
        } else {
            format!(
                "A stroke prints {:.3} pt wide, below {min_width_pt} pt.",
                h.width_pt
            )
        };
        sink.push(Finding {
            rect: Some(h.rect),
            measured: Some(Measure {
                value: h.width_pt,
                limit: min_width_pt,
                unit: MeasureUnit::Points,
            }),
            ..finding(rule, Some(number), h.target, message)
        });
    }
}

/// A page-space [`Bounds`] as a [`Rect`].
pub(super) fn rect(b: Bounds) -> Rect {
    Rect {
        llx: b.min.x,
        lly: b.min.y,
        urx: b.max.x,
        ury: b.max.y,
    }
}

/// Every image object and form leaf placed below `limit` on either axis
/// (`below`), or above it on either axis.
fn image_ppi(
    rule: &ProfileRule,
    limit: f64,
    below: bool,
    objs: &vector::PageObjects,
    number: usize,
    sink: &mut Sink,
) {
    let top = objs
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (FindingTarget::PageObject(i), o));
    let leaves = objs
        .leaves
        .iter()
        .enumerate()
        .map(|(i, l)| (FindingTarget::FormLeaf(i), &l.object));
    for (target, object) in top.chain(leaves) {
        let Some((px, py, image)) = placed_ppi(object) else {
            continue;
        };
        let (value, hit, word) = if below {
            (px.min(py), px.min(py) < limit, "below")
        } else {
            (px.max(py), px.max(py) > limit, "above")
        };
        if hit {
            sink.push(Finding {
                rect: Some(rect(image.page_bbox)),
                measured: Some(Measure {
                    value,
                    limit,
                    unit: MeasureUnit::Ppi,
                }),
                ..finding(
                    rule,
                    Some(number),
                    target,
                    format!("An image draws at {px:.0} x {py:.0} ppi, {word} {limit:.0}."),
                )
            });
        }
    }
}

/// An image object's drawn resolution on its two axes; `None` for an
/// inline form image or one whose size or placement is unknown.
fn placed_ppi(object: &VectorObject) -> Option<(f64, f64, &vector::ImageObject)> {
    let VectorObject::Image(image) = object else {
        return None;
    };
    if image.source == ImageSource::Form {
        return None;
    }
    let (w, h) = image.pixel_size?;
    let m = image.ctm;
    let px = axis_ppi(w, m.a.hypot(m.b))?;
    let py = axis_ppi(h, m.c.hypot(m.d))?;
    Some((px, py, image))
}

/// One finding per colour use whose family the rule lists (shared with PDF/X CS-1).
pub(super) fn colour(
    rule: &ProfileRule,
    events: &[(Origin, Event)],
    number: usize,
    sink: &mut Sink,
) {
    for (origin, event) in events {
        let at = |use_: Option<&ColourUse>, fallback: FindingTarget| match origin {
            Origin::Annotation(id) => FindingTarget::Annotation(*id),
            Origin::Page => use_
                .and_then(|u| {
                    u.image
                        .map(FindingTarget::Image)
                        .or(u.space.map(FindingTarget::ColourSpace))
                })
                .unwrap_or(fallback),
        };
        let where_ = match origin {
            Origin::Page => "the page",
            Origin::Annotation(_) => "an annotation",
        };
        match (&rule.check, event) {
            (_, Event::Unwalked(why)) => sink.push(incomplete(rule, Some(number), why)),
            (Check::ColourSpaceUsed { families }, Event::Colour(u))
                if families.contains(&u.family) =>
            {
                let message = format!("{where_} paints in {}.", u.family.as_str());
                sink.push(finding(
                    rule,
                    Some(number),
                    at(Some(u), FindingTarget::Page),
                    capitalise(&message),
                ));
            }
            (Check::SpotNotAllowed { allowed }, Event::Colour(u)) => {
                for spot in u.spots.iter().filter(|s| !allowed.contains(s)) {
                    let message = format!(
                        "{where_} uses the spot ink \"{spot}\", which the profile does not allow."
                    );
                    sink.push(finding(
                        rule,
                        Some(number),
                        at(Some(u), FindingTarget::Page),
                        capitalise(&message),
                    ));
                }
            }
            (Check::OverprintUsed, Event::Overprint(gs)) => {
                let fallback = gs.map_or(FindingTarget::Page, FindingTarget::GraphicsState);
                let message = format!("{where_} turns overprint on.");
                sink.push(finding(
                    rule,
                    Some(number),
                    at(None, fallback),
                    capitalise(&message),
                ));
            }
            _ => {}
        }
    }
}

/// `s` with its first character upper-cased, for sentence-initial messages.
pub(super) fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
