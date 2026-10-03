---
name: project-v073-released-890th-filing
description: v0.73.0 RELEASED (890th filing) — first release with OCR add-on zips; dirty-build/binary-stamp lesson recurred a 2nd time
metadata:
  type: project
---

`v0.73.0` RELEASED 2026-10-02, tag at `7f83ef86`, completing the 889th
filing's "RELEASE IN PROGRESS" entry (`ROADMAP.md`, `SESSION_LOG.md` 890th
filing). First release shipping non-PaddleOCR engines (ocrcer, ocrs,
tesseract) as separate `<build>-ocr-addon-<name>.zip` GitHub release assets
— base portable now bundles only PaddleOCR. OneDrive slot `pdfcer2` updated
(`pdfcer1` keeps `v0.72.0`); next release writes `pdfcer1`.

**Dirty-build-vs-binary-stamp disagreement recurred a 2nd time** (first was
`v0.72.0`, same filing shape): `package-portable`'s folder-name dirty check
and the binary's build-script version stamp check different path sets —
uncommitted agent-memory files don't trip the folder name but do trip the
stamp. Expect this to recur again on a 3rd release unless packaging is
fixed to check the same tree the stamp does.

**Owed, not a release blocker:** renderer reports a false "structural
oddity" for a form XObject with `/Resources <<>>` — pdfcer's own `add_emf`
writes exactly that for shapes-only EMF pictures, so pdfcer's own valid
output triggers the oddity warning on itself.

**Why this matters for the next filing:** check whether the
`/Resources <<>>` fix shipped before citing it as still-owed, and watch
for a 3rd dirty-build occurrence — if it recurs, it's worth a RAG finding
in `D:/dev/rag/rust/` (packaging-script dirty-check scope mismatch), not
just a 3rd session-log repetition.
