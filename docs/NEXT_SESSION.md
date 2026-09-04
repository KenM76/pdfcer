# NEXT SESSION — start here

Engineer-owned handoff. Read this **before** `ROADMAP.md` — that says what
shipped, this says what to do next. **Overwrite it once acted on.**

Per standing rule `R216` this file carries **no edit-history layer**. What is
true now, plus a pointer. Corrections and their prior wording live in the
append-only record (`ROADMAP.md`, `SESSION_LOG.md`).

Written **2026-09-03**, at the end of the session that shipped the operator's
export/copy-out request (**`Pass 248.0`–`248.2`**, released `v0.29.1`) and
the two follow-ons he said "yes" to (**`248.3`** native SVG gradients,
**`248.4`** EMF, released **`v0.30.0`**). Everything below was measured
with a shell.

**For the ledger — Pass ceiling, rule ceiling, decision ceiling, filing count —
run `python tools/check-ledger-numbers.py`.** Do not mint from memory.

---

## §0 WHERE YOU ARE

- **`D:\Dev\pdfcer` is the project** (`D:\Dev\pdfce` is the frozen backup —
  never write there). Crates `pdfcer-core`, `pdfcer-render`, `pdfcer-cli`
  (binary `pdfcer`), `pdfcer-print`, `pdfcer-fetch`. The GUI is the separate
  `D:\dev\pdfcer-gui` project, on `pdfcer` crates since its v0.5.0.
- `main` = `1a31d2d` + the 411th filing = `origin/main`; tag `v0.30.0` at
  `1f9eb1d`; release page up, asset SHA-256 `835c29b3…5cbb`; OneDrive
  `pdfcer2` = 0.30.0, `pdfcer1` = 0.29.1. `v0.29.0` is a tag that was never
  released (red off-Windows) — leave it.
- `cargo test --workspace` green: 183+ test binaries plus doctests.

---

## §1 ★ NEXT: `Pass 5.4` — encryption authoring, `/R` 6 / AES-256 only

Unchanged from the previous two hand-offs and still at the *Next up* head:
`EditSession::set_encryption` (owner + user password, `/R` 6 only — `/R` 2–4
and 5 refused by name), `set_permissions`, `remove_encryption`, all
owner-authenticated or refused. Spec SOURCED:
`PDF_Spec\security\security__aes256_r6.md` (Algorithm 2.B complete) and
`iso32000__ref__encryption_impl.md` rows A8/W17/W19/W20/A13. Owed with it:
the crate's stale "not available in the spec corpus" strings
(`crypto/standard.rs`'s `EncryptionUnsupported::UnsourcedRevision`, doc
comments in `crypto/mod.rs`, `standard.rs`, `aes.rs`, `r5.rs`). Writer side:
`Contents` is never encrypted and a signature digest is over CIPHERTEXT (ETSI
EN 319 142-1 §5.5) — the exemption list must carry it. Disclosure sentence
for permissions is in the channel's
`reply_signature_integrity_first_then_encryption_and_your_two_sentences.md`.
Ship the `pdfcer` subcommands in the same Pass (rule 11).

## §2 QUEUED AFTER

1. Mesh shadings deposit spot planes; the 8 unresolved conformance patches;
   `sh` cutting under a redaction mark; `set_page_tabs` when asked.
2. iccce's `reply_all_four_asks_measured_and_your_bpc_would_have_done_nothing.md`
   is STILL unread by any engine session.
3. Optional, unasked, recorded only: a `copy-selection` CLI verb (the GUI
   route is `ObjectClip::to_pdf` → `export_svg`/`export_emf`); true
   `<radialGradient fr>` for two-circle radials (SVG 2 only).

---

## §3 STATE OF THE EXPORT ARC (so it is not rebuilt)

- **One recording, three writers.** `record_page_for_export` (export mode
  of the display-list recorder, decision 132) feeds `svg.rs`, `emf.rs` and
  the raster path. Never build a writer over `vector::PageObjects`.
