//! Step 4: drop form XObjects wholly outside the region and inline the ones
//! crossing its edge, because redaction cuts page content but not the
//! inside of a form.
//!
//! An inlined form is the §8.10.1 Do procedure written out:
//! `q <Matrix> cm <BBox> re W n <content> Q`, with the form's own resource
//! names prefixed and merged into a direct page `/Resources`. A form drawn
//! from another inlined form becomes page-level content in the next pass,
//! bounded by [`MAX_PASSES`].

use crate::content::{ContentStream, ContentTokenKind};
use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::{self, Page, Rect};
use crate::span::ByteSpan;
use crate::vector::{Bounds, ImageSource, Matrix, VectorObject, decompose_page};
use crate::view::DocumentView;
use crate::writer::{DirtySet, SaveOptions, save_full};

use super::{PageOpError, RegionError, RegionReport};

/// Form nesting depth inlined before the rest is left as it is.
const MAX_PASSES: usize = 32;

/// Resource categories whose names appear as content-stream operands.
const CATEGORIES: [&[u8]; 7] = [
    b"Font",
    b"XObject",
    b"ExtGState",
    b"ColorSpace",
    b"Pattern",
    b"Shading",
    b"Properties",
];

/// Inline or drop forms until none crosses the edge, returning the saved
/// bytes. Leaves `doc` unchanged in substance when nothing needed doing.
pub(super) fn inline_forms(
    mut doc: Document,
    rect: Rect,
    report: &mut RegionReport,
) -> Result<Vec<u8>, RegionError> {
    for pass in 0..MAX_PASSES {
        match one_pass(&doc, rect, pass, report)? {
            Some(bytes) => doc = Document::from_bytes(bytes)?,
            None => return Ok(doc.bytes().to_vec()),
        }
    }
    report.notes.push(format!(
        "forms nested more than {MAX_PASSES} deep were not inlined; content outside the region may \
         survive inside them"
    ));
    Ok(doc.bytes().to_vec())
}

/// One page-level sweep. `None` when there was nothing to drop or inline,
/// which is when the forms still crossing the edge are counted.
fn one_pass(
    doc: &Document,
    rect: Rect,
    pass: usize,
    report: &mut RegionReport,
) -> Result<Option<Vec<u8>>, RegionError> {
    let pages = page_tree::pages(doc).map_err(PageOpError::from)?;
    let Some(page) = pages.first() else {
        return Ok(None);
    };
    let view = doc.view();
    let objects = decompose_page(&view, page, Matrix::IDENTITY)?;
    let content = ContentStream::from_page(&view, page)?;
    let mut resources = page.resources.clone();
    let mut edits: Vec<(ByteSpan, Vec<u8>)> = Vec::new();
    let mut refused: Vec<&'static str> = Vec::new();
    for form in objects.objects.iter().filter_map(|o| match o {
        VectorObject::Image(i) if i.source == ImageSource::Form => Some(i),
        _ => None,
    }) {
        match placement(form.page_bbox, rect) {
            Placement::Inside => {}
            Placement::Outside => {
                edits.push((form.bytes, Vec::new()));
                report.forms_dropped += 1;
            }
            Placement::Straddles => {
                let prefix = unique_prefix(&view, &resources, pass, edits.len());
                let inlined = form
                    .xobject
                    .ok_or("it is a direct stream with no object of its own")
                    .and_then(|id| inline_one(&view, id, &prefix, &mut resources));
                match inlined {
                    Ok(bytes) => {
                        edits.push((form.bytes, bytes));
                        report.forms_inlined += 1;
                    }
                    Err(why) => refused.push(why),
                }
            }
        }
    }
    if edits.is_empty() {
        report.forms_kept_straddling = refused.len();
        report.notes.extend(refused.iter().map(|why| {
            format!("a form crossing the region's edge was not inlined because {why}; its content outside the region survives")
        }));
        return Ok(None);
    }
    let spliced = splice(&content.buf, &mut edits);
    prune_xobjects(&view, &mut resources, spliced.clone())?;
    write(doc, page, spliced, resources).map(Some)
}

