//! A rich-text value: an `exData` with `contentType="text/html"` in a
//! `draw`'s, `field`'s or caption's `value`, read into styled paragraphs
//! (XFA 3.3 Ch.5 pp.215-223, Ch.27 pp.1187-1223; slice
//! `adobe_ext__xfa_rich_text.md`).
//!
//! Elements read: `html`, `body`, `p`, `span`, `b`, `i`, `br`, `sub`,
//! `sup`, `a`, `ul`, `ol`, `li`. An unrecognised element is dropped with
//! its content and an unrecognised style property is ignored (Ch.27
//! opening, slice §3). Whitespace collapses as XHTML's does, except inside
//! `xfa-spacerun:yes`. The container's `font` and `para` give the starting
//! style. A floating field (`span` with `embed="#id"`) shows the named
//! node's plain value.

use super::text::face;
use super::{XfaNode, parse_rgb};
use crate::fontdata::Std14;
use crate::xfa::Measurement;

/// The default tab interval: half an inch.
const TAB_INTERVAL: f64 = 36.0;

/// Element nesting deeper than this is dropped, as unrecognised.
const MAX_DEPTH: usize = 64;

/// How one run of characters is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RunStyle {
    pub(super) face: Std14,
    pub(super) size: f64,
    pub(super) rgb: [f64; 3],
    pub(super) underline: bool,
    pub(super) strike: bool,
    /// Baseline shift, up, in points (`sub`, `sup`, `vertical-align`).
    pub(super) shift: f64,
}

/// One piece of a paragraph, in order.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Piece {
    /// Characters with no breakable space in them.
    Text(String, RunStyle),
    /// A breakable space; `kept` under `xfa-spacerun`, where it neither
    /// collapses nor is dropped at a line start.
    Space(RunStyle, bool),
    /// A tab (`xfa-tab-count`).
    Tab(RunStyle),
    /// A forced line break (`br`).
    Break(RunStyle),
}

/// One paragraph: its pieces and its box.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Para {
    pub(super) pieces: Vec<Piece>,
    /// Fraction of the free width left of each line: 0 left, 0.5 centre,
    /// 1 right.
    pub(super) align: f64,
    pub(super) indent: f64,
    pub(super) margin_left: f64,
    pub(super) margin_right: f64,
    pub(super) space_before: f64,
    pub(super) space_after: f64,
    pub(super) line_height: Option<f64>,
    pub(super) tab_interval: f64,
    pub(super) tab_stops: Vec<f64>,
    /// The style an empty line takes its height from.
    pub(super) base: RunStyle,
}

/// A rich-text value read for drawing.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Rich {
    pub(super) paras: Vec<Para>,
    /// Markup was dropped or drawn approximately.
    pub(super) approximated: bool,
    /// A face is not a standard-14 one.
    pub(super) substituted: bool,
}

/// The starting style, from the container's `font` and `para`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Base {
    pub(super) family: String,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) size: f64,
    pub(super) rgb: [f64; 3],
    pub(super) align: f64,
    pub(super) indent: f64,
    pub(super) margin_left: f64,
    pub(super) margin_right: f64,
    pub(super) space_before: f64,
    pub(super) space_after: f64,
    pub(super) line_height: Option<f64>,
}

/// The inherited style at one point of the walk.
#[derive(Debug, Clone)]
struct Cascade {
    family: String,
    bold: bool,
    italic: bool,
    size: f64,
    rgb: [f64; 3],
    underline: bool,
    strike: bool,
    shift: f64,
    spacerun: bool,
    align: f64,
    indent: f64,
    line_height: Option<f64>,
    tab_interval: f64,
    tab_stops: Vec<f64>,
}

/// `holder`'s rich-text `exData`, when its `value` holds one.
pub(super) fn rich_value(holder: &XfaNode) -> Option<&XfaNode> {
    holder
        .child("value")?
        .child("exData")
        .filter(|e| e.attr("contentType").map(str::trim) == Some("text/html"))
}

