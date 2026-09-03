//! Integration tests for the document-outline reader
//! ([`pdfcer_core::outline`], ISO 32000-1 §12.3.3 / §12.3.2).
//!
//! ## Why these exist alongside the module's own unit tests
//!
//! `src/outline.rs`'s unit tests drive a hand-built `ObjectGraph`. That
//! is the right tool for pinning traversal *logic* — it can express an
//! outline shape no file format would let you write down twice — but it
//! bypasses the lexer, the object parser, and the cross-reference table
//! entirely. A reader that is correct over a synthesised graph and wrong
//! over parsed bytes is a reader that fails on every real document while
//! its test suite stays green.
//!
//! These tests therefore run the same claims through **whole files**,
//! parsed from bytes by [`Document::from_bytes`]. Every fixture is
//! wholly synthetic and byte-authored by `tools/gen-outline-fixtures.py`
//! (`docs/LEGAL.md` §5 category (a)); see
//! `fixtures/synthetic/outline/PROVENANCE.md`.
//!
//! Each test's doc comment says what defect it would catch. That is the
//! project's standing bar for a test: a failure should name the mistake,
//! not just report that outlines are wrong.

use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::outline::{
    DestView, Destination, MAX_OUTLINE_DEPTH, Outline, RemoteTarget, read_outline,
};

/// Load a fixture and read its outline.
///
/// Panics on a parse failure, which in a test is exactly right: a
/// fixture that no longer parses is a broken fixture, and the panic
/// names it.
fn outline_of(name: &str) -> Outline {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/outline/"
    );
    let bytes = std::fs::read(format!("{path}{name}"))
        .unwrap_or_else(|error| panic!("fixture {name} unreadable: {error}"));
    let doc = Document::from_bytes(bytes)
        .unwrap_or_else(|error| panic!("fixture {name} did not parse: {error}"));
    read_outline(&doc)
}

/// Every fixture, for the smoke test below.
const FIXTURES: &[&str] = &[
    "basic-tree.pdf",
    "lying-counts.pdf",
    "named-dests.pdf",
    "actions.pdf",
    "both-dest-and-a.pdf",
    "broken-dests.pdf",
    "cycle.pdf",
    "deep.pdf",
    "titles.pdf",
    "no-outline.pdf",
];

/// Would catch: any fixture failing to parse, and — more importantly —
/// the reader panicking, aborting, or hanging on one of them.
///
/// The malformed fixtures (`cycle.pdf`, `broken-dests.pdf`, `deep.pdf`)
/// are the point. `pdfcer-core` denies `unwrap`/`expect`/`panic`/
/// unchecked indexing crate-wide precisely because it parses untrusted
/// input, and an outline is unusually easy to weaponise: this test is
/// the one that fails (or never finishes) if that policy is violated
/// somewhere in the walk.
#[test]
fn every_fixture_reads_without_panicking_or_hanging() {
    for name in FIXTURES {
        let outline = outline_of(name);
        // Flatten too — it is a second traversal with its own stack
        // discipline, and a tree that reads fine can still blow up here.
        let flat = outline.flatten();
        assert_eq!(
            flat.len(),
            outline.diagnostics.items,
            "{name}: flatten() must visit every item the read counted"
        );
    }
}

/// Would catch: `/Count`'s sign being read backwards; the root's
/// `/Count` being read with the *item* rule (which the PDF_Spec RAG
/// calls "the single most likely defect in an outline reader"); and —
/// the reason this fixture is entirely well-formed — any diagnostic
/// that fires on a clean document.
///
/// That last one matters more than it looks. A diagnostic that is
/// noisy on good files gets ignored on bad ones, so
/// `OutlineDiagnostics::is_faithful` must be **true** here, over parsed
/// bytes, with every count in the file correct.
#[test]
fn a_well_formed_outline_reads_faithfully() {
    let outline = outline_of("basic-tree.pdf");
    assert_eq!(outline.items.len(), 2);
    assert_eq!(outline.diagnostics.items, 5);
    assert_eq!(outline.diagnostics.max_depth, 1);

    let chapter1 = &outline.items[0];
    assert_eq!(chapter1.title, "Chapter 1");
    assert_eq!(chapter1.declared_count, Some(2));
    assert!(chapter1.open, "/Count +2 is OPEN");
    assert_eq!(chapter1.children.len(), 2);
    assert_eq!(chapter1.level, 0);
    assert_eq!(chapter1.children[0].level, 1);

    let chapter2 = &outline.items[1];
    assert_eq!(chapter2.title, "Chapter 2");
    assert_eq!(chapter2.declared_count, Some(-1));
    assert!(!chapter2.open, "/Count -1 is CLOSED");
    assert_eq!(
        chapter2.children.len(),
        1,
        "closed still means it HAS a child"
    );

    // The ROOT's /Count counts a different quantity from an item's: all
    // visible items INCLUDING the top level. Both chapters, plus
    // Chapter 1's two children; Chapter 2 is closed so its child is not
    // visible. Five items exist; four are visible.
    assert_eq!(outline.visible_item_count(), 4);
    assert_eq!(outline.diagnostics.declared_root_count, Some(4));
    assert!(!outline.diagnostics.root_count_disagreement);
    assert_eq!(outline.diagnostics.count_disagreements, 0);

    assert!(
        outline.diagnostics.is_faithful(),
        "a well-formed outline must read as faithful: {:?}",
        outline.diagnostics
    );
}

