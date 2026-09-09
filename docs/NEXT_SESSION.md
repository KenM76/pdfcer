# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-09, after `Pass 280.0`.

---

## STATE

Workspace version `0.49.0`. **Last release is still `v0.45.0`** (2026-09-07) —
everything since is pushed-or-pushable but unreleased. Releasing is
standing-authorized (decision 121); nobody has needed it yet, and the consuming
project reads `docs/core-api/` from the repo rather than from a tarball.

**Four Passes shipped today**, each closing an inbound request from
`pdfcer-gui`:

| Pass | commit | what |
|---|---|---|
| `277.0` | `fccd6cd` | a sticky note refused a resize by claiming pdfcer had not drawn it |
| `278.0` | `c8a6697` | freehand `/Ink` strokes became editable, per point and per stroke |
| `279.0` | `5b8ec61` | the font refusal named a face that led in a circle |
| `280.0` | `26ef381` | `run_repertoire` — the alphabet, asked before the first keystroke |

Plus three librarian filings (478th `ac0fcb2`, 479th `7890800`, 480th
`6384587`); the 481st (for `280.0`) is being written as this file is saved.

`tools/run-gates.sh` **PASS on all 29 commands** as of `26ef381`.
`cargo test --workspace` 5,013+ passing, 0 failures.

---

## ★ THE QUEUE — updated 2026-09-09 after `Pass 281.0`

**`Pass 281.0` (`1177221`) closed the hybrid/redaction blocker** — a full
rewrite of a §7.5.8.4 hybrid file now emits the three-part unit instead of
refusing, so `redact-apply` reaches such files. Verified end to end through the
binary and against the corpus harness (225/237 → 237/237 per-object verbatim,
no shortfalls either way).

**Two requests remain, both from `pdfcer-gui`, in this order:**

1. **`request_redacted_text_carries_single_characters_on_a_per_glyph_producer_so_the_absence_proof_is_blind.md`**
   — CONFIRMED at the source and replied to; not built. `redacted_text` is
   accumulated **per show operator**, so a per-glyph producer yields single
   characters and their absence proof greps for the alphabet. ★ **It is the same
   bug as the `carrier_info` gap below**: that field has a second consumer inside
   the engine, and the two want opposite granularities — joining runs (what they
   asked for) makes the `/Info` under-match *worse*. Ship both halves in one
   Pass: per-mark joined text, plus a `carrier_info` match rule that does not
   depend on granularity, plus the granularity stated in the report.
2. **`request_resize_annotation_refuses_a_pdfcer_authored_stamp_as_foreign.md`**
   — `/Stamp` is the **third** authoring family `resize_annotation`'s appearance
   test does not know. Same shape as `Pass 276.0`'s `/FreeText`. ★ Look for a
   fourth while you are there — three found one at a time is `R245` at n=3.

**★ The redaction-diligence gap, owed and unbuilt:** `redact::carrier_info`
drops an `/Info` string that CONTAINS a redacted run, so a run longer than the
metadata string cannot match — and the carrier line reports `scrubbed` either
way. Measured during `Pass 281.0`'s smoke test. This is the hard-rule area.

---

## ~~THE QUEUE — TWO REQUESTS, READ AND QUEUED, NEITHER STARTED~~ (superseded above)

Both arrived 2026-09-09 while `Pass 280.0` was being built. **Both channels
checked by diff at session end.** Take them in this order:

### 1. `request_a_hybrid_reference_file_cannot_be_redacted_because_its_full_rewrite_is_refused.md`

**Highest severity in the queue, and not because it is hard.** A full rewrite of
a hybrid-reference file (`/XRefStm`, §7.5.8.4) is refused by name. Redaction
**must** be a full rewrite (`R35` — an incremental save leaves the un-redacted
content in a prior revision), so **redaction is unreachable on such files**. The
refusal's own remedy — *"use incremental save"* — is the one thing the redaction
pipeline is forbidden to take.

