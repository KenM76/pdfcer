//! Decision 175's retype: the run's show operators are removed from the
//! content stream and the new run text is set in their place, at the first
//! operator's text matrix, size, colour and spacing (ISO 32000-1 §9.3,
//! §9.4.2), in the run's own font when it encodes the text, else in the
//! fallback face. The text after the run is held in place by a compensating
//! `TJ` number (§9.4.3, §9.4.4).

use std::collections::BTreeSet;

use crate::object::Dict;
use crate::text_edit::edit::{
    EditLayout, EditPlan, EditRequest, EncodedReplacement, FollowerDisposition, FontClass,
    FontWrites, OpRec, PlanMode, Rec, ShowData, ShowElem, ShowOp, advance_before, carried_codes,
    classify_font, compensating_tj, edit_report, emit_show, encode_in, find_anchor, glyph_advance,
    is_subset_tag, resolve_font_dict, same_line, splice, target_disclosures, trust_disclosure,
    writes_vertically,
};
use crate::text_edit::fallback::{self, Fallback, FallbackFace, RunAt};
use crate::text_edit::workaround::Planning;
use crate::text_extract::font::ExtractFont;
use crate::writer::content::emit_number;

/// The face a retype uses when [`EditOptions::fallback`] names none.
///
/// [`EditOptions::fallback`]: crate::text_edit::EditOptions::fallback
pub(crate) const DEFAULT_FACE: &str = "Helvetica";

/// The text of every show operator, joined in content order.
pub(crate) struct Joined {
    text: String,
    /// Per show operator with text: (record index, first byte, byte length).
    ops: Vec<(usize, usize, usize)>,
    /// Per byte: the text object (counted by `ET`) it is drawn in.
    object_of: Vec<usize>,
}

impl Joined {
    /// Join `recs`' show-operator text, recording which operator and which
    /// text object drew each byte.
    pub(crate) fn of(recs: &[OpRec]) -> Self {
        let (mut text, mut ops, mut object_of) = (String::new(), Vec::new(), Vec::new());
        let mut object = 0;
        for (i, r) in recs.iter().enumerate() {
            match &r.rec {
                Rec::Show(s) if !s.text.is_empty() => {
                    ops.push((i, text.len(), s.text.len()));
                    text.push_str(&s.text);
                    object_of.resize(text.len(), object);
                }
                Rec::EndText => object += 1,
                _ => {}
            }
        }
        Self {
            text,
            ops,
            object_of,
        }
    }

    /// The first match of a non-empty `find` as a byte range; with `from`,
    /// the first that starts inside that record's text.
    pub(crate) fn find(&self, find: &str, from: Option<usize>) -> Option<(usize, usize)> {
        if find.is_empty() {
            return None;
        }
        let (start, limit) = match from {
            Some(rec) => {
                let &(_, start, len) = self.ops.iter().find(|o| o.0 == rec)?;
                (start, start + len)
            }
            None => (0, self.text.len()),
        };
        let pos = start + self.text.get(start..)?.find(find)?;
        (pos < limit).then_some((pos, pos + find.len()))
    }

    /// The record index of the show operator drawing byte `pos`.
    pub(crate) fn op_at(&self, pos: usize) -> Option<usize> {
        self.ops
            .iter()
            .find(|&&(_, start, len)| (start..start + len).contains(&pos))
            .map(|o| o.0)
    }

    /// The text object (counted from 0 by `ET`) drawing byte `pos`.
    pub(crate) fn object_at(&self, pos: usize) -> Option<usize> {
        self.object_of.get(pos).copied()
    }

    /// The record indices of the show operators drawing `pos..end`.
    pub(crate) fn ops_in(&self, pos: usize, end: usize) -> Vec<usize> {
        self.ops
            .iter()
            .filter(|&&(_, start, len)| start < end && pos < start + len)
            .map(|o| o.0)
            .collect()
    }

    /// The byte range of record `rec`'s text.
    fn range_of(&self, rec: usize) -> Option<(usize, usize)> {
        self.ops
            .iter()
            .find(|o| o.0 == rec)
            .map(|&(_, start, len)| (start, start + len))
    }
}

