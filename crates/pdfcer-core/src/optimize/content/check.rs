//! The re-read check: a rewrite must parse back to the kept tokens.

use super::rewrite::same_object;
use crate::content::{ContentStream, ContentTokenKind};

/// Whether `out` re-reads as exactly the tokens of `original` marked in
/// `keep`: the same operators and inline images byte for byte, and the same
/// operands by value (a `rounded` one by its rounded value).
pub(super) fn same(
    original: &ContentStream,
    (keep, rounded): (&[bool], &[Option<Vec<u8>>]),
    out: &[u8],
) -> bool {
    let Ok(reread) = ContentStream::parse(out.to_vec()) else {
        return false;
    };
    let kept = original.tokens.iter().zip(keep).enumerate();
    let mut fresh = reread.tokens.iter();
    for (i, (old, _)) in kept.filter(|(_, (_, k))| **k) {
        let Some(new) = fresh.next() else {
            return false;
        };
        let bytes =
            |buf: &[u8], t: &crate::content::ContentToken| t.span.slice(buf).map(<[u8]>::to_vec);
        let equal = match (&old.kind, &new.kind) {
            (ContentTokenKind::Operand(a), ContentTokenKind::Operand(b)) => {
                match rounded.get(i).and_then(Option::as_deref) {
                    Some(text) => rounded_to(b, text),
                    None => same_object(a, b),
                }
            }
            (ContentTokenKind::Operator, ContentTokenKind::Operator)
            | (ContentTokenKind::InlineImage { .. }, ContentTokenKind::InlineImage { .. }) => {
                bytes(&original.buf, old) == bytes(&reread.buf, new)
            }
            _ => false,
        };
        if !equal {
            return false;
        }
    }
    fresh.next().is_none()
}

/// Whether `b` is the number `text` spells.
fn rounded_to(b: &crate::object::Object, text: &[u8]) -> bool {
    let want = std::str::from_utf8(text)
        .ok()
        .and_then(|t| t.parse::<f64>().ok());
    want.is_some() && b.as_number() == want
}
