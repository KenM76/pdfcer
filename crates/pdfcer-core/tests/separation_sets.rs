//! # Preseparated page sets survive structural page operations
//!
//! ISO 32000-1 §14.11.4 lets one logical page be **several page objects**,
//! one per printing plate, tied together by a `/SeparationInfo` dictionary.
//! Table 364 states the tie in unusually strong terms:
//!
//! > "One of the page objects in the array shall be the one with which
//! > this separation dictionary is associated, and **all of them shall
//! > have separation dictionaries (`SeparationInfo` entries) containing
//! > `Pages` arrays identical to this one**."
//!
//! `Pages` is **Required**. So a page operation that selects some members
//! of a set and not others falsifies every survivor, and pdfcer shipped
//! four such operations — delete, extract, split, merge — with no
//! knowledge of the key at all.
//!
//! ## The two distinct defects these tests pin
//!
//! They failed differently, which is why the fix has two halves and this
//! suite has two halves.
//!
//! - **Delete** (`EditSession::delete_pages`, incremental save) left the
//!   surviving plates' `/Pages` arrays *verbatim*, naming an object that
//!   had just been written as a §7.5.4 free entry. A dangling reference
//!   in a Required array, uncounted by `census_dangling`, which walks
//!   outlines, links, named destinations and page labels — and not this.
//! - **Extract / split** (`pageops::assemble`) hit the copier's barrier.
//!   Its documented rule is that an array containing a barrier hit refuses
//!   *as a whole*, and a dictionary entry whose value refused is
//!   *dropped* — generically right, specifically wrong here, because what
//!   it dropped was a Required entry. The output kept a separation
//!   dictionary with **no `/Pages` at all**, and the loss was reported as
//!   one anonymous tick in `dangling_references`, indistinguishable from a
//!   broken link.
//!
//! ## Why the crown assertion is `separation_arrays_are_identical`
//!
//! Checking that an array "no longer mentions the deleted page" is the
//! weaker test and it would pass for a repair that produced *different*
//! arrays on different survivors — which is still a §14.11.4 violation,
//! and a subtler one. Table 364's word is **identical**, so the assertion
//! is identity across every member, and it is applied to the **saved
//! bytes** (R159), never to the in-memory model that the repair just
//! wrote.

use std::collections::HashSet;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::pageops::{
    DocumentView, SeparationPolicy, SplitCriterion, extract, merge, separation_of, split,
};
use pdfcer_core::writer::SaveOptions;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
///
/// A local copy of `page_ops.rs`'s helper, deliberately: the in-crate
/// `tests_support::build_pdf` is `pub(crate)` on the stated grounds that a
/// minimal-PDF builder is a testing tool and not part of the engine's API,
/// and an integration test sits outside the crate.
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

/// A preseparated CMYK page: **four** page objects (3, 4, 5, 6) that are
/// one logical page, plus an ordinary fifth page (7) that is not part of
/// any set.
///
/// Every plate's `/Pages` array is the same four references in the same
/// order, which is what Table 364 requires and therefore what the
/// operations under test must preserve or correctly reduce. Each plate
/// carries a distinct `/DeviceColorant` so the disclosure has something
/// to name.
fn cmyk_preseparated() -> Vec<u8> {
    const SET: &str = "[3 0 R 4 0 R 5 0 R 6 0 R]";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R] /Count 5 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (
            3,
            &format!(
                "<< /Type /Page /Parent 2 0 R /SeparationInfo \
                 << /Pages {SET} /DeviceColorant /Cyan >> >>"
            ),
        ),
        (
            4,
            &format!(
                "<< /Type /Page /Parent 2 0 R /SeparationInfo \
                 << /Pages {SET} /DeviceColorant /Magenta >> >>"
            ),
        ),
        (
            5,
            &format!(
                "<< /Type /Page /Parent 2 0 R /SeparationInfo \
                 << /Pages {SET} /DeviceColorant /Yellow >> >>"
            ),
        ),
        (
            6,
            &format!(
                "<< /Type /Page /Parent 2 0 R /SeparationInfo \
                 << /Pages {SET} /DeviceColorant /Black >> >>"
            ),
        ),
        (7, "<< /Type /Page /Parent 2 0 R >>"),
    ])
}

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).expect("fixture must load"))
}

