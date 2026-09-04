# NEXT SESSION — start here

Engineer-owned handoff. Read this **before** `ROADMAP.md` — that says what
shipped, this says what to do next. **Overwrite it once acted on.**

Per standing rule `R216` this file carries **no edit-history layer**. What is
true now, plus a pointer. Corrections and their prior wording live in the
append-only record (`ROADMAP.md`, `SESSION_LOG.md`).

Written **2026-09-03**, at the end of the session that shipped **`Pass 5.4`**
(encrypt on save, AES-256 `/R` 6 only) and cut release **`v0.31.0`**.
Everything below was measured with a shell.

**For the ledger — Pass ceiling, rule ceiling, decision ceiling, filing count —
run `python tools/check-ledger-numbers.py`.** Do not mint from memory.

---

## §0 WHERE YOU ARE

- **`D:\Dev\pdfcer` is the project** (`D:\Dev\pdfce` is the frozen backup —
  never write there). Crates `pdfcer-core`, `pdfcer-render`, `pdfcer-cli`
  (binary `pdfcer`), `pdfcer-print`, `pdfcer-fetch`. The GUI is the separate
  `D:\dev\pdfcer-gui` project, on `pdfcer` crates since its v0.5.0.
- `main` = `624ba1e` = `origin/main`: `743830d` (`Pass 5.4`) + `d37219f`
  (412th filing) + `624ba1e` (version bump to 0.31.0). Previous release
  `v0.30.0` at `1f9eb1d`; OneDrive `pdfcer2` = 0.30.0, `pdfcer1` = 0.29.1.
  `v0.29.0` is a tag that was never released (red off-Windows) — leave it.
- **Release `v0.31.0` — verify its final state before assuming.** At the
  moment this file was written it was IN PROGRESS: packaged at `624ba1e`
  (`D:\builds\pdfcer-20260903-2234-624ba1e`, `pdfcer.exe` 20,544,512 B),
  fresh-folder smoke test passed (encrypt / remove-encryption / the A13
  diagnostic all surfaced from the copied binary), CI run `33830016873` was
  building. **Owed if not done:** tag `v0.31.0` at `624ba1e` + push; `gh
  release create` with the packaged zip; `deploy-onedrive.py` (next slot is
  **`pdfcer1`** — `pdfcer2` holds 0.30.0); `verify-release.py`; a release
  filing. `git tag -l v0.31.0` and `gh release view v0.31.0` tell you where
  it actually stopped.
- `cargo test --workspace` green; `run-gates.sh` PASS (29 commands).

---

## §1 ★ NEXT — consult `ROADMAP.md`'s *Next up* head

`Pass 5.4` shipped, so the previous three hand-offs' "NEXT: 5.4" is spent.
The genuine next item is whatever now sits at the head of *Next up* in
`ROADMAP.md` — read it, do not mint from here. The recorded queue is §2.

## §2 QUEUED AFTER

1. Mesh shadings deposit spot planes; the 8 unresolved conformance patches;
   `sh` cutting under a redaction mark; `set_page_tabs` when asked.
2. iccce's `reply_all_four_asks_measured_and_your_bpc_would_have_done_nothing.md`
   is STILL unread by any engine session.
3. Optional, unasked, recorded only: a `copy-selection` CLI verb (the GUI
   route is `ObjectClip::to_pdf` → `export_svg`/`export_emf`); true
   `<radialGradient fr>` for two-circle radials (SVG 2 only).
4. **`pdfceGUI` consumes the encryption verbs** (their O108 read-side
   security tab) when they wire them — `docs/core-api` §5.5 is the contract.

---

## §3 STATE OF THE ENCRYPTION ARC (so it is not rebuilt)

- **AES-256 `/R` 6 only, in BOTH directions.** Read: `standard.rs`
  `authenticate_r5` picks `r5::Hasher::R6(A13Reading::default())` when
  `revision == 6`. Write: `crypto/encrypt.rs::build_aes256_r6` (Algorithms
  8/9/10), `crypto/r6.rs::hash_2b` (Algorithm 2.B). RC4 / `/R` 2–5 are never
  written (W14/W17).
- **The seam is `r5::Hasher`** (Sha256 vs R6). The owner path feeds `/U` into
  BOTH the seed string and every K0 — proven against pypdf, do not "simplify".
- **A13 is a SETTING** (`A13Reading`, default `PerformThenTest` = what pypdf
  and Acrobat write). A `/R` 6 auth failure raises `DocError::PasswordRequiredR6`,
  which NAMES A13 — that is the only interoperable reading, so a file written
  under the other one is the disclosed failure mode.