/// Read `ex` from `base`; `lookup` gives a floating field's value by `id`.
pub(super) fn read(ex: &XfaNode, base: &Base, lookup: &dyn Fn(&str) -> Option<String>) -> Rich {
    let cascade = Cascade {
        family: base.family.clone(),
        bold: base.bold,
        italic: base.italic,
        size: base.size,
        rgb: base.rgb,
        underline: false,
        strike: false,
        shift: 0.0,
        spacerun: false,
        align: base.align,
        indent: base.indent,
        line_height: base.line_height,
        tab_interval: TAB_INTERVAL,
        tab_stops: Vec::new(),
    };
    let mut r = Reader {
        out: Rich::default(),
        cur: None,
        base: base.clone(),
        lookup,
    };
    r.walk(ex, &cascade, 0);
    r.finish();
    r.out
}

struct Reader<'l> {
    out: Rich,
    cur: Option<Para>,
    base: Base,
    lookup: &'l dyn Fn(&str) -> Option<String>,
}

impl Reader<'_> {
    fn walk(&mut self, node: &XfaNode, c: &Cascade, depth: usize) {
        for child in &node.children {
            if child.tag == XfaNode::TEXT_TAG {
                self.text(&child.text, c);
            } else if depth >= MAX_DEPTH {
                self.out.approximated = true;
            } else {
                self.element(child, c, depth + 1);
            }
        }
    }

    fn element(&mut self, e: &XfaNode, c: &Cascade, depth: usize) {
        let mut c = c.clone();
        match e.tag.as_str() {
            "b" => c.bold = true,
            "i" => c.italic = true,
            "sub" => c.shift -= c.size / 3.0,
            "sup" => c.shift += c.size / 3.0,
            "ul" | "ol" => self.out.approximated = true,
            "html" | "body" | "span" | "a" | "p" | "li" | "br" => {}
            _ => {
                self.out.approximated = true;
                return;
            }
        }
        let margins = self.style(e, &mut c);
        match e.tag.as_str() {
            "br" => {
                let style = run_style(&c, &mut self.out);
                self.push(Piece::Break(style), &c);
            }
            "p" | "li" => {
                self.finish();
                self.cur = Some(para(&c, margins, &self.base, &mut self.out));
                self.walk(e, &c, depth);
                self.finish();
            }
            _ if e.attr("embed").is_some() => self.embed(e, &c),
            _ => self.walk(e, &c, depth),
        }
    }

    /// A floating field: the value of the node `embed` names (Ch.5 p.219).
    fn embed(&mut self, e: &XfaNode, c: &Cascade) {
        let id = e.attr("embed").unwrap_or_default().trim();
        let found = id
            .strip_prefix('#')
            .filter(|_| e.attr("embedType").is_none_or(|t| t.trim() == "uri"))
            .and_then(|id| (self.lookup)(id));
        match found {
            Some(v) => self.text(&v, c),
            None => self.out.approximated = true,
        }
    }

    /// Apply `e`'s `style` to `c`; returns its box margins (left, right,
    /// top, bottom), which do not inherit.
    fn style(&mut self, e: &XfaNode, c: &mut Cascade) -> [Option<f64>; 4] {
        let mut m = [None; 4];
        for decl in e.attr("style").unwrap_or_default().split(';') {
            let Some((prop, value)) = decl.split_once(':') else {
                continue;
            };
            let (prop, v) = (prop.trim().to_ascii_lowercase(), value.trim());
            if !self.property(&prop, v, c, &mut m) {
                self.out.approximated |= !ignorable(&prop, v);
            }
        }
        m
    }

    /// Apply one declaration; `false` when it is not one drawn here.
    fn property(&mut self, prop: &str, v: &str, c: &mut Cascade, m: &mut [Option<f64>; 4]) -> bool {
        let lower = v.to_ascii_lowercase();
        match prop {
            "font-family" => {
                let first = v.split(',').next().unwrap_or_default();
                c.family = first.trim().trim_matches(['\'', '"']).to_owned();
            }
            "font-size" => {
                let size = length(v, c.size).filter(|s| *s > 0.0);
                return set(&mut c.size, size);
            }
            "font-weight" => {
                c.bold = match lower.as_str() {
                    "bold" | "bolder" => true,
                    "normal" | "lighter" => false,
                    n => n.parse::<u16>().is_ok_and(|n| n >= 600),
                }
            }
            "font-style" => c.italic = matches!(lower.as_str(), "italic" | "oblique"),
            "color" => return set(&mut c.rgb, css_color(&lower)),
            "text-decoration" => {
                c.underline = lower.contains("underline");
                c.strike = lower.contains("line-through");
            }
            "vertical-align" => return self.vertical(&lower, c),
            "text-align" => return self.align(&lower, c),
            "text-indent" => return set(&mut c.indent, length(v, c.size)),
            "line-height" => c.line_height = line_height(v, c.size),
            "tab-interval" => {
                return set(&mut c.tab_interval, length(v, c.size).filter(|t| *t > 0.0));
            }
            "tab-stops" => c.tab_stops = tab_stops(v, c.size),
            "xfa-spacerun" => c.spacerun = lower == "yes",
            "xfa-tab-count" => {
                let n = lower.parse::<usize>().unwrap_or(0).min(64);
                let style = run_style(c, &mut self.out);
                for _ in 0..n {
                    self.push(Piece::Tab(style.clone()), c);
                }
            }
            "margin-left" => m[0] = length(v, c.size),
            "margin-right" => m[1] = length(v, c.size),
            "margin-top" => m[2] = length(v, c.size),
            "margin-bottom" => m[3] = length(v, c.size),
            "margin" => return margin(v, c.size, m),
            _ => return false,
        }
        true
    }

    fn vertical(&mut self, v: &str, c: &mut Cascade) -> bool {
        match v {
            "baseline" => {}
            "sub" => c.shift -= c.size / 3.0,
            "super" => c.shift += c.size / 3.0,
            other => match length(other, c.size) {
                Some(l) => c.shift += l,
                None => return false,
            },
        }
        true
    }

    fn align(&mut self, v: &str, c: &mut Cascade) -> bool {
        c.align = match v {
            "center" => 0.5,
            "right" => 1.0,
            "left" => 0.0,
            "justify" => {
                self.out.approximated = true;
                0.0
            }
            _ => return false,
        };
        true
    }

    /// Character data: collapsed to single breakable spaces unless under
    /// `xfa-spacerun`.
    fn text(&mut self, t: &str, c: &Cascade) {
        let style = run_style(c, &mut self.out);
        for ch in t.chars() {
            if ch == ' ' && c.spacerun {
                self.push(Piece::Space(style.clone(), true), c);
            } else if matches!(ch, ' ' | '\t' | '\n' | '\r') && !c.spacerun {
                let collapse = self.cur.as_ref().is_none_or(|p| {
                    matches!(
                        p.pieces.last(),
                        None | Some(Piece::Space(..) | Piece::Break(_))
                    )
                });
                if !collapse {
                    self.push(Piece::Space(style.clone(), false), c);
                }
            } else {
                self.char(ch, &style, c);
            }
        }
    }

    fn char(&mut self, ch: char, style: &RunStyle, c: &Cascade) {
        if let Some(Some(Piece::Text(s, st))) = self.cur.as_mut().map(|p| p.pieces.last_mut())
            && st == style
        {
            s.push(ch);
            return;
        }
        self.push(Piece::Text(ch.to_string(), style.clone()), c);
    }

    fn push(&mut self, piece: Piece, c: &Cascade) {
        if self.cur.is_none() {
            self.cur = Some(para(c, [None; 4], &self.base, &mut self.out));
        }
        if let Some(p) = self.cur.as_mut() {
            p.pieces.push(piece);
        }
    }

    fn finish(&mut self) {
        if let Some(p) = self.cur.take() {
            self.out.paras.push(p);
        }
    }
}

