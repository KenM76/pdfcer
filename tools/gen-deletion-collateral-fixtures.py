#!/usr/bin/env python3
"""Generate the documents that size the EIGHT deletion-collateral sites the
`Pass 191.0` audit named but did not fix.

WHY THESE EXIST
===============

`Pass 191.0` fixed ONE instance of a defect class:

    A verb dereferences a dictionary key, assumes the pointee is the kind of
    object that key is defined to hold, and DELETES it -- or deletes and
    overwrites objects reached through it.

There, `/AP` `/N` named a `/Widget` dictionary; the deletion cascade harvested
that widget's `/P` (which names the PAGE), deleted the page, returned `Ok`, and
`to_full_bytes` wrote a file that reloads with `PageTreeError::BadKid`. Nothing
complained in a release build, because `debug_assert_page_tree_still_walks` is
`#[cfg(debug_assertions)]`.

The audit run under R219 -- enumerate the class rather than wait for the next
fuzz finding -- named eight MORE sites of the same shape. An audit that reads
code can be wrong about reachability, so those eight were CLAIMS. These
fixtures are what turns each claim into a measured yes or no, driven by
`crates/pdfcer-core/examples/collateral_probe.rs`.

THE EIGHT SITES, AND THE FILE THAT MAKES EACH FALSIFIABLE
=========================================================

| # | Site                                   | Hostile fixture                        |
|---|----------------------------------------|----------------------------------------|
| 1 | `delete_dimension` sidecar `/Ap`       | `dim-sidecar-ap-names-pagetree.pdf`    |
| 1 | `delete_dimension` sidecar `/Annot`    | `dim-sidecar-annot-names-pagetree.pdf` |
| 2 | `regenerate_dimension_writes` `/Ap`    | `dim-sidecar-ap-names-pagetree.pdf`    |
| 3 | `delete_redaction_mark` `/AP` `/N`     | `redact-ap-names-pagetree.pdf`         |
| 3 | `delete_redaction_mark` `annot_id`     | `redact-mark-is-pagetree-node.pdf`     |
| 4 | `flatten_fields` `/AcroForm` `/Fields` | `flatten-field-is-page.pdf`            |
| 5 | `outline_subtree` `/First`             | `outline-first-names-pagetree.pdf`     |
| 5 | `delete_outline_item` entry gate       | `outline-item-is-page.pdf`             |
| 6 | cascade 1's `/Popup`                   | `popup-is-pagetree-node.pdf`           |
| 7 | `detach_file` name-tree value          | `attach-namevalue-is-pagetree.pdf`     |
| 7 | `detach_file` `/EF` `/F`               | `attach-ef-is-pagetree.pdf`            |
| 8 | `unembed_fonts` `/CIDSet`              | `cidset-names-pagetree.pdf`            |

Sites 1 and 2 are about **ce dimensions** -- the `/Line` + `/IT
/LineDimension` annotations pdfcer AUTHORS, with their `/PieceInfo` sidecar
(project rule 15). Nothing here touches **pdf dimensions**, the measurements a
CAD exporter drew into page content; those are ordinary page content and no
deletion verb reaches them by key.

WHY EVERY SITE ALSO GETS A CONTROL
==================================

Seven control files, and they are the reason this set can tell a FIX from a
REGRESSION. Every one of these verbs has a legitimate success path, and the
cheapest way to make all twelve hostile files behave is to refuse everything --
which would silently break ce-dimension deletion, redaction-mark rejection,
form flattening, bookmark deletion, comment deletion, attachment detachment and
font unembedding, all at once, and pass this fixture set while doing it.

`gen-annot-ap-cascade-fixtures.py` states the same reasoning for its own three
controls out of five. This set follows it deliberately: whatever guard the fix
installs, `*-control.pdf` must still succeed, must still free exactly the
objects the verb is supposed to free, and must still leave the page tree
walking.

WHY THE TRAP IS ALWAYS THE PAGE-TREE ROOT (OBJECT 2), NOT THE PAGE
==================================================================

Object 2 is the `/Pages` node the catalog names in every file here. Pointing
the hostile key at it rather than at the `/Page` is a deliberate choice:

  * Deleting the ROOT produces `NoPageTreeRoot` -- an unambiguous, total loss
    that no fail-clean recovery path can paper over. Deleting a `/Kids` member
    produces `BadKid`, which is the `Pass 191.0` signature and equally fatal,
    but a future recovery pass could plausibly learn to drop a bad kid and
    still open the file. The root cannot be recovered from.
  * It also proves the reach is not special to `/Page` dictionaries. Several of
    these verbs would be *partially* protected by a `/Type /Page` name test
    (`annotation_deletion_guards` has exactly such a test), and a fix that only
    added one would look green against a page-shaped trap. `refuse_if_in_page_tree`
    covers the page, its ancestors AND the catalog; these files are shaped to
    require that, not a name comparison.

Two files deviate and say why in their own docstrings: `flatten-field-is-page.pdf`
(reproducing `Pass 185.1`'s literal input, where the collision IS with a
`/Page`) and `outline-item-is-page.pdf` (whose whole claim is that the entry
gate accepts a `/Page` because a `/Page` carries `/Parent`).

PROVENANCE
==========

Wholly synthetic, byte-authored, `LEGAL.md` Section 5 category (a). Hand-rolled
`assemble()` rather than produced by `pdfcer`, for the reason every sibling
generator in this directory gives: **a fixture generated by the program under
test cannot falsify that program.** The one borrowed byte-blob is the sfnt
donor at `fixtures/synthetic/text/subset-fstype-editable.ttf`, itself a tracked
synthetic font this repository authored, needed because site 8 requires a
GENUINELY removable embedded font alongside the poisoned `/CIDSet` -- the
`/FontFile*` sibling IS type-guarded (`fontinfo.rs`, `let Object::Stream(..)
else { NotAStream }`), so a fake program would be blocked before the `/CIDSet`
was ever reached.

Usage:  python tools/gen-deletion-collateral-fixtures.py
"""

import pathlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
OUT = ROOT / "fixtures" / "synthetic" / "deletion-collateral"
DONOR = ROOT / "fixtures" / "synthetic" / "text" / "subset-fstype-editable.ttf"


# ---------------------------------------------------------------------------
# Serialisation
# ---------------------------------------------------------------------------
def assemble(objects: list[bytes], root: int = 1) -> bytes:
    """Wrap object bodies in a header, classic xref table and trailer.

    Object bodies are emitted verbatim, so a body may be a dictionary or a
    complete dictionary/stream/endstream triple; offsets are computed from the
    emitted bytes either way. Plain ISO 32000-1 Section 7.5.4 xref -- no xref
    streams, no object streams. A fixture that needs a compressed-object parser
    to be read is testing two things at once, and when one of them fails you
    cannot tell which.
    """
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"
    startxref = len(out)
    n = len(objects) + 1
    out += b"xref\n0 " + str(n).encode() + b"\n0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        b"trailer\n<< /Size " + str(n).encode()
        + b" /Root " + str(root).encode() + b" 0 R >>\nstartxref\n"
        + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def stream(dict_body: bytes, payload: bytes) -> bytes:
    """A stream object: dictionary text plus payload, `/Length` correct."""
    head = dict_body.rstrip()
    assert head.endswith(b">>")
    head = head[:-2] + f" /Length {len(payload)} >>".encode()
    return head + b"\nstream\n" + payload + b"\nendstream"


# A minimal, valid form XObject. Content is irrelevant to every verb here --
# what matters is that the object IS a stream, so a type-testing fix admits it
# and the control files keep succeeding.
AP_STREAM = stream(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 60 20] /Resources << >> >>",
    b"0 0 0 rg 0 0 60 20 re f\n",
)

PAGES = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
PAGE_PLAIN = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> >>"
)


def page_with_annots(annots: bytes) -> bytes:
    return (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] "
        b"/Resources << >> /Annots " + annots + b" >>"
    )


