//! Underline and strikethrough that belong to the text they decorate.
//!
//! PDF has no decoration operator (ISO 32000-1 §9.4), so a decorated run is
//! two pieces of ordinary content tied together by marked content (§14.6):
//!
//! - a **marker** inside the `BT` around the decorated show operator,
//!   `/pdfc_Deco <</Line /Underline /Id 1>> BDC (text) Tj EMC`;
//! - a **rule** right after the closing `ET`, a filled rectangle per line,
//!   `/pdfc_Deco <</Rule 1>> BDC q a b c d e f cm 0 g x y w h re f Q EMC`.
//!
//! The marker is the source of truth; every rule is derived from the
//! marker's glyphs by [`refresh`], which the edit session runs after every
//! command that rewrites a page's or a form XObject's content. A moved
//! run's rule moves, a deleted run's rule goes, a reflowed run gets one rule
//! per line. Inside a form the rule is written into the form's own stream,
//! so every invocation of a shared form draws it under its own copy of the
//! text.
//!
//! Marker property list keys: `/Line` (a name, or an array of names, from
//! `/Underline` and `/StrikeOut`), `/Id` (integer), optional `/C` (fill
//! colour components: 1 gray, 3 RGB, 4 CMYK), `/W` (rule thickness in
//! thousandths of an em) and `/M /Standard` ([`DecorationMetrics::Standard`];
//! absent means [`DecorationMetrics::FontTables`]). Spec note:
//! `PDF_Spec/iso32000/iso32000__ref__text_decoration.md`.

use crate::content::{ContentStream, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::page_tree::Page;
use crate::span::ByteSpan;
use crate::text_extract::{ContentStreamRef, ExtractedGlyph, GlyphProvenance, TextColor};
use crate::view::DocumentView;
use crate::writer::content::emit_number;

mod metrics;
pub(crate) mod tagged;
use metrics::LineMetrics;

/// The marked-content tag of both the marker and its rule.
pub const DECORATION_TAG: &[u8] = b"pdfc_Deco";

/// Which lines decorate a run. Underline and strikethrough combine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DecorationSet {
    /// A line under the baseline.
    pub underline: bool,
    /// A line through the middle of the lower-case letters.
    pub strikethrough: bool,
}

impl DecorationSet {
    /// No decoration; formatting a run with it clears its lines.
    pub const NONE: Self = Self {
        underline: false,
        strikethrough: false,
    };
    /// Underline only.
    pub const UNDERLINE: Self = Self {
        underline: true,
        strikethrough: false,
    };
    /// Strikethrough only.
    pub const STRIKETHROUGH: Self = Self {
        underline: false,
        strikethrough: true,
    };

    /// Whether no line is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.underline && !self.strikethrough
    }

    /// `self` with the underline set to `on`.
    #[must_use]
    pub const fn with_underline(mut self, on: bool) -> Self {
        self.underline = on;
        self
    }

    /// `self` with the strikethrough set to `on`.
    #[must_use]
    pub const fn with_strikethrough(mut self, on: bool) -> Self {
        self.strikethrough = on;
        self
    }
}

/// Where a decoration's position and thickness come from. Stored on the
/// marker, so every later refresh draws the rule the same way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecorationMetrics {
    /// The embedded program's own metrics: `post` underline and `OS/2`
    /// strikeout (what a word processor uses), each falling back to
    /// [`Self::Standard`] when the font does not carry it.
    #[default]
    FontTables,
    /// Fixed typographic metrics for every font: the standard-14 AFM
    /// underline (0.1 em below the baseline, 0.05 em thick) and a strikeout
    /// at half the x-height, else a quarter em.
    Standard,
}

/// Where a strikethrough's centre line comes from, most specific first.
/// Reported so the shell can disclose it (rule 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StrikeSource {
    /// The embedded program's `OS/2` `yStrikeoutPosition` and
    /// `yStrikeoutSize`.
    FontTable,
    /// Half the font descriptor's `/XHeight` (§9.8.1 Table 120), or, for an
    /// unembedded standard-14 font without one, its AFM `XHeight`.
    XHeight,
    /// A quarter of an em: the font declares no x-height.
    QuarterEm,
}

