//! Session page import keeps preseparated page sets truthful (ISO 32000-2
//! §14.11.4, Table 364): `EditSession::insert_pages*`, `paste_pages`,
//! `copy_pages` and `merge_document` copy a plate's `/SeparationInfo` with a
//! `/Pages` array naming only the plates copied in the same operation, never
//! follow `/Pages` into a page that was not asked for, and honour
//! [`SeparationPolicy`]. Every assertion reads the saved bytes.

use std::collections::BTreeSet;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::pageops::{
    DocumentView, InsertOptions, InsertPosition, SeparationPolicy, extract, merge,
};
use pdfcer_core::writer::SaveOptions;

/// An offset-consistent classic PDF from `(number, body)` pairs.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

const TREE: &str = "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R] /Count 5 \
                    /MediaBox [0 0 200 100] /Resources << >> >>";

/// Four plates (3–6) of one logical page, each with its own direct
/// `/SeparationInfo`, plus an ordinary page 7.
fn cmyk_direct() -> Vec<u8> {
    let plate = |colorant: &str| {
        format!(
            "<< /Type /Page /Parent 2 0 R /SeparationInfo \
             << /Pages [3 0 R 4 0 R 5 0 R 6 0 R] /DeviceColorant /{colorant} >> >>"
        )
    };
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, TREE),
        (3, &plate("Cyan")),
        (4, &plate("Magenta")),
        (5, &plate("Yellow")),
        (6, &plate("Black")),
        (7, "<< /Type /Page /Parent 2 0 R >>"),
    ])
}

/// The same four plates sharing ONE indirect `/SeparationInfo` (object 8).
fn cmyk_shared() -> Vec<u8> {
    let plate = "<< /Type /Page /Parent 2 0 R /SeparationInfo 8 0 R >>";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, TREE),
        (3, plate),
        (4, plate),
        (5, plate),
        (6, plate),
        (7, "<< /Type /Page /Parent 2 0 R >>"),
        (
            8,
            "<< /Pages [3 0 R 4 0 R 5 0 R 6 0 R] /DeviceColorant /Process >>",
        ),
    ])
}

/// A one-page document with no separations.
fn plain() -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
    ])
}

fn load(bytes: &[u8]) -> Document {
    Document::from_bytes(bytes.to_vec()).expect("fixture must load")
}

fn saved(session: &EditSession) -> Document {
    let bytes = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed")
        .0;
    load(&bytes)
}

fn pages_of(doc: &Document) -> Vec<ObjId> {
    pdfcer_core::page_tree::pages(doc)
        .expect("page tree must walk")
        .into_iter()
        .map(|slot| slot.id)
        .collect()
}

/// `page`'s `/SeparationInfo /Pages`, or `None` without a `/SeparationInfo`.
fn members(doc: &Document, page: ObjId) -> Option<Vec<ObjId>> {
    let dict = doc
        .resolved(page)
        .as_dict()
        .expect("a page is a dictionary");
    let info = doc.resolve(dict.get(b"SeparationInfo")?);
    let info = info.as_dict().expect("/SeparationInfo is a dictionary");
    let pages = info
        .get(b"Pages")
        .map(|v| doc.resolve(v))
        .expect("Table 364 makes /Pages REQUIRED");
    Some(
        pages
            .as_array()
            .expect("/Pages is an array")
            .iter()
            .filter_map(Object::as_reference)
            .collect(),
    )
}

/// Every object reachable from the catalog.
fn reachable(doc: &Document) -> BTreeSet<ObjId> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<Object> = doc.trailer_entry(b"Root").cloned().into_iter().collect();
    while let Some(value) = stack.pop() {
        match value {
            Object::Reference(id) => {
                if seen.insert(id)
                    && let Some(inner) = doc.value(id)
                {
                    stack.push(inner.clone());
                }
            }
            Object::Array(items) => stack.extend(items),
            Object::Dict(dict) => stack.extend(dict.iter().map(|(_, v)| v.clone())),
            Object::Stream(stream) => stream.dict.iter().for_each(|(_, v)| stack.push(v.clone())),
            _ => {}
        }
    }
    seen
}

/// Live objects nothing reaches that are a page or a separation dictionary
/// — the phantom plates and orphaned `/SeparationInfo` copies this suite
/// exists to rule out.
fn orphans(doc: &Document) -> Vec<ObjId> {
    let live = reachable(doc);
    let mut out: Vec<ObjId> = doc
        .objects()
        .filter(|o| !live.contains(&o.id))
        .filter(|o| {
            o.value.as_dict().is_some_and(|d| {
                d.contains_key(b"DeviceColorant")
                    || d.get(b"Type")
                        .and_then(Object::as_name)
                        .map(|n| n.as_bytes())
                        == Some(b"Page")
            })
        })
        .map(|o| o.id)
        .collect();
    out.sort();
    out
}