/// The run a retype replaces: its show operators (record indices) and its
/// old and new text.
struct Selection {
    ops: Vec<usize>,
    old: String,
    new: String,
}

fn select(recs: &[OpRec], req: &EditRequest) -> Result<Selection, String> {
    let joined = Joined::of(recs);
    let pinned = match req.pinned_span {
        Some(_) => Some(find_anchor(recs, req).map_err(|e| e.to_string())?),
        None => None,
    };
    if let (Some(i), true) = (pinned, req.find.is_empty()) {
        let Some(Rec::Show(s)) = recs.get(i).map(|r| &r.rec) else {
            return Err("the pin names no show operator".to_owned());
        };
        return Ok(Selection {
            ops: vec![i],
            old: s.text.clone(),
            new: req.replace.clone(),
        });
    }
    let (pos, end) = joined
        .find(&req.find, pinned)
        .ok_or_else(|| "the find text is not in the run".to_owned())?;
    let (first, last) = (joined.op_at(pos), joined.op_at(end - 1));
    let (Some(first), Some(last)) = (first, last) else {
        return Err("the match is drawn by no show operator".to_owned());
    };
    let (run_start, _) = joined.range_of(first).unwrap_or((pos, end));
    let (_, run_end) = joined.range_of(last).unwrap_or((pos, end));
    let piece = |a: usize, b: usize| joined.text.get(a..b).unwrap_or("").to_owned();
    let (before, after) = (piece(run_start, pos), piece(end, run_end));
    if before.contains('\u{FFFD}') || after.contains('\u{FFFD}') {
        return Err(
            "the run holds a glyph with no known character, which a retype would lose".to_owned(),
        );
    }
    let ops = (first..=last)
        .filter(|&k| matches!(recs.get(k).map(|r| &r.rec), Some(Rec::Show(_))))
        .collect();
    Ok(Selection {
        ops,
        old: piece(run_start, run_end),
        new: format!("{before}{}{after}", req.replace),
    })
}

fn show(recs: &[OpRec], i: usize) -> Option<&ShowData> {
    match recs.get(i).map(|r| &r.rec) {
        Some(Rec::Show(s)) => Some(s),
        _ => None,
    }
}

/// Whether only ignorable records lie strictly between records `a` and `b`,
/// so `b` starts where `a` ended (§9.4.2: only `Tm`, `Td`, `TD`, `T*` and
/// the quote operators move the line matrix).
fn continues(recs: &[OpRec], a: usize, b: usize) -> bool {
    recs.get(a + 1..b)
        .is_some_and(|between| between.iter().all(|r| matches!(r.rec, Rec::Ignore)))
}

/// The run's operators after the first must continue its line: same CTM,
/// `Tj`/`TJ`, and either drawn on from the one before or on the first's
/// baseline. An operator with bytes but no text would draw something the
/// retype cannot carry.
fn check_run(recs: &[OpRec], ops: &[usize]) -> Result<(), String> {
    let head = ops.first().and_then(|&i| show(recs, i)).ok_or("no run")?;
    if !head.matrix_known {
        return Err("the run's position is not known (an unresolved font before it)".to_owned());
    }
    for pair in ops.windows(2) {
        let (&[prev, k], Some(s)) = (pair, pair.get(1).and_then(|&k| show(recs, k))) else {
            continue;
        };
        let same_ctm = head
            .ctm
            .iter()
            .zip(&s.ctm)
            .all(|(a, b)| (a - b).abs() < 1e-6);
        let on_line = continues(recs, prev, k) || same_line(head, &s.text_matrix);
        if !same_ctm || !on_line || !matches!(s.op, ShowOp::Tj | ShowOp::TJ) {
            return Err("the run's operators do not continue one line".to_owned());
        }
        if s.text.is_empty()
            && s.elems
                .iter()
                .any(|e| matches!(e, ShowElem::Str(b) if !b.is_empty()))
        {
            return Err("an operator in the run draws glyphs with no known characters".to_owned());
        }
    }
    Ok(())
}

