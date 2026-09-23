# pdfcer — Project Instructions

An open-source (MIT), non-monetized, feature-for-feature replacement for
Adobe Acrobat Pro: a Rust engine (`pdfcer-core`, `pdfcer-render`) plus a
first-class CLI (`pdfcer`). The desktop GUI is the separate `pdfcer-gui`
project. See `README.md` and `docs/ARCHITECTURE.md`.

Everything below is binding, on top of the global rules in
`C:\Users\Ken\.claude\CLAUDE.md`. History behind each rule lives in
`docs/ARCHITECTURE.md` §12 (decisions) and git — not here.

## Project agents

| Agent | Role | When to dispatch |
|---|---|---|
| `pdfcer-engineer.md` | Lead engineer | The default role. If you're the orchestrator, **be this agent** — read its file at session start. |
| `pdfcer-librarian.md` | Keeper of `ROADMAP.md` / `FEATURES.md` / `SESSION_LOG.md` / the `ARCHITECTURE.md` decision log | Every new request (→ roadmap entry), every Pass completion (→ Shipped row), pre-compaction captures, findings that graduate to `D:\dev\rag\rust\`, `D:\dev\rag\egui\` or `C:\personal_rag\pdf\`. |
| `pdfcer-spec-librarian.md` | PDF-standard RAG at `D:\Dev\Rag-Specialized\PDF_Spec\` | Any spec question needing canonical sourcing. |
| `pdfcer-acrobat-librarian.md` | Acrobat Pro feature-parity RAG at `D:\Dev\Rag-Specialized\Acrobat_Features\` | Scoping a Backlog bucket into a Pass. Capabilities only — never Acrobat's GUI mechanics. |
| `pdfcer-inkscape-librarian.md` | Inkscape parity RAG at `D:\Dev\Rag-Specialized\Inkscape_Features\` | Scoping vector-editing Passes. Inkscape is GPL — behavioural reference only (R61). |
| `pdfcer-ui-specialist.md` | egui/eframe UX review | Non-trivial UI decisions. Returns a critique + change list; writes no code. |

## Read first

**`docs/NEXT_SESSION.md` first, then only what the task needs.**

- `docs/NEXT_SESSION.md` — engineer handoff: state, in flight, owed, what to distrust.
- `docs/ROADMAP.md` — *Next up*, *Backlog*, the *Standing rules* index.
  Shipped history is in `docs/history/` — grep it, do not read it.
- `docs/ARCHITECTURE.md` — invariants (§3, §5); §12 decisions when cited.
  **Grep for the section; do not read the file.**
- When the task touches them: `FEATURES.md` (what pdfcer can do, core/cli/gui
  boxes; librarian-maintained in the same filing as every ROADMAP change;
  ROADMAP wins on disagreement), `SESSION_LOG.md`'s newest entry, `LEGAL.md`
  (licensing, spec sourcing, test corpus), `docs/core-api/` (the GUI
  project's contract).

## Project-specific rules (binding)

### 1. Spec fidelity

Never implement spec-governed behavior (byte layout, filters, xref, font
encoding, crypto) from memory. Check `D:\Dev\Rag-Specialized\PDF_Spec\`;
dispatch `pdfcer-spec-librarian` if it's missing. Cite the ISO/ITU-T/ETSI
clause in the doc comment.

### 2. GUI-core separation

`pdfcer-core` and `pdfcer-render` never gain a GUI/windowing dependency.
Verify with `cargo tree -p pdfcer-core` / `-p pdfcer-render` on any Pass
touching their `Cargo.toml`. This keeps the web/WASM fork a shell swap.

### 3. Round-trip / minimal-diff editing

Objects pdfcer didn't logically touch are re-emitted byte-identical (full
rewrite) or omitted (incremental save, the default). Redaction is the one
exception: it must truly remove covered content. `ARCHITECTURE.md` §5.

### 4. Fuzzy, never sneaky

Anything pdfcer **inferred** — OCR text, auto-detected fields, recognised
text blocks, snapped points, best-fit geometry, derived centrelines, reflow
results, suggested Bates ranges, substituted or synthesised fonts — is
**disclosed, never silent**.

- **The commit point is SAVE**: Undo rejects, Save commits; nothing in an
  open edit session is document state.
- Inferred content **renders exactly as saved content will render** — no
  badge, tint, red flag, dashed outline or "provisional" layer on the page.
- The disclosure lives **off-canvas** (status line, results panel,
  post-command report, properties field); it never blocks, never needs
  acknowledgement, is never positioned relative to the document.
- **No accept/reject gate in front of anything.** Uncertainty (a residual,
  a font-trust downgrade, an overflowing reflow) is stated in the disclosure.
- In `pdfcer` the invocation is the commit, so the CLI **prints** what it
  inferred. What the rule forbids is **silence**, in both shells.
- It bites hardest on inferences the operator cannot see: mode-3 OCR text,
  a plausible font substitution, an over-eager snap.

This is a **correctness** rule: a provisional marking is a second rendering
path for the same content, and two paths drift. Test: *would a screenshot
of the editing canvas differ from the same document saved and reopened,
because pdfcer is marking its own uncertainty?* If yes, that's the defect.
Still allowed: the redaction save confirmation (§11.2 — it warns about a
destructive save) and editor chrome (R167: selection highlight, handles, a
dashed outline for a widget with no paintable `/AP`) — those disclose
editability, not uncertainty. Decisions 024 §4.4 and 059.

### 5. Roadmap discipline

New request → engineer parses into Pass entries → `pdfcer-librarian` files
them → report the Pass IDs. Pass complete → librarian moves it to Shipped
and appends `SESSION_LOG.md`.

### 6. Documentation-first

Per the global rule, bounded: public contracts, user-facing help and the
non-obvious why get full detail; private internals, restated code, history
and decoration get none.

### 7. Test corpus

Fixture PDFs are synthetic or clearly rights-cleared — never a downloaded
real-world PDF of unknown provenance. `LEGAL.md` §5.

### 8. License, push and release

- License is **MIT** (`LEGAL.md` §1). All dependencies are permissive.
- **Pushing `main` (fast-forward) is standing-authorized** ("always push",
  decision 090). **Releasing is standing-authorized** ("always go ahead and
  push the latest one", decision 121): tag, package, smoke-test, deploy to
  OneDrive — but only after a green `tools/run-gates.sh`, a fresh-folder
  smoke test and `verify-release.py`.
- **Still needs an explicit, current go-ahead each time:** `git push
  --force` or anything rewriting published history (breaks every cited
  hash); pushing any branch other than `main`; remote branches or tags
  other than the release tag.
- **The repository is public** (`github.com/KenM76/pdfcer`; the archived
  predecessor is `KenM76/pdfce`), so anything committed is published.
  Run `check-suite-name-absent.py` green before every push, and read CI's
  colour from GitHub. Confidential material already in history was
  reviewed and accepted by the operator (`LEGAL.md` §1.1) — settled.
- **GPL/AGPL is categorically out** as a dependency or code source (MuPDF,
  Poppler, Ghostscript, Inkscape). `LEGAL.md` §6.1.
- `THIRD_PARTY_LICENSES.md` is generated by `cargo-about`, never
  hand-edited; regenerate whenever the dependency set changes.

### 9. Cross-project knowledge bases

- `D:\Dev\Rag-Specialized\PDF_Spec\` — canonical PDF standard; written by
  `pdfcer-spec-librarian`.
- `C:\personal_rag\pdf\` — empirical real-world producer quirks (Word,
  LibreOffice, Chrome, scanners, CAD). Grep it before re-deriving producer
  behaviour. Written by `pdfcer-librarian`.
- `D:\dev\rag\rust\` — Rust/Cargo/packaging findings, plus
  `rust-style-guide-and-api-guidelines.md` (rule 10).
- `D:\dev\rag\egui\` — egui/eframe/wgpu findings.
- `C:\personal_rag\claude_code\` — Claude Code tooling.

### 10. Rust Style Guide + API Guidelines

`cargo fmt --check` and `cargo clippy -- -D warnings` clean before any Pass
ships. Any `pub` item in `pdfcer-core` (or the CLI's argument/output
surface) is checked against `D:\dev\rag\rust\rust-style-guide-and-api-guidelines.md`
(naming, trait derives, `thiserror` errors, runnable examples).
`ARCHITECTURE.md` §8.

### 11. CLI (`pdfcer`)

A genuine, scriptable batch interface, not a debug tool. Same separation,
round-trip and fuzzy-never-sneaky discipline. Each feature Pass ships its
subcommand in the same session. `ARCHITECTURE.md` §7.

### 12. Acrobat feature-parity RAG

Before scoping a Backlog bucket, dispatch `pdfcer-acrobat-librarian` so
acceptance criteria reflect what Acrobat actually does. It catalogs
capability and behaviour only — never Acrobat's GUI structure.

### 13. Dependency licensing & attribution

Classify every new dependency's license (`LEGAL.md` §6.1) and check
`docs/PRIOR_ART.md` first. Weak copyleft (LGPL/MPL) is always flagged to the
operator, never decided solo; GPL/AGPL cannot be linked at all.
`ARCHITECTURE.md` §9.

### 14. RAG format

Every RAG this project writes is for **LLM consumption**: dense,
schema-consistent, grep-first, no narrative padding.

### 15. "pdf dimensions" vs "ce dimensions" — never bare "dimensions"

- **pdf dimensions** — already in the PDF, exported by CAD or another tool.
  pdfcer reads and measures against them and must not silently alter them.
- **ce dimensions** — dimension objects **pdfcer authors** (`/Line` +
  `/IT /LineDimension` with a baked `/AP`, groups, scale, `/Measure`,
  `/PieceInfo`; `crates/pdfcer-core/src/dimension/`). Editable and deletable.

The distinction is provenance, not representation. Binding in every reply,
commit, doc, RAG entry and **subagent dispatch**. When the operator says
"dimension" unqualified, echo back the qualified term.

## Typical session

1. Read `docs/NEXT_SESSION.md`.
2. Parse the request into Passes; dispatch `pdfcer-acrobat-librarian` when
   scoping a new bucket, then `pdfcer-librarian` to file them.
3. Work the Pass: spec RAG for spec-governed behaviour, `pdfcer-ui-specialist`
   for non-trivial UI.
4. Ship: tests green, `cargo tree` invariant, packaging smoke test if
   packaging changed.
5. Librarian files the completion; brief the operator.

## Outstanding open items (surface when relevant)

- **XFA** — demand negligible (0.08% of organic files) and XFA is
  deprecated in ISO 32000-2 itself. Authoring is decided (decision 020:
  dynamic XFA out of scope, static-XFA hybrid field creation refused).
  Whether to narrow the item to read/fill only, or retire it, is open
  operator question **(p)**.
- **OCR** — engine: **both** engines behind Cargo features, ranked on
  multi-language coverage (operator). **Still open, question (bl):** whether
  a CC-BY-SA-4.0 model file (the pure-Rust `ocrs` engine, the only WASM-capable
  route) may ship in the MIT portable folder. Default until answered: ship
  neither model set. Surya's weights carry a revenue-capped licence — do not
  re-evaluate them. Tesseract's default Windows build ships LGPL binaries.
  Survey: `docs/ocr-engine-survey.md`.
- **Poppler's exact license** (GPL vs LGPL) — unresolved in `PRIOR_ART.md`;
  moot while GPL/AGPL is out, re-verify if it ever matters.
