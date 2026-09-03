//! # Optional content — layers (OCGs), read (ISO 32000-1:2008 §8.11)
//!
//! A PDF's *optional content groups* are what a viewer shows in its
//! **Layers** panel. This module turns the catalog's `/OCProperties`
//! structure — a registry array, a default configuration dictionary, and
//! a presentation-order tree that may nest — into an owned, ordered
//! [`Vec<Layer>`] plus the [`OrderNode`] tree the document declared.
//!
//! It exists so a panel can *list* layers. It is deliberately the only
//! half of the job: see [the read-only section](#this-module-is-read-only)
//! below, which is a contract, not a scope note.
//!
//! ## Why this is not a two-line dictionary read
//!
//! §8.11 stores four facts about a layer in four different places, and
//! three of the four are places the layer itself cannot see:
//!
//! | Fact | Where it lives |
//! |---|---|
//! | the layer's **name** | the OCG dictionary's `/Name` (§8.11.2.1, Table 98) |
//! | whether it starts **visible** | the catalog's `/OCProperties /D` configuration — `/BaseState`, then `/ON`/`/OFF` (§8.11.4.3, Table 101) |
//! | whether it can be **toggled** | that same `/D`'s `/Locked` array (Table 101, PDF 1.6) |
//! | whether toggling it turns **something else off** | that same `/D`'s `/RBGroups` array (Table 101) |
//!
//! An OCG dictionary read on its own therefore tells you a name and
//! nothing that matters. Everything a panel needs to *behave* correctly
//! is in the configuration, and the configuration addresses groups by
//! indirect reference — which is why every field on [`Layer`] except
//! `name` is derived from a set-membership test against the catalog, not
//! from the group's own dictionary.
//!
//! ## The four load-bearing decisions in this file
//!
//! ### 1. Visibility is delegated, never re-derived
//!
//! [`Layer::visible_by_default`] is computed by calling
//! [`crate::annot::optional_content_default_off`] and testing membership.
//! That function is the **same** one
//! [`crate::annot::oc_is_hidden`] uses to decide whether an annotation is
//! painted, and the same one `pdfcer-render`'s annotation pass calls.
//!
//! This is not code reuse for tidiness. A second resolver would drift —
//! not immediately, but on the first `/BaseState` edge case one of them
//! learned and the other did not — and the symptom of that drift is a
//! layers panel that reports a layer "on" about content the renderer is
//! hiding. The operator has no way to diagnose that: both surfaces are
//! authoritative-looking and they disagree. Delegating makes the class of
//! bug unrepresentable rather than merely unlikely.
//!
//! One consequence of the delegation is inherited and is stated plainly
//! rather than papered over — see
//! [the `/BaseState` caveat](#the-basestate-off--unregistered-group-caveat).
//!
//! ### 2. Declared order is reported, never a sorted order
//!
//! Table 101's `/Order` "specif[ies] the order for presentation of
//! optional content groups in a conforming reader's user interface", and
//! its elements may nest. pdfcer reports that tree ([`Layers::order`]) and
//! flattens it, **pre-order**, into [`Layers::layers`].
//!
//! A panel that sorts alphabetically is not showing a tidier version of
//! the document; it is showing a *different* document. A CAD exporter
//! that grouped `Hidden lines` under `Geometry` and put `Geometry` after
//! `Border` said something with that arrangement, and the arrangement is
//! the only place it said it. There is no `/Order`-independent notion of
//! "the right order" to fall back on.
//!
//! **The nesting rule is derived from the standard's examples, not from
//! a sentence in it**, and that is worth knowing before trusting it. The
//! table enumerates the element forms and never assigns a parent to a
//! subtree; the rule is recoverable only from EXAMPLE 2's rendering
//! (`/Order [1 0 R [2 0 R 3 0 R]]` shows `Sublayer A`/`Sublayer B`
//! indented under `Layer 1`) and EXAMPLE 1's (`[[(Frog Anatomy) 1 0 R
//! 2 0 R] …]` shows a heading with two entries under it). The rule
//! [`merge_nested`] implements satisfies both examples. What neither
//! example nor any sentence covers is a **labelled array that is also a
//! child of a preceding group** — `[1 0 R [(Label) 2 0 R]]` — which is
//! precisely the shape a producer would emit for a *named sublayer
//! folder*, so the gap sits at a likely input. pdfcer resolves it by
//! letting the label win (the array becomes a labelled subtree, sibling
//! to the preceding group rather than its child), because a label that
//! silently became a child's child would move content the author
//! grouped. Recorded in the spec RAG as ambiguity `DA-A3`.
//!
//! Two further consequences of that table, both easy to get backwards:
//!
//! - **`/Order`'s default in `/D` is the empty array, and that is a
//!   `shall`** — "In the default configuration dictionary, the default
//!   value shall be an empty array", and `[]` "explicitly specifies that
//!   no groups shall be presented". So a `/D` with no `/Order` means a
//!   conforming panel shows **nothing**, not "everything in `/OCGs`".
//!   pdfcer still *lists* those groups, with [`Layer::in_order`] `false`,
//!   and leaves the presentation decision to the shell — a reader that
//!   silently showed an empty panel for a document full of layers would
//!   be conforming and useless, and the flag is what lets a caller be
//!   both.
//! - **Table 101 says groups not in `/Order` "shall not be presented in
//!   any user interface that uses the configuration."** Note the second
//!   qualifier — *that uses the configuration*. A deliberately labelled,
//!   non-default "show every registered group" view is arguably outside
//!   the `shall`; a default panel that quietly ignores `/Order` is not.
//!
//! ### 3. An unregistered group is a layer, not a parse error
//!
//! §8.11.4.2 says `/OCProperties /OCGs` is Required and shall list
//! **every** OCG in the document. Files break that constantly: a group
//! survives in `/D /Order`, or on an annotation's `/OC`, or in a page's
//! `/Resources /Properties`, after an editing tool rewrote the registry
//! and missed it.
//!
//! The standard states **no reader behaviour** for the violation. pdfcer's
//! choice is therefore a *disclosure*, not a conformance verdict: the
//! group is listed, with [`Layer::in_default_config`] `false` and
//! [`Layer::discovered_via`] naming the route that found it. Dropping it
//! would be worse in the specific way that matters — the content is on
//! screen and the panel has no row for it, so the operator can see it and
//! cannot turn it off.
//!
//! ### 4. Radio-group membership is reported *before* a toggle, not after
//!
//! Table 101's `/RBGroups` is "an array of arrays, each of which
//! represents a collection of optional content groups whose states are
//! intended to be mutually exclusive" — at most one member ON. A caller
//! that discovers this by toggling has already changed two layers. So
//! [`Layer::radio_group`] carries the index and [`Layers::radio_groups`]
//! carries the full membership, both populated by the read, so a UI can
//! render the constraint (a radio widget rather than a checkbox) before
//! the operator's first click.
//!
//! Groups may appear in more than one inner array. The standard does not
//! forbid it and does not say what a reader does with it, and the
//! constraints are not jointly satisfiable in the obvious way.
//! [`Layer::radio_group`] therefore reports the **first** array a group
//! belongs to, and [`LayerDiagnostics::overlapping_radio_groups`] counts
//! the overlaps so a caller cannot mistake "first" for "only".
//!
//! That silence is a **genuine, permanent gap in the standard**, not a
//! clause nobody found. The proof is a sibling contrast: §8.11.4.4 states
//! the multi-membership rule explicitly for `/AS` ("If a given optional
//! content group appears in more than one `OCGs` array, its state shall
//! be ON only if all categories … have a state of ON"), and §12.6.4.12
//! states it for the `SetOCGState` action ("A group **may** appear more
//! than once in the `State` array"). Three structurally identical
//! questions; two answered, one silent — and ISO 32000-2 rewrites the
//! `/RBGroups` row without addressing it. Recorded as `DA-N1`.
//!
//! Radio membership also collides with `/Locked`, and the standard does
//! not resolve that either (`DA-A8`): a locked group's state "cannot be
//! changed through the user interface", while a sibling being turned ON
//! means "all others **shall** be turned OFF". Reported, not resolved —
//! resolving it is the toggling surface's decision to make and to
//! disclose.
//!
//! ## This module is read-only
//!
//! There is **no toggle verb here, and that is deliberate.**
//!
//! §8.11.2.1 is explicit that a group's ON/OFF state is *"not part of the
//! PDF document"* — it is initialised from `/OCProperties /D` when the
//! document opens and lives in the consumer thereafter. Acrobat behaves
//! the same way and the behaviour is recorded in
//! `Acrobat_Features/layers__ocg_visibility_and_defaults.md`: toggling a
//! layer in the viewer is **session-scoped with zero file-format
//! footprint**, and the `/D` configuration changes only if the operator
//! takes a further, explicit act of saving.
//!
//! So the live visibility of a layer is the **shell's** session state.
//! This module hands the shell its starting values ([`Layer::visible_by_default`])
//! and the constraints those values move under ([`Layer::locked`],
//! [`Layer::radio_group`]); what the shell does with them afterwards is
//! not a document edit and must not become one by accident.
//!
//! Writing a changed state back — rewriting `/D /ON` and `/D /OFF` — is a
//! real capability and a real Pass, and it belongs on an explicit save
//! path that does not exist yet. Putting a `set_visible` next to
//! [`read_layers`] would make "the operator clicked a checkbox" and "the
//! operator edited the file" the same call, which is precisely the
//! confusion `CLAUDE.md` rule 3 (minimal-diff editing) and rule 4 (fuzzy,
//! never sneaky) exist to prevent.
//!
//! Note also that [`Layer::locked`] is *reported*, not *enforced*, at this
//! layer: this module has nothing to enforce against, since it changes
//! nothing. Enforcement is the toggling surface's obligation, and the
//! Acrobat reference is unambiguous that a locked layer's visibility
//! cannot be changed through the ordinary viewing control.
//!
//! ## Contract
//!
//! - **Infallible.** [`read_layers`] returns a value, never a `Result`.
//!   A document with no optional content yields an empty listing with
//!   clean diagnostics — which is the correct answer for the
//!   overwhelming majority of PDFs, not a degraded one.
//! - **Never panics.** No `unwrap`, no `expect`, no indexing; the crate
//!   denies all four (`lib.rs`). Every traversal is iterative or
//!   depth-bounded, so a hostile file cannot exhaust the stack either.
//! - **Bounded and cycle-safe.** At most [`MAX_LAYERS`] groups, an
//!   `/Order` tree at most [`MAX_ORDER_DEPTH`] deep and
//!   [`MAX_ORDER_NODES`] wide, and at most [`MAX_RESOURCE_NODES`]
//!   resource dictionaries visited. No array object in `/Order` is
//!   entered twice, so `20 0 obj [20 0 R] endobj` — a well-formed file
//!   describing an infinite tree — terminates. Every limit that bites
//!   sets a counter on [`LayerDiagnostics`]; a truncated listing is never
//!   presented as a complete one.
//! - **Nothing is dropped silently.** An unregistered group is listed
//!   and flagged. A group with no `/Name` is listed with
//!   [`Layer::name_declared`] `false` rather than an invented name. An
//!   `/Order` element of a type the table does not permit is counted.
//! - **Read-only.** Nothing here mutates a document or touches the
//!   round-trip path (`CLAUDE.md` rule 3): the structure is parsed into a
//!   parallel value tree and the file's own objects are untouched.
//!
//! ## Where a layer can be reached from
//!
//! [`LayerSource`] enumerates the routes, and the enum's ordering *is*
//! the scan order, which *is* the order unregistered groups appear in the
//! flat listing. §8.11.3.1 says there are exactly two ways content
//! declares membership — marked-content sections (§8.11.3.2) and an `/OC`
//! entry on an XObject or annotation (§8.11.3.3) — and §8.11.4 adds the
//! configuration dictionaries. All of them are scanned:
//!
//! 1. `/OCProperties /D /Order`, pre-order — the presentation tree.
//! 2. `/OCProperties /OCGs` — the registry, in its declared order.
//! 3. The rest of `/D`: `/ON`, `/OFF`, `/Locked`, `/RBGroups`, and the
//!    `/OCGs` of each `/AS` usage-application dictionary (Table 103).
//! 4. Each alternate configuration in `/OCProperties /Configs`, same
//!    sub-arrays. Alternate configurations are scanned for **discovery
//!    only** — they never affect [`Layer::visible_by_default`],
//!    [`Layer::locked`] or [`Layer::radio_group`], all three of which are
//!    `/D`'s alone, because `/D` is by definition the configuration in
//!    force when the document opens.
//! 5. Every page's annotations (`/OC`, §8.11.3.3) and resource tree
//!    (`/Properties` for the `BDC /OC` operand required by §14.6.2, and
//!    `/XObject` / `/Pattern` streams' own `/OC`, recursively through
//!    their nested `/Resources`).
//!
//! Step 5 costs a page-tree walk, so it is selectable: see [`LayerScan`].
//! It is **on by default**, because a panel that omits a layer the
//! content actually uses is exactly the failure mode decision 3 above is
//! about, and paying for a page walk to avoid it is the right trade for a
//! panel that opens once per document.
//!
//! ### The `/BaseState /OFF` + unregistered-group caveat
//!
//! Table 101 says `/BaseState` initialises the states of **all** groups
//! before `/ON`/`/OFF` override. [`crate::annot::optional_content_default_off`]
//! implements "all groups" as "every group in `/OCProperties /OCGs`",
//! because that array is the only enumeration of them the catalog offers.
//!
//! For a file that is both `/BaseState /OFF` *and* has unregistered
//! groups, those two readings differ: the spec's "all" would include the
//! unregistered group, the implementation's does not, so pdfcer reports it
//! `visible_by_default == true`.
//!
//! pdfcer keeps the shared implementation's answer rather than correcting
//! it here, for decision 1's reason: the renderer will paint that content,
//! and a panel saying "off" about content that is on screen is a worse
//! failure than a panel agreeing with a renderer that is arguably too
//! lenient. The situation is **disclosed**, not hidden —
//! [`LayerDiagnostics::base_state_off_with_unregistered`] is set exactly
//! when the combination occurs. Fixing it properly means changing
//! `annot.rs`'s function so both surfaces move together; this module must
//! not fork the semantics to do it.
//!
//! ## Not covered here, and named so a later session does not assume it was
//!
//! - **Evaluating an OCMD's visibility policy.** Table 99's `/P`
//!   (`AllOn`/`AnyOn`/`AnyOff`/`AllOff`) and PDF 1.6's `/VE` visibility
//!   expressions decide whether *content* is shown. That is
//!   [`crate::annot::oc_is_hidden`]'s job. This module only *expands* an
//!   OCMD to find which OCGs exist behind it; the policy is irrelevant to
//!   the question "what layers does this document have?".
//! - **`/VE` traversal for discovery.** A group named only inside a `/VE`
//!   array and nowhere else is not found. `/VE`'s own NOTE 2 tells writers
//!   to supply `/OCGs` alongside it for compatibility, so the case needs a
//!   file that is malformed *and* ignored that note; it is a real gap, and
//!   a bounded `/VE` walk is the fix if a corpus ever shows it happening.
//! - **Two further OCG reference sites**, both real and both unscanned:
//!   an **alternate image dictionary's own `/OC`** (§8.9.5.4, Table 91)
//!   and the **`SetOCGState` action's `/State` array** (§12.6.4.12, Table
//!   213). A group reachable only from one of those is missed. The
//!   `SetOCGState` case is the more likely of the two — it is how a
//!   document ships a "show all layers" button — and finding it means
//!   walking every annotation action and outline action, which belongs
//!   with an action-graph pass rather than here.
//! - **`/Usage` dictionaries** (Table 102) — `/Print`, `/View`, `/Zoom`,
//!   `/Language`, `/Export`, `/PageElement`. Recognised as present but not
//!   read. A panel that offers a separate *print* visibility column needs
//!   them; the Acrobat reference records that as an open comparison point.
//! - **`/AS` auto-state application.** The usage-application dictionaries
//!   are scanned for the OCG references they name, never *applied* — pdfcer
//!   does not change a layer's state because the zoom changed.
//! - **Type 3 font `/CharProcs` resources.** A glyph procedure is a
//!   content stream and may carry `BDC /OC`, so an OCG could in principle
//!   be reachable only from there. Not walked: it needs the font
//!   dictionary layer, and no observed file does it.
//! - **Parsing content streams.** `BDC /OC /P1` names `/P1` in the
//!   resource `/Properties` dictionary; pdfcer reads the *whole*
//!   `/Properties` dictionary rather than parsing the stream to see which
//!   names are actually used. That over-reports (an entry left behind by an
//!   editor is listed) and never under-reports, which is the right
//!   direction for a panel.
//!
//! ## An apparent contradiction that RECONCILES — decision 038, ruled
//!
//! Table 101's `/BaseState` row says the base state is applied and then
//! "the `ON` **and** `OFF` arrays shall be processed". §8.11.4.5 b) says
//! instead that **either** the `ON` or the `OFF` array is processed,
//! "whichever is opposite to `BaseState`". Read as two procedures, those
//! disagree for a group named in **both** arrays.
//!
//! **They are not two procedures.** Table 101's own `ON` and `OFF` rows
//! each add a sentence that settles it: *"If the `BaseState` entry is
//! `ON`, this entry is redundant"*, and the mirror for `OFF`. Redundancy
//! is a **testable** claim — delete the entry and compare — and it holds
//! under exactly one processing order: **the matching array first, the
//! opposite array last.** An array applied immediately after `BaseState`
//! set every group to that same value is a no-op. So Table 101 read in
//! full IS §8.11.4.5 b) with a redundant no-op prepended: same function
//! on every input, for `BaseState` `ON` and `OFF` alike.
//!
//! So pdfcer is not "taking a side". It implements what **both** loci
//! require, and the doc comments cite both — citing only §8.11.4.5 b)
//! made the code look like it had ignored the table.
//!
//! Honest weight: the redundancy sentences are descriptive, not `shall`s,
//! so the argument is interpretive. It is the strongest available — an
//! interpretation that falsifies the standard's own factual statement
//! about its data model is the wrong interpretation — and the two
//! alternatives were measured out of existence (`in case of conflict`
//! occurs zero times in 756 pages; ISO/IEC Directives Part 2 states no
//! precedence convention).
//!
//! **Cite the edition.** `Table 101` is ISO 32000-1 only; ISO 32000-2
//! renumbers the configuration-dictionary table to **Table 99**.
//!
//! **A deployed reader disagrees.** Mozilla `pdf.js` applies `/ON` then
//! `/OFF` unconditionally, with no `BaseState` dependence — the order
//! Table 101's own redundancy sentence rules out. For a group in both
//! arrays under `/BaseState /OFF`, pdf.js hides it and pdfcer shows it.
//! That divergence is real and shipped, so a "match other readers"
//! tiebreak would push AWAY from the ruling; it is recorded rather than
//! quietly followed, because agreement with an implementation is not
//! evidence about the standard. Acrobat's behaviour in that cell has
//! **not** been measured.
//!
//! The genuine residue is `/BaseState /Unchanged` with a both-listed
//! group, which neither locus defines — unreachable in pdfcer, since
//! `Unchanged` is illegal in `/D`. It is named here
//! so a later session does not rediscover the disagreement from a
//! confusing file.
//!
//! ## Spec sources
//!
//! **Every clause and table number below is ISO 32000-1:2008 (PDF 1.7)**,
//! stated rather than assumed, because a citation without an edition is a
//! citation that will eventually be read against the wrong table — and
//! ISO 32000-2:2020 renumbers exactly the tables this module leans on
//! hardest:
//!
//! | Structure | 32000-1:2008 | 32000-2:2020 |
//! |---|---|---|
//! | optional content **configuration** dictionary | Table 101 | **Table 99** (verified) |
//! | optional content **usage** dictionary | Table 102 | **Table 100** (verified) |
//! | OCG dict / OCMD / `/OCProperties` / usage-application | 98 / 99 / 100 / 103 | 96 / 97 / 98 / 101 — *inferred from the −2 offset, unverified* |
//!
//! Clause numbers are unchanged between the editions. Where the 2.0 table
//! number is only inferred, cite the clause alone. The one substantive
//! 2.0 change in this module's scope is to `/RBGroups`, which gains
//! "None of the inner array elements shall be an empty array" — `[[]]`
//! becomes non-conforming, and is still handled here (it yields an empty
//! inner group and no members).
//!
//! - `iso32000__s__8.11.md` — §8.11 in full: Table 98 (OCG dictionary),
//!   Table 99 (OCMD), Table 100 (`/OCProperties`), Table 101
//!   (configuration dictionary: `/BaseState`, `/ON`, `/OFF`, `/Order`,
//!   `/RBGroups`, `/Locked`, `/ListMode`, `/Intent`, `/AS`), Tables
//!   102/103 (usage and usage-application), §8.11.2.3 (intent),
//!   §8.11.3.1–3.3 (the two membership mechanisms), §8.11.4.2 (the
//!   "`/OCProperties` absent ⇒ ignore all optional content" rule).
//! - `iso32000__s__7.7.2.md` — §7.7.2 Table 28: `/OCProperties` on the
//!   document catalog, "Required if a document contains optional
//!   content".
//! - `iso32000__s__7.9.2.md` — §7.9.2 text strings; `/Name` is one, so
//!   both the PDFDocEncoding and UTF-16BE forms decode here.
//! - `iso32000__s__14.6.md` — §14.6.2's requirement that a `BDC` property
//!   list operand naming an OCG be a **named resource** in
//!   `/Properties`, which is why scanning that dictionary finds
//!   marked-content layers without parsing a content stream.
//! - `iso32000__s__12.5.2.md` / `iso32000__s__12.5.3.md` — the annotation
//!   `/OC` entry, and the rule (§8.11.3.3) that annotation visibility is
//!   the OC state **ANDed** with the §12.5.3 flags.
//! - `iso32000__ref__optional_content_order.md` — the derived
//!   enumerator consolidator built for this module: the `/Order` element
//!   grammar with both worked examples, the ambiguity register
//!   (`DA-A1`…`DA-A11`, `DA-N1`…`DA-N8`, erratum `DA-E1`), and the
//!   fourteen places in the format an OCG reference can appear.
//! - `Acrobat_Features/layers__ocg_visibility_and_defaults.md` — the
//!   behavioural reference: session-scoped toggling, locked layers, the
//!   radio-group open question.