fn save_incremental(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed")
        .0
}

// ---------------------------------------------------------------------------
// Reading the result back — always from the saved bytes (R159)
// ---------------------------------------------------------------------------

/// Every page of `doc`, in tree order.
fn pages_of(doc: &Document) -> Vec<ObjId> {
    pdfcer_core::page_tree::pages(doc)
        .expect("page tree must walk")
        .into_iter()
        .map(|slot| slot.id)
        .collect()
}

fn page_dict(doc: &Document, id: ObjId) -> Dict {
    doc.resolved(id)
        .as_dict()
        .expect("a page must be a dictionary")
        .clone()
}

/// The `/SeparationInfo /Pages` array of `page`, as object ids.
///
/// Panics if the page has a separation dictionary without the Required
/// `/Pages` — which is the exact shipped defect on the assemble path, so
/// the panic message says so rather than reading as a fixture problem.
fn separation_members(doc: &Document, page: ObjId) -> Option<Vec<ObjId>> {
    let dict = page_dict(doc, page);
    let info = dict.get(b"SeparationInfo").map(|v| doc.resolve(v))?;
    let info = info
        .as_dict()
        .expect("/SeparationInfo must be a dictionary");
    let pages = info
        .get(b"Pages")
        .map(|v| doc.resolve(v))
        .unwrap_or_else(|| {
            panic!(
                "§14.11.4 Table 364 makes /Pages REQUIRED, and it is absent — \
             this is the copier's barrier dropping the entry"
            )
        });
    Some(
        pages
            .as_array()
            .expect("/Pages must be an array")
            .iter()
            .filter_map(Object::as_reference)
            .collect(),
    )
}

fn colorant(doc: &Document, page: ObjId) -> Option<Vec<u8>> {
    separation_of(doc, &page_dict(doc, page)).and_then(|dict| dict.colorant)
}

/// Table 364's actual requirement, asserted directly: every member of a
/// set holds the **same** `/Pages` array, and each member's array contains
/// that member.
fn assert_set_invariant(doc: &Document) {
    let pages = pages_of(doc);
    let mut arrays: Vec<(ObjId, Vec<ObjId>)> = Vec::new();
    for page in &pages {
        if let Some(members) = separation_members(doc, *page) {
            assert!(
                members.contains(page),
                "Table 364: a member's /Pages must contain the member itself, \
                 but {page:?}'s array is {members:?}"
            );
            arrays.push((*page, members));
        }
    }
    // Members of the same set must agree. With one set per fixture this is
    // a global check; grouping by array identity would hide exactly the
    // disagreement being tested for.
    if let Some((first_id, first)) = arrays.first() {
        for (id, members) in &arrays[1..] {
            assert_eq!(
                first, members,
                "Table 364 requires IDENTICAL /Pages arrays across a set, \
                 but {first_id:?} has {first:?} and {id:?} has {members:?}"
            );
        }
    }
    // And nothing may name a page that is not in the document.
    let live: HashSet<ObjId> = pages.iter().copied().collect();
    for (id, members) in &arrays {
        for member in members {
            assert!(
                live.contains(member),
                "{id:?}'s /Pages names {member:?}, which is not a page of this document"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Delete — the incremental-save path
// ---------------------------------------------------------------------------

#[test]
fn deleting_one_plate_repairs_the_surviving_plates() {
    // The headline defect: before the fix, pages 4/5/6 kept `[3 0 R 4 0 R
    // 5 0 R 6 0 R]` while object 3 was written as a free entry.
    let source = cmyk_preseparated();
    let mut s = session(&source);
    s.delete_pages(&[0]).expect("delete must succeed");
    let after = Document::from_bytes(save_incremental(&s)).expect("result must load");

    assert_set_invariant(&after);

    let pages = pages_of(&after);
    let members = separation_members(&after, pages[0]).expect("the plates keep their dictionaries");
    assert_eq!(members.len(), 3, "one plate left, three remain");
    assert!(
        !members.contains(&ObjId::new(3, 0)),
        "the deleted plate must not be named any more"
    );
}

#[test]
fn the_delete_names_the_colorant_that_left() {
    // Rule 4: "3 separations removed" is not actionable; "you removed
    // Cyan" is. The name is read from the DEPARTING page's own dictionary,
    // which is only readable before the sweep — so this also pins that the
    // census happens at the right moment.
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0]).expect("delete must succeed");

    assert_eq!(
        outcome.separations.sets_split, 3,
        "three survivors repaired"
    );
    assert_eq!(outcome.separations.pages_changed, 3);
    assert_eq!(
        outcome.separations.colorants_removed,
        vec![b"Cyan".to_vec()],
        "the departed plate is named, once"
    );
    let kept = &outcome.separations.colorants_kept;
    assert!(kept.contains(&b"Magenta".to_vec()) && kept.contains(&b"Black".to_vec()));
    assert!(!outcome.separations.is_empty());
}

#[test]
fn deleting_two_plates_names_both_and_leaves_two() {
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0, 3]).expect("delete must succeed");

    assert_eq!(
        outcome.separations.colorants_removed,
        vec![b"Cyan".to_vec(), b"Black".to_vec()],
        "both departed plates named, in encounter order, no duplicates"
    );
    let after = Document::from_bytes(save_incremental(&s)).expect("result must load");
    assert_set_invariant(&after);
    let pages = pages_of(&after);
    assert_eq!(separation_members(&after, pages[0]).unwrap().len(), 2);
}

