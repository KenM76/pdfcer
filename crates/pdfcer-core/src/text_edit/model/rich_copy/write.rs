//! [`RichText`] serialised as plain text, HTML, Windows `CF_HTML` and RTF.

use std::fmt::Write as _;

use super::{
    RICH_COPY_MAX_OUTPUT_BYTES, RICH_COPY_MAX_PARAGRAPHS, RICH_COPY_MAX_SPANS,
    RICH_COPY_MAX_TEXT_BYTES, RichParagraph, RichSpan, RichText, SpanStyle, font_size_pt, space_pt,
};

pub(super) const START_FRAGMENT: &str = "<!--StartFragment-->";
pub(super) const END_FRAGMENT: &str = "<!--EndFragment-->";

/// Most characters of a font family written; a longer name is cut.
pub(super) const MAX_FAMILY_CHARS: usize = 64;
/// Upper bound on the bytes one run adds beyond its text, in either format:
/// its HTML `<span>` with every style and a [`MAX_FAMILY_CHARS`] family, or
/// its RTF group plus its own font-table and colour-table entries.
/// `worst_span_fits_its_bound` measures it.
pub(super) const SPAN_BYTES_MAX: usize = 2048;
/// Most output bytes one input text byte becomes (`&nbsp;`, `\line `).
const TEXT_EXPANSION_MAX: usize = 6;
/// Upper bound on one paragraph's markup without its runs.
const PARAGRAPH_BYTES_MAX: usize = 128;
/// Upper bound on a document's fixed head and tail, `CF_HTML` header
/// included.
const DOCUMENT_BYTES_MAX: usize = 1024;
/// The writer checks [`WRITE_LIMIT`] before each paragraph, run and
/// character, so one run's markup and the closing tail may follow the last
/// check; this much is kept free for them.
const RESERVE: usize = 64 * 1024;
const WRITE_LIMIT: usize = RICH_COPY_MAX_OUTPUT_BYTES - RESERVE;

// A copy within the paragraph, run and text caps never reaches the limit.
const _: () = assert!(
    RICH_COPY_MAX_SPANS * SPAN_BYTES_MAX
        + RICH_COPY_MAX_TEXT_BYTES * TEXT_EXPANSION_MAX
        + RICH_COPY_MAX_PARAGRAPHS * PARAGRAPH_BYTES_MAX
        + DOCUMENT_BYTES_MAX
        <= WRITE_LIMIT
);

impl RichText {
    /// The text with paragraphs separated by `\n`, a wrapped line joined by
    /// a space, and a blank paragraph as an empty line.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let paras: Vec<String> = self
            .paragraphs
            .iter()
            .map(|p| p.spans.iter().map(|s| s.text.as_str()).collect())
            .collect();
        paras.join("\n")
    }

    /// A UTF-8 HTML document, one `<p>` per paragraph with inline styles,
    /// the body between `<!--StartFragment-->` and `<!--EndFragment-->`.
    #[must_use]
    pub fn to_html(&self) -> String {
        let mut out = String::from("<html><head><meta charset=\"utf-8\"></head><body>\n");
        out.push_str(START_FRAGMENT);
        for (p, spans) in capped(&self.paragraphs) {
            if full(&out) {
                break;
            }
            html_paragraph(&mut out, p, spans);
        }
        out.push_str(END_FRAGMENT);
        out.push_str("\n</body></html>\n");
        out
    }

    /// [`Self::to_html`] wrapped in the Windows `CF_HTML` clipboard header
    /// (`Version:0.9` with `StartHTML`/`EndHTML`/`StartFragment`/
    /// `EndFragment` byte offsets), ready to place on the clipboard under
    /// the registered format name `HTML Format`.
    #[must_use]
    pub fn to_cf_html(&self) -> String {
        let html = self.to_html();
        let header_len = cf_header(0, 0, 0, 0).len();
        let frag_start = html
            .find(START_FRAGMENT)
            .map_or(0, |i| i + START_FRAGMENT.len());
        let frag_end = html.find(END_FRAGMENT).unwrap_or(html.len());
        let mut out = cf_header(
            header_len,
            header_len + html.len(),
            header_len + frag_start,
            header_len + frag_end,
        );
        out.push_str(&html);
        out
    }

    /// An RTF 1.x document: a font table of the families used, a colour
    /// table, one `\par` per paragraph and `\line` for a break inside one.
    /// Non-ASCII characters are written as `\uN?`.
    #[must_use]
    pub fn to_rtf(&self) -> String {
        let mut fonts: Vec<&str> = Vec::new();
        let mut colors: Vec<[u8; 3]> = Vec::new();
        for s in capped(&self.paragraphs).flat_map(|(_, spans)| spans) {
            if let Some(f) = s.style.family.as_deref()
                && !fonts.contains(&f)
            {
                fonts.push(f);
            }
            if let Some(c) = s.style.color
                && !colors.contains(&c)
            {
                colors.push(c);
            }
        }
        let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl");
        if fonts.is_empty() {
            out.push_str("{\\f0\\froman Times New Roman;}");
        }
        for (i, f) in fonts.iter().enumerate() {
            let _ = write!(out, "{{\\f{i}\\fnil ");
            rtf_font_name(&mut out, f);
            out.push_str(";}");
        }
        out.push_str("}{\\colortbl;");
        for [r, g, b] in &colors {
            let _ = write!(out, "\\red{r}\\green{g}\\blue{b};");
        }
        out.push_str("}\n");
        for (p, spans) in capped(&self.paragraphs) {
            if full(&out) {
                break;
            }
            rtf_paragraph(&mut out, p, spans, &fonts, &colors);
        }
        out.push('}');
        out
    }
}