use std::collections::{BTreeMap, BTreeSet};

use crate::annot::{optional_content_default_off, page_annotations};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::pages_in;
use crate::textstring::decode_text_string;

/// Maximum number of distinct layers reported (pdfcer policy,
/// `ARCHITECTURE.md` §10).
///
/// §8.11 imposes no limit and Annex C's implementation limits do not
/// mention optional content, so this is pdfcer policy on untrusted input,
/// not a spec constant. Real documents are in the tens; a CAD assembly
/// drawing with a layer per component might reach the low hundreds. Four
/// thousand is far past anything authored on purpose and still small
/// enough that the listing cannot be used to exhaust memory. Exceeding it
/// sets [`LayerDiagnostics::layer_truncation`].
pub const MAX_LAYERS: usize = 4096;

/// Maximum `/Order` nesting entered.
///
/// Matches [`crate::outline::MAX_OUTLINE_DEPTH`] deliberately: the two are
/// the same kind of limit on the same kind of structure (an author-visible
/// tree in a viewer's side panel), and a document one surface can display
/// while the other silently flattens is a worse failure than either cap
/// alone. Real nesting reaches three or four. Exceeding it sets
/// [`LayerDiagnostics::order_depth_truncations`] and drops only the
/// subtree below the cap, never the ancestors already read.
pub const MAX_ORDER_DEPTH: usize = 32;

/// Maximum `/Order` elements examined across the whole tree.
///
/// A width bound as well as a depth bound, because a single flat array of
/// a million references is depth 1 and would otherwise be walked in full.
/// Exceeding it sets [`LayerDiagnostics::order_node_truncation`].
pub const MAX_ORDER_NODES: usize = 8192;

/// Maximum resource dictionaries entered during the page sweep.
///
/// Form XObjects carry their own `/Resources`, which may hold further form
/// XObjects; a file can nest that arbitrarily and can make two forms
/// reference each other. Visited objects are tracked so a cycle
/// terminates, and this bound then caps the honest-but-enormous case.
/// Exceeding it sets [`LayerDiagnostics::resource_scan_truncated`].
pub const MAX_RESOURCE_NODES: usize = 8192;

/// How much of the document [`read_layers_with`] examines.
///
/// The distinction is a real cost/completeness trade, so it is a
/// parameter rather than a hidden constant — but the *default* is the
/// complete one, because the incomplete answer's failure mode (a layer
/// the content uses and the panel does not list) is invisible to the
/// operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LayerScan {
    /// Read `/OCProperties` only — the registry, `/D`, and `/Configs`.
    ///
    /// Cheap and constant-time in the page count. Correct for every
    /// conforming file, since §8.11.4.2 requires every OCG to be
    /// registered. Misses exactly the groups that requirement is broken
    /// for.
    CatalogOnly,
    /// `/OCProperties` **plus** every page's annotations and resource
    /// tree. The default.
    #[default]
    CatalogAndPages,
}