/// Would catch: `/Count`'s **magnitude** being used as a child count,
/// which is the single easiest §12.3.3 mistake to make because the key
/// is named `Count` and sits beside `/First` and `/Last`.
///
/// Every count in this fixture has the right sign and the wrong
/// magnitude, so the two readings give different answers for every
/// item. It also pins the cross-check that reports the lie: structure
/// comes from the linked list, and the magnitude is used for nothing
/// except detecting that the file contradicts itself.
#[test]
fn lying_count_magnitudes_are_ignored_for_structure_and_reported() {
    let outline = outline_of("lying-counts.pdf");
    assert_eq!(outline.items.len(), 2);
    assert_eq!(outline.diagnostics.items, 5);

    let open = &outline.items[0];
    assert_eq!(open.declared_count, Some(9));
    assert!(open.open, "+9 is OPEN whatever the magnitude claims");
    assert_eq!(open.children.len(), 2, "two children, not nine");

    let shut = &outline.items[1];
    assert_eq!(shut.declared_count, Some(-7));
    assert!(!shut.open, "-7 is CLOSED whatever the magnitude claims");
    assert_eq!(shut.children.len(), 1, "one child, not seven");

    // Both items and the root disagree with what the tree actually is.
    assert_eq!(outline.diagnostics.count_disagreements, 2);
    assert_eq!(outline.diagnostics.declared_root_count, Some(99));
    assert!(outline.diagnostics.root_count_disagreement);
    assert!(!outline.diagnostics.is_faithful());

    // The true visible total: "Open" and its two children, plus "Shut"
    // itself. "Shut" is closed, so its own child is hidden — but "Shut"
    // is still visible, which is the half of the rule a reader that
    // "skips closed subtrees" gets wrong.
    assert_eq!(outline.visible_item_count(), 4);
}

/// Would catch: destination page references being mapped to object
/// *numbers* rather than 0-based page *indices*, and view parameters
/// being read at the wrong array offsets.
///
/// The fixture's pages are objects 3..7 while their indices are 0..4, so
/// the two are never equal and the confusion cannot pass.
#[test]
fn basic_tree_resolves_pages_and_views() {
    let outline = outline_of("basic-tree.pdf");
    let flat = outline.flatten();
    // (title, page index, view)
    let expected: &[(&str, usize, DestView)] = &[
        ("Chapter 1", 0, DestView::Fit),
        (
            "Section 1.1",
            0,
            DestView::Xyz {
                left: Some(72.0),
                top: Some(720.0),
                zoom: None,
            },
        ),
        ("Section 1.2", 1, DestView::FitH { top: Some(700.0) }),
        (
            "Chapter 2",
            2,
            // `/XYZ null null 2`: keep the current position, set zoom 2.
            DestView::Xyz {
                left: None,
                top: None,
                zoom: Some(2.0),
            },
        ),
        (
            "Section 2.1",
            3,
            DestView::FitR {
                left: Some(10.0),
                bottom: Some(20.0),
                right: Some(300.0),
                top: Some(400.0),
            },
        ),
    ];
    assert_eq!(flat.len(), expected.len());
    for (item, (title, page_index, view)) in flat.iter().zip(expected) {
        assert_eq!(&item.title, title);
        assert_eq!(item.page_index(), Some(*page_index), "for {title}");
        match &item.destination {
            Some(Destination::Page { view: got, .. }) => {
                assert_eq!(got, view, "for {title}");
            }
            other => panic!("{title}: expected a resolved page, got {other:?}"),
        }
    }

    // `/FitR`'s four numbers assemble in the order the array gave them.
    let section21 = flat[4];
    let Some(Destination::Page { view, .. }) = &section21.destination else {
        panic!("expected a resolved page");
    };
    assert_eq!(view.rect(), Some([10.0, 20.0, 300.0, 400.0]));

    // `/XYZ` with a null zoom asks the viewer to keep the current one.
    let Some(Destination::Page { view, .. }) = &flat[1].destination else {
        panic!("expected a resolved page");
    };
    assert!(view.zoom_is_retain());
}