# ---------------------------------------------------------------------------
# The ce-dimension `/PieceInfo` sidecar (ISO 32000-1 Section 14.5)
# ---------------------------------------------------------------------------
def sidecar(annot_ref: bytes, ap_ref: bytes) -> bytes:
    """The catalog `/PieceInfo /pdfcer /Private` sidecar, hand-written.

    Schema per `crates/pdfcer-core/src/dimension/sidecar.rs`: `/Version` 4,
    a `/Groups` array that MUST contain group `/Id 0` (`DEFAULT_GROUP_ID`, or
    `deserialize_model` answers `None` and the caller starts a fresh empty
    model, and the probe would then measure nothing at all), and a
    `/Dimensions` array whose records carry the two attacker-supplied object
    ids this generator exists to weaponise: `/Annot` and `/Ap`.

    ** THE ONLY VALIDATION THIS PASSES THROUGH IS A VERSION COMPARISON.**
    `EditSession::check_dimension_sidecar` reads `/Version` and refuses only a
    number GREATER than `SIDECAR_VERSION`. It does not resolve `/Annot`, does
    not resolve `/Ap`, and does not ask what either one points at. That is the
    whole of the claim for sites 1 and 2, restated as a fixture.
    """
    return (
        b"/PieceInfo << /pdfcer << /LastModified (D:20260831000000Z) /Private << "
        b"/Version 4 "
        b"/Groups [<< /Id 0 /Name (Default) /Scale /one /Unit (mm) "
        b"/Frac /decimal /Places 2 /Visible true >>] "
        b"/Dimensions [<< /Id 1 /Group 0 /Kind /linear /A [10 10] /B [110 10] "
        b"/Constraint /aligned /Annot " + annot_ref + b" /Ap " + ap_ref + b" >>] "
        b">> >> >>"
    )


CE_DIM_ANNOT = (
    b"<< /Type /Annot /Subtype /Line /IT /LineDimension /Rect [0 0 300 300] "
    b"/P 3 0 R /L [10 10 110 10] /F 4 >>"
)


# ---------------------------------------------------------------------------
# SITE 1 + SITE 2 -- the ce-dimension sidecar
# ---------------------------------------------------------------------------
def dim_sidecar_ap_names_pagetree() -> bytes:
    """Sites 1 AND 2: the ce-dimension sidecar's `/Ap` names the page-tree root.

    One file, two claims, because the two verbs read the SAME field and do
    opposite things to it:

      * `delete_dimension` puts `record.ap` straight into `removals` with
        `is_deleted: true`. Object 2 -- the `/Pages` node -- is DELETED.
      * `regenerate_dimension_writes` emits `ObjectWrite { id: ap_id, after:
        Some(Object::Stream(..)) }`, destructively OVERWRITING whatever that id
        holds. Object 2 becomes a form XObject. Reachable from
        `set_dimension_label`, `set_group_scale`, `set_dimension_group` and
        `delete_dimension_group_with(Reassign)` -- so **a label edit can
        destroy the page tree**, which is the part of the audit's claim worth
        measuring rather than reasoning about.

    The ce dimension itself is otherwise entirely well-formed: a real `/Line`
    with `/IT /LineDimension` on a real page, listed in that page's `/Annots`,
    with a `/P` that names the page. Only `/Ap` lies.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R " + sidecar(b"4 0 R", b"2 0 R") + b" >>",
            PAGES,
            page_with_annots(b"[4 0 R]"),
            CE_DIM_ANNOT,
        ]
    )


def dim_sidecar_annot_names_pagetree() -> bytes:
    """Site 1, second arm: the sidecar's `/Annot` names the page-tree root.

    `delete_dimension` resolves `record.annot`, reads `/P` off it to find the
    page, and then deletes the object. So the trap needs a `/P` -- which is why
    object 2 carries one here, alongside its `/Kids` and `/Count`. A
    page-tree node with a stray `/P` is malformed, and nothing in the verb
    looks at `/Type` to notice.

    Kept separate from the `/Ap` file because the two fields are read at
    different points and a fix applied to one and not the other is a shape this
    project has shipped before (see this generator's sibling on `/N`'s
    reference-versus-inline branches).
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R " + sidecar(b"2 0 R", b"4 0 R") + b" >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /P 3 0 R >>",
            page_with_annots(b"[2 0 R]"),
            AP_STREAM,
        ]
    )


