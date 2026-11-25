//! Reading content streams within one allowance for the whole scan.
//!
//! A page's `/Contents` array has "the effect … as if all of the streams in
//! the array were concatenated, in order, to form a single stream" (ISO
//! 32000-2 §7.7.3.3 Table 30, §7.8.2). Its pieces are joined by one space:
//! the split falls at a token boundary, so the space separates two tokens
//! the way readers do, while a `%` comment left open at a piece's end still
//! runs on into the next piece up to its first end of line (§7.2.3).
//!
//! Each stream is decoded once however many pages and roles read it, and
//! freed after its last use.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::content;
use super::scan::Notes;
use crate::content::ContentStream;
use crate::document::Document;
use crate::filters::MAX_DECODED_LEN;
use crate::object::ObjId;

/// The byte budgets one scan decodes and re-reads content within.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    /// Bytes decoded in all, each stream charged once.
    pub decoded: usize,
    /// Bytes of an already-decoded stream read again: a stream shared by
    /// several pages, or read both as a page piece and alone.
    pub reread: usize,
    /// Bytes of one page's joined content.
    pub page: usize,
}

impl Limits {
    /// The scan's defaults: one page within [`MAX_DECODED_LEN`], four times
    /// that decoded in all, and one byte under twice the decoded total
    /// re-read, so each fits a 32-bit `usize`.
    pub(super) const DEFAULT: Limits = Limits {
        decoded: 4 * MAX_DECODED_LEN,
        reread: 8 * MAX_DECODED_LEN - 1,
        page: MAX_DECODED_LEN,
    };
}

type Piece = Option<Rc<Vec<u8>>>;

/// Decodes content streams within [`Limits`] and records those it could not
/// read: a stream that does not decode or tokenize, or one past a limit.
pub(super) struct Decoder<'a> {
    doc: &'a Document,
    left: Limits,
    /// Per stream, the reads still to come.
    uses: HashMap<ObjId, usize>,
    /// Streams decoded with reads still to come; `None` for one that could
    /// not be read.
    held: HashMap<ObjId, Piece>,
    /// What each member list already scanned needs: pages sharing one list
    /// are read once.
    lists: HashMap<Vec<ObjId>, Notes>,
    lost: Vec<ObjId>,
    decodes: usize,
}

impl<'a> Decoder<'a> {
    /// A decoder for the member lists of `pages` and the streams read
    /// `alone`, within `limits`.
    pub(super) fn new(
        doc: &'a Document,
        limits: Limits,
        pages: &HashMap<ObjId, Vec<ObjId>>,
        alone: &HashSet<ObjId>,
    ) -> Self {
        let mut uses: HashMap<ObjId, usize> = HashMap::new();
        let lists: HashSet<&Vec<ObjId>> = pages.values().collect();
        let members = lists
            .into_iter()
            .flat_map(|l| l.iter().collect::<HashSet<_>>());
        for id in members.chain(alone) {
            *uses.entry(*id).or_insert(0) += 1;
        }
        Decoder {
            doc,
            left: limits,
            uses,
            held: HashMap::new(),
            lists: HashMap::new(),
            lost: Vec::new(),
            decodes: 0,
        }
    }

    /// How many streams were decoded.
    pub(super) fn decodes(&self) -> usize {
        self.decodes
    }

    /// The streams found unreadable since the last call, each once.
    pub(super) fn take_lost(&mut self) -> Vec<ObjId> {
        std::mem::take(&mut self.lost)
    }

    fn lose(&mut self, id: ObjId) {
        if !self.lost.contains(&id) {
            self.lost.push(id);
        }
    }

    /// Scan stream `id` as content on its own.
    pub(super) fn alone(&mut self, id: ObjId, notes: &mut Notes) {
        let Some((bytes, again)) = self.fetch(id) else {
            return;
        };
        if again && !self.reread(bytes.len()) {
            self.lose(id);
            return;
        }
        let data = Rc::try_unwrap(bytes).unwrap_or_else(|b| (*b).clone());
        if !content::scan_content(data, notes) {
            self.lose(id);
        }
    }

    /// Scan a page's content `members` as the one stream they form. A
    /// member that cannot be read, or would pass a limit, is left out and
    /// counted. When the joined bytes do not tokenize, the members that do
    /// not tokenize alone are counted, or every member when each does.
    pub(super) fn page(&mut self, members: &[ObjId], notes: &mut Notes) {
        if let Some(known) = self.lists.get(members) {
            notes.extend(known.iter().cloned());
            return;
        }
        let mut pieces: HashMap<ObjId, (Rc<Vec<u8>>, bool)> = HashMap::new();
        for &m in members {
            if !pieces.contains_key(&m)
                && let Some(p) = self.fetch(m)
            {
                pieces.insert(m, p);
            }
        }
        let mut joined = Vec::new();
        let mut used = Vec::new();
        for &m in members {
            let Some((bytes, again)) = pieces.get(&m) else {
                continue;
            };
            let again = *again || used.contains(&m);
            if joined.len().saturating_add(bytes.len()) >= self.left.page
                || (again && !self.reread(bytes.len()))
            {
                self.lose(m);
                continue;
            }
            joined.extend_from_slice(bytes);
            joined.push(b' ');
            used.push(m);
        }
        let mut found = Notes::new();
        if !content::scan_content(joined, &mut found) {
            self.blame(&used, &pieces);
        }
        notes.extend(found.iter().cloned());
        self.lists.insert(members.to_vec(), found);
    }

