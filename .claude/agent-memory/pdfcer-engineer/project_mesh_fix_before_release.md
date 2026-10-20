---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 goal "fix the 3D compressed-mesh failures" is MET 2026-10-20 (614.0) — door 155/1/0, trailer 127/14/0; what the fixes had in common and how to judge any future mesh rule
metadata:
  type: project
---

Ken's goal, set 2026-10-16: fix the 3D compressed-mesh decode failures
(PRC 8137 compressed tessellation, his door and trailer samples) before
anything else. **It was met on 2026-10-20 by Pass 614.0.** On the census,
the door is 155 unique / 1 best-fit / 0 failed and the trailer is
127 / 14 / 0. The feature-request loop is unblocked.

**Trajectory.** Trailer U/B/F:
- v0.84.0: 117/19/5
- 603.0–605.0: 127/10/4
- 606.0: m48 and m63 fixed
- 607.0: m107 F→B
- 613.0: m64; a borrowing planar face signals only its tall triangles
- 614.0: m95; the event search may add a choice up to EVENT_BACK 8 before the set's last, budget 5M

No mesh got worse at any step. The ledger is the spec RAG
`threed/prc__8137__tess_3d_compressed.md`, R17 and X42–X78.

**Lessons that held:**
- Each fix was either the encoder's exact arithmetic in a rare branch, or the search failing to reach a choice. None was a looser threshold. A looser threshold fails through stage-deepening ambiguity.
- A failure that "no flip fixes" (an earlier X67 claim for m64/m95) can be a search-ordering artefact. Two choices can be jointly needed when the earlier one is inert alone. Probe this with drop-one, add-one and swap scans (ev_swap) before calling a class unfixable.
- A sibling instance of the same part is an exact oracle.

**Still open, not failures:**
- "Best-fit" is not "right". m52 and m96 are mirrored best fits.
- Fold-backs exist even in unique fits (X50/X51).

**How to apply:**
- Judge any mesh-rule change by the per-mesh U/B/F census on a release build against the previous state. No mesh may get worse.
- Never read nanoPRC, libPRC, Asymptote, MuPDF, Ghostscript or Poppler source (decision 169).
- Never commit probe modules (`// P603 LOCAL`), real PDFs or dumps.
