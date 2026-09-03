# NEXT SESSION — start here

Engineer-owned handoff. Read this **before** `ROADMAP.md` — that says what
shipped, this says what to do next. **Overwrite it once acted on.**

Per standing rule `R216` this file carries **no edit-history layer**. What is
true now, plus a pointer. Corrections and their prior wording live in the
append-only record (`ROADMAP.md`, `SESSION_LOG.md`).

Written **2026-09-03**, at the end of the session that shipped the operator's
export/copy-out request as **`Pass 248.0` / `248.1` / `248.2`** and released
it as **`v0.29.1`**. Everything below was measured with a shell.

**For the ledger — Pass ceiling, rule ceiling, decision ceiling, filing count —
run `python tools/check-ledger-numbers.py`.** Do not mint from memory.

---

## §0 WHERE YOU ARE

- **`D:\Dev\pdfcer` is the project** (`D:\Dev\pdfce` is the frozen backup —
  never write there). Crates `pdfcer-core`, `pdfcer-render`, `pdfcer-cli`
  (binary `pdfcer`), `pdfcer-print`, `pdfcer-fetch`. The GUI is the separate
  `D:\dev\pdfcer-gui` project, on `pdfcer` crates since its v0.5.0.
- `main` = `49adf4c` = `origin/main` = tag `v0.29.1`; release page up, asset
  SHA-256 `3a8797fb…e446`; OneDrive `pdfcer1` = 0.29.1, `pdfcer2` = 0.29.0.
  **`v0.29.0` is a tag that was never released** (red off-Windows; see §4).
- Test total: `cargo test --workspace` green (183 test binaries + doctests);
  the count moved by +7 render (export_image) +11 render (export_svg) +7 CLI
  (export_image) +2 CLI (copy_page) +4+4 unit since 4,733.

---

## §1 ★ NEXT: `Pass 5.4` — encryption authoring, `/R` 6 / AES-256 only

Unchanged from the previous hand-off and still at the *Next up* head:
`EditSession::set_encryption` (owner + user password, `/R` 6 only — `/R` 2–4
and 5 refused by name), `set_permissions`, `remove_encryption`, all
owner-authenticated or refused. Spec SOURCED: `PDF_Spec\security\security__aes256_r6.md`
(Algorithm 2.B complete) and `iso32000__ref__encryption_impl.md` rows
A8/W17/W19/W20/A13. Owed with it: the crate's stale "not available in the
spec corpus" strings (`crypto/standard.rs`'s
`EncryptionUnsupported::UnsourcedRevision`, doc comments in `crypto/mod.rs`,
`standard.rs`, `aes.rs`, `r5.rs`). Writer side: `Contents` is never
encrypted and a signature digest is over CIPHERTEXT (ETSI EN 319 142-1
§5.5) — the exemption list must carry it. Disclosure sentence for
permissions is in the channel's
`reply_signature_integrity_first_then_encryption_and_your_two_sentences.md`.
Ship the `pdfcer` subcommands in the same Pass (rule 11).

## §2 QUEUED AFTER / FOLLOW-ONS FROM TODAY

1. **Export follow-ons, none asked for, all recorded as deliberately not
   done:** true `<linearGradient>`/`<radialGradient>` for axial/radial
   shadings (today they are rasterised at `--dpi` and counted in
   `ExportTally::shadings_rasterised`); `CF_ENHMETAFILE` (only LibreOffice
   24.x needs it — needs an EMF writer + `SetEnhMetaFileBits` since
   clipboard-win has no metafile setter); a `copy-selection` CLI verb
   (the GUI route is `ObjectClip::to_pdf` → `export_svg`, core-api §7.9).
2. Mesh shadings deposit spot planes; the 8 unresolved conformance patches;
   `sh` cutting under a redaction mark; `set_page_tabs` when asked.
3. iccce's `reply_all_four_asks_measured_and_your_bpc_would_have_done_nothing.md`
   is STILL unread by any engine session.
