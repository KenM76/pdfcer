# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-10, after `Pass 288.0`.

---

## STATE

Workspace version `0.50.0`. **`v0.50.0` is RELEASED** (2026-09-10) — tagged,
both channels, `verify-release.py` clean on every check, fresh-folder smoke
test run. OneDrive slot `pdfcer1`, with `0.49.0` preserved in `pdfcer2`.

★ **The previous handoff said "last release is `v0.45.0`" and that was stale by
four versions** — `v0.49.0` had shipped two days earlier. **Nothing checks this
file's facts.** If you carry a release number forward, verify it with
`gh release list` first; it costs one command and it was wrong for a day.

**`main` is pushed through `12fab12`.** `tools/run-gates.sh` **PASS, 29/29**.
`cargo test --workspace` green. Clippy and fmt clean.

**Twelve Passes shipped across this session:**

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
| `284.0` | `ea4acb3` | **redaction sweeps the FILE, not the graph** — decision 146, `R249` |
| `285.0` | `1366138` | an abandoned content stream's drawn text is blanked |
| `286.0` | `369d4de` | the redaction report carries the words a MARK covered |
| `287.0` | `1bbb7c1` | **a stamp's label size is a property** — decision 147 |
| `288.0` | `554897e` | **Acrobat-compatible stamp collections** — decision 148, `R250` |

Filings: 485th `6b10e92` … 491st `1bee454`. `R247` **claimed** (489th).
Ledger: rules → `R250`, decisions → `148`, filings → `491`.

---

## ★★ WHAT THIS SESSION ESTABLISHED THAT OUTLIVES ITS PASSES

Four postures landed. Read the ones that touch your area **before** working in
it, because each replaced a default that used to look reasonable.

### Decision 145 — a damaged file OPENS (reader)

Six malformation classes now open instead of costing the document; every
decision is recorded with what it chose *between*; `--on-malformed` takes the
other one. **Fail-clean never meant refuse.** The question is no longer *"is
this file conforming?"* but *"can pdfcer continue without inventing anything?"*
— and §7.3.10's undefined-object rule is usually the answer. Only a missing
`/Root` stays fatal, because continuing would mean fabricating a catalog.

### Decision 146 + `R249` — evidence, never reachability (writer/redaction)

Carriers were found by **navigating the graph** while the writer emits by
**enumerating the xref**. Everything in the difference was copied through
verbatim while the report said `scrubbed`.

**`R249` generalises well past redaction:** *before scoping any destructive
sweep to satisfy an outcome-shaped obligation, scope it to the evidence the
obligation itself names — never to a computed reachability or liveness walk,
which drops content **silently** rather than refusing loudly.* Three object
classes are unreferenced **by design** (object streams via type-2 entries,
xref streams via byte offset, the linearization dictionary by a `shall`).

★ The warrant is empirical: the census probe built to *measure* the fix
reproduced the exact failure it was written to catch, **twice**, moving my
reported figure 21% → 12%.

### Decision 147 — borrow the sibling subtype's key, never a private sidecar

Where the spec defines **no** key for a subtype's derived parameter, use the
key the standard already defines for the **identical** problem on a sibling
subtype. `/DA` on `/Stamp`, from §12.7.3.3's `/FreeText` entry — **not**
`/PieceInfo`. A font size is not private data; burying a legible answer in an
application-keyed sidecar makes every other tool unable to read what pdfcer
could write in the open.

★ Its asymmetry is the reusable half: a **size** is recovered (from `/DA`, or
the baked `Tf`); a **fit policy** is settable but **never** recovered, because
no stored key records an author's intent and a geometric guess would invent one
nobody made.

### Decision 148 + `R250` — a `(c)` label is a pointer, not a licence

`pdfcer-acrobat-librarian` did its job exactly right: correct shape from
convergent **community** sources, labelled `(c)`, two gaps flagged **by name**.
**That labelling is what made verification cheap** — I knew precisely what to
check. Adobe's own stamp files were on this machine and both answers were in
them.

⇒ **`R250`: a Feature-RAG finding labelled `(c)` is a pointer at what to verify
against a primary artifact when one exists on disk — not a licence to build
from unchecked.** Recorded as a habit too: Acrobat's install directory ships
directly-readable primary assets.

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

