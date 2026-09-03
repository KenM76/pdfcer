//! **`delete_field_group` computed the same quantity twice and the two answers
//! disagreed** — and three of the four ways they disagreed are visible in the
//! build operators run (`Pass 190.0`).
//!
//! ## Where this came from, and why the stated severity was wrong
//!
//! `group_deletion_preflight` predicts how many grouping nodes a removal will
//! take by walking `form.groups` **by NAME**. `remove_fields_from_form`
//! performs it by walking `/Parent` **by OBJECT**. A `debug_assert_eq!`
//! compares them, and `fuzz/fuzz_targets/form_edit_sequence.rs` tripped it on
//! 2026-08-30.
//!
//! It was sized then as *"a `debug_assert`, so in release it is a wrong
//! `nodes_removed` in a disclosure — not corruption"*, and carried at that
//! priority for a day. **That sizing was right about the assertion and wrong
//! about the defect.** Building the four shapes the two derivations disagree
//! on showed what else lives underneath:
//!
//! | shape | what the assertion said | what RELEASE does |
//! |---|---|---|
//! | `/T`-less terminal | `0` vs `1` | **returns `Ok`, deletes nothing** |
//! | duplicate `/T` | `2` vs `1` | a wrong count (the stated severity) |
//! | `/T`-less intermediate | `2` vs `1` | a wrong count |
//! | terminal with no `/Parent` | `0` vs `1` | **writes a dangling `/Kids`** |
//!
//! ★ The lesson worth carrying is not about forms. **A `debug_assert` that
//! fires is evidence that two derivations disagree; it is not evidence about
//! what the disagreement COSTS.** Sizing the defect from the assertion's
//! compile-time behaviour answered a question about the guard, not about the
//! bug it was guarding.
//!
//! ## The two root causes, which are different
//!
//! **1. A name is not an identity.** Two grouping nodes share one fully
//! qualified name whenever any `/T`-less node sits in the chain — §12.7.3.2
//! says such a node contributes no segment, so it silently *aliases* its
//! parent's name — or whenever a producer writes a duplicate `/T`, which
//! nothing forbids. A `Vec<String>` cannot represent two nodes bearing one
//! name; a `BTreeSet<ObjId>` can. The prediction used the first.
//!
//! **2. `/Parent` is a back-link, not the structure.** `/Kids` is the tree.
//! Deriving a node's container from `/Parent` means a field that is reachable
//! downward and invisible upward gets deleted while the array naming it is
//! never patched.
//!
//! ## What these tests assert on
//!
//! **The saved bytes, and the struct's internal consistency** — not the
//! `debug_assert`, which is exactly the thing that was already known to fire
//! and exactly the thing a release build cannot see. `R159`: a defect that
//! lives in the bytes is asserted in the bytes.
//!
//! Fixtures: `tools/gen-form-group-deletion-fixtures.py`, wholly synthetic
//! (`LEGAL.md` §5 category (a)). Each is the smallest legal document that
//! exhibits its shape.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::writer::SaveOptions;
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(name: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(&format!("forms/{name}"))).expect("load fixture"))
}

/// Every `/Kids` entry in the document that names an object which is **not in
/// the file**.
///
/// This is the assertion that catches the dangling-reference shape, and it is
/// deliberately structural rather than a count: shape 4's node count is `0`
/// both before and after the fix, so a count assertion passes on the broken
/// engine at `0 == 0` while the file it wrote is damaged.
fn dangling_kids(s: &EditSession) -> Vec<(ObjId, ObjId)> {
    let g = s.graph();
    let mut out = Vec::new();
    // Walk DOWN from `/AcroForm` `/Fields`, which is the structure -- the same
    // direction the fix teaches the cascade to use, and the only direction in
    // which a node with no `/Parent` is visible at all.
    let mut stack: Vec<ObjId> = Vec::new();
    if let Some(Object::Dict(acro)) = g
        .trailer_entry(b"Root")
        .map(|r| g.resolve(r).clone())
        .and_then(|c| c.as_dict().and_then(|d| d.get(b"AcroForm")).cloned())
        .map(|a| g.resolve(&a).clone())
        && let Some(Object::Array(roots)) = acro.get(b"Fields").map(|o| g.resolve(o).clone())
    {
        stack.extend(roots.iter().filter_map(Object::as_reference));
    }
    let mut seen: std::collections::BTreeSet<ObjId> = stack.iter().copied().collect();
    while let Some(id) = stack.pop() {
        let Some(Object::Dict(d)) = g.value(id) else {
            continue;
        };
        let Some(Object::Array(items)) = d.get(b"Kids").map(|o| g.resolve(o).clone()) else {
            continue;
        };
        for item in &items {
            let Some(child) = item.as_reference() else {
                continue;
            };
            if g.value(child).is_none() {
                out.push((id, child));
            } else if seen.insert(child) {
                stack.push(child);
            }
        }
    }
    out
}