    /// Count the `used` pieces that do not tokenize alone, or all of them.
    fn blame(&mut self, used: &[ObjId], pieces: &HashMap<ObjId, (Rc<Vec<u8>>, bool)>) {
        let bad: Vec<ObjId> = used
            .iter()
            .copied()
            .filter(|m| {
                pieces
                    .get(m)
                    .is_some_and(|(b, _)| ContentStream::parse(b.to_vec()).is_err())
            })
            .collect();
        for m in if bad.is_empty() { used } else { &bad } {
            self.lose(*m);
        }
    }

    /// Charge `len` bytes read again; `false` when past the limit.
    fn reread(&mut self, len: usize) -> bool {
        match self.left.reread.checked_sub(len) {
            Some(rest) => {
                self.left.reread = rest;
                true
            }
            None => false,
        }
    }

    /// Stream `id`'s decoded data, and whether it was decoded for an earlier
    /// read; `None` when it cannot be read. Freed after its last read.
    fn fetch(&mut self, id: ObjId) -> Option<(Rc<Vec<u8>>, bool)> {
        let rest = self.uses.get_mut(&id).map_or(0, |n| {
            *n = n.saturating_sub(1);
            *n
        });
        if let Some(held) = self.held.get(&id).cloned() {
            if rest == 0 {
                self.held.remove(&id);
            }
            return held.map(|b| (b, true));
        }
        let read = self.decode(id).map(Rc::new);
        if rest > 0 {
            self.held.insert(id, read.clone());
        }
        read.map(|b| (b, false))
    }

    /// Decode stream `id` within what is left of the allowance.
    fn decode(&mut self, id: ObjId) -> Option<Vec<u8>> {
        let data = if self.left.decoded == 0 {
            None
        } else {
            self.decodes += 1;
            content::decoded(self.doc, id)
        };
        match data {
            Some(d) if d.len() <= self.left.decoded => {
                self.left.decoded -= d.len();
                Some(d)
            }
            _ => {
                self.lose(id);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::optimize::tests::build;
    use crate::space_audit::tests::stream;

    fn streams(data: &[&[u8]]) -> (Document, Vec<ObjId>) {
        let bodies: Vec<Vec<u8>> = data.iter().map(|d| stream("", d)).collect();
        let ids = (1..=data.len())
            .map(|n| ObjId::new(u32::try_from(n).unwrap(), 0))
            .collect();
        (Document::from_bytes(build(&bodies)).unwrap(), ids)
    }

    fn limits(decoded: usize, reread: usize, page: usize) -> Limits {
        Limits {
            decoded,
            reread,
            page,
        }
    }

    #[test]
    fn a_stream_two_pages_and_a_form_read_is_decoded_once() {
        let (doc, ids) = streams(&[b"/OC /MC0", b"BDC EMC"]);
        let (s, t) = (ids[0], ids[1]);
        let pages = HashMap::from([
            (ObjId::new(10, 0), vec![s, t]),
            (ObjId::new(11, 0), vec![s]),
        ]);
        let alone = HashSet::from([s]);
        // Exactly both streams' bytes: decoding `s` twice would pass it.
        let mut d = Decoder::new(&doc, limits(15, 1 << 20, 1 << 20), &pages, &alone);
        let mut notes = Notes::new();
        d.page(&[s, t], &mut notes);
        d.page(&[s], &mut Notes::new());
        d.alone(s, &mut Notes::new());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(d.decodes(), 2);
        assert!(d.take_lost().is_empty());
        assert!(d.held.is_empty(), "freed after the last read");
    }

    #[test]
    fn a_stream_past_the_decode_allowance_is_counted_once() {
        let (doc, ids) = streams(&[b"q Q", b"q Q"]);
        let pages = HashMap::from([
            (ObjId::new(10, 0), vec![ids[0], ids[1]]),
            (ObjId::new(11, 0), vec![ids[1]]),
        ]);
        let mut d = Decoder::new(&doc, limits(4, 1 << 20, 1 << 20), &pages, &HashSet::new());
        d.page(&[ids[0], ids[1]], &mut Notes::new());
        d.page(&[ids[1]], &mut Notes::new());
        assert_eq!(d.take_lost(), [ids[1]]);
        assert_eq!(d.decodes(), 2);
    }

    #[test]
    fn one_pages_joined_content_stays_within_its_cap() {
        // A 4-byte stream named four times joins 5 bytes a time: a 12-byte
        // cap keeps two and counts the stream once.
        let (doc, ids) = streams(&[b"q Q "]);
        let members = vec![ids[0]; 4];
        let pages = HashMap::from([(ObjId::new(10, 0), members.clone())]);
        let mut d = Decoder::new(&doc, limits(1 << 20, 1 << 20, 12), &pages, &HashSet::new());
        d.page(&members, &mut Notes::new());
        assert_eq!(d.take_lost(), [ids[0]]);
        assert_eq!(d.decodes(), 1);
    }

    #[test]
    fn a_reread_past_its_allowance_is_counted() {
        let (doc, ids) = streams(&[b"q Q"]);
        let pages = HashMap::from([(ObjId::new(10, 0), vec![ids[0]])]);
        let alone = HashSet::from([ids[0]]);
        let mut d = Decoder::new(&doc, limits(1 << 20, 2, 1 << 20), &pages, &alone);
        d.page(&[ids[0]], &mut Notes::new());
        assert!(d.take_lost().is_empty());
        d.alone(ids[0], &mut Notes::new());
        assert_eq!(d.take_lost(), [ids[0]]);
    }

    #[test]
    fn only_the_piece_that_does_not_tokenize_is_blamed() {
        let (doc, ids) = streams(&[b"q Q", b"(open"]);
        let pages = HashMap::from([(ObjId::new(10, 0), ids.clone())]);
        let mut d = Decoder::new(&doc, Limits::DEFAULT, &pages, &HashSet::new());
        d.page(&ids, &mut Notes::new());
        assert_eq!(d.take_lost(), [ids[1]]);
    }
}
