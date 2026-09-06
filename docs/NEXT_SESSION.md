# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-06, end of the third session of the batch. **Released:**
**v0.41.0** at `c4e39e7` (GitHub release + OneDrive `pdfcer1`; `pdfcer2` holds
0.40.0). **Unreleased on `main` since then — the 0.42.0 batch:** `187fa09`
Pass 10.14 (signing hardening), `72b7296` Pass 179.0 (automatic bold ladder),
`02bb1ba` Pass 10.12 (certifying signatures), `ab40127` Pass 10.13 (sign into
a pre-placed field), plus `9a3dd53`/`3ae1fb4`-class chores and the filings.
Workspace version is `0.41.0` (bump to `0.42.0` at the next release). Batch
rule (operator, 2026-09-05): build everything pending, then ONE release.

## THE NEXT WORK — in order

### 1. Cut the 0.42.0 batch release
The procedure below; four Passes are unreleased. Then refresh this file.

### 2. Pick from *Backlog* — the inbound queue is EMPTY
Candidates, in the order I would take them: `10.10`/`10.11` (shell-side
signers; B-T timestamp — the seed-value evaluator now names "B-T not built"
as a refusal, so a timestamp is the most-asked-for gap), `142.0` (the
embedded-donor half of FF-C: `format_text` to a face neither on the page nor
standard-14 — rung 3 of the style ladder), `/SV /Cert` evaluation (Table 235,
refused by name today), non-merged (`/Kids`) signature fields, a forms verb to
AUTHOR an empty signature field.

### 3. Check `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open` and `iccce_FeatureRequests\open` first
pdfcer-gui consumed 256.0/256.1/257.0 (their `done_…CONSUMED.md`); four
notices posted this session (sign appearance, automatic bold, certifying,
sign-into-field). `D:\Dev\FeatureRequests\pdfcer-gui\` is a GUI-shell review
handoff addressed to the pdfcer-gui project — nothing owed by core.

## Gotchas this session paid for (all in agent memory too)
- **Insert-before-anchor orphans the anchor's doc block — FOUR times this
  session** (`looks_like_pdf_date`, `std14_by_base_font`, `RealFaceAvailable`,
  `is_none`, `locate_hole`). The public-fns gate catches it; the fix is to
  anchor the insert on the DOC BLOCK start (walk back over `///`/`#[` lines).
- Sabotage-and-revert is ONE Python script; never a git verb in a chain
  (recurred 2026-09-06, hook installed in `~/.claude/hooks/`).
- A Rust `\` continuation inside a Python literal is eaten by Python; build
  with `chr(92)`; `check-string-gaps.sh` is the detector.
- `run-gates.sh` as one process exceeds the 10-minute foreground limit;
  pre-warm `cargo test --workspace --no-run`, run tests, then loop the
  non-cargo gates from `run-gates.sh --list`.
- Measured, not assumed: OpenSSL 1.1.1 and pdfcer both accept an UNSORTED
  signedAttrs SET when signed consistently (as-received rule); the spec RAG's
  CB-4 was split accordingly. Sort before signing; never reorder after.
- `format_twins.pdf`: name `/FB2` by resource key; `Times-Bold` binds the
  `/Differences` twin.
- The librarian finds rule-11 survivors in `crates/` on every filing; fix them
  in the next commit, not later (zero owed at close).

## Signing state
PAdES B-B from a `.pfx` (10.7–10.9), hardened (10.14: CMS sabotage tests, no-
localKeyId pairing, pyHanko foreign fixtures both directions, P-384, composed
visible appearance), certifying signatures with DocMDP (10.12), signing into a
pre-placed field with `/Lock`→`/FieldMDP` and total `/SV` enforcement (10.13).
Not built: B-T timestamp, B-LT revocation, `/SV /Cert`, `/Kids` fields, shell
signers (store/PKCS#11). Memory: `project_signing_arc_state.md` (refresh it).

## Build environment — READ before any release build
This box has **~4.4 GB free RAM**. Release build: **`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
CARGO_BUILD_JOBS=2 cargo build --release -p pdfcer-cli`** (~6 min warm). Never
background `cargo test --workspace`, `run-gates.sh` or `gh run watch`; poll
`gh run view --json status,conclusion` in a foreground loop (CI ≈ 13 min).

## Release procedure (worked 2026-09-05 twice and 2026-09-06)
bump `Cargo.toml` version (`cargo metadata --offline` for both lockfiles,
incl. `fuzz/Cargo.lock` and `tools/content-identity/Cargo.lock`) → chore
commit → librarian filing for any unfiled code commit → PUSH (hook runs gates;
`GIT_TERMINAL_PROMPT=0`, 3-minute timeout, alone) → poll CI green →
`git tag -a vX -m … <sha>` → rebuild → `tools/package-portable.py --no-build
--note "…"` → fresh-folder smoke test (`--version`, `sign` with
`rsa2048-modern.pfx` password `pdfcer`, `verify-signatures`, `edit-text`) →
zip via Python `zipfile` + sha256 → `git push origin vX` → `gh release create`
→ `tools/deploy-onedrive.py` → `tools/verify-release.py vX` → librarian
release filing → refresh this file.

## Standing habits
- Check BOTH FeatureRequests channels every session.
- Announce every new public TYPE/SIGNATURE on the channel by name.
- Anchor a splice on the DOC BLOCK, not the item.
- Batch releases: build everything pending, then ONE portable release.