/// The same question asked of the SAVED file, which is the one that ships.
fn dangling_kids_after_save(s: &mut EditSession) -> Vec<(ObjId, ObjId)> {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("the saved file must reload");
    let reloaded = EditSession::new(doc);
    dangling_kids(&reloaded)
}

fn group_names(s: &EditSession) -> Vec<String> {
    forms::parse_acroform(&s.graph())
        .map(|f| {
            f.groups
                .iter()
                .map(|g| g.fully_qualified_name.clone())
                .collect()
        })
        .unwrap_or_default()
}

// -------------------------------------------------------------------------
// The invariant, on every shape
// -------------------------------------------------------------------------

/// ★ The outcome must be internally consistent — `nodes.len()` and
/// `nodes_removed` are two names for one quantity.
///
/// They were spliced from two different derivations by a `..preview` struct
/// update, and **the two shells read different ones**: `pdfcer` prints
/// `nodes_removed`, `pdfcer-gui` prints `nodes.len() - 1`. So the same deletion
/// on the same document was reported differently depending on which program
/// the operator ran. That is not a disclosure defect in one shell; it is one
/// struct claiming two things.
#[test]
fn the_outcome_reports_one_number_not_two() {
    for (name, group) in [
        ("group-delete-t-less-child.pdf", "A"),
        ("group-delete-duplicate-t.pdf", "A"),
        ("group-delete-t-less-intermediate.pdf", "A"),
        ("group-delete-orphan-no-parent.pdf", "A"),
    ] {
        let mut s = session(name);
        let Ok(out) = s.delete_field_group(group) else {
            // A refusal is a legitimate answer for a shape pdfcer declines to
            // touch; what must not happen is an inconsistent success.
            continue;
        };
        assert_eq!(
            out.nodes.len(),
            out.nodes_removed,
            "{name}: the outcome claims {} nodes in its list and {} in its count",
            out.nodes.len(),
            out.nodes_removed
        );
    }
}

/// The preview must predict what the deletion does. This is the
/// `debug_assert`'s own claim, promoted to a hard assertion on inputs that are
/// known rather than fuzzer-supplied — so it holds in a release test run too.
#[test]
fn the_dry_run_and_the_real_run_agree() {
    for (name, group) in [
        ("group-delete-t-less-child.pdf", "A"),
        ("group-delete-duplicate-t.pdf", "A"),
        ("group-delete-t-less-intermediate.pdf", "A"),
        ("group-delete-orphan-no-parent.pdf", "A"),
    ] {
        let mut s = session(name);
        let preview = s.field_group_deletion_preview(group);
        let real = s.delete_field_group(group);
        match (preview, real) {
            (Ok(p), Ok(r)) => {
                assert_eq!(
                    p.nodes_removed, r.nodes_removed,
                    "{name}: --dry-run says {} nodes, the real run takes {}. This verb is \
                     DESTRUCTIVE; a dry run that disagrees with it is worse than no dry run.",
                    p.nodes_removed, r.nodes_removed
                );
            }
            (Err(_), Err(_)) => {}
            (p, r) => panic!(
                "{name}: the dry run and the real run disagree about whether this is even \
                 possible: {:?} vs {:?}",
                p.map(|x| x.nodes_removed),
                r.map(|x| x.nodes_removed)
            ),
        }
    }
}

// -------------------------------------------------------------------------
// Shape by shape, on what RELEASE does
// -------------------------------------------------------------------------

