//! Integration tests for the optional-content (layer / OCG) reader
//! ([`pdfcer_core::layers`], ISO 32000-1:2008 §8.11).
//!
//! ## Why these exist alongside the module's own unit tests
//!
//! `src/layers.rs`'s unit tests drive a hand-built `ObjectGraph`. That is
//! the right tool for pinning traversal *logic* — it can express an
//! `/Order` shape no file format lets you write down twice — but it
//! bypasses the lexer, the object parser and the cross-reference table
//! entirely, and it cannot exercise the page sweep at all, because a
//! synthesised graph has no page tree.
//!
//! These tests therefore run the same claims through **whole files**,
//! parsed from bytes by [`Document::from_bytes`]. A reader that is correct
//! over a synthesised graph and wrong over parsed bytes is a reader that
//! fails on every real document while its test suite stays green.
//!
//! Every fixture is wholly synthetic and byte-authored by
//! `tools/gen-layer-fixtures.py` (`docs/LEGAL.md` §5 category (a)); see
//! `fixtures/synthetic/layers/PROVENANCE.md`.
//!
//! Each test's doc comment says what defect it would catch. That is the
//! project's standing bar for a test: a failure should name the mistake,
//! not merely report that layers are wrong.

use pdfcer_core::document::Document;
use pdfcer_core::layers::{
    LayerScan, LayerSource, Layers, list_layers, read_layers, read_layers_with,
};
use pdfcer_core::object::ObjId;

/// Load a fixture and read its optional content.
///
/// Panics on a parse failure, which in a test is exactly right: a fixture
/// that no longer parses is a broken fixture, and the panic names it.
fn layers_of(name: &str) -> Layers {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/layers/"
    );
    let bytes = std::fs::read(format!("{path}{name}"))
        .unwrap_or_else(|error| panic!("fixture {name} unreadable: {error}"));
    let doc = Document::from_bytes(bytes)
        .unwrap_or_else(|error| panic!("fixture {name} did not parse: {error}"));
    read_layers(&doc)
}

/// Every fixture, for the smoke test below.
const FIXTURES: &[&str] = &[
    "basic-layers.pdf",
    "nested-order.pdf",
    "unregistered-ocg.pdf",
    "radio-locked.pdf",
    "basestate-off.pdf",
    "order-cycle.pdf",
    "ocmd-membership.pdf",
    "malformed-groups.pdf",
    "no-layers.pdf",
];

/// The names of a listing, in listed order — the thing almost every
/// assertion below is really about.
fn names(layers: &Layers) -> Vec<&str> {
    layers.layers.iter().map(|l| l.name.as_str()).collect()
}

/// Every fixture parses, reads, and returns without hanging or panicking.
///
/// **Catches:** the whole class of "a hostile structure took the reader
/// out". Three of these files exist specifically to be hostile — a
/// self-referential `/Order`, a forty-deep nesting chain, a registry full
/// of things that are not groups — and the *first* property any of them
/// must prove is that the call comes back at all. A hang is not a failing
/// test, it is a test run that never ends, so this runs before anything
/// makes a claim about content.
#[test]
fn every_fixture_reads_without_incident() {
    for name in FIXTURES {
        let layers = layers_of(name);
        // Diagnostics are always populated, never left at a default that
        // happens to look clean.
        let _ = layers.diagnostics.is_faithful();
    }
}