4. Consider adding a Linux-target clippy of `pdfcer-cli` to `run-gates.sh`
   (§4 — the librarian was asked to suggest it; the engineer's call).

---

## §3 STATE OF THE ARC THAT SHIPPED TODAY (so it is not rebuilt)

- **SVG comes from the renderer's display-list recording in an EXPORT mode
  that never refuses** (decision 132): shadings rasterised into a scratch
  and harvested as image fills under their clip; soft masks kept with the
  layer; overprint / non-separable / non-isolated / colorant-buffer
  approximations COUNTED. Cache mode unchanged, AND it now refuses an
  elementary object under `gs /SMask` (it used to replay it unmasked —
  pinned by a test).
- Oracles: `tests/export_svg.rs` (resvg `=0.45.1`, no raster-images — the
  duplicates guard sees dev-deps) + an Inkscape end-to-end test that runs
  when Inkscape is installed. Word paste measured through combridge
  (`word run-script` into a NEW doc, closed unsaved): type 17 + `svgBlip`.
- `docs/core-api/03-capabilities.md` §7.7–§7.9 are the consumer contract;
  the channel note `note_export_to_png_jpeg_svg_and_copy_out_ship_here_is_what_to_wire.md`
  is open for pdfcer-gui to consume.

---

## §4 THINGS A NEW SESSION MUST KNOW BEFORE TOUCHING ANYTHING

- **★ Cross-target check the CLI before pushing a `cfg(windows)` change.**
  `thiserror` declared only under the Windows block compiled here and broke
  ubuntu/macOS/clippy in CI after the tag was pushed and OneDrive deployed.
  `cargo clippy -p pdfcer-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`
  and `cargo check --target aarch64-apple-darwin` are installed and cheap.
  And: do not tag/deploy a target-gated change until CI has answered.
- **A background `cargo test --workspace` / `run-gates.sh` gets killed by
  the harness after ~10 min** (three times today). Run the doctests
  separately in the foreground (`cargo test --workspace --doc`, ~2 min) and
  the python gates from `D:\Dev\temp\tail-gates.txt`'s list; everything
  else fits under the foreground cap per crate.
- **The Bash tool's cwd persists** across calls: a `cd fuzz` in one call
  made the next `python tools/…` fail. Use absolute paths or `cd` back.
- **A Python patch through a heredoc loses `\n`** (again, twice today) and
  **a regex with an optional repeated group over the 40k-line main.rs
  backtracks for minutes** — plain string search, scripts via the Write
  tool, every anchor asserted.
- **Diffing two RGBA renders: composite both over one backdrop first.** The
  RGB of alpha-0 pixels is noise and read as a 100 % mismatch on an exact
  render.
- **`clip-path` on an element that also carries `transform` is evaluated
  post-transform** — put clips on a wrapping `<g>`. Found by Inkscape, not
  by any unit test.
- **Stage by path. Never `git add -A`.** The librarian has had no shell in
  three of four dispatches today — commit its filing yourself, by path,
  with a message that says so.

## §5 MEASURED NEGATIVES — DO NOT RE-DERIVE

1. Inkscape's CLI has no `paste` action; a headless paste cannot be driven.
2. resvg 0.48 pulls tiny-skia 0.12 + png 0.18 beside the crate's own; its
   `raster-images` feature a second zune-jpeg line — both refused by the
   duplicates guard. 0.45.1 is the last line on tiny-skia 0.11.
3. tiny-skia's `StrokeDash` has no accessor for its array; SVG dashes are
   pre-applied with `Path::dash`.
4. The prior hand-off's negatives (rename debt: `pdfceF{n}` is a deliberate
   keep; `pdfce-gui`/`pdfce@cce414e:` citations/`pdfce_FeatureRequests`
   folder are NOT to be renamed) still stand.

## §6 ITEMS OWED BY THE OPERATOR

- Global `C:\Users\Ken\.claude\CLAUDE.md` still references `D:\Dev\pdfce\`.
- Open questions `(ca)`, `(cb)`, `(cc)` — unchanged.
