//! Role classification: a breadth-first walk from the document's roots.
//!
//! An object takes the first role that reaches it, in seed order: the page
//! tree, then annotations and thumbnails (deferred so page content claims a
//! shared resource first), then the rest of the catalog, then the trailer.
//! Cycles are handled by the visited set; nesting of direct values is
//! bounded by [`MAX_DIRECT_DEPTH`].

use std::collections::{BTreeMap, VecDeque};

use super::{SpaceCategory as C, dict_of, name_of};
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};

/// Direct (non-indirect) nesting depth past which a value is not scanned.
const MAX_DIRECT_DEPTH: usize = 64;

#[derive(Default)]
struct Walk {
    roles: BTreeMap<ObjId, C>,
    primary: VecDeque<(ObjId, C)>,
    deferred: VecDeque<(ObjId, C)>,
}

/// Every object's role: the first seed to reach it, else unreferenced.
pub(super) fn classify(doc: &Document) -> BTreeMap<ObjId, C> {
    let mut walk = Walk::default();
    let trailer = doc.trailer();
    let catalog = trailer.get(b"Root").and_then(Object::as_reference);
    if let Some(pages) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Pages"))
        .and_then(Object::as_reference)
    {
        walk.primary.push_back((pages, C::PageStructure));
    }
    walk.drain(doc);
    if let Some(root) = catalog {
        walk.primary.push_back((root, C::PageStructure));
    }
    walk.drain(doc);
    for (key, value) in trailer.iter() {
        let role = match key.as_bytes() {
            b"Info" => C::Metadata,
            b"Encrypt" => C::FileOverhead,
            _ => C::OtherReferenced,
        };
        walk.scan(value, role, 0);
    }
    walk.drain(doc);
    for id in linearization_machinery(doc) {
        walk.primary.push_back((id, C::FileOverhead));
    }
    walk.drain(doc);
    for obj in doc.objects() {
        walk.roles.entry(obj.id).or_insert_with(|| {
            dict_of(&obj.value)
                .and_then(container_role)
                .unwrap_or(C::Unreferenced)
        });
    }
    walk.roles
}

impl Walk {
    fn drain(&mut self, doc: &Document) {
        loop {
            let next = self
                .primary
                .pop_front()
                .or_else(|| self.deferred.pop_front());
            let Some((id, inherited)) = next else {
                return;
            };
            if self.roles.contains_key(&id) {
                continue;
            }
            let Some(obj) = doc.get(id) else {
                continue;
            };
            let role =
                dict_of(&obj.value).map_or(inherited, |d| intrinsic(d, &obj.value, inherited));
            self.roles.insert(id, role);
            self.scan(&obj.value, role, 0);
        }
    }