/// The happy path: names decode, `/OFF` decides visibility, `/Order`
/// decides sequence, and a registered-but-unordered group is still listed.
///
/// **Catches:** four separate defects at once, which is only acceptable
/// because each has its own assertion. (1) A reader that ignores `/OFF`
/// reports everything visible. (2) One that iterates `/OCGs` instead of
/// `/Order` cannot tell the two apart here — but the fourth group is in
/// the registry and not in `/Order`, so its position and its `in_order`
/// flag both move if the reader confuses them. (3) A UTF-16BE `/Name`
/// read as raw bytes produces mojibake. (4) A PDFDocEncoding `/Name`
/// read as Latin-1 produces a plausible, wrong, silent answer — byte 0xA0
/// is EURO in Annex D.3 and NO-BREAK SPACE in Latin-1.
#[test]
fn basic_layers_decodes_names_order_and_default_visibility() {
    let layers = layers_of("basic-layers.pdf");
    assert_eq!(
        names(&layers),
        [
            "Dimensions",
            "Hidden Notes",
            "\u{3ba}\u{3b5}\u{3c6}",
            "\u{20ac}5 tier"
        ]
    );
    assert!(layers.layers[0].visible_by_default);
    assert!(!layers.layers[1].visible_by_default, "group 5 is in /OFF");
    assert!(layers.layers[2].visible_by_default);

    // The fourth is registered but absent from /Order.
    assert!(layers.layers[3].in_default_config);
    assert!(!layers.layers[3].in_order);
    assert_eq!(layers.layers[3].discovered_via, LayerSource::Registry);
    for layer in &layers.layers[..3] {
        assert!(layer.in_order && layer.in_default_config);
        assert_eq!(layer.discovered_via, LayerSource::Order);
    }

    assert_eq!(layers.config_name.as_deref(), Some("Default"));
    assert_eq!(layers.list_mode.as_deref(), Some("AllPages"));
    assert!(
        layers
            .layers
            .iter()
            .all(|l| l.name_declared && l.name_exact)
    );
    assert!(
        layers
            .layers
            .iter()
            .all(|l| l.type_declared && l.intent_view)
    );
    assert!(
        layers.diagnostics.is_faithful(),
        "the happy-path fixture must produce no diagnostics: {:?}",
        layers.diagnostics
    );
}

/// `/Order`'s nesting survives a whole-file round trip, and the flat
/// listing is its pre-order — not the registry's order and not the
/// alphabet's.
///
/// **Catches:** a reader that flattens the tree, sorts by name, or
/// attaches a nested array to the wrong parent. The fixture's names run
/// Z, Y, X, W, V *in declared order*, so registry order, alphabetical
/// order and declared order are three different answers and exactly one
/// of them passes. A reader that got the parenting wrong still produces
/// the right flat list, which is why the tree is asserted separately.
#[test]
fn nested_order_survives_parsing_and_is_not_sorted() {
    let layers = layers_of("nested-order.pdf");
    assert_eq!(
        names(&layers),
        ["ZULU", "YANKEE", "XRAY", "WHISKEY", "VICTOR"]
    );

    // /Order [(Sheet metal) [4 0 R [5 0 R 6 0 R]] 7 0 R [8 0 R]]
    assert_eq!(layers.order.len(), 2);
    let folder = &layers.order[0];
    assert_eq!(folder.label.as_deref(), Some("Sheet metal"));
    assert!(
        folder.group.is_none(),
        "a label is non-selectable and carries no group"
    );
    assert_eq!(folder.children.len(), 1, "ZULU is the folder's only child");
    let zulu = &folder.children[0];
    assert_eq!(
        zulu.children.len(),
        2,
        "YANKEE and XRAY are ZULU's sublayers"
    );

    let whiskey = &layers.order[1];
    assert!(whiskey.group.is_some());
    assert_eq!(whiskey.children.len(), 1, "the trailing [8 0 R] is VICTOR");
    assert!(layers.diagnostics.is_faithful());
}

