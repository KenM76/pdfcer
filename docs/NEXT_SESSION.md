# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-09, after `Pass 285.0`.

---

## STATE

Workspace version `0.49.0`. **Last release is still `v0.45.0`** (2026-09-07) —
everything since is pushed but unreleased. Releasing is standing-authorized
(decision 121); nobody has needed it yet, and the consuming project reads
`docs/core-api/` from the repo rather than from a tarball. **If you have budget
for a release, cutting one is overdue rather than forbidden.**

**`main` is pushed through `a2adb54`.** `tools/run-gates.sh` **PASS, 29/29**,
including both filing gates. `cargo test --workspace` green.

**Nine Passes shipped today**, each closing an inbound request, an operator
report, or a defect found while shipping one of the others:

| Pass | commit | what |
|---|---|---|
| `277.0` | `fccd6cd` | a sticky refused a resize by claiming pdfcer had not drawn it |
| `278.0` | `c8a6697` | freehand `/Ink` strokes became editable, per point and per stroke |
| `279.0` | `5b8ec61` | the font refusal named a face that led in a circle |
| `280.0` | `26ef381` | `run_repertoire` — the alphabet, asked before the first keystroke |
| `281.0` | `1177221` | hybrid-reference files are rewritten, so redaction reaches them |
| `282.0` | — | the `/Info` half of the redaction-diligence gap |
| `283.0` | `dce2223` | **a PDF with errors OPENS** — decision 145, rule `R248` |
| `283.1` | `d8fcb68` | that intervention, reachable from a **path** |
| `284.0` | `ea4acb3` | **redaction sweeps the FILE, not the graph** — decision 146, rule `R249` |
| `285.0` | `1366138` | an abandoned content stream's drawn text is blanked |

Filings: 485th `6b10e92`, 486th `5bf8704`, 487th `a2adb54`.

---

## ★★ TWO POSTURE CHANGES LANDED TODAY. READ BOTH BEFORE TOUCHING THEIR AREAS

### Decision 145 — a damaged file OPENS (reader)

`Pass 283.0` turned two mid-session operator rulings into a posture change for
the whole reader:

> *"We should be making pdfcer so that it opens pdfs that have errors, and have
> a way that it manages those errors such that they aren't fatal, and if the
> user can intervene in a decision that should always be an option along with
> them not having to intervene."*
>
> *"We should be doing this for all defects where it is possible to continue and
> open the file."*

**Fail-clean never meant refuse** — it meant never silently do the wrong thing,
and a counted, disclosed, overridable decision is not silent. Six malformation
classes now open instead of costing the document; every decision is recorded
with what it chose *between*; `--on-malformed` and `LoadOptions` take the other
one.

★ **The next reader defect you meet is governed by this, not by taste.** The
question is no longer *"is this file conforming?"* but *"can pdfcer continue
without inventing anything?"* — and §7.3.10's undefined-object rule is usually
the answer. The one line that stays fatal is a file with no `/Root`, because
continuing there would mean fabricating a catalog; a test pins it.

**Where to look next:** anywhere the reader still returns a hard error for a
*part* of a file — `pages()`, the font loaders, the content interpreter, the
annotation walkers.

### Decision 146 and `R249` — evidence, never reachability (writer/redaction)

`Pass 284.0` found that every redaction carrier located its target by
**navigating the document graph** while the writer emits objects by
**enumerating the cross-reference table**. Everything in the difference was
copied through verbatim — while `info action=scrubbed` and
`prior_revisions action=dropped_by_rewrite` were both **true**. No false line,
content still present.

**`R249`, and it generalises well past redaction:** *before scoping any
destructive sweep to satisfy an outcome-shaped obligation ("remove all X"),
scope it to the evidence the obligation itself names. Do not substitute a
computed reachability or liveness walk as a proxy* — such a walk on a
graph-shaped format **drops content silently rather than refusing loudly**
(§7.3.10 makes a dangling reference *"not … an error"*).