fn cf_header(start_html: usize, end_html: usize, start_frag: usize, end_frag: usize) -> String {
    format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_frag:010}\r\nEndFragment:{end_frag:010}\r\n"
    )
}

/// Whether `out` has reached the writer's limit.
fn full(out: &str) -> bool {
    out.len() >= WRITE_LIMIT
}

/// The paragraphs a writer serialises, each with the runs of it written: at
/// most [`RICH_COPY_MAX_PARAGRAPHS`], and [`RICH_COPY_MAX_SPANS`] runs in all.
/// A paragraph whose runs are all past the run cap ends the sequence.
fn capped(paragraphs: &[RichParagraph]) -> impl Iterator<Item = (&RichParagraph, &[RichSpan])> {
    let mut left = RICH_COPY_MAX_SPANS;
    paragraphs
        .iter()
        .take(RICH_COPY_MAX_PARAGRAPHS)
        .map_while(move |p| {
            if left == 0 && !p.spans.is_empty() {
                return None;
            }
            let n = p.spans.len().min(left);
            left -= n;
            Some((p, p.spans.get(..n).unwrap_or_default()))
        })
}

/// A C0 or C1 control character other than tab and line feed: never
/// written as text (a NUL ends the text for some clipboard readers).
fn is_dropped(c: char) -> bool {
    c.is_control() && c != '\t' && c != '\n'
}

fn html_paragraph(out: &mut String, p: &RichParagraph, spans: &[RichSpan]) {
    let _ = write!(
        out,
        "<p style=\"margin:0 0 {}pt 0",
        num(space_pt(p.space_after))
    );
    if spans.is_empty() {
        let _ = writeln!(
            out,
            ";font-size:{}pt\">&nbsp;</p>",
            num(font_size_pt(p.size))
        );
        return;
    }
    out.push_str("\">");
    for s in spans {
        if full(out) {
            break;
        }
        html_span(out, s);
    }
    out.push_str("</p>\n");
}

fn html_span(out: &mut String, s: &RichSpan) {
    let st = &s.style;
    out.push_str("<span style=\"");
    if let Some(f) = &st.family {
        out.push_str("font-family:'");
        css_family(out, f);
        out.push_str("';");
    }
    let _ = write!(out, "font-size:{}pt", num(font_size_pt(st.size)));
    if st.bold {
        out.push_str(";font-weight:bold");
    }
    if st.italic {
        out.push_str(";font-style:italic");
    }
    match (st.underline, st.strikethrough) {
        (true, true) => out.push_str(";text-decoration:underline line-through"),
        (true, false) => out.push_str(";text-decoration:underline"),
        (false, true) => out.push_str(";text-decoration:line-through"),
        (false, false) => {}
    }
    if let Some([r, g, b]) = st.color {
        let _ = write!(out, ";color:#{r:02x}{g:02x}{b:02x}");
    }
    out.push_str("\">");
    html_text(out, &s.text);
    out.push_str("</span>");
}