/// Seven groups reachable, one registered — and all seven are listed,
/// each naming the route that found it.
///
/// **Catches:** the central failure this module exists to prevent — a
/// panel that intersects what it finds with `/OCProperties /OCGs` and so
/// silently omits six layers whose content is on screen. Also catches a
/// resource walk that does not recurse: object 10 is reachable *only*
/// through a form XObject nested inside another form XObject's
/// `/Resources`, so a one-level scan loses exactly that group and nothing
/// else, which is the kind of near-miss a coarser fixture would hide.
#[test]
fn unregistered_groups_are_listed_with_their_discovery_route() {
    let layers = layers_of("unregistered-ocg.pdf");
    assert_eq!(
        names(&layers),
        [
            "Registered",
            "Order only",
            "Off only",
            "Annotation only",
            "Marked content only",
            "XObject only",
            "Nested XObject only",
        ]
    );
    let sources: Vec<LayerSource> = layers.layers.iter().map(|l| l.discovered_via).collect();
    assert_eq!(
        sources,
        [
            LayerSource::Order,
            LayerSource::Order,
            LayerSource::DefaultConfig,
            LayerSource::Annotation,
            LayerSource::MarkedContent,
            LayerSource::XObject,
            LayerSource::XObject,
        ]
    );

    assert!(layers.layers[0].in_default_config);
    assert!(
        layers.layers[1..].iter().all(|l| !l.in_default_config),
        "only the first group is in /OCProperties /OCGs"
    );
    assert_eq!(layers.diagnostics.unregistered_groups, 6);
    assert!(
        !layers.layers[2].visible_by_default,
        "/OFF applies to an unregistered group too"
    );
    assert!(!layers.diagnostics.is_faithful());
    assert!(!layers.diagnostics.page_scan_failed);
}

/// Dropping the page sweep drops exactly the four content-only groups,
/// and nothing else.
///
/// **Catches:** a [`LayerScan::CatalogOnly`] that secretly still walks
/// pages (the parameter would be a lie, and a caller choosing it for cost
/// reasons would not get the saving), and a `CatalogAndPages` that
/// secretly does not (the default would silently under-report). Asserting
/// the *difference* rather than each listing separately is what makes the
/// test about the parameter.
#[test]
fn catalog_only_scan_drops_exactly_the_content_reached_groups() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/layers/unregistered-ocg.pdf"
    );
    let bytes = std::fs::read(path).expect("fixture unreadable");
    let doc = Document::from_bytes(bytes).expect("fixture did not parse");

    let catalog = read_layers_with(&doc, LayerScan::CatalogOnly);
    let both = read_layers_with(&doc, LayerScan::CatalogAndPages);
    assert_eq!(names(&catalog), ["Registered", "Order only", "Off only"]);
    assert_eq!(both.layers.len(), 7);
    assert_eq!(
        both.layers[..3],
        catalog.layers[..],
        "the catalog-derived rows must be identical, not merely similar"
    );
}

/// Radio membership and locks reach the caller from a parsed file,
/// before any toggle happens.
///
/// **Catches:** a panel that renders mutually exclusive layers as
/// independent checkboxes. The constraint is knowable from the file, and
/// a caller that has to discover it by toggling has already changed two
/// layers — which `CLAUDE.md` rule 4 forbids. Also pins that an
/// overlapping member reports the *first* array (a plain map insert makes
/// the last one win, which is a silent off-by-one in a field a UI renders
/// directly) and that the overlap is disclosed rather than resolved.
#[test]
fn radio_groups_and_locks_reach_the_caller_from_bytes() {
    let layers = layers_of("radio-locked.pdf");
    let ids: Vec<ObjId> = layers.layers.iter().map(|l| l.id).collect();
    assert_eq!(
        layers.radio_groups,
        vec![vec![ids[0], ids[1]], vec![ids[1], ids[2]]]
    );
    assert_eq!(layers.layers[0].radio_group, Some(0));
    assert_eq!(layers.layers[1].radio_group, Some(0), "first array wins");
    assert_eq!(layers.layers[2].radio_group, Some(1));
    assert_eq!(layers.layers[3].radio_group, None);

    assert!(layers.layers[0].locked, "/Locked names the first group");
    assert!(layers.layers[1..].iter().all(|l| !l.locked));
    assert_eq!(layers.diagnostics.overlapping_radio_groups, 1);
    assert!(!layers.diagnostics.is_faithful());
}

