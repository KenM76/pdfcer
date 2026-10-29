# Decision 206 — a `pageArea` keep is laid per the spec by default, Adobe's `none` as a setting

Recorded 2026-10-29 (1287th filing), at `Pass 647.2` tenth cut `f64f12fe`. Applies decision 204; gap G-L12.

**Question.** What does `keep@next`/`keep@previous="pageArea"` mean? XFA 3.3 Ch.8 p.312 defines it as "adjacent parts on the same page". Adobe has never supported it and treats it as `none` (Ch.28 p.1229). Before this cut pdfcer laid it as `contentArea`.

**Decides.** `PageAreaKeep { Spec (default), Adobe }` (`#[non_exhaustive]`, re-exported from `pdfcer_core::xfa`), set with `LayoutOptions::with_page_area_keep` (field `page_area_keep`).
- `Spec`: a page-kept pair may part between content areas of one page; it is held together only at the last content area of the page. Applies to top-level adhering chains and to split lines inside a subform.
- `Adobe`: a `pageArea` keep is `none`.
- When the two sides of a pair disagree, the stricter `contentArea` keep governs (G-L13).
- `LayoutWarning::PageAreaKeep { path }` is emitted under either setting, because the two readings can lay the form differently (rule 4: disclosed, never silent).

**Why a setting, and why this default.** Two readings are defensible, so both ship and one is the default (the operator's standing rule on spec ambiguity). The spec is the default because it is the published standard; the warning discloses where Adobe would differ.

**Code.** `crates/pdfcer-core/src/xfa/layout/`; tests in `tests/adhere.rs`; documented in `docs/core-api/03-capabilities.md`. Core only; the CLI flag arrives with `Pass 647.5`.
