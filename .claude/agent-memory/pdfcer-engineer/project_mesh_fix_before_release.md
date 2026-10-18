---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 — the standing goal is the 3D compressed-mesh decode failures; 603.0 fixed door m2, 604.0 (class-split window) trailer m108/m53, 605.0 (degenerate-frame Y from own X) door m23/m39 + trailer m52/m96; five still fail (door m48, trailer m63/m64/m95/m107) + trailer m39 best-fit; spec RAG R17/X42–X65 is the ledger
metadata:
  type: project
---

The standing goal is the 3D mesh rendering failures (PRC 8137 compressed
tessellation, the operator's door and trailer samples). No feature-request
loop and no other Passes while it is open.

**State at 2026-10-18 (after 605.0, `28e9a53f`, unreleased with 603.0/604.0):**
door 155 unique / 0 / 1 failed (m48); trailer 127 / 10 best-fit / 4 failed
(m63 m64 m95 m107; m39 best-fit). Trajectory: v0.84.0 door 152, trailer
117/19/5 → 603.0 153, 123/12/6 → 604.0 153, 125/11/5 → 605.0 155, 127/10/4.
No regressions at any step. The spec RAG
`threed/prc__8137__tess_3d_compressed.md` holds every hypothesis scored:
R17, X42–X65 (X65 = 605.0). Research agent found no public non-GPL fold
rule; patent US 8,207,965 B2 expiry is Google-Patents-only, unverified.

**What the three fixes have in common:** none was a new rule; each was the
encoder's exact arithmetic in a rare branch (blind normal; window
crowding; which copy of X a degenerate frame unitizes). 605.0's lever was
measured: `unitize` is ~1e-8 accurate in length, the walk is chaotic
(1e-13 tol → 5e-4 tol by triangle 450), and a degenerate frame after a
thin triangle flips the `along` sign on that noise. So "almost the same
formula" fails whole meshes; look for ~1e-8 arithmetic differences, not
for thresholds. A sibling instance of the same part (m23↔m45, m39↔m153)
is an exact per-vertex oracle — find one before trying a rule. Ken's
observation that faces before a reported failure are already wrong was
confirmed; a fold-back census is a diagnostic, not a rule (m44 folds back
legitimately). Probe scaffolding backed up in `D:\Dev\pdfTests\prc-probe\p603\`.

**Why:** Ken 2026-10-16: "Your only goal before next release is to fix the
mesh rendering problems." then "release a new version and let pdfcer-gui
know of the changes so it can implement the options into the latest
version then continue trying to fix the issue." He allowed a web-research
agent on a lighter model for the lookup.

**How to apply:** work the fix; releases follow the normal standing
authorisation again (next one writes `pdfcer2`). Judge any rule change by
per-mesh U/B/F against the previous release binary in `D:/builds/`
(`3d-render --index 0` note lines; release build only — debug takes
>20 min on the trailer), not by the fold it changes — a looser threshold
fails by stage-deepening ambiguity. Never read nanoPRC/libPRC/Asymptote/
MuPDF/Ghostscript/Poppler source (decision 169). Never commit probe
modules (`// P603 LOCAL`), real PDFs or dumps.