/// Drop `/XObject` entries the new content no longer draws, so a dropped
/// form's stream is unreachable and the final copy does not write it.
fn prune_xobjects(
    view: &DocumentView<'_>,
    resources: &mut Dict,
    content: Vec<u8>,
) -> Result<(), RegionError> {
    let Some(xobjects) = resources
        .get(b"XObject")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return Ok(());
    };
    let parsed = ContentStream::parse(content)?;
    let drawn: Vec<&[u8]> = parsed
        .tokens
        .iter()
        .zip(parsed.tokens.iter().skip(1))
        .filter(|(_, op)| op.span.slice(&parsed.buf) == Some(&b"Do"[..]))
        .filter_map(|(operand, _)| match &operand.kind {
            ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes()),
            _ => None,
        })
        .collect();
    let mut kept = Dict::default();
    for (name, value) in xobjects.iter() {
        if drawn.contains(&name.as_bytes()) {
            kept.insert(name.clone(), value.clone());
        }
    }
    resources.insert(Name::from(b"XObject"), Object::Dict(kept));
    Ok(())
}

enum Placement {
    Inside,
    Outside,
    Straddles,
}

/// Where a form's transformed `/BBox` sits. A non-finite box is left alone.
fn placement(b: Bounds, rect: Rect) -> Placement {
    let finite = [b.min.x, b.min.y, b.max.x, b.max.y]
        .iter()
        .all(|v| v.is_finite());
    if !finite {
        return Placement::Inside;
    }
    if b.max.x <= rect.llx || b.min.x >= rect.urx || b.max.y <= rect.lly || b.min.y >= rect.ury {
        Placement::Outside
    } else if b.min.x >= rect.llx
        && b.max.x <= rect.urx
        && b.min.y >= rect.lly
        && b.max.y <= rect.ury
    {
        Placement::Inside
    } else {
        Placement::Straddles
    }
}

/// A name prefix no existing resource name starts with.
fn unique_prefix(view: &DocumentView<'_>, resources: &Dict, pass: usize, k: usize) -> Vec<u8> {
    let taken = |prefix: &[u8]| {
        CATEGORIES.iter().any(|cat| {
            resources
                .get(cat)
                .map(|o| view.graph().resolve(o))
                .and_then(Object::as_dict)
                .is_some_and(|d| d.iter().any(|(n, _)| n.as_bytes().starts_with(prefix)))
        })
    };
    let mut n = k;
    loop {
        let prefix = format!("Rg{pass}x{n}_").into_bytes();
        if !taken(&prefix) {
            return prefix;
        }
        n += 1;
    }
}

/// The inlined bytes for form `id`, merging its resources into `resources`
/// under `prefix`. `Err` names why inlining would change what is drawn.
fn inline_one(
    view: &DocumentView<'_>,
    id: ObjId,
    prefix: &[u8],
    resources: &mut Dict,
) -> Result<Vec<u8>, &'static str> {
    let graph = view.graph();
    let Some(Object::Stream(Stream { dict, .. })) = graph.value(id) else {
        return Err("it is not a stream");
    };
    if dict.get(b"Group").is_some() {
        return Err("it is a transparency group, which composites as a unit");
    }
    if dict.get(b"OC").is_some() {
        return Err("it belongs to optional content outside the default configuration");
    }
    let bbox = dict
        .get(b"BBox")
        .and_then(|o| page_tree::parse_rect(graph, o, "BBox").ok())
        .ok_or("it has no usable /BBox")?;
    let matrix = form_matrix(graph, dict.get(b"Matrix"));
    let content = ContentStream::from_form(view, id).map_err(|_| "its content does not parse")?;
    let own = dict
        .get(b"Resources")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict);
    let body = rewrite(&content, own.map(|d| (d, prefix)), graph)?;
    if let Some(own) = own {
        merge(graph, own, prefix, resources);
    }
    let [a, b, c, d, e, f] = matrix.map(num);
    let mut out = format!(
        "q {a} {b} {c} {d} {e} {f} cm {} {} {} {} re W n\n",
        num(bbox.llx),
        num(bbox.lly),
        num(bbox.width()),
        num(bbox.height())
    )
    .into_bytes();
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\nQ");
    Ok(out)
}

