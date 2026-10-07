//! Building a page's decoded content and decomposition, resuming the previous
//! one where the edits since it was built leave a prefix of `/Contents`
//! byte-identical (pdfcer-gui request G140).
//!
//! The walk over `/Contents[..k]` depends only on those bytes and on the
//! objects the page's resources reach. When every object written since the
//! last build is one of the page's content streams and streams `..k` kept
//! their bytes, their tokens, objects and form leaves are reused and only
//! streams `k..` are decoded, tokenized and walked. The result equals a
//! full build; `tests/page_model_resume.rs` holds it to that.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{EditSession, PageModelKey};
use crate::content::{ContentError, ContentStream, ContentToken};
use crate::object::{ObjId, Object};
use crate::page_tree::Page;
use crate::vector::decompose::WalkCheckpoint;
use crate::vector::decompose::resume::{
    LeafMark, collect_leaves_marked, decompose_checkpointed, decompose_resumed,
};
use crate::vector::{DocumentFonts, DocumentXObjects, FormLeaf, PageObjects, VectorObject};
use crate::view::DocumentView;

/// Past this many distinct writes the log is dropped: a session that has
/// edited this much elsewhere gains nothing from tracking it.
const MAX_LOGGED_WRITES: usize = 256;

/// One place a later build may resume: before `/Contents[part]`.
#[derive(Debug, Clone)]
struct ResumePoint {
    part: usize,
    /// Joined-buffer length after `/Contents[..part]`.
    byte: usize,
    walk: WalkCheckpoint,
    leaves: LeafMark,
}

/// What a page-model build leaves for the next one.
#[derive(Debug, Clone, Default)]
pub(super) struct PageModelResume {
    /// Joined-buffer length after each `/Contents` entry.
    part_ends: Vec<usize>,
    points: Vec<ResumePoint>,
    /// Objects written since the build.
    writes: BTreeSet<ObjId>,
    /// A write the log cannot describe (a deletion, a trailer change, an
    /// overflowing log) happened since the build.
    spoiled: bool,
}

/// The built model: content, decomposition, and where to resume next time.
type Built = (Arc<ContentStream>, Arc<PageObjects>, PageModelResume);

impl EditSession {
    /// Record that `id` is being written, for the memo's resume test.
    pub(super) fn note_page_model_write(&mut self, id: ObjId) {
        if let Some(r) = self.page_objects_cache.as_mut().map(|c| &mut c.resume)
            && !r.spoiled
        {
            r.writes.insert(id);
            r.spoiled = r.writes.len() > MAX_LOGGED_WRITES;
        }
    }

    /// Record a change the write log cannot describe.
    pub(super) fn forget_page_model_resume(&mut self) {
        if let Some(c) = self.page_objects_cache.as_mut() {
            c.resume.spoiled = true;
        }
    }

    /// `page`'s content and decomposition (with leaves), resuming the memo's
    /// when [`resume_from`] finds a point to resume at. The memo is consumed:
    /// when no caller still shares it, its buffers are truncated and reused
    /// rather than copied.
    pub(super) fn build_page_model(
        &mut self,
        page: &Page,
        key: &PageModelKey,
    ) -> Result<Built, ContentError> {
        let old = self.page_objects_cache.take();
        let point = old.as_ref().and_then(|c| resume_from(c, key)).cloned();
        let view = self.view();
        match (old, point) {
            (Some(c), Some(point)) => {
                let prefix = reuse_prefix(c.stream, c.objects, &point);
                resume_build(&view, page, prefix, &c.resume, &point)
            }
            _ => full_build(&view, page),
        }
    }
}

/// The memo's content and objects cut back to `at`: moved out when nothing
/// else holds them, copied otherwise.
struct Prefix {
    buf: Vec<u8>,
    tokens: Vec<ContentToken>,
    objects: Vec<VectorObject>,
    leaves: Vec<FormLeaf>,
}

fn reuse_prefix(stream: Arc<ContentStream>, model: Arc<PageObjects>, at: &ResumePoint) -> Prefix {
    let (mut buf, mut tokens) = match Arc::try_unwrap(stream) {
        Ok(s) => (s.buf, s.tokens),
        Err(shared) => (
            shared.buf.get(..at.byte).unwrap_or_default().to_vec(),
            shared
                .tokens
                .get(..at.walk.token)
                .unwrap_or_default()
                .to_vec(),
        ),
    };
    buf.truncate(at.byte);
    tokens.truncate(at.walk.token);
    let (mut objects, mut leaves) = match Arc::try_unwrap(model) {
        Ok(m) => (m.objects, m.leaves),
        Err(shared) => (
            shared
                .objects
                .get(..at.walk.objects)
                .unwrap_or_default()
                .to_vec(),
            shared
                .leaves
                .get(..at.leaves.leaves)
                .unwrap_or_default()
                .to_vec(),
        ),
    };
    objects.truncate(at.walk.objects);
    leaves.truncate(at.leaves.leaves);
    Prefix {
        buf,
        tokens,
        objects,
        leaves,
    }
}

/// The point the memo `c` may resume at for `key`: the last one before the
/// first `/Contents` entry written or moved since, provided only content
/// streams of this page were written.
fn resume_from<'c>(c: &'c super::PageObjectsCache, key: &PageModelKey) -> Option<&'c ResumePoint> {
    let r = &c.resume;
    let same_page = c.key.page_id == key.page_id
        && c.key.resources == key.resources
        && c.key.contents.len() == key.contents.len()
        && c.key
            .contents
            .iter()
            .zip(&key.contents)
            .all(|(a, b)| a.0 == b.0);
    let ids: BTreeSet<ObjId> = key.contents.iter().map(|(id, _)| *id).collect();
    if r.spoiled || !same_page || !r.writes.is_subset(&ids) {
        return None;
    }
    let first_changed = c
        .key
        .contents
        .iter()
        .zip(&key.contents)
        .position(|(a, b)| a.1 != b.1 || r.writes.contains(&b.0))?;
    r.points.iter().rev().find(|p| p.part <= first_changed)
}

