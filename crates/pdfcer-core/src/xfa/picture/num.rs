//! `num{}` pictures on canonical numbers (pp.1176-1182): digits, sign,
//! radix, grouping, currency and percent, formatted on the decimal string
//! so no float rounding enters. Rounding is half-up (gap G-W10).

use super::{Out, Tok, tokens};

/// A canonical number as signed decimal digits.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dec {
    neg: bool,
    int: String,
    frac: String,
    radix: bool,
}

fn parse(v: &str) -> Option<Dec> {
    let v = v.trim();
    let (neg, body) = match v.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if (int.is_empty() && frac.is_empty()) || !digits(int) || !digits(frac) {
        return None;
    }
    let int = int.trim_start_matches('0').to_owned();
    let neg = neg && (int.chars().chain(frac.chars()).any(|c| c != '0'));
    Some(Dec {
        neg,
        int,
        frac: frac.to_owned(),
        radix: body.contains('.'),
    })
}

/// Whether `value` is a number equal to zero (`zero{}`, p.1185).
pub(super) fn is_zero(value: &str) -> bool {
    parse(value).is_some_and(|d| d.int.is_empty() && d.frac.chars().all(|c| c == '0'))
}

impl Dec {
    /// Times 100, for `%`.
    fn percent(mut self) -> Self {
        let mut frac = std::mem::take(&mut self.frac);
        while frac.len() < 2 {
            frac.push('0');
        }
        let (moved, rest) = frac.split_at(2);
        self.int = format!("{}{moved}", self.int)
            .trim_start_matches('0')
            .to_owned();
        self.frac = rest.to_owned();
        self
    }

    /// Rounded half-up to `k` fraction digits.
    fn round(mut self, k: usize) -> Self {
        if self.frac.len() <= k {
            return self;
        }
        let up = self.frac.as_bytes().get(k).is_some_and(|&b| b >= b'5');
        self.frac.truncate(k);
        if up {
            let mut all: Vec<u8> = format!("0{}{}", self.int, self.frac).into_bytes();
            for b in all.iter_mut().rev() {
                if *b == b'9' {
                    *b = b'0';
                } else {
                    *b += 1;
                    break;
                }
            }
            let s = String::from_utf8(all).unwrap_or_default();
            let (i, f) = s.split_at(s.len() - k);
            self.int = i.trim_start_matches('0').to_owned();
            self.frac = f.to_owned();
        }
        self
    }
}

/// One picture piece, CR/DB and the radix symbols resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Lit(String),
    Digit(char),
    Group,
    /// `.`/`V` (shown) or `v` (implied, never shown; p.1180).
    Radix(bool),
    /// `S`, `s`, `(`, `)`, `CR`, `cr`, `DB`, `db`: shown when negative,
    /// else the given blank.
    Sign(&'static str, &'static str),
    Currency,
    Percent,
}

/// `body`'s pieces; `Err` for a symbol pdfcer does not format.
fn pieces(body: &str) -> Result<Vec<Piece>, ()> {
    let toks = tokens(body);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(t) = toks.get(i) {
        let next = toks.get(i + 1);
        let pair = |a: char, b: char| {
            matches!(t, Tok::Sym(c, 1) if *c == a)
                && matches!(next, Some(Tok::Sym(c, 1)) if *c == b)
        };
        let two = [
            ("CR", 'C', 'R'),
            ("cr", 'c', 'r'),
            ("DB", 'D', 'B'),
            ("db", 'd', 'b'),
        ]
        .into_iter()
        .find(|&(_, a, b)| pair(a, b));
        if let Some((name, ..)) = two {
            let blank = if name.starts_with(char::is_uppercase) {
                "  "
            } else {
                ""
            };
            out.push(Piece::Sign(
                if name.starts_with('C') || name.starts_with('c') {
                    "CR"
                } else {
                    "DB"
                },
                blank,
            ));
            i += 2;
            continue;
        }
        match t {
            Tok::Lit(l) => out.push(Piece::Lit(l.clone())),
            Tok::Sym(c, n) => {
                for _ in 0..*n {
                    out.push(match c {
                        '9' | 'z' | 'Z' | '8' => Piece::Digit(*c),
                        ',' => Piece::Group,

                        '.' | 'V' => Piece::Radix(true),
                        'v' => Piece::Radix(false),
                        'S' => Piece::Sign("-", " "),
                        's' => Piece::Sign("-", ""),
                        '(' => Piece::Sign("(", " "),
                        ')' => Piece::Sign(")", " "),
                        '$' => Piece::Currency,
                        '%' => Piece::Percent,
                        c if super::is_literal(*c) => Piece::Lit(c.to_string()),
                        _ => return Err(()),
                    });
                }
            }
        }
        i += 1;
    }
    Ok(out)
}