fn form_matrix(graph: &dyn ObjectGraph, matrix: Option<&Object>) -> [f64; 6] {
    let values: Vec<f64> = matrix
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| graph.resolve(v).as_number())
                .collect()
        })
        .unwrap_or_default();
    match <[f64; 6]>::try_from(values) {
        Ok(m) if m.iter().all(|v| v.is_finite()) => m,
        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

/// A content-stream number: integral values without a fraction, others to
/// six places with trailing zeros dropped.
fn num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The form's content with its own resource names prefixed, balanced so it
/// cannot leak graphics, text or marked-content state into the page.
fn rewrite(
    content: &ContentStream,
    own: Option<(&Dict, &[u8])>,
    graph: &dyn ObjectGraph,
) -> Result<Vec<u8>, &'static str> {
    let buf = &content.buf;
    let mut renames: Vec<usize> = Vec::new();
    let (mut q, mut bt, mut mc) = (0usize, 0usize, 0usize);
    let mut run_start = 0usize;
    for (i, tok) in content.tokens.iter().enumerate() {
        match &tok.kind {
            ContentTokenKind::Operand(_) => continue,
            ContentTokenKind::InlineImage { params, .. } => {
                if own.is_some() && named_colour_space(params) {
                    return Err("an inline image in it names a colour space resource");
                }
            }
            _ => {
                let op = buf
                    .get(tok.span.start..tok.span.start + tok.span.len)
                    .unwrap_or(&[]);
                match op {
                    b"q" => q += 1,
                    b"Q" => q = q.checked_sub(1).ok_or("its q/Q operators are unbalanced")?,
                    b"BT" => bt += 1,
                    b"ET" => {
                        bt = bt
                            .checked_sub(1)
                            .ok_or("its BT/ET operators are unbalanced")?
                    }
                    b"BMC" | b"BDC" => mc += 1,
                    b"EMC" => {
                        mc = mc
                            .checked_sub(1)
                            .ok_or("its marked content is unbalanced")?
                    }
                    _ => {}
                }
                if let Some((own, _)) = own
                    && let Some(at) = rename_target(content, op, run_start, i, own, graph)
                {
                    renames.push(at);
                }
            }
        }
        run_start = i + 1;
    }
    if bt != 0 {
        return Err("it ends inside a text object");
    }
    let prefix = own.map_or(&[][..], |(_, p)| p);
    let mut edits: Vec<(ByteSpan, Vec<u8>)> = renames
        .iter()
        .filter_map(|&at| content.tokens.get(at))
        .map(|tok| {
            let name = buf
                .get(tok.span.start + 1..tok.span.start + tok.span.len)
                .unwrap_or(&[]);
            let mut new = vec![b'/'];
            new.extend_from_slice(prefix);
            new.extend_from_slice(name);
            (tok.span, new)
        })
        .collect();
    let mut out = splice(buf, &mut edits);
    out.extend(std::iter::repeat_n(&b"\nEMC"[..], mc).flatten());
    out.extend(std::iter::repeat_n(&b"\nQ"[..], q).flatten());
    Ok(out)
}

/// Whether an inline image's `/ColorSpace` is a resource name rather than a
/// device space (§8.9.7: a name other than a device space is looked up in
/// the `/ColorSpace` resources).
fn named_colour_space(params: &Dict) -> bool {
    params
        .get(b"ColorSpace")
        .and_then(Object::as_name)
        .is_some_and(|n| !is_device_space(n.as_bytes()))
}

fn is_device_space(name: &[u8]) -> bool {
    matches!(
        name,
        b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern"
    )
}

