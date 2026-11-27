//! The two field-paste branches behind
//! [`paste_field`](EditSession::paste_field): `Ctrl+V` plants an independent
//! field from a [`FieldClip`], `Ctrl+Shift+V` adds a widget to a field that
//! already exists. See `crate::formclip` for what a clip carries.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Command, CommandKind, EditError, EditSession, ObjectWrite, TooltipChoice, encode_text_string,
    placed_rects, rect_array, rewrite_da_font,
};
use crate::formclip::{FieldClip, FieldPasteOutcome};
use crate::forms;
use crate::forms_author::{self, FieldPath};
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{self, PageSlot};
use crate::xcut::carry::{take_markers, unique_name};

/// Widget keys a clip carries as `null` markers (they named source objects)
/// and the paste drops, counting each into
/// [`FieldPasteOutcome::dropped_keys`].
const PASTE_DROPPED_WIDGET_KEYS: &[&[u8]] = &[b"StructParent", b"Popup", b"IRT"];

/// What the `Ctrl+V` branch fixed before its widgets are written.
struct Landing<'c> {
    clip: &'c FieldClip,
    field_id: ObjId,
    page_id: ObjId,
    rect: page_tree::Rect,
    copy_actions: bool,
    /// The `/DR` font renames (copied name to installed name), applied to the
    /// field's and every widget's `/DA`.
    da_renames: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// The `/AcroForm` jobs of one `Ctrl+V`, applied in a single write: two
/// whole-dictionary writes to one object in one command do not compose.
struct AcroFormJobs {
    register_root: Option<ObjId>,
    dr_fonts: Vec<(Vec<u8>, ObjId)>,
    calculation: Option<ObjId>,
    sig_flag: bool,
}

impl AcroFormJobs {
    fn apply(&self, af: &mut Dict) {
        // §12.7.3.1 makes `/Fields` the ROOT list; a node with a `/Parent`
        // must not appear there, or the walk gives it two names.
        if let Some(root) = self.register_root {
            append_to_array(af, b"Fields", root);
        }
        insert_dr_fonts(af, &self.dr_fonts);
        if let Some(field_id) = self.calculation {
            append_to_array(af, b"CO", field_id);
        }
        if self.sig_flag {
            let current = af.get(b"SigFlags").and_then(Object::as_int).unwrap_or(0);
            af.insert(Name::from(b"SigFlags"), Object::Integer(current | 1));
        }
    }
}

/// Bind each `(name, font)` in the form's `/DR /Font`.
fn insert_dr_fonts(af: &mut Dict, fonts: &[(Vec<u8>, ObjId)]) {
    if fonts.is_empty() {
        return;
    }
    let mut dr = direct_dict(af, b"DR");
    let mut table = direct_dict(&dr, b"Font");
    for (name, id) in fonts {
        table.insert(Name::from(name.as_slice()), Object::Reference(*id));
    }
    dr.insert(Name::from(b"Font"), Object::Dict(table));
    af.insert(Name::from(b"DR"), Object::Dict(dr));
}

