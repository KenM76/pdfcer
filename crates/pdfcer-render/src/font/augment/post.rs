//! `post` for decision 173 §4: 3.0 is kept; 2.0 gains one name per appended
//! glyph (OpenType `post`: `glyphNameIndex` below 258 names a standard Mac
//! glyph, above it an appended Pascal string).

use crate::font::sfnt::{read_u16, read_u32};

use super::AugmentError;

const STANDARD_NAMES: u16 = 258;

/// `post` with `names` appended for the glyphs following the old ones.
/// `names[i]` is either a standard index (below 258) or a string to store.
pub(crate) fn append_names(post: &[u8], names: &[PostName]) -> Result<Vec<u8>, AugmentError> {
    match read_u32(post, 0) {
        Some(0x0003_0000) => Ok(post.to_vec()),
        Some(0x0002_0000) => append_v2(post, names),
        _ => Err(AugmentError::UnsupportedPostFormat),
    }
}

/// One appended glyph's `post` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostName {
    /// A standard Macintosh glyph-name index (below 258).
    Standard(u16),
    /// A name stored in the table's string data.
    Custom(String),
}

/// The face's `post` 2.0 entry for `gid`; `None` for any other version.
pub(crate) fn face_name(post: &[u8], gid: u16) -> Option<PostName> {
    if read_u32(post, 0)? != 0x0002_0000 {
        return None;
    }
    let n = read_u16(post, 32)?;
    if gid >= n {
        return None;
    }
    let index = read_u16(post, 34 + 2 * usize::from(gid))?;
    if index < STANDARD_NAMES {
        return Some(PostName::Standard(index));
    }
    let mut at = 34 + 2 * usize::from(n);
    for _ in STANDARD_NAMES..index {
        at += 1 + usize::from(*post.get(at)?);
    }
    let len = usize::from(*post.get(at)?);
    let s = std::str::from_utf8(post.get(at + 1..at + 1 + len)?).ok()?;
    Some(PostName::Custom(s.to_owned()))
}

fn append_v2(post: &[u8], names: &[PostName]) -> Result<Vec<u8>, AugmentError> {
    let malformed = || AugmentError::MalformedFace {
        detail: "the subset's post table is truncated".into(),
    };
    let n = usize::from(read_u16(post, 32).ok_or_else(malformed)?);
    let strings_at = 34 + 2 * n;
    let strings = post.get(strings_at..).ok_or_else(malformed)?;
    let mut indexes = Vec::with_capacity(n + names.len());
    for i in 0..n {
        indexes.push(read_u16(post, 34 + 2 * i).ok_or_else(malformed)?);
    }
    let mut stored = count_strings(strings);
    let mut extra = Vec::new();
    for name in names {
        match name {
            PostName::Standard(i) if *i < STANDARD_NAMES => indexes.push(*i),
            PostName::Standard(_) => return Err(malformed()),
            PostName::Custom(s) => {
                let len = u8::try_from(s.len()).map_err(|_| malformed())?;
                indexes.push(
                    u16::try_from(usize::from(STANDARD_NAMES) + stored).map_err(|_| malformed())?,
                );
                stored += 1;
                extra.push(len);
                extra.extend_from_slice(s.as_bytes());
            }
        }
    }
    let mut out = post.get(..32).ok_or_else(malformed)?.to_vec();
    out.extend_from_slice(
        &u16::try_from(indexes.len())
            .map_err(|_| malformed())?
            .to_be_bytes(),
    );
    for i in indexes {
        out.extend_from_slice(&i.to_be_bytes());
    }
    out.extend_from_slice(strings);
    out.extend_from_slice(&extra);
    Ok(out)
}

/// The number of complete Pascal strings in `data`.
fn count_strings(data: &[u8]) -> usize {
    let (mut at, mut n) = (0, 0);
    while let Some(&len) = data.get(at) {
        at += 1 + usize::from(len);
        if at > data.len() {
            break;
        }
        n += 1;
    }
    n
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

    fn v2(indexes: &[u16], strings: &[&str]) -> Vec<u8> {
        let mut p = vec![0, 2, 0, 0];
        p.resize(32, 0);
        p.extend_from_slice(&u16::try_from(indexes.len()).unwrap().to_be_bytes());
        for i in indexes {
            p.extend_from_slice(&i.to_be_bytes());
        }
        for s in strings {
            p.push(u8::try_from(s.len()).unwrap());
            p.extend_from_slice(s.as_bytes());
        }
        p
    }

    #[test]
    fn names_append_after_the_existing_strings() {
        let post = v2(&[0, 36, 258], &["Aring.alt"]);
        let out = append_names(
            &post,
            &[PostName::Standard(40), PostName::Custom("Eacute.sc".into())],
        )
        .unwrap();
        assert_eq!(out, v2(&[0, 36, 258, 40, 259], &["Aring.alt", "Eacute.sc"]));
        assert_eq!(
            face_name(&out, 4),
            Some(PostName::Custom("Eacute.sc".into()))
        );
        assert_eq!(face_name(&out, 3), Some(PostName::Standard(40)));
    }

    #[test]
    fn version_3_is_kept_and_2_5_refused() {
        let mut v3 = vec![0, 3, 0, 0];
        v3.resize(32, 0);
        assert_eq!(append_names(&v3, &[PostName::Standard(1)]).unwrap(), v3);
        let mut v25 = v3.clone();
        v25[1] = 2;
        v25[2] = 0x50;
        assert!(matches!(
            append_names(&v25, &[]),
            Err(AugmentError::UnsupportedPostFormat)
        ));
    }
}
