# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-06, session close, on the operator's instruction ("make a
handoff doc … stop all work after making the handoff"). **Released:**
**v0.42.0** at `e59b084` (GitHub release + OneDrive `pdfcer2`; `pdfcer1` holds
0.41.0); `verify-release.py v0.42.0` nine of nine. **Unreleased on `main`:**
`b64ddb6` **Pass 14.6** (a Type 0 font's `/FontDescriptor` is read from the
descendant CIDFont — composite runs report embedded and the R-INV-1 subset
floor now guards them; filed 456th `f9ed0dc`). **Confirmed green after
close:** `origin/main` = `821ab47` (this handoff commit, pushed) — `gh run
list -L 1` shows `success` on it, 15m23s. Workspace version `0.42.0` (bump to `0.43.0` at
the next release). Ledger: filings 456, Pass ceiling 257.0, decisions 138,
rules R241. Batch rule (operator, 2026-09-05): build everything pending, then
ONE release.

---

## THE NEXT WORK — IN THIS ORDER (operator's instruction, 2026-09-06)

### 1. "The font thing" — `Pass 142.0`, the embedded-donor half of FF-C
`format_text` / `format-text --set-font <face>` to a face that is **neither on
the page nor standard-14**: subset + embed a donor font program supplied via
`--font-dir` (decision 012) into the document, then re-encode the run into it.
This is **rung 3** of the automatic style ladder (`Pass 179.0` ships with rung
3 absent and grows it when 142.0 lands — a `--bold` on a `Verdana` run with
`Verdana-Bold` in `--font-dir` should bind it). Entry: `docs/ROADMAP.md`
~line 116195 (`Pass 142.0` / `142.1`, "DE-PRIORITISED, NOT CLOSED" — the
operator has now re-prioritised it to FIRST; tell the librarian so the entry's
status line moves). Facts to carry in:
- `add_text --embed-font` ALREADY subsets and embeds a donor face for NEW
  text (§9.6.4 ST1–ST4) — reuse that path (`text_edit/addtext.rs` and the
  embed/subset modules it calls); the missing piece is binding it from
  `plan_font` (`text_edit/format.rs`, the `created` payload) when
  `resolve_target_resource` misses and `std14_by_base_font` misses too.
- The coverage gate (`accept_font_target`, R221) must run against the
  DONOR's cmap: every character of the run must have a glyph in the donor,
  else refuse by name (never `.notdef`).
- Report + disclosure: a NEW embedded font program was added (bytes, subset
  tag, face name) — rule 4; CLI prints it; `SignatureImpact` must treat a new
  font object like any other addition.
- Tests: fixture `fixtures/synthetic/text/subset-donor.ttf` exists (a donor
  TrueType); `format_twins.pdf`/`format_other.pdf` for the "not on page, not
  std14" refusal that must now become a bind. Style-ladder test
  `per_axis_a_real_bold_binds_while_italic_is_synthesised` documents the
  Verdana case rung 3 will change — update it deliberately.
- Public surface: probably `FormatRequest.font_dirs`/`donor` on the request
  (the CLI already has `--font-dir` for rendering/measurement), a
  `FormatReport.embedded_font: Option<…>` disclosure, `StyleRung::DonorFace`.
  Announce on the channel by name.

### 2. NEW — "resize page contents" (a page-ops Pass family; IDs not minted yet)
Operator, verbatim: *"add a resize page contents with all the usual options
(scale to fit without distortions, to fill page without distortion, to fill
page with distortion, set custom size, etc)."* Nothing exists in ROADMAP or
FEATURES for this (grep `resize page|scale to fit` → nothing). **Session
start: dispatch `pdfcer-acrobat-librarian` first** (rule 12 — what Acrobat's
page-scaling / set-page-boxes / resize capabilities actually do: anchors,
uniform vs non-uniform scale, whether the page box or the content moves,
rotation interplay, annotations/form widgets scaling with content, links,
what happens to `/MediaBox` vs `/CropBox`), then dispatch `pdfcer-librarian`
to mint the Pass IDs and file acceptance criteria. The engineer's read of the
shape, to be checked against the RAG, not assumed:
- Options: **fit** (uniform scale so content fits the target box, no
  distortion, centred / anchored), **fill without distortion** (uniform scale
  to cover the box, overflow clipped), **fill with distortion** (non-uniform
  scale to the box exactly), **custom size** (target page size — presets
  Letter/A4/… and explicit W×H, plus a percentage scale), keep or change the
  page box, anchor (9 positions), margins.
- Mechanism: the minimal-diff way is a `cm` prefix on the page's content
  (wrapped `q … Q`, prepended stream — the `Pass 3.2`/`text_edit_command`
  content-object machinery) plus `/MediaBox`/`/CropBox` rewrites; annotation
  `/Rect`s and widget appearances transform with the content (the
  `move_annotation`/`transform` verbs and `annot_author` re-bake); ce
  dimensions' `/Measure` must be re-scaled or refused by name; a signed
  document refuses (SignatureImpact — content change under a signature).
- Both shells: `EditSession::resize_page_contents(page, spec)` (one undo
  entry per page, or one per batch) and `pdfcer resize-pages --pages … --mode
  fit|fill|stretch|size --size Letter|A4|WxH --anchor … --scale N%`.
  Fuzzy-never-sneaky: the report says the scale factors applied per page.
- Round trip: untouched objects byte-identical; a page NOT selected is
  untouched.

