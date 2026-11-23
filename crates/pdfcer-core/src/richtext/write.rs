//! Styled [`Run`]s to an `/RV` rich text string — the inverse of
//! [`super::parse`] (ISO 32000-1 §12.7.3.4, Tables 223–225).

use std::fmt::Write as _;

use super::{Align, MAX_RICH_BYTES, Run, Stretch, Style};

/// Table 224 `xmlns`. The XFA binding keeps the trailing slash of the
/// clause's EXAMPLE and of XFA itself (erratum E5: the table omits it).
const BODY_NAMESPACES: &str = concat!(
    "xmlns=\"http://www.w3.org/1999/xhtml\" ",
    "xmlns:xfa=\"http://www.xfa.org/schema/xfa-data/1.0/\""
);

/// The most blank paragraphs one call writes. A run list whose paragraph
/// gaps need more would produce a string over [`MAX_RICH_BYTES`], which no
/// fill accepts; the bound keeps a hostile index (`usize::MAX`) from
/// allocating without limit.
const MAX_BLANK_PARAGRAPHS: usize = MAX_RICH_BYTES / "<p></p>".len();

/// Write `runs` as an XHTML rich text string (§12.7.3.4) — a `/RV` value.
///
/// One `<p>` per paragraph, one `<span style="…">` per styled run, every
/// set [`Style`] attribute written as its Table 225 declaration. A
/// paragraph whose runs all share one `text-align` carries it on the `<p>`,
/// where other readers look for it. The `<body>` carries the four Table 224
/// attributes; `xfa:spec` is written `2.4` (RT-A1 emission policy, the
/// PDF 1.7 binding), while the markup itself uses only the PDF 1.5 grammar
/// every later level includes.
///
/// # Round trip
///
/// `parse(&to_xhtml(&runs), None) == Ok(runs)` for any `runs` already in
/// the normal form below, which every [`super::parse`] result is in — so
/// `parse(to_xhtml(parse(x)))` equals `parse(x)`. Input outside it is
/// written as its normal form:
///
/// - **Empty runs** are dropped (`parse` never returns one).
/// - **Paragraph indices** are renumbered from 0; a gap is kept as blank
///   `<p></p>`s, and a decrease starts the next paragraph.
/// - **`font-weight`** off Table 225's 100..=900 step-100 scale, and a
///   non-finite size or baseline shift, are omitted.
/// - **`color`** is quantised to `#rrggbb`, components clamped to
///   `0.0..=1.0`; a non-finite component omits the colour.
/// - **`font-family`** names lose `,` and `;` (the list and declaration
///   separators) and surrounding whitespace and quotes; empty names are
///   dropped.
/// - **Decoration**: an explicit `false` beside an unset partner reads
///   back as both `false` — `text-decoration: none` is the only way to say
///   "off", and it says it for both.
///
/// Adjacent runs with equal styles stay separate runs: each is its own
/// `<span>`, so run boundaries the caller chose survive.
///
/// Characters XML 1.0 forbids (C0 controls other than tab and line feed,
/// U+FFFE, U+FFFF) and carriage return are written as character
/// references, so no text is lost; [`super::parse`] decodes them, a strict
/// XML 1.0 reader may refuse all but `&#xD;`.
///
/// # Examples
///
/// ```
/// use pdfcer_core::richtext::{self, Run, Style};
///
/// let runs = vec![
///     Run { text: "plain ".into(), style: Style::default(), paragraph: 0 },
///     Run {
///         text: "bold & red".into(),
///         style: Style { weight: Some(700), color: Some([1.0, 0.0, 0.0]), ..Style::default() },
///         paragraph: 0,
///     },
/// ];
/// let rv = richtext::to_xhtml(&runs);
/// assert!(rv.contains("<span style=\"font-weight:700; color:#FF0000\">bold &amp; red</span>"));
/// assert_eq!(richtext::parse(&rv, None)?, runs);
/// # Ok::<(), richtext::RichTextError>(())
/// ```
#[must_use]
pub fn to_xhtml(runs: &[Run]) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\"?><body {BODY_NAMESPACES} xfa:contentType=\"text/html\" \
         xfa:APIVersion=\"pdfcer:{}\" xfa:spec=\"2.4\">",
        env!("CARGO_PKG_VERSION")
    );
    let mut blanks_left = MAX_BLANK_PARAGRAPHS;
    for (gap, para) in paragraphs(runs) {
        let blanks = gap.min(blanks_left);
        blanks_left -= blanks;
        for _ in 0..blanks {
            out.push_str("<p></p>");
        }
        write_paragraph(&mut out, &para);
    }
    out.push_str("</body>");
    out
}

/// The non-empty runs grouped into paragraphs, each with the number of
/// blank paragraphs before it.
fn paragraphs(runs: &[Run]) -> Vec<(usize, Vec<&Run>)> {
    let mut out: Vec<(usize, Vec<&Run>)> = Vec::new();
    let mut last = None;
    for run in runs.iter().filter(|r| !r.text.is_empty()) {
        match (last, out.last_mut()) {
            (Some(p), Some((_, para))) if p == run.paragraph => para.push(run),
            (Some(p), _) => {
                let gap = run.paragraph.saturating_sub(p).saturating_sub(1);
                out.push((gap, vec![run]));
            }
            (None, _) => out.push((0, vec![run])),
        }
        last = Some(run.paragraph);
    }
    out
}

