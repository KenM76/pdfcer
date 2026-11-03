//! Picture clauses for display: a field's `format/picture` applied to its
//! canonical value (XFA 3.3 Ch.26 pp.1150-1185; spec RAG slice
//! `adobe_ext__xfa_widgets_paint` §9).
//!
//! Locale data is the built-in `en_US` set: month and day names, the
//! predefined `date.short{}`-style pictures, `$`, `.` and `,`. Under any
//! other locale those come out in English and the result says so.

mod date;
mod num;

/// The type a value element gives its content (p.486, p.492).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::xfa) enum ValueKind {
    Text,
    Date,
    Time,
    DateTime,
    Num,
}

impl ValueKind {
    /// The kind of a `value` child element `tag`.
    pub(in crate::xfa) fn of(tag: &str) -> Self {
        match tag {
            "date" => Self::Date,
            "time" => Self::Time,
            "dateTime" => Self::DateTime,
            "integer" | "decimal" | "float" => Self::Num,
            _ => Self::Text,
        }
    }
}

/// A value as a picture shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::xfa) struct Shown {
    pub text: String,
    /// The picture uses something pdfcer does not format (shown as
    /// `text`, the raw value), or locale data it only has in English.
    pub approximated: bool,
}

/// `value` formatted by `picture`, trying each alternate in turn (p.1159);
/// `None` when none matches, so the value passes through unchanged.
pub(in crate::xfa) fn format(
    picture: &str,
    value: &str,
    kind: ValueKind,
    english: bool,
) -> Option<Shown> {
    for alt in split_unquoted(picture, '|') {
        let alt = alt.trim();
        let Some((category, body)) = compound(alt, kind) else {
            return Some(raw(value));
        };
        let out = match category {
            "date" => date::format_date(body, value),
            "time" => date::format_time(body, value),
            "num" => num::format(body, value),
            "text" => text(body, value),
            "null" => (value.is_empty()).then(|| literal_only(body)).flatten(),
            "zero" => num::is_zero(value)
                .then(|| num::format(body, value))
                .flatten(),
            _ => return Some(raw(value)),
        };
        if let Some(out) = out {
            return Some(match out {
                Out::Text(text, locale) => Shown {
                    text,
                    approximated: locale && !english,
                },
                Out::Unsupported => raw(value),
            });
        }
    }
    None
}

/// What one alternate made of a value: its text and whether it used locale
/// data, or a symbol pdfcer does not format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Out {
    Text(String, bool),
    Unsupported,
}

fn raw(value: &str) -> Shown {
    Shown {
        text: value.to_owned(),
        approximated: true,
    }
}

/// `category{body}`, `category.sub{}` or `category(locale){body}`; a bare
/// picture's category is `kind`'s (p.1157). A predefined sub-picture comes
/// back as its `en_US` body.
fn compound(alt: &str, kind: ValueKind) -> Option<(&str, &str)> {
    let Some(open) = alt.find('{').filter(|_| alt.ends_with('}')) else {
        let category = match kind {
            ValueKind::Date => "date",
            ValueKind::Time => "time",
            ValueKind::Num => "num",
            ValueKind::Text => "text",
            ValueKind::DateTime => return None,
        };
        return Some((category, alt));
    };
    let head = alt.get(..open)?;
    let body = alt.get(open + 1..alt.len() - 1)?;
    let head = head.split_once('(').map_or(head, |(h, _)| h);
    match head.split_once('.') {
        None => Some((head, body)),
        Some((category, sub)) if body.is_empty() => Some((category, predefined(category, sub)?)),
        Some(_) => None,
    }
}

/// The `en_US` predefined pictures (p.1155; `default` = medium).
fn predefined(category: &str, sub: &str) -> Option<&'static str> {
    Some(match (category, sub) {
        ("date", "short") => "M/D/YY",
        ("date", "medium" | "default") => "MMM D, YYYY",
        ("date", "long") => "MMMM D, YYYY",
        ("date", "full") => "EEEE, MMMM D, YYYY",
        ("time", "short") => "h:MM A",
        ("time", "medium" | "default" | "long" | "full") => "h:MM:SS A",
        ("num", "integer") => "z,zzz,zzz,zz9",
        ("num", "decimal") => "z,zzz,zzz,zz9.888",
        ("num", "currency") => "$z,zzz,zzz,zz9.99",
        ("num", "percent") => "z,zzz,zzz,zz9%",
        _ => return None,
    })
}

/// `s` split at `sep` outside single quotes.
fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in s.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            out.push(s.get(start..i).unwrap_or_default());
            start = i + c.len_utf8();
        }
    }
    out.push(s.get(start..).unwrap_or_default());
    out
}

/// A picture's pieces: quoted or punctuation literals, and runs of one
/// symbol character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tok {
    Lit(String),
    Sym(char, usize),
}

/// Tokenise `body`: `'...'` is literal with `''` a quote (p.1152); the
/// global symbols `?`, `*` and `+` show as a space; every other character
/// is a symbol run, for the category to read or reject.
pub(super) fn tokens(body: &str) -> Vec<Tok> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '\'' {
            let mut lit = String::new();
            i += 1;
            while let Some(&q) = chars.get(i) {
                if q == '\'' && chars.get(i + 1) == Some(&'\'') {
                    lit.push('\'');
                    i += 2;
                } else if q == '\'' {
                    i += 1;
                    break;
                } else {
                    lit.push(q);
                    i += 1;
                }
            }
            out.push(Tok::Lit(lit));
            continue;
        }
        if matches!(c, '?' | '*' | '+') {
            out.push(Tok::Lit(" ".to_owned()));
            i += 1;
            continue;
        }
        let run = chars
            .get(i..)
            .map_or(0, |r| r.iter().take_while(|&&x| x == c).count());
        out.push(Tok::Sym(c, run));
        i += run;
    }
    out
}

/// Punctuation every category reads as itself (p.1152).
pub(super) fn is_literal(c: char) -> bool {
    !c.is_alphanumeric() && c != '\''
}

/// A `null{}` picture's text: its literals only.
fn literal_only(body: &str) -> Option<Out> {
    let mut s = String::new();
    for t in tokens(body) {
        match t {
            Tok::Lit(l) => s.push_str(&l),
            Tok::Sym(c, n) if is_literal(c) => s.extend(std::iter::repeat_n(c, n)),
            Tok::Sym(..) => return Some(Out::Unsupported),
        }
    }
    Some(Out::Text(s, false))
}

/// A `text{}` picture (p.1183): `A` a letter, `X` anything, `O`/`0` a
/// letter or digit, `9` a digit, consumed in order; `None` unless every
/// symbol takes a character and every character is taken.
fn text(body: &str, value: &str) -> Option<Out> {
    let mut chars = value.chars();
    let mut s = String::new();
    for t in tokens(body) {
        match t {
            Tok::Lit(l) => s.push_str(&l),
            Tok::Sym(c, n) if matches!(c, 'A' | 'X' | 'O' | '0' | '9') => {
                for _ in 0..n {
                    let v = chars.next()?;
                    let ok = match c {
                        'A' => v.is_alphabetic(),
                        '9' => v.is_ascii_digit(),
                        'O' | '0' => v.is_alphanumeric(),
                        _ => true,
                    };
                    if !ok {
                        return None;
                    }
                    s.push(v);
                }
            }
            Tok::Sym(c, n) if is_literal(c) => s.extend(std::iter::repeat_n(c, n)),
            Tok::Sym(..) => return Some(Out::Unsupported),
        }
    }
    chars.next().is_none().then_some(Out::Text(s, false))
}

#[cfg(test)]
mod tests;