/// The token index of the resource-name operand of operator `op` (tokens
/// `start..op_at` are its operands), if that name is in the form's own
/// resources.
fn rename_target(
    content: &ContentStream,
    op: &[u8],
    start: usize,
    op_at: usize,
    own: &Dict,
    graph: &dyn ObjectGraph,
) -> Option<usize> {
    let last = op_at.checked_sub(1).filter(|&l| l >= start);
    let (at, category): (Option<usize>, &[u8]) = match op {
        b"Tf" => (Some(start).filter(|&s| s < op_at), b"Font"),
        b"Do" => (last, b"XObject"),
        b"gs" => (last, b"ExtGState"),
        b"cs" | b"CS" => (last, b"ColorSpace"),
        b"scn" | b"SCN" => (last, b"Pattern"),
        b"sh" => (last, b"Shading"),
        b"BDC" | b"DP" => (Some(start + 1).filter(|&s| s < op_at), b"Properties"),
        _ => return None,
    };
    let at = at?;
    let ContentTokenKind::Operand(Object::Name(name)) = &content.tokens.get(at)?.kind else {
        return None;
    };
    if category == b"ColorSpace" && is_device_space(name.as_bytes()) {
        return None;
    }
    own.get(category)
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(name.as_bytes()))
        .map(|_| at)
}

/// Copy the form's resources into the page's under `prefix`.
fn merge(graph: &dyn ObjectGraph, own: &Dict, prefix: &[u8], resources: &mut Dict) {
    for category in CATEGORIES {
        let Some(entries) = own
            .get(category)
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        let mut target = resources
            .get(category)
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();
        for (name, value) in entries.iter() {
            let mut key = prefix.to_vec();
            key.extend_from_slice(name.as_bytes());
            target.insert(Name(key), value.clone());
        }
        resources.insert(Name::from(category), Object::Dict(target));
    }
}

/// `buf` with each span replaced. Spans must not overlap.
fn splice(buf: &[u8], edits: &mut [(ByteSpan, Vec<u8>)]) -> Vec<u8> {
    edits.sort_by_key(|(span, _)| span.start);
    let mut out = Vec::with_capacity(buf.len());
    let mut at = 0usize;
    for (span, bytes) in edits.iter() {
        if span.start < at {
            continue;
        }
        out.extend_from_slice(buf.get(at..span.start).unwrap_or(&[]));
        out.push(b'\n');
        out.extend_from_slice(bytes);
        out.push(b'\n');
        at = span.start + span.len;
    }
    out.extend_from_slice(buf.get(at..).unwrap_or(&[]));
    out
}

/// Save `doc` with the page's content replaced by `content` (one new
/// uncompressed stream) and its resources by `resources`, made direct.
fn write(
    doc: &Document,
    page: &Page,
    content: Vec<u8>,
    resources: Dict,
) -> Result<Vec<u8>, RegionError> {
    let Some(page_dict) = doc.value(page.id).and_then(Object::as_dict) else {
        return Err(PageOpError::NoPages.into());
    };
    let Some(number) = doc.next_object_number() else {
        return Err(RegionError::NoObjectNumber);
    };
    let content_id = ObjId::new(number, 0);
    let mut page_dict = page_dict.clone();
    page_dict.insert(Name::from(b"Contents"), Object::Reference(content_id));
    page_dict.insert(Name::from(b"Resources"), Object::Dict(resources));
    let len = content.len();
    let span = ByteSpan {
        start: doc.bytes().len(),
        len,
    };
    // bypass-exempt: writes a scratch revision of a one-page copy that only
    // `extract_region` reads; no editing session or operator file is involved.
    let mut dirty = DirtySet::empty();
    dirty.replace(
        content_id,
        crate::text_edit::edit::make_raw_stream(span, len),
    );
    dirty.replace(page.id, Object::Dict(page_dict));
    // bypass-exempt: as above, the staged bytes are the new content stream.
    dirty.set_staging(content);
    // bypass-exempt: as above.
    Ok(save_full(doc, &dirty, &SaveOptions::identity())?.0)
}