fn write_paragraph(out: &mut String, runs: &[&Run]) {
    let shared = runs
        .first()
        .and_then(|r| r.style.align)
        .filter(|a| runs.iter().all(|r| r.style.align == Some(*a)));
    out.push_str("<p");
    if let Some(a) = shared {
        let _ = write!(out, " style=\"text-align:{}\"", align_name(a));
    }
    out.push('>');
    // A bare text node after another would merge with it on reading, so
    // only a run following markup may go bare.
    let mut prev_bare = false;
    for run in runs {
        let decls = declarations(&run.style, shared.is_none());
        if decls.is_empty() && !prev_bare {
            escape(out, &run.text, false);
            prev_bare = true;
            continue;
        }
        out.push_str("<span");
        if !decls.is_empty() {
            out.push_str(" style=\"");
            escape(out, &decls, true);
            out.push('"');
        }
        out.push('>');
        escape(out, &run.text, false);
        out.push_str("</span>");
        prev_bare = false;
    }
    out.push_str("</p>");
}

/// `style`'s Table 225 declarations, `; `-joined; `text-align` only when
/// `with_align`.
fn declarations(style: &Style, with_align: bool) -> String {
    let mut d: Vec<String> = Vec::new();
    if let Some(a) = style.align.filter(|_| with_align) {
        d.push(format!("text-align:{}", align_name(a)));
    }
    if let Some(v) = style.baseline_shift_pt.filter(|v| v.is_finite()) {
        d.push(format!("vertical-align:{v}pt"));
    }
    if let Some(v) = style.size_pt.filter(|v| v.is_finite()) {
        d.push(format!("font-size:{v}pt"));
    }
    if let Some(i) = style.italic {
        d.push(format!(
            "font-style:{}",
            if i { "italic" } else { "normal" }
        ));
    }
    if let Some(w) = style
        .weight
        .filter(|w| (100..=900).contains(w) && w % 100 == 0)
    {
        d.push(format!("font-weight:{w}"));
    }
    if let Some(f) = family_list(&style.family) {
        d.push(format!("font-family:{f}"));
    }
    if let Some(c) = style.color.and_then(hex_color) {
        d.push(format!("color:{c}"));
    }
    d.extend(decoration(style.underline, style.strikethrough));
    if let Some(s) = style.stretch {
        d.push(format!("font-stretch:{}", stretch_name(s)));
    }
    d.join("; ")
}

/// The `text-decoration` declarations for an underline/line-through pair.
/// `none` turns both off, so one "off" beside one "on" is `none` followed
/// by the "on" — read in order, the second overrides only its own flag.
fn decoration(underline: Option<bool>, strike: Option<bool>) -> Vec<String> {
    let td = |v: &str| format!("text-decoration:{v}");
    match (underline, strike) {
        (None, None) => vec![],
        (Some(true), None) => vec![td("underline")],
        (None, Some(true)) => vec![td("line-through")],
        (Some(true), Some(true)) => vec![td("underline line-through")],
        (Some(true), Some(false)) => vec![td("none"), td("underline")],
        (Some(false), Some(true)) => vec![td("none"), td("line-through")],
        (Some(false), _) | (_, Some(false)) => vec![td("none")],
    }
}

/// The family list as a CSS value, each name quoted; `None` when no name
/// survives sanitising.
fn family_list(family: &[String]) -> Option<String> {
    let names: Vec<String> = family
        .iter()
        .map(|f| {
            let kept: String = f.chars().filter(|c| !matches!(c, ',' | ';')).collect();
            kept.trim().trim_matches(['"', '\'']).trim().to_owned()
        })
        .filter(|f| !f.is_empty())
        .map(|f| {
            let q = if f.contains('\'') { '"' } else { '\'' };
            format!("{q}{f}{q}")
        })
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// Table 225 `#rrggbb`, the components scaled back to 0..=255.
fn hex_color(c: [f64; 3]) -> Option<String> {
    if !c.iter().all(|v| v.is_finite()) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0.0..=255.0 and rounded first"
    )]
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let [r, g, b] = c;
    Some(format!("#{:02X}{:02X}{:02X}", byte(r), byte(g), byte(b)))
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn stretch_name(s: Stretch) -> &'static str {
    match s {
        Stretch::UltraCondensed => "ultra-condensed",
        Stretch::ExtraCondensed => "extra-condensed",
        Stretch::Condensed => "condensed",
        Stretch::SemiCondensed => "semi-condensed",
        Stretch::Normal => "normal",
        Stretch::SemiExpanded => "semi-expanded",
        Stretch::Expanded => "expanded",
        Stretch::ExtraExpanded => "extra-expanded",
        Stretch::UltraExpanded => "ultra-expanded",
    }
}

/// Append `s` XML-escaped; `attr` also escapes `"` and the whitespace an
/// attribute-value normaliser would turn into spaces.
fn escape(out: &mut String, s: &str, attr: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\t' | '\n' if attr => {
                let _ = write!(out, "&#x{:X};", u32::from(c));
            }
            '\t' | '\n' => out.push(c),
            '\r' | '\u{0}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => {
                let _ = write!(out, "&#x{:X};", u32::from(c));
            }
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