/// The route by which a layer was first reached.
///
/// Recorded per layer because it answers the operator's real question
/// about an unregistered group — *"where is this thing actually used?"* —
/// and because the variant order is the scan order, which fixes the
/// position of unregistered groups in the flat listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LayerSource {
    /// Named in the default configuration's `/Order` (Table 101).
    Order,
    /// Listed in `/OCProperties /OCGs` (Table 100) — the conforming case.
    Registry,
    /// Named by some other `/D` entry: `/ON`, `/OFF`, `/Locked`,
    /// `/RBGroups`, or an `/AS` usage-application dictionary.
    DefaultConfig,
    /// Named only by an alternate configuration in `/OCProperties
    /// /Configs`.
    AlternateConfig,
    /// Reached through an annotation's `/OC` entry (§8.11.3.3), possibly
    /// via an OCMD.
    Annotation,
    /// Reached through a page resource dictionary's `/Properties` — the
    /// operand a content stream's `BDC /OC` names (§8.11.3.2, §14.6.2).
    MarkedContent,
    /// Reached through the `/OC` entry of a form or image XObject, or a
    /// pattern stream (§8.11.3.3).
    XObject,
}

/// One optional content group, with everything a panel needs to render a
/// row and honour the constraints that row moves under.
///
/// `#[non_exhaustive]` so later Passes can add `/Usage`-derived fields (a
/// separate print-visibility column is the known candidate) without a
/// breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layer {
    /// The OCG's object identity.
    ///
    /// Always known, because every route that reaches a group reaches it
    /// by indirect reference. A group written as a **direct** dictionary
    /// inside `/OCGs` is syntactically legal and has no identity — nothing
    /// can point `/OFF` at it and nothing can toggle it — so it is
    /// counted in [`LayerDiagnostics::direct_group_dicts`] and not listed.
    pub id: ObjId,
    /// `/Name` (Table 98, **Required**), decoded as a §7.9.2 text string.
    ///
    /// Empty when the group declares no usable name — check
    /// [`Layer::name_declared`] before displaying it. It is empty rather
    /// than synthesised ("Layer 3", the object number, the word
    /// "Untitled") because `CLAUDE.md` rule 4 forbids pdfcer presenting
    /// something it made up as though the document said it. A UI that
    /// wants a placeholder is welcome to one; it must be the UI's, and it
    /// must look like a placeholder.
    pub name: String,
    /// Whether `/Name` was present **and** was a string.
    ///
    /// `false` covers both "absent" — which Table 98 forbids, so it is a
    /// malformation — and "present but the wrong type", which is the same
    /// malformation with a different spelling.
    pub name_declared: bool,
    /// Whether every byte of `/Name` decoded ([`crate::textstring::DecodedText::exact`]).
    ///
    /// `false` means at least one U+FFFD was substituted — an undefined
    /// PDFDocEncoding code, an odd trailing byte after a UTF-16BE BOM, or
    /// an unpaired surrogate. Surfaced so a panel can say "some characters
    /// could not be decoded" rather than showing a replacement glyph as if
    /// the author had typed it.
    pub name_exact: bool,
    /// Whether the group is **ON** when the document opens, per
    /// `/OCProperties /D` (§8.11.4.3).
    ///
    /// Computed by delegating to
    /// [`crate::annot::optional_content_default_off`] — the same
    /// resolution the renderer uses, deliberately (see the module docs,
    /// decision 1). Note that this is the *initial* state only: §8.11.2.1
    /// puts the live state outside the document entirely.
    pub visible_by_default: bool,
    /// Whether the group is in `/D /Locked` (Table 101, PDF 1.6) — its
    /// state "cannot be changed through the user interface".
    ///
    /// **Reported, not enforced, here** — this module changes nothing, so
    /// it has nothing to enforce against. The toggling surface owns that.
    ///
    /// **A lock is a UI lock, not immutability, and the table says so
    /// itself:** "A conforming reader may allow the states of optional
    /// content groups to be changed by means other than the user
    /// interface, such as JavaScript or items in the `AS` entry of a
    /// configuration dictionary." So `locked` means *the operator's
    /// checkbox is disabled*, not *this layer's state is fixed*. A UI
    /// that presents it as tamper-proofing is overstating it, and the
    /// Acrobat reference is consistent: free Reader cannot unlock a
    /// locked layer, which is a permission on the toggle, not protection
    /// of the content.
    ///
    /// Note also `DA-E1`, an erratum: `/Locked` is the only one of `/D`'s
    /// three list-shaped entries with **no** cross-configuration
    /// inheritance clause (`/Order` and `/RBGroups` both default to
    /// `/D`'s value in an alternate configuration). Read as written, an
    /// alternate configuration with no `/Locked` locks nothing. Not
    /// amended in ISO 32000-2. Immaterial here — pdfcer reads locks from
    /// `/D` only — but load-bearing the moment alternate configurations
    /// become selectable.
    pub locked: bool,
    /// Index into [`Layers::radio_groups`] of the **first** `/D
    /// /RBGroups` inner array this group belongs to, if any.
    ///
    /// Membership means at most one group in that array is ON at a time,
    /// so a UI should render the row as a radio button and must expect
    /// turning it on to turn a sibling off. When a group is in more than
    /// one array — legal, unaddressed by the standard, and not jointly
    /// satisfiable — this is the first, and
    /// [`LayerDiagnostics::overlapping_radio_groups`] is non-zero.
    pub radio_group: Option<usize>,
    /// Whether the group is listed in `/OCProperties /OCGs` (Table 100),
    /// which §8.11.4.2 says it shall be.
    ///
    /// `false` is a real, common malformation and not a reason to hide the
    /// row — see the module docs, decision 3.
    pub in_default_config: bool,
    /// Whether the group appears anywhere in `/D /Order`.
    ///
    /// Table 101: groups not listed in `/Order` are **not presented** in
    /// the reader's user interface. pdfcer reports them anyway, flagged,
    /// because "the author hid this from the panel" and "the author does
    /// not have this layer" are different facts and only one of them is
    /// worth acting on.
    pub in_order: bool,
    /// Whether the group participates under the default `View` intent
    /// (§8.11.2.3).
    ///
    /// `true` when `/Intent` is absent (the default is `View`) or names
    /// `View` among its values. `false` means the group declares only
    /// `Design` — the author's structural organisation of the artwork —
    /// and a reader configured for `View`, which `/D`'s own `/Intent`
    /// shall be, may legitimately ignore it. A panel that shows it as an
    /// ordinary toggle is claiming an effect it may not have.
    pub intent_view: bool,
    /// `/Intent` exactly as declared, `None` when the key is absent.
    ///
    /// Kept alongside [`Layer::intent_view`] because §8.11.2.3 says a
    /// conforming **writer** shall use only `View` or `Design`, and says
    /// nothing to stop a reader meeting a third value. Rather than fold an
    /// unknown intent into a boolean, the raw names are preserved.
    pub intent: Option<Vec<String>>,
    /// Whether the group's dictionary carries a `/Usage` entry (Table 102).
    ///
    /// Not parsed — see the module's "Not covered here". Reported so that
    /// a caller can tell a group whose author supplied print/zoom/export
    /// automation apart from one that did not, without this module
    /// pretending to understand it.
    pub has_usage: bool,
    /// Whether the object declared `/Type /OCG`.
    ///
    /// `false` for an untyped, group-shaped dictionary. Those are accepted
    /// as groups — matching [`crate::annot::oc_is_hidden`], which treats
    /// any non-`/OCMD` `/OC` target as the group itself — because
    /// diverging would give the panel and the renderer different opinions
    /// about the same object, which is decision 1's whole subject.
    pub type_declared: bool,
    /// The first route that reached this group.
    pub discovered_via: LayerSource,
}

/// One node of the `/Order` presentation tree (Table 101).
///
/// A single node type covers all three element forms the table permits,
/// because they are not disjoint in practice — a text-string label heads a
/// subtree, a group may itself have children, and an unlabelled nested
/// array is a subtree with neither.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct OrderNode {
    /// A heading. Table 101: a nested array "may optionally have as its
    /// first element a text string to be used as a **non-selectable**
    /// label in a conforming reader's user interface".
    ///
    /// *Non-selectable* is the standard's own word and is a UI
    /// obligation: a label row **must not carry a checkbox**. It has no
    /// object identity, so it cannot be in `/OFF`, `/Locked`, `/RBGroups`
    /// or `/AS`, and there is nothing about it to toggle. A node with a
    /// `label` and no [`OrderNode::group`] is exactly that row; a node
    /// with both would be a group that also had a heading, which the
    /// format cannot express and this module never produces.
    ///
    /// Table 101 also warns that labels are for "collections of related
    /// optional content groups, and **not** to communicate actual nesting
    /// of content inside multiple layers of groups" — real sublayer
    /// nesting is expressed by an *unlabelled* nested array. So a labelled
    /// node is a folder in the panel; an unlabelled parent is a layer with
    /// sublayers. They look alike and mean different things.
    pub label: Option<String>,
    /// The group this node presents, or `None` for a pure label or an
    /// unlabelled grouping array.
    pub group: Option<ObjId>,
    /// Nested nodes. Non-empty for a subtree; empty for a leaf.
    pub children: Vec<OrderNode>,
}

/// Everything [`read_layers`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Layers {
    /// Every group, flattened in the order the module docs describe:
    /// `/Order` pre-order first, then the registry, then the rest of the
    /// default configuration, then alternate configurations, then the page
    /// sweep. First appearance wins; a group is listed exactly once.
    pub layers: Vec<Layer>,
    /// The `/Order` tree as declared, unsorted and unflattened.
    pub order: Vec<OrderNode>,
    /// `/D /RBGroups` (Table 101), outer array preserved so
    /// [`Layer::radio_group`] indexes into it. Inner arrays hold the
    /// member object ids in declaration order.
    pub radio_groups: Vec<Vec<ObjId>>,
    /// `/D /Name` (Table 101) — the configuration's own name, for a UI
    /// that shows which configuration is in force. `None` when absent.
    pub config_name: Option<String>,
    /// `/D /ListMode` (Table 101): `AllPages` (the default) or
    /// `VisiblePages`, verbatim as a name. Reported, not acted on — pdfcer
    /// has no per-page layer filter yet.
    pub list_mode: Option<String>,
    /// What went wrong, or did not.
    pub diagnostics: LayerDiagnostics,
}