Measured by them on the operator's own file: `SW41177 MATERIAL
REQUIREMENTS.pdf`, 34,971 bytes, SolidWorks-exported. His other two sheets from
the same drawing set rewrite fine. He has asked three times.

The ask: read the classic table and the `/XRefStm` stream as one object set (the
reader already does, or the file would not open) and write a **single**
conforming revision with no hybrid remnant. Their own argument for why that is
allowed: the hybrid form exists for pre-1.5 readers, and `SaveReport.delinearized`
already records a comparable structural change. **Check the spec RAG on
§7.5.8.4 before accepting that reasoning** — it is plausible and it is theirs,
not mine.

### 2. `request_resize_annotation_refuses_a_pdfcer_authored_stamp_as_foreign.md`

`/Stamp` is the **third** authoring family `resize_annotation`'s appearance test
does not know — the markup family, then `/FreeText` (`Pass 276.0`), now stamps.
Unlike the sticky (`Pass 277.0`, refused deliberately: a `/Text` has no size), a
**stamp genuinely has a size** and Acrobat resizes its own. Operator's words:
*"there's still no way to edit the size of a placed stamp."*

Straightforward: re-bake through the stamp builder at the new `/Rect`, exactly as
`Pass 276.0` did for `/FreeText`; a foreign stamp keeps today's refusal, which is
true for it. ★ **Look for a fourth family while you are in there** — three
have now been found one at a time, which is `R245`'s shape at n=3.

### After those, the operator's own ordered plan (2026-09-06) is still untouched

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).

---

## OWED

- **`R221`'s recorded instance count is wrong and I made it worse.** `Pass 279.0`'s
  commit message says "third recorded instance"; the Standing Rules entry is
  already past three, and the 480th filing flagged the discrepancy rather than
  guessing. Reconcile it in a session with budget for it. Do not copy the
  ordinal from a commit message.
- **`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on private functions. Staged fix, its own change.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded**
  (55.3 %). Pre-existing; `LEGAL.md` §5 makes it a licensing statement, not
  tidiness.
- **Backup bundle is well over 150 commits behind `HEAD`.**

---

## ★★ WHAT THIS SESSION GOT WRONG — the three worth carrying

### A correct test, on a fixture that could not fail. TWICE, six hours apart.

- `Pass 278.0`: a sabotage survived because the test removed the **last** ink
  stroke, where the naive code answers `0` by accident. Re-pointed at stroke 0 —
  where the surviving stroke slides into the index — it goes red.
- `Pass 279.0`: `refusal_names_a_font.rs` already ran the complete
  refuse → take the named face → switch → re-edit loop. Correct, green, and
  **structurally incapable** of catching the shadowing bug, because its
  fixture's font is `AAAAAA+pdfcerSymbolicPrivate` and no standard-14 name
  matches that stem.

**The question to ask of any test you inherit: which fixture could ever have
made this go red?** Both were found by sabotage; neither would have been found
by reading.

### An enumerating gate caught a NEW route within the hour — and that is the contrast

`route_enumeration.rs` scans for every function that locates a text anchor and
demands it resolve the find. `Pass 280.0`'s new verb was a fourth such route and
the gate named it by function, before any consumer saw it. **A fixture-bound
test cannot catch a new case; an enumerating gate catches a new route.** Prefer
the latter when a family keeps growing.

### I told another project they had not answered — 67 minutes after they had

Second instance in two days of the same file-channel blindness: no notification,
no version token, so a reader who has already looked is blind to anything that
arrives after. The remedy was **already written down** and not applied.

⇒ **`stat` the inbound directory immediately before writing any reply**, not
only at session start. It cost nothing today because the correction went out on
the same channel within minutes — and it is how the 477th filing's retracted
motivation happened too.

### Smaller, and all recurrences

- **The doc-comment splice orphaned a doc block AGAIN** (`candidate_chars`
  inserted above `encode_str`). Anchor on the DOC BLOCK, not the item.
- **A sabotage aimed by string hit the wrong match arm** and stayed green,
  reading exactly like a surviving sabotage. Aim by *function* first, then by
  string inside it, and re-read the diff before believing the result.
- **A rustdoc example did not compile** (`Document::load` takes `&Path`). Only
  the doctest pass reads an example as code; `cargo check` never will.
- **Prose through the Bash tool broke twice more** — a python heredoc with a
  trailing `\` inside a single-quoted string, and a wrapped string literal that
  lost its continuation. Write the file with the Write tool; splice by line
  index.

---

## Standing habits (unchanged, and all of them earned their place again today)

- Check BOTH FeatureRequests channels **by diff**, at session start **and before
  every reply**.
- Write a reply for every request you close; correct a reply that turns out
  false, on the same channel, promptly.
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

`target/` was **187 GB** at session end on a disk at **96 % full**, and
`rm -rf target/debug/incremental` reclaimed **24 GB** (now 164 G, disk 94 %,
65 G free). Both checks were run before the delete and both must be:
`git ls-files target` returns 0 and `git check-ignore -q target` passes.
`du -sh target/` every session — this is the second consecutive session where
the same 24–26 GB had rebuilt.

**Foreground survives where background dies.** `tools/run-gates.sh` takes well
over ten minutes; run it in the background and poll, but run the expensive
`cargo test --workspace` in the **foreground** with `--test-threads=2`.