/// Every page's `/Pages` names only pages in the tree, including itself.
fn assert_members_live(doc: &Document) {
    let pages = pages_of(doc);
    for page in &pages {
        if let Some(set) = members(doc, *page) {
            assert!(set.contains(page), "{page:?}'s set {set:?} omits itself");
            for m in &set {
                assert!(
                    pages.contains(m),
                    "{page:?}'s set names {m:?}, not in the tree"
                );
            }
        }
    }
}

fn insert(target: &[u8], source: &[u8], pages: &[usize]) -> (EditSession, Document) {
    let src = load(source);
    let mut session = EditSession::new(load(target));
    let out = session
        .insert_pages(&src.view(), pages, InsertPosition::End)
        .expect("insert must succeed");
    assert_eq!(out.pages_inserted, pages.len());
    let doc = saved(&session);
    (session, doc)
}

#[test]
fn two_plates_of_one_split_set_count_one_set_and_two_pages() {
    let src = load(&cmyk_direct());
    let mut session = EditSession::new(load(&plain()));
    let out = session
        .insert_pages(&src.view(), &[0, 2], InsertPosition::End)
        .expect("insert must succeed");
    assert_eq!(
        out.separations.sets_split, 1,
        "one source set, counted once"
    );
    assert_eq!(out.separations.pages_changed, 2);
}

#[test]
fn inserting_one_plate_adds_one_page_whose_set_names_only_itself() {
    let src = load(&cmyk_direct());
    let mut session = EditSession::new(load(&plain()));
    let out = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert must succeed");
    let doc = saved(&session);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 2);
    assert_eq!(
        orphans(&doc),
        Vec::<ObjId>::new(),
        "sibling plates copied as phantoms"
    );
    assert_eq!(members(&doc, pages[1]), Some(vec![pages[1]]));
    assert_eq!(out.separations.sets_split, 1);
    assert_eq!(out.separations.pages_changed, 1);
    assert_eq!(out.separations.policy, SeparationPolicy::Repair);
    assert_eq!(out.separations.colorants_kept, vec![b"Cyan".to_vec()]);
    assert_eq!(
        out.separations.colorants_removed,
        vec![b"Magenta".to_vec(), b"Yellow".to_vec(), b"Black".to_vec()]
    );
}

#[test]
fn inserting_all_four_plates_in_one_call_remaps_the_whole_set() {
    let src = load(&cmyk_direct());
    let mut session = EditSession::new(load(&plain()));
    let out = session
        .insert_pages(&src.view(), &[0, 1, 2, 3], InsertPosition::End)
        .expect("insert must succeed");
    assert!(out.separations.is_empty(), "nothing was split");
    let doc = saved(&session);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 5);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    for page in &pages[1..] {
        assert_eq!(members(&doc, *page), Some(pages[1..].to_vec()));
    }
}

#[test]
fn inserting_the_same_plate_twice_gives_two_one_member_sets() {
    let (_, doc) = insert(&plain(), &cmyk_direct(), &[0, 0]);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 3);
    assert_ne!(pages[1], pages[2]);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_eq!(members(&doc, pages[1]), Some(vec![pages[1]]));
    assert_eq!(members(&doc, pages[2]), Some(vec![pages[2]]));
}

#[test]
fn a_repeat_beside_its_whole_set_copies_no_phantom_plates() {
    // The repeat is copied through a fresh mapping; followed, its `/Pages`
    // would deep-copy the three selected siblings a second time.
    let (_, doc) = insert(&plain(), &cmyk_direct(), &[0, 1, 2, 3, 0]);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 6);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    for page in &pages[1..5] {
        assert_eq!(members(&doc, *page), Some(pages[1..5].to_vec()));
    }
    assert_eq!(members(&doc, pages[5]), Some(vec![pages[5]]));
}

#[test]
fn inserting_one_plate_of_a_shared_set_leaves_no_orphaned_copy() {
    let (_, doc) = insert(&plain(), &cmyk_shared(), &[1]);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 2);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_eq!(members(&doc, pages[1]), Some(vec![pages[1]]));
}

#[test]
fn two_plates_of_a_shared_set_share_one_rewritten_dictionary() {
    let (_, doc) = insert(&plain(), &cmyk_shared(), &[0, 2]);
    let pages = pages_of(&doc);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_eq!(members(&doc, pages[1]), Some(pages[1..].to_vec()));
    assert_eq!(members(&doc, pages[2]), Some(pages[1..].to_vec()));
    let info = |p: ObjId| {
        doc.resolved(p)
            .as_dict()
            .unwrap()
            .get(b"SeparationInfo")
            .cloned()
    };
    assert!(matches!(info(pages[1]), Some(Object::Reference(_))));
    assert_eq!(info(pages[1]), info(pages[2]), "still one shared object");
}

#[test]
fn a_shared_set_after_delete_names_only_pages_in_the_tree() {
    let mut session = EditSession::new(load(&cmyk_shared()));
    let out = session.delete_pages(&[0]).expect("delete must succeed");
    assert_eq!(out.separations.sets_split, 1);
    let doc = saved(&session);
    assert_members_live(&doc);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    let pages = pages_of(&doc);
    assert_eq!(members(&doc, pages[0]), Some(pages[..3].to_vec()));
}