/// The run's own font, when a retype may set text in it.
struct Own<'a> {
    dict: &'a Dict,
    font: ExtractFont,
    class: FontClass,
    vertical: bool,
    /// The edit gate classified the font; text may be set in it.
    usable: bool,
}

fn own_font<'a>(p: &Planning<'a>, head: &ShowData) -> Option<Own<'a>> {
    let dict = resolve_font_dict(p.doc, &p.target.resources, &head.font_name)?;
    let font = ExtractFont::resolve(p.doc, dict);
    let class = classify_font(p.doc, dict, &font).ok();
    let vertical = writes_vertically(p.doc, dict);
    Some(Own {
        dict,
        // `classify_font` refuses a vertical font, so it is never usable.
        usable: class.is_some(),
        class: class.unwrap_or(FontClass {
            embedded: false,
            subset: is_subset_tag(&font.base_font),
        }),
        font,
        vertical,
    })
}

/// `text` in the run's font, held to the codes a subset already carries.
fn encode_own(own: &Own<'_>, recs: &[OpRec], key: &[u8], text: &str) -> Option<EncodedReplacement> {
    if !own.usable {
        return None;
    }
    let e = encode_in(&own.font, &BTreeSet::new(), text).ok()?;
    let carried = own.class.subset.then(|| carried_codes(recs, key));
    let held = carried.is_none_or(|c| e.codes.iter().all(|code| c.contains(code)));
    held.then_some(e)
}

/// How the new text is set.
enum Setting {
    Own(EncodedReplacement),
    Face(Box<Fallback>),
}

fn choose(
    p: &Planning<'_>,
    own: Option<&Own<'_>>,
    head: &ShowData,
    text: &str,
) -> Result<Setting, String> {
    let key = head.font_name.as_slice();
    if let Some(e) = own.and_then(|o| encode_own(o, p.recs, key, text)) {
        return Ok(Setting::Own(e));
    }
    let mut to_face: BTreeSet<char> = text.chars().collect();
    if let Some(o) = own {
        to_face.retain(|c| encode_own(o, p.recs, key, c.encode_utf8(&mut [0; 4])).is_none());
    }
    let own_text: String = text.chars().filter(|c| !to_face.contains(c)).collect();
    let own_codes = match own.filter(|_| !own_text.is_empty()) {
        Some(o) => encode_own(o, p.recs, key, &own_text),
        None => Some(EncodedReplacement::default()),
    };
    let own_codes = own_codes.unwrap_or_else(|| {
        to_face = text.chars().collect();
        EncodedReplacement::default()
    });
    let default_face = FallbackFace::Named(DEFAULT_FACE.to_owned());
    let face = p.opts.fallback.unwrap_or(&default_face);
    let empty = Dict::default();
    let at = RunAt {
        doc: p.doc,
        resources: &p.target.resources,
        recs: p.recs,
        own_dict: own.map_or(&empty, |o| o.dict),
        anchor: head,
    };
    fallback::retype_split(&at, face, text, &to_face, &own_codes)
        .map(|fb| Setting::Face(Box::new(fb)))
}

/// An operator's whole advance (§9.4.4), when it can be measured: a
/// horizontal font that resolves and decodes every byte the operator shows.
fn old_advance(p: &Planning<'_>, s: &ShowData) -> Option<f64> {
    let dict = resolve_font_dict(p.doc, &p.target.resources, &s.font_name)?;
    if writes_vertically(p.doc, dict) {
        return None;
    }
    let shown: usize = s
        .elems
        .iter()
        .map(|e| match e {
            ShowElem::Str(b) => b.len(),
            ShowElem::Num(_) => 0,
        })
        .sum();
    let decoded: usize = s.slots.iter().map(|sl| usize::from(sl.width)).sum();
    let font = ExtractFont::resolve(p.doc, dict);
    (shown == decoded).then(|| advance_before(&font, s, s.elems.len(), 0))
}