fn append_to_array(dict: &mut Dict, key: &[u8], id: ObjId) {
    let mut items = match dict.get(key) {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    items.push(Object::Reference(id));
    dict.insert(Name::from(key), Object::Array(items));
}

fn direct_dict(dict: &Dict, key: &[u8]) -> Dict {
    match dict.get(key) {
        Some(Object::Dict(d)) => d.clone(),
        _ => Dict::new(),
    }
}

/// The font resource name a direct `/DA` string selects with `Tf`.
fn own_da_font(dict: &Dict) -> Option<Vec<u8>> {
    match dict.get(b"DA")? {
        Object::String(da) => crate::vartext::parse_default_appearance(da)
            .ok()
            .map(|parsed| parsed.font_name),
        _ => None,
    }
}

/// Rewrite the `Tf` font of `dict`'s `/DA` when `renames` maps it.
fn rename_da(dict: &mut Dict, renames: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let Some(from) = own_da_font(dict) else {
        return;
    };
    let (Some(to), Some(Object::String(da))) = (renames.get(&from), dict.get(b"DA")) else {
        return;
    };
    let rewritten = rewrite_da_font(da, &from, to);
    dict.insert(Name::from(b"DA"), Object::String(rewritten));
}

/// [`FieldPasteOutcome::font_renames`] from the byte-keyed rename map.
fn font_renames(renames: &BTreeMap<Vec<u8>, Vec<u8>>) -> BTreeMap<String, String> {
    renames
        .iter()
        .map(|(from, to)| {
            (
                String::from_utf8_lossy(from).into_owned(),
                String::from_utf8_lossy(to).into_owned(),
            )
        })
        .collect()
}

impl EditSession {
    /// The `Ctrl+V` half: plant an independent field.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paste_new_field(
        &mut self,
        clip: &FieldClip,
        name: &str,
        page_id: ObjId,
        slots: &[PageSlot],
        path: FieldPath,
        rect: page_tree::Rect,
        tooltip: &TooltipChoice,
        copy_value: bool,
        copy_actions: bool,
        ft: forms::FieldType,
        mut disclosures: Vec<String>,
        imported: &mut BTreeMap<u32, ObjId>,
    ) -> Result<FieldPasteOutcome, EditError> {
        // The preflight MERGES a same-type terminal, which is what
        // `Ctrl+Shift+V` wants and what `Ctrl+V` must refuse.
        let FieldPath::Vacant { deepest, remaining } = path else {
            return Err(EditError::FieldNameTaken {
                name: name.to_owned(),
            });
        };
        let field_id = ObjId::new(self.alloc_number()?, 0);
        // Deferred: the `/AcroForm` is written once, by `AcroFormJobs`.
        let (mut objects, parent, partial, register_root) =
            self.place_new_field_deferred(deepest, &remaining, field_id)?;
        let mut field = self.pasted_field_half(
            clip,
            (copy_value, copy_actions, ft),
            imported,
            &mut objects,
            &mut disclosures,
        )?;
        name_pasted_field(&mut field, &partial, parent, tooltip);
        let (dr_fonts, da_renames) =
            self.install_pasted_da_fonts(clip, None, imported, &mut objects, &mut disclosures)?;
        rename_da(&mut field, &da_renames);
        let font_renames = font_renames(&da_renames);
        let materialized_keys = materialized_keys(clip, &field, &mut disclosures);
        let landing = Landing {
            clip,
            field_id,
            page_id,
            rect,
            copy_actions,
            da_renames,
        };
        let mut dropped_keys = BTreeMap::new();
        let widget_ids = self.paste_field_widgets(
            &landing,
            field,
            slots,
            imported,
            &mut objects,
            &mut dropped_keys,
            &mut disclosures,
        )?;
        let jobs = AcroFormJobs {
            register_root,
            dr_fonts,
            calculation: (copy_actions && clip.carries_calculation()).then_some(field_id),
            sig_flag: ft == forms::FieldType::Signature,
        };
        self.commit_pasted_field(&jobs, objects, &mut disclosures)?;
        Ok(FieldPasteOutcome {
            field_id,
            widget_ids,
            merged: clip.widget_count() == 1,
            created: true,
            disclosures,
            radio_on_state: None,
            materialized_keys,
            dropped_keys,
            font_renames,
        })
    }

    /// Write the `/AcroForm` jobs and commit the paste as one undo entry.
    fn commit_pasted_field(
        &mut self,
        jobs: &AcroFormJobs,
        mut objects: Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> Result<(), EditError> {
        objects.extend(self.pasted_field_acroform(jobs, disclosures)?);
        self.commit(Command {
            kind: CommandKind::PasteFormField,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }

    /// The clip's field half materialised here, minus the value and actions
    /// the operator declined (`flags` is `copy_value`, `copy_actions`, `/FT`).
    fn pasted_field_half(
        &mut self,
        clip: &FieldClip,
        (copy_value, copy_actions, ft): (bool, bool, forms::FieldType),
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> Result<Dict, EditError> {
        let mut field = self
            .clip_materialize_value(
                clip.objects(),
                &Object::Dict(clip.field_dict().clone()),
                imported,
                objects,
            )?
            .as_dict()
            .cloned()
            .unwrap_or_default();
        if !copy_value && field.remove(b"V").is_some() {
            disclosures.push(
                "paste: the copied field's VALUE was not carried -- the new field starts empty. Its default value (/DV) travelled, so Reset Form restores what the original reset to."
                    .to_owned(),
            );
        }
        if ft == forms::FieldType::Signature && field.remove(b"V").is_some() {
            disclosures.push(
                "paste: a signature's value covers a byte range of the document it was made in (ISO 32000-1 12.7.4.5), so it cannot travel. The pasted field is an EMPTY signature field."
                    .to_owned(),
            );
        }
        if !copy_actions {
            if field.remove(b"AA").is_some() || clip.carries_actions() {
                disclosures.push(
                    "paste: the copied field's actions (/AA on the field, /A and /AA on its widgets -- format, calculate, validate, keystroke or click scripts) were NOT carried. Nothing on the page shows this; a field that used to reformat or compute now does neither."
                        .to_owned(),
                );
            }
        } else if clip.carries_actions() {
            disclosures.push(
                "paste: the copied field's actions (/AA) travelled with it. pdfcer never executes them (decision 008, NF4), and it does NOT check whether the field names inside a script exist in this document -- a calculation that referred to fields the source had can be silently inert here."
                    .to_owned(),
            );
        }
        Ok(field)
    }

    /// Install the clip's `/DA` fonts (only `only`, when given) into this
    /// document's `/DR /Font` under the names the `/DA` strings use
    /// (§12.7.3.3). A name this document binds to a DIFFERENT font gets a free
    /// name instead, never one the clip itself installs. Returns the `/DR`
    /// entries to write and the renames every pasted `/DA` must receive.
    #[allow(clippy::type_complexity)] // Two independent results.
    fn install_pasted_da_fonts(
        &mut self,
        clip: &FieldClip,
        only: Option<&[u8]>,
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> Result<(Vec<(Vec<u8>, ObjId)>, BTreeMap<Vec<u8>, Vec<u8>>), EditError> {
        let entries: Vec<(Vec<u8>, u32)> = clip
            .da_font_entries()
            .iter()
            .filter(|(name, _)| only.is_none_or(|o| o == name.as_slice()))
            .cloned()
            .collect();
        let mut reserved: BTreeSet<Vec<u8>> = entries.iter().map(|(n, _)| n.clone()).collect();
        let (mut installs, mut renames) = (Vec::new(), BTreeMap::new());
        for (wanted, clip_object) in entries {
            let font_id = self.clip_materialize(clip.objects(), clip_object, imported, objects)?;
            let shown = String::from_utf8_lossy(&wanted).into_owned();
            let name = match (self.dr_font_object(&wanted), self.dr_font_name_of(font_id)) {
                (Some(present), _) if self.same_font_object(&present, clip, clip_object) => {
                    wanted.clone()
                }
                // An earlier paste of the same batch installed this very object.
                (_, Some(installed)) => {
                    if installed != wanted {
                        disclosures.push(format!(
                            "paste: the copied font /{shown} is already installed here as /{}, so every pasted /DA that named it was rewritten to that name.",
                            String::from_utf8_lossy(&installed),
                        ));
                    }
                    installed
                }
                (Some(_), None) => {
                    let fresh = self.free_dr_font_name(&wanted, &reserved);
                    reserved.insert(fresh.clone());
                    disclosures.push(format!(
                        "paste: this document already uses the font resource /{shown} for a different font, so the copied one was installed as /{} and every pasted /DA that named it rewritten to name it. The pasted field therefore keeps the face, size and colour it had.",
                        String::from_utf8_lossy(&fresh),
                    ));
                    installs.push((fresh.clone(), font_id));
                    fresh
                }
                (None, None) => {
                    installs.push((wanted.clone(), font_id));
                    wanted.clone()
                }
            };
            if name != wanted {
                renames.insert(wanted, name);
            }
        }
        Ok((installs, renames))
    }

    /// Write the widgets: one merged (Shape A) dictionary for a single-widget
    /// clip, else the field with a `/Kids` of bare widgets (Shape B).
    #[allow(clippy::too_many_arguments)]
    fn paste_field_widgets(
        &mut self,
        landing: &Landing<'_>,
        mut field: Dict,
        slots: &[PageSlot],
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        dropped: &mut BTreeMap<String, u64>,
        disclosures: &mut Vec<String>,
    ) -> Result<Vec<ObjId>, EditError> {
        let (clip, field_id, page_id) = (landing.clip, landing.field_id, landing.page_id);
        let rects = placed_rects(clip, landing.rect, disclosures);
        let rect_of = |i: usize| rect_array(rects.get(i).copied().unwrap_or(landing.rect));
        if clip.widget_count() == 1 {
            // Shape A (§12.5.6.19), the shape every `add_*_field` verb writes.
            let widget = self.pasted_widget(landing, 0, field_id, imported, objects, dropped)?;
            fold_widget_into_field(&mut field, widget, disclosures);
            mark_widget(&mut field, page_id, rect_of(0));
            objects.push(ObjectWrite {
                id: field_id,
                before: None,
                after: Some(Object::Dict(field)),
            });
            objects.extend(self.annots_writes(page_id, field_id, slots)?);
            return Ok(vec![field_id]);
        }
        let mut widget_ids = Vec::with_capacity(clip.widget_count());
        let mut pending = Vec::with_capacity(clip.widget_count());
        for index in 0..clip.widget_count() {
            let widget_id = ObjId::new(self.alloc_number()?, 0);
            let mut widget =
                self.pasted_widget(landing, index, widget_id, imported, objects, dropped)?;
            // R101: a kid with `/T`, `/FT` or `/Kids` is read as a child
            // field. Its own `/AA` is a legal annotation trigger (Table 194).
            for key in forms_author::FIELD_ONLY_KEYS
                .iter()
                .filter(|k| **k != b"AA")
            {
                widget.remove(key);
            }
            mark_widget(&mut widget, page_id, rect_of(index));
            widget.insert(Name::from(b"Parent"), Object::Reference(field_id));
            widget_ids.push(widget_id);
            pending.push(ObjectWrite {
                id: widget_id,
                before: None,
                after: Some(Object::Dict(widget)),
            });
        }
        let kids = widget_ids.iter().map(|id| Object::Reference(*id)).collect();
        field.insert(Name::from(b"Kids"), Object::Array(kids));
        objects.push(ObjectWrite {
            id: field_id,
            before: None,
            after: Some(Object::Dict(field)),
        });
        objects.extend(pending);
        objects.extend(self.annots_append(page_id, &widget_ids, slots)?);
        Ok(widget_ids)
    }

    /// One clip widget materialised for `Ctrl+V`: a fresh `/NM` when the
    /// source had one, and the field's `/DA` font rename applied.
    fn pasted_widget(
        &mut self,
        landing: &Landing<'_>,
        index: usize,
        widget_id: ObjId,
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        dropped: &mut BTreeMap<String, u64>,
    ) -> Result<Dict, EditError> {
        let (mut widget, renamed) = self.materialize_widget_dict(
            landing.clip,
            index,
            landing.copy_actions,
            imported,
            objects,
            dropped,
        )?;
        if renamed {
            self.name_pasted_widget(&mut widget, widget_id, landing.page_id);
        }
        rename_da(&mut widget, &landing.da_renames);
        Ok(widget)
    }

    /// Give a widget whose source had an `/NM` a name unique on `page_id`
    /// (§12.5.2 Table 164).
    fn name_pasted_widget(&self, widget: &mut Dict, seed: ObjId, page_id: ObjId) {
        let taken = self.page_annotation_names(page_id);
        widget.insert(
            Name::from(b"NM"),
            Object::String(unique_name(seed.num, &taken)),
        );
    }

    /// The single `/AcroForm` write of a `Ctrl+V`, with the disclosures its
    /// calculation-order and signature-flag jobs owe.
    fn pasted_field_acroform(
        &mut self,
        jobs: &AcroFormJobs,
        disclosures: &mut Vec<String>,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        if jobs.calculation.is_some() {
            disclosures.push(
                "paste: the copied field carries a CALCULATE action, so it was appended to the document's calculation order (/AcroForm /CO) -- ISO 32000-1 Table 218 requires that array once any field has one. It is appended at the end; if this field must compute before another, reorder it deliberately."
                    .to_owned(),
            );
        }
        if jobs.sig_flag {
            disclosures.push(
                "paste: /AcroForm /SigFlags bit 1 (SignaturesExist) was set, because the document now contains a signature field."
                    .to_owned(),
            );
        }
        let (dr_fonts, mut objects) = self.dr_font_objects(&[])?;
        objects.extend(self.acroform_write(&mut |af| {
            Self::ensure_default_resources(af, &dr_fonts);
            jobs.apply(af);
        })?);
        Ok(objects)
    }

    /// The `Ctrl+Shift+V` half: another view of a field that already exists.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paste_additional_widget(
        &mut self,
        clip: &FieldClip,
        name: &str,
        page_id: ObjId,
        slots: &[PageSlot],
        path: FieldPath,
        rect: page_tree::Rect,
        mut disclosures: Vec<String>,
        imported: &mut BTreeMap<u32, ObjId>,
        widget: Option<usize>,
    ) -> Result<FieldPasteOutcome, EditError> {
        // The mirror refusal: a field that is not here has no view to add,
        // and creating one would be an independent field nobody asked for.
        let FieldPath::Terminal { id, shape, .. } = path else {
            return Err(EditError::FieldNotFound {
                name: name.to_owned(),
            });
        };
        if clip.widget_count() > 1 && widget.is_none() {
            disclosures.push(format!(
                "paste: the copied field had {} widgets; ONE was added here. Pasting them all would have given this field several views carrying the same export values, which for a radio group means buttons that select together.",
                clip.widget_count()
            ));
        }
        let mut objects: Vec<ObjectWrite> = Vec::new();
        let mut dropped_keys = BTreeMap::new();
        let (mut widget, renamed) = self.materialize_widget_dict(
            clip,
            widget.unwrap_or(0),
            true,
            imported,
            &mut objects,
            &mut dropped_keys,
        )?;
        if renamed {
            self.name_pasted_widget(&mut widget, id, page_id);
        }
        let (dr_fonts, da_renames) = self.install_widget_da_font(
            clip,
            &mut widget,
            imported,
            &mut objects,
            &mut disclosures,
        )?;
        widget.insert(Name::from(b"P"), Object::Reference(page_id));
        widget.insert(Name::from(b"Rect"), rect_array(rect));
        let radio_on_state =
            self.claim_radio_state(id, name, &mut widget, &mut objects, &mut disclosures)?;
        let (merge_writes, widget_id) =
            self.merge_widget_into_field(id, shape, widget, page_id, slots)?;
        objects.extend(merge_writes);
        if !dr_fonts.is_empty() {
            objects.extend(self.acroform_write(&mut |af| insert_dr_fonts(af, &dr_fonts))?);
        }
        self.commit(Command {
            kind: CommandKind::PasteFormField,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        disclosures.push(
            "paste: this is another view of the SAME field -- it shares one value, so typing in either shows in both, and its font, colour, alignment, default value and actions are the original's exactly because the field object was not touched."
                .to_owned(),
        );
        Ok(FieldPasteOutcome {
            field_id: id,
            widget_ids: vec![widget_id],
            merged: false,
            created: false,
            disclosures,
            radio_on_state,
            materialized_keys: Vec::new(),
            dropped_keys,
            font_renames: font_renames(&da_renames),
        })
    }

    /// Install the font a pasted widget's own `/DA` names and rewrite that
    /// `/DA` on a rename: the field is untouched, but the widget's font must
    /// still resolve in this document's `/DR` (§12.7.3.3).
    #[allow(clippy::type_complexity)] // Two independent results.
    fn install_widget_da_font(
        &mut self,
        clip: &FieldClip,
        widget: &mut Dict,
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> Result<(Vec<(Vec<u8>, ObjId)>, BTreeMap<Vec<u8>, Vec<u8>>), EditError> {
        let Some(font) = own_da_font(widget) else {
            return Ok((Vec::new(), BTreeMap::new()));
        };
        let installed =
            self.install_pasted_da_fonts(clip, Some(&font), imported, objects, disclosures)?;
        rename_da(widget, &installed.1);
        Ok(installed)
    }

    /// Materialise one clip widget's dictionary into this session, removing
    /// its dropped-key markers into `dropped`. The flag is whether the source
    /// had an `/NM` ([`Self::name_pasted_widget`] owes it a fresh one).
    fn materialize_widget_dict(
        &mut self,
        clip: &FieldClip,
        index: usize,
        copy_actions: bool,
        imported: &mut BTreeMap<u32, ObjId>,
        objects: &mut Vec<ObjectWrite>,
        dropped: &mut BTreeMap<String, u64>,
    ) -> Result<(Dict, bool), EditError> {
        let Some(source) = clip.widgets().get(index) else {
            return Ok((Dict::new(), false));
        };
        let mut dict = self
            .clip_materialize_value(
                clip.objects(),
                &Object::Dict(source.dict().clone()),
                imported,
                objects,
            )?
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let renamed = take_markers(&mut dict, PASTE_DROPPED_WIDGET_KEYS, b"NM", dropped);
        if !copy_actions {
            dict.remove(b"A");
            dict.remove(b"AA");
        }
        Ok((dict, renamed))
    }
}

/// `/T`, `/Parent` and `/TU` for the new field: its identity is the paste's.
fn name_pasted_field(
    field: &mut Dict,
    partial: &str,
    parent: Option<ObjId>,
    tooltip: &TooltipChoice,
) {
    field.insert(
        Name::from(b"T"),
        Object::String(encode_text_string(partial)),
    );
    if let Some(p) = parent {
        field.insert(Name::from(b"Parent"), Object::Reference(p));
    }
    match tooltip.text() {
        Some(t) => {
            field.insert(Name::from(b"TU"), Object::String(encode_text_string(t)));
        }
        None => {
            field.remove(b"TU");
        }
    }
}

/// `/Type`, `/Subtype`, `/P` and `/Rect` every pasted widget gets.
fn mark_widget(widget: &mut Dict, page_id: ObjId, rect: Object) {
    widget.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
    widget.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Widget")));
    widget.insert(Name::from(b"P"), Object::Reference(page_id));
    widget.insert(Name::from(b"Rect"), rect);
}

/// Fold a widget half over its field for a merged paste. The widget wins
/// every key but `/AA`, whose trigger entries are merged (field triggers
/// and annotation triggers share one dictionary in a merged field, Table
/// 194/196); a trigger both define, or an `/AA` either holds by reference,
/// keeps the field's and is disclosed.
fn fold_widget_into_field(field: &mut Dict, widget: Dict, disclosures: &mut Vec<String>) {
    let mut lost = false;
    for (key, value) in widget.iter() {
        let Some(own) = field.get(b"AA").filter(|_| key.as_bytes() == b"AA") else {
            field.insert(key.clone(), value.clone());
            continue;
        };
        match (own, value) {
            (Object::Dict(own), Object::Dict(theirs)) => {
                let mut merged = own.clone();
                for (trigger, action) in theirs.iter() {
                    match merged.get(trigger.as_bytes()) {
                        None => {
                            merged.insert(trigger.clone(), action.clone());
                        }
                        Some(existing) => lost |= existing != action,
                    }
                }
                field.insert(Name::from(b"AA"), Object::Dict(merged));
            }
            _ => lost |= own != value,
        }
    }
    if lost {
        disclosures.push(
            "paste: the copied field and its widget both defined actions (/AA) for the same trigger; the pasted single-widget field holds one /AA, so the field's was kept and the widget's dropped."
                .to_owned(),
        );
    }
}

/// The inherited keys the new field carries, disclosed (§12.7.3.1: the
/// pasted field has no parent to inherit them from).
fn materialized_keys(clip: &FieldClip, field: &Dict, disclosures: &mut Vec<String>) -> Vec<String> {
    let keys: Vec<String> = clip
        .inherited_keys()
        .iter()
        .filter(|k| field.contains_key(k.as_bytes()))
        .cloned()
        .collect();
    if !keys.is_empty() {
        let names: Vec<String> = keys.iter().map(|k| format!("/{k}")).collect();
        disclosures.push(format!(
            "paste: the copied field inherited {} from a parent field or the form's defaults (ISO 32000-1 12.7.3.1), which did not travel, so the pasted field carries them itself. Changing the source's parent will not change this copy.",
            names.join(" ")
        ));
    }
    keys
}

/// The disclosure for [`FieldPasteOutcome::dropped_keys`]. A dropped
/// `/StructParent` in a tagged document is already disclosed by the
/// placement sentence.
pub(super) fn field_dropped_keys_disclosure(
    dropped: &BTreeMap<String, u64>,
    tagged: bool,
) -> Option<String> {
    let what = |key: &str| match key {
        "StructParent" if !tagged => Some("its structure-tree entry (/StructParent)"),
        "Popup" => Some("its pop-up window (/Popup)"),
        "IRT" => Some("its in-reply-to link (/IRT)"),
        _ => None,
    };
    let parts: Vec<String> = dropped
        .iter()
        .filter_map(|(k, n)| what(k).map(|w| format!("{n} lost {w}")))
        .collect();
    (!parts.is_empty()).then(|| {
        format!(
            "paste: pasted widgets named objects in the document they were copied from, which were not carried: {}",
            parts.join("; ")
        )
    })
}