- **The three verbs are SAVE TRANSFORMS**, not undoable commands:
  `EditSession::set_encryption` (`&self`, plaintext ⇒ encrypted),
  `set_permissions` (`&mut self`, owner-only re-key), `remove_encryption`
  (`&mut self`, owner-only). `set_permissions`/`remove_encryption` call
  `Document::clear_encryption` first (objects are already plaintext in memory
  from decrypt-in-place at load).
- **Encryption waives minimal-diff by design** (decision 007 W8): every string
  and stream re-serialised through `EncryptingEncoder`; classic-table output,
  object streams promoted out. A signed document is REFUSED by name
  (`EncryptError::SignedDocument`) — never silently invalidated.
- **Criterion-7 branch chosen:** incremental save of a still-encrypted base is
  REFUSED (`WriteError::EncryptedSaveUnsupported`), not append-with-existing-key.
  If a future ask needs the latter, it is a new increment.
- **The write side is proven by the read side + pypdf 6.7.0** both directions.
  `enc-aes-256-r6.pdf` is now a decryption fixture, not refusal-only.

## §4 STATE OF THE EXPORT ARC (still current from v0.30.0)

- **One recording, three writers.** `record_page_for_export` (export mode of
  the display-list recorder, decision 132) feeds `svg.rs`, `emf.rs` and the
  raster path. Never build a writer over `vector::PageObjects`.
- **SVG** gradients native for axial and focal-radial; raster fallback for the
  rest, counted in `ExportTally`. Oracles resvg `=0.45.1` + Inkscape.
- **EMF** from `D:\dev\rag\emf\`; oracle is REAL GDI (`PlayEnhMetaFile`), NOT
  `System.Drawing` (mis-plays `EMR_ALPHABLEND`).
- `docs/core-api/03-capabilities.md` §7.7–§7.10 is that consumer contract.

---

## §5 THINGS A NEW SESSION MUST KNOW BEFORE TOUCHING ANYTHING

- **Push (`main`, fast-forward) and RELEASE are STANDING-AUTHORIZED**
  (decisions 090, 121) — no per-act go-ahead. `--force`, non-`main` branches,
  and non-release tags are NOT. Scrub `check-suite-name-absent.py` green
  before any push (public repo).
- **★ Cross-target check the CLI before pushing a `cfg(windows)` change**
  (`cargo clippy -p pdfcer-cli --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`);
  a skipped one cost the dead `v0.29.0` tag.
- **A filing commit carries ONLY docs.** A version-bump chore commit is a
  separate code commit — file it with the release filing.
- **`run-gates.sh` FOREGROUND with a warm cache.** Backgrounding it gets it
  killed (~10 min) and 0xc0000142 doctest failures look like real ones.
- **Patch scripts through the Write tool**, plain string anchors, every anchor
  asserted; anchor on the DOC BLOCK not the `fn`, and a `//` line between the
  `///` doc and the item makes `check-public-fns-documented.py` see it as
  undocumented (bit this Pass — `to_encrypt_dict`). Inserting a method BEFORE
  another orphans the later one's doc (bit this Pass — `parse`).
- **`cargo fmt` collapses `\`-continued string literals** into one physical
  line, baking the leading whitespace into a gap — `check-string-gaps.sh`
  catches it. Write long `#[error(...)]` messages as ONE line.
- **The librarian has no shell** — paste exact commit hashes into its dispatch.

## §6 MEASURED NEGATIVES — DO NOT RE-DERIVE

1. `/R` 6's only interoperable A13 reading is `PerformThenTest`; the other
   reading fails to open a pypdf/Acrobat file. Kept switchable for tests only.
2. Inkscape's CLI has no `paste`; Word's `PasteSpecial(wdPasteEnhancedMetafile)`
   via COM hung.
3. resvg 0.48 pulls a duplicate tiny-skia/zune-jpeg — the duplicates guard
   refuses it; `=0.45.1` is the last on tiny-skia 0.11.
4. GDI+ (`System.Drawing`) mis-plays `EMR_ALPHABLEND`; real GDI is exact.
5. Rename debt unchanged: `pdfceF{n}` is a deliberate keep; `pdfce-gui`,
   `pdfce@cce414e:` citations, `pdfce_FeatureRequests` are NOT to be renamed.

## §7 ITEMS OWED BY THE OPERATOR

- Global `C:\Users\Ken\.claude\CLAUDE.md` still references `D:\Dev\pdfce\`.
- Open questions `(ca)`, `(cb)`, `(cc)` — unchanged.
- The XFA read/fill re-scoping (open question `(p)`) and the OCR model-file
  licence call (`(bl)`) remain the operator's.