def dim_sidecar_control() -> bytes:
    """CONTROL for sites 1 and 2: an ordinary, well-formed ce dimension.

    `/Annot` names the `/Line` annotation and `/Ap` names a real form XObject
    nothing else reaches. Both verbs must keep working:

      * `delete_dimension` must return `Ok`, free objects 4 and 5, drop 4 from
        the page's `/Annots`, and leave the page tree walking.
      * `set_dimension_label(Some(..))` must return `Ok` with `changed: true`,
        rewrite object 5's stream, and leave the page tree walking.

    A fix that refuses on any `/Ap` it cannot vouch for -- or that stops
    regenerating appearances at all -- fails here while passing both hostile
    files, which is exactly the false green this control exists to prevent.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R " + sidecar(b"4 0 R", b"5 0 R") + b" >>",
            PAGES,
            page_with_annots(b"[4 0 R]"),
            b"<< /Type /Annot /Subtype /Line /IT /LineDimension /Rect [0 0 300 300] "
            b"/P 3 0 R /L [10 10 110 10] /F 4 /AP << /N 5 0 R >> >>",
            AP_STREAM,
        ]
    )


# ---------------------------------------------------------------------------
# SITE 3 -- `delete_redaction_mark`
# ---------------------------------------------------------------------------
def redact_ap_names_pagetree() -> bytes:
    """Site 3, first arm: a `/Redact` mark whose `/AP` `/N` names the page-tree root.

    `delete_redaction_mark` resolves `/AP` then takes `/N`'s reference with no
    type test, and pushes it into `removals`. Its own doc comment asserts the
    safety that is missing: *"the `/AP` too, because a redaction mark's
    appearance stream is authored by `add_redaction` SOLELY for that mark and
    is referenced by nothing else"*. That is true of marks pdfcer authored and
    says nothing about a mark that arrived in the file, which is every mark the
    verb can actually be handed -- `redact::redaction_marks` accepts any
    `/Annots` entry with `/Subtype /Redact`, and both halves of that test are
    attacker-writable.

    ISO 32000-1 Section 12.5.5 settles what `/N` may be: *"Each appearance
    stream is a form XObject"*. A `/Pages` node is not one.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            PAGES,
            page_with_annots(b"[4 0 R]"),
            b"<< /Type /Annot /Subtype /Redact /Rect [20 20 200 60] "
            b"/AP << /N 2 0 R >> >>",
        ]
    )


def redact_mark_is_pagetree_node() -> bytes:
    """Site 3, second arm: `annot_id` ITSELF is the page-tree root.

    The sharper half of the claim, and the one that distinguishes this verb
    from `delete_annotation`. `annotation_deletion_guards` -- which calls
    `refuse_if_in_page_tree`, checks the catalog and the `/AcroForm`, and
    rejects a `/Type` of `/Catalog`, `/Pages` or `/Page` -- is on the GENERAL
    path only. `delete_redaction_mark` is a `pub` verb reachable directly, and
    it runs none of them.

    Object 2 is the page-tree root wearing `/Subtype /Redact`, listed in page
    3's `/Annots`. `pages_in` still walks it (nothing about `/Subtype`
    disturbs the `/Kids` traversal), so `redaction_marks` finds it, and the
    verb is then handed the id of the object the whole document hangs from.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Subtype /Redact >>",
            page_with_annots(b"[2 0 R]"),
        ]
    )


def redact_control() -> bytes:
    """CONTROL for site 3: an ordinary unapplied `/Redact` mark.

    Object 4 is a real mark with a real, solely-owned appearance stream at
    object 5. The verb exists to reject one member of a bulk
    `mark_redactions_by_search` batch without undoing the other thirty-nine,
    so it must return `Ok`, drop 4 from `/Annots`, free BOTH 4 and 5, and leave
    the page tree walking. A guard that admitted no `/AP` would orphan a stream
    in every subsequent save.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            PAGES,
            page_with_annots(b"[4 0 R]"),
            b"<< /Type /Annot /Subtype /Redact /Rect [20 20 200 60] "
            b"/AP << /N 5 0 R >> >>",
            AP_STREAM,
        ]
    )


