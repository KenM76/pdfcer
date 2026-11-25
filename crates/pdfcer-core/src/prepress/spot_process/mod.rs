//! Spot to process: replace a `Separation` or `DeviceN` space whose
//! alternate is `DeviceCMYK` with `DeviceCMYK` itself, evaluating the
//! space's tint transform (ISO 32000-2 §8.6.6.4, §8.6.6.5) for every
//! colour that names it.
//!
//! The space's own array is never rewritten, so anything this pass does not
//! reach (a Type 3 glyph, a soft mask, a refused object) still draws
//! correctly with the spot. Only the tint transform is evaluated; a space
//! whose alternate is anything but `DeviceCMYK` is refused, because getting
//! CMYK from it is a colour conversion, which pdfcer leaves to icccer
//! (decision 064).

mod content;
mod samples;

use std::collections::BTreeMap;

use crate::function::PdfFunction;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::optimize::objects::is_image;
use crate::preflight::walk::{MAX_COLORANTS, is_spot_colorant};
use crate::view::DocumentView;

use super::inks::{InkPlan, InkSelection, each_array, family_name, ink_space, live_ids, text};
use super::rewrite::{SHADING_SEGMENTS, current, keep};

/// Nesting followed inside one object.
const MAX_NESTING: usize = 24;

/// How one convertible space maps its tints to CMYK.
pub(super) struct Conversion {
    /// Components the space takes.
    n: usize,
    /// Its tint transform.
    function: PdfFunction,
    /// The spot names, for reports.
    inks: Vec<Vec<u8>>,
}

impl Conversion {
    /// CMYK for `tints`, each output clamped to 0..1.
    pub(super) fn cmyk(&self, tints: &[f64]) -> Option<[f64; 4]> {
        let out = self.function.eval(tints).ok()?;
        let c = |i: usize| out.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        Some([c(0), c(1), c(2), c(3)])
    }

    /// CMYK for the space's initial colour, every tint 1.0 (§8.6.6.4,
    /// §8.6.6.5).
    pub(super) fn initial(&self) -> Option<[f64; 4]> {
        self.cmyk(&vec![1.0; self.n])
    }

    /// The first spot name.
    pub(super) fn ink(&self) -> &[u8] {
        self.inks.first().map_or(b"".as_slice(), Vec::as_slice)
    }
}

/// Which spaces convert, decided once per distinct space.
pub(super) struct Spaces<'v, 'd> {
    pub(super) view: &'v DocumentView<'d>,
    selection: &'v InkSelection,
    memo: Vec<(Object, Option<usize>)>,
    pub(super) conversions: Vec<Conversion>,
}

impl<'v, 'd> Spaces<'v, 'd> {
    fn new(view: &'v DocumentView<'d>, selection: &'v InkSelection) -> Self {
        Self {
            view,
            selection,
            memo: Vec::new(),
            conversions: Vec::new(),
        }
    }

    /// The conversion for colour space `cs`, or `None` when it is not a
    /// selected space or cannot be converted (recorded in `plan` once).
    pub(super) fn lookup(
        &mut self,
        cs: &Object,
        at: Option<ObjId>,
        plan: &mut InkPlan,
    ) -> Option<usize> {
        let resolved = self.view.resolve(cs);
        if !matches!(resolved, Object::Array(_)) {
            return None;
        }
        if let Some((_, hit)) = self.memo.iter().find(|(o, _)| o == resolved) {
            return *hit;
        }
        let key = resolved.clone();
        let at = cs.as_reference().or(at);
        let hit = self.decide(&key, at, plan).map(|c| {
            self.conversions.push(c);
            self.conversions.len() - 1
        });
        if hit.is_some() {
            plan.report.spaces += 1;
        }
        self.memo.push((key, hit));
        hit
    }

