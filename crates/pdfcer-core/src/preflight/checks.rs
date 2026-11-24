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

    fn push(&mut self, finding: Finding) {
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

const fn is_page_level(check: &Check) -> bool {
    matches!(
        check,
        Check::ImageBelowPpi { .. }
            | Check::NoTrimBox
            | Check::ColourSpaceUsed { .. }
            | Check::SpotNotAllowed { .. }
            | Check::OverprintUsed
            | Check::StrokeBelow { .. }
    )
}

const fn uses_walk(check: &Check) -> bool {
    matches!(
        check,
        Check::ColourSpaceUsed { .. } | Check::SpotNotAllowed { .. } | Check::OverprintUsed
    )
}

fn finding(
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

fn incomplete(rule: &ProfileRule, page: Option<usize>, why: &str) -> Finding {
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
            _ => {}
        }
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

fn fonts(view: &DocumentView<'_>, rule: &ProfileRule, sink: &mut Sink) {
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
            Check::ImageBelowPpi { min_ppi } => {
                let decomposed = objects
                    .get_or_insert_with(|| vector::decompose_page(view, page, Matrix::IDENTITY));
                match decomposed {
                    Ok(objs) => low_ppi(rule, *min_ppi, objs, number, sink),
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
            check if uses_walk(check) => colour(rule, &events, number, sink),
            _ => {}
        }
    }
}

fn no_trim_box(rule: &ProfileRule, page: &Page, number: usize, sink: &mut Sink) {
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

/// Every image object and form leaf placed below `min_ppi` on either axis.
fn low_ppi(
    rule: &ProfileRule,
    min_ppi: f64,
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
        let VectorObject::Image(image) = object else {
            continue;
        };
        if image.source == ImageSource::Form {
            continue;
        }
        let Some((w, h)) = image.pixel_size else {
            continue;
        };
        let m = image.ctm;
        let (Some(px), Some(py)) = (axis_ppi(w, m.a.hypot(m.b)), axis_ppi(h, m.c.hypot(m.d)))
        else {
            continue;
        };
        let worst = px.min(py);
        if worst < min_ppi {
            sink.push(Finding {
                rect: Some(rect(image.page_bbox)),
                measured: Some(Measure {
                    value: worst,
                    limit: min_ppi,
                    unit: MeasureUnit::Ppi,
                }),
                ..finding(
                    rule,
                    Some(number),
                    target,
                    format!("An image draws at {px:.0} x {py:.0} ppi, below {min_ppi:.0}."),
                )
            });
        }
    }
}

fn colour(rule: &ProfileRule, events: &[(Origin, Event)], number: usize, sink: &mut Sink) {
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

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