/// ★★★ The dangling reference. This is the one that damages the file.
///
/// `A` -> `X`, where `X` has no `/Parent`. The cascade derives every node's
/// container from `/Parent`, so `X` is deleted and `A`'s `/Kids` — which still
/// names it — is never patched.
///
/// Asserted **structurally on the saved file**, not on a count: the node count
/// is `0` both before and after the fix, so a count assertion passes on the
/// broken engine at `0 == 0` while the bytes it wrote are damaged.
#[test]
fn deleting_a_parentless_field_does_not_leave_a_dangling_kids_entry() {
    let mut s = session("group-delete-orphan-no-parent.pdf");
    assert!(dangling_kids(&s).is_empty(), "the fixture starts clean");

    // Whether this succeeds or refuses is a separate question — what it must
    // never do is succeed and leave the file naming an object that is gone.
    let _ = s.delete_field_group("A");

    assert!(
        dangling_kids(&s).is_empty(),
        "the session holds a /Kids entry naming a deleted object: {:?}",
        dangling_kids(&s)
    );
    let after = dangling_kids_after_save(&mut s);
    assert!(
        after.is_empty(),
        "the SAVED FILE holds a /Kids entry naming a deleted object: {after:?}"
    );
}

/// The same hole through the single-field verb, which shares the cascade.
#[test]
fn deleting_a_parentless_field_by_name_does_not_dangle_either() {
    let mut s = session("group-delete-orphan-no-parent.pdf");
    let _ = s.delete_field("A.X");
    let after = dangling_kids_after_save(&mut s);
    assert!(
        after.is_empty(),
        "delete_field shares remove_fields_from_form and shares this hole: {after:?}"
    );
}

/// ★★ A success that changes nothing is worse than a refusal.
///
/// `A`'s only terminal has no `/T`, so its fully qualified name *equals* `A`.
/// `descendants_of` is a prefix match on `"A."`, which that name never
/// matches, so the subtree is invisible and the deletion selects nothing.
/// Before the fix this returned `Ok` with the document untouched.
#[test]
fn deleting_a_group_whose_child_has_no_partial_name_is_not_a_silent_no_op() {
    let mut s = session("group-delete-t-less-child.pdf");
    let before = group_names(&s);
    assert_eq!(before, vec!["A".to_owned()]);

    match s.delete_field_group("A") {
        Ok(out) => {
            assert!(
                out.nodes_removed > 0,
                "returned Ok claiming {} nodes removed",
                out.nodes_removed
            );
            assert_ne!(
                group_names(&s),
                before,
                "returned Ok and the document is unchanged — the operator asked to delete a \
                 subtree and was told it worked"
            );
        }
        // A named refusal is acceptable: it tells the operator nothing
        // happened, which is the honest half of what was missing.
        Err(_) => {
            assert_eq!(group_names(&s), before, "a refusal must change nothing");
        }
    }
}

/// Two roots sharing one `/T`. The cascade takes both; a name-keyed list can
/// only hold one.
#[test]
fn two_nodes_sharing_a_name_are_counted_as_two() {
    let mut s = session("group-delete-duplicate-t.pdf");
    assert_eq!(group_names(&s).len(), 2, "two nodes, one name");

    let out = s
        .delete_field_group("A")
        .expect("both roots are named A and both are deletable");
    assert_eq!(
        out.nodes_removed, 2,
        "two grouping-node objects were removed; a list keyed by name said 1"
    );
    assert_eq!(out.nodes.len(), 2);
    assert!(
        group_names(&s).is_empty(),
        "both nodes must be gone, not one"
    );
}

/// The legal, ordinary way two nodes come to share a name: a `/T`-less node
/// between two named ones aliases its parent's.
#[test]
fn a_t_less_intermediate_node_is_counted_as_its_own_node() {
    let mut s = session("group-delete-t-less-intermediate.pdf");
    let out = s
        .delete_field_group("A")
        .expect("A is a resolvable grouping node");
    assert_eq!(
        out.nodes_removed, 2,
        "the named node and the /T-less one beneath it are two objects"
    );
    assert!(group_names(&s).is_empty());
    assert!(dangling_kids_after_save(&mut s).is_empty());
}
