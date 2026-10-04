//! `EditOptions::replacement_faces` (decision 178): the replacement-face
//! ladder picks an installed face for the characters the run's font cannot
//! encode, skips one whose `fsType` forbids the edit, and says so.
//! `fallback-font.pdf`'s `/F0` is `FBKAAA+pdfcerFbRun`, carrying only "Qu5".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::fontinfo::EmbeddingPermission;
use pdfcer_core::text_edit::{
    self, EditOptions, EditRequest, FaceRung, FallbackSource, ReplacementFaces,
};
use pdfcer_render::FontData;
use pdfcer_render::font::{FaceCatalog, InstalledFaces};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const TYPED: &str = "Qu\u{20AC} 5";

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("run tools/gen-fallback-font-fixture.py")
}

fn faces(files: &[&str]) -> InstalledFaces {
    let mut faces = InstalledFaces::new();
    for f in files {
        faces.insert(f, FontData::new(fixture(f)));
    }
    faces
}

fn edit(faces: impl ReplacementFaces + 'static) -> text_edit::EditOutcome {
    let opts = EditOptions::default().with_replacement_faces(Box::leak(Box::new(faces)));
    let doc = Document::from_bytes(fixture("fallback-font.pdf")).unwrap();
    text_edit::edit_text(&doc, &EditRequest::find_replace(0, "Qu5", TYPED), &opts).unwrap()
}

#[test]
fn a_face_is_described_from_its_own_tables() {
    let faces = faces(&["fallback-donor.ttf", "fallback-run-face-restricted.ttf"]);
    let c = faces.candidates(&['Q', '\u{2264}']);
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].source, "fallback-donor.ttf");
    assert_eq!(c[1].family, "pdfcerFbRun");
    assert_eq!(c[0].missing, ['\u{2264}']);
    let permission = c[1].fs_type.map(|b| b.permission);
    assert_eq!(permission, Some(EmbeddingPermission::Restricted));
}

#[test]
fn a_covering_face_is_embedded_and_its_file_disclosed() {
    let out = edit(faces(&["fallback-donor.ttf"]));
    let used = out.report.fallback.expect("the fallback was used");
    assert_eq!(used.source, FallbackSource::EmbeddedSubset);
    let m = used.chosen_by.expect("the ladder chose it");
    assert_eq!(m.rung, FaceRung::Coverage);
    assert_eq!(m.source.as_deref(), Some("fallback-donor.ttf"));
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("replacement face:") && d.contains("fallback-donor.ttf")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn the_run_fonts_own_restricted_face_is_skipped_and_named() {
    let out = edit(faces(&[
        "fallback-run-face-restricted.ttf",
        "fallback-donor.ttf",
    ]));
    let m = out.report.fallback.unwrap().chosen_by.unwrap();
    assert_eq!(m.source.as_deref(), Some("fallback-donor.ttf"));
    assert_eq!(m.skipped.len(), 1);
    assert_eq!(m.skipped[0].rung, FaceRung::ExactName);
    assert!(
        m.disclosure().contains("fallback-run-face-restricted.ttf"),
        "{}",
        m.disclosure()
    );
}

/// The floor for the sans run font is Helvetica, which the page already
/// shows as `/F1`, so it is reused rather than added.
#[test]
fn with_no_face_offered_the_standard14_floor_is_named() {
    let out = edit(InstalledFaces::new());
    let used = out.report.fallback.unwrap();
    assert_eq!(used.source, FallbackSource::PageResource);
    assert_eq!(used.font_resource, b"F1");
    let m = used.chosen_by.unwrap();
    assert_eq!(
        (m.rung, m.face.as_str(), m.source.as_deref()),
        (FaceRung::Standard14, "Helvetica", None)
    );
}

/// A catalogue over `files` whose loader counts its reads.
fn catalog(files: &[&str]) -> (FaceCatalog, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&reads);
    let mut catalog = FaceCatalog::new(move |label: &str| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(fixture(label))
    });
    for f in files {
        catalog.add(f, &fixture(f));
    }
    (catalog, reads)
}

#[test]
fn a_catalogue_describes_its_faces_exactly_as_installed_faces_does() {
    let files = ["fallback-donor.ttf", "fallback-run-face-restricted.ttf"];
    let chars = ['Q', 'u', '5', '€', '≤', 'A'];
    let (catalog, reads) = catalog(&files);
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.candidates(&chars), faces(&files).candidates(&chars));
    assert_eq!(reads.load(Ordering::SeqCst), 0, "describing read a file");
}

#[test]
fn a_catalogue_reads_back_only_the_face_it_embeds() {
    let files = ["fallback-run-face-restricted.ttf", "fallback-donor.ttf"];
    let (catalog, reads) = catalog(&files);
    let out = edit(catalog);
    let m = out.report.fallback.unwrap().chosen_by.unwrap();
    assert_eq!(m.source.as_deref(), Some("fallback-donor.ttf"));
    assert_eq!(m.skipped.len(), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_file_that_changed_since_it_was_catalogued_is_refused() {
    let mut catalog = FaceCatalog::new(|_: &str| Ok(fixture("fallback-run-face-restricted.ttf")));
    catalog.add("fallback-donor.ttf", &fixture("fallback-donor.ttf"));
    let c = &catalog.candidates(&['Q'])[0];
    let err = catalog.plan(c, &['Q']).unwrap_err();
    assert!(err.contains("changed since it was catalogued"), "{err}");
}