/// The operator-facing sentence for a decoration request (rule 4: the
/// strikethrough height is inferred from font metrics).
pub(crate) fn disclosure(set: DecorationSet, metrics: DecorationMetrics) -> String {
    let mut out = match (set.underline, set.strikethrough) {
        (false, false) => {
            return "underline and strikethrough removed from the matched text".to_owned();
        }
        (true, false) => "underlined".to_owned(),
        (false, true) => "struck through".to_owned(),
        (true, true) => "underlined and struck through".to_owned(),
    };
    out.push_str("; the line follows the text through later moves, deletes and reflows");
    out.push_str(match metrics {
        DecorationMetrics::FontTables => {
            "; position and thickness come from the embedded font's own underline and \
             strikeout metrics where it has them, else the standard underline (0.1 em \
             below the baseline, 0.05 em thick) and a strikethrough at half the x-height \
             or a quarter em"
        }
        _ => {
            "; at the standard underline position (0.1 em below the baseline, 0.05 em \
             thick), the strikethrough at half the font's x-height or a quarter em"
        }
    });
    out
}

/// One decorated stretch of content: a marker's body and its lines.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DecoratedSpan {
    /// The marker's `/Id`.
    pub id: u32,
    /// The lines it carries.
    pub set: DecorationSet,
    /// Bytes between the marker's `BDC` and `EMC`, in the decoded content
    /// buffer it was read from (the page's, or the form's in
    /// [`PageDecorations::forms`]).
    pub body: ByteSpan,
}

/// The decorations on a page, read from its content.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PageDecorations {
    /// Every marker on the page's own content, in stream order.
    pub spans: Vec<DecoratedSpan>,
    /// Every marker in each form XObject the page paints, keyed by the
    /// form stream's object number.
    pub forms: std::collections::BTreeMap<u32, Vec<DecoratedSpan>>,
}

impl PageDecorations {
    /// The lines on the glyph `provenance` describes; empty when the glyph
    /// is undecorated.
    #[must_use]
    pub fn of(&self, provenance: &GlyphProvenance) -> DecorationSet {
        let spans = match provenance.content_stream {
            ContentStreamRef::Page => &self.spans,
            ContentStreamRef::Form { object } => match self.forms.get(&object) {
                Some(spans) => spans,
                None => return DecorationSet::NONE,
            },
        };
        let op = provenance.operator_span;
        spans
            .iter()
            .find(|s| op.start >= s.body.start && op.start + op.len <= s.body.start + s.body.len)
            .map_or(DecorationSet::NONE, |s| s.set)
    }
}

/// Read the decorations on `page`'s own content and in every form XObject
/// it paints. A form whose content does not decode contributes nothing.
///
/// # Errors
///
/// [`crate::content::ContentError`] when the page content does not decode.
pub fn page_decorations(
    view: &DocumentView<'_>,
    page: &Page,
) -> Result<PageDecorations, crate::content::ContentError> {
    let cs = ContentStream::from_page(view, page)?;
    let mut forms = std::collections::BTreeMap::new();
    for form in super::forms::scan_page_forms(view, page).forms {
        if forms.contains_key(&form.id.num) {
            continue;
        }
        if let Ok(fcs) = ContentStream::from_form(view, form.id) {
            forms.insert(form.id.num, spans_of(&fcs));
        }
    }
    Ok(PageDecorations {
        spans: spans_of(&cs),
        forms,
    })
}

fn spans_of(cs: &ContentStream) -> Vec<DecoratedSpan> {
    Scan::of(cs)
        .markers
        .iter()
        .map(|m| DecoratedSpan {
            id: m.id,
            set: m.set,
            body: ByteSpan::new(m.bdc.1, m.emc.0.saturating_sub(m.bdc.1)),
        })
        .collect()
}

/// A marker as written: its `BDC` (operand start, operator end) and `EMC`
/// (start, end) byte ranges.
#[derive(Debug, Clone)]
pub(crate) struct Marker {
    pub(crate) id: u32,
    pub(crate) set: DecorationSet,
    color: Option<Vec<f64>>,
    width: Option<f64>,
    metrics: DecorationMetrics,
    pub(crate) bdc: (usize, usize),
    pub(crate) emc: (usize, usize),
    /// End of the first `ET` after the marker, where its rules go.
    et_end: Option<usize>,
    /// Whether a marked-content sequence with `/MCID`, or an `Artifact`,
    /// is still open at that `ET`, so a rule there is already tagged.
    et_tagged: bool,
}