★ **The warrant is empirical.** The census probe built to *measure* the fix
reproduced the exact failure it was written to catch — **twice** — and my
reported impact figure moved 21% → 12% as a result. Three object classes are
unreferenced **by design**: object streams (type-2 xref entries),
cross-reference streams (byte offset), and the linearization dictionary
(Annex F.3.3).

---

## ★★★ THE THREE HABITS THIS SESSION KEPT PROVING, IN ORDER OF WHAT THEY COST

### 1. Grep the CONSUMING project before calling a Pass done (`283.1`)

`Pass 283.0` was complete, correct, tested and documented — and its
intervention was **unreachable by the only shell that needed it**. The
alternative reading shipped on `Document::from_bytes_with_options`;
`pdfcer-gui` opens files with `Document::load`.

★ `R245`'s shape applied to an **affordance** rather than a **guard** — harder
to see, because *nothing is wrong on the route you are reading*. Do not ask
"does the API have a way?" Ask **"can the intended caller reach it without
duplicating the function beside it?"**

### 2. A sabotage is only as discriminating as its fixture (`285.0` — the near-miss)

Blanking the **whole buffer** instead of only the show operators' operand spans
**left all eight tests green**. The fixture had put the redacted word only
inside a string.

★★ **The defect that would have shipped:** `q /CONFIDENTIALIm Do Q` rewritten
to `q /XXXXXXXXXXXXIm Do Q` — a resource name resolving to nothing, an image
silently not drawn. **Content destroyed to fix a leak, under a doc comment
that explicitly promised the opposite.**

★ Recorded as `R225`'s **16th** instance, with the severity escalation stated:
every prior instance was *"the test measured less than its NAME claimed"*; this
was *"less than the DOCUMENTATION claimed"*, which is strictly worse — a doc
comment is what a future reader trusts **instead of** re-deriving.

Third instance this session, after `278.0`'s last-ink-stroke and `279.0`'s
`refusal_names_a_font.rs`. **Ask of any test you inherit: which fixture could
ever have made this go red?**

### 3. Dispatch the spec librarian BEFORE reasoning from a clause, not after

`Pass 283.0`'s first draft justified keep-last from §7.5.6's incremental-update
ordering — an analogy in costume; §7.3.7's own preceding sentence says entry
order *"shall be ignored"*. `Pass 284.0`'s answer arrived with the `UO-A1`
reachability trap, three unfiled carriers (thread `/I` dictionaries, XMP routes
B and C) and the finding that `carrier_xmp` reads **route A of four**. Neither
would have come from reading the code.

---

## ★ THE QUEUE

★ **The item that headed this queue all session — the orphaned metadata object
surviving redaction — is CLOSED** (`Pass 284.0` + `285.0`). It was one instance
of a class; the class is closed too. What remains of it is the sweep's existing
floor, not a new gap: **a stream that does not parse as a content stream**, and
**text drawn through a subset font whose operand bytes are glyph codes rather
than characters**. Closing the second needs the glyph machinery the *live*
content path already uses, and it is the same floor `redacted_text` has — so it
belongs with item 1 below, not on its own.

**Both remaining items are from `pdfcer-gui`. Take them in this order:**

1. **`request_redacted_text_carries_single_characters_on_a_per_glyph_producer_so_the_absence_proof_is_blind.md`**
   — CONFIRMED at the source and replied to; not built. `redacted_text` is
   accumulated **per show operator**, so a per-glyph producer yields single
   characters and their absence proof greps for the alphabet.

   ★ **It is the same bug as the `carrier_info` gap was**: that field has a
   second consumer inside the engine, and the two want opposite granularities —
   joining runs (what they asked for) makes metadata under-matching *worse*.
   Ship both halves in one Pass: per-mark joined text, plus a match rule that
   does not depend on granularity, plus the granularity stated in the report.

   ★★ **And it now has a third consumer**: `residual_sweep`'s
   `redaction_evidence` (`Pass 284.0`). Whatever granularity you choose, check
   it against the sweep as well — a change that helps the absence proof and
   quietly narrows the sweep would re-open a leak this session just closed.
   **That is the thing to be careful about in this Pass.**