/// `/BaseState /OFF` is followed, `/ON` overrides it, and the departure
/// from Table 101's `shall` is disclosed rather than corrected.
///
/// **Catches:** a reader that "helpfully" honours Table 101's "in the
/// default configuration dictionary, the value of this entry shall be ON"
/// by ignoring what the file says. That reports the exact inverse for
/// every group `/ON` does not mention — a drawing the author shipped with
/// one layer lit would open with everything lit. Also catches a reader
/// that follows the file but says nothing about it, leaving a caller
/// unable to tell a deliberate authoring choice from a producer bug.
#[test]
fn base_state_off_is_followed_and_disclosed_from_bytes() {
    let layers = layers_of("basestate-off.pdf");
    assert_eq!(
        names(&layers),
        ["Off by base state", "On by override", "Off twice over"]
    );
    assert!(!layers.layers[0].visible_by_default);
    assert!(
        layers.layers[1].visible_by_default,
        "/ON overrides BaseState"
    );
    assert!(!layers.layers[2].visible_by_default);
    assert!(layers.diagnostics.base_state_off_in_default);
    assert!(
        !layers.diagnostics.base_state_off_with_unregistered,
        "every group here is registered, so the known caveat does not apply"
    );
    assert!(!layers.diagnostics.is_faithful());
}

/// A self-referential `/Order`, a mutual loop, and a forty-deep chain all
/// terminate — and the groups reachable before them survive.
///
/// **Catches:** a missing cycle guard or a missing depth cap. Neither
/// produces a wrong answer: one hangs and one overflows the stack, and
/// `pdfcer-core`'s panic-free policy (`lib.rs`) treats a stack overflow on
/// untrusted input exactly as it treats an `unwrap`. The assertion that
/// the pre-loop group is still listed is what stops a "fix" that bails out
/// of the whole `/Order` on the first cycle.
#[test]
fn order_cycles_and_depth_terminate_without_losing_reachable_groups() {
    let layers = layers_of("order-cycle.pdf");
    assert_eq!(
        layers.diagnostics.order_cycles, 2,
        "self-loop and mutual loop"
    );
    assert_eq!(layers.diagnostics.order_depth_truncations, 1);

    // All three groups are still reported: two through /Order, the third
    // through the registry once the deep chain was cut.
    assert_eq!(
        names(&layers),
        [
            "Reachable before the loop",
            "Behind the mutual loop",
            "At the bottom of the deep chain",
        ]
    );
    assert_eq!(layers.layers[2].discovered_via, LayerSource::Registry);
    assert!(!layers.layers[2].in_order);
    assert!(!layers.diagnostics.is_faithful());
}

/// An annotation `/OC` pointing at an OCMD contributes the OCMD's
/// *members*, not the OCMD itself.
///
/// **Catches:** a reader that lists the membership dictionary as a layer.
/// An OCMD has no `/Name` and no state of its own — a row for it would be
/// blank and untoggleable. Also pins Table 99's "dictionary **or** array"
/// for `/OCGs`: object 12's `/OCGs` is a single dictionary, and a reader
/// that assumes an array reads it as no members and silently loses a
/// layer that is genuinely there.
#[test]
fn ocmd_members_become_layers_but_the_ocmd_does_not() {
    let layers = layers_of("ocmd-membership.pdf");
    assert_eq!(
        names(&layers),
        ["Registered A", "Registered B", "Unregistered OCMD member"]
    );
    assert_eq!(layers.layers[2].discovered_via, LayerSource::Annotation);
    assert!(!layers.layers[2].in_default_config);
    // "Registered B" is reached only through the single-dictionary /OCGs
    // form as far as the sweep is concerned, but it is in /Order too, so
    // its presence proves the registry path rather than Table 99's. The
    // real proof of the single-dictionary form is that the OCMD with the
    // empty /OCGs contributed nothing and the listing is still exactly 3.
    assert_eq!(layers.layers.len(), 3);
    assert_eq!(layers.diagnostics.unregistered_groups, 1);
}

