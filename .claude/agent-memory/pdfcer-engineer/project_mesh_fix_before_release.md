---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 — the standing goal is the 3D compressed-mesh decode failures; 603.0 fixed door m2, 604.0 (class-split window) fixed trailer m108/m53; eight still fail (door m23/m39/m48, trailer m39/m63/m64/m95/m107); 'unique fit' meshes can still be wrong (fold-back census); spec RAG R17/X42–X51 is the ledger
metadata:
  type: project
---

The standing goal is the 3D mesh rendering failures (PRC 8137 compressed
tessellation, the operator's door and trailer samples). No feature-request
loop and no other Passes while it is open.

**State at 2026-10-16 evening:** Pass 603.0 (`5fbbbebd`, unreleased)
shipped the blind-normal rule (a record normal in the triangle's plane
makes its reversed bit noise, |cos| < 0.01) and narrowed `thin` to weak AND
collinear. Door 152 → 153/156 (m2 fixed); trailer 117/19/5 → 123 unique /
12 best-fit / 6 failed (m108 regressed best-fit → failed; m108 is the
first thing to examine next). The spec RAG
`threed/prc__8137__tess_3d_compressed.md` holds every hypothesis scored:
R17, X42–X49; X38/X41 closed. Research agent found no public non-GPL fold
rule; patent US 8,207,965 B2 expiry is Google-Patents-only, unverified.

**2026-10-18:** Pass 604.0 (`be4bbe81`, unreleased) split the search's
candidate window by class (40 unsignalled folds+turns, at most 20
signalled weak folds): trailer 125 unique / 11 best-fit / 5 failed, door
153/0/3 unchanged. Ken's observation confirmed by measurement: faces
before a reported failure are already wrong — a wrong fold still consumes
every stored number. A fold-back census (anti-parallel neighbour across
a shared edge, dot < -0.995, non-sliver) finds fold-backs in all 12
trailer best-fits, 27/123 trailer uniques (the nut family, ~74% through)
and 10/153 door; "unique" is not "right". Rejecting fold-backs in the walk
fails every such mesh (no flip or apex mirror rescues them; the nut
family's fold-back apexes are REUSED vertices → upstream traversal error),
and m44 (thin plate) folds back legitimately — the detector is a
diagnostic, not a rule. Probe scaffolding backed up in
`D:\Dev\pdfTests\prc-probe\p603\`.

**Why:** Ken 2026-10-16: "Your only goal before next release is to fix the
mesh rendering problems." then "release a new version and let pdfcer-gui
know of the changes so it can implement the options into the latest
version then continue trying to fix the issue." He allowed a web-research
agent on a lighter model for the lookup.

**How to apply:** work the fix; releases follow the normal standing
authorisation again (next one writes `pdfcer2`). Judge any rule change by
per-mesh U/B/F against the previous release binary in `D:/builds/`
(`3d-render` note lines), not by the fold it changes — a looser threshold
fails by stage-deepening ambiguity. Never read nanoPRC/libPRC/Asymptote/
MuPDF/Ghostscript/Poppler source (decision 169). Never commit probe
modules (`// P603 LOCAL`), real PDFs or dumps; the probe backup is in
`D:\Dev\pdfTests\prc-probe\p603\`.
