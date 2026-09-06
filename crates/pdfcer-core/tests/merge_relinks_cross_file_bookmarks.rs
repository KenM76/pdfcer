//! # `Pass 258.3` — merging a table of contents with the files it points at
//!
//! ## The problem, stated by someone else and confirmed by measurement
//!
//! An operator brought us another engineer's answer to his own question,
//! whose closing paragraph was this:
//!
//! > *"Launch and GoToR actions point at external files by name, so once
//! > everything is one document the targets no longer exist and the
//! > bookmarks break. If you merge, you need to rewrite each bookmark to an
//! > internal destination afterwards."*
//!
//! That was accurate about pdfcer. **Measured before building anything**:
//! merging a table-of-contents PDF with the four files its bookmarks
//! launched gave `outline_kept=0 outline_dropped=4`. Every one of the
//! operator's own bookmark titles was discarded, leaving only the
//! per-source headings pdfcer generates itself. pdfcer *disclosed* the loss
//! — which is the floor, not the ceiling, and the same distinction
//! `Pass 258.0` turned on for a dashed border.
//!
//! ## Why the merge can do better than a post-processing script
//!
//! The advice was to rebuild the mapping afterwards with a script. But the
//! merge **already knows** which file each source came from: it is holding
//! the list. The mapping the operator would otherwise reconstruct by hand
//! is free at exactly the moment it is needed, and no other stage of the
//! pipeline has both halves at once.
//!
//! ## Order is the fix
//!
//! Re-pointing has to happen **before** pruning. A `/Launch` entry has no
//! page in its own source, so the pruner discards it before anything can
//! notice that the file it names is sitting in the same merge. Getting
//! this backwards produces a correct-looking implementation that never
//! fires.
//!
//! ## What is deliberately NOT done
//!
//! A link naming a file that was **not** merged stays unresolved and prunes
//! as before. That bookmark is genuinely dead — the file it names is not in
//! the document and not on the way in — and inventing a destination for it
//! would be a worse answer than dropping it and saying so.

use pdfcer_core::document::Document;
use pdfcer_core::pageops::merge;
use pdfcer_core::view::DocumentView;

/// A minimal `pages`-page document.
fn plain_doc(pages: usize) -> Vec<u8> {
    let mut objs: Vec<(u32, Vec<u8>)> = Vec::new();
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    objs.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
    objs.push((
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        )
        .into_bytes(),
    ));
    for i in 0..pages {
        objs.push((
            u32::try_from(3 + i).expect("small"),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        ));
    }
    assemble_pdf(&objs)
}

/// A one-page document whose outline entries open OTHER files.
///
/// Four bookmarks, covering every shape a real table of contents mixes:
/// an indirect `/Launch` filespec, a direct `/Launch` string, a `/Launch`
/// carrying only the PDF-2.0-deprecated `/Win` dictionary, and a `/GoToR`
/// that names a page **inside** its target.
fn toc_doc() -> Vec<u8> {
    let objs: Vec<(u32, Vec<u8>)> = vec![
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 10 0 R >>".to_vec(),
        ),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        ),
        (
            10,
            b"<< /Type /Outlines /First 11 0 R /Last 14 0 R /Count 4 >>".to_vec(),
        ),
        (
            11,
            b"<< /Title (Chapter One) /Parent 10 0 R /Next 12 0 R /A 20 0 R >>".to_vec(),
        ),
        (20, b"<< /S /Launch /F 21 0 R >>".to_vec()),
        (
            21,
            b"<< /Type /Filespec /F (one.pdf) /UF (one.pdf) >>".to_vec(),
        ),
        (
            12,
            b"<< /Title (Chapter Two) /Parent 10 0 R /Prev 11 0 R /Next 13 0 R /A 22 0 R >>"
                .to_vec(),
        ),
        (22, b"<< /S /Launch /F (two.pdf) >>".to_vec()),
        (
            13,
            b"<< /Title (Legacy Appendix) /Parent 10 0 R /Prev 12 0 R /Next 14 0 R /A 23 0 R >>"
                .to_vec(),
        ),
        (
            23,
            b"<< /S /Launch /Win << /F (three.pdf) /O (open) >> >>".to_vec(),
        ),
        (
            14,
            b"<< /Title (Deep Link) /Parent 10 0 R /Prev 13 0 R /A 24 0 R >>".to_vec(),
        ),
        // Page INDEX 1 of two.pdf -- its second page, which exists.
        (24, b"<< /S /GoToR /F (two.pdf) /D [1 /Fit] >>".to_vec()),
    ];
    assemble_pdf(&objs)
}

