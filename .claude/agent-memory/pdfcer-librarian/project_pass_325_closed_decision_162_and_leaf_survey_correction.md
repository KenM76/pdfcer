---
name: project-pass-325-closed-decision-162-and-leaf-survey-correction
description: Pass 325.0 (core crate split) fully closed 2026-09-26, decision 162, 605th filing — a "leaves exhausted" claim from the prior filing was itself wrong
metadata:
  type: project
---

`Pass 325.0` (split `pdfcer-core`) closed 2026-09-26 at the 605th filing,
`3057b06c`, decision `162` (`ARCHITECTURE.md` §12). Final shape: `pdfcer-model`
at the bottom; seven leaves depending only on it (`image-codec`, `fonts`,
`pkix`, `color`, `text`, `function`); `pdfcer-core` is the facade + session
layer (`edit`, left unsplit — one strongly-connected component, and
`pdfcer-render` depends on core's `settings`/`annot`/`text_edit`/`edit`
directly so splitting further wouldn't shrink render's own rebuild anyway).

**Why worth remembering:** the 603rd filing's own "leaves exhausted" verdict
for step 3 was wrong — a ninth leaf (`pdfcer-function`, 5.5k lines) was found
one filing later, missed because that survey inspected modules already
suspected of being leaves rather than computing every module's `crate::` edge
set mechanically. RAG lesson written at
`D:\dev\rag\rust\find_leaf_crates_by_computing_every_modules_edges_not_by_inspecting_suspects.md`
(includes the one-liner recipe: `grep -ohE "crate::[a-z_0-9]+"` per module,
`sort -u`, minus itself).

**How to apply:** when a future filing reports "candidates exhausted" for
ANY exhaustive-sounding survey (leaf modules, dead code, orphaned tests,
etc.), treat "worked the named list to completion" and "surveyed everything"
as different claims — ask whether the list was assembled by inspection or by
a mechanical sweep before accepting "exhausted" at face value. This filing
had NO shell (relayed from the dispatching engineer per hard rule 8); did not
independently verify any of the commit hashes or line counts.
