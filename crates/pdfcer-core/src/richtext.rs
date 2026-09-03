//! # Rich text strings — `/RV` + `/DS` (ISO 32000-1 §12.7.3.4)
//!
//! Parses a form field's **rich text value** (`/RV`, an XHTML-subset
//! document) and its **default style string** (`/DS`, a bare CSS
//! declaration list) into a flat list of styled [`Run`]s — one run per
//! contiguous stretch of text sharing one resolved style.
//!
//! This module **reads and models**. It does not render, does not author,
//! and does not touch a document. Appearance generation from these runs is
//! a separate concern with a separate and much larger caveat (see
//! "Appearance generation is NOT specified" below).
//!
//! ## Why a flat run list and not a tree
//!
//! Because that is the shape the format actually is, and the shape every
//! consumer wants. `<b>` and `<i>` nest, but nesting carries no meaning
//! beyond the style it contributes: `<b><i>x</i></b>` and
//! `<i><b>x</b></i>` are the same picture. Acrobat's own scripting model
//! agrees — its `Span` object is a **run-length-encoded array**, one entry
//! per contiguous formatting run, not a DOM (verified against
//! `Acrobat_Features/forms__rich_text_fields.md`, 2026-08-10). Keeping a
//! tree would preserve a distinction with no observable consequence and
//! force every consumer to flatten it anyway.
//!
//! Paragraphs DO survive, as [`Run::paragraph`], because `<p>` is a line
//! break and that is observable.
//!
//! ## The grammar is small, closed, and version-gated
//!
//! At **PDF 1.5** — which is what ISO 32000-1 pins — the whole grammar is
//! five elements and ten style attributes:
//!
//! | Table 223 elements | meaning |
//! |---|---|
//! | `<body>` | root; carries the Table 224 namespace attributes |
//! | `<p>` | a paragraph |
//! | `<b>` | bold |
//! | `<i>` | italic |
//! | `<span>` | groups text **solely** to apply Table 225 styles |
//!
//! Table 225 adds `text-align`, `vertical-align`, `font-size`,
//! `font-style`, `font-weight`, `font-family`, `font` (shorthand),
//! `color`, `text-decoration`, `font-stretch` — and nothing else. No
//! `<br>`, no lists, no tables, no margins, no line-height, no
//! `text-indent`.
//!
//! **PDF 1.6 and 1.7 defer outward** to XFA 2.2 / 2.4, whose supersets
//! ISO 32000-1 does not enumerate anywhere. This module implements the 1.5
//! set and treats anything beyond it as unknown markup (below).
//!
//! ## `/DS` is not XML, and that is easy to get wrong
//!
//! `/RV` is an XML document; `/DS` is a **bare CSS declaration list** —
//! `font: 12pt Helvetica; color: #FF0000` — with no element around it.
//! Per **RT-M6** it supplies the default for any Table 225 attribute a run
//! does not set itself, and it is a `shall`-input to appearance generation
//! alongside `/RV`. Feeding `/DS` to an XML parser produces nothing useful
//! and no error worth reading, so it has its own parser here.
//!
//! ## Unknown markup is IGNORED, and its text is KEPT
//!
//! An element outside Table 223 contributes no style, but its character
//! data is still the field's text and is emitted with the enclosing style.
//! Dropping it would silently lose content — the single worst outcome for
//! a value the operator typed. An unrecognised style property is likewise
//! skipped rather than failing the parse: a PDF 1.7 file may legally carry
//! XFA-2.4 attributes this module does not model, and refusing the whole
//! value because one attribute is unfamiliar would make pdfcer unable to
//! read files Acrobat reads fine.
//!
//! This is a deliberate asymmetry with pdfcer's usual strictness. It is
//! justified by which way the damage runs: a missed style is a cosmetic
//! difference the operator can see; a dropped run is text that vanishes.
//!
//! ## ★ Appearance generation is NOT specified, and that is load-bearing
//!
//! §12.7.3.3 explicitly switches OFF its own appearance-generation
//! conventions for rich-text fields and **puts nothing in their place** —
//! the only positive mandates are that `/DS` and `/RV` *shall* be the
//! inputs (RT-M6) and that the entire appearance *shall* be regenerated on
//! every value change (RT-M2, unconditionally, regardless of
//! `/NeedAppearances`).
//!
//! So any pdfcer rendering built on these runs is **policy, not
//! conformance** — a defensible choice among several, not "what the
//! standard says". Project rule 4 requires that be disclosed as pdfcer's
//! choice rather than presented as the specification's. This module's job
//! is to make the inputs to that choice exact; it does not make the
//! choice.
//!
//! ## What this module deliberately leaves undecided
//!
//! **`/DA` versus `/DS` precedence is UNDEFINED** (RT-A6) when both could
//! set the same attribute on a rich-text field. ISO 32000-1 states no
//! rule; no Acrobat tiebreak has been found either. Under the operator's
//! standing instruction — never hard-code a choice the standard leaves
//! open — that resolution is a **setting**, and it does not live here:
//! this module never reads `/DA`. [`parse`] takes `/RV` and `/DS` only,
//! so the precedence question is forced to be answered explicitly by
//! whoever combines them, rather than silently settled by the order two
//! branches happen to be written in.
//!
//! ## Spec sources
//!
//! `PDF_Spec/iso32000/iso32000__s__12.7.3.4.md` — Tables 223 (elements),
//! 224 (`<body>` attributes), 225 (style attributes), 222 (`/DS`, `/RV`
//! rows), 246 (FDF `/RV`); mandates RT-M2, RT-M6, RT-M12; ambiguity
//! RT-A6.