#[test]
fn deleting_an_unrelated_page_touches_no_separation_object() {
    // Rule 3's minimal-diff obligation is a constraint ON the repair, not
    // an exception to it: a set that lost nothing is already correct, so
    // not one plate object may be re-emitted. Asserted at the byte level,
    // because a re-emitted-but-equal dictionary would pass a value check
    // and still be a diff.
    let source = cmyk_preseparated();
    let before = Document::from_bytes(source.clone()).expect("fixture must load");
    let mut s = session(&source);
    let outcome = s.delete_pages(&[4]).expect("deleting the ordinary page");

    assert!(
        outcome.separations.is_empty(),
        "no set lost a member, so there is nothing to disclose"
    );

    let after = Document::from_bytes(save_incremental(&s)).expect("result must load");
    for num in [3u32, 4, 5, 6] {
        let id = ObjId::new(num, 0);
        let was = before
            .get(id)
            .and_then(|io| io.file_span())
            .and_then(|span| span.slice(before.bytes()))
            .expect("the plate is a file-level object in the fixture");
        let now = after
            .get(id)
            .and_then(|io| io.file_span())
            .and_then(|span| span.slice(after.bytes()));
        assert_eq!(
            now,
            Some(was),
            "plate {num} lost its verbatim bytes: an intact set must not be re-emitted"
        );
    }
}

#[test]
fn a_document_with_no_separations_reports_nothing() {
    // The overwhelmingly common case. One key lookup per page, no writes,
    // nothing to say.
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
    ]);
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0]).expect("delete must succeed");
    assert!(outcome.separations.is_empty());
    assert_eq!(outcome.separations.pages_changed, 0);
}

#[test]
fn refusing_declines_the_split_by_name() {
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let error = s
        .delete_pages_with(&[0], SeparationPolicy::Refuse)
        .expect_err("Refuse must decline");

    assert!(matches!(error, EditError::SeparationSplit(_)));
    let message = error.to_string();
    assert!(
        message.contains("14.11.4") && message.contains("preseparated"),
        "the refusal must name the reason, not just fail: {message}"
    );
}

#[test]
fn refusing_leaves_the_document_untouched() {
    // R27's fail-clean posture: a refusal that half-applied would be worse
    // than the corruption it declined to cause.
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let _ = s.delete_pages_with(&[0], SeparationPolicy::Refuse);
    assert_eq!(
        save_incremental(&s),
        source,
        "a refused delete must not change one byte"
    );
}