/// Everything that was malformed, truncated, or merely surprising.
///
/// Every field is a **count or a flag about the file**, never a verdict:
/// pdfcer reports what it measured and leaves conformance judgements to a
/// validator. Populated on every read; check
/// [`LayerDiagnostics::is_faithful`] for the one-question summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct LayerDiagnostics {
    /// The catalog has no `/OCProperties`. §8.11.4.2: a reader shall then
    /// ignore all optional content. **Not a defect** — it is the normal
    /// state of most PDFs — and deliberately excluded from
    /// [`LayerDiagnostics::is_faithful`].
    pub no_optional_content: bool,
    /// `/OCProperties` is present but `/D` is absent or is not a
    /// dictionary. Table 100 marks `/D` **Required**. Consequence: no
    /// default states, no order, no locks; every group reads as visible.
    pub missing_default_config: bool,
    /// `/OCProperties` is present but `/OCGs` is absent or is not an
    /// array. Table 100 marks it **Required**. Every group found is then
    /// unregistered by construction.
    pub missing_registry: bool,
    /// Groups reported with [`Layer::in_default_config`] `false`.
    pub unregistered_groups: usize,
    /// Groups reported with [`Layer::name_declared`] `false` — `/Name`
    /// absent or not a string, against Table 98's **Required**.
    pub groups_without_name: usize,
    /// Groups whose `/Name` did not decode cleanly.
    pub names_inexact: usize,
    /// Entries in `/OCGs` or `/Order` that were **direct dictionaries**
    /// rather than indirect references. Legal syntax; unusable as layers,
    /// because a direct object has no identity for `/OFF`, `/Locked` or a
    /// toggle to name.
    pub direct_group_dicts: usize,
    /// References in `/OCGs`, `/Order` or a `/D` array that resolved to
    /// nothing — the object is not in the file, or its generation is
    /// stale.
    ///
    /// **Deliberately excluded from [`LayerDiagnostics::is_faithful`].**
    /// §7.3.10 is explicit that "an indirect reference to an undefined
    /// object shall not be considered an error"; it resolves to null. So
    /// this is a *measurement* — usually an editor that deleted an object
    /// and left the array alone — and not a conformance failure pdfcer is
    /// entitled to pronounce. It is counted rather than swallowed because
    /// an operator chasing a layer that "should be there" needs to know
    /// the file points at something that is not.
    pub dangling_group_references: usize,
    /// `/Order` subtrees dropped for exceeding [`MAX_ORDER_DEPTH`].
    /// Ancestors already read are kept.
    pub order_depth_truncations: usize,
    /// `/Order` traversal stopped at [`MAX_ORDER_NODES`].
    pub order_node_truncation: bool,
    /// `/Order` array objects entered a second time, i.e. cycles. Each is
    /// cut, not followed.
    pub order_cycles: usize,
    /// Elements of `/OCGs` or `/Order` whose *type* is not one the
    /// governing table enumerates — a number, a boolean, a name.
    ///
    /// Distinct from [`LayerDiagnostics::dangling_group_references`],
    /// which is legal. This one is a genuine type confusion and does
    /// count against [`LayerDiagnostics::is_faithful`].
    ///
    /// Note that Table 101's `/Order` row says its elements "**may**
    /// include" OCG dictionaries and arrays, not "shall be one of" — so
    /// the enumeration is not formally closed, and a top-level text
    /// string (which no example shows and no sentence permits) is
    /// accepted as a label rather than counted here. pdfcer is lenient in
    /// the direction that loses nothing.
    pub malformed_group_elements: usize,
    /// Groups appearing in more than one `/RBGroups` inner array.
    pub overlapping_radio_groups: usize,
    /// `/D /BaseState` is `OFF`, which Table 101 says shall not happen in
    /// the **default** configuration ("the value of this entry shall be
    /// ON"). pdfcer follows the file rather than the *shall* — see the
    /// `basestate-off.pdf` fixture for why — and records it here.
    pub base_state_off_in_default: bool,
    /// `/D /BaseState` is `OFF` **and** at least one unregistered group
    /// was found — the case decision 037 was claimed for.
    ///
    /// # ★ MEASURED 2026-08-11: pdfcer's reading matches Acrobat
    ///
    /// The open question was whether `/BaseState /OFF`'s "all groups"
    /// means all groups **registered** in `/OCProperties /OCGs`, or
    /// literally every OCG-shaped object reachable. pdfcer answered
    /// "registered only" as a pragmatic choice while shipping — an
    /// unregistered OCG-shaped dictionary reports VISIBLE — and that was
    /// named rather than ratified, precisely because nobody had checked.
    ///
    /// The `base-state-off-unregistered.pdf` fixture was built to settle
    /// it: three squares, one registered and in `/ON`, one registered and
    /// not, one never registered anywhere. Opened in the installed
    /// Acrobat, it paints the **first and third** and hides the second —
    /// measured as two dark runs 180 pt apart on a scanline, not
    /// eyeballed. pdfcer renders exactly the same three-way answer.
    ///
    /// # What is falsified, precisely — and it is not the standard
    ///
    /// An earlier version of this comment said "the more literal reading
    /// is falsified". That overstated it in the direction that matters.
    ///
    /// The literal reading remains a **correct reading of the text**.
    /// §8.11.2.1 says a group "shall be assigned a state"; Table 101 says
    /// "all the optional content groups **in a document**"; §8.11.4.5 a)
    /// says "applied to **all the groups**". **None of the three says
    /// "all groups listed in `/OCGs`."** The registry-narrowed reading is
    /// not what ISO wrote.
    ///
    /// What the measurement falsifies is that any reader implements the
    /// literal one. Acrobat narrows the quantifier to the `/OCProperties
    /// /OCGs` registry, and `pdf.js` narrows it identically — two
    /// independent implementations agreeing on something the standard
    /// nowhere states. pdfcer matches both.
    ///
    /// So this is a deliberate, measured divergence from the literal text
    /// toward what documents are actually authored against, not a
    /// discovery that the text meant that all along. The diagnostic stays
    /// and now earns its keep twice over: the configuration is unusual,
    /// AND a document relying on it relies on reader convention rather
    /// than on anything Table 101 spells out.
    pub base_state_off_with_unregistered: bool,
    /// Groups named in **both** `/D /ON` and `/D /OFF` — a
    /// self-contradictory configuration (decision 038).
    ///
    /// # Why this is disclosed rather than merely resolved
    ///
    /// It IS resolved, and correctly: §8.11.4.5 b) and Table 101 agree
    /// that the array matching `/BaseState` is redundant, so the
    /// **opposite** array decides. Under `/D` — where `/BaseState` shall
    /// be `ON` — a both-listed group is therefore OFF.
    ///
    /// But the file said two things, and pdfcer picked one. An operator
    /// looking at a layer that is off, in a document whose `/ON` array
    /// names it, has no way to tell a correct resolution from a bug
    /// without being told. The disclosure names the RESOLUTION, not just
    /// the fault: "this file is contradictory" leaves the operator
    /// unable to predict what they are looking at, which is the opposite
    /// of the point.
    ///
    /// Nothing forbids a writer from doing this — both Table 101 rows
    /// bind the reader ("whose state **shall be** set to…"), not the
    /// writer, and §8.11 has no `shall not` about array membership. So it
    /// is a disclosure, not an error.
    pub contradictory_on_off_groups: usize,
    /// `/D /BaseState` is a name other than `ON` or `OFF` — `Unchanged`,
    /// or something Table 101 does not define.
    ///
    /// **This is recovery from non-conforming input, not a clause being
    /// applied**, and the distinction is the whole reason the field
    /// exists. Table 101 requires `/D`'s `/BaseState` to be `ON` if
    /// present, so any other value violates a `shall`; and §8.11.2.1's
    /// "states are not part of the document" means `Unchanged` has no
    /// prior state to preserve at first open — it is meaningful only for
    /// an alternate configuration applied to an already-open document.
    ///
    /// pdfcer recovers by treating it as `ON`, which is both Table 101's
    /// stated default and the only value `/D` was allowed to carry. The
    /// rival recovery — "leave everything as found, process no arrays" —
    /// would make `/OFF` inert and paint every layer the author turned
    /// off, so this is also the safe direction.
    pub base_state_unrecognised: bool,
    /// Groups whose state a `View`-event `/AS` usage application
    /// auto-manages (§8.11.4.4).
    ///
    /// # Why a read-only listing has to say this
    ///
    /// [`Layer::visible_by_default`] is the `/D`-initial state, and that
    /// is the honest quantity for an enumerator to report: §8.11.4.5
    /// makes the viewer's state a function of magnification, so "the"
    /// state of an auto-managed group is not a property of the document
    /// at all.
    ///
    /// But a panel listing that state beside a canvas rendering the
    /// usage-adjusted one shows two different answers to the same
    /// question. A layer banded to a zoom range reads "visible" here
    /// while its content is absent from the page, and the operator has
    /// no way to tell that from a defect.
    ///
    /// So the count is reported and the shells say what it means. Not a
    /// fault — nothing is malformed — which is why it does NOT count
    /// against [`LayerDiagnostics::is_faithful`]: the listing is a
    /// faithful transcription of the file, and the file simply declares
    /// a state that moves.
    pub auto_managed_groups: usize,
    /// The listing stopped at [`MAX_LAYERS`].
    pub layer_truncation: bool,
    /// The page sweep stopped at [`MAX_RESOURCE_NODES`].
    pub resource_scan_truncated: bool,
    /// The page tree could not be walked ([`crate::page_tree::pages_in`]
    /// failed), so annotations and page resources contributed nothing.
    /// The catalog-derived listing is still complete and correct for a
    /// conforming file.
    pub page_scan_failed: bool,
}

impl LayerDiagnostics {
    /// Whether the listing is a faithful transcription of the file.
    ///
    /// Excludes [`LayerDiagnostics::no_optional_content`] and
    /// [`LayerDiagnostics::page_scan_failed`] on purpose. The first is not
    /// a defect at all — most PDFs have no layers, and a diagnostic that
    /// is noisy on good files gets ignored on bad ones, which is the only
    /// way a diagnostic can do harm. The second is a fact about the page
    /// tree, reported in its own field and better surfaced by whatever
    /// already failed to open the pages.
    #[must_use]
    pub fn is_faithful(&self) -> bool {
        !self.missing_default_config
            && !self.missing_registry
            && self.unregistered_groups == 0
            && self.groups_without_name == 0
            && self.names_inexact == 0
            && self.direct_group_dicts == 0
            && self.order_depth_truncations == 0
            && !self.order_node_truncation
            && self.order_cycles == 0
            && self.malformed_group_elements == 0
            && self.overlapping_radio_groups == 0
            && !self.base_state_off_in_default
            && self.contradictory_on_off_groups == 0
            && !self.base_state_unrecognised
            && !self.layer_truncation
            && !self.resource_scan_truncated
    }
}

/// List a document's optional content groups, in the order the document
/// declares.
///
/// The convenience form of [`read_layers`] for a caller that wants the
/// rows and not the tree. Equivalent to `read_layers(graph).layers`.
///
/// Every caveat in the module documentation applies: the visibility
/// reported is the **initial** state (§8.11.2.1 puts the live state
/// outside the document), an unregistered group is still listed, and a
/// group with no `/Name` has an empty one rather than an invented one.
#[must_use]
pub fn list_layers<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<Layer> {
    read_layers(graph).layers
}

/// Read a document's optional content: groups, order tree, radio groups,
/// and diagnostics.
///
/// Uses [`LayerScan::CatalogAndPages`], the complete scan. Use
/// [`read_layers_with`] to opt out of the page sweep.
#[must_use]
pub fn read_layers<G: ObjectGraph + ?Sized>(graph: &G) -> Layers {
    read_layers_with(graph, LayerScan::CatalogAndPages)
}

