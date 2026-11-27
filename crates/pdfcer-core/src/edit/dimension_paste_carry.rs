//! A pasted ce dimension's carried dictionary, planted over the one
//! `add_dimension` authored (`Pass 781.4`, decision 228).
//!
//! The model re-registers the sidecar record and group; the carried
//! dictionary supplies every key the model does not describe. The carried
//! `/AP` is kept byte-for-byte when the placement is a translation and the
//! destination group draws exactly as the copied one did; otherwise the
//! authored keys win and the redraw is disclosed.

use std::collections::BTreeMap;

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, PASTE_DROPPED_ANNOT_KEYS};
use crate::dimension::{AUTHORED_ANNOT_KEYS, AUTHORED_MEASURE_KEY, DimensionId};
use crate::object::{Dict, Name, ObjId, Object};
use crate::vector::clip::RawAnnotation;
use crate::vector::{ClipAnnotation, Matrix};

/// The copied group's drawing settings, compared against the destination's.
type Drawing = (
    crate::dimension::NumberFormat,
    crate::dimension::ScaleState,
    crate::dimension::DimStandard,
    crate::dimension::GroupStyle,
);

impl EditSession {
    /// Plant `annotation`'s carried dictionary over the ce dimension `id`
    /// just authored for it, folding everything committed since `mark`
    /// (the undo serial before the paste began) into one entry.
    pub(super) fn overlay_pasted_dimension(
        &mut self,
        id: DimensionId,
        annotation: &ClipAnnotation,
        at: Matrix,
        mark: u64,
        disclosures: &mut Vec<String>,
    ) -> Result<(), EditError> {
        let ClipAnnotation::Dimension {
            format,
            scale,
            standard,
            group_style,
            raw,
            ..
        } = annotation
        else {
            return Ok(());
        };
        let model = self.read_dimension_model();
        let Some(record) = model.dimension(id) else {
            return Ok(());
        };
        let (Some(annot_id), ap_id) = (record.annot, record.ap) else {
            return Ok(());
        };
        let destination = model
            .group(record.group)
            .map(|g| (g.format, g.scale, g.standard, g.style));
        let Some(Object::Dict(authored)) = self.value(annot_id).cloned() else {
            return Ok(());
        };
        let translation_only = at.b == 0.0 && at.c == 0.0 && at.a == 1.0 && at.d == 1.0;
        let copied: Drawing = (*format, *scale, *standard, *group_style);
        let keep_ap = translation_only && destination == Some(copied);
        let mut objects = Vec::new();
        let mut dict =
            self.materialize_carried_dimension(raw, annot_id, ap_id, keep_ap, &mut objects)?;
        let rebake = !keep_ap || !dict.contains_key(b"AP");
        if rebake {
            for key in AUTHORED_ANNOT_KEYS
                .iter()
                .copied()
                .chain([AUTHORED_MEASURE_KEY, b"AP".as_slice()])
            {
                match authored.get(key) {
                    Some(v) => {
                        dict.insert(Name::from(key), v.clone());
                    }
                    None => {
                        dict.remove(key);
                    }
                }
            }
        } else {
            self.place_pasted_rect(raw, &mut dict, &mut objects, at, disclosures);
        }
        if !dict.contains_key(b"OC")
            && let Some(oc) = authored.get(b"OC")
        {
            dict.insert(Name::from(b"OC"), oc.clone());
        }
        self.finish_pasted_dimension(annot_id, dict, objects)?;
        let own = self.entries_since(mark);
        if !self.coalesce_last(own, CommandKind::PasteObjects) {
            disclosures.push(
                "paste: undoing this ce dimension's paste takes more than one step -- its parts could not be grouped.".to_owned(),
            );
        }
        if rebake {
            disclosures.push(
                "paste: this ce dimension was rotated or scaled, or lands in a dimension group that draws differently from the one it was copied from, so pdfcer REDREW its appearance from its measurement; every key it does not draw from was carried unchanged.".to_owned(),
            );
        }
        Ok(())
    }

    /// The carried dictionary with its closure written at fresh numbers,
    /// except a single carried `/AP /N` stream, which is written INTO the
    /// record's own appearance object `ap_id` so the sidecar keeps pointing at
    /// the stream the annotation shows. Without `keep_ap` (or with no such
    /// stream) the carried `/AP` is left out.
    fn materialize_carried_dimension(
        &mut self,
        raw: &RawAnnotation,
        annot_id: ObjId,
        ap_id: Option<ObjId>,
        keep_ap: bool,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<Dict, EditError> {
        let mut imported: BTreeMap<u32, ObjId> = raw
            .objects
            .iter()
            .filter(|(_, o)| {
                matches!(&o.value, Object::Name(n) if n.as_bytes() == crate::formclip::CLIP_SELF_MARKER)
            })
            .map(|(n, _)| (*n, annot_id))
            .collect();
        let mut carried = raw.dict.clone();
        let normal = carried_normal_stream(&carried);
        match (keep_ap, ap_id, normal) {
            (true, Some(ap_id), Some(clip_id)) => {
                self.clip_materialize_into(&raw.objects, clip_id, ap_id, &mut imported, objects)?;
            }
            _ => {
                carried.remove(b"AP");
            }
        }
        let mut dict = self
            .clip_materialize_value(&raw.objects, &Object::Dict(carried), &mut imported, objects)?
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let regenerate_name = crate::xcut::carry::take_markers(
            &mut dict,
            PASTE_DROPPED_ANNOT_KEYS,
            b"NM",
            &mut BTreeMap::new(),
        );
        if regenerate_name && let Some(page_id) = self.page_of_annotation(annot_id) {
            let taken = self.page_annotation_names(page_id);
            dict.insert(
                Name::from(b"NM"),
                Object::String(crate::xcut::carry::unique_name(annot_id.num, &taken)),
            );
        }
        Ok(dict)
    }

    /// Commit the overlaid dictionary, keeping the authored `/Type` and `/P`.
    fn finish_pasted_dimension(
        &mut self,
        annot_id: ObjId,
        mut dict: Dict,
        mut objects: Vec<ObjectWrite>,
    ) -> Result<(), EditError> {
        let Some(Object::Dict(authored)) = self.value(annot_id).cloned() else {
            return Ok(());
        };
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
        if let Some(p) = authored.get(b"P") {
            dict.insert(Name::from(b"P"), p.clone());
        }
        objects.push(ObjectWrite {
            id: annot_id,
            before: Some(Object::Dict(authored)),
            after: Some(Object::Dict(dict)),
        });
        self.commit(Command {
            kind: CommandKind::PasteObjects,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }

    /// How many undo entries were pushed after serial `mark`.
    fn entries_since(&self, mark: u64) -> usize {
        self.undo
            .iter()
            .rev()
            .take_while(|e| e.serial > mark)
            .count()
    }

    /// The page whose `/P` the authored annotation names.
    fn page_of_annotation(&self, annot_id: ObjId) -> Option<ObjId> {
        match self.value(annot_id) {
            Some(Object::Dict(d)) => d.get(b"P").and_then(Object::as_reference),
            _ => None,
        }
    }
}

/// The clip-local id of a carried `/AP` whose `/N` is one stream reference
/// and which has no `/R` or `/D` (a dictionary of states would need more than
/// one target object).
fn carried_normal_stream(dict: &Dict) -> Option<u32> {
    let ap = dict.get(b"AP")?.as_dict()?;
    if ap.len() != 1 {
        return None;
    }
    ap.get(b"N")?.as_reference().map(|r| r.num)
}
