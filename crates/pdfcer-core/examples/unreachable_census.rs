//! Census probe: how many objects in a real PDF does the document graph never
//! reach, and do any of them carry text?
//!
//! # Why this exists
//!
//! `redact::apply_redactions` finds carriers by **navigating the document
//! graph** — the trailer's `/Info`, the catalog's `/Metadata`, the pages'
//! content streams. `writer::save_full` emits objects by **enumerating the
//! cross-reference table** (`doc.xref().iter()`). Those two sets are not the
//! same set, and every object in the difference is re-emitted verbatim into a
//! redacted file without ever having been offered to a carrier pass.
//!
//! Before choosing a remedy, this measures whether the difference is a
//! curiosity or a population: run it over a directory of real files and it
//! reports, per file, how many objects the xref names, how many a
//! reachability walk from the trailer actually reaches, and how many of the
//! unreachable ones carry strings or decodable stream text.
//!
//! ```text
//! cargo run --release -p pdfcer-core --example unreachable_census -- <dir> [more...]
//! ```
//!
//! It only reads. Nothing is written, and no file is modified.

use pdfcer_core::document::Document;
use pdfcer_core::object::{Dict, ObjId, Object};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Collect every indirect reference appearing anywhere inside one object.
///
/// Deliberately structural rather than semantic: it does not know what any
/// key means, so it cannot be wrong about a key it has never heard of. A
/// reachability walk built on a table of known keys would under-count exactly
/// the exotic files this census exists to find.
fn refs_in(obj: &Object, out: &mut Vec<ObjId>) {
    match obj {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => {
            for item in items {
                refs_in(item, out);
            }
        }
        Object::Dict(d) => refs_in_dict(d, out),
        Object::Stream(s) => refs_in_dict(&s.dict, out),
        _ => {}
    }
}

/// The dictionary half of [`refs_in`], split out because a stream reaches its
/// references through its dictionary and nothing else.
fn refs_in_dict(d: &Dict, out: &mut Vec<ObjId>) {
    for (_, v) in d.iter() {
        refs_in(v, out);
    }
}

/// Every object id reachable from the trailer by following references.
///
/// # ★ `UO-A1` — reachability is NOT "walk the references from `/Root`"
///
/// Two object classes are unreferenced **by design**, and a walk that knows
/// only indirect references miscounts every one of them as an orphan:
///
/// * **Object streams** (§7.5.7). A compressed object is reached through a
///   **type-2 cross-reference entry**, not through a `243 0 R` — the clause
///   says so in terms: an object stream shall have an xref entry *"although
///   there might not be any references to it (of the form `243 0 R`)"*.
/// * **The linearization dictionary** (Annex F.3.3), which is unreferenced by
///   a `shall`: *"There shall be no references to this dictionary anywhere in
///   the document; however, the first-page cross-reference table (part 3)
///   shall contain a normal entry for it."*
///
/// So the seed set is the trailer's closure **plus** every `/ObjStm` a type-2
/// entry names, and the linearization dictionary is excluded from the orphan
/// count by its `/Linearized` key rather than by reference.
fn reachable(doc: &Document) -> BTreeSet<ObjId> {
    let mut seen: BTreeSet<ObjId> = BTreeSet::new();
    let mut stack: Vec<ObjId> = Vec::new();
    let mut roots: Vec<ObjId> = Vec::new();
    refs_in_dict(doc.trailer(), &mut roots);

    // Every container a type-2 entry names is live, however nothing points
    // at it. Without this the sweep reports every object stream in the file.
    for (_, entry) in doc.xref().iter() {
        if let pdfcer_core::xref::XrefEntry::InStream { stream_num, .. } = entry {
            roots.push(ObjId::new(stream_num, 0));
        }
    }
    stack.extend(roots);

    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(io) = doc.get(id) {
            let mut more = Vec::new();
            refs_in(&io.value, &mut more);
            stack.extend(more);
        }
    }
    seen
}

/// Whether an object could carry **drawn text** — the only kind of content a
/// redaction is asked to remove from a page.
///
/// ★ This is the decision-relevant metric, and it is deliberately narrower
/// than [`carries_text`]. An abandoned content stream full of path operators
/// has long printable runs and carries no words; a stream with a `Tj` may
/// carry the very glyphs the operator redacted. Counting the first as a leak
/// risk overstates the problem, which the first cut of this census did.
fn carries_drawn_text(doc: &Document, obj: &Object) -> bool {
    match obj {
        Object::String(s) => !s.is_empty(),
        Object::Dict(d) => d
            .iter()
            .any(|(_, v)| matches!(v, Object::String(s) if !s.is_empty())),
        Object::Stream(st) => {
            if st
                .dict
                .iter()
                .any(|(_, v)| matches!(v, Object::String(s) if !s.is_empty()))
            {
                return true;
            }
            let Some(raw) = st.data_span.slice(doc.bytes()) else {
                return false;
            };
            let data =
                pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap_or_else(|_| raw.to_vec());
            // A show operator is a token, so require a delimiter before it —
            // otherwise `BT` inside a name or a number matches.
            has_show_operator(&data)
        }
        _ => false,
    }
}

