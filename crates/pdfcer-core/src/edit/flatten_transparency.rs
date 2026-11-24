//! [`EditSession::flatten_transparency`]: replace a page's transparent
//! painting with an opaque drawing of how it composites (ISO 32000-2 §11).
//!
//! The drawing comes from an injected [`TransparencyFlattener`]; this file
//! writes it. In [`FlattenMode::Overlay`] the page keeps its content and
//! the transparency that content selects is switched off copy-on-write: an
//! `ExtGState` becomes a direct opaque copy, an image a copy without its
//! soft mask, a form a copy without its `/Group`, so an object another page
//! shares is never changed under it.

use std::collections::{BTreeMap, BTreeSet};

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, PageSlot};
use crate::graph::ObjectGraph as _;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::transparency::{
    self, FlattenMode, FlattenOptions, FlattenTransparencyReport, FlattenedPage, PageFlattenReport,
    RasterPatch, TransparencyFlattener, TransparencyUse,
};

/// Each original object's opaque replacement (`None`: it needed none), and
/// the objects being replaced right now, so a cycle stops.
#[derive(Default)]
struct Neutral {
    done: BTreeMap<ObjId, Option<ObjId>>,
    open: BTreeSet<ObjId>,
}

/// One page to write: index, page object, the scan, the drawing.
type Plan = (usize, ObjId, TransparencyUse, FlattenedPage);