- **SVG:** gradients native for axial and focal-radial (`Shading::gradient_spec`,
  `Brush::Gradient`); raster fallback for the rest, counted in
  `ExportTally`. Oracles: resvg (`=0.45.1`, no raster-images — the
  duplicates guard sees dev-deps) and Inkscape when installed.
- **EMF:** `emf.rs` from `D:\dev\rag\emf\` (built today from [MS-EMF] v18.0
  + LibreOffice/Inkscape source; its golden file verified). Oracle is REAL
  GDI (`PlayEnhMetaFile` via PowerShell P/Invoke, script embedded in
  `tests/export_emf.rs`). **`System.Drawing.Imaging.Metafile` is NOT an
  oracle for `EMR_ALPHABLEND`** (plays a half-alpha premultiplied pixel as
  nearly opaque). LibreOffice 24.x ignores the fill rule and renders
  `PS_USERSTYLE` solid — dashes are pre-applied, nonzero multi-subpath
  fills are counted.
- **Clipboard (`copy-page`):** `image/svg+xml` (+NUL), `CF_ENHMETAFILE`
  (windows crate: `SetEnhMetaFileBits` + `SetClipboardData` inside
  clipboard-win's guard), `PNG`, `CF_DIBV5`, `application/pdf`. Word's
  default paste takes the SVG (measured: `svgBlip`); its Paste Special →
  EMF through combridge COM did NOT return (killed; not measured).
- `docs/core-api/03-capabilities.md` §7.7–§7.10 is the consumer contract;
  the channel note (with its addendum) is open for pdfcer-gui.

---

## §4 THINGS A NEW SESSION MUST KNOW BEFORE TOUCHING ANYTHING

- **★ Cross-target check the CLI before pushing a `cfg(windows)` change**
  (`cargo clippy -p pdfcer-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`,
  `cargo check --target aarch64-apple-darwin`); done for 248.4, skipped
  for 248.2 and it cost a dead tag.
- **A filing commit carries ONLY docs.** `fuzz/Cargo.lock` riding in the
  407th filing made it an unfiled code commit and blocked a push hours
  later (410th filing). Stage by the doc paths.
- **A background `cargo test --workspace` / `run-gates.sh` gets killed by
  the harness after ~10 min** (four times today). Run test binaries per
  crate and doctests separately in the foreground; python gates from
  `D:\Dev\temp\tail-gates.txt`'s list (or `check-ci-parity.py --list`).
- **The Bash tool's cwd persists**; `cd fuzz` in one call broke the next.
- **Patch scripts through the Write tool**, plain string anchors, every
  anchor asserted; anchor on the DOC BLOCK not the `fn` (an `is_paintable`
  doc was welded onto `gradient_spec` today; the doc gate caught it).
- **The librarian had no shell in 7 of 8 dispatches today** — commit its
  filing yourself, by path, and say so in the message.
- **Diff RGBA renders over one backdrop**; **`clip-path` on a transformed
  element is post-transform** (wrap in `<g>`).

## §5 MEASURED NEGATIVES — DO NOT RE-DERIVE

1. Inkscape's CLI has no `paste` action; Word's `PasteSpecial(wdPasteEnhancedMetafile)`
   via COM hung (client killed after minutes; Word itself fine).
2. resvg 0.48 pulls tiny-skia 0.12 + png 0.18; `raster-images` a second
   zune-jpeg — both refused by the duplicates guard. 0.45.1 is the last on
   tiny-skia 0.11.
3. tiny-skia's `StrokeDash` has no accessor; dashes are pre-applied.
4. GDI+ (`System.Drawing`) mis-plays `EMR_ALPHABLEND`; real GDI is exact.
5. Rename debt unchanged: `pdfceF{n}` is a deliberate keep; `pdfce-gui`,
   `pdfce@cce414e:` citations, `pdfce_FeatureRequests` are NOT to be renamed.

## §6 ITEMS OWED BY THE OPERATOR

- Global `C:\Users\Ken\.claude\CLAUDE.md` still references `D:\Dev\pdfce\`.
- A scratch Word document (`Document1`, unsaved, empty) may be open in his
  running Word from the hung Paste Special probe — close it without saving.
- Open questions `(ca)`, `(cb)`, `(cc)` — unchanged.