#[test]
fn refusing_still_allows_deleting_a_page_outside_any_set() {
    // Refuse must decline the split, not the operation category.
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let outcome = s
        .delete_pages_with(&[4], SeparationPolicy::Refuse)
        .expect("the ordinary page is in no set");
    assert_eq!(outcome.pages_removed, 1);
}

#[test]
fn the_impact_records_which_policy_produced_it() {
    // Added after a real defect: the CLI and GUI disclosures announced
    // that surviving pages "had their /Pages array rewritten" under EVERY
    // policy, which is false for `Discard` — that removes the dictionary
    // outright. The counts were right and the sentence was wrong, which
    // is the worse failure, because a true-looking disclosure gets
    // believed. The fix was to carry the policy on the impact so a front
    // end can describe what actually happened instead of assuming; this
    // pins that it is carried.
    let source = cmyk_preseparated();

    for policy in [SeparationPolicy::Repair, SeparationPolicy::Discard] {
        let mut s = session(&source);
        let outcome = s
            .delete_pages_with(&[0], policy)
            .expect("both policies succeed");
        assert_eq!(
            outcome.separations.policy, policy,
            "the impact must say which policy produced it"
        );
    }

    // And an untouched document reports the default rather than junk.
    let mut s = session(&source);
    let outcome = s.delete_pages(&[4]).expect("the ordinary page");
    assert!(outcome.separations.is_empty());
}

#[test]
fn discarding_demotes_the_survivors_to_ordinary_pages() {
    let source = cmyk_preseparated();
    let mut s = session(&source);
    let outcome = s
        .delete_pages_with(&[0], SeparationPolicy::Discard)
        .expect("Discard must succeed");
    assert_eq!(outcome.separations.pages_changed, 3);

    let after = Document::from_bytes(save_incremental(&s)).expect("result must load");
    for page in pages_of(&after) {
        assert!(
            separation_members(&after, page).is_none(),
            "Discard removes the dictionary entirely"
        );
    }
}

#[test]
fn a_malformed_separation_dictionary_is_counted_not_invented() {
    // `/SeparationInfo` with no `/Pages` is already non-conforming on
    // arrival. Repairing it would mean guessing which pages were meant to
    // be in the set, and the file holds no evidence to guess from.
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /SeparationInfo << /DeviceColorant /Cyan >> >>",
        ),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
    ]);
    let mut s = session(&source);
    let outcome = s.delete_pages(&[1]).expect("delete must succeed");

    assert_eq!(outcome.separations.malformed, 1, "counted");
    assert_eq!(outcome.separations.pages_changed, 0, "and left alone");
}

#[test]
fn undo_restores_the_repaired_plates_byte_for_byte() {
    // §11.1's contract, on the operation that now writes objects the
    // operator did not name. A repair that could not be undone would have
    // traded one silent corruption for another.
    let source = cmyk_preseparated();
    let mut s = session(&source);
    s.delete_pages(&[0]).expect("delete must succeed");
    s.undo().expect("undo must succeed");
    assert_eq!(
        save_incremental(&s),
        source,
        "edit → undo → save must be byte-identical to the input"
    );
}

// ---------------------------------------------------------------------------
// Extract / split / merge — the assemble path
// ---------------------------------------------------------------------------

#[test]
fn extracting_one_plate_keeps_the_required_pages_entry() {
    // The assemble-path defect, pinned at its narrowest: before the fix
    // the output carried `/SeparationInfo << /DeviceColorant /Cyan >>`
    // with the Required `/Pages` silently gone, and the loss showed up as
    // one anonymous tick in `dangling_references`.
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let (bytes, report) = extract(&view, &[0]).expect("extract must succeed");

    let out = Document::from_bytes(bytes).expect("result must load");
    assert_set_invariant(&out);

    let pages = pages_of(&out);
    let members = separation_members(&out, pages[0]).expect("the dictionary is carried");
    assert_eq!(
        members,
        vec![pages[0]],
        "a one-member set naming itself — Table 364 sets no minimum length"
    );
    assert_eq!(
        colorant(&out, pages[0]),
        Some(b"Cyan".to_vec()),
        "which plate this was is exactly the information worth keeping"
    );
    assert_eq!(report.separations.sets_split, 1);
    assert_eq!(
        report.separations.colorants_removed,
        vec![b"Magenta".to_vec(), b"Yellow".to_vec(), b"Black".to_vec()]
    );
}