/// Every marker and rule in one content buffer.
#[derive(Debug, Default)]
pub(crate) struct Scan {
    pub(crate) markers: Vec<Marker>,
    /// Rule companions: (start of `BDC` operands, end of `EMC`).
    rules: Vec<(usize, usize)>,
}

/// A `BDC` whose tag is [`DECORATION_TAG`], classified by its property list.
enum Opened {
    Marker(Marker),
    Rule(usize),
    /// Any other sequence; `true` when it tags its content (`/MCID` or
    /// `Artifact`).
    Other(bool),
}

impl Scan {
    /// Every decoration marker and rule in `cs`, with where each ends.
    pub(crate) fn of(cs: &ContentStream) -> Self {
        let buf = cs.buf.as_slice();
        let mut scan = Self::default();
        let mut open: Vec<Opened> = Vec::new();
        let mut waiting_et: Vec<usize> = Vec::new();
        for op in cs.operations() {
            let end = op.operator.span.start + op.operator.span.len;
            match op.operator_name(buf) {
                Some(b"BDC") => {
                    let start = op
                        .operands
                        .first()
                        .map_or(op.operator.span.start, |t| t.span.start);
                    open.push(classify(op.operands, start, end));
                }
                Some(b"BMC") => open.push(Opened::Other(is_artifact(op.operands))),
                Some(b"EMC") => match open.pop() {
                    Some(Opened::Marker(mut m)) => {
                        m.emc = (op.operator.span.start, end);
                        waiting_et.push(scan.markers.len());
                        scan.markers.push(m);
                    }
                    Some(Opened::Rule(start)) => scan.rules.push((start, end)),
                    _ => {}
                },
                Some(b"ET") => {
                    let tagged = open.iter().any(|o| matches!(o, Opened::Other(true)));
                    for i in waiting_et.drain(..) {
                        if let Some(m) = scan.markers.get_mut(i) {
                            m.et_end = Some(end);
                            m.et_tagged = tagged;
                        }
                    }
                }
                _ => {}
            }
        }
        scan
    }
}

fn classify(operands: &[crate::content::ContentToken], start: usize, end: usize) -> Opened {
    let operand = |i: usize| match operands.get(i).map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(o)) => Some(o),
        _ => None,
    };
    let tagged = operand(0)
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == DECORATION_TAG);
    let Some(props) = operand(1).and_then(Object::as_dict).filter(|_| tagged) else {
        let mcid = operand(1)
            .and_then(Object::as_dict)
            .is_some_and(|d| d.get(b"MCID").is_some());
        return Opened::Other(mcid || is_artifact(operands));
    };
    if props.get(b"Rule").is_some() {
        return Opened::Rule(start);
    }
    let set = read_set(props.get(b"Line"));
    if set.is_empty() {
        return Opened::Other(false);
    }
    let id = props
        .get(b"Id")
        .and_then(Object::as_int)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0);
    let color = props
        .get(b"C")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_number).collect());
    let width = props
        .get(b"W")
        .and_then(Object::as_number)
        .filter(|w| *w > 0.0);
    let metrics = match props.get(b"M").and_then(Object::as_name) {
        Some(n) if n.as_bytes() == b"Standard" => DecorationMetrics::Standard,
        _ => DecorationMetrics::FontTables,
    };
    Opened::Marker(Marker {
        id,
        set,
        color,
        width,
        metrics,
        bdc: (start, end),
        emc: (0, 0),
        et_end: None,
        et_tagged: false,
    })
}

fn is_artifact(operands: &[crate::content::ContentToken]) -> bool {
    matches!(
        operands.first().map(|t| &t.kind),
        Some(ContentTokenKind::Operand(o)) if o.as_name().is_some_and(|n| n.as_bytes() == b"Artifact")
    )
}

fn read_set(line: Option<&Object>) -> DecorationSet {
    let mut set = DecorationSet::NONE;
    let mut take = |o: &Object| match o.as_name().map(crate::object::Name::as_bytes) {
        Some(b"Underline") => set.underline = true,
        Some(b"StrikeOut") => set.strikethrough = true,
        _ => {}
    };
    match line {
        Some(Object::Array(items)) => items.iter().for_each(&mut take),
        Some(o) => take(o),
        None => {}
    }
    set
}