/// Would catch: only one of §12.3.2.3's two named-destination
/// namespaces being searched; the name-tree walk stopping at the root
/// instead of descending through `/Kids`; the `<< /D … >>` wrapper not
/// being unwrapped; and an unresolvable name being dropped rather than
/// preserved.
///
/// All four are silent failures — the bookmark simply stops working —
/// which is why each gets its own row rather than a single "names
/// resolve" assertion.
#[test]
fn both_named_destination_namespaces_resolve() {
    let outline = outline_of("named-dests.pdf");
    assert_eq!(
        outline.diagnostics.named_destinations_defined, 4,
        "one legacy /Dests entry plus three name-tree entries"
    );

    let flat = outline.flatten();
    assert_eq!(flat.len(), 6);

    // 1. The PDF 1.1 catalog /Dests DICTIONARY, keyed by a name object.
    assert_eq!(flat[0].title, "Legacy intro");
    assert_eq!(flat[0].page_index(), Some(0));

    // 2. The PDF 1.2 /Names -> /Dests NAME TREE, reached through /Kids.
    assert_eq!(flat[1].title, "Tree body");
    assert_eq!(flat[1].page_index(), Some(1));

    // 3. The same tree, value wrapped as << /D [...] >> (§12.3.2.3).
    assert_eq!(flat[2].title, "Tree wrapped");
    assert_eq!(flat[2].page_index(), Some(2));
    match &flat[2].destination {
        Some(Destination::Page { view, .. }) => {
            assert_eq!(*view, DestView::FitV { left: Some(40.0) });
        }
        other => panic!("expected a resolved page, got {other:?}"),
    }

    // 4. NOTE 2's other wrapper form: the value carries a go-to ACTION
    //    instead of a /D.
    assert_eq!(flat[3].title, "Tree action");
    assert_eq!(
        flat[3].destination,
        Some(Destination::Page {
            page_index: 1,
            view: DestView::FitB,
        })
    );

    // 5. DEST-A1: a legacy-DICTIONARY key spelled as a STRING. It
    //    resolves — pdfcer searches both namespaces — and the leniency is
    //    disclosed rather than assumed.
    assert_eq!(flat[4].title, "Crossed namespace");
    assert_eq!(flat[4].page_index(), Some(0));
    assert_eq!(outline.diagnostics.cross_namespace_resolutions, 1);

    // 6. A name neither namespace defines: KEPT, not dropped.
    assert_eq!(flat[5].title, "Nowhere");
    assert_eq!(
        flat[5].destination,
        Some(Destination::Named {
            name: b"nowhere".to_vec()
        })
    );
    assert_eq!(outline.diagnostics.unresolved_names, 1);
    assert!(!outline.diagnostics.is_faithful());
}