/// Whether decoded content-stream bytes contain a text-showing operator
/// (§9.4.3: `Tj`, `TJ`, `'`, `"`), token-aligned.
fn has_show_operator(data: &[u8]) -> bool {
    let is_delim = |b: u8| b.is_ascii_whitespace() || b"[]<>(){}/%".contains(&b);
    for (i, w) in data.windows(2).enumerate() {
        let before_ok = i == 0 || is_delim(data[i - 1]);
        if !before_ok {
            continue;
        }
        if w == b"Tj" || w == b"TJ" {
            let after = data.get(i + 2).copied();
            if after.is_none_or(is_delim) {
                return true;
            }
        }
    }
    // `'` and `"` are single-character operators and always token-aligned.
    data.iter().any(|b| *b == b'\'' || *b == b'"')
}

/// A short printable sample of whatever text an object holds, for `--dump`.
fn text_sample(doc: &Document, obj: &Object) -> String {
    fn clip(bytes: &[u8]) -> String {
        let text: String = bytes
            .iter()
            .take(160)
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        text
    }
    match obj {
        Object::String(s) => format!("string {:?}", clip(s)),
        Object::Dict(d) => {
            let parts: Vec<String> = d
                .iter()
                .filter_map(|(k, v)| match v {
                    Object::String(s) => Some(format!(
                        "/{}={:?}",
                        String::from_utf8_lossy(k.as_bytes()),
                        clip(s)
                    )),
                    _ => None,
                })
                .collect();
            format!("dict {}", parts.join(" "))
        }
        Object::Stream(st) => {
            let raw = st.data_span.slice(doc.bytes()).unwrap_or(&[]);
            let data =
                pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap_or_else(|_| raw.to_vec());
            format!("stream({} B) {:?}", data.len(), clip(&data))
        }
        _ => "-".to_string(),
    }
}

fn census(path: &Path) {
    let Ok(doc) = Document::load(path) else {
        println!("  SKIP (does not open)  {}", path.display());
        return;
    };
    let in_xref: BTreeSet<ObjId> = doc.objects().map(|io| io.id).collect();
    let live = reachable(&doc);
    let orphans: Vec<ObjId> = in_xref
        .iter()
        .copied()
        .filter(|id| !live.contains(id))
        // Two more classes that are unreferenced BY DESIGN and are therefore
        // not orphans in any sense this census means:
        //
        // * the linearization dictionary — Annex F.3.3 makes it unreferenced
        //   by a `shall`;
        // * cross-reference streams (`/Type /XRef`) — reached from
        //   `startxref` and `/Prev` by BYTE OFFSET, never by an indirect
        //   reference (§7.5.8).
        //
        // Counting either as a leak is the same `UO-A1` error as counting
        // object streams, and the first cut of this census made it.
        .filter(|id| {
            !doc.get(*id).is_some_and(|io| match &io.value {
                Object::Dict(d) => d.get(b"Linearized").is_some(),
                Object::Stream(st) => {
                    matches!(st.dict.get(b"Type"), Some(Object::Name(n)) if n.as_bytes() == b"XRef")
                }
                _ => false,
            })
        })
        .collect();

    // What each orphan actually IS, so a class that is unreferenced by design
    // cannot be silently counted as a leak.
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for id in &orphans {
        let kind = doc.get(*id).map_or_else(
            || "missing".to_string(),
            |io| match &io.value {
                Object::Stream(st) => match st.dict.get(b"Type") {
                    Some(Object::Name(n)) => {
                        format!("stream/{}", String::from_utf8_lossy(n.as_bytes()))
                    }
                    _ => "stream-untyped".to_string(),
                },
                Object::Dict(d) => match d.get(b"Type") {
                    Some(Object::Name(n)) => {
                        format!("dict/{}", String::from_utf8_lossy(n.as_bytes()))
                    }
                    _ => "dict-untyped".to_string(),
                },
                Object::String(_) => "string".to_string(),
                _ => "other".to_string(),
            },
        );
        *kinds.entry(kind).or_default() += 1;
    }
    let kind_list: Vec<String> = kinds.iter().map(|(k, n)| format!("{k}x{n}")).collect();

    // With `--dump`, show a sample of what each text-carrying orphan holds.
    // Knowing an orphan exists sizes the population; knowing what is IN one
    // is what says whether the population matters.
    if std::env::args().any(|a| a == "--dump") {
        for id in &orphans {
            let Some(io) = doc.get(*id) else { continue };
            if !carries_drawn_text(&doc, &io.value) {
                continue;
            }
            let sample = text_sample(&doc, &io.value);
            println!("      {id:?} -> {sample}");
        }
    }
    let with_text = orphans
        .iter()
        .filter(|id| {
            doc.get(**id)
                .is_some_and(|io| carries_drawn_text(&doc, &io.value))
        })
        .count();

    println!(
        "  {:>5} in xref  {:>5} reachable  {:>4} ORPHAN  {:>4} with-text  [{}]  {}",
        in_xref.len(),
        live.len(),
        orphans.len(),
        with_text,
        kind_list.join(" "),
        path.file_name().unwrap_or_default().to_string_lossy()
    );
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--dump").collect();
    if args.is_empty() {
        eprintln!("usage: unreachable_census <dir-or-file> [more...]");
        return;
    }
    let mut files = Vec::new();
    for a in &args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            collect(&p, &mut files);
        } else {
            files.push(p);
        }
    }
    files.sort();
    println!("{} file(s)", files.len());
    for f in &files {
        census(f);
    }
}
