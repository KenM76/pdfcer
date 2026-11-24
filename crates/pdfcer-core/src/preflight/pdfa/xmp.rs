//! PDF/A metadata rows: the catalog XMP stream and its packet header
//! (ISO 19005-1 §6.7.2, §6.7.5; -2/-3 §6.6.2.1), the identification schema
//! (-1 §6.7.11, -2/-3 §6.6.4) and, at 1b, Info-to-XMP agreement (-1 §6.7.3).
//!
//! The packet is read as text, not parsed as RDF: a property is found in
//! attribute form (`pdfaid:part="2"`) or element form, with `rdf:li` items
//! for arrays. Well-formedness (MD-5) is not checked.

use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::preflight::checks::Sink;
use crate::preflight::cos::{catalog, decoded};
use crate::view::DocumentView;

use super::Ctx;

/// The PDF/A identification schema namespace (-1 §6.7.11, -2/-3 §6.6.4).
const PDFAID_NS: &str = "http://www.aiim.org/pdfa/ns/id/";

/// Info keys and the XMP property each must match at 1b (-1 §6.7.3
/// Table 1), by row.
const INFO_MAP: [(&str, &[u8], &str); 8] = [
    ("PAI-MD-20", b"CreationDate", "xmp:CreateDate"),
    ("PAI-MD-21", b"Title", "dc:title"),
    ("PAI-MD-22", b"Author", "dc:creator"),
    ("PAI-MD-23", b"Subject", "dc:description"),
    ("PAI-MD-24", b"Keywords", "pdf:Keywords"),
    ("PAI-MD-25", b"Creator", "xmp:CreatorTool"),
    ("PAI-MD-26", b"Producer", "pdf:Producer"),
    ("PAI-MD-27", b"ModDate", "xmp:ModifyDate"),
];

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    if !ctx.row.id.starts_with("PAI-MD-") {
        return false;
    }
    let stream = catalog(view)
        .and_then(|c| c.get(b"Metadata"))
        .map(|o| view.resolve(o));
    let Some(Object::Stream(stream)) = stream else {
        if matches!(ctx.row.id, "PAI-MD-1" | "PAI-MD-15") {
            sink.push(ctx.doc("The catalog has no /Metadata stream."));
        }
        return true;
    };
    if ctx.row.id == "PAI-MD-2" {
        if stream.dict.get(b"Filter").is_some() {
            sink.push(ctx.doc("The catalog's metadata stream is filtered."));
        }
        return true;
    }
    if ctx.row.id == "PAI-MD-1" {
        return true;
    }
    let Some(bytes) = decoded(view, stream) else {
        sink.push(ctx.unread(None, "the catalog's metadata stream could not be decoded"));
        return true;
    };
    packet_row(view, ctx, &bytes, sink);
    true
}

fn packet_row(view: &DocumentView<'_>, ctx: &Ctx<'_>, bytes: &[u8], sink: &mut Sink) {
    if ctx.row.id == "PAI-MD-6" {
        if std::str::from_utf8(bytes).is_err() {
            sink.push(ctx.doc("The XMP packet is not UTF-8."));
        }
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    let m = match ctx.row.id {
        "PAI-MD-3" | "PAI-MD-4" => header_attribute(&text, ctx.row.id == "PAI-MD-3"),
        "PAI-MD-15" | "PAI-MD-16" | "PAI-MD-17" | "PAI-MD-18" => identification(ctx, &text),
        id => {
            info_row(view, ctx, id, &text, sink);
            None
        }
    };
    if let Some(m) = m {
        sink.push(ctx.doc(&m));
    }
}

/// MD-3 (`bytes`) or MD-4 (`encoding`) in the `<?xpacket begin` header.
fn header_attribute(text: &str, bytes: bool) -> Option<String> {
    let start = text.find("<?xpacket begin")?;
    let end = text[start..].find("?>").map_or(text.len(), |e| start + e);
    let header = &text[start..end];
    let attr = if bytes { "bytes" } else { "encoding" };
    let present = header
        .match_indices(attr)
        .any(|(at, _)| header[at + attr.len()..].trim_start().starts_with('='));
    present.then(|| format!("The xpacket header has a {attr} attribute."))
}

/// The prefix the document binds to `ns`, if any (`xmlns:p="ns"`).
fn prefix_of<'t>(text: &'t str, ns: &str) -> Option<&'t str> {
    text.match_indices("xmlns:").find_map(|(at, _)| {
        let rest = &text[at + 6..];
        let eq = rest.find('=')?;
        let value = rest[eq + 1..].trim_start();
        let quote = value.chars().next()?;
        let uri = value.get(1..)?.split(quote).next()?;
        (uri == ns).then(|| rest[..eq].trim())
    })
}