2. **`request_resize_annotation_refuses_a_pdfcer_authored_stamp_as_foreign.md`**
   — **amended twice and RE-ESCALATED**, in the operator's words: *"if I drew
   the stamp too small for the text to fit, resizing just stretches the entire
   object … I should be able to … edit just the box size without affecting the
   text."* Their shell shipped a stopgap (uniform carry), which is why he can
   resize at all and why he hit its ceiling within the hour.

   ★★ **MEASURED, and it changes the design:** a stamp's label size is
   `(h * 0.42).clamp(8.0, 28.0)` — **derived from the box height, stored
   nowhere.** So the obvious fix ("re-bake like `Pass 276.0` did for
   `/FreeText`") recomputes the size from the new height and **scales the text
   with the box**, which is exactly what he is trying to escape. The size must
   first become something a re-bake can *keep*: recover it from the baked `/AP`
   (the `Pass 276.0` both-ways byte-comparison trick, which is how `/FreeText`
   recovers `multiline`) **and** add an explicit `StampStyle` so it is settable.
   Recovery alone leaves him unable to change it; a stored property alone
   silently breaks every stamp already in a document.

   ★ Look for a **fourth** authoring family while you are there — three found
   one at a time is `R245` at n=3, and the enumeration is the defect.

### After those, the operator's own ordered plan (2026-09-06) is still untouched

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).


## OWED

- **`R221`'s recorded instance count is wrong and a commit message made it
  worse.** `Pass 279.0` says "third recorded instance"; the Standing Rules entry
  is already past three, and the 480th filing flagged the discrepancy rather
  than guessing. Reconcile it in a session with budget. **Do not copy an ordinal
  from a commit message.**
- **★★ `R247` is reserved-but-unclaimed and now FLANKED BY THREE minted
  neighbours** — `R246`, `R248`, `R249` — having been flagged for three
  consecutive filings. Every mint since has been numbered deliberately *past*
  it to avoid entangling with the unresolved reservation, which works and
  compounds. **Resolve it before a fourth candidate lands**; it needs a session
  with time, not a drive-by.
