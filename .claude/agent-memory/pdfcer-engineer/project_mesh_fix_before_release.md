---
name: project_mesh_fix_before_release
description: Ken 2026-10-16 — the ONLY goal before the next release is the 3D compressed-mesh decode failures; partial/continue floor shipped (602.0), root cause still open; PAUSE, no release, he starts a new session
metadata:
  type: project
---

The only goal before the next release is the 3D mesh rendering failures
(PRC 8137 compressed tessellation: door sample m2/m23/m39/m48 nuts,
trailer sample 5 meshes). When fixed, STOP — do not release; Ken starts a
new session for that.

**State at 2026-10-16 end of session:** the root cause (which way the
encoder folds at a sliver) is NOT found; every sign/inheritance/branch-frame
hypothesis was scored against a fold oracle and rejected (spec RAG
`threed/prc__8137__tess_3d_compressed.md` §2d, rows X30–X38 — read it
before trying another). Ken's requested floor shipped as Pass 602.0
(`0505ec42`): `--mesh-fit partial|continue` draws the sound prefix or a
bounded forgiving continuation, disclosed; default unchanged.

**Why:** Ken 2026-10-16: "Your only goal before next release is to fix the
mesh rendering problems. After you have fixed the mesh rendering you'll
pause work before the next release so I can start a new session." He also
allowed a web-research agent on a lighter model to look up what may be
wrong, and asked for the partial/continue options while the fix is sought.

**How to apply:** no feature-request loop, no other Passes, until Ken
re-directs in a new session. Release is NOT standing-authorized for this
arc — the next release (carrying 600.0–602.0) waits for his go-ahead.
Never read nanoPRC/libPRC/Asymptote/MuPDF/Ghostscript/Poppler source for
the fix (licence rule); behavioural research only.