/// The replacement bytes for each of the run's operators.
struct Emitted {
    edits: Vec<(usize, usize, Vec<u8>)>,
    held: bool,
}

/// `ops[0]` becomes `body` (behind the `T*` a quote operator implies, with
/// `"`'s spacing); the others are removed. When a `Tj`/`TJ` continues from
/// the run's end, the last operator carries the `TJ` number that puts it
/// back where it was.
fn emit(p: &Planning<'_>, ops: &[usize], body: Vec<u8>, new_adv: f64) -> Result<Emitted, String> {
    let mut edits = Vec::new();
    let mut drift = Some(0.0);
    for (n, &k) in ops.iter().enumerate() {
        let (Some(r), Some(s)) = (p.recs.get(k), show(p.recs, k)) else {
            continue;
        };
        if n > 0
            && ops
                .get(n - 1)
                .is_some_and(|&prev| !continues(p.recs, prev, k))
        {
            drift = Some(0.0);
        }
        let added = if n == 0 { new_adv } else { 0.0 };
        drift = drift.zip(old_advance(p, s)).map(|(d, old)| d + added - old);
        let bytes = if n == 0 {
            quote_prefix(s, &body)
        } else {
            Vec::new()
        };
        edits.push((r.start, r.end, bytes));
    }
    let (Some(&last), Some((_, _, tail))) = (ops.last(), edits.last_mut()) else {
        return Err("no run".to_owned());
    };
    let followed = p
        .recs
        .iter()
        .skip(last + 1)
        .find(|r| !matches!(r.rec, Rec::Ignore))
        .is_some_and(|r| matches!(&r.rec, Rec::Show(s) if matches!(s.op, ShowOp::Tj | ShowOp::TJ)));
    let mut held = false;
    if followed {
        let s = show(p.recs, last).ok_or("no run")?;
        let d = drift.ok_or("the text after the run continues from its end, and the run's old advance cannot be measured")?;
        if d.abs() > 1e-9 {
            let n = compensating_tj(d, s.tf_size, s.th()).ok_or("the run has zero size")?;
            if !tail.is_empty() {
                tail.push(b' ');
            }
            tail.push(b'[');
            emit_number(tail, n);
            tail.extend_from_slice(b"] TJ");
            held = true;
        }
    }
    Ok(Emitted { edits, held })
}

fn quote_prefix(s: &ShowData, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if s.op == ShowOp::DoubleQuote {
        emit_number(&mut out, s.tw());
        out.extend_from_slice(b" Tw ");
        emit_number(&mut out, s.tc());
        out.extend_from_slice(b" Tc ");
    }
    if matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote) {
        out.extend_from_slice(b"T* ");
    }
    out.extend_from_slice(body);
    out
}