use crate::fdf::{XmlElement, XmlNode};

/// Horizontal alignment — Table 225 `text-align`.
///
/// Exactly three values. CSS's `justify` is **not** in Table 225 and is
/// not accepted; a file carrying it is using markup outside the PDF 1.5
/// grammar, and it is skipped like any other unknown property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// `text-align: left`.
    Left,
    /// `text-align: center`.
    Center,
    /// `text-align: right`.
    Right,
}

/// Font width — Table 225 `font-stretch`, the nine-step scale.
///
/// Ordered narrowest to widest, which is the order Table 225 lists them
/// in and the order a font-matching pass wants. `Ord` is derived for
/// exactly that: picking the nearest available width needs comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stretch {
    /// `ultra-condensed`.
    UltraCondensed,
    /// `extra-condensed`.
    ExtraCondensed,
    /// `condensed`.
    Condensed,
    /// `semi-condensed`.
    SemiCondensed,
    /// `normal`.
    Normal,
    /// `semi-expanded`.
    SemiExpanded,
    /// `expanded`.
    Expanded,
    /// `extra-expanded`.
    ExtraExpanded,
    /// `ultra-expanded`.
    UltraExpanded,
}

/// One run's resolved style: every Table 225 attribute, each `None` when
/// neither the run nor `/DS` set it.
///
/// `None` means **unspecified**, not "default" — the distinction matters
/// because the fallback for an unspecified attribute is a rendering
/// decision (policy, see the module docs), and collapsing it to a
/// concrete default here would make that decision invisibly, in the
/// parser, where nobody would look for it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    /// `text-align`.
    pub align: Option<Align>,
    /// `vertical-align`, in points. **Positive is superscript, negative
    /// is subscript** (Table 225) — a baseline adjustment, not a line
    /// position, so the sign convention is the spec's, not CSS's.
    pub baseline_shift_pt: Option<f64>,
    /// `font-size`, in points.
    pub size_pt: Option<f64>,
    /// `font-style: italic` (`true`) or `normal` (`false`).
    pub italic: Option<bool>,
    /// `font-weight`, normalised to the numeric scale: Table 225 states
    /// `normal` == 400 and `bold` == 700, so the keywords are stored as
    /// their numbers and a consumer never has to handle both spellings.
    pub weight: Option<u16>,
    /// `font-family`, in preference order. Table 225: *"If a list is
    /// provided, the first one containing glyphs for the specified text
    /// shall be used"* — so the order is normative and must be preserved,
    /// not collapsed to a single name.
    pub family: Vec<String>,
    /// `color`, as DeviceRGB components in `0.0..=1.0`.
    ///
    /// Stored already converted. **RT-M12 is a `shall`**: the sRGB values
    /// written in `/RV` *"shall be transformed into values in a non-ICC
    /// based colour space"* for appearance generation. Doing that at parse
    /// time means no consumer can forget it.
    pub color: Option<[f64; 3]>,
    /// `text-decoration: underline`.
    pub underline: Option<bool>,
    /// `text-decoration: line-through`.
    pub strikethrough: Option<bool>,
    /// `font-stretch`.
    pub stretch: Option<Stretch>,
}