/// `value` by a num picture; `None` when it does not fit (too many
/// integer digits, or negative with no sign symbol).
pub(super) fn format(body: &str, value: &str) -> Option<Out> {
    let Ok(ps) = pieces(body) else {
        return Some(Out::Unsupported);
    };
    let mut d = parse(value)?;
    if ps.contains(&Piece::Percent) {
        d = d.percent();
    }
    let split = ps
        .iter()
        .position(|p| matches!(p, Piece::Radix(_)))
        .unwrap_or(ps.len());
    let (int_ps, frac_ps) = ps.split_at(split);
    let frac_slots: Vec<char> = frac_ps
        .iter()
        .filter_map(|p| {
            if let Piece::Digit(c) = p {
                Some(*c)
            } else {
                None
            }
        })
        .collect();
    if !frac_slots.contains(&'8') {
        d = d.round(frac_slots.len());
    }
    let int_slots = int_ps
        .iter()
        .filter(|p| matches!(p, Piece::Digit(_)))
        .count();
    let has_sign = ps.iter().any(|p| matches!(p, Piece::Sign(..)));
    if d.int.len() > int_slots || (d.neg && !has_sign) {
        return None;
    }
    let mut s = int_part(int_ps, &d);
    if let Some(Piece::Radix(shown)) = frac_ps.first() {
        frac_part(
            frac_ps.get(1..).unwrap_or_default(),
            &frac_slots,
            &d,
            *shown,
            &mut s,
        );
    }
    let currency = ps.contains(&Piece::Currency);
    Some(Out::Text(s, currency))
}

/// The integer side: digits right-aligned into the slots, `,` shown once a
/// digit is to its left.
fn int_part(ps: &[Piece], d: &Dec) -> String {
    let mut digits = d.int.chars().rev();
    let mut slot_out: Vec<Option<String>> = vec![None; ps.len()];
    for (i, p) in ps.iter().enumerate().rev() {
        if let Piece::Digit(c) = p {
            let Some(slot) = slot_out.get_mut(i) else {
                continue;
            };
            *slot = Some(match (digits.next(), c) {
                (Some(g), _) => g.to_string(),
                (None, '9') => "0".to_owned(),
                (None, 'Z') => " ".to_owned(),
                (None, _) => String::new(),
            });
        }
    }
    let mut s = String::new();
    let (mut seen, mut prev_z) = (false, false);
    for (p, o) in ps.iter().zip(slot_out) {
        match p {
            Piece::Digit(c) => {
                let o = o.unwrap_or_default();
                seen |= o.chars().any(|c| c.is_ascii_digit());
                prev_z = *c == 'Z';
                s.push_str(&o);
            }
            Piece::Group if seen => s.push(','),
            Piece::Group if prev_z => s.push(' '),
            Piece::Group => {}
            other => push_common(other, d, &mut s),
        }
    }
    s
}

/// The fraction side: `9` pads 0, `Z` pads space, `z` shows only digits
/// the value has, `8` shows every digit the value has.
fn frac_part(ps: &[Piece], slots: &[char], d: &Dec, shown: bool, s: &mut String) {
    let fixed = slots.iter().any(|c| matches!(c, '9' | 'Z'));
    if shown && (fixed || d.radix || !d.frac.is_empty()) {
        s.push('.');
    }
    let mut digits = d.frac.chars();
    for p in ps {
        match p {
            Piece::Digit('8') => s.extend(digits.by_ref()),
            Piece::Digit(c) => match (digits.next(), c) {
                (Some(g), _) => s.push(g),
                (None, '9') => s.push('0'),
                (None, 'Z') => s.push(' '),
                (None, _) => {}
            },
            Piece::Group | Piece::Radix(_) => {}
            other => push_common(other, d, s),
        }
    }
}

fn push_common(p: &Piece, d: &Dec, s: &mut String) {
    match p {
        Piece::Lit(l) => s.push_str(l),
        Piece::Sign(neg, blank) => s.push_str(if d.neg { neg } else { blank }),
        Piece::Currency => s.push('$'),
        Piece::Percent => s.push('%'),
        _ => {}
    }
}
