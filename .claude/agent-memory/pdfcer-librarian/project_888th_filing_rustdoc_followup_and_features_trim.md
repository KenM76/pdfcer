---
name: project-888th-filing-rustdoc-followup-and-features-trim
description: 888th filing (2026-10-02) — Pass 446.0 rustdoc follow-up fix (cabaa30c) + FEATURES.md EMF-export row trimmed under the 1200-char cap
metadata:
  type: project
---

888th filing, 2026-10-02, no shell (hashes relayed, not verified). Two
unrelated small items filed together, both housekeeping on `Pass 446.0`
(`ea8ea9cd`, 887th filing):

1. `cabaa30c` deleted a redundant outer `///` on `pub mod emf_import;` in
   `pdfcer-core/src/lib.rs` — merged with the module's own inner `//!`
   header, it broke `cargo doc -D rustdoc::broken_intra_doc_links`. This
   is the **third same-day occurrence** of the exact mechanism already
   documented at
   `D:\dev\rag\rust\a_module_headers_intra_doc_links_resolve_in_the_parent_modules_scope_not_its_own.md`
   (Instance 1: `pdfcer-gui`, 2026-09-12; Instance 2: `79773ee8`,
   `ocr/addons`/`ocr/addon_manifest`, earlier the same day). Added as
   **Instance 3** to the same file rather than a new RAG file — hard
   rule 4 (don't duplicate) clearly applied since the mechanism was
   already fully written up twice.
2. `FEATURES.md`'s EMF-export row had drifted to 1,303 characters
   (over `check-register-entry-size.py`'s 1,200 cap, not in the
   baseline file). Trimmed to verdict + boxes + hash citations, per
   the file's own stated rule: "replace, never append a note."

**Why:** three same-day instances of one mechanism is a stronger signal
than the project's usual "don't propose a checker" posture for prose
disclosures (hard rule 11's note that no mechanical gate can content-check
a disclosure) — this one IS mechanically checkable (a textual pattern: an
outer `///` directly above a `pub mod x;` line where `x`'s file opens with
`//!`), so I noted in the RAG instance that it may be worth a grep-based
pre-commit check rather than relying on `cargo doc` to catch it each time.
This is a suggestion left in the RAG file, not something I'm positioned to
build — that's the engineer's call.

**How to apply:** if a 4th same-day-or-later instance of this exact
mechanism shows up, that's a strong signal to actually propose the grep
check rather than just noting it again.