impl Style {
    /// Overlay `other` onto `self`, with `other` winning where it is set.
    ///
    /// The CSS-ish cascade, restricted to what Table 225 allows: an
    /// attribute set nearer the text wins over one set further out, and an
    /// attribute nobody set stays `None`. `family` uses emptiness as its
    /// "unset", since it is a list rather than an `Option`.
    fn overlay(&mut self, other: &Self) {
        if other.align.is_some() {
            self.align = other.align;
        }
        if other.baseline_shift_pt.is_some() {
            self.baseline_shift_pt = other.baseline_shift_pt;
        }
        if other.size_pt.is_some() {
            self.size_pt = other.size_pt;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.weight.is_some() {
            self.weight = other.weight;
        }
        if !other.family.is_empty() {
            self.family.clone_from(&other.family);
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strikethrough.is_some() {
            self.strikethrough = other.strikethrough;
        }
        if other.stretch.is_some() {
            self.stretch = other.stretch;
        }
    }
}

/// One contiguous stretch of text sharing one resolved style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The run's text, entity-decoded, **not trimmed**.
    ///
    /// Whitespace between two styled runs is content: trimming turns
    /// `<b>bold</b> word` into `boldword`.
    pub text: String,
    /// The fully-resolved style — `/DS` defaults with the run's own
    /// cascade applied over them.
    pub style: Style,
    /// Which `<p>` this run belongs to, counting from zero.
    ///
    /// Runs in different paragraphs are separated by a line break. Text
    /// that appears directly under `<body>` outside any `<p>` is legal —
    /// Table 223 never forbids it (RT-A9) — and is assigned to the
    /// paragraph in progress.
    pub paragraph: usize,
}

/// Why a rich-text value could not be read.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum RichTextError {
    /// The `/RV` string is not well-formed XML.
    #[error("the rich text value is not well-formed XML: {0}")]
    MalformedXml(String),
    /// The document's root element is not `<body>`.
    ///
    /// Table 223: *"The element at the root of the XML document."* A
    /// different root means this is not a rich text string at all, and
    /// guessing at its shape would be inventing content.
    #[error("the rich text value's root element is {found:?}, not <body>")]
    RootNotBody {
        /// The element name actually found at the root.
        found: String,
    },
}