/// Registry entries that are not usable groups are counted, not listed;
/// a group with no `/Name` keeps an empty one.
///
/// **Catches:** a reader that invents a name ("Layer 4", "Untitled") for a
/// group missing Table 98's Required `/Name`. `CLAUDE.md` rule 4: a
/// placeholder that looks like a name is a claim about the document. Also
/// catches a reader that lists a *direct* dictionary as a row — it has no
/// object identity, so the row's `id` would be fabricated and nothing
/// could ever toggle it — and one that reports a phantom row for the
/// dangling `99 0 R`, or that calls the file corrupt over it (§7.3.10
/// makes a dangling reference legal).
#[test]
fn malformed_registry_entries_are_counted_not_listed() {
    let layers = layers_of("malformed-groups.pdf");
    assert_eq!(layers.layers.len(), 4);
    assert_eq!(layers.diagnostics.groups_without_name, 2);
    assert!(layers.layers[0].name.is_empty() && !layers.layers[0].name_declared);
    assert!(layers.layers[1].name.is_empty() && !layers.layers[1].name_declared);

    assert_eq!(layers.diagnostics.direct_group_dicts, 1);
    assert_eq!(layers.diagnostics.dangling_group_references, 1);

    // §8.11.2.3 intent.
    assert_eq!(names(&layers)[2], "Design intent only");
    assert!(!layers.layers[2].intent_view);
    assert_eq!(
        layers.layers[2].intent.as_deref(),
        Some(["Design".to_owned()].as_slice())
    );
    assert!(layers.layers[3].intent_view, "[/View /Design] participates");
}

/// A document with no `/OCProperties` reads as empty, faithful and quiet.
///
/// **Catches:** a reader that treats §8.11.4.2's "a conforming reader
/// shall ignore any optional content structures" as a failure, or that
/// emits diagnostics for it. This is the shape of the overwhelming
/// majority of real PDFs; a diagnostic that fires on every ordinary file
/// is one every caller learns to ignore, and it takes the diagnostics
/// that matter down with it.
#[test]
fn a_document_without_layers_is_empty_and_quiet() {
    let layers = layers_of("no-layers.pdf");
    assert!(layers.layers.is_empty());
    assert!(layers.order.is_empty());
    assert!(layers.radio_groups.is_empty());
    assert!(layers.diagnostics.no_optional_content);
    assert!(layers.diagnostics.is_faithful());
}

/// `list_layers` agrees with `read_layers` on a real file.
///
/// **Catches:** the convenience wrapper drifting — a different scan mode,
/// a different order — which would let two call sites in the same program
/// disagree about the same document, with no obvious reason why.
#[test]
fn list_layers_matches_read_layers_on_a_real_file() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/layers/unregistered-ocg.pdf"
    );
    let bytes = std::fs::read(path).expect("fixture unreadable");
    let doc = Document::from_bytes(bytes).expect("fixture did not parse");
    assert_eq!(list_layers(&doc), read_layers(&doc).layers);
}

/// ★ **A group in both `/ON` and `/OFF` resolves to the OPPOSITE
/// array's answer, and is disclosed** (decision 038).
///
/// Table 101's `/ON` row says the array is *redundant* when
/// `/BaseState` is `ON` — and an array carrying no information
/// cannot override anything, so the opposite array decides. That is
/// exactly §8.11.4.5 b), which is why the two loci that look like a
/// contradiction are one function.
///
/// The second assertion matters as much as the first: a resolution
/// nobody is told about leaves an operator unable to tell it from a
/// bug, in a document whose `/ON` array plainly names the layer they
/// are looking at.
#[test]
fn a_group_in_both_arrays_is_off_and_said_so() {
    let read = layers_of("on-off-contradiction.pdf");
    let both = read
        .layers
        .iter()
        .find(|l| l.name == "In both arrays")
        .expect("the fixture's both-listed group");
    assert!(
        !both.visible_by_default,
        "with /BaseState ON the opposite array (/OFF) decides"
    );
    let neither = read
        .layers
        .iter()
        .find(|l| l.name == "In neither")
        .expect("the control group");
    assert!(
        neither.visible_by_default,
        "a group in neither array is unaffected — a reader that just hid everything \
             would pass the first assertion and fail this one"
    );
    assert_eq!(read.diagnostics.contradictory_on_off_groups, 1);
    assert!(!read.diagnostics.is_faithful());
}