- **`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on private functions. Staged fix, its own change.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded**
  (55.3 %). Pre-existing; `LEGAL.md` §5 makes it a licensing statement, not
  tidiness.
- **Backup bundle is well over 150 commits behind `HEAD`.**

---

## ★★ WHAT THIS SESSION GOT WRONG — the rest of it

The three that generalise are above, under **THE THREE HABITS**. These are
the remainder, kept because each is a concrete instance a future session can
recognise.

### A correct test, on a fixture that could not fail. FOUR times.

The fourth (`Pass 285.0`, the whole-buffer blank) is above and is the worst
of them — it broke a promise the DOC COMMENT made, not merely one the test
name made. The other three:

- `Pass 277.0`: *"a refusal writes nothing"* measured on a sticky stayed green
  with the guard moved after the write — a sticky never reaches the write
  anyway, because the refusal it used to get also returned early. It measured
  *"some refusal returns early"* under a name claiming *"this guard does."*
- `Pass 278.0`: a sabotage survived because the test removed the **last** ink
  stroke, where the naive code answers `0` by accident.
- `Pass 279.0`: `refusal_names_a_font.rs` ran the complete loop, green, and was
  **structurally incapable** of catching the shadowing bug — its fixture's font
  is `AAAAAA+pdfcerSymbolicPrivate` and no standard-14 name matches that stem.

**The question to ask of any test you inherit: which fixture could ever have
made this go red?** All four were found by sabotage; none by reading.

### An enumerating gate caught a NEW route within the hour — that is the contrast

`route_enumeration.rs` scans for every function that locates a text anchor and
demands it resolve the find. `Pass 280.0`'s new verb was a fourth such route and
the gate named it by function, before any consumer saw it. **A fixture-bound
test cannot catch a new case; an enumerating gate catches a new route.** Prefer
the latter when a family keeps growing. `check-core-api-verbs.py` did the same
job for `283.1` — it went red on a stale line count in `index.md`.

### A citation that was an analogy in costume

`Pass 283.0`'s first draft justified keep-last from §7.5.6's incremental-update
ordering. The spec librarian killed it: §7.3.7's own preceding sentence says a
dictionary's entry order *"shall be ignored"*, and ISO's one resolved
duplicate-key erratum picks its winner by **content**, rejecting positional
logic by name. **Dispatch the librarian before citing a clause you are reasoning
from, not after.** The real support was observed reader behaviour (qpdf, pdf.js,
pdfium all keep-last).

### Smaller, and all recurrences

- **I told another project they had not answered — 67 minutes after they had.**
  Second instance in two days. ⇒ `stat` the inbound directory immediately before
  writing **any** reply, not only at session start.
- **A producer misattribution inherited from a request sentence** — the hybrid
  file is Excel-365-exported, not SolidWorks, and the measurement was already in
  `C:\personal_rag\pdf\`. **Grep the corpus before repeating a fact someone else
  asserted.**
- **The doc-comment splice orphaned a doc block again.** Anchor on the DOC
  BLOCK, not the item.
- **A rustdoc example did not compile** (`Document::load` takes `&Path`). Only
  the doctest pass reads an example as code.
- **Prose through the Bash tool broke repeatedly, and once it reached a pushed
  commit.** Heredoc backticks command-substituted; `\` inside single-quoted
  python strings; **a multi-line `str.replace` that silently matched zero times
  because the file is CRLF and my pattern was LF**; and — the one that got
  away — **`git commit -m "…"` with backticked code in the message ate two
  fragments**, including the worked example that was the whole point of the
  paragraph. It was already pushed, and rewriting published history is not
  authorised, so it stands as written.
  ⇒ **NEVER pass a commit message inline. Always `git commit -F <file>`, with
  the file written by the Write tool.** Every other message this session did
  exactly that and survived; the one that did not is the one that lost content.
  The archive escaped only because `ROADMAP.md` and `03-capabilities.md`
  carried the same example independently — redundancy did the work that
  discipline should have.

---

## Standing habits (unchanged, and all of them earned their place again today)

- Check BOTH FeatureRequests channels **by diff**, at session start **and before
  every reply**.
- Write a reply for every request you close; correct a reply that turns out
  false, on the same channel, promptly.
- **Grep the consuming project for the call site before calling a Pass done.**
- Sabotage every new test — and check what the sabotage **fell through to**.
- When a sabotage survives, ask whether the FIXTURE could ever have failed
  before you conclude the code is fine.
- Register any new report struct in `check-outcome-disclosed`'s
  `OUTCOME_STRUCTS` in the SAME commit; the gate is opt-in and prints "clean"
  about what it was not told.
- Update `docs/core-api/` in the same Pass that changes a `pub` item, and bump
  **every** stated count (verbs, `EditError` variants, per-file line/clause
  figures in `index.md`).
- A filing commit never carries code.

---

## BUILD ENVIRONMENT

`target/` was **187 GB** at the start of the previous session on a disk at 96 %
full; `rm -rf target/debug/incremental` reclaimed **24 GB**. `du -sh target/`
every session — the same 24–26 GB rebuilds each time. Both checks must be run
before any delete: `git ls-files target` returns 0 and `git check-ignore -q
target` passes.

**Foreground survives where background dies.** `tools/run-gates.sh` takes well
over ten minutes; run it in the background and poll, but run the expensive
`cargo test --workspace` in the **foreground** with `--test-threads=2`.

★ **`run-gates.sh` piped into `tail` buffers everything** — the output file
stays 0 bytes until the run ends, which looks exactly like a hung sweep.
Redirect to a file and tail *that*, or just wait for the completion
notification.