fn identification(ctx: &Ctx<'_>, text: &str) -> Option<String> {
    let prefix = prefix_of(text, PDFAID_NS);
    let part = property(text, "pdfaid:part").and_then(|p| p.into_iter().next());
    match ctx.row.id {
        "PAI-MD-15" => (prefix.is_none() || part.is_none())
            .then(|| "The XMP has no PDF/A identification (pdfaid:part).".to_owned()),
        "PAI-MD-16" => {
            let want = ctx.level.part().to_string();
            part.filter(|p| p.trim() != want).map(|p| {
                format!(
                    "pdfaid:part is \"{}\"; {} needs \"{want}\".",
                    p.trim(),
                    ctx.level.claim()
                )
            })
        }
        "PAI-MD-17" => {
            let allowed: &[&str] = if ctx.level.part() == 1 {
                &["A", "B"]
            } else {
                &["A", "B", "U"]
            };
            let conformance =
                property(text, "pdfaid:conformance").and_then(|p| p.into_iter().next());
            match conformance {
                None if part.is_some() => Some("The XMP has no pdfaid:conformance.".to_owned()),
                Some(c) if !allowed.contains(&c.trim()) => Some(format!(
                    "pdfaid:conformance is \"{}\"; {} allows {}.",
                    c.trim(),
                    ctx.level.standard(),
                    allowed.join(", ")
                )),
                _ => None,
            }
        }
        _ => prefix.filter(|p| *p != "pdfaid").map(|p| {
            format!("The PDF/A identification schema is bound to the prefix \"{p}\", not pdfaid.")
        }),
    }
}

/// MD-20 to MD-27: an Info entry and its XMP equivalent agree.
fn info_row(view: &DocumentView<'_>, ctx: &Ctx<'_>, id: &str, text: &str, sink: &mut Sink) {
    let Some(&(_, key, xmp)) = INFO_MAP.iter().find(|(row, ..)| *row == id) else {
        return;
    };
    let info = view
        .trailer_entry(b"Info")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let value = match info.and_then(|i| i.get(key)).map(|o| view.resolve(o)) {
        Some(Object::String(s)) => crate::textstring::decode_text_string(s).text,
        _ => return,
    };
    let key = String::from_utf8_lossy(key);
    let items = property(text, xmp);
    let agrees = match (&items, key.as_ref()) {
        (None, _) => false,
        (Some(items), "Author") => items.len() == 1 && items.first() == Some(&value),
        (Some(items), "CreationDate" | "ModDate") => items
            .first()
            .is_some_and(|x| same_instant(&value, x).unwrap_or(false)),
        (Some(items), _) => items.first() == Some(&value),
    };
    if !agrees {
        let shown = items.map_or("absent".to_owned(), |i| format!("\"{}\"", i.join("; ")));
        sink.push(ctx.doc(&format!("Info /{key} is \"{value}\" but {xmp} is {shown}.")));
    }
}

/// The value of property `qname`: one item for a simple value, one per
/// `rdf:li` for an array (an `rdf:Alt`'s `x-default` item first).
fn property(text: &str, qname: &str) -> Option<Vec<String>> {
    attribute(text, qname)
        .map(|v| vec![v])
        .or_else(|| element(text, qname))
}

fn attribute(text: &str, qname: &str) -> Option<String> {
    text.match_indices(qname).find_map(|(at, _)| {
        let before = text[..at].chars().next_back()?;
        let rest = text[at + qname.len()..].trim_start().strip_prefix('=')?;
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let value = rest[1..].split(quote).next()?;
        before.is_whitespace().then(|| unescape(value))
    })
}

fn element(text: &str, qname: &str) -> Option<Vec<String>> {
    let open = format!("<{qname}");
    let close = format!("</{qname}>");
    let at = text.match_indices(&open).find_map(|(at, _)| {
        let next = text[at + open.len()..].chars().next()?;
        (next == '>' || next.is_whitespace()).then_some(at + open.len())
    })?;
    let body_start = at + text[at..].find('>')? + 1;
    if text[..body_start].ends_with("/>") {
        return Some(vec![String::new()]);
    }
    let body = &text[body_start..body_start + text[body_start..].find(&close)?];
    if !body.contains("<rdf:li") {
        return Some(vec![unescape(body.trim())]);
    }
    let mut items: Vec<(bool, String)> = Vec::new();
    for li in body.split("<rdf:li").skip(1) {
        let Some(gt) = li.find('>') else { continue };
        let default = li[..gt].contains("x-default");
        let value = li[gt + 1..].split("</rdf:li>").next().unwrap_or("");
        items.push((default, unescape(value.trim())));
    }
    items.sort_by_key(|(default, _)| !default);
    Some(items.into_iter().map(|(_, v)| v).collect())
}

