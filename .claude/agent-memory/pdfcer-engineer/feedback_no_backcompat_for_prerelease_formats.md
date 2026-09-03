---
name: no-backcompat-for-prerelease-formats
description: Ken 2026-09-03 — do not build backward-compatibility layers for formats pdfcer wrote before 1.0; nobody (including him) has used it in production, so there is nothing to stay compatible with; ask first if it seems needed
metadata:
  type: feedback
---

**Do not add a compatibility shim for a pre-release format pdfcer itself
wrote. If one looks necessary, ask; do not ship it.**

**Why:** during the rename (Pass 247.1) I built a reader fallback for the
old `/PieceInfo /pdfce` ce-dimension sidecar key plus retire-on-write and
three tests, and the librarian minted decision 131 around it. Ken removed
it the same day, verbatim: *"This is unecessary as no one has actually
used the software yet in production including myself. This compatibility
layer can be removed."* The measurement that backed him: the only
documents carrying the old key were the project's own fixtures.

**How to apply:** pre-1.0, the product's own on-disk shapes (sidecars,
private keys, settings grammar, resource-name prefixes) may change
without a migration path. Re-key fixtures instead of teaching the reader
two spellings. Compatibility with OTHER producers' files is a different
thing and unaffected. When the project is in production this rule
presumably flips — ask then, too.
