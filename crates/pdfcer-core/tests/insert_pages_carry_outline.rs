//! `EditSession::insert_pages_with_options`: the source's bookmarks aimed at
//! the inserted pages (ISO 32000-1 §12.3.3) and its named destinations
//! naming them (§12.3.2.3) arrive in the same command as the pages.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::outline::read_outline;
use pdfcer_core::pageops::{DestinationResolver, InsertOptions, InsertPosition, InsertedOutline};
use pdfcer_core::writer::SaveOptions;

const BASIC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/outline/basic-tree.pdf"
);
const NAMED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/synthetic/outline/named-dests.pdf"
);

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max = offsets.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for n in 1..=max {
        match offsets.iter().find(|(k, _)| *k == n) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

fn load(path: &str) -> Document {
    Document::from_bytes(std::fs::read(path).expect("fixture")).expect("parse")
}

/// One page; defines `tree-body` and `chap` for it, so a carried key with
/// either name collides.
fn target() -> EditSession {
    EditSession::new(
        Document::from_bytes(build(&[
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /Names << /Dests << /Names [(chap) [3 0 R /Fit] (tree-body) [3 0 R /Fit]] >> >> >>",
            ),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
            ),
            (3, "<< /Type /Page /Parent 2 0 R >>"),
        ]))
        .expect("target"),
    )
}

fn saved(session: &EditSession) -> Document {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    Document::from_bytes(bytes).expect("reparse")
}

fn opts(outline: InsertedOutline, dests: bool) -> InsertOptions {
    InsertOptions::default()
        .with_outline(outline)
        .with_named_destinations(dests)
}

/// Would catch: a bookmark aimed at a page left behind arriving, the
/// explicit view being replaced by `/Fit`, or the destination not rebound.
#[test]
fn append_carries_only_the_bookmarks_aimed_at_inserted_pages() {
    let src = load(BASIC);
    let mut s = target();
    let out = s
        .insert_pages_with_options(
            &src.view(),
            &[2, 3],
            InsertPosition::End,
            &opts(InsertedOutline::Append, false),
        )
        .expect("insert");
    assert_eq!(out.outline_items_carried, 2, "Chapter 2 and Section 2.1");
    assert!(!out.source_outline_dropped);

    let doc = saved(&s);
    let o = read_outline(&doc);
    assert_eq!(o.items.len(), 1);
    assert_eq!(o.items[0].title, "Chapter 2");
    assert_eq!(o.items[0].page_index(), Some(1));
    assert!(!o.items[0].open, "carried items arrive closed");
    assert_eq!(o.items[0].children[0].title, "Section 2.1");
    assert_eq!(o.items[0].children[0].page_index(), Some(2));
    assert!(o.diagnostics.is_faithful(), "{:?}", o.diagnostics);
    let text = String::from_utf8_lossy(
        &s.to_incremental_bytes(&SaveOptions::identity())
            .expect("save")
            .0,
    )
    .into_owned();
    assert!(text.contains("/FitR 10 20 300 400"), "view kept:\n{text}");
}

/// Would catch: a container whose own page stayed behind being dropped with
/// its inserted child, or keeping a destination into the source.
#[test]
fn a_container_of_an_inserted_bookmark_arrives_without_a_destination() {
    let src = load(BASIC);
    let mut s = target();
    let out = s
        .insert_pages_with_options(
            &src.view(),
            &[1],
            InsertPosition::End,
            &opts(InsertedOutline::Append, false),
        )
        .expect("insert");
    assert_eq!(out.outline_items_carried, 2);
    let o = read_outline(&saved(&s));
    assert_eq!(o.items[0].title, "Chapter 1");
    assert!(o.items[0].destination.is_none());
    assert_eq!(o.items[0].children.len(), 1);
    assert_eq!(o.items[0].children[0].title, "Section 1.2");
    assert_eq!(o.items[0].children[0].page_index(), Some(1));
}

