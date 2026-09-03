---
name: decision012-supplied-fonts
description: Decision 012 (operator-supplied fonts) first cut — shipped scope, the R61 rule-number collision residual, and key API/design facts
metadata:
  type: project
---

Decision 012 (operator-supplied fonts) FIRST CUT implemented 2026-07-31 (render + CLI + GUI + tests + demo). NOT yet committed; librarians NOT yet dispatched (per task directive).

**R61–R65 RULE-NUMBER COLLISION (blocking for the librarian, unresolved):**
Decision 012's proposed standing rules R61–R65 collide with ROADMAP.md's EXISTING R61 (Inkscape "behavioral reference only", filed under decision 010, ~ROADMAP.md:3606). R62–R65 were free at last read. The whole 012 block must be RENUMBERED before pdfce-librarian files it. Surfaced by pdfce-ui-specialist. I referred to 012's rules by number in code comments (R61=shell-sources-fonts, R62=three-trust-levels, R63=supplied-outside-determinism-gate) — those comments will need updating when the renumber lands.
**Why:** filing colliding rule numbers corrupts the standing-rules index.
**How to apply:** when dispatching pdfce-librarian for 012, tell it to renumber R61–R65 first and fix the in-code rule-number comments to match.

**Shipped scope (012 §5.3 first cut):**
- `GlyphSource{Embedded,Bundled,Supplied}` replaced `LoadedFont.substituted:bool` (font/mod.rs enum; text.rs).
- `FontProgram::face_names()` in font/program.rs reuses the ONE skrifa parse (R21).
- Diagnostics gained `glyphs_supplied`/`supplied_fonts` distinct from bundled `glyphs_substituted`/`substituted_fonts`.
- CLI `--font-dir <DIR>` (repeatable) on render-page only; stable line appended `supplied=`/`supplied_registered=`.
- GUI: Tool::FontFolders (session-state, NOT persisted — R15 partition still doesn't exist), font-env threaded into raster with a 4th staleness key `font_env_generation`, diagnostics panel splits bundled vs supplied, clean-banner now includes glyphs_supplied.

**Deviations from the decision (noted for operator):**
- `--font-dir` added to render-page ONLY, not inspect/extract: inspect just prints version, extract-text is core-based (no rasterization), so the flag would be a no-op there (fuzzy-never-sneaky → not added).
- GUI font folders are SESSION-state, not persisted to the R15 partition (which doesn't exist yet) — consistent with the GUI's existing deliberate no-persistence stance; ui-specialist agreed this is correct for the first cut.
- ui-specialist's suggested inline "Supply this font…" entry point from the diagnostics line: deferred (nice-to-have), Tools-dock entry is the single entry point for now.

**Durable API facts (read-fonts 0.39.2 / skrifa 0.42.1):**
- sfnt names: `FontRef::name()` → iterate `.name_record()`, filter `record.name_id()` against `NameId::{POSTSCRIPT_NAME,FULL_NAME,FAMILY_NAME}`, `record.string(name_table.string_data())` returns `NameString` (impl Display, so `.to_string()`).
- bare CFF names: `CffFontRef::metadata()` → `Metadata::{name,full_name,family_name}()` (PostScript name is from the Name INDEX). CffFontRef itself has NO direct name accessor.
- Type1 names: `Type1Font::{name,full_name,family_name}()`.

Demo fixture generator: tools/gen-supplied-font-fixtures.py → fixtures/synthetic/nonembedded_calibri.pdf. Supplied-face demo uses copies of the rights-cleared Foxit CFFs (assets/fonts/Foxit*.cff) renamed Calibri.cff. Byte-identical-PNG proof: supply FoxitSans.cff (== bundled Sans fallback) → pixel-identical to the no-font-dir render, disclosure flips bundled→supplied.
