//! Negative controls: each comparator must fail on the defect it exists to
//! catch, or a green gate means nothing.

use std::io::Write as _;

use super::fixture::{self, SENTINEL, SENTINEL_DATA, SENTINEL_KEY};
use super::runner::{copy, dump, paste};
use crate::document::Document;
use crate::edit::EditSession;
use crate::object::{Dict, Name, Object};
use crate::vector::ClipAnnotation;
use crate::writer::SaveOptions;
use crate::xcut::ObjectKind;
use crate::xcut::dump::compare;
use crate::xcut::policy::clipboard::RAW;

/// Where each needle survives in a saved file: its raw bytes, any string in
/// any object (object-stream members included, as the parser expands them)
/// and every decoded stream. Empty means removed.
fn survivors(bytes: &[u8], needles: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let raw_hit = |n: &str| bytes.windows(n.len()).any(|w| w == n.as_bytes());
    let Ok(doc) = Document::from_bytes(bytes.to_vec()) else {
        return vec!["the saved file does not open".to_owned()];
    };
    let view = doc.view();
    for needle in needles {
        if raw_hit(needle) {
            found.push(format!("{needle}: raw bytes"));
        }
        for object in doc.objects() {
            if let Object::Stream(stream) = &object.value {
                let data = crate::graph::ObjectGraph::stream_raw(&view, stream)
                    .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok());
                if data.is_some_and(|d| d.windows(needle.len()).any(|w| w == needle.as_bytes())) {
                    found.push(format!("{needle}: stream {}", object.id.num));
                }
            }
        }
    }
    found
}

#[test]
#[allow(clippy::unwrap_used)]
fn a_paste_that_drops_the_sentinel_fails_the_comparator() {
    let (s, id) = fixture::build(ObjectKind::Stamp);
    let before = dump(&s, id);
    let mut clip = copy(&s, id).unwrap();
    let [ClipAnnotation::Raw(raw)] = clip.annotations.as_mut_slice() else {
        panic!("a stamp travels raw");
    };
    raw.dict.remove(SENTINEL_KEY);
    let mut dst = fixture::destination();
    let landed = paste(&mut dst, 0, &clip, (0.0, 0.0)).unwrap();
    let faults = compare(&before, &landed.after, RAW, (0.0, 0.0));
    assert!(
        faults.iter().any(|f| f.starts_with("/XXpdfcerSentinel:")),
        "{faults:?}"
    );
}

#[test]
#[allow(clippy::unwrap_used)]
fn an_honest_paste_passes_the_comparator() {
    let (s, id) = fixture::build(ObjectKind::Stamp);
    let before = dump(&s, id);
    let clip = copy(&s, id).unwrap();
    let landed = paste(&mut fixture::destination(), 0, &clip, (0.0, 0.0)).unwrap();
    assert_eq!(
        compare(&before, &landed.after, RAW, (0.0, 0.0)),
        Vec::<String>::new()
    );
}

/// A redact that leaves the target must fail the `Remove` check, including
/// for a needle only visible once a compressed stream is decoded.
#[test]
#[allow(clippy::unwrap_used)]
fn a_redact_that_leaves_the_sentinel_fails_the_remove_check() {
    let (mut s, _) = fixture::build(ObjectKind::Redact);
    let hidden = "xcut-compressed-7810";
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(hidden.as_bytes()).unwrap();
    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    s.xcut_add_stream(dict, &z.finish().unwrap());
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).unwrap();
    assert!(!bytes.windows(hidden.len()).any(|w| w == hidden.as_bytes()));

    let found = survivors(&bytes, &[SENTINEL, SENTINEL_DATA, hidden]);
    for needle in [SENTINEL, SENTINEL_DATA, hidden] {
        assert!(
            found.iter().any(|f| f.starts_with(needle)),
            "{needle}: {found:?}"
        );
    }
}

#[test]
#[allow(clippy::unwrap_used)]
fn a_clean_file_passes_the_remove_check() {
    let s: EditSession = fixture::destination();
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).unwrap();
    assert_eq!(
        survivors(&bytes, &[SENTINEL, SENTINEL_DATA]),
        Vec::<String>::new()
    );
}