/// Would catch: `/GoToR`'s remote page number being converted to a local
/// page index (which would scroll this document to a page the bookmark
/// never meant); a file-specification dictionary's `/UF` being ignored
/// in favour of the legacy `/F`; and a non-navigation action being
/// reported as a broken bookmark instead of a disclosed one.
#[test]
fn actions_are_classified_and_never_executed() {
    let outline = outline_of("actions.pdf");
    let flat = outline.flatten();
    assert_eq!(flat.len(), 5);

    // /GoTo — the only action that yields a local page index.
    assert_eq!(flat[0].title, "GoTo local");
    assert_eq!(flat[0].page_index(), Some(1));

    // /GoToR by integer page number, carried verbatim.
    assert_eq!(flat[1].title, "GoToR by number");
    match &flat[1].destination {
        Some(Destination::Remote {
            file,
            target,
            view,
            new_window,
        }) => {
            assert_eq!(file.as_deref(), Some(b"other.pdf".as_slice()));
            assert_eq!(*target, RemoteTarget::PageNumber(7));
            assert_eq!(*view, DestView::Fit);
            assert_eq!(*new_window, Some(true));
        }
        other => panic!("expected Remote, got {other:?}"),
    }
    assert_eq!(
        flat[1].page_index(),
        None,
        "a remote page number is NOT a local page index"
    );

    // /GoToR by name, with a file-specification DICTIONARY: /UF wins.
    assert_eq!(flat[2].title, "GoToR by name");
    match &flat[2].destination {
        Some(Destination::Remote {
            file,
            target,
            new_window,
            ..
        }) => {
            assert_eq!(
                file.as_deref(),
                Some(b"unicode.pdf".as_slice()),
                "/UF is the Unicode form and must be preferred over /F"
            );
            assert_eq!(*target, RemoteTarget::Named(b"remote-name".to_vec()));
            assert_eq!(*new_window, None, "absent is not the same as false");
        }
        other => panic!("expected Remote, got {other:?}"),
    }

    // Non-navigation actions: named, not executed, not "broken".
    for (index, subtype) in [(3usize, "URI"), (4, "JavaScript")] {
        match &flat[index].destination {
            Some(Destination::NonNavigation { action: Some(name) }) => {
                assert_eq!(name.as_bytes(), subtype.as_bytes());
            }
            other => panic!("expected NonNavigation /{subtype}, got {other:?}"),
        }
        assert_eq!(flat[index].page_index(), None);
    }
    assert_eq!(outline.diagnostics.unreadable_actions, 0);
}

/// Would catch: the `/Dest`-over-`/A` precedence drifting away from
/// `pageops::references::resolve_target`.
///
/// The two keys point at *different* pages in this fixture, so the
/// winner is provable rather than inferred. If these ever disagree, the
/// bookmarks panel will tell an operator a bookmark is fine while the
/// page-delete census reports it as about to break — or the reverse.
#[test]
fn dest_beats_a_when_a_malformed_file_carries_both() {
    let outline = outline_of("both-dest-and-a.pdf");
    assert_eq!(outline.items.len(), 1);
    assert_eq!(
        outline.items[0].page_index(),
        Some(0),
        "/Dest names page 0; the /A action names page 1"
    );
    assert_eq!(outline.diagnostics.dest_and_action_both_present, 1);
    assert!(!outline.diagnostics.is_faithful());
}

/// Would catch: a bookmark whose destination cannot reach a page being
/// silently dropped from the tree — the failure mode that turns a
/// repairable corruption into an invisible one. Brief requirement (1).
///
/// Three different corruptions, all of which must keep their bookmark:
/// a reference to an object that does not exist, a reference to an
/// object that exists but is not a page, and an empty array.
#[test]
fn destinations_that_reach_no_page_keep_their_bookmarks() {
    let outline = outline_of("broken-dests.pdf");
    assert_eq!(outline.items.len(), 3, "no bookmark may vanish");
    assert_eq!(outline.diagnostics.unmapped_pages, 3);
    assert!(
        outline.diagnostics.page_tree_error.is_none(),
        "the page tree is fine here; the destinations are not"
    );

    let titles: Vec<&str> = outline.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, vec!["Dangling page", "Not a page", "Empty array"]);

    for item in &outline.items {
        match &item.destination {
            Some(Destination::UnmappedPage { .. }) => {}
            other => panic!("{}: expected UnmappedPage, got {other:?}", item.title),
        }
        assert_eq!(item.page_index(), None);
    }

    // The object that DOES exist but is not a page is named, so a repair
    // flow has something to work with. (Object 1 is the catalog — a real
    // object, reachable, and emphatically not a page.)
    assert_eq!(
        outline.items[1].destination,
        Some(Destination::UnmappedPage {
            page: Some(ObjId::new(1, 0)),
            view: DestView::Fit,
        })
    );

    // The empty array has no fit style either, and says so.
    match &outline.items[2].destination {
        Some(Destination::UnmappedPage { page, view }) => {
            assert_eq!(*page, None);
            assert_eq!(*view, DestView::Absent);
        }
        other => panic!("expected UnmappedPage, got {other:?}"),
    }
    assert_eq!(outline.diagnostics.malformed_views, 1);
}