# ---------------------------------------------------------------------------
# SITE 4 -- `flatten_fields`
# ---------------------------------------------------------------------------
def flatten_field_is_page() -> bytes:
    """Site 4: `Pass 185.1`'s LITERAL input, aimed at the verb that never got the fix.

    `/AcroForm /Fields` names object 3, which is also the `/Page`. That is the
    exact document `refuse_if_in_page_tree` was written for -- and
    `flatten_fields` does not call it, or any other structural check.
    `forms::parse_acroform` models object 3 as a field, correctly: the form
    dictionary says it is one, and Section 12.7.3 states no rule that a field
    may not also be something else. `flatten_fields` then ends its per-field
    loop with an unconditional `delete_ids.push(field.id)`.

    ** THE TRAP HERE IS A `/Page`, NOT THE `/Pages` ROOT**, breaking this
    generator's own convention on purpose: the point of this file is that
    `185.1`'s input still works somewhere, so it must BE `185.1`'s input. The
    field carries `/FT /Tx` and a `/T` so it is a well-formed field, and the
    page carries `/Parent`, `/MediaBox` and `/Resources` so it is a well-formed
    page. Both readings are correct, which is why refusal rather than repair is
    the answer `185.1` settled on.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [3 0 R] >> >>",
            PAGES,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> "
            b"/FT /Tx /T (trap) /V (collateral) >>",
        ]
    )


def flatten_control() -> bytes:
    """CONTROL for site 4: an ordinary merged widget/field that must still flatten.

    Object 4 is the Section 12.7.3.1 merged shape -- one dictionary that is both
    the field and its only widget -- with a real `/AP` `/N` form XObject at
    object 5. Flattening must return `Ok` with one field and one widget burned,
    append the burn stream to the page's `/Contents`, register object 5 in the
    page `/Resources /XObject`, drop 4 from `/Annots` and from `/AcroForm
    /Fields`, free object 4, KEEP object 5 (it is a page resource now), and
    leave the page tree walking.

    The widest control in the set, because `flatten_fields` is the widest verb:
    a fix that refuses any field it cannot prove is not structural must still
    let this one through untouched.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            PAGES,
            page_with_annots(b"[4 0 R]"),
            b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (hello) "
            b"/Rect [20 20 80 40] /P 3 0 R /F 4 /AP << /N 5 0 R >> >>",
            AP_STREAM,
        ]
    )


# ---------------------------------------------------------------------------
# SITE 5 -- `outline_subtree` / `delete_outline_item`
# ---------------------------------------------------------------------------
def outline_first_names_pagetree() -> bytes:
    """Site 5, first arm: an outline item whose `/First` names the page-tree root.

    `outline_subtree` walks `/First` and `/Next` unconditionally -- its own doc
    comment says so -- and pushes EVERY dictionary it reaches onto the removal
    list. It is depth- and cycle-guarded (Section 10 hostile-file discipline)
    but not TYPE-guarded, and the two are different properties: a bounded walk
    that reaches the wrong object still deletes the wrong object, it just
    terminates while doing it.

    Object 5 is a legitimate bookmark under a legitimate `/Outlines` root. Only
    its `/First` lies, naming object 2. Section 12.3.3 Table 153 defines
    `/First` as *"the first of this item's immediate children in the outline
    hierarchy"* -- an outline item, never a page-tree node.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
            PAGES,
            PAGE_PLAIN,
            b"<< /Type /Outlines /First 5 0 R /Last 5 0 R /Count 1 >>",
            b"<< /Title (Chapter One) /Parent 4 0 R /First 2 0 R /Last 2 0 R /Count 1 >>",
        ]
    )


def outline_item_is_page() -> bytes:
    """Site 5, second arm: the entry gate accepts a `/Page`, because a `/Page` has `/Parent`.

    `delete_outline_item` refuses exactly one thing before it starts deleting:
    an object with NO `/Parent`, on the grounds that it must then be the
    outline root (Table 152) and deleting the whole outline is a different act.
    Every `/Page` in every conforming document carries `/Parent` (Section 7.7.3.3
    Table 30, required), so every page satisfies that gate.

    ** THE TRAP HERE IS THE `/Page`, NOT THE `/Pages` ROOT**, and deliberately:
    the claim is about what the gate ADMITS, and it is `/Parent` that admits
    it. A `/Pages` node would be admitted too, but only the page makes the
    reason legible.

    Note this document has NO OUTLINE AT ALL. Nothing in the verb requires one
    -- it takes a bare `ObjId` from the caller and starts reading keys -- which
    is itself part of the finding.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            PAGES,
            PAGE_PLAIN,
        ]
    )