/// Plan the retype of the run `req` names.
///
/// # Errors
///
/// Why the run cannot be retyped.
pub(crate) fn plan(p: &Planning<'_>, req: &EditRequest) -> Result<EditPlan, String> {
    let sel = select(p.recs, req)?;
    check_run(p.recs, &sel.ops)?;
    let head = sel
        .ops
        .first()
        .and_then(|&i| show(p.recs, i))
        .ok_or("no run")?;
    let own = own_font(p, head);
    let setting = choose(p, own.as_ref(), head, &sel.new)?;
    let (body, new_adv, layout) = set(&setting, own.as_ref(), head, &sel.new);
    let emitted = emit(p, &sel.ops, body, new_adv)?;
    let old_adv: Option<f64> = sel
        .ops
        .iter()
        .map(|&k| show(p.recs, k).and_then(|s| old_advance(p, s)))
        .sum();
    let mut disclosures = vec![statement(&sel, emitted.held)];
    let base_font = own.as_ref().map_or_else(
        || String::from_utf8_lossy(&head.font_name).into_owned(),
        |o| o.font.base_font.clone(),
    );
    let class = own.as_ref().map_or(
        FontClass {
            embedded: false,
            subset: false,
        },
        |o| FontClass {
            embedded: o.class.embedded,
            subset: o.class.subset,
        },
    );
    disclosures.push(match &setting {
        Setting::Own(_) => trust_disclosure(class.embedded, &base_font),
        Setting::Face(fb) => fb.disclosure(&base_font),
    });
    if own.as_ref().is_some_and(|o| o.vertical) {
        disclosures.push(VERTICAL_NOTE.to_owned());
    }
    disclosures.push(SAVE_NOTE.to_owned());
    target_disclosures(p.doc, p.target, head, &mut disclosures);
    let moved = (
        old_adv.map_or(0.0, |old| new_adv - old),
        0,
        sel.ops.len() as u64,
    );
    let mut report = edit_report(
        p.target,
        &base_font,
        &class,
        p.opts,
        moved,
        head,
        disclosures,
    );
    report.disposition = FollowerDisposition::Pin;
    let mut emitted = emitted;
    let (fallback_use, created_font) = match setting {
        Setting::Face(fb) => (Some(fb.used.clone()), fb.into_created()),
        Setting::Own(_) => (None, None),
    };
    report.fallback = fallback_use;
    Ok(EditPlan {
        new_content: match p.mode {
            PlanMode::Commit => splice(&p.stream.buf, &mut emitted.edits),
            PlanMode::Preview => Vec::new(),
        },
        report,
        layout,
        font_writes: FontWrites::default(),
        rewritten: None,
        font_program: None,
        created_font,
    })
}

/// The body that replaces the first operator, its advance, and the layout.
fn set(
    setting: &Setting,
    own: Option<&Own<'_>>,
    head: &ShowData,
    text: &str,
) -> (Vec<u8>, f64, EditLayout) {
    match setting {
        Setting::Own(e) => {
            let Some(o) = own else {
                return (
                    Vec::new(),
                    0.0,
                    EditLayout::placed(
                        head,
                        &Dict::default(),
                        "",
                        std::iter::empty(),
                        0.0,
                        (0.0, 0.0),
                    ),
                );
            };
            let chars: Vec<char> = text.chars().collect();
            let per_char = chars.len() == e.codes.len();
            let items = e.codes.iter().enumerate().map(|(i, &code)| {
                let ch = chars.get(i).copied().filter(|_| per_char);
                (ch, code, glyph_advance(&o.font, code, head))
            });
            let advance: f64 = e
                .codes
                .iter()
                .map(|&c| glyph_advance(&o.font, c, head))
                .sum();
            let metrics = (f64::from(o.font.ascent()), f64::from(o.font.descent()));
            let layout = EditLayout::placed(head, o.dict, &o.font.base_font, items, 0.0, metrics);
            (
                emit_show(&[ShowElem::Str(e.bytes.clone())]),
                advance,
                layout,
            )
        }
        Setting::Face(fb) => {
            let own_font = own.map_or_else(|| fb.face().clone(), |o| o.font.clone());
            let empty = Dict::default();
            let own_dict = own.map_or(&empty, |o| o.dict);
            let layout = fb.layout(head, own_dict, &own_font, 0.0);
            let body = fb.emit_run(&head.font_name, head.tf_size);
            (body, fb.advance(&own_font, head), layout)
        }
    }
}

/// Rule 4: what the retype removed and what it set.
fn statement(sel: &Selection, held: bool) -> String {
    let held = if held {
        "; the text after the run is held where it was by a compensating TJ"
    } else {
        ""
    };
    format!(
        "retype: the {} show operator(s) drawing {:?} were removed from the content stream and \
         {:?} was set in their place at the first operator's origin, size, colour and spacing; \
         the original kerning and per-glyph positioning were not kept{held}",
        sel.ops.len(),
        sel.old,
        sel.new
    )
}

const VERTICAL_NOTE: &str = "retype: the run's font writes vertically (WMode 1); the retyped \
     text is set horizontally from the run's origin, in the fallback face";

const SAVE_NOTE: &str = "save: the removed show operators are gone from the edited content \
     stream, but an incremental save keeps the prior revision, which still holds them; to \
     remove the old text from the file, redact it or save with a full rewrite";