/// Parse a rich text value and its default style string into styled runs.
///
/// `ds` is the field's `/DS`, if present: per RT-M6 it supplies the
/// default for every Table 225 attribute a run does not set. Passing
/// `None` is legal and means every unset attribute stays `None`.
///
/// `/DA` is deliberately not a parameter — see the module docs on RT-A6.
///
/// # Errors
///
/// [`RichTextError::MalformedXml`] if `/RV` will not parse;
/// [`RichTextError::RootNotBody`] if its root element is not `<body>`.
///
/// Note that neither unknown elements nor unknown style properties are
/// errors — see the module docs on why that asymmetry is deliberate.
///
/// # Examples
///
/// ```
/// use pdfcer_core::richtext::{self, Align};
///
/// let rv = "<body xmlns='http://www.w3.org/1999/xhtml' \
///           xmlns:xfa='http://www.xfa.org/schema/xfa-data/1.0/' xfa:spec='2.4'>\
///           <p style='text-align:center'>plain <b>bold</b></p></body>";
/// let rt = richtext::parse(rv, Some("font-size:12pt")).unwrap();
///
/// assert_eq!(rt.len(), 2);
/// assert_eq!(rt[0].text, "plain ");
/// assert_eq!(rt[1].text, "bold");
///
/// // `<b>` contributes weight 700; the /DS size and the paragraph's
/// // alignment reach both runs.
/// assert_eq!(rt[0].style.weight, None);
/// assert_eq!(rt[1].style.weight, Some(700));
/// assert_eq!(rt[0].style.size_pt, Some(12.0));
/// assert_eq!(rt[1].style.align, Some(Align::Center));
/// ```
pub fn parse(rv: &str, ds: Option<&str>) -> Result<Vec<Run>, RichTextError> {
    let root = crate::fdf::parse_xml_document(rv).map_err(RichTextError::MalformedXml)?;

    // Table 223: `<body>` is THE root. A namespace prefix is stripped
    // before comparing because `xhtml:body` and `body` name the same
    // element and the parser keeps prefixes verbatim.
    if local_name(&root.name) != "body" {
        return Err(RichTextError::RootNotBody {
            found: root.name.clone(),
        });
    }

    // RT-M6: `/DS` is the base every run cascades over.
    let base = ds.map(parse_declarations).unwrap_or_default();

    let mut runs: Vec<Run> = Vec::new();
    let mut paragraph = 0usize;
    walk(&root, &base, &mut paragraph, &mut runs);

    // Empty runs carry no text and no information; a `<span>` with only
    // attributes is a style declaration, not content.
    runs.retain(|r| !r.text.is_empty());
    Ok(runs)
}

/// Walk one element's content in document order, emitting runs.
///
/// `inherited` is the style in force at this element — `/DS` at the root,
/// then each nested element's contribution overlaid. `paragraph` is
/// threaded by `&mut` rather than returned because `<p>` may appear at
/// any depth and the counter has to advance across the whole walk, not
/// per branch.
fn walk(el: &XmlElement, inherited: &Style, paragraph: &mut usize, out: &mut Vec<Run>) {
    for node in &el.nodes {
        match node {
            XmlNode::Text(t) => out.push(Run {
                text: t.clone(),
                style: inherited.clone(),
                paragraph: *paragraph,
            }),
            XmlNode::Child(i) => {
                let Some(child) = el.children.get(*i) else {
                    // Unreachable for a parser-produced tree; skipping
                    // rather than panicking because a malformed index
                    // must not take down a document open.
                    continue;
                };
                let name = local_name(&child.name);

                // A `<p>` after any content starts a new paragraph. The
                // "after any content" guard is what stops the FIRST
                // paragraph counting as a break and offsetting every
                // index by one.
                if name == "p" && !out.is_empty() {
                    *paragraph += 1;
                }

                let mut style = inherited.clone();
                // Element-contributed style first, then the `style`
                // attribute, so `<b style="font-weight:normal">` obeys the
                // explicit attribute. Nearer the text wins.
                match name {
                    // Table 223: `<b>` is bold, `<i>` is italic. Expressed
                    // as their Table 225 numeric/boolean equivalents so a
                    // consumer never has to ask "element or attribute?".
                    "b" => style.weight = Some(700),
                    "i" => style.italic = Some(true),
                    _ => {}
                }
                if let Some(decl) = child.attr("style") {
                    style.overlay(&parse_declarations(decl));
                }
                walk(child, &style, paragraph, out);
            }
        }
    }
}

