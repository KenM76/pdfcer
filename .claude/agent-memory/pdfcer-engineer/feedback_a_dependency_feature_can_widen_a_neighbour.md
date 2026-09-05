---
name: a-dependency-feature-can-widen-a-neighbour
description: A new crate's FEATURE can turn on a feature of a crate you already audited (rsa/sha2 → sha2/oid); the CI dependency-feature guard is CI-ONLY and caught a red push on 2026-09-05 — run `cargo tree -e features` locally before pushing any Cargo.toml change
metadata:
  type: feedback
---

# Run `cargo tree -e features` locally when a Cargo.toml changes — the feature guard is CI-only

**Rule:** before pushing any change to a `Cargo.toml`, run
`cargo tree -p pdfcer-core -e features | grep -E 'aes feature|sha2 feature'`
(and any other crate a decision record fences) and compare against the CI
guards in `.github/workflows/ci.yml`. `check-ci-parity.py --list` marks that
step "genuinely CI-only" — the local gate sweep does NOT run it.

**Why:** 2026-09-05, the signing arc added `rsa` with its `sha2` feature. That
feature enables `sha2/oid` (= `digest/oid` = const-oid trait impls). Decision
039's CI step forbids any extra feature on `sha2` without a new decision record,
so the push of a 7-commit batch went RED on the one job the local sweep cannot
run, after every local gate was green and a release build was already
packaged and locally tagged. The fix was correct-by-process (decision 138 +
guard amended) but it cost a rebuild, a retag and an hour.

**How to apply:** a dependency line is not just "one more crate" — it is a set
of feature edges into crates already in the tree. When adding a crate, read its
`Cargo.toml` `[features]` for `dep-name/feature` entries that touch anything a
decision record fences (`aes`, `sha2`, image codecs). If one does, mint the
decision BEFORE pushing and amend the guard in the same batch. Do not tag a
release until CI on the pushed tree is green — a local tag on a red tree is a
claim the tree cannot back.

Related: [[gates-i-owe-myself]], [[a-gate-sweep-certifies-the-tree-it-ran-on]].