    fn decide(&self, cs: &Object, at: Option<ObjId>, plan: &mut InkPlan) -> Option<Conversion> {
        let items = cs.as_array()?;
        let space = ink_space(self.view, items)?;
        let spots: Vec<&Vec<u8>> = space.names.iter().filter(|n| is_spot_colorant(n)).collect();
        let first = spots.iter().find(|n| self.selection.selects(n))?;
        let report = &mut plan.report;
        if let Some(other) = spots.iter().find(|n| !self.selection.selects(n)) {
            let why = format!(
                "its space also names the spot \"{}\", which was not selected",
                text(other)
            );
            report.refuse(at, first, why);
            return None;
        }
        if space.capped {
            let why = format!(
                "its DeviceN space names more than {MAX_COLORANTS} colorants, the most pdfcer reads from one space (ISO 32000-1 Annex C)"
            );
            report.refuse(at, first, why);
            return None;
        }
        let alternate = family_name(self.view, space.alternate)
            .unwrap_or_else(|| "an unreadable space".to_owned());
        if !matches!(self.view.resolve(space.alternate), Object::Name(n) if n.as_bytes() == b"DeviceCMYK")
        {
            let why = format!(
                "its alternate space is {alternate}, not DeviceCMYK; converting it is a colour conversion, which pdfcer leaves to icccer"
            );
            report.refuse(at, first, why);
            return None;
        }
        let n = space.names.len();
        match PdfFunction::load(self.view, space.tint) {
            Ok(f) if f.inputs() == n && f.outputs() == 4 => Some(Conversion {
                n,
                function: f,
                inks: spots.into_iter().cloned().collect(),
            }),
            Ok(f) => {
                let why = format!(
                    "its tint transform takes {} and gives {} values, not {n} and 4",
                    f.inputs(),
                    f.outputs()
                );
                report.refuse(at, first, why);
                None
            }
            Err(e) => {
                report.refuse(
                    at,
                    first,
                    format!("its tint transform cannot be read ({e})"),
                );
                None
            }
        }
    }
}

/// Work out converting the selected spot inks to process everywhere this
/// pass reaches.
pub(crate) fn plan_spot_to_process(view: &DocumentView<'_>, selection: &InkSelection) -> InkPlan {
    let mut plan = InkPlan::default();
    let mut spaces = Spaces::new(view, selection);
    let ids = live_ids(view);
    if let InkSelection::Only(names) = selection {
        unknown_names(view, &ids, names, &mut plan);
    }
    content::rewrite_pages(&mut spaces, &mut plan);
    for id in ids {
        let Some(mut obj) = current(view, &plan.writes, id) else {
            continue;
        };
        if let Object::Stream(s) = &obj
            && is_image(&s.dict)
            && let Some(planned) = samples::image(&mut spaces, id, s, &mut plan)
        {
            plan.writes.insert(id, planned);
            plan.report.images += 1;
            continue;
        }
        if objects(&mut spaces, &mut obj, Some(id), 0, &mut plan) {
            keep(&mut plan.writes, id, obj);
        }
    }
    if plan.report.shadings > 0 {
        plan.report.disclose(format!(
            "Converted shadings are sampled at {} points along their axis and joined linearly.",
            SHADING_SEGMENTS + 1
        ));
    }
    plan
}

/// Rewrite `Indexed` spaces and shadings inside `obj`; true when it changed.
fn objects(
    spaces: &mut Spaces<'_, '_>,
    obj: &mut Object,
    at: Option<ObjId>,
    depth: usize,
    plan: &mut InkPlan,
) -> bool {
    if depth > MAX_NESTING {
        return false;
    }
    match obj {
        Object::Array(items) => {
            if let Some(converted) = samples::indexed(spaces, items, at, plan) {
                *obj = converted;
                plan.report.lookups += 1;
                return true;
            }
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= objects(spaces, item, at, depth + 1, plan);
            }
            changed
        }
        Object::Dict(d) => entries(spaces, d, at, depth, plan),
        Object::Stream(s) => entries(spaces, &mut s.dict, at, depth, plan),
        _ => false,
    }
}

fn entries(
    spaces: &mut Spaces<'_, '_>,
    d: &mut Dict,
    at: Option<ObjId>,
    depth: usize,
    plan: &mut InkPlan,
) -> bool {
    if d.contains_key(b"ShadingType") && samples::shading(spaces, d, at, plan) {
        plan.report.shadings += 1;
        return true;
    }
    let mut changed = false;
    for (_, value) in &mut d.0 {
        changed |= objects(spaces, value, at, depth + 1, plan);
    }
    changed
}

/// Disclose selected names that no space defines.
fn unknown_names(view: &DocumentView<'_>, ids: &[ObjId], names: &[String], plan: &mut InkPlan) {
    let mut defined: BTreeMap<Vec<u8>, ()> = BTreeMap::new();
    for id in ids {
        if let Some(value) = view.graph().value(*id) {
            each_array(value, 0, &mut |items| {
                if let Some(space) = ink_space(view, items) {
                    defined.extend(space.names.into_iter().map(|n| (n, ())));
                }
            });
        }
    }
    for name in names.iter().filter(|n| !defined.contains_key(n.as_bytes())) {
        plan.report
            .disclose(format!("No colour space names \"{name}\"."));
    }
}

/// `/DeviceCMYK`.
pub(super) fn device_cmyk() -> Object {
    Object::Name(Name::from(b"DeviceCMYK"))
}
