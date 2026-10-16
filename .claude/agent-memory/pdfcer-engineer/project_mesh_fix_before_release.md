---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 — the standing goal is the 3D compressed-mesh decode failures (door m2/m23/m39/m48, trailer 5 meshes); v0.84.0 shipped the partial/continue floor mid-arc on his directive; root cause still open — read spec RAG §2d before trying another hypothesis
metadata:
  type: project
---

The standing goal is the 3D mesh rendering failures (PRC 8137 compressed
tessellation: door sample m2/m23/m39/m48 nuts, trailer sample m39/m63/m64/
m95/m107). No feature-request loop and no other Passes while it is open.

**State at 2026-10-16:** Ken's requested floor shipped as Pass 602.0
(`0505ec42`): `--mesh-fit partial|continue` draws the sound prefix or a
bounded forgiving continuation, disclosed. He then directed a release
(v0.84.0, tag at `3331ddda`, published 2026-10-16) and a notice to
pdfcer-gui so the shell can expose the options, and said to continue the
fix afterwards — so the earlier "pause, no release" is superseded. Every
sign/inheritance/branch-frame hypothesis scored against the fold oracle is
in the spec RAG `threed/prc__8137__tess_3d_compressed.md` §2d (X30–X39);
read it before trying another. Acrobat Reader draws the failing nut
(X39), so a rule exists.

**Why:** Ken 2026-10-16: "Your only goal before next release is to fix the
mesh rendering problems." then "release a new version and let pdfcer-gui
know of the changes so it can implement the options into the latest
version then continue trying to fix the issue." He allowed a web-research
agent on a lighter model for the lookup.

**How to apply:** work the fix; releases follow the normal standing
authorisation again. Never read nanoPRC/libPRC/Asymptote/MuPDF/
Ghostscript/Poppler source for it (licence rule, decision 169);
behavioural research only. Never commit probe modules, real PDFs or dumps.