/// Read a document's optional content, choosing how far to look.
///
/// # Algorithm
///
/// 1. Resolve the catalog's `/OCProperties` (Table 100). Absent ⇒ return
///    an empty listing with `no_optional_content` set: §8.11.4.2 says a
///    reader shall ignore optional content entirely in that case, and an
///    empty result *is* ignoring it.
/// 2. Resolve `/D` (Table 101) and read its scalar entries (`/Name`,
///    `/ListMode`, `/BaseState`), its `/RBGroups` and `/Locked` sets, and
///    ask [`crate::annot::optional_content_default_off`] for the OFF set.
/// 3. Walk `/Order` depth-first, building [`Layers::order`] and recording
///    each group's first appearance. Bounded by [`MAX_ORDER_DEPTH`] and
///    [`MAX_ORDER_NODES`], with a visited set over array objects so a
///    self-referential `/Order` terminates.
/// 4. Add `/OCGs`, then the remaining `/D` arrays, then `/Configs`, then —
///    if the scan asks for it — every page's annotations and resource
///    tree.
/// 5. Materialise a [`Layer`] per discovered id, in discovery order,
///    reading each group's own dictionary for `/Name`, `/Intent`,
///    `/Usage` and `/Type`.
///
/// Discovery is separated from materialisation so that the *order* of the
/// listing is decided in exactly one place, and so that a group found
/// twice by two routes cannot produce two rows.
#[must_use]
pub fn read_layers_with<G: ObjectGraph + ?Sized>(graph: &G, scan: LayerScan) -> Layers {
    let mut out = Layers::default();

    // --- Step 1: the catalog registry ------------------------------------
    let Some(ocp) = graph
        .catalog_dict()
        .and_then(|cat| cat.get(b"OCProperties"))
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    else {
        out.diagnostics.no_optional_content = true;
        return out;
    };

    // Discovery state: insertion-ordered because `BTreeMap` would sort by
    // object number, and object number has nothing to do with the order an
    // author arranged their layers in.
    let mut found: Vec<(ObjId, LayerSource)> = Vec::new();
    let mut seen: BTreeSet<ObjId> = BTreeSet::new();
    let mut diag = LayerDiagnostics::default();

    // --- Step 2: the default configuration -------------------------------
    let d = ocp
        .get(b"D")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict);
    if d.is_none() {
        diag.missing_default_config = true;
    }

    let off = optional_content_default_off(graph);
    let mut locked: BTreeSet<ObjId> = BTreeSet::new();
    let mut radio_of: BTreeMap<ObjId, usize> = BTreeMap::new();

    if let Some(d) = d {
        out.config_name = d
            .get(b"Name")
            .map(|o| graph.resolve(o))
            .and_then(|o| match o {
                Object::String(bytes) => Some(decode_text_string(bytes).text),
                _ => None,
            });
        out.list_mode = d
            .get(b"ListMode")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(&n.0).into_owned());
        let base_state = d
            .get(b"BaseState")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .map(|n| n.0.clone());
        diag.base_state_off_in_default = base_state.as_deref().is_some_and(|n| n == b"OFF");
        // Absent is conforming (Table 101 gives `ON` as the default);
        // a name that is neither is not (decision 038's addendum).
        diag.base_state_unrecognised = base_state
            .as_deref()
            .is_some_and(|n| n != b"ON" && n != b"OFF");
        // Decision 038: a group in BOTH arrays is resolved by the
        // opposite-array rule and disclosed rather than silently picked.
        // Computed over the arrays as read, before any state is derived,
        // so it counts what the FILE says rather than what pdfcer made of
        // it.
        let on_listed: std::collections::BTreeSet<ObjId> =
            crate::annot::oc_refs(graph, d.get(b"ON"))
                .into_iter()
                .collect();
        diag.contradictory_on_off_groups = crate::annot::oc_refs(graph, d.get(b"OFF"))
            .into_iter()
            .filter(|g| on_listed.contains(g))
            .count();

        locked.extend(crate::annot::oc_refs(graph, d.get(b"Locked")));
        out.radio_groups = radio_groups(graph, d.get(b"RBGroups"));
        // `entry().or_insert()`, not `insert()`: the documented rule is
        // that the FIRST array a group belongs to is the one
        // `Layer::radio_group` reports, and a plain `insert` silently
        // makes the LAST one win. The overlap is counted only when the
        // group is already assigned to a *different* array, so a group
        // repeated inside one inner array is not miscounted as an overlap
        // between arrays.
        for (index, members) in out.radio_groups.iter().enumerate() {
            for member in members {
                let assigned = *radio_of.entry(*member).or_insert(index);
                if assigned != index {
                    diag.overlapping_radio_groups += 1;
                }
            }
        }

        // --- Step 3: /Order, the presentation tree -----------------------
        let mut walk = OrderWalk {
            graph,
            visited: BTreeSet::new(),
            budget: MAX_ORDER_NODES,
            found: &mut found,
            seen: &mut seen,
            diag: &mut diag,
        };
        out.order = walk.entry(d.get(b"Order"), 0);
    }

    // --- Step 4: every other route ---------------------------------------
    // The registry itself. Table 100 marks `/OCGs` Required; a file that
    // omits it makes every group unregistered by construction, which is
    // exactly what the flag records.
    match ocp.get(b"OCGs").map(|o| graph.resolve(o)) {
        Some(Object::Array(items)) => {
            for item in items {
                record_group_element(
                    graph,
                    item,
                    LayerSource::Registry,
                    &mut found,
                    &mut seen,
                    &mut diag,
                );
            }
        }
        // Absent, or present as something other than an array. Table 100
        // marks `/OCGs` Required and an array; either way there is no
        // registry, so every group found is unregistered by construction.
        _ => diag.missing_registry = true,
    }
    // The set of registered ids, for `Layer::in_default_config`. Read from
    // the array directly rather than from `found`, because `found` also
    // holds groups reached by every other route.
    let registered: BTreeSet<ObjId> = graph
        .resolve(ocp.get(b"OCGs").unwrap_or(&Object::Null))
        .as_array()
        .map(|items| items.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default();

    if let Some(d) = d {
        for key in [b"ON".as_slice(), b"OFF", b"Locked"] {
            for id in crate::annot::oc_refs(graph, d.get(key)) {
                note(id, LayerSource::DefaultConfig, &mut found, &mut seen);
            }
        }
        for members in &out.radio_groups {
            for id in members {
                note(*id, LayerSource::DefaultConfig, &mut found, &mut seen);
            }
        }
        // Counted from `/D` only: `/Configs` are alternate
        // configurations pdfcer never applies, so their `/AS` entries
        // describe a state this document is not in.
        diag.auto_managed_groups = usage_application_groups(graph, d.get(b"AS")).len();
        for id in usage_application_groups(graph, d.get(b"AS")) {
            note(id, LayerSource::DefaultConfig, &mut found, &mut seen);
        }
    }

    // Alternate configurations: discovery only. `/D` alone decides
    // visibility, locking and radio membership, because `/D` is by
    // definition the configuration in force when the document opens.
    if let Some(configs) = ocp
        .get(b"Configs")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for config in configs {
            let Some(cfg) = graph.resolve(config).as_dict() else {
                continue;
            };
            for key in [b"ON".as_slice(), b"OFF", b"Locked"] {
                for id in crate::annot::oc_refs(graph, cfg.get(key)) {
                    note(id, LayerSource::AlternateConfig, &mut found, &mut seen);
                }
            }
            for members in radio_groups(graph, cfg.get(b"RBGroups")) {
                for id in members {
                    note(id, LayerSource::AlternateConfig, &mut found, &mut seen);
                }
            }
            for id in usage_application_groups(graph, cfg.get(b"AS")) {
                note(id, LayerSource::AlternateConfig, &mut found, &mut seen);
            }
            // An alternate config's own `/Order` names groups too. Walked
            // for discovery, with the same guards, but its tree is
            // discarded: `Layers::order` is `/D`'s, and presenting an
            // alternate configuration's arrangement as the document's
            // would be a lie about which one is in force.
            let mut walk = OrderWalk {
                graph,
                visited: BTreeSet::new(),
                budget: MAX_ORDER_NODES,
                found: &mut found,
                seen: &mut seen,
                diag: &mut diag,
            };
            let _ = walk.entry(cfg.get(b"Order"), 0);
        }
    }

    // --- Step 4b: the page sweep -----------------------------------------
    let ordered_ids: BTreeSet<ObjId> = out.order.iter().flat_map(order_group_ids).collect();

    if scan == LayerScan::CatalogAndPages {
        sweep_pages(graph, &mut found, &mut seen, &mut diag);
    }

    // --- Step 5: materialise ---------------------------------------------
    for (id, source) in found {
        if out.layers.len() >= MAX_LAYERS {
            diag.layer_truncation = true;
            break;
        }
        let Some(layer) = build_layer(
            graph,
            id,
            source,
            &off,
            &locked,
            &radio_of,
            &registered,
            &ordered_ids,
            &mut diag,
        ) else {
            continue;
        };
        out.layers.push(layer);
    }

    diag.unregistered_groups = out.layers.iter().filter(|l| !l.in_default_config).count();
    diag.base_state_off_with_unregistered =
        diag.base_state_off_in_default && diag.unregistered_groups > 0;
    out.diagnostics = diag;
    out
}

// ---------------------------------------------------------------------------
// Discovery helpers
// ---------------------------------------------------------------------------

/// Record `id` as found, if it has not been already.
///
/// First route wins, which is what makes [`LayerSource`]'s variant order
/// meaningful: a group in both `/Order` and `/OCGs` reports `Order`,
/// because that is where the author placed it.
fn note(
    id: ObjId,
    source: LayerSource,
    found: &mut Vec<(ObjId, LayerSource)>,
    seen: &mut BTreeSet<ObjId>,
) {
    if seen.insert(id) {
        found.push((id, source));
    }
}

/// Record one element of an array that is supposed to hold OCG
/// references, classifying the ways it can fail to be one.
///
/// The three failure shapes are kept distinct because they mean different
/// things to whoever has to fix the file: a **direct dictionary** is a
/// producer that forgot indirect objects are required for anything the
/// configuration must address; an **unresolvable reference** is usually an
/// editor that deleted an object and left the array alone; anything else
/// is a type confusion.
fn record_group_element<G: ObjectGraph + ?Sized>(
    graph: &G,
    item: &Object,
    source: LayerSource,
    found: &mut Vec<(ObjId, LayerSource)>,
    seen: &mut BTreeSet<ObjId>,
    diag: &mut LayerDiagnostics,
) {
    match item {
        Object::Reference(id) => {
            if graph.resolved(*id).as_dict().is_some() {
                note(*id, source, found, seen);
            } else {
                diag.dangling_group_references += 1;
            }
        }
        Object::Dict(_) | Object::Stream(_) => diag.direct_group_dicts += 1,
        // A number, a name, a boolean, a nested array: Table 100 says
        // `/OCGs` is "an array of optional content groups", and none of
        // these is one. A type confusion, not a §7.3.10 dangling
        // reference, so it counts against `is_faithful`.
        _ => diag.malformed_group_elements += 1,
    }
}

/// `/RBGroups` (Table 101) — an array of arrays of OCG references.
///
/// Inner elements that are not references are skipped silently here; the
/// radio structure is about *membership*, and a malformed member simply is
/// not a member. An inner element that is not an array at all is skipped
/// for the same reason — there is no sensible way to read a number as a
/// set of mutually exclusive groups.
fn radio_groups<G: ObjectGraph + ?Sized>(graph: &G, obj: Option<&Object>) -> Vec<Vec<ObjId>> {
    let Some(outer) = obj.map(|o| graph.resolve(o)).and_then(Object::as_array) else {
        return Vec::new();
    };
    outer
        .iter()
        .filter_map(|inner| graph.resolve(inner).as_array())
        .map(|members| {
            members
                .iter()
                .filter_map(|m| graph.resolve(m).as_reference().or_else(|| m.as_reference()))
                .collect()
        })
        .collect()
}

/// The OCG references named by a configuration's `/AS` array — usage
/// application dictionaries (§8.11.4.4, Table 103), each of which has an
/// `/OCGs` array naming the groups its automatic state applies to.
///
/// Scanned for **discovery only**. pdfcer never applies a usage
/// application: changing a layer's state because the zoom changed is a
/// behaviour, and this module has none.
fn usage_application_groups<G: ObjectGraph + ?Sized>(
    graph: &G,
    obj: Option<&Object>,
) -> Vec<ObjId> {
    let Some(entries) = obj.map(|o| graph.resolve(o)).and_then(Object::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| graph.resolve(e).as_dict())
        .flat_map(|d| crate::annot::oc_refs(graph, d.get(b"OCGs")))
        .collect()
}

