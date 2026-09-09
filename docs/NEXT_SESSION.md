# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-09, after `Pass 283.1`.

---

## STATE

Workspace version `0.49.0`. **Last release is still `v0.45.0`** (2026-09-07) —
everything since is pushed but unreleased. Releasing is standing-authorized
(decision 121); nobody has needed it yet, and the consuming project reads
`docs/core-api/` from the repo rather than from a tarball. **If you have budget
for a release, cutting one is overdue rather than forbidden.**

**`main` is pushed through `d8fcb68`.** `tools/run-gates.sh` **PASS, 29/29**,
including both filing gates. `cargo test --workspace` green.

**Seven Passes shipped today**, each closing an inbound request or an operator
report:

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

---

## ★★ READ DECISION 145 BEFORE ANY READER WORK

`Pass 283.0` turned two mid-session operator rulings into a **posture change for
the whole reader**, and the Pass is the small half of it:

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

**Where to look for the next candidates:** anywhere the reader still returns a
hard error for a *part* of a file — `pages()`, the font loaders, the content
interpreter, the annotation walkers. Each wants the same treatment: continue,
record, offer the choice if one exists.

---

## ★★★ THE FINDING FROM `283.1`, AND IT IS A HABIT, NOT A BUG

`Pass 283.0` was complete, correct, tested, documented — and its **intervention
was unreachable by the only shell that needed it.** The alternative reading
shipped on `Document::from_bytes_with_options`; `pdfcer-gui` opens files with
`Document::load`. Taking the other value meant re-implementing `std::fs::read`
at the call site. `Pass 283.1` added `Document::load_with_options`.

★ **This is `R245`'s shape applied to an AFFORDANCE rather than a GUARD** — a
facility present on one route and absent on its twin. `R245` was written about a
*check* that fired on one path and not the other; this second form is **harder to
see, because nothing is wrong on the route you are reading.**
`from_bytes_with_options` is faultless in isolation. The defect only exists from
the caller's side.

⇒ **The habit that found it, and the one to keep:** when you ship a capability
for a named consumer, **grep that consumer's tree for its actual call site**
before calling the Pass done. Do not ask "does the API have a way?" — it did.
Ask "can the caller reach it without duplicating the function beside it?" An
intervention only reachable by rewriting the route next to it is **present, not
offered**, and the operator's ruling says *"if the user can intervene … that
should always be an option."*

The librarian was asked to decide whether this claims a standing-rule number or
appends to `R245`'s family; **nothing in the code or docs claims a new number** —
`docs/core-api/01-reading-and-model.md` §3.6b cites `R245` by name. Check what
was recorded before citing it yourself.

---

## ★ THE QUEUE

★ **START HERE: an orphaned metadata object survives a redaction.** On the file
that motivated `Pass 281.0`, an `/Info`-**shaped** object the trailer does not
point at (superseded by a later one, still listed in the cross-reference table)
is re-emitted verbatim by the forced full rewrite with its `/Keywords` intact.
`carrier_info` scrubs the trailer's `/Info`; **nothing scrubs an orphan.**
`prior_revisions action=dropped_by_rewrite` is **true** — it is about superseded
byte ranges, not about objects the xref still names — so **no report line is
false and the content is still there**, which is the worst combination. Consult
the spec RAG on what §12.5.6.23's "all content" obliges before choosing between
scrub, drop, or disclose.

**Then, both from `pdfcer-gui`, in this order:**

1. **`request_redacted_text_carries_single_characters_on_a_per_glyph_producer_so_the_absence_proof_is_blind.md`**
   — CONFIRMED at the source and replied to; not built. `redacted_text` is
   accumulated **per show operator**, so a per-glyph producer yields single
   characters and their absence proof greps for the alphabet. ★ **It is the same
   bug as the `carrier_info` gap**: that field has a second consumer inside the
   engine, and the two want opposite granularities — joining runs (what they
   asked for) makes the `/Info` under-match *worse*. Ship both halves in one
   Pass: per-mark joined text, plus a `carrier_info` match rule that does not
   depend on granularity, plus the granularity stated in the report.

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

---

## OWED

- **`R221`'s recorded instance count is wrong and a commit message made it
  worse.** `Pass 279.0` says "third recorded instance"; the Standing Rules entry
  is already past three, and the 480th filing flagged the discrepancy rather
  than guessing. Reconcile it in a session with budget. **Do not copy an ordinal
  from a commit message.**
- **`R247` is reserved-but-unclaimed and contested** between two unrelated
  triggers. `R248` was deliberately minted *past* it. `283.1` produced a third
  candidate (the affordance/guard widening). Resolving `R247` needs a session
  with time, not a drive-by.
- **`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on private functions. Staged fix, its own change.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded**
  (55.3 %). Pre-existing; `LEGAL.md` §5 makes it a licensing statement, not
  tidiness.
- **Backup bundle is well over 150 commits behind `HEAD`.**

---

## ★★ WHAT THIS SESSION GOT WRONG — the four worth carrying

### A capability that shipped complete and unreachable

See `283.1` above. **The new one, and the most general.** Every other item here
is about a test or a tool; this one is about believing a Pass was finished
because the *API* was finished.

### A correct test, on a fixture that could not fail. THREE times.

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
made this go red?** All three were found by sabotage; none by reading.

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
- **Prose through the Bash tool broke repeatedly** — heredoc backticks command-
  substituted, `\` inside single-quoted python strings, and **a multi-line
  `str.replace` that silently matched zero times because the file is CRLF and my
  pattern was LF.** Write the payload with the Write tool and splice by line
  index, or join patterns with an explicit `\r\n`.

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
