# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-05, end of the second session of the day. **Released:**
**v0.40.0** at `de0e963` (GitHub release + OneDrive `pdfcer2`; `pdfcer1` holds
0.39.0). `tools/verify-release.py v0.40.0` clean. **Ledger after the 441st
filing:** filings 441, Pass ceiling **256.1**, decisions **138** (next 139),
rules R241 (next R242). Workspace version is `0.40.0` (bump to `0.41.0` at the
next release). The batch the operator ruled on 2026-09-05 ("build all before
the next portable release") is DONE and shipped.

---

## Unreleased since v0.40.0 (goes into 0.41.0)
- `8670523` fix: `EditableTextModel::hit_test` returns `None` beyond one
  line-height of every line (pdfcer-gui request, measured at 1e9 pt); their
  click-on-blank-paper-to-add-text arm can now fire. Reply posted.

## THE NEXT WORK — in order

### 1. `Pass 256.0` — edit text ACROSS show operators (*Next up*) ← START HERE
Operator hit it on the first real document he tried to correct (`clien` →
`client`; the producer writes ONE show operator PER GLYPH, and `edit-text`
finds within one operator by contract). Acceptance criteria are in
`docs/ROADMAP.md` (439th filing). Facts to carry in:
- **Ask (b) already exists**: `EditRequest::pinned_span` + empty `find` =
  whole-operator target. Verified on his file (`C:\Users\Ken\OneDrive\pdfTests\
  apartment work - signed.pdf`, page 2, text object 12, last run
  `ByteSpan{7250,7}`): replace `"nt"` → "…with client". pdfcer-gui has been told
  (`reply_2026-09-05-one-glyph-per-operator-the-pin-already-fixes-his-typo.md`).
- Code map (`crates/pdfcer-core/src/text_edit/edit.rs`, 3629 lines):
  `find_anchor` (2286) picks ONE `OpRec` whose `ShowData.text` contains `find`;
  `match_run` (2461) maps the match to slots within ONE `TJ` element and refuses
  cross-element already ("cross-element edit deferred"); `plan_edit_target`
  (1652) does the re-encode + follower reflow; `same_line` (2133) is the
  follower test. 256.0 = an anchor that is a RANGE of consecutive `Show` recs
  (same font resource, `tf_size`, baseline, one text object / no MCID crossing),
  text concatenated for the `find`, replacement written into the operator
  holding the match END, matched glyphs removed from earlier operators, and the
  following operators' `Td`/`Tm` shifted by the net advance (the existing
  follower machinery). Add `EditReport.operators_spanned`. Fixture: a synthetic
  one-glyph-per-operator composite (generator under `tools/`), plus a `TJ`-split
  case. No new CLI flag; rewrite the four `--help` sentences that state the
  one-operator contract (`main.rs` ~6002/6086/22704/23528).
- Refuse by name where the span crosses an MCID or a font change.

### 2. Then: `Pass 256.1` (`/ToUnicode` partial inversion, Backlog), and the
signing gaps `10.12` (certifying `/DocMDP`), `10.13` (sign into a pre-placed
empty `/Sig` field), `10.14` (hardening: CMS sabotage tests, composed visible
appearance, content-identity run), `10.10` (Windows-store / PKCS#11 shell-side
signers — the `Signer` trait + `sign::verify_raw_signature` are ready for it),
`10.11` (B-T timestamp; the TSA round trip is the CLI's — first network client
in a shell, §1.1 README claim falls due).

---

## Signing arc — what is true now (so nothing is re-derived)
Shipped `7734261` (Passes 10.7–10.9): `sign::Signer`, `Pkcs12Signer` (both
PKCS#12 eras, MAC first), in-house DER writer + CAdES `SignedData`,
`EditSession::sign` (incremental, `/ByteRange` to EOF, self-verified with
`signature_verify`), `pdfcer sign`. Tests cross-check with `openssl cms
-verify`. Level is always B-B. Encrypted docs refused outright. RSA refuses on
wasm32 (no blinding entropy); ECDSA works there. Crate stack = decision 137
(`rsa 0.10 rc` under the open Marvin advisory, accepted on recorded reasoning:
signing never runs the decryption oracle; blinded paths only). Memory:
`.claude/agent-memory/pdfcer-engineer/project_signing_arc_state.md`.

## The red push, and the rule it left
The 7-commit batch push went RED on ONE CI step: `rsa`'s optional `sha2`
dependency has `features = ["oid"]`, which the decision-039 guard forbids on
`sha2`. Decision 138 admitted `oid` (const-oid trait impls only, needed for the
PKCS#1 v1.5 DigestInfo) and the guard regex moved. **That step is CI-only** —
`check-ci-parity.py --list` marks 3 steps as such. Before pushing any
`Cargo.toml` change: `cargo tree -p pdfcer-core -e features | grep -E 'aes
feature|sha2 feature'` and compare with `.github/workflows/ci.yml`. **Never tag
a release before CI is green on the pushed tree** — a local tag was made and
had to be deleted this time.

## Build environment — READ before any release build
This box has **~4.4 GB free RAM**. Release build: **`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
CARGO_BUILD_JOBS=2 cargo build --release -p pdfcer-cli`** (~6 min with a warm
cache). The environment reaps long BACKGROUND processes: `tools/run-gates.sh`
and `cargo test --workspace` cannot be backgrounded — `--no-run` first, then run
in the foreground (195 test binaries in < 10 min warm). `gh run watch` also gets
reaped in the background; poll `gh run view --json status,conclusion` in a short
foreground loop instead.

## Release procedure (worked 2026-09-05, twice)
bump `Cargo.toml` version → chore commit → PUSH and wait for CI green → tag at
that commit → rebuild (the banner reads `git describe`) → `tools/package-portable.py
--no-build --note "…"` → fresh-folder smoke test (copy the build dir, run
`--version`, `sign`, `verify-signatures`) → zip with Python `zipfile` (Compress-
Archive misquotes `$VAR` paths) + `sha256sum … > .sha256` → `git push origin
vX` → `gh release create vX zip sha256 --notes-file …` → `tools/deploy-onedrive.py`
→ `tools/verify-release.py vX` → librarian release filing.

## Standing habits this session reinforced
- Check BOTH FeatureRequests channels every session (`D:\Dev\FeatureRequests\
  pdfce_FeatureRequests\open`, `iccce_FeatureRequests\open`). Three replies were
  posted today; nothing new inbound at close.
- A new public TYPE gets announced on the channel by name (pdfcer-gui's gates
  key on verbs and FEATURES rows only) — done in the one-glyph reply.
- A Python patch script eats a Rust `\`-continuation even via the Write tool
  (Python's own line continuation); use raw strings.
- Anchor a splice on the DOC BLOCK, not the item.
- Never bundle code into a filing commit; a chore (even a lockfile) is a CODE
  commit for `check-commits-filed` — name it in the next filing.

## Not for the engineer to decide (operator's)
- Buy-me-a-coffee link in the MIT notice — operator's own task.
- Open question `(bl)` (CC-BY-SA OCR model) still stands.
