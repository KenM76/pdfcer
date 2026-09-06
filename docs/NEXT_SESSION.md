# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-06 (UTC; the 2026-09-05 late-evening session). **Released:**
**v0.41.0** at `c4e39e7` (GitHub release + OneDrive `pdfcer1`; `pdfcer2` holds
0.40.0). `tools/verify-release.py v0.41.0` nine of nine ok. CI run
34007817244 green 10/10. **Ledger after the 449th filing:** filings 449, Pass
ceiling **257.0**, decisions **138** (next 139), rules R241 (next R242).
Workspace version is `0.41.0` (bump to `0.42.0` at the next release).
**Nothing is unreleased on `main`.** Both FeatureRequests channels are
empty of unanswered inbound (`pdfce_FeatureRequests\open` holds only my
replies; `iccce_FeatureRequests\open` likewise).

---

## What this session shipped (all in v0.41.0)
- `5e95805` **Pass 257.0** — every text-edit planner takes `&DocumentView<'_>`
  and every `EditSession` verb passes `self.view()`, so a `/Font` that
  `format_text` authored this session is typeable by the next `edit_text`
  (pdfcer-gui's three measurements are `tests/session_graph_resolution.rs`).
  Handing a planner `&self.base` is now a COMPILE ERROR. `reflow_block`'s two
  "save and reopen before reflowing" refusals (page set changed; content
  already edited — trap T-14) are REMOVED; only the Pass 251.0 appended-run
  refusal remains. Public signature change: `text_edit::forms::{scan_page_forms,
  invocation_set, form_objects_on_page}` lost their leading `&Document`.
  SHIPPED reply posted and marked released.
- `56dde4d` Pass 256.1 (per-character `/ToUnicode` refusal), `5f9beb3` Pass
  142.2 (font pre-flight for the candidate text + standard 14), `1343f0e` Pass
  256.0 (edit across show operators), `8670523` Pass 14.5 (`hit_test`
  presence), `3ae1fb4` two stale-wording survivors.

## THE NEXT WORK — in order

### 1. Pick from *Next up* / *Backlog* in `docs/ROADMAP.md` — the inbound queue is EMPTY
Candidates, in the order I would take them: `10.14` (signing hardening: CMS
sabotage tests, composed visible appearance, content-identity run), `10.12`
(certifying signatures `/DocMDP`), `10.13` (sign into a pre-placed empty
`/Sig` field), `10.10`/`10.11` (shell-side signers; B-T timestamp), `142.0`
(the embedded-donor half of FF-C: `format_text` to a face that is neither on
the page nor standard-14). A smaller one now within reach: let `reflow_block`
COMPOSE with an appended run instead of refusing (the plan reads the view now;
what remains is re-emitting the block across contents[0] and the extras
rather than sweeping the extras).

### 2. Check `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open` and `iccce_FeatureRequests\open` first
pdfcer-gui files requests there at any hour; three landed during this session.
`D:\Dev\FeatureRequests\pdfcer-gui\` is a GUI-shell REVIEW handoff (2026-09-03,
REVIEW.md + mockups) addressed to the pdfcer-gui project — nothing in it is
owed by core; do not re-read it as an inbound.

## Gotchas this session paid for (all recorded in agent memory)
- **Reverting git verbs in a chain — RECURRED.** `git checkout -- <file>
  2>/dev/null; echo "NO — do not checkout"` typed into a test command reverted
  the whole uncommitted `edit.rs` (2026-09-02 shape, character for character).
  Recovered because every edit was a `%TEMP%` script. New rule: a sabotage
  script contains its own revert; any `checkout/restore/reset/clean` is the ONLY
  command in its call. A PreToolUse hook is the structural fix — proposed to Ken.
- Python patch scripts: a Rust `\`-continuation in a `'''…'''` literal is eaten
  by Python itself; build the line with `chr(92)`. `tools/check-string-gaps.sh`
  is the detector, run after `cargo fmt`.
- `tools/run-gates.sh` after a VERSION BUMP exceeds the 10-minute foreground
  limit and gets moved to background (this time it survived; usually it is
  reaped). Pre-warm: `cargo clippy --workspace --all-targets`, then `cargo test
  --workspace --no-run`, then `cargo test --workspace` (≈4 min warm), THEN
  run-gates.
- The pre-push hook re-runs the gates, so `git push` sits silent for minutes
  and looks hung; use `GIT_TERMINAL_PROMPT=0` and a 3-minute timeout, alone.
- `format_twins.pdf`: `FontSelector::new("Times-Bold")` binds `/FB1` (the
  `/Differences` twin) — name `/FB2` by resource key in tests.
- A librarian filing can find rule-11 survivors in `crates/` that are the
  engineer's to fix; fix them before the release commit, not after.

## Signing state (unchanged this session)
PAdES B-B from a `.pfx` (Passes 10.7–10.9, v0.40.0): `sign::{der_out,
cms_build, pkcs12, apply}`, `EditSession::sign`, `pdfcer sign`. Tests
cross-check with `openssl cms -verify`. Level is always B-B. Encrypted docs
refused outright. RSA refuses on wasm32 (no blinding entropy); ECDSA works
there. Crate stack = decision 137 (`rsa 0.10 rc` under the open Marvin
advisory, accepted: signing never runs the decryption oracle). Memory:
`.claude/agent-memory/pdfcer-engineer/project_signing_arc_state.md`.

## The red push of v0.40.0, and the rule it left
`rsa`'s optional `sha2` dependency has `features = ["oid"]`, which the
decision-039 guard forbade. Decision 138 admitted `oid`. **That step is
CI-only** — `check-ci-parity.py --list` marks 3 steps as such. Before pushing
any `Cargo.toml` change: `cargo tree -p pdfcer-core -e features | grep -E 'aes
feature|sha2 feature'` and compare with `.github/workflows/ci.yml`. **Never tag
a release before CI is green on the pushed tree.**

## Build environment — READ before any release build
This box has **~4.4 GB free RAM**. Release build: **`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
CARGO_BUILD_JOBS=2 cargo build --release -p pdfcer-cli`** (~6 min with a warm
cache). Long BACKGROUND processes get reaped: never background `cargo test
--workspace` or `run-gates.sh`; `gh run watch` too — poll `gh run view --json
status,conclusion` in a foreground loop (CI ≈ 13 min).

## Release procedure (worked 2026-09-05 twice and 2026-09-06)
bump `Cargo.toml` version (`cargo metadata --offline` for both lockfiles,
incl. `fuzz/Cargo.lock`) → chore commit → librarian filing for any unfiled
code commit (`check-commits-filed.py`; the tip is allowed to be deferred) →
PUSH (hook runs gates) and poll CI green → `git tag -a vX -m … <sha>` → rebuild
(the banner reads `git describe`) → `tools/package-portable.py --no-build
--note "…"` → fresh-folder smoke test (copy the build dir, run `--version`,
`sign` with `fixtures/synthetic/signing/rsa2048-modern.pfx` password `pdfcer`,
`verify-signatures`, `edit-text`) → zip with Python `zipfile` + sha256 → `git
push origin vX` → `gh release create vX zip sha256 --title … --notes-file …` →
`tools/deploy-onedrive.py` → `tools/verify-release.py vX` → librarian release
filing → refresh this file.

## Standing habits
- Check BOTH FeatureRequests channels every session.
- A new public TYPE or a changed public SIGNATURE gets announced on the
  channel by name (pdfcer-gui's gates key on verbs and FEATURES rows only).
- Anchor a splice on the DOC BLOCK, not the item.
- Batch releases: build everything pending, then ONE portable release
  (operator, 2026-09-05).