def outline_control() -> bytes:
    """CONTROL for site 5: two sibling bookmarks; deleting the first must work.

    Object 5 has a `/Next` and object 6 has a `/Prev`, so the verb must relink
    them around the hole, decrement the root's `/Count`, free object 5, return
    `Ok(1)`, and leave the page tree walking. A fix that refuses any item whose
    links it cannot vouch for fails here -- and bookmark deletion is a verb
    operators reach constantly, unlike the hostile shapes above.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
            PAGES,
            PAGE_PLAIN,
            b"<< /Type /Outlines /First 5 0 R /Last 6 0 R /Count 2 >>",
            b"<< /Title (First) /Parent 4 0 R /Next 6 0 R >>",
            b"<< /Title (Second) /Parent 4 0 R /Prev 5 0 R >>",
        ]
    )


# ---------------------------------------------------------------------------
# SITE 6 -- `plan_annotation_deletion` cascade 1
# ---------------------------------------------------------------------------
def popup_is_pagetree_node() -> bytes:
    """Site 6: the `/Popup` id joins `removing` AFTER the guards have run.

    `annotation_deletion_guards` is thorough and runs on exactly ONE id: the
    one the caller named. It calls `refuse_if_in_page_tree(&[annot_id])`,
    checks the catalog and `/AcroForm`, and rejects a `/Type` of `/Catalog`,
    `/Pages` or `/Page`. Then `plan_annotation_deletion` -- a pure associated
    function with no `&self`, so it CANNOT re-run any of that -- appends the
    `/Popup` target to `removing`, and `removing` is what gets deleted.

    The cascade is guarded, just not against this. It requires the popup to be
    a real entry on this document's `/Annots` with `is_popup` true, so object 2
    is the page-tree root carrying `/Subtype /Popup` and listed in page 3's
    `/Annots` alongside the `/Text` note that names it. Section 12.5.6.14 makes
    `/Popup` *"an indirect reference to a pop-up annotation"*; nothing checks.

    The verb called is `delete_annotation(4)` -- deleting an ordinary sticky
    note, the single most common annotation gesture there is.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Subtype /Popup >>",
            page_with_annots(b"[4 0 R 2 0 R]"),
            b"<< /Type /Annot /Subtype /Text /Rect [20 20 40 40] /Contents (note) "
            b"/Popup 2 0 R >>",
        ]
    )


def popup_control() -> bytes:
    """CONTROL for site 6: a real pop-up that MUST go with its parent.

    Section 12.5.6.14 says a pop-up *"shall not appear alone"*, and Section
    12.5.6.2 NOTE 2 gives the sharper reason: an orphaned pop-up starts
    displaying its own `/Contents`, so a copy of the comment just deleted
    reappears on the page. Deleting object 4 must therefore also delete object
    5, report `popup_removed`, and leave the page tree walking.

    This is the control that stops the fix from being "never follow `/Popup`".
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            PAGES,
            page_with_annots(b"[4 0 R 5 0 R]"),
            b"<< /Type /Annot /Subtype /Text /Rect [20 20 40 40] /Contents (note) "
            b"/Popup 5 0 R >>",
            b"<< /Type /Annot /Subtype /Popup /Rect [60 20 200 90] /Parent 4 0 R >>",
        ]
    )


# ---------------------------------------------------------------------------
# SITE 7 -- `detach_file`
# ---------------------------------------------------------------------------
def attach_namevalue_is_pagetree() -> bytes:
    """Site 7, first arm: the `/EmbeddedFiles` name-tree VALUE names the page-tree root.

    `detach_file` splits the flat `[key value key value ...]` array, and for the
    matched entry does `if let Object::Reference(spec_id) = victim { doomed.push(spec_id); ... }`.
    No resolution, no type test -- the reference is doomed on the strength of
    the array position alone.

    Section 7.7.4 Table 31 makes `/EmbeddedFiles` *"a name tree mapping name
    strings to file specifications"*. Object 2 is the `/Pages` node. Nothing in
    the verb asks.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles "
            b"<< /Names [(trap) 2 0 R] >> >> >>",
            PAGES,
            PAGE_PLAIN,
        ]
    )