impl EditSession {
    /// Flatten transparency on `pages` (0-based) as one undo entry,
    /// [`CommandKind::FlattenTransparency`].
    ///
    /// A page whose content uses no transparency
    /// ([`crate::transparency::has_transparency`]) is left untouched and
    /// reported with `mode: None`. Annotations that use transparency are
    /// left as they are and listed in
    /// [`PageFlattenReport::annotations_left`]; flatten them first with
    /// [`Self::flatten_annotations`] to include them. Everything the
    /// flattener inferred (rasterised areas, outlined text, spot inks
    /// redrawn in process colour) is in the report and its
    /// [`FlattenTransparencyReport::disclosures`] (rule 4).
    ///
    /// Every page is read and flattened before the first write, so a
    /// failure writes nothing.
    ///
    /// # Errors
    ///
    /// - [`EditError::DocumentEncrypted`], a certification refusal or
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`]: the same
    ///   guards as [`Self::flatten_annotations`].
    /// - [`EditError::FlattenPpiOutOfRange`]: `raster_ppi` is outside
    ///   [`FlattenOptions::MIN_PPI`]`..=`[`FlattenOptions::MAX_PPI`].
    /// - [`EditError::PageOutOfRange`]: an index past the last page.
    /// - [`EditError::TransparencyFlattenFailed`]: the flattener failed,
    ///   or named a raster patch the page already uses.
    pub fn flatten_transparency(
        &mut self,
        pages: &[usize],
        options: &FlattenOptions,
        flattener: &dyn TransparencyFlattener,
    ) -> Result<FlattenTransparencyReport, EditError> {
        self.flatten_guards()?;
        let ppi = options.raster_ppi;
        if !(FlattenOptions::MIN_PPI..=FlattenOptions::MAX_PPI).contains(&ppi) {
            return Err(EditError::FlattenPpiOutOfRange {
                ppi,
                min: FlattenOptions::MIN_PPI,
                max: FlattenOptions::MAX_PPI,
            });
        }
        let slots = self.page_slots()?;
        let (plans, mut report) = self.plan_flatten(pages, options, flattener, &slots)?;
        if plans.is_empty() {
            report.disclosures = disclosures(&report, ppi);
            return Ok(report);
        }
        let mut objects: Vec<ObjectWrite> = Vec::new();
        let mut memo = Neutral::default();
        let mut wrap = None;
        let count = plans.len();
        for (index, page_id, found, flat) in plans {
            self.write_flattened(page_id, &slots, &flat, &mut memo, &mut wrap, &mut objects)?;
            report.pages.push(page_report(index, found, flat));
        }
        report.pages.sort_by_key(|p| {
            pages
                .iter()
                .position(|&i| i == p.page)
                .unwrap_or(usize::MAX)
        });
        report.changed = true;
        report.disclosures = disclosures(&report, ppi);
        self.commit(Command {
            kind: CommandKind::FlattenTransparency { pages: count },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(report)
    }

    /// The read phase: scan each page asked for once, and flatten those
    /// whose content uses transparency.
    fn plan_flatten(
        &self,
        pages: &[usize],
        options: &FlattenOptions,
        flattener: &dyn TransparencyFlattener,
        slots: &[PageSlot],
    ) -> Result<(Vec<Plan>, FlattenTransparencyReport), EditError> {
        let all = crate::page_tree::pages_in(&self.graph())?;
        let mut seen = BTreeSet::new();
        let mut wanted = Vec::new();
        for &index in pages {
            let page = all.get(index).ok_or(EditError::PageOutOfRange {
                index,
                count: all.len(),
            })?;
            if seen.insert(index) {
                wanted.push((index, page));
            }
        }
        let mut report = FlattenTransparencyReport::default();
        let mut plans = Vec::new();
        for (index, page) in wanted {
            let found = transparency::scan_page(&self.graph(), page);
            if !found.in_page_content() {
                report
                    .pages
                    .push(PageFlattenReport::untouched(index, found));
                continue;
            }
            let failed = |reason: String| EditError::TransparencyFlattenFailed {
                page: index,
                reason,
            };
            let flat = flattener
                .flatten_page(&self.view(), index, options)
                .map_err(|e| failed(e.to_string()))?;
            if let Some(name) = self.taken_patch_name(page.id, slots, &flat) {
                return Err(failed(format!("the raster name /{name} is already in use")));
            }
            plans.push((index, page.id, found, flat));
        }
        Ok((plans, report))
    }

    /// A raster patch name an overlay would shadow in the page's
    /// `/XObject` resources.
    fn taken_patch_name(
        &self,
        page_id: ObjId,
        slots: &[PageSlot],
        flat: &FlattenedPage,
    ) -> Option<String> {
        if flat.mode != FlattenMode::Overlay {
            return None;
        }
        let res = self.effective_resources(page_id, slots);
        let graph = self.graph();
        let xobj = res
            .get(b"XObject")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)?;
        flat.images
            .iter()
            .find(|p| xobj.get(p.name.as_bytes()).is_some())
            .map(|p| p.name.clone())
    }

    /// Write one page's drawing: the content stream, its raster patches,
    /// and the page dictionary without `/Group`.
    fn write_flattened(
        &mut self,
        page_id: ObjId,
        slots: &[PageSlot],
        flat: &FlattenedPage,
        memo: &mut Neutral,
        wrap: &mut Option<crate::page_tree::WrapStreams>,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(), EditError> {
        let Some(Object::Dict(page_dict)) = self.value(page_id).cloned() else {
            return Err(EditError::NotADictionary {
                id: page_id,
                key: "Contents",
            });
        };
        let mut updated = page_dict;
        updated.remove(b"Group");
        let mut xobjects = Vec::new();
        for patch in &flat.images {
            let id = self.write_patch(patch, objects)?;
            xobjects.push((patch.name.as_bytes().to_vec(), id));
        }
        let content = self.flate_stream(Dict::new(), &flat.content, objects)?;
        match flat.mode {
            FlattenMode::Rebuilt => {
                updated.insert(Name::from(b"Contents"), Object::Reference(content));
                updated.insert(Name::from(b"Resources"), Object::Dict(Dict::new()));
            }
            _ => {
                let graph = self.graph();
                let res = updated
                    .get(b"Resources")
                    .map(|o| graph.resolve(o))
                    .and_then(Object::as_dict)
                    .cloned()
                    .unwrap_or_else(|| self.effective_resources(page_id, slots));
                let res = self
                    .neutral_resources(&res, memo, 0, objects)?
                    .unwrap_or(res);
                updated.insert(Name::from(b"Resources"), Object::Dict(res));
                self.append_page_content(&mut updated, content, wrap, objects)?;
            }
        }
        if !xobjects.is_empty() {
            self.add_page_xobjects(&mut updated, page_id, &xobjects, slots);
        }
        objects.push(self.page_write(page_id, updated));
        Ok(())
    }

    /// A new stream holding `data` Flate-compressed (§7.4.4), or as is
    /// when compression fails.
    fn flate_stream(
        &mut self,
        mut dict: Dict,
        data: &[u8],
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        match super::flate_encode_for_save(data) {
            Some(packed) => {
                dict.insert(
                    Name::from(b"Filter"),
                    Object::Name(Name::from(b"FlateDecode")),
                );
                self.new_stream(dict, &packed, objects)
            }
            None => self.new_stream(dict, data, objects),
        }
    }

    /// An 8-bit `DeviceRGB` image XObject for `patch` (§8.9.5).
    fn write_patch(
        &mut self,
        patch: &RasterPatch,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let mut dict = Dict::new();
        let name = |n: &[u8]| Object::Name(Name::from(n));
        dict.insert(Name::from(b"Type"), name(b"XObject"));
        dict.insert(Name::from(b"Subtype"), name(b"Image"));
        dict.insert(Name::from(b"Width"), Object::Integer(patch.width.into()));
        dict.insert(Name::from(b"Height"), Object::Integer(patch.height.into()));
        dict.insert(Name::from(b"ColorSpace"), name(b"DeviceRGB"));
        dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
        self.flate_stream(dict, &patch.rgb, objects)
    }

    /// `res` with its transparency switched off, or `None` when it has
    /// none. Changed `ExtGState`s become direct opaque copies; changed
    /// XObjects, patterns and Type 3 fonts become new objects.
    fn neutral_resources(
        &mut self,
        res: &Dict,
        memo: &mut Neutral,
        depth: usize,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<Option<Dict>, EditError> {
        if depth > transparency::MAX_DEPTH {
            return Ok(None);
        }
        let mut out = res.clone();
        let mut changed = false;
        if let Some(gs) = self.resolved_sub(res, b"ExtGState") {
            let mut next = gs.clone();
            for (name, value) in gs.iter() {
                if let Some(opaque) = self.opaque_gstate(value) {
                    next.insert(name.clone(), Object::Dict(opaque));
                    changed = true;
                }
            }
            out.insert(Name::from(b"ExtGState"), Object::Dict(next));
        }
        for key in [&b"XObject"[..], b"Pattern", b"Font"] {
            let Some(sub) = self.resolved_sub(res, key) else {
                continue;
            };
            let mut next = sub.clone();
            for (name, value) in sub.iter() {
                let Some(id) = value.as_reference() else {
                    continue;
                };
                if let Some(new) = self.neutral_object(id, memo, depth + 1, objects)? {
                    next.insert(name.clone(), Object::Reference(new));
                    changed = true;
                }
            }
            out.insert(Name(key.to_vec()), Object::Dict(next));
        }
        Ok(changed.then_some(out))
    }

    /// The sub-dictionary `key` of `res`, resolved and cloned.
    fn resolved_sub(&self, res: &Dict, key: &[u8]) -> Option<Dict> {
        let graph = self.graph();
        res.get(key)
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
    }

    /// An opaque copy of the `ExtGState` `value`, or `None` when it sets
    /// no transparency (§11.6.4.4, §11.3.5, §11.6.5.2).
    fn opaque_gstate(&self, value: &Object) -> Option<Dict> {
        let graph = self.graph();
        let gs = graph.resolve(value).as_dict()?;
        if transparency::gstate_kinds(&graph, gs).is_empty() {
            return None;
        }
        let mut out = gs.clone();
        out.insert(Name::from(b"CA"), Object::Integer(1));
        out.insert(Name::from(b"ca"), Object::Integer(1));
        out.insert(Name::from(b"BM"), Object::Name(Name::from(b"Normal")));
        out.insert(Name::from(b"SMask"), Object::Name(Name::from(b"None")));
        Some(out)
    }

    /// A copy of XObject, pattern or font `id` with its transparency
    /// switched off, or `None` when it needed none (or is being replaced
    /// already, a cycle).
    fn neutral_object(
        &mut self,
        id: ObjId,
        memo: &mut Neutral,
        depth: usize,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<Option<ObjId>, EditError> {
        if let Some(done) = memo.done.get(&id) {
            return Ok(*done);
        }
        if depth > transparency::MAX_DEPTH || !memo.open.insert(id) {
            return Ok(None);
        }
        let value = self.graph().resolved(id).clone();
        let (mut dict, span) = match value {
            Object::Stream(s) => (s.dict, Some(s.data_span)),
            Object::Dict(d) => (d, None),
            _ => (Dict::new(), None),
        };
        let mut changed = self.strip_own_transparency(&mut dict);
        if let Some(res) = self.resolved_sub(&dict, b"Resources")
            && let Some(next) = self.neutral_resources(&res, memo, depth, objects)?
        {
            dict.insert(Name::from(b"Resources"), Object::Dict(next));
            changed = true;
        }
        memo.open.remove(&id);
        let replaced = if changed {
            let new = ObjId::new(self.alloc_number()?, 0);
            let after = match span {
                Some(data_span) => Object::Stream(Stream { dict, data_span }),
                None => Object::Dict(dict),
            };
            objects.push(ObjectWrite {
                id: new,
                before: None,
                after: Some(after),
            });
            Some(new)
        } else {
            None
        };
        memo.done.insert(id, replaced);
        Ok(replaced)
    }

    /// Remove what makes this object itself transparent: an image's
    /// `/SMask` and `/SMaskInData` (§11.6.5.3), a form's transparency
    /// `/Group` (§11.6.6), a shading pattern's transparent `/ExtGState`.
    /// True when anything changed.
    fn strip_own_transparency(&self, dict: &mut Dict) -> bool {
        let graph = self.graph();
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).cloned();
        let mut changed = false;
        if subtype.as_ref().is_some_and(|n| n.as_bytes() == b"Image") {
            changed |= dict.remove(b"SMask").is_some();
            changed |= dict.remove(b"SMaskInData").is_some();
        }
        if transparency::is_transparency_group(&graph, dict.get(b"Group")) {
            dict.remove(b"Group");
            changed = true;
        }
        if let Some(opaque) = dict.get(b"ExtGState").and_then(|g| self.opaque_gstate(g)) {
            dict.insert(Name::from(b"ExtGState"), Object::Dict(opaque));
            changed = true;
        }
        changed
    }
}

/// The report row for a flattened page.
fn page_report(page: usize, found: TransparencyUse, flat: FlattenedPage) -> PageFlattenReport {
    let mut row = PageFlattenReport::untouched(page, found);
    row.mode = Some(flat.mode);
    row.vector_regions = flat.vector_regions;
    row.rasterised = flat.rasterised;
    row.text_runs_outlined = flat.text_runs_outlined;
    row.strokes_outlined = flat.strokes_outlined;
    row.spot_inks = flat.spot_inks;
    row.notes = flat.notes;
    row
}

/// The operator-facing sentences for `report`.
fn disclosures(report: &FlattenTransparencyReport, ppi: f64) -> Vec<String> {
    let mut out = Vec::new();
    for p in &report.pages {
        let n = p.page + 1;
        match p.mode {
            None => out.push(format!(
                "page {n}: its content uses no transparency; left untouched"
            )),
            Some(mode) => {
                let how = match mode {
                    FlattenMode::Rebuilt => "redrawn in full",
                    _ => "transparent areas painted over opaquely",
                };
                out.push(format!(
                    "page {n}: {how}; {} vector region(s), {} area(s) rasterised at {ppi} ppi",
                    p.vector_regions,
                    p.rasterised.len()
                ));
            }
        }
        for r in &p.rasterised {
            let rect = r.rect;
            out.push(format!(
                "page {n}: rasterised [{:.1} {:.1} {:.1} {:.1}] ({})",
                rect.llx,
                rect.lly,
                rect.urx,
                rect.ury,
                r.reason.as_str()
            ));
        }
        if p.text_runs_outlined > 0 {
            out.push(format!(
                "page {n}: drew {} text run(s) as outlines",
                p.text_runs_outlined
            ));
        }
        if p.strokes_outlined > 0 {
            out.push(format!(
                "page {n}: drew {} stroke(s) as filled outlines",
                p.strokes_outlined
            ));
        }
        if !p.spot_inks.is_empty() {
            out.push(format!(
                "page {n}: redrew paint in spot ink(s) {} as process colour",
                p.spot_inks.join(", ")
            ));
        }
        if !p.annotations_left.is_empty() {
            out.push(format!(
                "page {n}: left {} annotation(s) that use transparency as they are",
                p.annotations_left.len()
            ));
        }
        if p.found.too_deep > 0 {
            out.push(format!(
                "page {n}: resources nested past {} levels were not checked",
                transparency::MAX_DEPTH
            ));
        }
        out.extend(p.notes.iter().map(|s| format!("page {n}: {s}")));
    }
    if report.changed {
        out.push(
            "an incremental save keeps the transparent original in the previous revision; a full rewrite removes it"
                .to_owned(),
        );
    }
    out
}