/// A paragraph opened under `c`; an omitted margin takes the container
/// `para`'s.
fn para(c: &Cascade, m: [Option<f64>; 4], base: &Base, out: &mut Rich) -> Para {
    Para {
        pieces: Vec::new(),
        align: c.align,
        indent: c.indent,
        margin_left: m[0].unwrap_or(base.margin_left),
        margin_right: m[1].unwrap_or(base.margin_right),
        space_before: m[2].unwrap_or(base.space_before),
        space_after: m[3].unwrap_or(base.space_after),
        line_height: c.line_height,
        tab_interval: c.tab_interval,
        tab_stops: c.tab_stops.clone(),
        base: run_style(c, out),
    }
}

fn run_style(c: &Cascade, out: &mut Rich) -> RunStyle {
    let (face, substituted) = face(&c.family, c.bold, c.italic);
    out.substituted |= substituted;
    RunStyle {
        face,
        size: c.size,
        rgb: c.rgb,
        underline: c.underline,
        strike: c.strike,
        shift: c.shift,
    }
}

fn set<T>(slot: &mut T, v: Option<T>) -> bool {
    v.map(|v| *slot = v).is_some()
}

/// Properties with no visible effect at the value given: skipped silently.
fn ignorable(prop: &str, v: &str) -> bool {
    let zero = length(v, 0.0) == Some(0.0) || v.eq_ignore_ascii_case("normal");
    matches!(prop, "letter-spacing" | "word-spacing") && zero
        || matches!(
            prop,
            "orphans"
                | "widows"
                | "kerning-mode"
                | "page-break-after"
                | "page-break-before"
                | "page-break-inside"
        )
}