def attach_ef_is_pagetree() -> bytes:
    """Site 7, second arm: a well-formed filespec whose `/EF` `/F` names the page-tree root.

    One hop further in than the file above, and the more realistic shape: the
    name-tree value IS a `/Type /Filespec` dictionary, so anything that
    validated only the top level would pass it. The `/EF` sub-dictionary is
    then read with `if let Some(Object::Reference(id)) = ef.get(k)` and the id
    pushed to `doomed`.

    Section 7.11.4 Table 45 defines `/EF` as *"a dictionary containing a
    subset of the keys F, UF, DOS, Mac and Unix, corresponding to the entries
    by those names in the file specification dictionary. The value of each
    such key shall be an embedded file stream."* A stream, by `shall`. Object 2
    is a dictionary.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles "
            b"<< /Names [(trap) 4 0 R] >> >> >>",
            PAGES,
            PAGE_PLAIN,
            b"<< /Type /Filespec /F (trap.txt) /UF (trap.txt) /EF << /F 2 0 R >> >>",
        ]
    )


def attach_control() -> bytes:
    """CONTROL for site 7: an ordinary attachment that must still detach.

    A conforming filespec with `/F` and `/UF` both naming ONE embedded file
    stream -- which is what this crate's own writer emits, and the reason
    `detach_file` dedups before freeing. Detaching must return `Ok`, drop the
    pair from the name tree, free objects 4 and 5 exactly once each, and leave
    the page tree walking.
    """
    return assemble(
        [
            b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles "
            b"<< /Names [(note.txt) 4 0 R] >> >> >>",
            PAGES,
            PAGE_PLAIN,
            b"<< /Type /Filespec /F (note.txt) /UF (note.txt) /EF << /F 5 0 R /UF 5 0 R >> >>",
            stream(
                b"<< /Type /EmbeddedFile /Subtype /text#2Fplain >>",
                b"collateral probe attachment\n",
            ),
        ]
    )


# ---------------------------------------------------------------------------
# SITE 8 -- `unembed_fonts`'s `/CIDSet`
# ---------------------------------------------------------------------------
DESC_METRICS = (
    b"/Flags 32 /FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 "
    b"/Descent -200 /CapHeight 700 /StemV 80"
)
SIMPLE_FONT_TAIL = (
    b"/FirstChar 65 /LastChar 67 /Widths [600 600 600] /Encoding /WinAnsiEncoding"
)


def _cidset_doc(cidset_ref: bytes, extra: list[bytes]) -> list[bytes]:
    """The shared body of both `/CIDSet` files -- one removable embedded font.

    Deliberately a REAL font: object 7 is the repository's own synthetic sfnt
    donor, wrapped as a `/FontFile2` with a correct `/Length1`. That is not
    decoration. The `/FontFile*` sibling IS type-guarded (`fontinfo.rs`:
    `let Object::Stream(stream) = resolved else { return NotAStream }`), so a
    font whose program is not a real stream never becomes a removable target
    and `/CIDSet` is never reached. Site 8 only exists if a genuinely
    removable font can be presented ALONGSIDE the poisoned key.

    `/CIDSet` on a SIMPLE font is malformed -- Section 9.8.1 Table 124 puts it
    on a CIDFont descriptor -- and is here on purpose, exactly as
    `gen-unembed-fixtures.py` explains for its own `unembed-charset-cidset.pdf`:
    phase A never classifies a composite font `Removable`, so a conforming
    document cannot present "removable font whose descriptor carries
    `/CIDSet`", and the handling exists anyway.
    """
    program = DONOR.read_bytes()
    return [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        PAGES,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] "
        b"/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        stream(b"<< >>", b"BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n"),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /FFFFFF+pdfceCoverage "
        + SIMPLE_FONT_TAIL + b" /FontDescriptor 6 0 R >>",
        b"<< /Type /FontDescriptor /FontName /FFFFFF+pdfceCoverage " + DESC_METRICS
        + b" /CharSet (/A/B/C) /CIDSet " + cidset_ref + b" /FontFile2 7 0 R >>",
        stream(b"<< /Length1 %d >>" % len(program), program),
    ] + extra


def cidset_names_pagetree() -> bytes:
    """Site 8: `/FontDescriptor /CIDSet` names the page-tree root.

    `font_unembed`'s resolver does `out.cid_set_id = cid_set.as_reference()`
    with no type test, and `unembed_fonts` then chains that id into the freed
    set beside the font program. Section 9.7.4.2 Table 117 defines `/CIDSet` as
    *"a stream identifying which CIDs are present in the CIDFont file"* -- a
    stream, and one describing the program being removed.

    The contrast with its `/FontFile*` sibling is the whole point of this
    fixture. Both keys are read off the same descriptor, in the same function,
    ten lines apart; one is guarded and one is not. There is no design reason
    for the asymmetry, which is what makes it a defect rather than a trade-off.
    """
    return assemble(_cidset_doc(b"2 0 R", []))


def cidset_control() -> bytes:
    """CONTROL for site 8: a real `/CIDSet` stream that MUST be freed with the program.

    Object 8 is a genuine Section 9.7.4.2 CID-set stream (bits indexed by CID,
    high-order bit first; three glyphs). A descriptor carrying `/CIDSet` is
    asserting *"this used to be a subset"*, which is precisely the false claim
    unembedding must not leave behind -- so unembedding must return `Ok`, strip
    `/FontFile2`, `/CharSet` and `/CIDSet` from the descriptor, free BOTH
    object 7 and object 8, and leave the page tree walking.
    """
    return assemble(_cidset_doc(b"8 0 R", [stream(b"<< >>", bytes([0b11100000]))]))


# ---------------------------------------------------------------------------
def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    files = {
        # site 1 + 2
        "dim-sidecar-ap-names-pagetree.pdf": dim_sidecar_ap_names_pagetree(),
        "dim-sidecar-annot-names-pagetree.pdf": dim_sidecar_annot_names_pagetree(),
        "dim-sidecar-control.pdf": dim_sidecar_control(),
        # site 3
        "redact-ap-names-pagetree.pdf": redact_ap_names_pagetree(),
        "redact-mark-is-pagetree-node.pdf": redact_mark_is_pagetree_node(),
        "redact-control.pdf": redact_control(),
        # site 4
        "flatten-field-is-page.pdf": flatten_field_is_page(),
        "flatten-control.pdf": flatten_control(),
        # site 5
        "outline-first-names-pagetree.pdf": outline_first_names_pagetree(),
        "outline-item-is-page.pdf": outline_item_is_page(),
        "outline-control.pdf": outline_control(),
        # site 6
        "popup-is-pagetree-node.pdf": popup_is_pagetree_node(),
        "popup-control.pdf": popup_control(),
        # site 7
        "attach-namevalue-is-pagetree.pdf": attach_namevalue_is_pagetree(),
        "attach-ef-is-pagetree.pdf": attach_ef_is_pagetree(),
        "attach-control.pdf": attach_control(),
        # site 8
        "cidset-names-pagetree.pdf": cidset_names_pagetree(),
        "cidset-control.pdf": cidset_control(),
    }
    for name, data in files.items():
        (OUT / name).write_bytes(data)
        print(f"  {name:40s} {len(data):7d} bytes")
    print(f"{len(files)} file(s) -> {OUT}")


if __name__ == "__main__":
    main()