/// Decode `/Contents[from..]` onto `buf`, joined as
/// [`ContentStream::from_page`] joins them, recording each entry's end.
fn join_parts(
    view: &DocumentView<'_>,
    page: &Page,
    from: usize,
    buf: &mut Vec<u8>,
    ends: &mut Vec<usize>,
) -> Result<(), ContentError> {
    for (i, id) in page.contents.iter().enumerate().skip(from) {
        let Some(Object::Stream(stream)) = view.graph().value(*id) else {
            return Err(ContentError::NotAStream);
        };
        let raw = view
            .slice(stream.data_span)
            .ok_or(ContentError::NotAStream)?;
        let decoded = crate::filters::decode_stream(&stream.dict, raw)?;
        if i > 0 && !decoded.is_empty() {
            buf.push(b'\n');
        }
        buf.extend_from_slice(&decoded);
        ends.push(buf.len());
    }
    Ok(())
}

/// For each entry after `after` whose start no token straddles, its index
/// and the token index it starts at.
fn clean_boundaries(
    stream: &ContentStream,
    ends: &[usize],
    after: usize,
) -> (Vec<usize>, Vec<usize>) {
    let mut parts = Vec::new();
    let mut tokens = Vec::new();
    for (part, &byte) in ends
        .iter()
        .enumerate()
        .take(ends.len().saturating_sub(1))
        .skip(after)
    {
        let t = stream.tokens.partition_point(|tok| tok.span.start < byte);
        let straddles = t
            .checked_sub(1)
            .and_then(|p| stream.tokens.get(p))
            .is_some_and(|tok| tok.span.end() > byte);
        if !straddles {
            parts.push(part + 1);
            tokens.push(t);
        }
    }
    (parts, tokens)
}

/// Pair each successful checkpoint with its entry, byte offset and leaf mark.
fn points(
    parts: &[usize],
    ends: &[usize],
    walks: Vec<Option<WalkCheckpoint>>,
    leaves: Vec<LeafMark>,
) -> Vec<ResumePoint> {
    let taken = parts
        .iter()
        .zip(walks)
        .filter_map(|(p, w)| w.map(|w| (*p, w)));
    taken
        .zip(leaves)
        .filter_map(|((part, walk), leaves)| {
            let byte = *ends.get(part.checked_sub(1)?)?;
            Some(ResumePoint {
                part,
                byte,
                walk,
                leaves,
            })
        })
        .collect()
}

fn full_build(view: &DocumentView<'_>, page: &Page) -> Result<Built, ContentError> {
    let mut buf = Vec::new();
    let mut ends = Vec::with_capacity(page.contents.len());
    join_parts(view, page, 0, &mut buf, &mut ends)?;
    let stream = ContentStream::parse(buf)?;
    let (parts, bounds) = clean_boundaries(&stream, &ends, 0);
    let xobjects = DocumentXObjects {
        view,
        resources: &page.resources,
    };
    let fonts = DocumentFonts::new(view, &page.resources);
    let (mut model, walks) = decompose_checkpointed(&stream, &xobjects, &fonts, &bounds);
    let splits: Vec<usize> = walks.iter().flatten().map(|w| w.objects).collect();
    let leaves = collect_leaves_marked(view, &mut model, 0, Vec::new(), None, &splits);
    let resume = PageModelResume {
        points: points(&parts, &ends, walks, leaves),
        part_ends: ends,
        ..PageModelResume::default()
    };
    Ok((Arc::new(stream), Arc::new(model), resume))
}

fn resume_build(
    view: &DocumentView<'_>,
    page: &Page,
    prefix: Prefix,
    old: &PageModelResume,
    at: &ResumePoint,
) -> Result<Built, ContentError> {
    let Prefix {
        mut buf,
        mut tokens,
        objects,
        leaves: prior,
    } = prefix;
    let mut ends = old.part_ends.get(..at.part).unwrap_or_default().to_vec();
    join_parts(view, page, at.part, &mut buf, &mut ends)?;
    // Room for the tail at the prefix's density, so the tokenizer does not
    // reallocate (and copy) the whole prefix on its first push.
    let tail = buf.len().saturating_sub(at.byte);
    tokens.reserve(tail.saturating_mul(at.walk.token) / at.byte.max(1) + 16);
    let stream = ContentStream::parse_from(buf, tokens, at.byte)?;
    let (parts, bounds) = clean_boundaries(&stream, &ends, at.part);
    let xobjects = DocumentXObjects {
        view,
        resources: &page.resources,
    };
    let fonts = DocumentFonts::new(view, &page.resources);
    let (mut model, walks) =
        decompose_resumed(&stream, &xobjects, &fonts, objects, &at.walk, &bounds);
    let splits: Vec<usize> = walks.iter().flatten().map(|w| w.objects).collect();
    let leaves = collect_leaves_marked(
        view,
        &mut model,
        at.walk.objects,
        prior,
        Some(at.leaves),
        &splits,
    );
    let mut kept: Vec<ResumePoint> = old
        .points
        .iter()
        .filter(|p| p.part <= at.part)
        .cloned()
        .collect();
    kept.extend(points(&parts, &ends, walks, leaves));
    let resume = PageModelResume {
        points: kept,
        part_ends: ends,
        ..PageModelResume::default()
    };
    Ok((Arc::new(stream), Arc::new(model), resume))
}
