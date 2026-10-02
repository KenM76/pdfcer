//! Decision 174: set the characters a run's font cannot carry in another font
//! resource on the same page that names the same face, switching to it with
//! `Tf` for the replacement only (ISO 32000-1 §9.3.1, §9.4.3).

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::edit::{MatchRun, ShowData, ShowElem, ShowOp, emit_show};
use crate::view::DocumentView;
use crate::writer::IdentityEncoder;
use crate::writer::content::emit_number;
use crate::writer::serialize::write_object;

/// Strip a §9.6.4 subset tag (`ABCDEF+Times-Bold` -> `Times-Bold`).
pub(crate) fn subset_stem(base: &str) -> &str {
    match base.split_once('+') {
        Some((tag, rest)) if crate::text_edit::edit::is_subset_tag(base) && !tag.is_empty() => rest,
        _ => base,
    }
}

/// The `/Font` resources other than `own_dict` whose `/BaseFont`, subset tag
/// stripped, equals `base_font`'s, in resource-key order.
pub(crate) fn candidates<'a>(
    doc: &'a DocumentView<'a>,
    resources: &'a Dict,
    own_dict: &Dict,
    base_font: &str,
) -> Vec<(Vec<u8>, &'a Dict)> {
    let stem = subset_stem(base_font);
    let Some(fonts) = resources
        .get(b"Font")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    else {
        return Vec::new();
    };
    let mut out: Vec<(Vec<u8>, &Dict)> = fonts
        .iter()
        .filter_map(|(k, v)| Some((k.as_bytes().to_vec(), doc.resolve(v).as_dict()?)))
        .filter(|(_, d)| !std::ptr::eq(*d, own_dict) && *d != own_dict)
        .filter(|(_, d)| {
            d.get(b"BaseFont")
                .and_then(|o| doc.resolve(o).as_name())
                .is_some_and(|n| subset_stem(&String::from_utf8_lossy(n.as_bytes())) == stem)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Whether the edit can switch fonts mid-operator: one `Tj`/`TJ` holds the
/// whole match (a quote operator's line move would be split from its text).
pub(crate) fn splittable(anchor: &ShowData, single_operator: bool) -> bool {
    single_operator && matches!(anchor.op, ShowOp::Tj | ShowOp::TJ)
}

/// Append `/<name> <size> Tf`, the name escaped per §7.3.5.
pub(crate) fn push_tf(out: &mut Vec<u8>, name: &[u8], size: f64) {
    if !out.is_empty() {
        out.push(b' ');
    }
    let name = Object::Name(Name::from(name));
    write_object(out, &name, ObjId::new(0, 0), &[], &IdentityEncoder);
    out.push(b' ');
    emit_number(out, size);
    out.extend_from_slice(b" Tf");
}

/// The anchor operator rewritten as: the text before the match in the run's
/// own font, `/<sibling> Tf`, the replacement, `/<own> Tf`, then the text
/// after the match and `pin_num`. Font size is the run's own `Tf` size.
pub(crate) fn emit_switched_operator(
    anchor: &ShowData,
    m: &MatchRun,
    new_codes: &[u8],
    pin_num: Option<f64>,
    sibling: &[u8],
) -> Vec<u8> {
    emit_segmented_operator(anchor, m, &[(true, new_codes.to_vec())], pin_num, sibling)
}

/// The anchor operator with its match replaced by `segments`, each
/// `(in_other, bytes)`: a run of segments in `other` is preceded by
/// `/<other> Tf` and the run's own font is restored with `/<own> Tf` before
/// the next own segment and after the last. Own segments at either end join
/// the text around the match. Every `Tf` carries the run's own size, so the
/// baseline, `Tc`, `Tw`, `Tz` and `Ts` are untouched (§9.3.1).
pub(crate) fn emit_segmented_operator(
    anchor: &ShowData,
    m: &MatchRun,
    segments: &[(bool, Vec<u8>)],
    pin_num: Option<f64>,
    other: &[u8],
) -> Vec<u8> {
    let (mut pre, mut post) = split_around(anchor, m);
    let mut mid = segments;
    if let [(false, head), rest @ ..] = mid {
        match pre.last_mut() {
            Some(ShowElem::Str(s)) => s.extend_from_slice(head),
            _ => pre.push(ShowElem::Str(head.clone())),
        }
        mid = rest;
    }
    if let [rest @ .., (false, tail)] = mid {
        match post.first_mut() {
            Some(ShowElem::Str(s)) => s.splice(0..0, tail.iter().copied()).for_each(drop),
            _ => post.insert(0, ShowElem::Str(tail.clone())),
        }
        mid = rest;
    }
    post.extend(pin_num.map(ShowElem::Num));
    let mut out = emit_show(&pre);
    for (in_other, bytes) in mid {
        let font = if *in_other { other } else { &anchor.font_name };
        push_tf(&mut out, font, anchor.tf_size);
        out.push(b' ');
        out.extend(emit_show(&[ShowElem::Str(bytes.clone())]));
    }
    push_tf(&mut out, &anchor.font_name, anchor.tf_size);
    let rest = emit_show(&post);
    if !rest.is_empty() {
        out.push(b' ');
        out.extend(rest);
    }
    out
}

/// The anchor's elements before and after the match `m`.
fn split_around(anchor: &ShowData, m: &MatchRun) -> (Vec<ShowElem>, Vec<ShowElem>) {
    let mut pre = Vec::new();
    let mut post = Vec::new();
    for (i, e) in anchor.elems.iter().enumerate() {
        match (i.cmp(&m.elem), i.cmp(&m.elem_hi), e) {
            (std::cmp::Ordering::Less, _, e) => pre.push(e.clone()),
            (_, std::cmp::Ordering::Greater, e) => post.push(e.clone()),
            (std::cmp::Ordering::Equal, _, ShowElem::Str(b)) => {
                let head = b.get(..m.b_lo).unwrap_or(&[]);
                if !head.is_empty() {
                    pre.push(ShowElem::Str(head.to_vec()));
                }
            }
            _ => {}
        }
        if i == m.elem_hi
            && let ShowElem::Str(b) = e
        {
            let tail = b.get(m.b_hi..).unwrap_or(&[]);
            if !tail.is_empty() {
                post.push(ShowElem::Str(tail.to_vec()));
            }
        }
    }
    (pre, post)
}