/// The `BDC` that opens a marker carrying `set` with `id`.
fn marker_open(set: DecorationSet, id: u32, metrics: DecorationMetrics) -> Vec<u8> {
    let mut out = b"/pdfc_Deco <</Line ".to_vec();
    match (set.underline, set.strikethrough) {
        (true, true) => out.extend_from_slice(b"[/Underline /StrikeOut]"),
        (true, false) => out.extend_from_slice(b"/Underline"),
        _ => out.extend_from_slice(b"/StrikeOut"),
    }
    out.extend_from_slice(format!(" /Id {id}").as_bytes());
    if metrics == DecorationMetrics::Standard {
        out.extend_from_slice(b" /M /Standard");
    }
    out.extend_from_slice(b">> BDC");
    out
}

/// The bytes that put `set` on the show operator at `start..end`, which lies
/// inside the marker `enclosing` when there is one: `(before, after)` to
/// wrap the operator's middle slice with.
///
/// Markers never nest: an enclosing marker is closed before the slice and
/// reopened after it (as a fresh `/Id`; [`refresh`] drops it if empty).
pub(crate) fn wrap_for(
    scan: &Scan,
    start: usize,
    end: usize,
    set: DecorationSet,
    metrics: DecorationMetrics,
) -> (Vec<u8>, Vec<u8>) {
    let mut next = scan.markers.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let enclosing = scan
        .markers
        .iter()
        .find(|m| m.bdc.1 <= start && end <= m.emc.0);
    let mut before = Vec::new();
    let mut after = Vec::new();
    if enclosing.is_some() {
        before.extend_from_slice(b"EMC ");
    }
    if !set.is_empty() {
        before.extend_from_slice(&marker_open(set, next, metrics));
        after.extend_from_slice(b"EMC");
        next += 1;
    }
    if let Some(m) = enclosing {
        if !after.is_empty() {
            after.push(b' ');
        }
        let mut reopened = Marker {
            id: next,
            ..m.clone()
        };
        reopened.id = next;
        after.extend_from_slice(&reopen_bytes(&reopened));
    }
    (before, after)
}

fn reopen_bytes(m: &Marker) -> Vec<u8> {
    let mut out = marker_open(m.set, m.id, m.metrics);
    let tail = b">> BDC";
    out.truncate(out.len() - tail.len());
    if let Some(c) = &m.color {
        out.extend_from_slice(b" /C [");
        for (i, v) in c.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            emit_number(&mut out, round4(*v));
        }
        out.push(b']');
    }
    if let Some(w) = m.width {
        out.extend_from_slice(b" /W ");
        emit_number(&mut out, round4(w));
    }
    out.extend_from_slice(tail);
    out
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// What [`refresh`] did, for the edit's report.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Refreshed {
    pub(crate) content: Vec<u8>,
    pub(crate) strike_sources: Vec<StrikeSource>,
}

