---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 — the standing goal is the 3D compressed-mesh decode failures; 603.0 (blind-normal rule) fixed door m2, nine meshes still fail (door m23/m39/m48, trailer m39/m63/m64/m95/m107/m108); spec RAG R17/X42–X49 is the hypothesis ledger — read it before trying another rule
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
modules (`// P603 LOCAL`), real PDFs or dumps; the probe backup is in the
session scratchpad `p603/probe_src/`.