## ★ THE QUEUE — EMPTY OF INBOUND WORK

★★ **Every request from `pdfcer-gui` is closed, and so is every item this
session's queue opened.** That has not been true before. The next session
starts from the operator's own plan, not from a backlog.

**Closed this session:** the sticky-resize refusal, `/Ink` node editing, the
circular font advice, `run_repertoire`, hybrid-file redaction, the `/Info`
scrub gap, the malformed-file posture, the whole-file redaction sweep, the
abandoned content stream, `redacted_text` granularity, the stamp text size,
and Acrobat-compatible stamp collections.

### The operator's own ordered plan (2026-09-06), now the front of the queue

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).

### Two open questions worth settling when the right file exists

1. **Does a PLACED stamp remember which stamp it came from?** The acrobat
   librarian flagged this as the single highest-value gap in its stamp file
   and it is **deliberately still open**: this session read collection *files*
   only, never a placed-and-saved annotation. Settling it needs a PDF stamped
   by Acrobat Pro, which is not on this machine (Reader only —
   `acrobat-reader-is-available-pro-is-not`). ★ It was left `GAP` rather than
   swept along with the two gaps that *were* closed, and that separation is
   the point: proximity to an answer is not an answer.

2. **Should the line-ending-agnostic edit helper become a real `tools/`
   script?** A multi-line `str.replace` against a CRLF file with an LF pattern
   matched **zero times, silently**, three separate times this session. I
   wrote a helper that normalises to the file's own line ending and refuses
   unless the pattern matches exactly once — but it lives in a job temp
   directory and dies with it. The librarian recorded it as a suggestion, not
   an owed item. **It is the durable fix for a failure that recurred three
   times in one day**, which is the argument for promoting it.

---

## OWED

- **`R221`'s recorded instance count is wrong and a commit message made it
  worse.** `Pass 279.0` says "third recorded instance"; the Standing Rules entry
  is already past three, and the 480th filing flagged the discrepancy rather
  than guessing. Reconcile it in a session with budget. **Do not copy an ordinal
  from a commit message.**
- **~~`R247` is reserved-but-unclaimed~~ — RESOLVED 2026-09-09 (489th filing).**
  It went to **a doc comment publishing a behavioural guarantee no test
  enforces**; the competing "alternate route" candidate was **withdrawn, not
  deferred**, because its instances were already being filed correctly inside
  `R225`'s family and a second number would have meant two rules to reconcile.
  Founding instance: `Pass 285.0`'s `blank_show_strings` span-scoping comment.
  Ceiling was `R249` then; `R250` has since been minted (491st filing).
  ★ **The lesson is in how long it took, not in the answer.** It sat through
  four filings because each one correctly declined to guess and each mint went
  *past* it — a discipline that works and compounds into a worse tangle. **A
  reservation nobody has authority to settle is not waiting for information; it
  is waiting for a decision.** Make it, or withdraw the slot.
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

★★ **`target/debug/deps` GROWS WITHOUT BOUND — cargo never garbage-collects
it.** It had reached **154 GB** (of a 168 GB `target/`) on a disk at **90 %**
full. `rm -rf target/debug` reclaimed **159 GB** and took the disk to 73 %.
Clearing `incremental` alone is not enough — that is only ~12 GB and it was the
previous session's mistake.

`du -sh target/debug/deps` every session. Both checks must be run before any
delete: `git ls-files target` returns 0 and `git check-ignore -q target`
passes. The cost of the delete is one full debug rebuild (~8 min), which is
cheaper than the disk filling mid-Pass.

★ **Low memory kills background tasks**, and it killed five this session. The
machine has 16 GB shared with Dropbox, Everything, Defender and several Claude
sessions; ~3 GB free is normal. Nothing is runaway — it is contention, and
background watchers are the first thing sacrificed. **Check CI directly rather
than leaving a poller running.**

**Foreground survives where background dies.** `tools/run-gates.sh` takes well
over ten minutes; run it in the background and poll, but run the expensive
`cargo test --workspace` in the **foreground** with `--test-threads=2`.

★ **`run-gates.sh` piped into `tail` buffers everything** — the output file
stays 0 bytes until the run ends, which looks exactly like a hung sweep.
Redirect to a file and tail *that*, or just wait for the completion
notification.