    /// Queue every reference inside `value`, with the role its key implies.
    fn scan(&mut self, value: &Object, role: C, depth: usize) {
        if depth > MAX_DIRECT_DEPTH {
            return;
        }
        match value {
            Object::Reference(id) => self.primary.push_back((*id, role)),
            Object::Array(items) => {
                for item in items {
                    self.scan(item, role, depth + 1);
                }
            }
            Object::Dict(_) | Object::Stream(_) => {
                let Some(dict) = dict_of(value) else {
                    return;
                };
                for (key, child) in dict.iter() {
                    let key = key.as_bytes();
                    if key == b"Parent" || key == b"P" {
                        continue;
                    }
                    if key == b"Annots" || key == b"Thumb" {
                        let child_role = if key == b"Thumb" {
                            C::Thumbnails
                        } else {
                            C::Annotations
                        };
                        self.defer(child, child_role, depth + 1);
                    } else {
                        self.scan(child, key_role(key, role), depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn defer(&mut self, value: &Object, role: C, depth: usize) {
        let before = self.primary.len();
        self.scan(value, role, depth);
        let queued: Vec<_> = self.primary.drain(before..).collect();
        self.deferred.extend(queued);
    }
}

/// The role a dictionary key gives the values under it.
fn key_role(key: &[u8], role: C) -> C {
    match key {
        b"Font" | b"FontDescriptor" | b"DescendantFonts" | b"ToUnicode" | b"CIDToGIDMap"
        | b"CIDSet" | b"FontFile" | b"FontFile2" | b"FontFile3" | b"CharProcs" => C::Fonts,
        b"ExtGState" => C::GraphicsStates,
        b"ColorSpace" | b"OutputIntents" | b"DestOutputProfile" => C::ColorSpaces,
        b"Pattern" | b"Shading" => C::PatternsAndShadings,
        b"Contents" if role == C::PageStructure => C::ContentStreams,
        b"Metadata" => C::Metadata,
        b"Outlines" => C::Bookmarks,
        b"Dests" => C::NamedDestinations,
        b"AcroForm" => C::Forms,
        b"StructTreeRoot" => C::StructureTree,
        b"OCProperties" => C::Layers,
        b"EmbeddedFiles" | b"AF" => C::EmbeddedFiles,
        b"JavaScript" | b"JS" => C::JavaScript,
        b"Names" | b"OpenAction" | b"AA" | b"ViewerPreferences" | b"PageLabels" => {
            C::OtherReferenced
        }
        _ => role,
    }
}

/// The role an object has whatever reached it, else the inherited one.
fn intrinsic(dict: &Dict, value: &Object, inherited: C) -> C {
    if let Some(role) = container_role(dict) {
        return role;
    }
    let ty = name_of(dict, b"Type");
    let sub = name_of(dict, b"Subtype");
    let is_stream = matches!(value, Object::Stream(_));
    match (ty.as_deref(), sub.as_deref()) {
        (_, Some("Image")) if is_stream => C::Images,
        (_, Some("Form")) if is_stream => match inherited {
            C::Annotations | C::Forms | C::Links | C::PatternsAndShadings | C::Fonts => inherited,
            _ => C::FormXObjects,
        },
        (Some("Font" | "FontDescriptor"), _) => C::Fonts,
        (Some("Metadata"), _) => C::Metadata,
        (Some("EmbeddedFile"), _) => C::EmbeddedFiles,
        (Some("Page" | "Pages" | "Catalog"), _) => C::PageStructure,
        (Some("ExtGState"), _) => C::GraphicsStates,
        (Some("Pattern"), _) => C::PatternsAndShadings,
        (Some("OCG" | "OCMD"), _) => C::Layers,
        (Some("StructTreeRoot" | "StructElem"), _) => C::StructureTree,
        (Some("Outlines"), _) => C::Bookmarks,
        (Some("Annot"), _) | (None, Some(_)) if inherited == C::Annotations => {
            annot_role(sub.as_deref())
        }
        _ => inherited,
    }
}

fn annot_role(subtype: Option<&str>) -> C {
    match subtype {
        Some("Link") => C::Links,
        Some("Widget") => C::Forms,
        _ => C::Annotations,
    }
}

/// The linearization dictionary and the hint streams its `/H` points at
/// (ISO 32000-2 Annex F.2: each `/H` offset is the start of the stream
/// object). Nothing references them, so without this they read as
/// unreferenced.
fn linearization_machinery(doc: &Document) -> Vec<ObjId> {
    let Some(lin) = doc
        .objects()
        .find(|o| dict_of(&o.value).is_some_and(|d| d.get(b"Linearized").is_some()))
    else {
        return Vec::new();
    };
    let offsets: Vec<usize> = dict_of(&lin.value)
        .and_then(|d| d.get(b"H"))
        .and_then(Object::as_array)
        .map(|h| {
            h.iter()
                .step_by(2)
                .filter_map(Object::as_int)
                .filter_map(|n| usize::try_from(n).ok())
                .collect()
        })
        .unwrap_or_default();
    let mut ids = vec![lin.id];
    ids.extend(
        doc.objects()
            .filter(|o| o.file_span().is_some_and(|s| offsets.contains(&s.start)))
            .map(|o| o.id),
    );
    ids
}

/// Objects that are file machinery rather than content.
fn container_role(dict: &Dict) -> Option<C> {
    match name_of(dict, b"Type").as_deref() {
        Some("XRef") => Some(C::CrossReference),
        Some("ObjStm") => Some(C::FileOverhead),
        _ => None,
    }
}