/// A CSS length in points: `em` against `size`, unitless only for 0.
fn length(v: &str, size: f64) -> Option<f64> {
    let v = v.trim();
    if let Ok(n) = v.parse::<f64>() {
        return (n == 0.0).then_some(0.0);
    }
    if let Some(px) = v.strip_suffix("px") {
        return px.trim().parse::<f64>().ok().map(|p| p * 0.75);
    }
    match Measurement::parse(v)? {
        Measurement::Em(e) => Some(e * size),
        Measurement::Percent(p) => Some(p / 100.0 * size),
        m => m.points(),
    }
}

/// `line-height`: a length, or a unitless multiple of the font size.
fn line_height(v: &str, size: f64) -> Option<f64> {
    match v.trim().parse::<f64>() {
        Ok(n) => Some(n * size),
        Err(_) => length(v, size),
    }
}

/// `tab-stops`: `[align] length` pairs; every stop is drawn as `left`.
fn tab_stops(v: &str, size: f64) -> Vec<f64> {
    let mut stops: Vec<f64> = v
        .split_whitespace()
        .filter_map(|t| length(t, size))
        .collect();
    stops.sort_by(f64::total_cmp);
    stops
}

/// The `margin` shorthand: 1 to 4 lengths, top-right-bottom-left.
fn margin(v: &str, size: f64, m: &mut [Option<f64>; 4]) -> bool {
    let l: Option<Vec<f64>> = v.split_whitespace().map(|t| length(t, size)).collect();
    let Some(l) = l else {
        return false;
    };
    let (top, right, bottom, left) = match l[..] {
        [a] => (a, a, a, a),
        [a, b] => (a, b, a, b),
        [a, b, c] => (a, b, c, b),
        [a, b, c, d] => (a, b, c, d),
        _ => return false,
    };
    *m = [Some(left), Some(right), Some(top), Some(bottom)];
    true
}

/// `#rgb`, `#rrggbb` or `rgb(r, g, b)` as 0-1 components.
fn css_color(v: &str) -> Option<[f64; 3]> {
    if let Some(inner) = v.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        return parse_rgb(inner);
    }
    let hex = v.strip_prefix('#')?;
    let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    let (r, g, b) = match digits[..] {
        [r, g, b] => (r * 17, g * 17, b * 17),
        [r1, r0, g1, g0, b1, b0] => (r1 * 16 + r0, g1 * 16 + g0, b1 * 16 + b0),
        _ => return None,
    };
    let f = |n: u32| f64::from(n) / 255.0;
    Some([f(r), f(g), f(b)])
}