/// `family` as the body of a single-quoted CSS string inside a
/// double-quoted `style` attribute. Control characters are dropped and the
/// name cut at [`MAX_FAMILY_CHARS`]; letters, digits, `-`, `_`, `.`, space
/// and non-ASCII are written as they are, and every other ASCII character
/// (quotes, `\`, `&`, `<`, `>`, `;`) as a CSS hex escape `\HH ` with its
/// terminating space (CSS Syntax Level 3 §4.3.7, §2.1 serialisation), so
/// neither the string nor the attribute can end early.
fn css_family(out: &mut String, family: &str) {
    for c in family
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_FAMILY_CHARS)
    {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') || !c.is_ascii() {
            out.push(c);
        } else {
            let _ = write!(out, "\\{:x} ", u32::from(c));
        }
    }
}

/// Escape for HTML text; `\n` becomes `<br>`, a run of spaces keeps its
/// width with `&nbsp;`, and [`is_dropped`] characters are left out. Stops
/// at the writer's limit.
fn html_text(out: &mut String, text: &str) {
    let mut prev_space = false;
    for c in text.chars() {
        if full(out) {
            return;
        }
        match c {
            c if is_dropped(c) => continue,
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br>"),
            ' ' if prev_space => out.push_str("&nbsp;"),
            c => out.push(c),
        }
        prev_space = c == ' ';
    }
}

fn rtf_paragraph(
    out: &mut String,
    p: &RichParagraph,
    spans: &[RichSpan],
    fonts: &[&str],
    colors: &[[u8; 3]],
) {
    let _ = write!(out, "\\pard\\sa{}", twips(p.space_after));
    if spans.is_empty() {
        let _ = writeln!(out, "\\fs{}\\par", half_points(p.size));
        return;
    }
    for s in spans {
        if full(out) {
            break;
        }
        out.push('{');
        rtf_style(out, &s.style, fonts, colors);
        out.push(' ');
        rtf_text(out, &s.text);
        out.push('}');
    }
    out.push_str("\\par\n");
}

fn rtf_style(out: &mut String, st: &SpanStyle, fonts: &[&str], colors: &[[u8; 3]]) {
    if let Some(i) = st
        .family
        .as_deref()
        .and_then(|f| fonts.iter().position(|&x| x == f))
    {
        let _ = write!(out, "\\f{i}");
    }
    let _ = write!(out, "\\fs{}", half_points(st.size));
    for (on, word) in [
        (st.bold, "\\b"),
        (st.italic, "\\i"),
        (st.underline, "\\ul"),
        (st.strikethrough, "\\strike"),
    ] {
        if on {
            out.push_str(word);
        }
    }
    if let Some(i) = st.color.and_then(|c| colors.iter().position(|&x| x == c)) {
        let _ = write!(out, "\\cf{}", i + 1);
    }
}

/// A font-table name: [`rtf_text`] without control characters or `;` (which
/// ends a font-table entry, RTF 1.9.1 `\fonttbl`), cut at
/// [`MAX_FAMILY_CHARS`], so the table keeps one entry per font.
fn rtf_font_name(out: &mut String, family: &str) {
    let name: String = family
        .chars()
        .filter(|&c| !c.is_control() && c != ';')
        .take(MAX_FAMILY_CHARS)
        .collect();
    rtf_text(out, &name);
}

/// Escape for RTF text: `\ { }` escaped, `\n` as `\line`, `\t` as `\tab`,
/// [`is_dropped`] characters left out, non-ASCII as signed 16-bit `\uN?`
/// per UTF-16 unit. Stops at the writer's limit.
fn rtf_text(out: &mut String, text: &str) {
    for c in text.chars() {
        if full(out) {
            return;
        }
        match c {
            c if is_dropped(c) => {}
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\line "),
            '\t' => out.push_str("\\tab "),
            c if c.is_ascii() => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for &u in c.encode_utf16(&mut units).iter() {
                    #[allow(clippy::cast_possible_wrap)] // RTF \u is signed 16-bit
                    let _ = write!(out, "\\u{}?", u as i16);
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation)] // space_pt bounds it to 0..=32760
fn twips(pt: f32) -> i32 {
    (space_pt(pt) * 20.0).round() as i32
}

#[allow(clippy::cast_possible_truncation)] // font_size_pt bounds it to 2..=3276
fn half_points(pt: f32) -> i32 {
    (font_size_pt(pt) * 2.0).round() as i32
}

/// A number with at most one decimal and no trailing `.0`; `0` for a
/// value that is not finite (callers pass clamped values).
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}