/// Recompute every rule in the content buffer `cs` (the page's own, or a
/// form's, named by `source`) from its markers' glyphs. `glyphs` are
/// extracted from the page painting it; `resources` resolve the buffer's
/// font names. `None` when nothing changes.
///
/// Rules are removed and rewritten after the `ET` that closes each marker;
/// an empty marker is unwrapped; a duplicate `/Id` is renumbered. A form
/// painted more than once yields one copy of its glyphs per invocation;
/// only the first invocation's are used, since a `Tm`-relative rule is the
/// same bytes under every one.
pub(crate) fn refresh(
    view: &DocumentView<'_>,
    resources: &Dict,
    source: ContentStreamRef,
    cs: &ContentStream,
    glyphs: &[&ExtractedGlyph],
) -> Option<Refreshed> {
    let scan = Scan::of(cs);
    if scan.markers.is_empty() && scan.rules.is_empty() {
        return None;
    }
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    for &(start, end) in &scan.rules {
        let from = if start > 0 && cs.buf.get(start - 1) == Some(&b'\n') {
            start - 1
        } else {
            start
        };
        edits.push((from, end, Vec::new()));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut next = scan.markers.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let mut inserts: std::collections::BTreeMap<usize, Vec<u8>> = std::collections::BTreeMap::new();
    let mut strike_sources = Vec::new();
    let mut fonts = std::collections::HashMap::new();
    let tagged_doc = super::addtext::is_tagged(view);
    for m in &scan.markers {
        let mut mine: Vec<&ExtractedGlyph> = glyphs
            .iter()
            .copied()
            .filter(|g| inside(g, source, m.bdc.1, m.emc.0))
            .collect();
        let first_ctm = mine
            .first()
            .and_then(|g| g.provenance.as_ref())
            .map(|p| p.ctm);
        mine.retain(|g| g.provenance.as_ref().map(|p| p.ctm) == first_ctm);
        if mine.is_empty() {
            edits.push((m.bdc.0, m.bdc.1, Vec::new()));
            edits.push((m.emc.0, m.emc.1, Vec::new()));
            continue;
        }
        let mut id = m.id;
        if !seen.insert(id) {
            id = next;
            next += 1;
            let mut renamed = m.clone();
            renamed.id = id;
            edits.push((m.bdc.0, m.bdc.1, reopen_bytes(&renamed)));
        }
        let Some(et_end) = m.et_end else { continue };
        let rules = inserts.entry(et_end).or_default();
        let key = (
            mine.first()
                .and_then(|g| g.provenance.as_ref())
                .and_then(|p| p.font_resource.clone()),
            m.metrics,
        );
        let metrics = *fonts.entry(key).or_insert_with(|| {
            let font = mine.first().and_then(|g| font_dict(view, resources, g));
            LineMetrics::for_font(view, font, m.metrics)
        });
        if m.set.strikethrough {
            strike_sources.push(metrics.strike_source);
        }
        for line in lines(&mine) {
            let artifact = tagged_doc && !m.et_tagged;
            rules.extend_from_slice(&line_rules(m, id, &line, metrics, artifact));
        }
    }
    for (at, bytes) in inserts {
        edits.push((at, at, bytes));
    }
    let content = apply(&cs.buf, edits)?;
    (content != cs.buf).then_some(Refreshed {
        content,
        strike_sources,
    })
}

fn inside(g: &ExtractedGlyph, source: ContentStreamRef, start: usize, end: usize) -> bool {
    g.provenance.as_ref().is_some_and(|p| {
        p.content_stream == source
            && p.operator_span.start >= start
            && p.operator_span.start + p.operator_span.len <= end
    })
}

fn font_dict<'v>(
    view: &'v DocumentView<'_>,
    resources: &'v Dict,
    g: &ExtractedGlyph,
) -> Option<&'v Dict> {
    let name = g.provenance.as_ref()?.font_resource.as_ref()?;
    let fonts = view.resolve(resources.get(b"Font")?).as_dict()?;
    view.resolve(fonts.get(name)?).as_dict()
}

/// Apply non-overlapping edits sorted by position; `None` if two overlap.
fn apply(buf: &[u8], mut edits: Vec<(usize, usize, Vec<u8>)>) -> Option<Vec<u8>> {
    edits.sort_by_key(|(s, e, _)| (*s, *e));
    let mut out = Vec::with_capacity(buf.len());
    let mut cursor = 0;
    for (start, end, bytes) in edits {
        if start < cursor {
            return None;
        }
        out.extend_from_slice(buf.get(cursor..start)?);
        out.extend_from_slice(&bytes);
        cursor = end;
    }
    out.extend_from_slice(buf.get(cursor..)?);
    Some(out)
}

/// One baseline's worth of a marker's glyphs, in the text space of its
/// first glyph: the frame matrix (`Tm` × CTM), the baseline height and the
/// horizontal extent.
struct Line<'g> {
    first: &'g GlyphProvenance,
    frame: [f64; 6],
    baseline: f64,
    x0: f64,
    x1: f64,
}

fn lines<'g>(glyphs: &[&'g ExtractedGlyph]) -> Vec<Line<'g>> {
    let mut out: Vec<Line<'g>> = Vec::new();
    for g in glyphs {
        let Some(p) = g.provenance.as_ref() else {
            continue;
        };
        let tol = f64::from(p.tf_size).abs().max(1.0) * 0.05;
        if let Some(line) = out.last_mut()
            && let Some(inv) = invert(line.frame)
        {
            let (sx, sy) = map(inv, f64::from(g.x), f64::from(g.y));
            let (ex, _) = map(
                inv,
                f64::from(g.advance_end().0),
                f64::from(g.advance_end().1),
            );
            if (sy - line.baseline).abs() <= tol && sx >= line.x0 - tol {
                line.x0 = line.x0.min(sx);
                line.x1 = line.x1.max(ex);
                continue;
            }
        }
        let frame = compose(widen(p.text_matrix), widen(p.ctm));
        let Some(inv) = invert(frame) else { continue };
        let (sx, sy) = map(inv, f64::from(g.x), f64::from(g.y));
        let (ex, _) = map(
            inv,
            f64::from(g.advance_end().0),
            f64::from(g.advance_end().1),
        );
        out.push(Line {
            first: p,
            frame,
            baseline: sy,
            x0: sx.min(ex),
            x1: sx.max(ex),
        });
    }
    out
}

