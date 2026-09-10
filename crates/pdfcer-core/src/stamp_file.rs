//! Acrobat-compatible **stamp collection files** (`Pass 288.0`).
//!
//! # What a custom stamp actually is
//!
//! Not a special format. A stamp **collection** is an ordinary PDF:
//!
//! * **one file per category**, **one page per stamp**;
//! * the **category name** is the file's `/Info` `/Title`;
//! * each stamp's names live in the document catalog's `/Names` → `/Pages`
//!   **name tree** (§7.7.4 Table 31 — "a name tree mapping name strings to
//!   *visible pages*"), as a single string `internal=display`;
//! * an internal name beginning `#` marks a **dynamic** stamp, whose text is
//!   recomputed when it is placed.
//!
//! "Exporting" a stamp is, at the file-format level, indistinguishable from
//! handing someone that PDF. There is no interchange format to implement.
//!
//! # ★★ Every claim above is MEASURED, not sourced from the internet
//!
//! The feature-parity research reached this shape from convergent community
//! sources and flagged two gaps by name: **where the category name is stored**
//! (`/Info` `/Title`? the filename? an Acrobat-side preference?) and whether
//! the `#` convention was real. Compatibility work built on secondary sourcing
//! is how a shipped feature silently fails to interoperate, so the answers
//! here come from **Adobe's own shipped stamp files** on this machine:
//!
//! ```text
//! …/Acrobat DC/Acrobat/plug_ins/Annotations/Stamps/ENU/StandardBusiness.pdf
//!   /Info  /Title (Standard Business)
//!   catalog /Names << /Pages 239 0 R >>
//!   239 0 obj << /Names [ (SBApproved=Approved) 244 0 R
//!                         (SBDraft=Draft)        18 0 R  … ] >>
//!
//! …/Stamps/ENU/Dynamic.pdf
//!   /Info  /Title (Dynamic)
//!   /Names [ (#DApproved=Approved) 29 0 R … ]      ← the `#` prefix, real
//!   /AcroForm << /CO [ … ] /Fields [ … ] >>        ← the dynamic machinery
//! ```
//!
//! Both gaps are closed by that: **the category is `/Info` `/Title`**, and the
//! `#` prefix is Adobe's own convention in Adobe's own file. `/PieceInfo`
//! appears in those files too and is a **red herring** — it carries
//! `/Illustrator` authoring data, nothing to do with stamps.
//!
//! # What pdfcer does and does not do here
//!
//! Reads a collection, and writes one. **Dynamic stamps are read and
//! reported, never authored**: their text comes from AcroForm calculation
//! JavaScript, and authoring a script that computes a date at placement time
//! is a different feature with a different risk profile
//! ([`StampEntry::dynamic`] says which is which, so a caller is never
//! surprised).

use crate::document::Document;
use crate::object::{Dict, Object};

/// One stamp inside a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StampEntry {
    /// The **internal** name — the half before `=`, Acrobat's stable
    /// identifier for the stamp, e.g. `SBApproved`.
    ///
    /// Adobe prefixes its own with a category tag (`SB` for Standard
    /// Business, `#D` for Dynamic). That is a convention, not a requirement:
    /// nothing reads the prefix except the `#` test below.
    pub internal: String,
    /// The **display** name — the half after `=`, what a picker shows, e.g.
    /// `Approved`. Empty when the stored name had no `=` at all.
    pub display: String,
    /// Whether the internal name begins `#`, which marks a stamp whose text
    /// Acrobat recomputes at placement time from AcroForm calculation
    /// scripts.
    ///
    /// ★ pdfcer **reports** this and does not author it. A dynamic stamp
    /// placed by pdfcer would carry whatever its page already draws, which is
    /// the design-time text — correct as a picture, wrong as a promise.
    pub dynamic: bool,
    /// 0-based index of the page this stamp is drawn on, when the named page
    /// is one of the document's own.
    pub page_index: Option<usize>,
}

/// A stamp collection file: one category, its stamps in name-tree order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StampCollection {
    /// The category name — the file's `/Info` `/Title`.
    ///
    /// `None` when the file has no title, which Acrobat's own files always
    /// do. A collection without one is readable but would show up unnamed.
    pub category: Option<String>,
    /// The stamps, in the order the name tree lists them.
    pub stamps: Vec<StampEntry>,
}

impl StampCollection {
    /// Whether this looks like a stamp collection at all.
    ///
    /// The test is the **name tree**, not the title: a PDF with a `/Title` and
    /// no named pages is just a PDF, while Adobe's own files are recognisable
    /// by their named pages alone.
    #[must_use]
    pub fn is_stamp_file(&self) -> bool {
        !self.stamps.is_empty()
    }
}

