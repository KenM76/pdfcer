//! [`RichText`] serialised as plain text, HTML, Windows `CF_HTML` and RTF.

use std::fmt::Write as _;

use super::{RichParagraph, RichSpan, RichText, SpanStyle};

const START_FRAGMENT: &str = "<!--StartFragment-->";
const END_FRAGMENT: &str = "<!--EndFragment-->";

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
        for p in &self.paragraphs {
            html_paragraph(&mut out, p);
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
        for s in self.paragraphs.iter().flat_map(|p| &p.spans) {
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
            rtf_text(&mut out, f);
            out.push_str(";}");
        }
        out.push_str("}{\\colortbl;");
        for [r, g, b] in &colors {
            let _ = write!(out, "\\red{r}\\green{g}\\blue{b};");
        }
        out.push_str("}\n");
        for p in &self.paragraphs {
            rtf_paragraph(&mut out, p, &fonts, &colors);
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

fn html_paragraph(out: &mut String, p: &RichParagraph) {
    let _ = write!(out, "<p style=\"margin:0 0 {}pt 0", num(p.space_after));
    if p.spans.is_empty() {
        let _ = writeln!(out, ";font-size:{}pt\">&nbsp;</p>", num(p.size));
        return;
    }
    out.push_str("\">");
    for s in &p.spans {
        html_span(out, s);
    }
    out.push_str("</p>\n");
}

fn html_span(out: &mut String, s: &RichSpan) {
    let st = &s.style;
    out.push_str("<span style=\"");
    if let Some(f) = &st.family {
        out.push_str("font-family:'");
        html_text(out, &f.replace('\'', ""));
        out.push_str("';");
    }
    let _ = write!(out, "font-size:{}pt", num(st.size));
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

/// Escape for HTML text; `\n` becomes `<br>` and a run of spaces keeps its
/// width with `&nbsp;`.
fn html_text(out: &mut String, text: &str) {
    let mut prev_space = false;
    for c in text.chars() {
        match c {
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

fn rtf_paragraph(out: &mut String, p: &RichParagraph, fonts: &[&str], colors: &[[u8; 3]]) {
    let _ = write!(out, "\\pard\\sa{}", twips(p.space_after));
    if p.spans.is_empty() {
        let _ = writeln!(out, "\\fs{}\\par", half_points(p.size));
        return;
    }
    for s in &p.spans {
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

/// Escape for RTF text: `\ { }` escaped, `\n` as `\line`, non-ASCII as
/// signed 16-bit `\uN?` per UTF-16 unit.
fn rtf_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\line "),
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

#[allow(clippy::cast_possible_truncation)] // a paragraph space or font size
fn twips(pt: f32) -> i32 {
    (pt * 20.0).round() as i32
}

#[allow(clippy::cast_possible_truncation)] // a font size
fn half_points(pt: f32) -> i32 {
    (pt * 2.0).round().max(2.0) as i32
}

/// A number with at most one decimal and no trailing `.0`.
fn num(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}