#[test]
fn extracting_two_plates_remaps_both_and_names_the_rest() {
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let (bytes, report) = extract(&view, &[0, 1]).expect("extract must succeed");

    let out = Document::from_bytes(bytes).expect("result must load");
    assert_set_invariant(&out);

    let pages = pages_of(&out);
    assert_eq!(pages.len(), 2);
    let members = separation_members(&out, pages[0]).expect("dictionary carried");
    assert_eq!(
        members, pages,
        "both survivors are named, in output object space"
    );
    assert_eq!(
        report.separations.colorants_removed,
        vec![b"Yellow".to_vec(), b"Black".to_vec()]
    );
}

#[test]
fn extracting_the_whole_set_changes_nothing_and_reports_nothing() {
    // No member was barriered, so the generic copier's own remapping is
    // already correct and complete. The repair must recognise that and
    // stand down.
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let (bytes, report) = extract(&view, &[0, 1, 2, 3]).expect("extract must succeed");

    assert!(
        report.separations.is_empty(),
        "an intact set is not a split set"
    );
    let out = Document::from_bytes(bytes).expect("result must load");
    assert_set_invariant(&out);
    let pages = pages_of(&out);
    assert_eq!(
        separation_members(&out, pages[0]).unwrap(),
        pages,
        "all four, remapped into the output"
    );
}

#[test]
fn extracting_the_ordinary_page_leaves_the_sets_alone() {
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let (bytes, report) = extract(&view, &[4]).expect("extract must succeed");

    assert!(report.separations.is_empty());
    let out = Document::from_bytes(bytes).expect("result must load");
    assert!(separation_members(&out, pages_of(&out)[0]).is_none());
}

#[test]
fn splitting_shatters_every_set_into_valid_one_member_sets() {
    // Split is the harshest case: each plate lands in its own file, so
    // every set loses three of four members at once.
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let parts =
        split(&view, &SplitCriterion::EveryN(1), "{stem}-{n}.pdf", "job").expect("split succeeds");

    assert_eq!(parts.len(), 5);
    for (index, (_, bytes, _)) in parts.iter().enumerate() {
        let out = Document::from_bytes(bytes.clone()).expect("each part must load");
        assert_set_invariant(&out);
        let page = pages_of(&out)[0];
        if index < 4 {
            assert_eq!(
                separation_members(&out, page),
                Some(vec![page]),
                "part {index} is a conforming one-member set"
            );
        } else {
            assert!(separation_members(&out, page).is_none());
        }
    }
}

#[test]
fn merging_preserves_an_intact_set_across_documents() {
    // Merge copies every page of every source, so no set can lose a
    // member — but the object numbers all change, and the arrays must
    // follow them into the output.
    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let plain = Document::from_bytes(build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
    ]))
    .expect("fixture must load");

    let views = [
        DocumentView::new(&plain, plain.bytes(), plain.version()),
        DocumentView::new(&doc, doc.bytes(), doc.version()),
    ];
    let (bytes, report) = merge(&views, &[]).expect("merge must succeed");

    assert!(
        report.separations.is_empty(),
        "nothing was split, so nothing is disclosed"
    );
    let out = Document::from_bytes(bytes).expect("result must load");
    assert_set_invariant(&out);

    let pages = pages_of(&out);
    let members = separation_members(&out, pages[1]).expect("the set survived the merge");
    assert_eq!(
        members,
        pages[1..5].to_vec(),
        "four plates, remapped past the plain document's page"
    );
}

#[test]
fn extract_can_refuse_to_split_a_set() {
    use pdfcer_core::pageops::{AssembleOptions, PageOpError, assemble};

    let source = cmyk_preseparated();
    let doc = Document::from_bytes(source).expect("fixture must load");
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let options = AssembleOptions::default().with_separations(SeparationPolicy::Refuse);
    let error = assemble(&[view], &[(0, 0)], &options).expect_err("Refuse must decline");
    assert!(matches!(error, PageOpError::SeparationSplit(_)));
}