/// `artifact` wraps each rule as a layout artifact, for a tagged document
/// whose `ET` is outside any tagged sequence: an untagged rule would be
/// real content belonging to no structure element (ISO 14289-1 7.1).
fn line_rules(
    m: &Marker,
    id: u32,
    line: &Line<'_>,
    metrics: LineMetrics,
    artifact: bool,
) -> Vec<u8> {
    let em = f64::from(line.first.tf_size) / 1000.0;
    let mut out = Vec::new();
    let mut rule = |centre: f64, thickness: f64| {
        let thickness = m.width.unwrap_or(thickness) * em;
        out.extend_from_slice(format!("\n/{} <</Rule {id}>> BDC", "pdfc_Deco").as_bytes());
        if artifact {
            out.extend_from_slice(b" /Artifact <</Type /Layout>> BDC");
        }
        out.extend_from_slice(b" q");
        // `Tm` alone: the CTM in force after `ET` is the one the glyphs were
        // shown under, so the rule's `cm` must not apply it a second time.
        for v in widen(line.first.text_matrix) {
            out.push(b' ');
            emit_number(&mut out, round4(v));
        }
        out.extend_from_slice(b" cm ");
        push_colour(&mut out, m.color.as_deref(), line.first.fill_color.as_ref());
        for v in [
            line.x0,
            line.baseline + centre * em - thickness / 2.0,
            line.x1 - line.x0,
            thickness,
        ] {
            emit_number(&mut out, round4(v));
            out.push(b' ');
        }
        out.extend_from_slice(if artifact {
            &b"re f Q EMC EMC"[..]
        } else {
            &b"re f Q EMC"[..]
        });
    };
    if m.set.underline {
        rule(metrics.underline_centre, metrics.underline_thickness);
    }
    if m.set.strikethrough {
        rule(metrics.strike_centre, metrics.strike_thickness);
    }
    out
}

/// The fill operator for a rule: the marker's `/C`, else the run's fill
/// colour; nothing (inherit) for an unmodelled colour space.
fn push_colour(out: &mut Vec<u8>, over: Option<&[f64]>, run: Option<&TextColor>) {
    let comps: Vec<f64> = match (over, run) {
        (Some(c), _) => c.to_vec(),
        (None, Some(TextColor::Gray(g))) => vec![f64::from(*g)],
        (None, Some(TextColor::Rgb(r, g, b))) => vec![f64::from(*r), f64::from(*g), f64::from(*b)],
        (None, Some(TextColor::Cmyk(c, m, y, k))) => {
            vec![f64::from(*c), f64::from(*m), f64::from(*y), f64::from(*k)]
        }
        (None, None) => vec![0.0],
        (None, Some(_)) => return,
    };
    let op: &[u8] = match comps.len() {
        1 => b"g ",
        3 => b"rg ",
        4 => b"k ",
        _ => return,
    };
    for v in comps {
        emit_number(out, round4(v));
        out.push(b' ');
    }
    out.extend_from_slice(op);
}

fn widen(m: [f32; 6]) -> [f64; 6] {
    m.map(f64::from)
}

/// `a` then `b`, in §8.3.3's row-vector convention.
fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    let [a0, a1, a2, a3, a4, a5] = a;
    let [b0, b1, b2, b3, b4, b5] = b;
    [
        a0 * b0 + a1 * b2,
        a0 * b1 + a1 * b3,
        a2 * b0 + a3 * b2,
        a2 * b1 + a3 * b3,
        a4 * b0 + a5 * b2 + b4,
        a4 * b1 + a5 * b3 + b5,
    ]
}

fn invert(m: [f64; 6]) -> Option<[f64; 6]> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det.abs() < 1e-12 {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some([ia, ib, ic, id, -(e * ia + f * ic), -(e * ib + f * id)])
}

fn map(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = m;
    (x * a + y * c + e, x * b + y * d + f)
}
