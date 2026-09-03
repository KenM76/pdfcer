---
name: project-pass-136-form-recursion-and-filing-count
description: Pass 136.0 (83fca59) + 136.1 (f62df4e) + 136.2 (a2f7b48), 2026-08-27, 282nd filing — objects inside a form XObject are now selectable AND reachable from pdfce-cli object-list via a new leaf line type, never merged into object rows because a leaf's token range indexes the form's stream, not the page's.
metadata:
  type: project
---

**Pass 136.0** (`83fca59`) — `decompose_page` now descends into form
XObjects; leaves live on a **new, separate** `PageObjects::leaves`, never
merged into the existing flat `objects` list. **Pass 136.1** (`f62df4e`) —
`hit_test_point_deep` + `HitTarget::{Object,Leaf}` do the actual
selection; `hit_test_point`/`hit_test_point_all` are UNCHANGED on purpose.
**Pass 136.2** (`a2f7b48`, 282nd filing) — `pdfce-cli object-list` gains a
new `leaf` line type (kind/bbox/containment/paint_order/editable) plus
appended summary keys `leaves`/`form_cycles`/`form_depth_overflows`. This
discharges the "filed as owed" item recorded in `Pass 136.1`'s own Shipped
entry.

**Why the separate-list design matters, and why it's a reusable shape
(now proven twice — core model AND cli surface):** eleven `edit.rs` sites
resolve a paint-order index and write to the **page's** content stream; a
leaf's token range indexes the **form's** stream. Merging the lists (or
printing leaves as `object` rows on the CLI) would let those sites/scripts
apply a form-relative range to the page and corrupt it silently, in
bounds, nothing to catch it. Keeping leaves a genuinely separate list —
separate `PageObjects` field in core, separate `leaf` line type on the
CLI, `editable=false` on every leaf row — makes both surfaces correct **by
construction**. Cite this forward whenever a new recursive/nested read
path is proposed alongside an existing flat index an editing surface
resolves against — ask "does anything treat this index as an offset into
a specific buffer?" at BOTH the core-model layer and any CLI/GUI surface
built on top of it later.

**Disclosure pattern worth reusing:** `form_cycles`/`form_depth_overflows`
are on `object-list`'s stable summary line (script-parseable) AND spelled
out in a human sentence naming what was skipped and why — because a
truncated leaf list is otherwise indistinguishable from a page that
simply has fewer objects. Same "silent truncation must announce itself"
shape as other disclosure findings this project has made.

**Also worth reusing:** grepping `text_extract` (which already recursed
into forms since Pass 1.1) before building the vector-side recursion
turned a large Pass into a small one — it already had the cycle guard,
depth bound, and the `ContentStreamRef`/`is_editable()` vocabulary the new
`FormLeaf` now reuses directly. Always check the sibling model
(text vs vector) for an already-solved recursion before rebuilding one.

**Depth constant deduplicated:** `pdfce-render::MAX_XOBJECT_DEPTH` and
`text_extract::max_form_depth` (docstring only *asserted* they matched) are
joined by `content::MAX_FORM_DEPTH = 64` (2x veraPDF's conformant 32-deep
chain). Retiring render's own copy is STILL FILED AS OWED after all three
sub-Passes — cross-crate breaking change, not yet done.

**FEATURES.md:** the Vector-objects Planned row now reads `[x]` core /
`[x]` cli / `[ ]` gui — cli ticked in the 282nd filing once `object-list`
verifiably printed leaf lines against the `Pass 136.0` fixtures. gui
remains a genuine gap (row stays in Planned, not Implemented).

**A hazard recorded in this role's own memory recurred anyway, worth
flagging generally:** `Pass 136.2`'s own first commit-message draft was
passed through `git commit -m` from a shell and had every backticked term
command-substituted into an empty string. Written-down hazards are not
self-enforcing; if a future filing ever needs prose containing backticks/
backslashes committed to git, write it to a file first and use
`git commit -F <file>`, never `-m` from an interactive shell.

**Filing-count landmark:** SESSION_LOG filings reached **282** as of this
entry (2026-08-27, `Pass 136.2`). Ledger unchanged across all three
sub-Passes: rules `R218` (next free `R219`), decisions `089` (next free
`090`) — per the engineer, not independently re-run (no shell available to
this role any of these filings).
