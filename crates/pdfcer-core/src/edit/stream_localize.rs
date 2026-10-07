//! Writing a page edit back to the one `/Contents` stream it changed.
//!
//! An edit splices the page's concatenated content (the array's streams
//! joined at token boundaries, ISO 32000-1 §7.8.2). When the bytes that
//! differ all fall inside one stream, writing that stream alone draws the same
//! page as folding everything into `/Contents[0]`, and leaves every other
//! stream -- including one another page also draws -- untouched.

use std::collections::BTreeSet;

use super::{Command, CommandKind, DecoupledContent, EditSession, ObjectWrite};
use crate::object::ObjId;
use crate::text_edit::edit::make_raw_stream;
use crate::view::DocumentView;

impl EditSession {
    /// The command writing `new_content` into the single stream it changed,
    /// or `None` when the change spans streams, lands in a stream another page
    /// draws (`shared`), in a stream listed twice, or in a pdfcer OCR layer,
    /// or when the page carries text decorations.
    pub(super) fn localized_text_edit_command(
        &mut self,
        kind: CommandKind,
        contents: &[ObjId],
        new_content: &[u8],
        shared: &BTreeSet<ObjId>,
        prior: &mut Vec<ObjectWrite>,
        disclosures: &mut Vec<String>,
    ) -> Option<(Command, Option<DecoupledContent>)> {
        let (id, payload) = changed_stream(&self.view(), contents, new_content, shared)?;
        disclosures.retain(|d| !d.starts_with("multi-stream page:"));
        let span = self.stage_bytes(&payload);
        let mut objects = vec![ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(make_raw_stream(span, payload.len())),
        }];
        objects.append(prior);
        let command = Command {
            kind,
            objects,
            removals: Vec::new(),
            trailer: None,
        };
        let report = DecoupledContent {
            content_object: id.num,
            emptied: 0,
        };
        Some((command, Some(report)))
    }
}

/// The one stream of `contents` that `folded` changes, with its new payload.
fn changed_stream(
    view: &DocumentView<'_>,
    contents: &[ObjId],
    folded: &[u8],
    shared: &BTreeSet<ObjId>,
) -> Option<(ObjId, Vec<u8>)> {
    // Decoration rules are recomputed only for a page whose whole content is
    // its first stream (`refresh_decorations`), so a decorated page folds.
    let tag = crate::text_edit::decoration::DECORATION_TAG;
    if contents.len() < 2 || folded.windows(tag.len()).any(|w| w == tag) {
        return None;
    }
    let parts = contents
        .iter()
        .map(|id| crate::ocr::refold::decoded(view, *id))
        .collect::<Option<Vec<_>>>()?;
    let (index, payload) = single_part_change(&parts, folded)?;
    let id = *contents.get(index)?;
    let listed_once = contents.iter().filter(|c| **c == id).count() == 1;
    let movable =
        listed_once && !shared.contains(&id) && !crate::ocr::marker::is_layer_stream(view, id);
    movable.then_some((id, payload))
}

/// Where `folded` differs from `parts` joined as
/// `ContentStream::from_page` joins them (a `\n` before every non-empty part
/// after the first): `Some((i, payload))` when every differing byte lies
/// inside non-empty part `i` and its new payload is non-empty, else `None`
/// (including when nothing differs).
fn single_part_change(parts: &[Vec<u8>], folded: &[u8]) -> Option<(usize, Vec<u8>)> {
    let mut old = Vec::with_capacity(folded.len());
    let mut ranges = Vec::with_capacity(parts.len());
    for (i, p) in parts.iter().enumerate() {
        if i > 0 && !p.is_empty() {
            old.push(b'\n');
        }
        ranges.push((old.len(), old.len() + p.len()));
        old.extend_from_slice(p);
    }
    if old == folded {
        return None;
    }
    let prefix = old.iter().zip(folded).take_while(|(a, b)| a == b).count();
    let room = old.len().min(folded.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(folded.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let changed_end = old.len() - suffix;
    let index = ranges
        .iter()
        .position(|&(s, e)| s < e && s <= prefix && changed_end <= e)?;
    let (start, end) = *ranges.get(index)?;
    let new_end = (end + folded.len()).checked_sub(old.len())?;
    let payload = folded.get(start..new_end)?;
    (!payload.is_empty())
        .then(|| payload.to_vec())
        .map(|p| (index, p))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::single_part_change;

    fn parts(p: &[&str]) -> Vec<Vec<u8>> {
        p.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn a_change_inside_one_part_rewrites_only_that_part() {
        let p = parts(&["0 0 m 9 9 l S", "1 1 m 5 5 l S", "2 2 m 7 7 l S"]);
        let got = single_part_change(&p, b"0 0 m 9 9 l S\n61 1 m 65 5 l S\n2 2 m 7 7 l S");
        assert_eq!(got, Some((1, b"61 1 m 65 5 l S".to_vec())));
    }

    #[test]
    fn a_change_spanning_a_separator_is_not_local() {
        let p = parts(&["0 0 m", "9 9 l S"]);
        assert_eq!(single_part_change(&p, b"0 0 m 9 9 l S"), None);
        assert_eq!(single_part_change(&p, b"0 1 m\n8 9 l S"), None);
    }

    #[test]
    fn empty_parts_carry_no_separator() {
        let p = parts(&["a b c", "", "d e f"]);
        assert_eq!(
            single_part_change(&p, b"a b c\nd x f"),
            Some((2, b"d x f".to_vec()))
        );
    }

    #[test]
    fn growth_and_shrink_at_either_end_of_a_part() {
        let p = parts(&["q Q", "1 2 m"]);
        assert_eq!(
            single_part_change(&p, b"q Q\n1 2 m 3 4 l S"),
            Some((1, b"1 2 m 3 4 l S".to_vec()))
        );
        assert_eq!(
            single_part_change(&p, b"q\nQ\n1 2 m"),
            Some((0, b"q\nQ".to_vec()))
        );
    }

    #[test]
    fn no_change_or_an_emptied_part_falls_back() {
        let p = parts(&["a", "b"]);
        assert_eq!(single_part_change(&p, b"a\nb"), None);
        assert_eq!(single_part_change(&p, b"a\n"), None);
    }
}
