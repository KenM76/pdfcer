//! Grouping markup annotations with `/IRT` + `/RT /Group` and dissolving the
//! group (pdfcer-gui request G280).

use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotationGroupError, CommandKind, EditError, EditSession, GroupNesting};
use pdfcer_core::object::ObjId;
use pdfcer_core::writer::SaveOptions;

/// Page 1: 6 square, 7 circle, 8 link, 9 a text reply to 6, 10 line, 12 ink,
/// 13 stamp, 14 caret.
/// Page 2: 11 square.
fn fixture() -> Vec<u8> {
    let annot = |kind: &str, extra: &str| {
        format!("<< /Type /Annot /Subtype /{kind} /Rect [10 10 50 50] {extra} >>")
    };
    let page = |annots: &str| {
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [{annots}] >>")
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        page("6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 12 0 R 13 0 R 14 0 R"),
        page("11 0 R"),
        "<< >>".to_owned(),
        annot("Square", "/Contents (primary)"),
        annot("Circle", "/Contents (member)"),
        annot("Link", ""),
        annot("Text", "/IRT 6 0 R"),
        annot("Line", "/L [10 10 50 50]"),
        annot("Square", ""),
        annot("Ink", ""),
        annot("Stamp", ""),
        annot("Caret", ""),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(fixture()).unwrap())
}

/// `(in_reply_to, is_group_subordinate)` of an annotation, saved and reopened.
fn link(s: &EditSession, n: u32) -> (Option<ObjId>, bool) {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let a = pdfcer_core::annot::page_annotations(&doc.view(), pages[0].id)
        .into_iter()
        .find(|a| a.id == Some(id(n)))
        .unwrap();
    (a.in_reply_to, a.is_group_subordinate())
}

#[test]
fn members_join_and_one_undo_releases_them() {
    let mut s = session();
    let out = s.group_annotations(id(6), &[id(7), id(10), id(7)]).unwrap();
    assert_eq!((out.primary, out.members), (id(6), vec![id(7), id(10)]));
    assert_eq!(s.undo_kind(), Some(CommandKind::GroupAnnotations));
    assert_eq!(link(&s, 7), (Some(id(6)), true));
    assert_eq!(link(&s, 10), (Some(id(6)), true));
    assert_eq!(link(&s, 9), (Some(id(6)), false), "the reply stays a reply");

    // Grouping a member again writes nothing.
    let depth = s.undo_depth();
    assert!(
        s.group_annotations(id(6), &[id(7)])
            .unwrap()
            .members
            .is_empty()
    );
    assert_eq!(s.undo_depth(), depth);

    s.undo().unwrap();
    assert_eq!(link(&s, 7), (None, false));
    assert!(!s.can_undo());
}

#[test]
fn ungrouping_by_any_member_releases_the_group_and_not_the_replies() {
    let mut s = session();
    s.group_annotations(id(6), &[id(7), id(10)]).unwrap();
    let out = s.ungroup_annotations(id(10)).unwrap();
    assert_eq!((out.primary, out.members), (id(6), vec![id(7), id(10)]));
    assert_eq!(s.undo_kind(), Some(CommandKind::UngroupAnnotations));
    assert_eq!(link(&s, 7), (None, false));
    assert_eq!(link(&s, 10), (None, false));
    assert_eq!(link(&s, 9), (Some(id(6)), false));
    assert!(matches!(
        s.ungroup_annotations(id(6)),
        Err(EditError::AnnotationGroup(
            AnnotationGroupError::NotGrouped { .. }
        ))
    ));
}

#[test]
fn what_cannot_be_grouped_is_refused_by_name() {
    let refusal = |primary: u32, members: &[u32]| {
        let mut s = session();
        let members: Vec<ObjId> = members.iter().map(|&n| id(n)).collect();
        let err = s.group_annotations(id(primary), &members).unwrap_err();
        assert!(!s.can_undo());
        match err {
            EditError::AnnotationGroup(e) => e,
            other => panic!("{other:?}"),
        }
    };
    use AnnotationGroupError as E;
    assert!(matches!(refusal(6, &[]), E::NoMembers { .. }));
    assert!(matches!(refusal(6, &[6]), E::MemberIsPrimary { .. }));
    assert!(matches!(refusal(6, &[8]), E::NotMarkup { .. }));
    assert!(matches!(refusal(8, &[6]), E::NotMarkup { .. }));
    assert!(matches!(refusal(6, &[11]), E::DifferentPage { .. }));
    assert!(matches!(refusal(7, &[9]), E::AlreadyLinked { .. }));
    assert!(matches!(refusal(9, &[7]), E::AlreadyLinked { .. }));

    let mut s = session();
    s.group_annotations(id(6), &[id(7)]).unwrap();
    assert!(matches!(
        s.group_annotations(id(10), &[id(6)]),
        Err(EditError::AnnotationGroup(E::MemberHasGroup { .. }))
    ));
    assert!(matches!(
        s.group_annotations(id(6), &[id(99)]),
        Err(EditError::AnnotationNotFound { .. })
    ));
}

/// Groups 6+7 and 12+13, then 14 on its own.
fn two_groups() -> EditSession {
    let mut s = session();
    s.group_annotations(id(6), &[id(7)]).unwrap();
    s.group_annotations(id(12), &[id(13)]).unwrap();
    s
}