/// **`/BaseState /Unchanged` recovers as `ON`, and says it is
/// recovering.**
///
/// Table 101 requires `/D`'s `/BaseState` to be `ON` if present, so
/// this file violates a `shall`; and §8.11.2.1's "states are not part
/// of the document" means there is no prior state for `Unchanged` to
/// preserve at first open. There is therefore no clause to apply,
/// only a recovery to choose — and `ON` is both Table 101's stated
/// default and the only value `/D` was allowed to carry.
///
/// The rival recovery is what the disclosure exists for: "leave
/// everything as found, process no arrays" would make `/OFF` inert
/// and paint every layer the author turned off, so the two readings
/// produce visibly different pages.
#[test]
fn an_unrecognised_base_state_recovers_as_on_and_discloses_it() {
    let read = layers_of("base-state-unchanged.pdf");
    let off = read
        .layers
        .iter()
        .find(|l| l.name == "In OFF")
        .expect("the group named in /OFF");
    assert!(
        !off.visible_by_default,
        "recovering as ON means /OFF is processed, so this group is hidden"
    );
    let untouched = read
        .layers
        .iter()
        .find(|l| l.name == "Left alone")
        .expect("the control group");
    assert!(untouched.visible_by_default);
    assert!(read.diagnostics.base_state_unrecognised);
    assert!(
        !read.diagnostics.base_state_off_in_default,
        "Unchanged is not OFF — the two diagnostics must not collapse into one"
    );
    assert!(!read.diagnostics.is_faithful());
}

/// ★ **Decision 037's open question, pinned at TODAY's answer.**
///
/// Under `/BaseState /OFF`, does "all the optional content groups in a
/// document" (Table 101) mean every group in the file, or only those
/// registered in `/OCProperties /OCGs`?
///
/// pdfcer currently answers **registered-only**, because the OFF set is
/// enumerated from the registry — so a group reachable only from a
/// page's `/Properties` reports VISIBLE where the literal reading would
/// hide it. `docs/decisions/037-base-state-off-covers-unregistered-groups.md`
/// rules for the literal reading and is **not yet implemented**: the
/// ruling carries a falsifier — open this fixture in Acrobat Reader and
/// see whether the unregistered group's mark appears — and the answer
/// decides whether this becomes a straight fix or a setting (`OC-A2`).
///
/// # This test exists to make the flip LOUD
///
/// It asserts the behaviour the decision expects to CHANGE. That is
/// deliberate and worth stating, because a test asserting a value
/// someone intends to change looks, to a future reader, like a test
/// defending it.
///
/// It is here so the change cannot happen quietly. Whoever implements
/// 037 must edit this test, and editing it means reading this comment
/// and confirming the falsifier actually ran — which is the step that
/// would otherwise be skipped, since the implementation is a
/// straightforward refactor and the evidence for it is not.
///
/// If this fails and you did not mean to change layer visibility, the
/// registry-enumeration path has moved and something else is now
/// deciding what an unregistered group does.
#[test]
fn base_state_off_currently_leaves_unregistered_groups_visible() {
    let read = layers_of("base-state-off-unregistered.pdf");
    let find = |name: &str| {
        read.layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("fixture must contain {name:?}"))
    };

    // The two controls, which BOTH readings agree on. Without them a
    // regression that hid or showed everything would still satisfy the
    // assertion that matters.
    assert!(
        find("Registered, in ON").visible_by_default,
        "/ON re-enables under /BaseState /OFF"
    );
    assert!(
        !find("Registered, not in ON").visible_by_default,
        "a registered group absent from /ON stays off"
    );

    // The experiment.
    let unregistered = find("Never registered");
    assert!(
        !unregistered.in_default_config,
        "the fixture's point is that this group is NOT in /OCGs"
    );
    assert!(
        unregistered.visible_by_default,
        "TODAY pdfcer reports an unregistered group visible under /BaseState /OFF. \
         If you are implementing decision 037, this is the assertion to invert — \
         and the falsifier (Acrobat Reader on this fixture) must have been run first."
    );

    // The disclosure that exists precisely because the question was left
    // open: an operator is told the file reached the one case where
    // pdfcer's answer is knowingly a choice rather than a reading.
    assert!(
        read.diagnostics.base_state_off_with_unregistered,
        "the open case must be disclosed, not silently resolved"
    );
}