/// Would catch: the heading missing, aimed elsewhere, or the carried items
/// left at the top level.
#[test]
fn under_parent_nests_the_carried_bookmarks_under_one_named_heading() {
    let src = load(BASIC);
    let mut s = target();
    let out = s
        .insert_pages_with_options(
            &src.view(),
            &[2, 3],
            InsertPosition::End,
            &opts(InsertedOutline::UnderParent("Appendix".into()), false),
        )
        .expect("insert");
    assert_eq!(out.outline_items_carried, 3, "the heading counts");
    let o = read_outline(&saved(&s));
    assert_eq!(o.items.len(), 1);
    assert_eq!(o.items[0].title, "Appendix");
    assert_eq!(o.items[0].page_index(), Some(1));
    assert_eq!(o.items[0].children[0].title, "Chapter 2");
    assert_eq!(o.items[0].children[0].children[0].page_index(), Some(2));
    assert!(o.diagnostics.is_faithful(), "{:?}", o.diagnostics);
}

/// Would catch: the default changing behaviour.
#[test]
fn the_default_carries_no_bookmarks_and_says_so() {
    let src = load(BASIC);
    let mut s = target();
    let out = s
        .insert_pages(&src.view(), &[2], InsertPosition::End)
        .expect("insert");
    assert_eq!(out.outline_items_carried, 0);
    assert_eq!(out.dests_carried, 0);
    assert!(out.source_outline_dropped);
    assert!(read_outline(&saved(&s)).items.is_empty());
}

/// Would catch: the bookmarks or destinations surviving an undo of the
/// insert, i.e. written in a second command.
#[test]
fn one_undo_reverts_the_pages_the_bookmarks_and_the_destinations() {
    let src = load(NAMED);
    let mut s = target();
    s.insert_pages_with_options(
        &src.view(),
        &[1, 2],
        InsertPosition::End,
        &opts(InsertedOutline::Append, true),
    )
    .expect("insert");
    assert!(s.undo().is_some());
    assert_eq!(s.page_slots().expect("pages").len(), 1);
    assert!(read_outline(&s.view()).items.is_empty());
    assert_eq!(DestinationResolver::new(&s.view()).named_count(), 2);
}

/// Would catch: a carried key overwriting the target's, a destination for a
/// page left behind arriving, or a bookmark still naming the old key.
#[test]
fn named_destinations_for_inserted_pages_arrive_renamed_on_collision() {
    let src = load(NAMED);
    let mut s = target();
    let out = s
        .insert_pages_with_options(
            &src.view(),
            &[1, 2],
            InsertPosition::End,
            &opts(InsertedOutline::Append, true),
        )
        .expect("insert");
    assert_eq!(out.dests_carried, 2, "tree-body and tree-wrapped");
    assert_eq!(out.dests_renamed, 1, "tree-body is taken");

    let doc = saved(&s);
    let names = DestinationResolver::new(&doc);
    assert!(names.lookup(b"LegacyIntro").is_none(), "its page stayed");
    for key in [&b"tree-body"[..], b"tree-body_2", b"tree-wrapped", b"chap"] {
        assert!(
            names.lookup(key).is_some(),
            "{}",
            String::from_utf8_lossy(key)
        );
    }
    let o = read_outline(&doc);
    let titles: Vec<&str> = o.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["Tree body", "Tree wrapped"]);
    assert_eq!(o.items[0].page_index(), Some(1), "via tree-body_2");
    assert_eq!(o.items[1].page_index(), Some(2));
}

/// Would catch: a link on an inserted page left naming the target's own
/// `chap`, which is a different page.
#[test]
fn a_link_on_an_inserted_page_follows_its_renamed_destination() {
    let src = Document::from_bytes(build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /Dests << /chap [3 0 R /Fit] >> >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R] >>"),
        (
            4,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /Dest /chap >>",
        ),
        (
            5,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /A << /S /GoTo /D /chap >> >>",
        ),
    ]))
    .expect("source");
    let mut s = target();
    let out = s
        .insert_pages_with_options(
            &src.view(),
            &[0],
            InsertPosition::End,
            &opts(InsertedOutline::Drop, true),
        )
        .expect("insert");
    assert_eq!((out.dests_carried, out.dests_renamed), (1, 1));
    let text = String::from_utf8_lossy(
        &s.to_incremental_bytes(&SaveOptions::identity())
            .expect("save")
            .0,
    )
    .into_owned();
    assert_eq!(
        text.matches("(chap_2)").count(),
        3,
        "tree key + 2 links:\n{text}"
    );
    assert!(!text.contains("/chap"), "{text}");
}