### 3. Finish B-T timestamps — `Pass 10.11` (+ its shell half)
Entry: `docs/ROADMAP.md` ~line 133719. Core embeds a supplied RFC 3161
token as the `id-aa-timeStampToken` unsigned attribute (the hole must be
sized for it — `SignRequest.reserve` grows); the TSA round trip is the CLI's
(operator-initiated network, decision 061 — `--tsa <url>`); the level printed
becomes `B-T` and the time's SOURCE is printed. The seed-value evaluator
(`apply::check_seed_value`) currently REFUSES a required `/TimeStamp` by name
("B-T not built") — flip that branch to honour it, and let a recommended
`/TimeStamp /URL` seed the `--tsa` default (disclosed). Verification:
`signature_verify` should surface the token's time; OpenSSL `ts -verify` as
the oracle if a free TSA is reachable (freetsa.org) — record NOT MEASURED
otherwise, never as passed.

**Then:** the 0.43.0 batch release (procedure below).

## Check the inbound channels FIRST — by DIFF, not by newest file
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open` and
`iccce_FeatureRequests\open`: list every `request_*`/`correction_*` and match
each against a `reply_*`/`done_*`/notice that names it. A request sat
unscoped 13 h on 2026-09-06 because the check read only the newest file. At
close: every request answered; `done_…CONSUMED` from pdfcer-gui covers
256.0/256.1/257.0; five notices + two replies posted this session.
`D:\Dev\FeatureRequests\pdfcer-gui\` is a GUI-shell review addressed to the
pdfcer-gui project — nothing owed by core.

## Gotchas this session paid for (all in agent memory / RAGs)
- **Disk hit ZERO**: `target/debug/deps` 141 GB of stale test binaries +
  45 GB incremental. First symptom was a rustc `STATUS_ACCESS_VIOLATION`.
  `du -sh target/debug/deps target/debug/incremental` at session start;
  prune when deps > 20 GB (`D:/dev/rag/rust/cargo_target_dir_…`).
- Insert-before-anchor orphaned a doc block FIVE times; anchor on the DOC
  BLOCK (walk back over `///`/`#[`), or insert after a closing brace;
  `check-public-fns-documented.py` catches the fn case.
- git's auto `%h` grew to 8 chars on the fresh CI clone → every filed commit
  looked unfiled; gates now pin `--abbrev=7` (`2da1d62`).
- Sabotage-and-revert is ONE Python script; reverting git verbs run ALONE.
  A PreToolUse hook in `~/.claude/hooks/` refuses them inside chains — and it
  matches the words anywhere in the command text, including heredoc bodies,
  so write prose that mentions those verbs with the Write tool, not Bash.
- Rust `\` continuations inside Python literals: build with `chr(92)`;
  `check-string-gaps.sh` detects the damage.
- `run-gates.sh` as one process exceeds the 10-minute foreground limit;
  pre-warm `cargo test --workspace --no-run`, run tests, then loop the
  non-cargo gates from `run-gates.sh --list`.
- OpenSSL 1.1.1 and pdfcer both verify signedAttrs AS RECEIVED (an unsorted
  set signed consistently is accepted; reordering after signing is caught);
  spec RAG CB-4 split accordingly — sort before signing, never reorder.
- The generator `gen-cidfont-nocmap-fixtures.py` is NOT byte-deterministic:
  regenerate, then restore the untouched siblings with a lone revert call.
- The librarian finds rule-11 survivors in `crates/` on every filing; fix
  them in the NEXT commit (zero owed at close).

## Signing state (memory `project_signing_arc_state.md` is current)
Shipped: 10.7–10.9 (B-B from .pfx), 10.14 (hardening, composed appearance,
P-384, pyHanko fixtures both directions), 10.12 (certify / DocMDP), 10.13
(sign into a pre-placed field; `/Lock`→`/FieldMDP`; total `/SV`
enforcement). Not built: B-T (item 3 above), B-LT/LTA, `/SV /Cert`, `/Kids`
signature fields, shell signers (store / PKCS#11), authoring an empty
signature field.

## Build environment — READ before any release build
~4.4 GB free RAM. Release build: `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
CARGO_BUILD_JOBS=2 cargo build --release -p pdfcer-cli` (~6.5 min warm). Never
background `cargo test --workspace`, `run-gates.sh` or `gh run watch`; poll
`gh run view --json status,conclusion` in a foreground loop (CI ≈ 13 min).

## Release procedure (worked 2026-09-05 twice and 2026-09-06 twice)
bump `Cargo.toml` version (`cargo metadata --offline` for the root and fuzz
lockfiles; `tools/content-identity/Cargo.lock` needs `cargo metadata` WITHOUT
`--offline` once) → chore commit → librarian filing for every unfiled code
commit (the push hook's commits-filed gate allows only the TIP unfiled) →
PUSH (`GIT_TERMINAL_PROMPT=0`, alone, 10-minute timeout) → poll CI green →
`git tag -a vX -m … <sha>` → rebuild → `tools/package-portable.py --no-build
--note "…"` → fresh-folder smoke test (`--version`, `sign` with
`rsa2048-modern.pfx` password `pdfcer`, `verify-signatures`, `edit-text`, plus
the new verbs) → zip via Python `zipfile` + sha256 → `git push origin vX` →
`gh release create` → `tools/deploy-onedrive.py` → `tools/verify-release.py
vX` → librarian release filing → refresh this file.

## Standing habits
- Check BOTH FeatureRequests channels every session, by diff.
- Announce every new public TYPE/SIGNATURE on the channel by name.
- Anchor a splice on the DOC BLOCK, not the item.
- Batch releases: build everything pending, then ONE portable release.
- `du -sh target/debug/deps` at session start.