/// Strip an XML namespace prefix: `xhtml:body` -> `body`.
///
/// The parser keeps prefixes verbatim (it does not resolve namespaces),
/// and Table 224 permits a prefixed serialisation, so element matching
/// has to be prefix-insensitive or a legally-prefixed document reads as
/// entirely unknown markup.
fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// Parse a CSS declaration list — `name: value; name: value` — into a
/// [`Style`].
///
/// Used for BOTH `/DS` (which is a bare declaration list with no element
/// around it) and a `style=` attribute, because they are the same
/// grammar. Unknown properties are skipped, not rejected; see the module
/// docs.
fn parse_declarations(s: &str) -> Style {
    let mut style = Style::default();
    for decl in s.split(';') {
        let Some((name, value)) = decl.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match name.as_str() {
            "text-align" => style.align = parse_align(value),
            "vertical-align" => style.baseline_shift_pt = parse_pt(value),
            "font-size" => style.size_pt = parse_pt(value),
            "font-style" => style.italic = parse_italic(value),
            "font-weight" => style.weight = parse_weight(value),
            "font-family" => style.family = parse_family(value),
            "font" => apply_font_shorthand(value, &mut style),
            "color" => style.color = parse_color(value),
            "text-decoration" => apply_text_decoration(value, &mut style),
            "font-stretch" => style.stretch = parse_stretch(value),
            // Not an error. A PDF 1.7 file may legally carry XFA-2.4
            // attributes this module does not model.
            _ => {}
        }
    }
    style
}

/// Table 225 `text-align`: `left`, `right`, `center`. Nothing else.
fn parse_align(v: &str) -> Option<Align> {
    match v.to_ascii_lowercase().as_str() {
        "left" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        _ => None,
    }
}

/// A `<decimal>pt` length, with an optional sign.
///
/// The `pt` suffix is optional here although Table 225's grammar states
/// it: a producer that omits it has written a number whose only possible
/// unit is points, and rejecting that would lose a size pdfcer could
/// honour. RT-A5 already records that Table 225's own `font` shorthand
/// example does not match its stated grammar, so exact-grammar strictness
/// would reject the specification's own example.
fn parse_pt(v: &str) -> Option<f64> {
    let t = v.trim();
    let num = t.strip_suffix("pt").unwrap_or(t).trim();
    num.parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Table 225 `font-style`: `normal` or `italic`.
fn parse_italic(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "italic" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

/// Table 225 `font-weight`: the keywords or the numeric scale.
///
/// *"`normal` is equivalent to `400`, and `bold` is equivalent to `700`"*
/// — so both spellings normalise to the number and no consumer handles
/// two representations of one value. Numbers outside the 100..=900 scale
/// are rejected rather than clamped: a clamp would silently invent a
/// weight the file did not ask for.
fn parse_weight(v: &str) -> Option<u16> {
    match v.to_ascii_lowercase().as_str() {
        "normal" => Some(400),
        "bold" => Some(700),
        n => n
            .parse::<u16>()
            .ok()
            .filter(|w| (100..=900).contains(w) && w % 100 == 0),
    }
}

/// Table 225 `font-family`: a comma-separated preference list.
///
/// Quotes are stripped — a family name containing a space is quoted in
/// CSS, and the quotes are syntax rather than part of the name. Order is
/// preserved because Table 225 makes it normative.
fn parse_family(v: &str) -> Vec<String> {
    v.split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).trim().to_owned())
        .filter(|f| !f.is_empty())
        .collect()
}