/// A bookmark pointing at a file that is NOT part of the merge.
fn toc_pointing_elsewhere() -> Vec<u8> {
    let objs: Vec<(u32, Vec<u8>)> = vec![
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 10 0 R >>".to_vec(),
        ),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        ),
        (
            10,
            b"<< /Type /Outlines /First 11 0 R /Last 11 0 R /Count 1 >>".to_vec(),
        ),
        (
            11,
            b"<< /Title (Somewhere Else) /Parent 10 0 R /A 20 0 R >>".to_vec(),
        ),
        (20, b"<< /S /Launch /F (not-in-this-merge.pdf) >>".to_vec()),
    ];
    assemble_pdf(&objs)
}

fn assemble_pdf(objs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    let mut sorted = objs.to_vec();
    sorted.sort_by_key(|(n, _)| *n);
    for (num, body) in &sorted {
        offsets.push((*num, out.len()));
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    let top = sorted.last().map_or(1, |(n, _)| n + 1);
    out.extend_from_slice(format!("xref\n0 {top}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..top {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, at)) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {top} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    out
}

/// The four documents of the worked example, merged in order.
fn merged() -> (Vec<u8>, pdfcer_core::pageops::AssembleReport) {
    let toc = Document::from_bytes(toc_doc()).expect("toc");
    let one = Document::from_bytes(plain_doc(2)).expect("one");
    let two = Document::from_bytes(plain_doc(2)).expect("two");
    let three = Document::from_bytes(plain_doc(2)).expect("three");
    let views = vec![
        DocumentView::new(&toc, toc.bytes(), toc.version()),
        DocumentView::new(&one, one.bytes(), one.version()),
        DocumentView::new(&two, two.bytes(), two.version()),
        DocumentView::new(&three, three.bytes(), three.version()),
    ];
    let titles: Vec<Vec<u8>> = ["toc", "one", "two", "three"]
        .iter()
        .map(|t| pdfcer_core::edit::encode_text_string(t))
        .collect();
    let files: Vec<Vec<u8>> = ["toc.pdf", "one.pdf", "two.pdf", "three.pdf"]
        .iter()
        .map(|f| f.as_bytes().to_vec())
        .collect();
    merge(&views, &titles, &files).expect("merge")
}

/// Every bookmark in a document, flattened to (title, 1-based page).
fn bookmarks(bytes: &[u8]) -> Vec<(String, Option<usize>)> {
    let doc = Document::from_bytes(bytes.to_vec()).expect("re-parse the merged file");
    let session = pdfcer_core::edit::EditSession::new(doc);
    let outline = pdfcer_core::outline::read_outline(&session.graph());
    fn walk(items: &[pdfcer_core::outline::OutlineItem], out: &mut Vec<(String, Option<usize>)>) {
        for it in items {
            out.push((it.title.clone(), it.page_index().map(|i| i + 1)));
            walk(&it.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&outline.items, &mut out);
    out
}

/// ★ **THE HEADLINE.** The operator's own bookmark titles survive the
/// merge, pointing at pages inside the merged document.
#[test]
fn cross_file_bookmarks_are_repointed_instead_of_dropped() {
    let (bytes, report) = merged();

    assert_eq!(
        report.outline_items_relinked, 4,
        "all four cross-file bookmarks must be re-pointed"
    );
    assert_eq!(
        report.outline_items_dropped, 0,
        "★ and NONE dropped. Before this Pass every one of them was \
         discarded, because a /Launch names no page in its own source and \
         the pruner ran first"
    );

    let marks = bookmarks(&bytes);
    let titles: Vec<&str> = marks.iter().map(|(t, _)| t.as_str()).collect();
    for expected in ["Chapter One", "Chapter Two", "Legacy Appendix", "Deep Link"] {
        assert!(
            titles.contains(&expected),
            "the operator's own title {expected:?} must survive; got {titles:?}"
        );
    }
}

/// Each re-pointed bookmark lands on the right page.
///
/// Layout: toc = page 1, one.pdf = pages 2-3, two.pdf = 4-5, three.pdf =
/// 6-7. So `/Launch one.pdf` is page 2, and the `/GoToR two.pdf /D [1]` —
/// page INDEX 1 inside that file — is page 5, not page 4.
#[test]
fn a_repointed_bookmark_lands_on_the_right_page() {
    let (bytes, _) = merged();
    let marks = bookmarks(&bytes);
    let page_of = |title: &str| marks.iter().find(|(t, _)| t == title).and_then(|(_, p)| *p);

    assert_eq!(page_of("Chapter One"), Some(2), "one.pdf's first page");
    assert_eq!(page_of("Chapter Two"), Some(4), "two.pdf's first page");
    assert_eq!(
        page_of("Legacy Appendix"),
        Some(6),
        "three.pdf's first page, reached through the /Win fallback"
    );
    assert_eq!(
        page_of("Deep Link"),
        Some(5),
        "★ /GoToR named page INDEX 1 of two.pdf — the SECOND page — so it \
         must land on 5 and not on two.pdf's first page. A re-pointer that \
         ignored /D would put it on 4 and look almost right"
    );
}

/// A link naming a file that was not merged is still dropped, and still
/// reported. Inventing a destination for it would be worse than losing it.
#[test]
fn a_link_to_a_file_outside_the_merge_is_still_dropped() {
    let toc = Document::from_bytes(toc_pointing_elsewhere()).expect("toc");
    let one = Document::from_bytes(plain_doc(1)).expect("one");
    let views = vec![
        DocumentView::new(&toc, toc.bytes(), toc.version()),
        DocumentView::new(&one, one.bytes(), one.version()),
    ];
    let titles: Vec<Vec<u8>> = ["toc", "one"]
        .iter()
        .map(|t| pdfcer_core::edit::encode_text_string(t))
        .collect();
    let files: Vec<Vec<u8>> = ["toc.pdf", "one.pdf"]
        .iter()
        .map(|f| f.as_bytes().to_vec())
        .collect();
    let (_, report) = merge(&views, &titles, &files).expect("merge");

    assert_eq!(report.outline_items_relinked, 0);
    assert_eq!(
        report.outline_items_dropped, 1,
        "the named file is not in this merge, so the bookmark is genuinely \
         dead and is dropped — and counted, so the operator is told"
    );
}

/// Passing no file names disables re-pointing entirely — the behaviour
/// every caller had before this Pass, preserved so that a caller which
/// does not know its sources' names is not silently changed.
#[test]
fn without_file_names_nothing_is_repointed() {
    let toc = Document::from_bytes(toc_doc()).expect("toc");
    let one = Document::from_bytes(plain_doc(2)).expect("one");
    let views = vec![
        DocumentView::new(&toc, toc.bytes(), toc.version()),
        DocumentView::new(&one, one.bytes(), one.version()),
    ];
    let titles: Vec<Vec<u8>> = ["toc", "one"]
        .iter()
        .map(|t| pdfcer_core::edit::encode_text_string(t))
        .collect();
    let (_, report) = merge(&views, &titles, &[]).expect("merge");
    assert_eq!(report.outline_items_relinked, 0);
    assert!(
        report.outline_items_dropped >= 4,
        "the pre-Pass behaviour, unchanged: every cross-file bookmark is \
         dropped when there are no file names to match against"
    );
}

/// Matching is on the file NAME, case-insensitively, ignoring any
/// directory the operator happened to type.
#[test]
fn matching_ignores_directories_and_case() {
    let toc = Document::from_bytes(toc_doc()).expect("toc");
    let one = Document::from_bytes(plain_doc(1)).expect("one");
    let views = vec![
        DocumentView::new(&toc, toc.bytes(), toc.version()),
        DocumentView::new(&one, one.bytes(), one.version()),
    ];
    let titles: Vec<Vec<u8>> = ["toc", "one"]
        .iter()
        .map(|t| pdfcer_core::edit::encode_text_string(t))
        .collect();
    // The operator merged `C:\work\ONE.PDF`; the bookmark says `one.pdf`.
    let files: Vec<Vec<u8>> = vec![b"toc.pdf".to_vec(), b"C:\\work\\ONE.PDF".to_vec()];
    let (_, report) = merge(&views, &titles, &files).expect("merge");
    assert_eq!(
        report.outline_items_relinked, 1,
        "a path and a bare name must match on their last component, and \
         case must not decide it — requiring equality would answer 'never'"
    );
}