#[test]
fn a_shared_set_after_extract_names_only_the_extracted_page() {
    let doc = load(&cmyk_shared());
    let (bytes, report) = extract(&doc.view(), &[0]).expect("extract must succeed");
    assert_eq!(report.separations.sets_split, 1);
    let out = load(&bytes);
    let pages = pages_of(&out);
    assert_eq!(orphans(&out), Vec::<ObjId>::new());
    assert_eq!(members(&out, pages[0]), Some(vec![pages[0]]));
}

#[test]
fn copy_then_paste_of_one_shared_plate_leaves_one_member_set() {
    let source = EditSession::new(load(&cmyk_shared()));
    let clip = source.copy_pages(&[3]).expect("copy must succeed");
    let mut target = EditSession::new(load(&plain()));
    target
        .paste_pages(&clip, InsertPosition::End)
        .expect("paste must succeed");
    let doc = saved(&target);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 2);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_eq!(members(&doc, pages[1]), Some(vec![pages[1]]));
}

#[test]
fn merging_two_preseparated_files_keeps_each_set_to_its_own_copies() {
    let direct = load(&cmyk_direct());
    let shared = load(&cmyk_shared());

    let mut session = EditSession::new(load(&cmyk_direct()));
    session
        .merge_document(&shared.view(), InsertPosition::End)
        .expect("merge must succeed");
    let doc = saved(&session);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 10);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_members_live(&doc);
    assert_eq!(members(&doc, pages[0]), Some(pages[..4].to_vec()));
    assert_eq!(members(&doc, pages[5]), Some(pages[5..9].to_vec()));

    let views = [
        DocumentView::new(&direct, direct.bytes(), direct.version()),
        DocumentView::new(&shared, shared.bytes(), shared.version()),
    ];
    let (bytes, report) = merge(&views, &[], &[]).expect("merge must succeed");
    assert!(report.separations.is_empty());
    let out = load(&bytes);
    let pages = pages_of(&out);
    assert_eq!(orphans(&out), Vec::<ObjId>::new());
    assert_eq!(members(&out, pages[0]), Some(pages[..4].to_vec()));
    assert_eq!(members(&out, pages[5]), Some(pages[5..9].to_vec()));
}

#[test]
fn inserting_from_two_preseparated_files_keeps_the_sets_apart() {
    let (mut session, _) = insert(&plain(), &cmyk_direct(), &[0, 1, 2, 3]);
    let shared = load(&cmyk_shared());
    session
        .insert_pages(&shared.view(), &[0, 1, 2, 3], InsertPosition::End)
        .expect("second insert must succeed");
    let doc = saved(&session);
    let pages = pages_of(&doc);
    assert_eq!(pages.len(), 9);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
    assert_eq!(members(&doc, pages[1]), Some(pages[1..5].to_vec()));
    assert_eq!(members(&doc, pages[5]), Some(pages[5..9].to_vec()));
}

#[test]
fn discard_policy_inserts_an_ordinary_page() {
    let src = load(&cmyk_direct());
    let mut session = EditSession::new(load(&plain()));
    let opts = InsertOptions::default().with_separations(SeparationPolicy::Discard);
    let out = session
        .insert_pages_with_options(&src.view(), &[0], InsertPosition::End, &opts)
        .expect("insert must succeed");
    assert_eq!(out.separations.policy, SeparationPolicy::Discard);
    assert_eq!(out.separations.pages_changed, 1);
    let doc = saved(&session);
    let pages = pages_of(&doc);
    assert_eq!(members(&doc, pages[1]), None);
    assert_eq!(orphans(&doc), Vec::<ObjId>::new());
}

#[test]
fn refuse_policy_declines_a_split_and_changes_nothing() {
    let src = load(&cmyk_direct());
    let mut session = EditSession::new(load(&plain()));
    let opts = InsertOptions::default().with_separations(SeparationPolicy::Refuse);
    let err = session
        .insert_pages_with_options(&src.view(), &[0], InsertPosition::End, &opts)
        .expect_err("Refuse must decline");
    assert!(matches!(err, EditError::SeparationSplit(_)));
    assert!(!session.can_undo(), "nothing was committed");
    assert_eq!(pages_of(&saved(&session)).len(), 1);
    // The whole set is not a split, so Refuse lets it through.
    session
        .insert_pages_with_options(&src.view(), &[0, 1, 2, 3], InsertPosition::End, &opts)
        .expect("an intact set is not refused");
}

#[test]
fn a_split_insert_is_one_undo_step() {
    let (mut session, _) = insert(&plain(), &cmyk_direct(), &[2]);
    assert!(session.undo().is_some());
    assert!(!session.can_undo());
    assert_eq!(pages_of(&saved(&session)).len(), 1);
}