/// XML's five named entities and numeric character references.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e => e
                .strip_prefix("#x")
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| e.strip_prefix('#').map(str::parse::<u32>))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A date's local time in seconds since 1970 and its UT offset in seconds,
/// when it states one.
type Instant = (i64, Option<i64>);

/// Whether a PDF date (ISO 32000-1 §7.9.4) and an XMP date (ISO 8601
/// profile) are the same instant; `None` when either is malformed. When
/// either omits its UT offset, local times are compared.
fn same_instant(pdf: &str, xmp: &str) -> Option<bool> {
    let (a, b) = (pdf_date(pdf)?, xmp_date(xmp)?);
    Some(match (a.1, b.1) {
        (Some(oa), Some(ob)) => a.0 - oa == b.0 - ob,
        _ => a.0 == b.0,
    })
}

/// Seconds since 1970 of the given fields, local time.
fn seconds(f: [i64; 6]) -> Option<i64> {
    let [y, mo, d, h, mi, s] = f;
    let ok = (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && s < 61;
    ok.then(|| crate::signature_verify::days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

/// `+hh'mm`, `-hh:mm` or `Z` as seconds east of UT.
fn offset(rest: &str) -> Option<i64> {
    let sign = match rest.chars().next()? {
        'Z' => return Some(0),
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
    let h = digits.get(0..2)?.parse::<i64>().ok()?;
    let m = digits
        .get(2..4)
        .map_or(Some(0), |m| m.parse::<i64>().ok())?;
    Some(sign * (h * 3600 + m * 60))
}

fn pdf_date(s: &str) -> Option<Instant> {
    let s = s.trim().strip_prefix("D:").unwrap_or(s.trim());
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    if !(4..=14).contains(&n) || n % 2 != 0 {
        return None;
    }
    let mut f = [0, 1, 1, 0, 0, 0];
    f[0] = s.get(..4)?.parse().ok()?;
    for (slot, at) in f.iter_mut().skip(1).zip((4..n).step_by(2)) {
        *slot = s.get(at..at + 2)?.parse().ok()?;
    }
    Some((seconds(f)?, offset(&s[n..])))
}

fn xmp_date(s: &str) -> Option<Instant> {
    let s = s.trim();
    let mut f = [0, 1, 1, 0, 0, 0];
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    for (i, part) in date.split('-').enumerate() {
        *f.get_mut(i)? = part.parse().ok()?;
    }
    let end = time.find(['Z', '+', '-']).unwrap_or(time.len());
    for (i, part) in time[..end].split(':').filter(|p| !p.is_empty()).enumerate() {
        let whole = part.split('.').next()?;
        *f.get_mut(i + 3)? = whole.parse().ok()?;
    }
    Some((seconds(f)?, offset(&time[end..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_compare_as_instants() {
        assert_eq!(
            same_instant("D:20240102030405+01'00'", "2024-01-02T02:04:05Z"),
            Some(true)
        );
        assert_eq!(
            same_instant("D:20240102", "2024-01-02T00:00:00"),
            Some(true)
        );
        assert_eq!(
            same_instant("D:20240102030405Z", "2024-01-02T03:04:06Z"),
            Some(false)
        );
        assert_eq!(same_instant("garbage", "2024"), None);
    }

    #[test]
    fn properties_in_three_forms() {
        let x = r#"<rdf:Description pdfaid:part="2" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
            <pdfaid:conformance>B</pdfaid:conformance>
            <dc:title><rdf:Alt><rdf:li xml:lang="fr">T</rdf:li><rdf:li xml:lang="x-default">A &amp; B</rdf:li></rdf:Alt></dc:title>
            </rdf:Description>"#;
        assert_eq!(property(x, "pdfaid:part"), Some(vec!["2".into()]));
        assert_eq!(property(x, "pdfaid:conformance"), Some(vec!["B".into()]));
        let title = property(x, "dc:title").and_then(|v| v.into_iter().next());
        assert_eq!(title.as_deref(), Some("A & B"));
        assert_eq!(prefix_of(x, PDFAID_NS), Some("pdfaid"));
    }
}
