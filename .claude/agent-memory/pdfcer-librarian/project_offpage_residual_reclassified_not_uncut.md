---
name: project-offpage-residual-reclassified-not-uncut
description: Pass 297.0's owed 12 off-page objects are a scan-classification gap (geometry vs ink), not an incomplete cut; register wording corrected in ROADMAP.md + FEATURES.md; pattern-naming declined at n=2
metadata:
  type: project
---

`7a22c523` (2026-09-12, 523rd filing, docs-only) inverted the standing assumption about
`Pass 297.0`'s owed 12 `partial` off-page residuals: `redact_image::covered_cells` snaps
OUTWARD, so the off-page samples of a crossing placement are already blank — the cut is
complete. `scan-offpage` still reports them because it classifies by GEOMETRY (bounding
box crosses the page edge), not by ink. Same shape as `Pass 294.2`'s empty-text-husk fix,
one type over: a classifier re-detecting its own successful output.

**Why:** `ROADMAP.md`'s `Pass 297.0` entry and `FEATURES.md:331` both said "a cut that
leaves a sliver," which reads as an incomplete cut — misleading for any future reader
hunting a cutting defect. Both corrected in place, struck-and-visible, pointing at the
`7a22c52` entry.

**Declined to mint a standing rule** for "classifier counts geometry where the operator's
question is about ink" at n=2 (`Pass 294.2` + this). The two share a symptom but diverge
on remedy — 294.2's fix was cheap, this one is refused on measured decode cost
(`Pass 294.1`: decoding every image during a scan cost ten minutes on one 6.9 MB file).
Flagged for the engineer's judgement rather than named, consistent with this role's
pattern-naming discipline ("would fixing one have prevented the other?" — arguable, not
demonstrated).

**How to apply:** if a third instance of "a scan/classifier re-detects its own correct
output because it measures structure instead of effect" surfaces, that is likely the
threshold to name it — check [[feedback_pattern_naming_needs_shared_mechanism_not_shared_moral]]
before doing so. Also: `docs/NEXT_SESSION.md` still carries the uncorrected "sliver"
wording as of this filing — engineer-owned, flagged not edited; verify it was fixed
before citing that file's off-page section as current.