#[test]
fn a_merge_is_one_flat_group_and_ungrouping_gives_both_groups_back() {
    let mut s = two_groups();
    let depth = s.undo_depth();
    // Named by members on both sides; the loner comes along.
    let out = s
        .merge_annotation_groups(id(7), &[id(13), id(14)], GroupNesting::Recorded)
        .unwrap();
    assert_eq!(out.primary, id(6));
    assert_eq!(out.members, vec![id(7), id(12), id(13), id(14)]);
    assert_eq!(out.disclosures.len(), 1, "{:?}", out.disclosures);
    assert!(out.disclosures[0].contains("2 earlier group(s)"));
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(s.undo_kind(), Some(CommandKind::GroupAnnotations));
    for n in [7, 12, 13, 14] {
        assert_eq!(link(&s, n), (Some(id(6)), true), "annotation {n}");
    }

    let out = s.ungroup_annotations(id(13)).unwrap();
    assert_eq!(out.primary, id(6));
    assert_eq!(out.restored, vec![id(6), id(12)]);
    assert!(out.disclosures.is_empty());
    assert_eq!(link(&s, 7), (Some(id(6)), true));
    assert_eq!(link(&s, 13), (Some(id(12)), true));
    assert_eq!(link(&s, 12), (None, false));
    assert_eq!(
        link(&s, 14),
        (None, false),
        "the loner had no group to go back to"
    );

    // The inner groups are ordinary groups again: one more level each.
    let out = s.ungroup_annotations(id(12)).unwrap();
    assert!(out.restored.is_empty());
    assert_eq!(link(&s, 13), (None, false));
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let tail = String::from_utf8_lossy(&bytes[fixture().len()..]).into_owned();
    assert!(!tail.contains("pdfcer_Groups"), "a spent record is removed");
}

#[test]
fn three_levels_come_apart_one_at_a_time() {
    let mut s = two_groups();
    let nested = GroupNesting::Recorded;
    s.merge_annotation_groups(id(6), &[id(12)], nested).unwrap();
    s.merge_annotation_groups(id(14), &[id(6)], nested).unwrap();
    for n in [6, 7, 12, 13] {
        assert_eq!(link(&s, n), (Some(id(14)), true), "annotation {n}");
    }
    let out = s.ungroup_annotations(id(14)).unwrap();
    assert_eq!(out.restored, vec![id(6)]);
    assert_eq!(link(&s, 6), (None, false));
    for n in [7, 12, 13] {
        assert_eq!(link(&s, n), (Some(id(6)), true), "annotation {n}");
    }
    let out = s.ungroup_annotations(id(6)).unwrap();
    assert_eq!(out.restored, vec![id(6), id(12)]);
    assert_eq!(link(&s, 13), (Some(id(12)), true));

    for _ in 0..4 {
        s.undo().unwrap();
    }
    assert_eq!(link(&s, 7), (Some(id(6)), true));
    assert_eq!(link(&s, 13), (Some(id(12)), true));
    assert_eq!(link(&s, 14), (None, false));
}

#[test]
fn a_flat_merge_records_nothing_and_ungrouping_releases_everyone() {
    let mut s = two_groups();
    let out = s
        .merge_annotation_groups(id(6), &[id(12)], GroupNesting::Flat)
        .unwrap();
    assert_eq!(out.members, vec![id(12), id(13)]);
    assert!(
        out.disclosures[0].contains("merged flat"),
        "{:?}",
        out.disclosures
    );
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert!(!String::from_utf8_lossy(&bytes).contains("pdfcer_Groups"));
    let out = s.ungroup_annotations(id(6)).unwrap();
    assert!(out.restored.is_empty());
    for n in [7, 12, 13] {
        assert_eq!(link(&s, n), (None, false), "annotation {n}");
    }
}

#[test]
fn a_recorded_group_whose_primary_is_gone_releases_and_says_so() {
    let mut s = two_groups();
    s.merge_annotation_groups(id(6), &[id(12)], GroupNesting::Recorded)
        .unwrap();
    s.delete_annotation(id(12)).unwrap();
    let out = s.ungroup_annotations(id(6)).unwrap();
    assert_eq!(out.restored, vec![id(6)]);
    assert_eq!(link(&s, 13), (None, false));
    assert_eq!(out.disclosures.len(), 1);
    assert!(out.disclosures[0].starts_with("1 annotation(s)"));
}

#[test]
fn a_merge_refuses_what_it_cannot_keep_and_writes_nothing_when_already_one() {
    let mut s = two_groups();
    let depth = s.undo_depth();
    let nested = GroupNesting::Recorded;
    use AnnotationGroupError as E;
    let refused = |s: &mut EditSession, primary: u32, others: &[u32]| {
        let others: Vec<ObjId> = others.iter().map(|&n| id(n)).collect();
        match s.merge_annotation_groups(id(primary), &others, nested) {
            Err(EditError::AnnotationGroup(e)) => e,
            other => panic!("{other:?}"),
        }
    };
    assert!(matches!(refused(&mut s, 6, &[]), E::NoMembers { .. }));
    assert!(matches!(refused(&mut s, 6, &[8]), E::NotMarkup { .. }));
    assert!(matches!(refused(&mut s, 6, &[11]), E::DifferentPage { .. }));
    assert!(matches!(refused(&mut s, 6, &[9]), E::AlreadyLinked { .. }));
    assert!(matches!(refused(&mut s, 9, &[12]), E::AlreadyLinked { .. }));
    let out = s.merge_annotation_groups(id(6), &[id(7)], nested).unwrap();
    assert!(out.members.is_empty() && out.disclosures.is_empty());
    assert_eq!(s.undo_depth(), depth);
}
