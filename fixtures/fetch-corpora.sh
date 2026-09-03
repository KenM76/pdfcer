#!/usr/bin/env bash
# Pulls confirmed-safe, redistribution-cleared PDF test corpora into
# fixtures/external/ (scratch space — gitignore it, don't commit it
# wholesale; hand-pick individual files into the tracked fixtures/
# tree instead, with a one-line provenance note per file).
#
# See fixtures/README.md for the source table and licensing notes,
# and docs/LEGAL.md §5 for the binding fixture-sourcing rule this
# script exists to support.
#
# Sources verified live 2026-07-23. Re-verify URLs if this script has
# sat unused for a while — repos move/get archived.

set -euo pipefail

DEST="$(dirname "$0")/external"
mkdir -p "$DEST"

echo "==> veraPDF corpus (PDF/A conformance test files)"
if [ ! -d "$DEST/veraPDF-corpus" ]; then
    git clone --depth 1 https://github.com/veraPDF/veraPDF-corpus.git "$DEST/veraPDF-corpus"
else
    echo "    already present, skipping (rm -rf to re-fetch)"
fi

echo "==> PDF Association PDF 2.0 examples"
if [ ! -d "$DEST/pdf20examples" ]; then
    git clone --depth 1 https://github.com/pdf-association/pdf20examples.git "$DEST/pdf20examples"
else
    echo "    already present, skipping (rm -rf to re-fetch)"
fi

echo "==> PDF Association corpora index (discovery only, not a direct file source)"
if [ ! -d "$DEST/pdf-corpora-index" ]; then
    git clone --depth 1 https://github.com/pdf-association/pdf-corpora.git "$DEST/pdf-corpora-index"
else
    echo "    already present, skipping (rm -rf to re-fetch)"
fi

cat <<'EOF'

Done. Next steps (do NOT commit fixtures/external/ wholesale):
  1. Browse fixtures/external/*/ for files that exercise a specific
     pdfcer-core code path you're testing.
  2. Copy the specific file(s) you need into fixtures/<category>/,
     with a one-line comment (in the test that uses it, or a sibling
     .md note) recording which source it came from and why.
  3. Add fixtures/external/ to .gitignore if it isn't already there.

Isartor test suite (PDF/A-1b conformance) is NOT included here — its
current download URL wasn't confirmed as of 2026-07-23 (see
fixtures/README.md). Check the PDF Association's site directly via a
browser before adding it to this script.
EOF