/// Table 225 `font` shorthand: `<font-style> <font-weight> <font-size>
/// <font-family>`.
///
/// Parsed positionally-but-tolerantly: each whitespace-separated token is
/// tested against style, weight and size in turn, and everything from the
/// first unrecognised token onward is the family list. That tolerance is
/// required, not generous — **RT-A5** records that Table 225's own
/// EXAMPLE, `font: 18pt Arial`, omits the style and weight its stated
/// grammar lists as leading. A strict positional parser rejects the
/// specification's own example.
fn apply_font_shorthand(v: &str, style: &mut Style) {
    let mut rest = v.trim();
    while !rest.is_empty() {
        let (tok, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if let Some(i) = parse_italic(tok) {
            style.italic = Some(i);
        } else if let Some(w) = parse_weight(tok) {
            style.weight = Some(w);
        } else if let Some(p) = parse_pt(tok) {
            style.size_pt = Some(p);
        } else {
            // First token that is none of the three: the family list runs
            // to the end, and may itself contain spaces and commas.
            style.family = parse_family(rest);
            return;
        }
        rest = tail.trim_start();
    }
}

/// Table 225 `text-decoration`: `underline` and/or `line-through`.
///
/// Both may appear in one declaration (CSS allows a space-separated
/// list), so this sets flags rather than returning one value. A keyword
/// that is present sets its flag `true`; absence leaves the flag `None`
/// rather than `false`, because "this declaration did not mention
/// underline" and "this declaration turned underline off" are different
/// and only the former is expressible in Table 225's vocabulary.
fn apply_text_decoration(v: &str, style: &mut Style) {
    for tok in v.split_whitespace() {
        match tok.to_ascii_lowercase().as_str() {
            "underline" => style.underline = Some(true),
            "line-through" => style.strikethrough = Some(true),
            "none" => {
                style.underline = Some(false);
                style.strikethrough = Some(false);
            }
            _ => {}
        }
    }
}

/// Table 225 `font-stretch`: the nine-step scale.
fn parse_stretch(v: &str) -> Option<Stretch> {
    match v.to_ascii_lowercase().as_str() {
        "ultra-condensed" => Some(Stretch::UltraCondensed),
        "extra-condensed" => Some(Stretch::ExtraCondensed),
        "condensed" => Some(Stretch::Condensed),
        "semi-condensed" => Some(Stretch::SemiCondensed),
        "normal" => Some(Stretch::Normal),
        "semi-expanded" => Some(Stretch::SemiExpanded),
        "expanded" => Some(Stretch::Expanded),
        "extra-expanded" => Some(Stretch::ExtraExpanded),
        "ultra-expanded" => Some(Stretch::UltraExpanded),
        _ => None,
    }
}

/// Table 225 `color`: `#rrggbb` or `rgb(rrr,ggg,bbb)`, returned as
/// DeviceRGB components in `0.0..=1.0`.
///
/// **The conversion is RT-M12, a `shall`**: the written values are sRGB,
/// and they *"shall be transformed into values in a non-ICC based colour
/// space when used to generate the annotation's appearance."* Performing
/// it here rather than at the call site means no consumer can forget it.
///
/// The transform itself is the componentwise 0-255 -> 0.0-1.0 scaling
/// that DeviceRGB uses; §8.6.4.3's DeviceRGB is precisely a non-ICC
/// space, so this satisfies the `shall` without a colour-management step.
/// Three-digit `#rgb` is NOT accepted: Table 225 says *"a 2-digit
/// hexadecimal value for each component"*, and the CSS shorthand is
/// outside the grammar the spec pins.
fn parse_color(v: &str) -> Option<[f64; 3]> {
    let t = v.trim();
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let c = |i: usize| -> f64 {
            f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0
        };
        return Some([c(0), c(2), c(4)]);
    }
    let inner = t
        .strip_prefix("rgb(")
        .or_else(|| t.strip_prefix("RGB("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut out = [0.0f64; 3];
    for (slot, p) in out.iter_mut().zip(parts) {
        *slot = f64::from(p.trim().parse::<u8>().ok()?) / 255.0;
    }
    Some(out)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// The `<body>` opening every fixture needs, per Table 224.
    fn body(inner: &str) -> String {
        format!(
            "<body xmlns='http://www.w3.org/1999/xhtml' \
             xmlns:xfa='http://www.xfa.org/schema/xfa-data/1.0/' xfa:spec='2.4'>\
             {inner}</body>"
        )
    }

    /// Mixed content splits into runs in document order, spaces intact.
    ///
    /// The spaces are the assertion that matters. Trim the text runs and
    /// `<b>bold</b> world` becomes `boldworld` — the failure the ordered
    /// `XmlNode` model was added to make impossible.
    #[test]
    fn mixed_content_becomes_ordered_runs_with_spaces_kept() {
        let runs = parse(&body("<p>Hello <b>bold</b> world</p>"), None).expect("parses");
        let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, vec!["Hello ", "bold", " world"]);
        assert_eq!(runs[0].style.weight, None);
        assert_eq!(runs[1].style.weight, Some(700));
        assert_eq!(runs[2].style.weight, None, "bold must not leak past </b>");
    }

    /// `/DS` supplies defaults; a run's own declaration overrides them.
    ///
    /// RT-M6. Both halves are asserted in one document because a parser
    /// that ignored `/DS` entirely and one that let `/DS` win over the run
    /// would each pass a test checking only the other.
    #[test]
    fn ds_defaults_apply_and_a_run_overrides_them() {
        let rv = body("<p>base <span style='font-size:20pt'>big</span></p>");
        let runs = parse(&rv, Some("font-size: 10pt; color: #FF0000")).expect("parses");
        assert_eq!(runs[0].style.size_pt, Some(10.0));
        assert_eq!(runs[1].style.size_pt, Some(20.0));
        // The colour comes from /DS and survives into BOTH runs.
        assert_eq!(runs[0].style.color, Some([1.0, 0.0, 0.0]));
        assert_eq!(runs[1].style.color, Some([1.0, 0.0, 0.0]));
    }

    /// Nested elements cascade, and the inner one wins.
    #[test]
    fn nested_elements_cascade_inner_wins() {
        let rv = body("<p><b><i>both</i></b></p>");
        let runs = parse(&rv, None).expect("parses");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style.weight, Some(700));
        assert_eq!(runs[0].style.italic, Some(true));
    }

    /// An explicit attribute beats the element that contains it.
    ///
    /// `<b style="font-weight:normal">` is contradictory markup, and the
    /// nearer-the-text rule settles it. Without this ordering the element
    /// would silently win and the file's own explicit instruction would be
    /// ignored.
    #[test]
    fn an_explicit_attribute_beats_its_element() {
        let runs = parse(&body("<p><b style='font-weight:normal'>x</b></p>"), None).unwrap();
        assert_eq!(runs[0].style.weight, Some(400));
    }

    /// Paragraph indices count breaks, and the FIRST `<p>` is not one.
    #[test]
    fn paragraphs_are_numbered_from_zero() {
        let runs = parse(&body("<p>one</p><p>two</p><p>three</p>"), None).unwrap();
        let paras: Vec<usize> = runs.iter().map(|r| r.paragraph).collect();
        assert_eq!(paras, vec![0, 1, 2]);
    }

    /// Unknown markup contributes no style and KEEPS its text.
    ///
    /// The asymmetry the module docs argue for: a missed style is visible
    /// and cosmetic, a dropped run is content that vanished.
    #[test]
    fn unknown_markup_keeps_its_text() {
        let runs = parse(&body("<p>a<blink>b</blink>c</p>"), None).unwrap();
        let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "abc");
    }

    /// An unknown style property does not fail the parse or the sibling
    /// properties in the same declaration.
    #[test]
    fn an_unknown_property_does_not_discard_its_neighbours() {
        let rv = body("<p style='letter-spacing:2pt; font-size:14pt'>x</p>");
        let runs = parse(&rv, None).unwrap();
        assert_eq!(runs[0].style.size_pt, Some(14.0));
    }

    /// Both colour spellings convert to DeviceRGB, and `#rgb` does not.
    #[test]
    fn colour_parses_both_spellings_into_devicergb() {
        assert_eq!(parse_color("#00FF80"), Some([0.0, 1.0, 128.0 / 255.0]));
        assert_eq!(
            parse_color("rgb(0, 255, 128)"),
            Some([0.0, 1.0, 128.0 / 255.0])
        );
        // Table 225 says two hex digits per component; the CSS shorthand
        // is outside the grammar.
        assert_eq!(parse_color("#0f8"), None);
        assert_eq!(parse_color("rebeccapurple"), None);
    }

    /// The `font` shorthand accepts Table 225's own EXAMPLE.
    ///
    /// RT-A5: the spec's example `font: 18pt Arial` omits the style and
    /// weight its stated grammar lists first. A strict positional parser
    /// rejects the specification's own example, which is why this one is
    /// tolerant.
    #[test]
    fn the_font_shorthand_accepts_the_specs_own_example() {
        let runs = parse(&body("<p style='font: 18pt Arial'>x</p>"), None).unwrap();
        assert_eq!(runs[0].style.size_pt, Some(18.0));
        assert_eq!(runs[0].style.family, vec!["Arial".to_owned()]);

        // And the full form still works.
        let runs = parse(
            &body("<p style='font: italic bold 9pt \"Times New Roman\", serif'>x</p>"),
            None,
        )
        .unwrap();
        assert_eq!(runs[0].style.italic, Some(true));
        assert_eq!(runs[0].style.weight, Some(700));
        assert_eq!(runs[0].style.size_pt, Some(9.0));
        assert_eq!(
            runs[0].style.family,
            vec!["Times New Roman".to_owned(), "serif".to_owned()]
        );
    }

    /// Weight keywords normalise to numbers; off-scale numbers are refused.
    #[test]
    fn weight_normalises_and_refuses_off_scale_values() {
        assert_eq!(parse_weight("normal"), Some(400));
        assert_eq!(parse_weight("bold"), Some(700));
        assert_eq!(parse_weight("300"), Some(300));
        // Not on the 100-step scale: refused rather than clamped, because
        // clamping invents a weight the file did not ask for.
        assert_eq!(parse_weight("350"), None);
        assert_eq!(parse_weight("1000"), None);
    }

    /// `vertical-align`'s sign is the spec's: positive is SUPERscript.
    #[test]
    fn vertical_align_keeps_the_specs_sign_convention() {
        let runs = parse(
            &body("<p><span style='vertical-align:4pt'>up</span></p>"),
            None,
        )
        .unwrap();
        assert_eq!(runs[0].style.baseline_shift_pt, Some(4.0));
        let runs = parse(
            &body("<p><span style='vertical-align:-3pt'>down</span></p>"),
            None,
        )
        .unwrap();
        assert_eq!(runs[0].style.baseline_shift_pt, Some(-3.0));
    }

    /// A non-`<body>` root is refused rather than guessed at.
    #[test]
    fn a_non_body_root_is_refused() {
        let err = parse("<div><p>x</p></div>", None).unwrap_err();
        assert!(matches!(err, RichTextError::RootNotBody { .. }), "{err:?}");
    }

    /// A namespace-prefixed serialisation is still recognised.
    ///
    /// The XML reader keeps prefixes verbatim, so without prefix-stripping
    /// a legally-prefixed document reads as entirely unknown markup and
    /// silently loses every style in it.
    #[test]
    fn a_prefixed_serialisation_is_recognised() {
        let rv = "<xhtml:body xmlns:xhtml='http://www.w3.org/1999/xhtml'>\
                  <xhtml:p><xhtml:b>x</xhtml:b></xhtml:p></xhtml:body>";
        let runs = parse(rv, None).expect("a prefixed body is still a body");
        assert_eq!(runs[0].style.weight, Some(700));
    }

    /// Malformed XML is an error, not a silent empty result.
    #[test]
    fn malformed_xml_is_an_error() {
        assert!(matches!(
            parse("<body><p>unclosed", None),
            Err(RichTextError::MalformedXml(_))
        ));
    }
}