/// Read a stamp collection out of an open document.
///
/// Returns a collection with no stamps when the document has no `/Names` →
/// `/Pages` tree — which is the honest answer for an ordinary PDF, and is why
/// this returns a value rather than an error.
#[must_use]
pub fn read(doc: &Document) -> StampCollection {
    let category = doc
        .trailer()
        .get(b"Info")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|info| info.get(b"Title").map(|o| doc.resolve(o)))
        .and_then(|o| match o {
            Object::String(s) => Some(crate::edit::decode_text_string(s).text),
            _ => None,
        });

    let mut stamps = Vec::new();
    if let Ok(catalog) = doc.catalog()
        && let Some(names) = catalog
            .get(b"Names")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        && let Some(pages) = names
            .get(b"Pages")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
    {
        let page_ids: Vec<_> = crate::page_tree::pages(doc)
            .map(|ps| ps.iter().map(|p| p.id).collect())
            .unwrap_or_default();
        collect(doc, pages, &page_ids, &mut stamps, 0);
    }

    StampCollection { category, stamps }
}

/// Walk one node of the `/Pages` name tree, following `/Kids` (§7.9.6).
///
/// `depth` is bounded because a malformed file may point a `/Kids` entry back
/// at an ancestor, and a stamp file is not worth hanging on. The limit is
/// generous relative to any real collection: Adobe's largest ships twelve
/// stamps in one flat node.
fn collect(
    doc: &Document,
    node: &Dict,
    page_ids: &[crate::object::ObjId],
    out: &mut Vec<StampEntry>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }

    if let Some(Object::Array(names)) = node.get(b"Names").map(|o| doc.resolve(o)) {
        // The array alternates name string, then the value it names.
        for pair in names.chunks(2) {
            let [Object::String(raw), value] = pair else {
                continue;
            };
            let stored = crate::edit::decode_text_string(raw).text;
            let (internal, display) = match stored.split_once('=') {
                Some((i, d)) => (i.to_owned(), d.to_owned()),
                // No `=` at all: the whole string is the identifier and there
                // is no separate display name. Reported rather than repaired.
                None => (stored.clone(), String::new()),
            };
            let page_index = value
                .as_reference()
                .and_then(|id| page_ids.iter().position(|p| *p == id));
            out.push(StampEntry {
                dynamic: internal.starts_with('#'),
                internal,
                display,
                page_index,
            });
        }
    }

    if let Some(Object::Array(kids)) = node.get(b"Kids").map(|o| doc.resolve(o)) {
        for kid in kids {
            if let Some(kid) = doc.resolve(kid).as_dict() {
                collect(doc, kid, page_ids, out, depth + 1);
            }
        }
    }
}

/// The name-tree string for a stamp, in Acrobat's `internal=display` form.
///
/// A free function because the reader splits on `=` and the writer joins on
/// it, and a format whose two halves are written in two places is a format
/// that drifts.
#[must_use]
pub fn stamp_name_string(internal: &str, display: &str) -> String {
    if display.is_empty() {
        internal.to_owned()
    } else {
        format!("{internal}={display}")
    }
}

/// Name the pages of an open session so the document becomes a stamp
/// collection (`Pass 288.0`).
///
/// `stamps[i]` names page `i` as `(internal, display)`. The category name is
/// the file's `/Info` `/Title` — set it with
/// [`crate::edit::EditSession::set_info_field`]; this verb owns the name tree
/// only, because a `/Title` is an ordinary metadata edit.
///
/// Entries past the last page are **skipped and reported** rather than
/// written: a name tree pointing at nothing is a stamp that appears in a
/// picker and then draws no page.
///
/// # Errors
///
/// [`crate::edit::EditError`] as [`crate::edit::EditSession::set_named_pages`].
///
/// # ★ Why this does not draw the stamps
///
/// A stamp's artwork **is a page**, and pdfcer already has every verb for
/// authoring pages. A function that also drew the artwork would be a second,
/// worse page-authoring API existing only here. The caller builds the pages
/// however it likes — imported, drawn, or an existing document's — and this
/// names them.
pub fn name_stamp_pages(
    session: &mut crate::edit::EditSession,
    stamps: &[(String, String)],
) -> Result<CollectionWritten, crate::edit::EditError> {
    let page_ids: Vec<crate::object::ObjId> = crate::page_tree::pages_in(&session.graph())
        .map(|ps| ps.iter().map(|p| p.id).collect())
        .unwrap_or_default();

    let mut pairs: Vec<(Vec<u8>, Object)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for (index, (internal, display)) in stamps.iter().enumerate() {
        match page_ids.get(index) {
            Some(id) => pairs.push((
                stamp_name_string(internal, display).into_bytes(),
                Object::Reference(*id),
            )),
            None => skipped.push(stamp_name_string(internal, display)),
        }
    }

    // ★ §7.9.6: a name tree's entries "shall be ordered lexicographically by
    // name". Acrobat's own files obey it — `StandardBusiness.pdf` lists
    // SBApproved (page 0) then SBCompleted (page 4) — so page order is NOT
    // tree order, and emitting page order would produce a tree a conforming
    // reader may binary-search wrongly.
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let named = pairs.len();
    let flat: Vec<Object> = pairs
        .into_iter()
        .flat_map(|(n, v)| [Object::String(n), v])
        .collect();

    session.set_named_pages(flat)?;

    Ok(CollectionWritten {
        stamps_named: named,
        skipped,
    })
}

/// What [`name_stamp_pages`] did — disclosed, never assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollectionWritten {
    /// How many stamps were named.
    pub stamps_named: usize,
    /// Stamps that named a page the document does not have, skipped rather
    /// than written as a name pointing at nothing.
    pub skipped: Vec<String>,
}