/// Would catch: the cycle guard being absent. **This test does not fail
/// on a broken reader — it hangs**, which is precisely the failure the
/// guard exists to prevent and precisely why it is worth a whole
/// fixture. Brief requirement (4).
///
/// Also catches a guard that breaks the loop without recording it: a
/// truncated tree presented as a complete one is `CLAUDE.md` rule 4's
/// sneaky case.
#[test]
fn a_file_full_of_outline_cycles_terminates_and_reports() {
    let outline = outline_of("cycle.pdf");

    // Two top-level items ("Ping", "Pong"); Ping has two children.
    assert_eq!(outline.items.len(), 2);
    let titles: Vec<&str> = outline.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, vec!["Ping", "Pong"]);
    assert_eq!(outline.items[0].children.len(), 2);
    assert_eq!(outline.diagnostics.items, 4);

    // Three loops, three refusals:
    //   /First 8 -> 8 (self-parent)
    //   /First 6 from item 9 (back to an ancestor)
    //   /Next 6 from item 7 (sibling loop)
    assert_eq!(outline.diagnostics.cycles_broken, 3);
    assert!(!outline.diagnostics.is_faithful());

    // No item appears twice — the guard is about identity, not just
    // about terminating.
    let flat = outline.flatten();
    let mut ids: Vec<_> = flat.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "an item was read into the tree twice");
}

/// Would catch: unbounded recursion on a deeply nested outline (a stack
/// overflow, which no `#[deny(panic)]` intercepts), the depth cap being
/// applied off by one, or a truncated subtree not being reported.
#[test]
fn nesting_past_the_cap_truncates_and_says_so() {
    let outline = outline_of("deep.pdf");

    // Exactly MAX_OUTLINE_DEPTH levels survive, in one chain.
    assert_eq!(outline.items.len(), 1);
    assert_eq!(outline.diagnostics.items, MAX_OUTLINE_DEPTH);
    assert_eq!(outline.diagnostics.max_depth, MAX_OUTLINE_DEPTH - 1);
    assert_eq!(outline.diagnostics.depth_truncations, 1);
    assert!(!outline.diagnostics.is_faithful());

    // The deepest surviving item keeps its own data and loses only its
    // subtree — truncation must not discard what was already read.
    let flat = outline.flatten();
    let deepest = flat.last().expect("at least one item");
    assert_eq!(deepest.level, MAX_OUTLINE_DEPTH - 1);
    assert_eq!(deepest.title, format!("Level {}", MAX_OUTLINE_DEPTH - 1));
    assert_eq!(deepest.page_index(), Some(0));
    assert!(deepest.children.is_empty());
}

/// Would catch: `/Title` being treated as raw bytes rather than a §7.9.2
/// text string.
///
/// Two of these are silent-wrongness cases. A UTF-16BE title read as
/// bytes renders as mojibake, which at least looks wrong; `0xA0` read as
/// Latin-1 renders as a plausible no-break space when PDFDocEncoding
/// says EURO, and nothing about the output announces the substitution.
/// The undefined `0xAD` pins the disclosure obligation from
/// `CLAUDE.md` rule 4: an inexact decode must be visible.
#[test]
fn titles_decode_as_pdf_text_strings() {
    let outline = outline_of("titles.pdf");
    let flat = outline.flatten();
    assert_eq!(flat.len(), 4);

    assert_eq!(flat[0].title, "Plain ASCII");
    assert!(flat[0].title_exact);

    // FE FF BOM => UTF-16BE. Greek kappa-epsilon-phi.
    assert_eq!(flat[1].title, "\u{3ba}\u{3b5}\u{3c6}");
    assert!(flat[1].title_exact);

    // 0xA0 is EURO in PDFDocEncoding (Annex D.3), NOT a no-break space.
    assert_eq!(flat[2].title, "\u{20AC}5 fee");
    assert!(flat[2].title_exact);

    // 0xAD is one of the 24 undefined codes: substituted AND disclosed.
    assert_eq!(flat[3].title, "bad\u{FFFD}byte");
    assert!(!flat[3].title_exact);
    assert_eq!(outline.diagnostics.titles_inexact, 1);
    assert!(!outline.diagnostics.is_faithful());
}

/// Would catch: a document with no `/Outlines` being reported as an
/// error, or as a document whose outline could not be read — the two
/// are very different things to tell an operator, and the overwhelming
/// majority of real PDFs are in the first category.
#[test]
fn a_document_with_no_outline_is_empty_and_faithful() {
    let outline = outline_of("no-outline.pdf");
    assert!(outline.items.is_empty());
    assert_eq!(outline.diagnostics.items, 0);
    assert_eq!(outline.diagnostics.named_destinations_defined, 0);
    assert!(outline.diagnostics.page_tree_error.is_none());
    assert!(
        outline.diagnostics.is_faithful(),
        "having no bookmarks is not a defect"
    );
}