/// Every group id mentioned anywhere in an [`OrderNode`] subtree.
///
/// Iterative with an explicit stack rather than recursive: the tree is
/// already depth-capped when it is built, but a helper that recurses over
/// caller-supplied `OrderNode`s would be a stack-overflow surface for
/// anyone who constructed one by hand.
fn order_group_ids(node: &OrderNode) -> Vec<ObjId> {
    let mut ids = Vec::new();
    let mut stack: Vec<&OrderNode> = vec![node];
    while let Some(current) = stack.pop() {
        if let Some(id) = current.group {
            ids.push(id);
        }
        for child in &current.children {
            stack.push(child);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// /Order traversal
// ---------------------------------------------------------------------------

/// State for the `/Order` walk (Table 101).
///
/// A struct rather than a pile of `&mut` parameters because the walk is
/// mutually recursive with itself through nested arrays and every call
/// needs all six pieces.
struct OrderWalk<'a, G: ?Sized> {
    graph: &'a G,
    /// Array **objects** already entered. Global, not path-scoped: an
    /// array reached twice is either a cycle or a shared subtree, and
    /// entering it once and reporting the second reach as a cycle is
    /// terminating in both cases, whereas a path-scoped set is not
    /// (a diamond of shared arrays would blow up exponentially).
    visited: BTreeSet<ObjId>,
    /// Remaining element budget, shared across the whole tree.
    budget: usize,
    found: &'a mut Vec<(ObjId, LayerSource)>,
    seen: &'a mut BTreeSet<ObjId>,
    diag: &'a mut LayerDiagnostics,
}

impl<G: ObjectGraph + ?Sized> OrderWalk<'_, G> {
    /// Enter the `/Order` entry itself, which may be a direct array or an
    /// indirect reference to one (§7.3.10 substitutability).
    fn entry(&mut self, obj: Option<&Object>, depth: usize) -> Vec<OrderNode> {
        let Some(raw) = obj else {
            return Vec::new();
        };
        // A let-chain, so the visited-set insert happens only when the
        // entry really is a reference: a direct `/Order` array has no
        // object identity and therefore needs no cycle guard.
        if let Some(id) = raw.as_reference()
            && !self.visited.insert(id)
        {
            self.diag.order_cycles += 1;
            return Vec::new();
        }
        let Some(items) = self.graph.resolve(raw).as_array() else {
            return Vec::new();
        };
        self.walk(items, depth)
    }

    /// Walk one `/Order` array level.
    ///
    /// Table 101 permits three element forms, and this loop is the whole
    /// grammar:
    ///
    /// - **an OCG reference** — becomes a leaf [`OrderNode`] with
    ///   `group` set;
    /// - **a text string** — becomes a node with `label` set and no
    ///   group, the "non-selectable label" case;
    /// - **a nested array** — becomes the *children* of the node that
    ///   precedes it, which is how the format expresses "these groups are
    ///   under that one". If there is no preceding node, or the preceding
    ///   node already has children, the nested array stands alone as an
    ///   unlabelled grouping node so nothing is lost.
    ///
    /// The one extra rule: if walking a nested array yields a first node
    /// that is a **pure label**, that label becomes the subtree's root and
    /// absorbs the array's remaining nodes as its children. That is what
    /// makes `[(Sheet metal) a b]` a heading with two entries rather than
    /// a heading followed by two unrelated siblings.
    ///
    /// Recursion depth is capped at [`MAX_ORDER_DEPTH`] before the
    /// recursive call, so the stack is bounded by a constant regardless of
    /// input.
    fn walk(&mut self, items: &[Object], depth: usize) -> Vec<OrderNode> {
        let mut out: Vec<OrderNode> = Vec::new();
        for item in items {
            if self.budget == 0 {
                self.diag.order_node_truncation = true;
                break;
            }
            self.budget -= 1;

            // A reference may lead to a group, to a nested array, or to a
            // string; resolve once and classify on the result, keeping the
            // id so a group keeps its identity and an array gets a cycle
            // guard.
            let (id, value) = match item.as_reference() {
                Some(id) => (Some(id), self.graph.resolved(id)),
                None => (None, item),
            };

            match value {
                Object::Dict(_) | Object::Stream(_) => match id {
                    Some(id) => {
                        note(id, LayerSource::Order, self.found, self.seen);
                        out.push(OrderNode {
                            label: None,
                            group: Some(id),
                            children: Vec::new(),
                        });
                    }
                    // A direct dictionary in `/Order`: displayable in
                    // principle, addressable by nothing, so it cannot be a
                    // row a caller may toggle.
                    None => self.diag.direct_group_dicts += 1,
                },
                Object::String(bytes) => out.push(OrderNode {
                    label: Some(decode_text_string(bytes).text),
                    group: None,
                    children: Vec::new(),
                }),
                Object::Array(nested) => {
                    if depth + 1 >= MAX_ORDER_DEPTH {
                        self.diag.order_depth_truncations += 1;
                        continue;
                    }
                    if let Some(id) = id
                        && !self.visited.insert(id)
                    {
                        self.diag.order_cycles += 1;
                        continue;
                    }
                    // `nested` borrows the graph, `self.walk` borrows
                    // `self` mutably; clone the slice's elements first so
                    // the two borrows do not overlap. Cloning an `/Order`
                    // level is cheap (references and short strings) and is
                    // bounded by the element budget above.
                    let nested: Vec<Object> = nested.to_vec();
                    let sub = self.walk(&nested, depth + 1);
                    merge_nested(&mut out, sub);
                }
                // A dangling reference resolves to null (§7.3.10); a
                // number, name or boolean is a type Table 101 does not
                // list. Both are counted, neither is fatal.
                Object::Null => {
                    if id.is_some() {
                        self.diag.dangling_group_references += 1;
                    } else {
                        self.diag.malformed_group_elements += 1;
                    }
                }
                _ => self.diag.malformed_group_elements += 1,
            }
        }
        out
    }
}

/// Attach the nodes a nested `/Order` array produced to the level above.
///
/// Split out of [`OrderWalk::walk`] so the three-way choice is stated once
/// and can be read without the surrounding loop:
///
/// 1. If the subtree's first node is a **pure label** (a `label`, no
///    `group`), it becomes the subtree's root and takes the rest as
///    children — Table 101's "may optionally have as its first element a
///    text string to be used as a non-selectable label", and EXAMPLE 1's
///    rendering of it.
///
///    **This rule is checked before rule 2 on purpose**, and that
///    ordering is pdfcer resolving ambiguity `DA-A3`. `[1 0 R [(Label)
///    2 0 R]]` — a labelled array following a group — is covered by
///    neither of the standard's examples nor by any sentence in it, and
///    it is the shape a producer would most plausibly emit for a named
///    sublayer folder. Letting rule 2 win would bury the label as the
///    preceding group's child and move content the author had grouped
///    under a heading; letting the label win keeps the heading a sibling,
///    which is what the label being "non-selectable" implies it is for.
/// 2. Otherwise, if the previous sibling exists and has no children yet,
///    the subtree becomes **its** children — the format's way of nesting
///    groups under a group (or under a label already emitted at this
///    level).
/// 3. Otherwise the subtree stands alone as an unlabelled grouping node,
///    so no group is lost to a shape the file did not quite express.
fn merge_nested(out: &mut Vec<OrderNode>, mut sub: Vec<OrderNode>) {
    if sub.is_empty() {
        return;
    }
    let leads_with_label = sub
        .first()
        .is_some_and(|n| n.label.is_some() && n.group.is_none());
    if leads_with_label {
        let mut root = sub.remove(0);
        root.children.extend(sub);
        out.push(root);
        return;
    }
    if let Some(previous) = out.last_mut()
        && previous.children.is_empty()
    {
        previous.children = sub;
        return;
    }
    out.push(OrderNode {
        label: None,
        group: None,
        children: sub,
    });
}

// ---------------------------------------------------------------------------
// The page sweep (§8.11.3.2, §8.11.3.3)
// ---------------------------------------------------------------------------

/// Find groups reachable only from page content: annotation `/OC`, the
/// `/Properties` resources a `BDC /OC` names, and XObject/pattern `/OC`.
///
/// Failure to walk the page tree is recorded and swallowed. A document
/// whose pages will not parse still has a catalog, and reporting no layers
/// because of an unrelated structural fault would be the wrong trade — the
/// catalog listing is complete for every conforming file, and this sweep
/// only ever *adds*.
fn sweep_pages<G: ObjectGraph + ?Sized>(
    graph: &G,
    found: &mut Vec<(ObjId, LayerSource)>,
    seen: &mut BTreeSet<ObjId>,
    diag: &mut LayerDiagnostics,
) {
    let Ok(pages) = pages_in(graph) else {
        diag.page_scan_failed = true;
        return;
    };

    // Annotations first, then resources, so a group used by both reports
    // the more specific of the two routes.
    for page in &pages {
        for annot in page_annotations(graph, page.id) {
            if let Some(oc) = annot.oc {
                expand_oc(graph, oc, LayerSource::Annotation, found, seen);
            }
        }
    }

    // A worklist over resource dictionaries. Iterative rather than
    // recursive: form XObjects nest without limit and may reference each
    // other, and `visited` plus `budget` make both terminate.
    let mut visited: BTreeSet<ObjId> = BTreeSet::new();
    let mut budget = MAX_RESOURCE_NODES;
    let mut queue: Vec<Dict> = pages.iter().map(|p| p.resources.clone()).collect();

    while let Some(resources) = queue.pop() {
        if budget == 0 {
            diag.resource_scan_truncated = true;
            break;
        }
        budget -= 1;

        // §14.6.2: a `BDC /OC` operand shall be a named resource in
        // `/Properties`, because OCGs and OCMDs are indirect objects and a
        // content stream cannot name one directly. Reading the whole
        // dictionary therefore finds every marked-content layer on the
        // page without parsing a byte of the content stream.
        if let Some(props) = resources
            .get(b"Properties")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
        {
            for (_, value) in &props.0 {
                if let Some(id) = value.as_reference() {
                    expand_oc(graph, id, LayerSource::MarkedContent, found, seen);
                }
            }
        }

        // §8.11.3.3: a form or image XObject may carry `/OC`. Form
        // XObjects also carry their own `/Resources`, so the walk
        // continues through them. `/Pattern` is included for the same
        // reason — a tiling pattern is a content stream with resources.
        for key in [b"XObject".as_slice(), b"Pattern"] {
            let Some(entries) = resources
                .get(key)
                .map(|o| graph.resolve(o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            for (_, value) in &entries.0 {
                let Some(id) = value.as_reference() else {
                    continue;
                };
                if !visited.insert(id) {
                    continue;
                }
                let Some(xobject) = graph.resolved(id).as_dict() else {
                    continue;
                };
                if let Some(oc) = xobject.get(b"OC").and_then(Object::as_reference) {
                    expand_oc(graph, oc, LayerSource::XObject, found, seen);
                }
                if let Some(nested) = xobject
                    .get(b"Resources")
                    .map(|o| graph.resolve(o))
                    .and_then(Object::as_dict)
                {
                    queue.push(nested.clone());
                }
            }
        }
    }
}

/// Turn one `/OC` target into the groups behind it.
///
/// §8.11.3.3 lets an `/OC` entry be **either** an OCG or an OCMD. The
/// discrimination here mirrors [`crate::annot::oc_is_hidden`] exactly, and
/// that is the point: an untyped, group-shaped dictionary is treated as an
/// OCG by both, so the panel lists precisely the objects the renderer
/// resolves against. Only `/Type /OCMD` takes the membership branch.
///
/// Table 99's `/OCGs` may be a single dictionary **or** an array, and both
/// forms are handled inline below. `/VE` is not traversed — see the
/// module's "Not covered here".
fn expand_oc<G: ObjectGraph + ?Sized>(
    graph: &G,
    oc: ObjId,
    source: LayerSource,
    found: &mut Vec<(ObjId, LayerSource)>,
    seen: &mut BTreeSet<ObjId>,
) {
    let Some(dict) = graph.resolved(oc).as_dict() else {
        return;
    };
    let is_ocmd = dict
        .get(b"Type")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .is_some_and(|n| n.0 == b"OCMD");
    if is_ocmd {
        for id in crate::annot::oc_refs(graph, dict.get(b"OCGs")) {
            note(id, source, found, seen);
        }
        // ★ `/VE` IS A DISCOVERY SITE TOO, AND IT WAS BEING MISSED.
        //
        // A visibility expression names groups that nothing else in the
        // document need mention. The renderer evaluates them correctly
        // (`annot::eval_ve`), so their content genuinely appears and
        // disappears — while the panel, walking only `/OCGs`, had no row
        // for them. Content that changes with a layer nobody can see or
        // toggle is the worst shape this panel can take: the operator
        // watches the page change and has nothing to attribute it to.
        //
        // Found by a spec sweep after `/VE` evaluation shipped, not by
        // the evaluation work itself — which had no reason to look at
        // the enumerator, and is exactly why the two halves of a feature
        // want checking against each other.
        ve_groups(graph, dict.get(b"VE"), 0, &mut Vec::new(), &mut |id| {
            note(id, source, found, seen);
        });
    } else {
        note(oc, source, found, seen);
    }
}

/// Walk a `/VE` visibility expression for the OCGs it names
/// (§8.11.2.2), for DISCOVERY only.
///
/// # Discovery, not evaluation — and the difference matters here
///
/// [`crate::annot::eval_ve`] decides what an expression MEANS; this only
/// asks which groups appear in it, so that every group a document
/// mentions can be listed. It is therefore deliberately more permissive:
/// an expression this refuses to evaluate — an unknown operator, a `Not`
/// with two operands — still names real groups, and those groups still
/// belong in the panel. Refusing to list them because the expression is
/// malformed would hide the very groups an operator most needs to see.
///
/// The operand rule (`DA-N17`: an OCMD is not a legal operand) is
/// likewise NOT enforced here. A nested OCMD is a malformation, and
/// walking into its `/OCGs` finds groups that exist; the panel's job is
/// to be complete about what the file mentions.
///
/// Shares [`MAX_ORDER_DEPTH`] and a cycle guard with the evaluator for
/// the same reason it has them: the grammar permits arbitrary indirect
/// nesting, so a self-referential array is legal syntax describing an
/// infinite tree.
fn ve_groups<G: ObjectGraph + ?Sized>(
    graph: &G,
    obj: Option<&Object>,
    depth: usize,
    visited: &mut Vec<ObjId>,
    note: &mut impl FnMut(ObjId),
) {
    let Some(obj) = obj else {
        return;
    };
    if depth > MAX_ORDER_DEPTH {
        return;
    }
    if let Some(id) = obj.as_reference() {
        if visited.contains(&id) {
            return;
        }
        visited.push(id);
    }
    match graph.resolve(obj) {
        Object::Array(items) => {
            for item in items {
                ve_groups(graph, Some(item), depth + 1, visited, note);
            }
        }
        // A non-array operand that resolves to a dictionary is a group
        // (or a malformed OCMD, whose own members are walked so nothing
        // the file names is lost).
        Object::Dict(d) => {
            if let Some(id) = obj.as_reference() {
                let is_ocmd = graph
                    .resolve(d.get(b"Type").unwrap_or(&Object::Null))
                    .as_name()
                    .is_some_and(|n| n.0 == b"OCMD");
                if is_ocmd {
                    for inner in crate::annot::oc_refs(graph, d.get(b"OCGs")) {
                        note(inner);
                    }
                } else {
                    note(id);
                }
            }
        }
        // The operator name, or anything else the grammar allows.
        _ => {}
    }
    if obj.as_reference().is_some() {
        visited.pop();
    }
}

// ---------------------------------------------------------------------------
// Materialisation
// ---------------------------------------------------------------------------

/// Read one group's own dictionary and combine it with the
/// catalog-derived sets into a [`Layer`].
///
/// Returns `None` only when the id does not resolve to a dictionary at
/// all, which means a route named it and the object is not there; the
/// caller counts nothing extra because [`record_group_element`] already
/// counted the reachable cases, and a `/D`-array reference to a missing
/// object is not a group the panel could show under any reading.
#[allow(clippy::too_many_arguments)]
fn build_layer<G: ObjectGraph + ?Sized>(
    graph: &G,
    id: ObjId,
    source: LayerSource,
    off: &BTreeSet<ObjId>,
    locked: &BTreeSet<ObjId>,
    radio_of: &BTreeMap<ObjId, usize>,
    registered: &BTreeSet<ObjId>,
    ordered: &BTreeSet<ObjId>,
    diag: &mut LayerDiagnostics,
) -> Option<Layer> {
    let dict = graph.resolved(id).as_dict()?;

    // §7.9.2 text string. Absent or wrong-typed leaves the name empty and
    // sets `name_declared` false — never a synthesised placeholder, per
    // `CLAUDE.md` rule 4.
    let decoded = match dict.get(b"Name").map(|o| graph.resolve(o)) {
        Some(Object::String(bytes)) => Some(decode_text_string(bytes)),
        _ => None,
    };
    let name_declared = decoded.is_some();
    if !name_declared {
        diag.groups_without_name += 1;
    }
    let name_exact = decoded.as_ref().is_none_or(|d| d.exact);
    if !name_exact {
        diag.names_inexact += 1;
    }
    let name = decoded.map(|d| d.text).unwrap_or_default();

    // §8.11.2.3: `/Intent` is a name or an array of names, default
    // `View`. Absent means the default, which is why `intent` is `None`
    // and `intent_view` is still true.
    let intent = match dict.get(b"Intent").map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => Some(vec![String::from_utf8_lossy(&n.0).into_owned()]),
        Some(Object::Array(items)) => Some(
            items
                .iter()
                .filter_map(|i| graph.resolve(i).as_name())
                .map(|n| String::from_utf8_lossy(&n.0).into_owned())
                .collect(),
        ),
        _ => None,
    };
    let intent_view = intent
        .as_ref()
        .is_none_or(|names| names.iter().any(|n| n == "View"));

    Some(Layer {
        id,
        name,
        name_declared,
        name_exact,
        visible_by_default: !off.contains(&id),
        locked: locked.contains(&id),
        radio_group: radio_of.get(&id).copied(),
        in_default_config: registered.contains(&id),
        in_order: ordered.contains(&id),
        intent_view,
        intent,
        has_usage: dict.get(b"Usage").is_some(),
        type_declared: dict
            .get(b"Type")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name)
            .is_some_and(|n| n.0 == b"OCG"),
        discovered_via: source,
    })
}

// ---------------------------------------------------------------------------
// Unit tests — hand-built graphs
// ---------------------------------------------------------------------------
//
// These drive a synthesised `ObjectGraph` so a traversal shape can be
// expressed that no file format would let you write down twice. The
// whole-file counterparts, which run the same claims through the lexer,
// the object parser and the xref table, live in `tests/layers.rs`.

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::object::{Dict, Name};
    use std::collections::BTreeMap;

    struct TestGraph {
        objects: BTreeMap<ObjId, Object>,
        trailer: Dict,
    }

    impl ObjectGraph for TestGraph {
        fn value(&self, id: ObjId) -> Option<&Object> {
            self.objects.get(&id)
        }
        fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
            self.trailer.get(key)
        }
    }

    fn id(n: u32) -> ObjId {
        ObjId::new(n, 0)
    }

    fn dict(entries: &[(&[u8], Object)]) -> Dict {
        Dict(
            entries
                .iter()
                .map(|(k, v)| (Name(k.to_vec()), v.clone()))
                .collect(),
        )
    }

    fn ocg(name: &str) -> Object {
        Object::Dict(dict(&[
            (b"Type", Object::Name(Name(b"OCG".to_vec()))),
            (b"Name", Object::String(name.as_bytes().to_vec())),
        ]))
    }

    /// Build a graph whose catalog (object 1) carries `ocproperties`, plus
    /// whatever extra objects the test needs.
    fn graph_with(ocproperties: Object, extra: &[(u32, Object)]) -> TestGraph {
        let mut objects = BTreeMap::new();
        objects.insert(
            id(1),
            Object::Dict(dict(&[
                (b"Type", Object::Name(Name(b"Catalog".to_vec()))),
                (b"OCProperties", ocproperties),
            ])),
        );
        for (n, obj) in extra {
            objects.insert(id(*n), obj.clone());
        }
        TestGraph {
            objects,
            trailer: dict(&[(b"Root", Object::Reference(id(1)))]),
        }
    }

    fn refs(nums: &[u32]) -> Object {
        Object::Array(nums.iter().map(|n| Object::Reference(id(*n))).collect())
    }

    /// A document with no `/OCProperties` yields an empty listing, not an
    /// error and not a diagnostic storm.
    ///
    /// **Catches:** a reader that treats §8.11.4.2's "shall ignore" as a
    /// failure. Most PDFs have no layers; if the common case produced
    /// noise, every caller would learn to ignore the diagnostics, and the
    /// ones that matter would go with them.
    #[test]
    fn no_optional_content_is_empty_and_quiet() {
        let graph = TestGraph {
            objects: [(
                id(1),
                Object::Dict(dict(&[(b"Type", Object::Name(Name(b"Catalog".to_vec())))])),
            )]
            .into_iter()
            .collect(),
            trailer: dict(&[(b"Root", Object::Reference(id(1)))]),
        };
        let layers = read_layers(&graph);
        assert!(layers.layers.is_empty());
        assert!(layers.diagnostics.no_optional_content);
        assert!(layers.diagnostics.is_faithful());
    }

    /// `/OFF` decides default visibility, and the reported order is
    /// `/Order`'s, not the registry's and not alphabetical.
    ///
    /// **Catches:** a reader that sorts rows by name, or that iterates
    /// `/OCGs` instead of `/Order`. The registry here lists 4, 5, 6 while
    /// `/Order` says 6, 4, 5 and the names are "Alpha", "Beta", "Gamma"
    /// — so registry order, alphabetical order and declared order are
    /// three different answers and only one of them passes.
    #[test]
    fn order_beats_registry_and_alphabet() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5, 6])),
                (
                    b"D",
                    Object::Dict(dict(&[(b"Order", refs(&[6, 4, 5])), (b"OFF", refs(&[4]))])),
                ),
            ])),
            &[(4, ocg("Alpha")), (5, ocg("Beta")), (6, ocg("Gamma"))],
        );
        let layers = read_layers(&graph);
        let names: Vec<&str> = layers.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Gamma", "Alpha", "Beta"]);
        assert!(!layers.layers[1].visible_by_default, "4 is in /OFF");
        assert!(layers.layers[0].visible_by_default);
        assert!(
            layers
                .layers
                .iter()
                .all(|l| l.in_order && l.in_default_config)
        );
    }

    /// A nested array becomes the preceding node's children, and a nested
    /// array that begins with a text string becomes a labelled subtree.
    ///
    /// **Catches:** a reader that flattens `/Order`, or that attaches a
    /// sub-array to the wrong parent. Both produce the same set of layers
    /// and a different document.
    #[test]
    fn order_nesting_and_labels() {
        let order = Object::Array(vec![
            Object::String(b"Sheet metal".to_vec()),
            Object::Array(vec![
                Object::Reference(id(4)),
                Object::Array(vec![Object::Reference(id(5)), Object::Reference(id(6))]),
            ]),
            Object::Reference(id(7)),
        ]);
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5, 6, 7])),
                (b"D", Object::Dict(dict(&[(b"Order", order)]))),
            ])),
            &[
                (4, ocg("Zulu")),
                (5, ocg("Yankee")),
                (6, ocg("Xray")),
                (7, ocg("Whiskey")),
            ],
        );
        let layers = read_layers(&graph);

        assert_eq!(layers.order.len(), 2, "the label subtree, then group 7");
        let label = &layers.order[0];
        assert_eq!(label.label.as_deref(), Some("Sheet metal"));
        assert!(label.group.is_none());
        assert_eq!(label.children.len(), 1);
        assert_eq!(label.children[0].group, Some(id(4)));
        assert_eq!(label.children[0].children.len(), 2);
        assert_eq!(label.children[0].children[0].group, Some(id(5)));
        assert_eq!(layers.order[1].group, Some(id(7)));

        // Pre-order flattening: 4 before its children, 7 last.
        let ids: Vec<ObjId> = layers.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [id(4), id(5), id(6), id(7)]);
    }

    /// A self-referential `/Order` terminates, is counted, and does not
    /// cost the groups that were reachable before the loop.
    ///
    /// **Catches:** a missing cycle guard. The failure mode is not a wrong
    /// answer — it is a hang, which a test suite reports as "still
    /// running" and a user reports as "it froze opening my drawing".
    #[test]
    fn order_cycle_terminates_and_is_counted() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4])),
                (
                    b"D",
                    Object::Dict(dict(&[(b"Order", Object::Reference(id(20)))])),
                ),
            ])),
            &[
                (4, ocg("Before the loop")),
                (
                    20,
                    Object::Array(vec![Object::Reference(id(4)), Object::Reference(id(20))]),
                ),
            ],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.diagnostics.order_cycles, 1);
        assert_eq!(layers.layers.len(), 1);
        assert_eq!(layers.layers[0].name, "Before the loop");
        assert!(!layers.diagnostics.is_faithful());
    }

    /// `/Order` nested past [`MAX_ORDER_DEPTH`] truncates the subtree and
    /// says so, rather than overflowing the stack.
    ///
    /// **Catches:** an unbounded recursive walk. `pdfcer-core`'s panic-free
    /// policy treats a stack overflow on untrusted input exactly as it
    /// treats an `unwrap`.
    #[test]
    fn order_depth_is_capped() {
        // A chain of MAX_ORDER_DEPTH + 8 singly-nested direct arrays with
        // a group at the bottom.
        let mut inner = Object::Array(vec![Object::Reference(id(4))]);
        for _ in 0..(MAX_ORDER_DEPTH + 8) {
            inner = Object::Array(vec![inner]);
        }
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4])),
                (b"D", Object::Dict(dict(&[(b"Order", inner)]))),
            ])),
            &[(4, ocg("Deep"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.diagnostics.order_depth_truncations, 1);
        // The group is still listed — via the registry, not via /Order.
        assert_eq!(layers.layers.len(), 1);
        assert!(!layers.layers[0].in_order);
        assert_eq!(layers.layers[0].discovered_via, LayerSource::Registry);
    }

    /// A group named by `/D` but absent from `/OCGs` is listed, flagged,
    /// and keeps its default-OFF state.
    ///
    /// **Catches:** a reader that intersects everything it finds with the
    /// registry. §8.11.4.2 requires the registry to be complete and files
    /// break that constantly; the content is on screen either way, so a
    /// dropped row is a layer the operator can see and cannot control.
    #[test]
    fn unregistered_group_is_listed_and_flagged() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4])),
                (
                    b"D",
                    Object::Dict(dict(&[(b"Order", refs(&[4])), (b"OFF", refs(&[5]))])),
                ),
            ])),
            &[(4, ocg("Registered")), (5, ocg("Ghost"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.layers.len(), 2);
        let ghost = &layers.layers[1];
        assert_eq!(ghost.name, "Ghost");
        assert!(!ghost.in_default_config);
        assert!(!ghost.in_order);
        assert!(!ghost.visible_by_default, "/OFF still applies to it");
        assert_eq!(ghost.discovered_via, LayerSource::DefaultConfig);
        assert_eq!(layers.diagnostics.unregistered_groups, 1);
    }

    /// Radio membership is reported per layer and in full, and an
    /// overlapping member is disclosed rather than silently resolved.
    ///
    /// **Catches:** a panel that renders radio-group members as
    /// independent checkboxes. A caller that has to toggle one to learn
    /// the constraint has already changed two layers, which is exactly
    /// what `CLAUDE.md` rule 4 forbids.
    #[test]
    fn radio_groups_and_locks_are_reported_before_any_toggle() {
        let rb = Object::Array(vec![refs(&[4, 5]), refs(&[5, 6])]);
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5, 6, 7])),
                (
                    b"D",
                    Object::Dict(dict(&[
                        (b"Order", refs(&[4, 5, 6, 7])),
                        (b"RBGroups", rb),
                        (b"Locked", refs(&[4])),
                    ])),
                ),
            ])),
            &[
                (4, ocg("Locked and radio")),
                (5, ocg("In both")),
                (6, ocg("Second only")),
                (7, ocg("Neither")),
            ],
        );
        let layers = read_layers(&graph);
        assert_eq!(
            layers.radio_groups,
            vec![vec![id(4), id(5)], vec![id(5), id(6)]]
        );
        assert_eq!(layers.layers[0].radio_group, Some(0));
        assert!(layers.layers[0].locked);
        assert_eq!(layers.layers[1].radio_group, Some(0), "first array wins");
        assert_eq!(layers.layers[2].radio_group, Some(1));
        assert_eq!(layers.layers[3].radio_group, None);
        assert!(!layers.layers[1].locked);
        assert_eq!(layers.diagnostics.overlapping_radio_groups, 1);
    }

    /// `/BaseState /OFF` inverts the default, `/ON` overrides it, and the
    /// spec violation is disclosed rather than corrected.
    ///
    /// **Catches:** a reader that honours Table 101's "shall be ON" by
    /// ignoring the file's `/BaseState`. That reports the exact inverse
    /// for every group the `/ON` array does not mention — a drawing shipped
    /// dark would open with every layer lit.
    #[test]
    fn base_state_off_is_followed_and_disclosed() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5])),
                (
                    b"D",
                    Object::Dict(dict(&[
                        (b"BaseState", Object::Name(Name(b"OFF".to_vec()))),
                        (b"ON", refs(&[5])),
                        (b"Order", refs(&[4, 5])),
                    ])),
                ),
            ])),
            &[(4, ocg("Dark")), (5, ocg("Lit"))],
        );
        let layers = read_layers(&graph);
        assert!(!layers.layers[0].visible_by_default);
        assert!(layers.layers[1].visible_by_default);
        assert!(layers.diagnostics.base_state_off_in_default);
        assert!(!layers.diagnostics.base_state_off_with_unregistered);
        assert!(!layers.diagnostics.is_faithful());
    }

    /// A group with no `/Name` gets an empty name and a `false` flag —
    /// never an invented one.
    ///
    /// **Catches:** a reader that synthesises "Layer 4" or "Untitled".
    /// `CLAUDE.md` rule 4: anything pdfcer inferred is visible as an
    /// inference. A placeholder that looks like a name is a claim about
    /// the document.
    #[test]
    fn missing_name_is_disclosed_not_invented() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5])),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[4, 5]))]))),
            ])),
            &[
                (
                    4,
                    Object::Dict(dict(&[(b"Type", Object::Name(Name(b"OCG".to_vec())))])),
                ),
                (
                    5,
                    Object::Dict(dict(&[
                        (b"Type", Object::Name(Name(b"OCG".to_vec()))),
                        (b"Name", Object::Integer(42)),
                    ])),
                ),
            ],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.layers.len(), 2);
        assert!(layers.layers.iter().all(|l| l.name.is_empty()));
        assert!(layers.layers.iter().all(|l| !l.name_declared));
        assert_eq!(layers.diagnostics.groups_without_name, 2);
    }

    /// `/Intent /Design` is reported as not participating under `View`;
    /// an absent `/Intent` is `View` by default.
    ///
    /// **Catches:** a panel that shows a Design-only group as an ordinary
    /// toggle. §8.11.2.3 lets a View-configured reader ignore it, so the
    /// toggle may do nothing the operator can see.
    #[test]
    fn design_only_intent_is_flagged() {
        let mut design = dict(&[
            (b"Type", Object::Name(Name(b"OCG".to_vec()))),
            (b"Name", Object::String(b"Construction".to_vec())),
        ]);
        design.0.push((
            Name(b"Intent".to_vec()),
            Object::Name(Name(b"Design".to_vec())),
        ));
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5])),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[4, 5]))]))),
            ])),
            &[(4, Object::Dict(design)), (5, ocg("Normal"))],
        );
        let layers = read_layers(&graph);
        assert!(!layers.layers[0].intent_view);
        assert_eq!(
            layers.layers[0].intent.as_deref(),
            Some(["Design".to_owned()].as_slice())
        );
        assert!(layers.layers[1].intent_view);
        assert!(layers.layers[1].intent.is_none());
    }

    /// An OCMD's members are expanded into layers; an untyped `/OC`
    /// target is itself the group.
    ///
    /// **Catches:** a reader that lists the OCMD object as though it were
    /// a layer (it has no `/Name` and cannot be toggled), or one that
    /// refuses an untyped group and so disagrees with
    /// [`crate::annot::oc_is_hidden`], which accepts it.
    #[test]
    fn ocmd_expands_to_members() {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let graph = graph_with(
            Object::Null,
            &[
                (
                    10,
                    Object::Dict(dict(&[
                        (b"Type", Object::Name(Name(b"OCMD".to_vec()))),
                        (b"OCGs", refs(&[4, 5])),
                    ])),
                ),
                (4, ocg("A")),
                (5, ocg("B")),
                // Untyped, group-shaped: an OCG by `oc_is_hidden`'s rule.
                (
                    6,
                    Object::Dict(dict(&[(b"Name", Object::String(b"Untyped".to_vec()))])),
                ),
            ],
        );
        expand_oc(
            &graph,
            id(10),
            LayerSource::Annotation,
            &mut found,
            &mut seen,
        );
        expand_oc(
            &graph,
            id(6),
            LayerSource::Annotation,
            &mut found,
            &mut seen,
        );
        let ids: Vec<ObjId> = found.iter().map(|(i, _)| *i).collect();
        assert_eq!(ids, [id(4), id(5), id(6)]);
    }

    /// `LayerScan::CatalogOnly` skips the page sweep; the default does
    /// not.
    ///
    /// **Catches:** a `CatalogOnly` that silently still walks pages (the
    /// parameter would be a lie), and a default that silently does not
    /// (the panel would miss content-only layers). Uses a hand-built
    /// graph with no page tree, so `CatalogAndPages` also proves the
    /// page-scan failure is survivable rather than fatal.
    #[test]
    fn scan_mode_controls_the_page_sweep() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4])),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[4]))]))),
            ])),
            &[(4, ocg("Only one"))],
        );
        let catalog_only = read_layers_with(&graph, LayerScan::CatalogOnly);
        assert!(!catalog_only.diagnostics.page_scan_failed);
        let both = read_layers_with(&graph, LayerScan::CatalogAndPages);
        assert!(
            both.diagnostics.page_scan_failed,
            "no page tree in this graph"
        );
        assert_eq!(catalog_only.layers, both.layers);
    }

    /// The `/D` scalars a panel displays are carried through verbatim.
    ///
    /// **Catches:** a reader that drops `/ListMode`, which a later
    /// per-page filter needs, or `/Name`, which is the only way a UI can
    /// tell the operator which configuration it is showing.
    #[test]
    fn config_scalars_round_trip() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4])),
                (
                    b"D",
                    Object::Dict(dict(&[
                        (b"Name", Object::String(b"As shipped".to_vec())),
                        (b"ListMode", Object::Name(Name(b"VisiblePages".to_vec()))),
                        (b"Order", refs(&[4])),
                    ])),
                ),
            ])),
            &[(4, ocg("One"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.config_name.as_deref(), Some("As shipped"));
        assert_eq!(layers.list_mode.as_deref(), Some("VisiblePages"));
    }

    /// A direct dictionary inside `/OCGs` is counted, not listed.
    ///
    /// **Catches:** a reader that lists it anyway and hands a caller a
    /// row whose `id` was invented. A direct object has no identity, so
    /// nothing could ever toggle it or name it in `/OFF`.
    #[test]
    fn direct_group_dict_is_counted_not_listed() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (
                    b"OCGs",
                    Object::Array(vec![Object::Reference(id(4)), ocg("Direct")]),
                ),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[4]))]))),
            ])),
            &[(4, ocg("Indirect"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.layers.len(), 1);
        assert_eq!(layers.diagnostics.direct_group_dicts, 1);
    }

    /// A reference in `/OCGs` that resolves to nothing is counted, not
    /// listed.
    ///
    /// **Catches:** a reader that reports a phantom row for an object the
    /// file does not define — and, in the other direction, a reader that
    /// calls the file corrupt over it. §7.3.10 makes a dangling reference
    /// legal and null-valued, so the listing stays *faithful*: the count
    /// is a measurement for whoever has to fix the file, not a verdict.
    #[test]
    fn dangling_registry_reference_is_counted_not_listed() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 99])),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[4]))]))),
            ])),
            &[(4, ocg("Real"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.layers.len(), 1);
        assert_eq!(layers.diagnostics.dangling_group_references, 1);
        assert!(
            layers.diagnostics.is_faithful(),
            "§7.3.10: a dangling reference is not an error"
        );
    }

    /// A `/D` with no `/Order` still lists every registered group, with
    /// `in_order` false throughout.
    ///
    /// **Catches:** a reader that equates "not in `/Order`" with "not a
    /// layer" and returns nothing. Table 101 makes `[]` the default and
    /// says `[]` presents no groups — so the *conforming panel* is empty,
    /// but the *document* has three layers, and only one of those two
    /// facts is useful to a caller. Also catches the inverse mistake: a
    /// reader that sets `in_order` true because it had nothing to check
    /// against.
    #[test]
    fn absent_order_still_lists_groups_but_not_as_ordered() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5, 6])),
                (b"D", Object::Dict(dict(&[(b"OFF", refs(&[5]))]))),
            ])),
            &[(4, ocg("A")), (5, ocg("B")), (6, ocg("C"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.layers.len(), 3);
        assert!(layers.order.is_empty());
        assert!(layers.layers.iter().all(|l| !l.in_order));
        assert!(layers.layers.iter().all(|l| l.in_default_config));
        assert!(!layers.layers[1].visible_by_default);
        assert!(layers.diagnostics.is_faithful());
    }

    /// A labelled nested array that follows a group becomes a **sibling**
    /// heading, not that group's child.
    ///
    /// **Catches:** the `DA-A3` ambiguity being resolved the other way.
    /// `[1 0 R [(Label) 2 0 R]]` is covered by neither of the standard's
    /// examples; burying the heading under the preceding group would move
    /// content the author had deliberately collected under it, and the
    /// move would be invisible — the same layers, in the same order, one
    /// level deeper.
    #[test]
    fn labelled_array_after_a_group_stays_a_sibling() {
        let order = Object::Array(vec![
            Object::Reference(id(4)),
            Object::Array(vec![
                Object::String(b"Folder".to_vec()),
                Object::Reference(id(5)),
            ]),
        ]);
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5])),
                (b"D", Object::Dict(dict(&[(b"Order", order)]))),
            ])),
            &[(4, ocg("Standalone")), (5, ocg("In the folder"))],
        );
        let layers = read_layers(&graph);
        assert_eq!(layers.order.len(), 2, "group, then the labelled folder");
        assert_eq!(layers.order[0].group, Some(id(4)));
        assert!(
            layers.order[0].children.is_empty(),
            "the label is NOT its child"
        );
        assert_eq!(layers.order[1].label.as_deref(), Some("Folder"));
        assert!(layers.order[1].group.is_none(), "a label is non-selectable");
        assert_eq!(layers.order[1].children.len(), 1);
        assert_eq!(layers.order[1].children[0].group, Some(id(5)));
    }

    /// `list_layers` is exactly `read_layers(..).layers`.
    ///
    /// **Catches:** the convenience wrapper drifting from the full read —
    /// a different scan mode, a different order — which would make two
    /// call sites in the same program disagree about the same document.
    #[test]
    fn list_layers_matches_read_layers() {
        let graph = graph_with(
            Object::Dict(dict(&[
                (b"OCGs", refs(&[4, 5])),
                (b"D", Object::Dict(dict(&[(b"Order", refs(&[5, 4]))]))),
            ])),
            &[(4, ocg("A")), (5, ocg("B"))],
        );
        assert_eq!(list_layers(&graph), read_layers(&graph).layers);
    }
}
