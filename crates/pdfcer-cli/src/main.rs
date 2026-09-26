//! # pdfcer — command-line batch shell
//!
//! The scriptable front end to the pdfcer engine (docs/ARCHITECTURE.md §7).
//! Unlike Adobe Acrobat Pro — which has no real CLI, only in-GUI Action
//! Wizard batch sequences — pdfcer ships a genuine command-line binary from
//! the start, so document pipelines can merge/split/stamp/convert/sign/
//! validate PDFs without opening a window.
//!
//! It depends on `pdfcer-core` (and, from the Pass that needs it,
//! `pdfcer-render`) exactly as `pdfce-gui` does, and is held to the same
//! zero-GUI-dependency invariant. Its very existence is the proof that the
//! GUI-core separation (docs/ARCHITECTURE.md §3) works: two completely
//! different front ends, one shared core, no logic duplicated.
//!
//! ## Implemented surface
//!
//! - `--version` / `--help` work and list the planned subcommand surface.
//! - `inspect <file>` (Pass 0): confirms the `%PDF-` header and prints the
//!   declared version (mirroring the GUI's Pass 0 bar).
//! - `render-page <file> [--page N] [--scale S] [--font-dir DIR]… -o
//!   <out.png>` (Pass 1; `--font-dir` decision 012): loads the document,
//!   resolves the page tree, rasterizes one page through `pdfcer-render`,
//!   and writes a PNG. Each `--font-dir` supplies operator fonts for the
//!   document's NON-embedded fonts (shell-side folder walk, R61). See
//!   [`cmd_render_page`] for the full contract.
//! - `export-image <file> [--pages SPEC] [--format png|jpeg] [--dpi D]
//!   [--transparent] [--quality Q] [--background #rrggbb] -o <file> |
//!   --output-dir <dir>` (`Pass 248.0`): renders each selected page and
//!   writes it as a PNG (with real alpha under `--transparent`) or a
//!   JPEG, with the DPI written into the file. Honours every render flag
//!   `render-page` does, through the same resolver. See
//!   [`cmd_export_image`].
//! - `copy-page <file> [--page N] [--dpi D] [--no-svg] [--no-raster]
//!   [--no-pdf] [--background #rrggbb]` (`Pass 248.2`, Windows): places
//!   the page on the OS clipboard as `image/svg+xml`, `PNG`, `CF_DIBV5`
//!   and `application/pdf` in one transaction, so it pastes as vectors
//!   into Word/PowerPoint/Excel/Inkscape and as an alpha raster
//!   elsewhere. See [`cmd_copy_page`] and `src/clipboard.rs`.
//! - `round-trip <file> [--mode M] [-o <out.pdf>] [--producer P]`
//!   (Pass 3.0): saves the document and verifies the
//!   `ARCHITECTURE.md` §5 round-trip invariant — byte identity in the
//!   shape the chosen mode promises, reloadability, and an identical
//!   page-1 raster. Its four verification-specific exit codes (5–8)
//!   are what make it usable as a corpus gate. See [`cmd_round_trip`].
//! - `set-info <in> -o <out> [--title …] [--clear …]` (Pass 3.1): edits
//!   the document information dictionary (§14.3.3). See [`cmd_set_info`].
//! - `rotate-page <in> -o <out> --page N --degrees D [--relative]`
//!   (Pass 3.1): sets one page's `/Rotate` (Table 30). See
//!   [`cmd_rotate_page`].
//! - `set-page-size <in> -o <out> --pages SPEC (--size NAME [--landscape]
//!   | --width W --height H)`: sets a page's `/MediaBox` — the sheet size
//!   (§7.7.3.3). Named sizes come from `pdfcer_core::paper`, so the CLI,
//!   the GUI and any future shell all quote the same numbers. See
//!   [`cmd_set_page_size`].
//! - Every other subcommand is a **documented stub** that exits with
//!   [`exit::UNIMPLEMENTED`]. The real bodies land alongside each feature's
//!   own Pass (docs/ROADMAP.md "CLI batch operations"). Stubs are listed
//!   now so the command surface — and the exit-code convention below — is
//!   established from the first commit rather than retrofitted.
//!
//! ## Exit-code contract (docs/ARCHITECTURE.md §7)
//!
//! pdfcer is meant to be genuinely scriptable, so it follows Unix
//! convention: `0` on success, non-zero on failure, with specific codes so
//! a calling script can distinguish failure modes. See the [`exit`] module
//! for the current assignments. Note `2` is reserved by `clap` for
//! argument/usage errors (its built-in convention) — pdfcer's own runtime
//! failure codes deliberately avoid it.
//!
//! ## English-only by design; locale-invariant stdout (decision 002, R5)
//!
//! pdfcer is **permanently English-only** — a positive design ruling
//! (`docs/decisions/002-i18n-timing.md` §5.4), not a deferral. Reasons:
//! clap's own generated text (`Options:`, `error:`, "unexpected argument
//! found", …) is hardcoded English with no localization API (clap-rs/clap
//! #380, open since 2016), so a "localized" CLI would be a half-English
//! chimera; and localizing a scripting interface breaks callers — the GNU
//! `LC_ALL=C` convention exists precisely because localized tool output
//! breaks parsers. Acrobat Pro has no CLI, so nothing is conceded.
//!
//! The binding contract that follows: **stdout is machine-readable,
//! locale-invariant output, permanently** — it never varies with
//! `LANG`/`LC_ALL`, and neither does the exit-code table above. Human
//! diagnostics go to **stderr**. If stderr prose is ever localized, no
//! script breaks, because this separation is designed in from Pass 0.
//!
//! ## stdout result-line format (stable, parseable)
//!
//! Every subcommand that succeeds prints exactly **one LF-terminated,
//! pure-ASCII line to stdout** summarizing what it did. The lines below are
//! part of the compatibility surface and are versioned like any other
//! public API (a change is breaking).
//!
//! **A `key=` whose value is a §7.9.2 TEXT STRING is debug-QUOTED**
//! (`name="Home Phone"`), because such a value may legally contain spaces
//! and a bare one would make the line unsplittable — or, worse, tempt the
//! printer into altering the value to keep it splittable, which is exactly
//! the defect fixed on 2026-08-09: `list-fields` mangled whitespace to `_`
//! and so reported field names that every write verb rejected. A value that
//! is absent prints as the bare sentinel `-`, which is therefore
//! distinguishable from a present-but-empty `""`.
//!
//! ```text
//! inspect:      <input>: PDF <major>.<minor>
//! list-fields:  field name=<"quoted"|(unnamed)> type=<T> button=<B> \
//!               flags=0x<H> value=<"quoted"|-> widgets=<N> ap=<0|1> \
//!               fillable=<0|1> readonly=<0|1> aa=<0|1> caption=<"quoted"|->
//!               …and a summary line whose action half (`Pass 133.0`) is
//!               emitted WHETHER OR NOT THE DOCUMENT HAS A FORM, because an
//!               action is a document property: `js_network_actions=<n>`
//!               `js_launch_actions=<n>` `annot_actions=<n>`
//!               `chained_actions=<n>` `page_trigger_actions=<n>`
//!               `outline_actions=<n>` `js_actions_anywhere=<n>`
//!               `actions_scanned=<n>` `action_scan_truncated=<0|1>`.
//!               Read the last one BEFORE the rest: a hazard count of zero
//!               from a truncated scan means pdfcer stopped looking, not that
//!               the document is clean.
//! list-annotations: …`action=<Type|Type+next|none>` — what the annotation
//!               does when activated. `+next` means the action carries a
//!               `/Next` chain, so the named type is NOT the whole story.
//! round-trip:   round-trip <input> mode=<M> -> <output>; \
//!               identical=<0|1> in_bytes=<N> out_bytes=<N> appended=<N> \
//!               objects=<N> verbatim=<N> reserialized=<N> reloaded=<0|1> \
//!               raster_compared=<0|1> raster_identical=<0|1> delinearized=<0|1> \
//!               promoted=<N>
//! emf:          emf: ops=<n> rasters=<n> alpha_rasterised=<n> blend_modes_dropped=<n> \
//!               gradients_rasterised=<n> images=<n> layers_rasterised=<n> \
//!               dashed_pre_applied=<n> nonzero_multi_subpath=<n>   (after an EMF export/copy)
//! copy-page:    copied <input> page <N> -> clipboard formats=<a,b,..> <W>x<H> \
//!               dpi=<D> background=<#rrggbb|none>; <render-page's counters>
//! export-image: exported <input> page <N> -> <path> <W>x<H> format=<png|jpeg> \
//!               dpi=<D> transparent=<0|1> background=<#rrggbb|none>; \
//!               <exactly render-page's counters, below — one format string>
//! render-page:  rendered <input> page <N> -> <output> <W>x<H>; \
//!               substituted=<n> notdef=<n> unsupported=<n> unknown=<n> \
//!               deferred=<n> images=<n> images_culled=<n> \
//!               images_unsupported=<n> forms=<n> \
//!               forms_culled=<n> subpixel_culled=<n> \
//!               images_codec_unsupported=<n> \
//!               codec_features=<n> codec_geometry_mismatch=<n> dct_cmyk=<n> \
//!               lzw_anomalies=<n> dct_cmyk_unverifiable=<n> jpx_preblended=<n> \
//!               annots=<n> annots_painted=<n> annots_no_ap=<n> \
//!               annots_hidden=<n> annots_state_missing=<n> annots_widget=<n> \
//!               annots_degenerate=<n> annots_out_of_scope=<n> \
//!               page_content_suppressed=<0|1> need_appearances=<0|1> \
//!               unsupported_type3=<n> unsupported_noncmap=<n> \
//!               unsupported_vertical=<n> \
//!               unsupported_composite_not_embedded=<n> \
//!               unsupported_unknown_subtype=<n> \
//!               unsupported_unusable_program=<n> supplied=<n> \
//!               supplied_registered=<n> contents_unresolved=<n> \
//!               images_masked=<n> images_mask_unsupported=<n> \
//!               masks_resampled=<n> mattes_undone=<n> mattes_not_undone=<n> \
//!               oc_hidden=<n> cs_unresolved=<n> colors_not_set=<n> \
//!               icc_alternate=<n> icc_device_fallback=<n> tint_applied=<n> \
//!               tint_not_applied=<n> sep_all_approximated=<n> \
//!               sep_none_suppressed=<n> pattern_spaces=<n> \
//!               patterns_unpainted=<n> indexed_clamped=<n> indexed_short=<n> \
//!               shadings=<n> shadings_via_sh=<n> shadings_paintable=<n> \
//!               shadings_painted=<n> shadings_refused=<n> shadings_mesh=<n> \
//!               mesh_records=<n> mesh_truncated=<n> mesh_unusable=<n> \
//!               type3_glyphs=<n> type3_glyphs_missing=<n> \
//!               type3_colors_ignored=<n> \
//!               img_colorant_none=<n> img_uncalibrated=<n> \
//!               blend_modes_applied=<n> blend_modes_ignored=<n> \
//!               soft_masks_ignored=<n> soft_masks_applied=<n> \
//!               soft_mask_tr_ignored=<n> soft_masks_reset_stale=<n> \
//!               groups_flattened=<n> groups_special=<n> groups_composited=<n> \
//!               groups_knockout_approx=<n> overprint_requested=<n> \
//!               overprint_opm1=<n> overprint_effective=<n> \
//!               overprint_composited=<n> overprint_refused=<n> \
//!               overprint_pixels=<n> nonseparable_composited=<n> \
//!               nonseparable_pixels=<n> groups_backdrop_reruns=<n> \
//!               soft_masks_on_group_result=<n> \
//!               overprint_images_unsupported=<n> overprint_shadings_unsupported=<n> \
//!               blend_space_subtractive=<n> \
//!               blend_space_from_output_intent=<0|1> \
//!               blends_in_wrong_space=<n> cmyk_buffer=<n> \
//!               cmyk_buffer_refused=<n> cmyk_bridged_pixels=<n> \
//!               cmyk_groups_approximated=<n> cmyk_unbridged_images=<n> \
//!               cmyk_native_image_pixels=<n> rendering_intents_set=<n>
//!               icc_managed_paints=<n> icc_unmanaged_paints=<n>
//!               overprint_process_images_unsupported=<n> \
//!               annots_icon_painted=<n> page_resources_defaulted=<0|1>
//! ```
//!
//! **`render-page` prints a SECOND line when `--probe-ink X,Y` is
//! given, and only then** (`Pass 174.0`). It is deliberately not part of
//! the template above, because the template is a contract about
//! `key=<integer>` pairs in a fixed order and this is neither integers nor
//! unconditional:
//!
//! ```text
//! ink-probe: x=<n> y=<n> source=<cmyk-buffer|screen-srgb|out-of-range> \
//!            c=<f|-> m=<f|-> y=<f|-> k=<f|-> alpha=<f|-> srgb=<r,g,b|->
//! ```
//!
//! Every key is present in every variant, with `-` where there is no
//! value — so *"this page was never composited in ink"* (`screen-srgb`,
//! no colorants) and *"this pixel has no ink on it"* (`cmyk-buffer`, all
//! zero) cannot be confused. The metrics line always comes FIRST, so a
//! script that reads one line off this command is unaffected.
//!
//! **Every key is listed, and the placeholders are uniform.** Two
//! things were wrong with the previous version of that block, and only the
//! second was cosmetic.
//!
//! **It stopped at `img_uncalibrated`, omitting the last 28 keys** — the
//! whole blend-mode, soft-mask, transparency-group, overprint and CMYK
//! families, 32 % of the line. The template's last extension was
//! `1e7a0be` (2026-08-17, the colour slice); the very next slice,
//! `bd244d9`, the SAME DAY, added counters and did not touch it, and
//! sixteen commits then edited the key list in `tests/render_page.rs`
//! without one of them updating this block. That is standing rule `R212`
//! in one sentence: **a contract written down in two places, one of which
//! is under test, drifts in the other — and always in the same direction,
//! because the test forces its copy and nothing forces this one.** The
//! remedy is not vigilance, it is `tools/check-metrics-line-contract.py`,
//! which now compares this block against the `println!` and fails if they
//! disagree.
//!
//! **The placeholders used to be `<K>`, `<L>`, `<J>` … `<ai>`**, a
//! distinct letter per counter to signal "these are all different
//! integers". At the time that scheme had exhausted the alphabet and gone
//! two-letter, which stopped conveying anything and made the block
//! painful to extend correctly — which is a small part of why nobody did.
//! Every counter is now `<n>`, meaning *a non-negative integer*, with
//! `need_appearances=<0|1>` kept distinct because it genuinely is a flag.
//! `annots=…` was also an elision hiding six real keys; it is expanded.
//!
//! `render-page`'s line is deliberately split by the first `"; "` into a
//! **narrative half** and a **metrics half**:
//!
//! - The narrative half echoes paths verbatim. Paths may contain spaces,
//!   so it is for logs and humans, not for field-splitting.
//! - The metrics half is `key=<non-negative integer>` pairs, separated by
//!   single spaces, in the **fixed order shown**, with no spaces inside a
//!   pair. A script parses it with
//!   `line.split("; ").nth(1)` then `split(' ')` then `split('=')` —
//!   robust regardless of what the paths contain.
//!
//!   New counters are normally **appended**, and no existing key ever
//!   changes meaning or disappears — so a parser that reads keys BY NAME
//!   keeps working. That is the guarantee that actually matters, and the
//!   one this project makes.
//!
//!   **What is NOT promised, corrected here: that a key never moves.**
//!   This paragraph used to say existing keys "never move", and `Pass
//!   74.4` broke it by inserting `forms_culled` directly after `forms`
//!   instead of at the end of the line. That was deliberate — "342 forms
//!   executed, 0 culled" only reads as a pair when the two are adjacent,
//!   and this line's second audience is a human scanning it. The promise
//!   worth keeping is name-stability, not ordinal stability. The five
//!   keys before `images` are the only ones whose POSITION is fixed, and
//!   `tests/render_page.rs` is what enforces that any addition is a
//!   decision somebody made on purpose rather than a drift.
//!
//!   Note the shape rather than only the correction: the narrower,
//!   correct version of this rule was written into
//!   `tests/render_page.rs` at the moment the key was added, and THIS
//!   copy — the published one, the one a consumer reads — did not get
//!   it. A contract restated in two places gets changed in one.
//!
//! The counters are `pdfcer_render::Diagnostics`'s honesty report
//! (decision 004 §6.4, rule R20) and their presence on stdout is
//! **mandatory, not decorative**: a batch pipeline that rasterizes 10,000
//! pages must be able to find the ones where pdfcer did not draw the
//! document's own glyphs without a human looking at every image.
//!
//! | key | source field | question it answers |
//! |---|---|---|
//! | `substituted` | `glyphs_substituted` | "are these the document's own letterforms, or a BUNDLED substitute?" |
//! | `supplied` | `glyphs_supplied` | "how many glyphs came from an operator-supplied `--font-dir` face (shapes only; positions still from `/Widths`)?" (decision 012) |
//! | `supplied_registered` | (shell) | "how many name→file registrations did `--font-dir` add?" (0 without the flag) |
//! | `notdef` | `glyphs_notdef` | "is any glyph missing entirely?" |
//! | `unsupported` | `fonts_unsupported` | "was any text skipped outright?" |
//! | `unknown` | `unknown_ops` | "were there operators pdfcer doesn't know?" |
//! | `deferred` | `deferred_ops` | "were there operators pdfcer knows but hasn't implemented?" |
//! | `images` | `images_rendered` | "how many sampled images were painted?" |
//! | `images_culled` | `images_culled` | "how many were skipped because the image's unit square missed the viewport?" (§8.9.5.2 confines an image to `[0,1] × [0,1]` in image space, so one whose unit square lands off-canvas cannot tint a pixel — EXACT, always on, and the raster is byte-identical with or without it. The images half of the pair `forms_culled` is the forms half of — read the two TOGETHER, because it was the GAP between them, 6,145 forms culled against 1 image on the same 400 × 200 region, that showed the image path had no viewport gate at all) |
//! | `images_unsupported` | `images_unsupported` | "how many images are simply MISSING from the raster?" |
//! | `contents_unresolved` | `contents_streams_unresolved` | "how many of this page's `/Contents` streams are not in the file at all, so their marks are MISSING from the raster?" (§7.3.10 + Table 30 — legal, but the page is incomplete) |
//! | `forms` | `forms_rendered` | "how many form XObjects were executed?" |
//! | `forms_culled` | `forms_culled` | "how many were skipped because their `/BBox` missed the viewport?" (§8.10.1 makes `/BBox` a clip, so such a form cannot paint a pixel — EXACT, always on, and the raster is byte-identical with or without it) |
//! | `subpixel_culled` | `subpixel_culled` | "how many objects did `--fast-subpixel` DROP?" (the one LOSSY number on this line. Zero unless the flag is set, and printed either way so a raster always carries the count of what it left out. Those objects were not invisible — each contributed anti-aliased coverage, and a page carrying hundreds of them renders measurably lighter without them, so a non-zero value here means this raster is not the reference one. Read against `forms_culled`, which is the exact cull and changes nothing) |
//! | `images_codec_unsupported` | `images_codec_unsupported` | "how many images need a codec this build doesn't have?" |
//! | `codec_features` | `codec_feature_unsupported` (summed) | "how many images need a codec *variant* this build doesn't have?" |
//! | `codec_geometry_mismatch` | `codec_geometry_mismatch` | "how many images disagree with their own codestream?" |
//! | `dct_cmyk` | `dct_cmyk_images` | "how many benign YCCK JPEGs appeared?" (census — decision 006 §4.4) |
//! | `lzw_anomalies` | `lzw_framing_anomalies` | "how many LZW streams were non-conformantly framed?" |
//! | `dct_cmyk_unverifiable` | `dct_cmyk_polarity_unverifiable` | "did the ONE polarity-ambiguous JPEG shape appear?" (decision 006 R30) |
//! | `jpx_preblended` | `jpx_smask_in_data_preblended` | "did any JPX image arrive preblended with a backdrop (`/SMaskInData 2`)?" |
//! | `images_masked` | `images_masked` | "how many images had their transparency COMPOSITED?" (census; a subset of `images`. The per-mechanism split — `smask` / `stencil` / `colour-key` / `jpx-embedded-alpha` — goes to stderr) |
//! | `images_mask_unsupported` | `images_mask_unsupported` | "how many images are on the page but TOO SOLID, because their `/SMask` or `/Mask` could not be applied?" (the transparency twin of `images_unsupported`: that one means missing, this one means opaque) |
//! | `masks_resampled` | `masks_resampled` | "how many masks had different pixel dimensions from their base image and were point-sampled across it?" (§8.9.6.3 / Table 145 — conformant and common; it exists so a pixel-parity investigation can tell resampling apart from decoding) |
//! | `mattes_undone` | `mattes_undone` | "how many `/Matte` preblends were inverted?" (§11.6.5.3 — census, but the inversion amplifies quantisation error by `1/α`, so a near-transparent fringe that disagrees with another engine is expected rather than a bug) |
//! | `mattes_not_undone` | `mattes_not_undone` | "how many `/Matte` preblends were NOT inverted, leaving colours shifted toward the matte colour?" (alpha still applied; the reason is in the image divergences) |
//!
//! The rows below were absent until 2026-08-22. The table stopped
//! at `mattes_not_undone` and documented 44 of the 87 keys the line
//! emits, so 43 of them -- every annotation, colour-space, shading,
//! blend-mode, soft-mask, transparency-group, overprint and CMYK
//! counter -- had no explanation anywhere a consumer of this CLI would
//! look. Their `Diagnostics` field docs were thorough throughout; the
//! gap was entirely in the SHELL that publishes them, which is the
//! `R212` shape again with a different pair of copies.
//!
//! Each row says which of three kinds of number it is, because that is
//! the thing an operator scanning ten thousand pages actually needs and
//! the thing a bare count cannot convey:
//!
//! - a **census** -- this happened, it is fine, and a non-zero value is
//!   not a problem (`images`, `annots_widget`, `blend_modes_applied`);
//! - a **divergence** -- pdfcer did NOT do what the file asked, so the
//!   raster differs from a conforming render (`blend_modes_ignored`,
//!   `overprint_refused`, `blends_in_wrong_space`);
//! - a **pair half** -- meaningless alone, and misleading if read alone.
//!   `shadings` without `shadings_painted`, or `blends_in_wrong_space`
//!   without `cmyk_buffer`, will be read as the opposite of the truth.
//!
//! | `annots` | `annotations_total` | "how many annotations does this page carry at all?" (census denominator, and it is taken under EVERY annotation scope — a narrowed or suppressed render still discloses what it is not showing. Meaningless alone: the gap between this and `annots_painted` is what the `/Annots` array asked for and did not get) |
//! | `annots_painted` | `annotations_painted` | "how many of them actually reached the raster?" (a §12.5.5 placement succeeded. Read against `annots`; the shortfall is apportioned across `annots_no_ap`, `annots_hidden`, `annots_state_missing` and `annots_degenerate`, plus scope withholdings this line does not carry — see the note on `annotations_out_of_scope` below the table) |
//! | `annots_no_ap` | (sum of `annotations_without_ap`) | "how many annotations have NO usable appearance at all — no `/AP`, no `/N`, or an `/N` that is neither stream nor subdictionary?" (a SUM, because the field is a per-`/Subtype` `BTreeMap` and this line's contract is `key=<integer>`; the per-subtype breakdown goes to stderr where a new key cannot break a parser. This is a fact about the FILE and stays true whether or not the annotation was drawn — read it with `annots_icon_painted`, and see that row for why the two are separate) |
//! | `annots_icon_painted` | `annotations_icon_painted` | "how many of the `annots_no_ap` did the operator nevertheless SEE?" (`Pass 289.0`. An annotation that **names a standard icon** — `/Text` §12.5.6.4, `/Stamp` §12.5.6.12 — is drawn from pdfcer's own artwork, because both tables put that duty on the reader with a `shall`: *"Conforming readers shall provide predefined icon appearances…"*. **`R43` is narrowed, not repealed**: a `/Square` or `/Line` with no `/AP` would need INVENTED GEOMETRY and is still left blank, because §12.5.6.8 addresses *the annotation*, not the reader. The file supplied the NAME; the picture is pdfcer's own, per `LEGAL.md` §4, and stderr says so per annotation) |
//! | `page_resources_defaulted` | `page_resources_defaulted` | "was this page's `/Resources` on neither the page nor any ancestor, so every name on it was looked up in an empty dictionary pdfcer supplied?" (`Pass 290.0`. §7.7.3.3 Table 30 calls the entry *required; inheritable* and §7.7.3.4 says a value *shall* be supplied in an ancestor node — but Acrobat writes pages that satisfy neither, and refusing them used to cost the WHOLE document, well-formed pages included. `1` on a page WITH content is the single cause behind an otherwise unexplained pile of `unsupported=` / `cs_unresolved=` / unpainted forms; `1` on a page with no `/Contents` is usually inert — but not by construction: §7.8.3 lets a form XObject, including an annotation's `/AP` stream, inherit the page's resource dictionary, which is exactly the stamp-page shape that motivated the Pass) |
//! | `annots_hidden` | `annotations_hidden` | "how many annotations did the DOCUMENT suppress?" (§12.5.3 Table 165's Hidden and NoView flags — a census of pdfcer obeying the file, not a shortfall, honoured AND counted under R50 because content the operator cannot see is still disclosed) |
//! | `annots_state_missing` | `annotations_appearance_state_missing` | "how many annotations carry a state subdictionary whose state could not be selected?" (§12.5.5 NOTE 3 — `/AS` absent against a multi-entry subdictionary, or naming a state that is not in it. Displayed as NOTHING, never guessed: a checkbox that should read "on" reads as blank, and this is the only thing that says why) |
//! | `annots_widget` | `annotations_widget` | "how much of this page's annotation load is FORM FIELDS?" (§12.5.6.19 — census, a subset of `annots`. Widgets are ~88 % of organic annotations, so their share is what drives forms prioritisation rather than anything about this page's correctness) |
//! | `annots_degenerate` | `annotations_placement_degenerate` | "how many annotations HAVE an appearance that could not be put anywhere?" (a missing `/Rect` or `/BBox` — the §12.5.5 placement inputs — or a transformed appearance box of zero width or height, which makes the step-b fit matrix singular. A named refusal, never a divide-by-zero and never a fabricated placement (risk X2); the specific reason is in the stderr annotation notes) |
//! | `annots_out_of_scope` | `annotations_out_of_scope` | "how many annotations did THIS RENDER withhold, as opposed to the document hiding them?" (the `--no-annotations` and form-fields-only scopes. A divergence from the file that the OPERATOR chose, not one pdfcer chose — but a divergence, and until 2026-08-22 it was computed, merged and unit-tested while being printed nowhere at all, so `--no-annotations` withheld content and reported no number saying how much. Distinct from `annots_hidden`, which is the document's own suppression) |
//! | `page_content_suppressed` | `page_content_suppressed` | "was the PAGE'S OWN content withheld?" (`0`/`1`, the only flag on this line besides `need_appearances`. `1` under the print-onto-pre-printed-paper scope, where the background is physical paper and drawing it again would double-print it — so a near-empty raster is the requested output rather than a failure, and this is the only thing that says which) |
//! | `need_appearances` | (shell) | "is this document telling me its field appearances are STALE?" (§12.7.2's `/AcroForm` `/NeedAppearances`. DOCUMENT-scoped, not page-scoped, so it prints `0`/`1` rather than a count and is read from the catalog rather than from `Diagnostics`. R51: pdfcer reports the condition and never silently regenerates on load — that would rewrite objects the operator never touched and pick appearances for them. A widget with an `/AP` `/N` is still painted from it regardless) |
//! | `oc_hidden` | `oc_sections_hidden` | "is something on this page deliberately NOT being shown?" (§8.11.3.2 — census of pdfcer obeying an OFF optional-content group, counted per SECTION entered rather than per operator suppressed, because one section can hide a whole drawing. R183: this is the disclosure channel for the one feature whose correct behaviour is "draw less", and without it a layer turned off is indistinguishable from a render that failed) |
//! | `cs_unresolved` | `color.spaces_unresolved` | "did any `cs`/`CS` name a colour space pdfcer could not resolve?" (§8.6 — DIVERGENCE. Counts DISTINCT resource names, not occurrences, on the same policy as `unsupported`: one broken resource used ten thousand times is one problem. The space is left UNSET rather than defaulted to `DeviceGray`, because a silent `DeviceGray` paints black marks that look exactly like a correct render — so read this beside `colors_not_set`, which is the residue it leaves) |
//! | `colors_not_set` | `color.colors_not_set` | "how many marks were painted in the PREVIOUS colour because a colour operator could not be honoured?" (`sc`/`scn`/`SC`/`SCN` against an unresolved space, or an operand count that did not match the space's component count. Counts OCCURRENCES, not names — each one is a potentially stale-coloured mark, and this is the half of the `cs_unresolved` pair that says whether the unresolved space mattered) |
//! | `icc_alternate` | `color.icc_alternate_used` | "how much of this page's colour went through the file's own `/Alternate` instead of its ICC profile?" (§8.6.5.5 Table 66 — a FIDELITY counter, not a shortfall: this is the spec's own fallback and it is visually close for the sRGB-like profiles that dominate real files. Distinct RESOLUTIONS, not paints. pdfcer is not colour-managing, so an operator matching a brand colour should not treat this render as colour-managed) |
//! | `icc_device_fallback` | `color.icc_device_fallback_used` | "how many `ICCBased` spaces had no usable `/Alternate` at all, so pdfcer fell to the device space `/N` implies?" (Table 66's `Alternate` row, second sentence: 1 → Gray, 3 → RGB, 4 → CMYK. The weaker half of the pair above — read the two together for how much of this page's colour is profile-free) |
//! | `tint_applied` | `color.tint_transforms_applied` | "are this page's spot colours the DOCUMENT's own?" (§7.10 tint transforms that WERE evaluated — census, and the positive twin of `tint_not_applied`. It exists so a shell can say the spot colours are the document's and not pdfcer's stand-in, which is the difference an operator checking a brand colour needs) |
//! | `tint_not_applied` | `color.tint_transform_not_applied` | "how many `Separation`/`DeviceN` conversions painted pdfcer's stand-in instead of the document's colour?" (DIVERGENCE, but a property of the FILE rather than a gap in pdfcer — the §7.10 evaluator is wired into both conversions, and this fires only when `/tintTransform` is absent, malformed, or of the wrong arity. What paints is a neutral in the alternate space: right lightness, WRONG HUE. A drawing whose spot colours look grey has its explanation here) |
//! | `sep_all_approximated` | `color.separation_all_approximated` | "how many `Separation /All` conversions were rendered as pdfcer's screen approximation?" (§8.6.6.4 says painting shall apply the tint to all available colorants at once; on an additive display pdfcer renders that as a neutral of luminance `1 − tint`. That is a CHOICE — the standard describes an ink behaviour, not a screen appearance — so it is disclosed rather than assumed) |
//! | `sep_none_suppressed` | `color.separation_none_suppressed` | "how many paints drew NOTHING because their colour space was `Separation /None` or an all-`/None` `DeviceN`?" (§8.6.6.4/.5 — CENSUS of pdfcer obeying the standard, not a shortfall. On the line because a page missing content for a CONFORMANT reason is otherwise indistinguishable from one that failed, which is the distinction the diagnostics exist to make. `img_colorant_none` is the image-side twin) |
//! | `pattern_spaces` | `color.pattern_spaces_selected` | "did this page's content select the `Pattern` colour space at all?" (§8.6.6.2 — census of SELECTIONS, which is all a `cs`/`CS` can tell you. Which kind of pattern was then named, and whether it drew, is decided at the paint site and lands on `patterns_unpainted` and `shadings_painted`. Meaningless alone; the trio is the story) |
//! | `patterns_unpainted` | `color.patterns_unpainted` | "how many `scn`/`SCN` pattern selections put NOTHING on the page?" (DIVERGENCE — and the counter that once caught a page reporting a clean render while every gradient fill in it painted nothing, which is precisely the silence rule 4 forbids. Since shading patterns began painting this is the REMAINDER: tiling patterns, a name with no matching `/Pattern` resource, a degenerate pattern matrix, or a shading pdfcer models but cannot draw. Nothing is drawn in its place deliberately — Table 74's initial `Pattern` colour "causes nothing to be painted", and an invented solid fill would be worse than a gap) |
//! | `indexed_clamped` | `color.indexed_index_clamped` | "how many `/Indexed` lookups ran off the end of their own range?" (§8.6.6.3 makes the clamp NORMATIVE — "if it is outside the range 0 to `hival`, it shall be adjusted to the nearest value within that range" — and it is a clamp, not a modulo. So this is a census of conformant behaviour, useful only because a stream that keeps hitting it is usually a producer bug) |
//! | `indexed_short` | `color.indexed_lookup_short` | "how many `/Indexed` lookups fell past the end of a SHORT lookup table and painted BLACK?" (producers routinely trim trailing unused entries, so it is tolerated rather than fatal — but the colour IS wrong where it fires, which makes this the named cause behind an unexpectedly black palette entry) |
//! | `shadings` | `shading.encountered` | "does this page have gradients at all?" (§8.7.4 — the inventory denominator, incremented by BOTH routes and incremented even for a shading that is then refused, so a page whose every gradient is malformed does not report the same zero as a page with no gradients. The load-bearing pair on this line is this beside `shadings_painted`) |
//! | `shadings_via_sh` | `shading.via_sh` | "how many of them arrived through the `sh` operator rather than a `PatternType 2` pattern fill?" (the two ANCHOR differently — `sh` in current user space, a shading pattern in pattern space (§8.7.4.5 SH1/SH6) — so a page that renders wrong in only one of them is a different bug, and this is what tells the two apart without opening the file) |
//! | `shadings_paintable` | `shading.paintable` | "will an update fix MY file?" (the question an operator actually has, and why this sits between the census and the result: a page whose shadings are all type-7 meshes reports `shadings_paintable=0` and needs a different answer from one where paintable equals `shadings`) |
//! | `shadings_painted` | `shading.painted` | "how many gradients actually reached the raster?" (the right half of this line's load-bearing pair — `shadings` non-zero beside `shadings_painted` zero is the honest statement that pdfcer found the gradients, understood them, and drew none. An unpainted shading leaves whatever was underneath it showing through. Read either number alone and you get the wrong answer) |
//! | `shadings_refused` | `shading.refused` | "how many shading dictionaries were rejected outright, with a named reason?" (DIVERGENCE. The finer counters behind it — no usable `/Function`, a `/Function` that would not load, a `/Function` output count disagreeing with the colour space's component count (§8.7.4.4), an incomplete colour ramp — are NOT on this line; they and the reason strings go to stderr, where a new key cannot break a parser) |
//! | `type3_glyphs` | `type3_glyph_procs_run` | "how many glyphs did pdfcer draw by RUNNING a content stream?" - Type 3 (ISO 32000-1 8.7.4.5 is shadings; this is 9.6.5) defines its glyphs as procedures rather than as a font program, so this is a census of a different KIND of glyph rather than a shortfall. Zero on almost every document; non-zero means the page carries a font whose shapes the file itself draws |
//! | `type3_glyphs_missing` | `type3_glyphs_missing` | "was a Type 3 code shown whose glyph does not exist?" (DISCLOSURE). Two causes, which the standard gives the same outcome and which are therefore one counter: the code has no `/Differences` entry (9.6.6.3 makes a Type 3 encoding TOTAL, with nothing to fall back on), or its glyph name is not a key in `/CharProcs` (9.6.5 step b, "no glyph shall be painted"). **The advance still happened** - the clause says nothing about the width and `/Widths` supplies it independently, so this counts glyphs that are absent and never text that has moved |
//! | `type3_colors_ignored` | `type3_colors_ignored` | "how many colour operators inside a `d1` glyph procedure did pdfcer drop?" (DISCLOSURE, and NOT a shortfall). Table 113 makes ignoring them the DEFINED behaviour - a `d1` glyph "specifies only shape, not colour" and takes the colour in force at the text-showing operator - and Acrobat was measured doing the same on 2026-08-25. It is reported because an operator debugging a glyph that came out the "wrong" colour needs to be told that the colour operator they can see in the stream was deliberately dropped rather than missed |
//! | `mesh_records` | `shading.mesh_records` | "how much geometry came out of the mesh streams?" - triangles for shading types 4/5, patches for types 6/7 (ISO 32000-1 8.7.4.5.5-.8). Zero beside a non-zero `shadings_mesh` and a zero `mesh_unusable` would be a bug; zero beside a non-zero `mesh_unusable` is the streams being unreadable, which is a different fact and is why they are two keys |
//! | `mesh_truncated` | `shading.mesh_truncated` | "did a mesh stream stop part-way through a record?" (DISCLOSURE). pdfcer paints the complete records and discards the remainder. The standard states an error condition for exactly ONE of the four mesh types (type 4) and is silent for the other three, so this is a product decision being reported rather than a conformance verdict. A type 5 stream that does not hold a whole number of rows counts here too |
//! | `mesh_unusable` | `shading.mesh_unusable` | "was a mesh shading found and NOT decodable?" (DISCLOSURE). Bad `/BitsPer...` widths, a missing or wrong-length `/Decode`, an `Indexed` colour space (which a literal reading permits and which would interpolate palette INDICES), a first patch with a nonzero edge flag and nothing to inherit from. Each occurrence names its reason in the notes below the line. Distinct from `shadings_refused`, which counts dictionaries rejected before their stream was reached |
//! | `shadings_mesh` | (derived — `shading.mesh()`, `by_type[3..7]` summed) | "how much of this page needs the mesh work rather than the parametric work?" (types 4–7, §8.7.4 — DERIVED at print time from the per-`ShadingType` census, which is itself not on the line. A subset of `shadings`; what remains after subtracting it is the function-based, axial and radial population) |
//! | `img_colorant_none` | `images_colorant_none` | "how many images were correctly painted as NOTHING?" (§8.6.6.4/.5 — a `/Separation /None` or all-`/None` `/DeviceN` image colour space. CENSUS of pdfcer's CORRECTNESS, and it is on the machine line rather than only in a note because a pixel-parity harness reads this line and nothing else: pdfium paints such an image BLACK (measured 2026-08-17), so the divergence will be maximal and it is pdfcer that is right) |
//! | `img_uncalibrated` | `images_uncalibrated_colorimetry` | "how many images went through pdfcer's OWN XYZ→sRGB rather than a colour-management engine?" (`Lab`, `CalGray`, `CalRGB` — Bradford to D65, no rendering intent. Defensible, not colour-managed. On the line rather than only in a note because a `Lab` image landed in a parity harness's *unexplained* bucket while a perfectly good stderr sentence explained it — a disclosure that reached a human and not a machine) |
//! | `blend_modes_applied` | `blend_modes_applied` | "how many `gs` operators selected a non-Normal blend mode that pdfcer APPLIED?" (§11.3.5 — a CENSUS of what ran, and applied is NOT applied correctly: an additive page's blends are computed in device sRGB, which is what §8.6.6.4 specifies for an additive device, while a subtractive page's go through §11.3.4's complement only when the colorant buffer engaged. Read it with `blend_space_subtractive`, `blends_in_wrong_space` and `cmyk_buffer` before calling it a pass. The four non-separable modes increment this, and increment `nonseparable_composited` ONLY when they are painted directly -- see that counter's own row, because the difference is measurable and misleading) |
//! | `blend_modes_ignored` | `blend_modes_ignored` | "were any marks composited as `Normal` when the document asked for something else?" (DIVERGENCE, and the INVISIBLE kind — a blend that fell back to Normal produces a perfectly ordinary-looking opaque overlay, where a missing image leaves a hole somebody notices. One reason reaches it now: a `/BM` name outside Tables 136 and 137, i.e. a typo or an extension mode. The four non-separable modes used to land here and no longer do, so a non-zero value no longer implicates them) |
//! | `soft_masks_ignored` | `soft_masks_ignored` | "were any marks painted UNMASKED that the document wanted faded or hidden?" (§11.6.5 — DIVERGENCE, and the FAILURE DIRECTION is what makes it the one to watch: an ignored soft mask paints MORE than the document asked for, so on a page whose design relies on a mask to hide something this is the difference between a rendering artefact and showing what was meant to be hidden) |
//! | `soft_masks_applied` | `soft_masks_applied` | "how many soft masks were BUILT and applied?" (§11.6.5 — census. It says a mask exists and multiplied coverage; it does NOT say where it was applied, which is `soft_masks_on_group_result`'s question, nor whether its `/TR` was honoured, which is `soft_mask_tr_ignored`'s. Three keys, one mechanism, and none of them answers the others) |
//! | `soft_mask_tr_ignored` | `soft_mask_transfer_ignored` | "did a mask carry a transfer function pdfcer read and never evaluated?" (§11.6.5's `/TR` — DIVERGENCE, and one the operator cannot see by looking: `/TR` is the natural place to INVERT a mask, so an ignored one can show exactly the content that should have been hidden) |
//! | `soft_masks_reset_stale` | `soft_masks_reset_stale` | "did a `gs /SMask /None` fail to restore the clip it found?" (fires when a `W n` intervened while the mask was in force, so the pre-mask clip no longer describes the geometry — counted rather than silently mis-clipped. It has a SECOND way to fire since group masks landed: a group whose mask could not be lifted out of the clip keeps the old per-element behaviour and lands here rather than on `soft_masks_on_group_result`, so read those two together) |
//! | `groups_flattened` | `transparency_groups_flattened` | "how many transparency groups were painted straight onto the page instead of composited as a UNIT?" (§11.4.7 Table 147 — DIVERGENCE that no blend-mode counter can express. A group is a compositing SCOPE: its contents render into a separate buffer and the group's RESULT is composited with the blend, alpha and soft mask in force at the `Do`. Flattening applies those to each object INSIDE instead — the same answer for a group holding one opaque object, a different answer for almost anything else) |
//! | `groups_special` | `transparency_groups_special` | "how many of the flattened groups were the ones where flattening stops being a good approximation?" (`/I true` isolated or `/K true` knockout — Table 147, NOT Table 96 which is the COMMON group-attributes table, and note that clause-11 table numbers shift by −2 in ISO 32000-2. An isolated group blends against a transparent initial backdrop rather than the page; a knockout group composites each element against the group's INITIAL backdrop, so flattening reverses the intended occlusion) |
//! | `groups_composited` | `transparency_groups_composited` | "how many groups were rendered into their own buffer and applied as a unit?" (§11.4.5 — the positive twin of `groups_flattened`, and NOT a clean census: a `tiny_skia::Pixmap` starts TRANSPARENT and a transparent initial backdrop IS isolated semantics (§11.4.7), so a non-isolated group under a `/BM` used to become an isolated one silently and count here as a success. Fixed on the additive path; it survives on a SUBTRACTIVE page, where the residue is counted as `cmyk_groups_approximated`) |
//! | `groups_knockout_approx` | `transparency_groups_knockout_approximated` | "inside a knockout group, how many ELEMENTS could not be given §11.4.6 semantics?" (exactly three kinds — a shading, an overprint composite (§11.7.4.3), a per-paint non-separable blend (§11.3.5.3) — all for one reason: they read the destination back, and §11.4.8 needs the element's own shape in isolation because it scales the destination by `(1 − f_s)` rather than `(1 − α_s)`. They layer instead of knocking out, which is the answer a NON-knockout group would give and also the answer every element gives at `q_s = 1`, so the shortfall is bounded. A ZERO DOES NOT MEAN THE PAGE HAD NO KNOCKOUT TO GET WRONG: only explicit `/K true` reaches this counter, while §9.3.8's `/TK` (initial value TRUE — every text object), §11.6.7's shading patterns and §11.7.4.4's `B`/`b` and text rendering modes 2 and 6 establish knockout with no `/K` key anywhere, and none of those is treated as knockout today) |
//! | `overprint_requested` | `overprint_requested` | "how many `gs` operators turned overprint ON?" (§8.6.7's `/OP` or `/op` — a CENSUS of demand that overstates the problem badly: producers set `/OP true` across whole documents as a default and most of those paints are no-ops. This is the denominator; `overprint_effective` is the honest measure) |
//! | `overprint_opm1` | `overprint_mode1_requested` | "how many of those also selected overprint mode 1?" (`/OPM 1`, §8.6.7's nonzero overprint mode — separated because mode 1 is where overprint stops being a component-SET question and becomes a per-component VALUE one: a zero `DeviceCMYK` component leaves the backdrop unchanged. §8.6.7 also makes it inert off `DeviceCMYK`, so a non-zero count is the clearest signal that a document expects an ink model) |
//! | `overprint_effective` | `overprint_effective` | "how many paints would actually LOOK different if overprint were honoured?" (the subset of `overprint_requested` that is a real visual difference: a source space specifying FEWER components than the backdrop has — a `Separation` or one-component `DeviceN` over CMYK — or mode 1 with a zero-valued component. §11.7.4.3's `CompatibleOverprint` picks the source component for every component the current space specifies and the backdrop's for the rest, so a `DeviceCMYK` fill over a `DeviceCMYK` backdrop at mode 0 is IDENTICAL to Normal) |
//! | `overprint_composited` | `overprint_composited` | "how many paints actually went through `CompatibleOverprint` rather than a `Normal` blend?" (§11.7.4.3 Table 149. Should equal `overprint_effective` minus `overprint_refused` — kept as its own counter rather than derived, because a derived number cannot disagree with reality and therefore cannot report a bug. A disagreement across the three is the signal worth chasing) |
//! | `overprint_refused` | `overprint_refused` | "how many paints fell back to a normal blend when overprint applied?" (DIVERGENCE, and the one to watch in this block: non-zero means the operator is looking at KNOCKED-OUT backdrops where a press would show overprinted ink, which is not detectable by looking at the page. Distinct from `overprint_images_unsupported` — this is "the composite was offered this paint and could not run it", that one is "the composite was never offered this object at all") |
//! | `overprint_pixels` | `overprint_pixels` | "did the overprint composites MOVE anything?" (the measurement that separates "overprint ran and mattered" from "overprint ran and was a no-op on this geometry" — two facts a paint count alone conflates. Meaningless without `overprint_composited` beside it, and vice versa) |
//! | `rendering_intents_set` | `rendering_intents_set` | "how many times did the document DECLARE a rendering intent?" (§8.6.5.8 — the `ri` operator or an `/ExtGState` `/RI`. A CENSUS OF WHAT THE FILE ASKED FOR, NOT OF WHAT pdfcer DID: as of `Pass 199.0` the intent is carried in the graphics state where it was previously discarded outright, and as of `Pass 199.2` it IS consumed: it selects the ICC transform for an `ICCBased` paint on a subtractive page. A non-zero value here with `icc_managed_paints=0` means the file declared an intent that still changed nothing -- normally because it named no `/OutputIntent` to convert toward. Measured: a failing ICC-RGB conformance patch declares an intent **19 times**) |
//! | `icc_managed_paints` | `icc_managed_paints` | "how many paints became ink via the COLOUR ENGINE rather than the fallback formula?" (`Pass 199.2`. Half of a pair — read it with `icc_unmanaged_paints` below. An `ICCBased` paint on a page that composites in ink is converted by iccce using the file's OWN embedded source profile and its `/OutputIntent` destination profile, at the intent the graphics state asked for. The fallback it replaces, `overprint::rgb_to_cmyk`, is an invertible round-trip transform that is correct for round trips and wrong as a terminal conversion — measured at **92 levels** maximum divergence on a conformance patch, moving the page closer to Acrobat when enabled. COUNTS GRAPHICS-STATE PAINTS ONLY. An `ICCBased` IMAGE is never colour-managed at all -- `image::Space` collapses it to a device space by `/N` and discards the profile -- so an image can never appear here. Read this number with `icc_unmanaged_paints`, which since `Pass 207.0` DOES see images) |
//! | `overprint_process_images_unsupported` | `overprint_process_images_unsupported` | "how many PROCESS-space images were painted under `/OP true` on a page that composites in ink?" (DIVERGENCE, `Pass 204.0`. ALWAYS ZERO SINCE `Pass 238.0` AND KEPT ON THE LINE FOR SCRIPT STABILITY: the image path now preserves the spot planes under `/OP true`, which is exactly the sub-row this counted the absence of, so nothing increments it any more; a non-zero value from an OLDER build meant the shape of the problem was present. The history below is kept because it explains what the number used to mean. §11.7.4.3 Table 149's row for *any process colour space* has TWO sub-rows: the process component reads `c_s` in all three columns, and **the spot colorant reads `c_b` under `OP true`**. Three source comments quoted the first, dropped the second, and concluded that painting such an image normally "IS the conforming result, not a shortfall" — while pdfcer's own Table 149 implementation had always returned `Backdrop` for that case. So the renderer's comments contradicted both the spec and the rest of the codebase. THIS COUNTS THE SITUATION, NOT CONFIRMED DAMAGE, and the limit is real: the IMAGE path does not deposit into a spot plane, so a backdrop laid down by an image has already been flattened into the process channels before this one paints, and pdfcer cannot tell whether a spot was underneath. (Said 'with no spot plane' until 2026-09-02 — planes exist since `Pass 225.0`; it is the image path that still lacks one.) A non-zero value means the page contains the shape of the problem. Measured: an `/Indexed /DeviceCMYK` drop shadow over a spot-green backdrop renders a neutral grey ramp on white paper where a press shows the same ramp on green. Closing it needs the per-spot-colorant plane) |
//! | `icc_unmanaged_paints` | `icc_unmanaged_paints` | "how many paints COULD have been colour-managed and were not?" (`Pass 199.2`. The other half, and the one that makes a zero interpretable: `icc_managed_paints=0` alone cannot distinguish "the engine ran and agreed" from "the branch was never reached". A non-zero value here means an `ICCBased` paint fell back to the approximate formula — and as of `Pass 207.0` also that an `ICCBased` IMAGE was drawn on such a page, which is now the MOST COMMON cause because images are never managed at all. The other causes: the document named no `/OutputIntent`, a profile would not parse, or the destination was not four-component. This list is enumerated and therefore has an expiry date -- it was exhaustive when written and went stale one Pass later, which is the third time in 24 hours an exhaustive enumeration in this file has done so. Every listed item stays correct, so the list reads as verified. This is the rule-4 disclosure for colour management: nothing on the page is drawn differently, and the fact that pdfcer approximated is reported off-canvas) |
//! | `nonseparable_composited` | `nonseparable_composited` | "how many DIRECT PAINTS went through `Hue`/`Saturation`/`Color`/`Luminosity`?" (§11.3.5.3 Table 137 — a census of a SECOND code path: pdfcer computes these four per pixel rather than handing the mode to the rasteriser, whose implementations are measurably wrong (decision 066). **IT DOES NOT COUNT A TRANSPARENCY GROUP COMPOSITED WITH ONE OF THOSE MODES**, and that omission is measurable: a page carrying `/BM /Hue` and `/BM /Saturation` reports `blend_modes_applied=15` with this at **0**, because its blending happens when the group is composited rather than when a path is painted. A reader who takes 0 for "no non-separable mode ran" is reading it wrong — this is a count of PAINTS, not of composites, and the name over-promises. The group half is filed, not implemented) |
//! | `nonseparable_pixels` | `nonseparable_pixels` | "did those composites move anything?" (the same companion relationship `overprint_pixels` has to `overprint_composited`: a composite that ran on zero pixels and one that repainted a whole swatch are both "1" on the count above, and only this distinguishes them) |
//! | `groups_backdrop_reruns` | `transparency_groups_backdrop_reruns` | "why did this page take twice as long as its neighbour?" (§11.4.4 — a COST counter, not a shortfall, and the only one on this line that names something pdfcer DID: a non-isolated group whose content stream was walked a SECOND time over a copy of its own backdrop, so the element formula and backdrop removal could be computed against it. It is the only place in the renderer where a page's content is interpreted more than once. Zero is the normal reading and does NOT mean non-isolated groups were mishandled — §11.4.4 NOTE 5 makes the single walk exact whenever the group's interior composites `Normal` throughout) |
//! | `soft_masks_on_group_result` | `soft_masks_on_group_result` | "were group soft masks applied ONCE to the composite, or once per object inside it?" (§11.4.5 — read against `soft_masks_applied`, which counts masks BUILT while this counts the ones that reached where the clause puts them. The difference is not a shortfall on its own: a mask on an ELEMENTARY object belongs in the clip, because §11.6.4.1 makes the mask value that object's `q_m` and a `q_m` multiplies coverage exactly as a clip does. What to look for is a document WITH transparency groups where this stays at zero while `soft_masks_reset_stale` climbs — that page's group masks are multiplying once per object, visible wherever two of them overlap) |
//! | `overprint_images_unsupported` | `overprint_images_unsupported` | "how many IMAGES were OWED §11.7.4.3's composite and did not get it?" (DIVERGENCE. THIS COUNTER CHANGED MEANING IN `Pass 130.2` AND IT NOW COUNTS A STRICTLY SMALLER SET — a script comparing a number from an older release against one from this one is comparing two different questions. It used to answer "was the composite offered this object CLASS?", and the answer was no for every image, so it counted every image painted under `/OP` whether or not anything was owed. It now answers "was the composite owed HERE, and did it fail to run?". WHY THE OLD SET WAS TOO BIG, and this half is unchanged from what that row always said: Table 149's first row is scoped `DeviceCMYK, specified directly, NOT IN A SAMPLED IMAGE`, so an image falls to the second row — "any process colour space (including other cases of `DeviceCMYK`)" — which is `c_s` in all three columns. For a PROCESS image, painting it normally IS the conforming behaviour and nothing was ever missing. Those images no longer appear here. WHAT REMAINS, and both are real: a `Separation`/`DeviceN` image naming ONLY spot colorants WHOSE COLORANT COULD NOT BE GIVEN A PLANE — roster cap, byte ceiling, or the composite device model (with a plane, since `Pass 238.0`, the image deposits its spot and preserves the whole process backdrop, which is the press's answer and is no longer counted here), and an image on a destination that cannot be read back (a recording canvas). Both also raise `overprint_refused`, so the `composited = effective - refused` identity holds across paths and images alike)  |
//! | `overprint_shadings_unsupported` | `overprint_shadings_unsupported` | "how many shadings were painted while overprint was in force and could not honour it?" (DIVERGENCE. **THE PROBLEM STATEMENT THAT USED TO SIT HERE DESCRIBED THE WHOLE CLASS AND NOW DESCRIBES ONLY PART OF IT.** It read: "a shading fails one step earlier than an image does — its colour ramp resolves to three-channel sRGB when the ramp is BUILT, so nothing downstream has colorants left to overprint WITH". That was true of EVERY shading until `Pass 122.6` gave the analytic ones a colorant ramp, then true only of meshes — and `Pass 137.1` closed that too, because `Shading::paint_cmyk` was already generic over the overprint rules, so giving a mesh its colorants gave it overprint in the same change. WHAT STILL COUNTS HERE, so the number is readable rather than merely smaller: a shading with **no authored ink at all** (an additive space, or a parametric one whose ramp yields no colorants) painted under `/OP`; and an analytic shading in `DeviceCMYK` specified DIRECTLY under `/OPM 1`, which is refused deliberately rather than for want of colorants — Table 149's `OPM 1` row is VALUE-DEPENDENT, so which components are "specified" differs per pixel and cannot be decided once for a whole ramp. The two halves of the mesh case are COUPLED: §11.7.4.3 makes `B(c_b, c_s)` equal `c_s` for every component 'specified in the current colour space', and a bridged sRGB scratch has specified all three, so an overprint composite alone would change nothing and native colorants alone would change nothing either. Visible on suite `PCS 1.0` cells e/j, where a `/DeviceN [/Cyan /Magenta]` shading over an orange ground should let the yellow beneath survive and read GREEN, and instead reads BLUE. A THIRD POPULATION WAS ADDED BY `Pass 202.0` AND THIS LIST DID NOT MENTION IT FOR ONE COMMIT: a **spot-only** `Separation`/`DeviceN` shading under `/OP true`, which Table 149 puts entirely in the backdrop column and which therefore paints NOTHING natively. It is now refused in favour of the flattening bridge, and the refusal increments this counter — where previously the bar rendered as bare white paper with this counter reading 0. Note the failure mode of the sentence you are reading: an EXHAUSTIVE enumeration is a promise that goes stale the moment a population is added, and nothing but a reader checks it) |
//! | `blend_space_subtractive` | `blend_space_subtractive` | "is this page's compositing governed by §11.3.4 at all?" (the page itself and every transparency group whose blending colour space is `DeviceCMYK`, `Separation`, `DeviceN`, or a four-component `ICCBased` resolving to one. A CENSUS OF EXPOSURE, not a shortfall — a page can be entirely `DeviceCMYK` and entirely correct, because `Normal` is `c_s` on either side of the complement. Not a small class: every patch in the suite transparency panel declares `/Group /CS /DeviceCMYK` on the PAGE, including one whose own objects are `ICCBased` RGB, because a non-isolated group inherits its blending space (Table 147's `/CS` row). Whether §11.3.4 was HONOURED is `cmyk_buffer`; what it cost when it was not is `blends_in_wrong_space`. Three numbers, three questions, and reading any one alone gets a wrong answer) |
//! | `blends_in_wrong_space` | `blends_in_wrong_space` | "how many blends were computed on the WRONG SIDE of §11.3.4's complement?" (DIVERGENCE, and the number that says a rendering is actually AFFECTED rather than merely exposed. The worked case is suite `PCS1_162`'s `Difference` cell: magenta under black gives `DeviceCMYK 1 0 1 0` — the green the patch is authored around — under §11.3.4, and `(237, 1, 140)` without it. It now fires ONLY where the colorant buffer did not run, so a subtractive page that composited in ink reports zero here — read it with `cmyk_buffer`, never alone) |
//! | `cmyk_buffer` | `cmyk_buffer_engaged` (a `bool`, printed `0`/`1`) | "did this page composite in INK, or in sRGB?" (THE KEY THAT CHANGES WHAT THE PREVIOUS ONE MEANS, and the only non-count on this half of the line — a parser treating every metrics key as a magnitude will misread it. At `1`, the blends `blends_in_wrong_space` counted were PERFORMED subtractively: that counter is fixed at `/BM`-selection time and measures exposure to §11.3.4, not failure. Read the pair, never the second alone) |
//! | `blend_space_from_output_intent` | `blend_space_from_output_intent` | "did pdfcer INFER this page's blending space from the output intent?" (DISCLOSURE, and the only key here that is a word rather than a number. `page_group` — the page declared `/Group /CS`, Table 147, nothing inferred. `device_native` — ISO 32000-1 §11.4.7/§11.6.3's answer for a page that declared none, which for pdfcer is sRGB. `output_intent` — **pdfcer INFERRED it from the document's output intent**, which ISO 32000-2's Annex P permits *informatively and without ranking it against the device*, so this is a choice the `page_blend_space_source` setting controls and not a fact about the file. Read it beside `blend_space_subtractive`: that one says a page composited in ink, this one says whether the FILE asked for that or pdfcer decided it. A blending space changes every colour on the page and draws nothing to say so, which is why it is disclosed here rather than left to be deduced) |
//! | `cmyk_buffer_refused` | `cmyk_buffer_refused` | "did the page ask for ink and not get it?" (DIVERGENCE with a named cause — the colorant buffer would not fit under `MAX_CMYK_BUFFER_BYTES`, a page-size ceiling. Non-zero means this render is the pre-colorant-buffer approximation and SAYS SO rather than failing, and it is the reason `cmyk_buffer=0` on a page whose `blend_space_subtractive` is non-zero) |
//! | `cmyk_bridged_pixels` | `cmyk_bridged_pixels` | "how much of this ink page was never authored as ink?" (pixels that entered the colorant buffer through the sRGB BRIDGE. **THE POPULATION THIS COUNTS HAS SHRUNK TWICE AND THE ROW HAS BEEN WRONG AFTER EACH — a script comparing this number across either Pass is comparing two different questions.** It used to say "shadings, the results of transparency groups, and any image NOT authored in `DeviceCMYK`". Images left in `Pass 130.1`: a `DeviceCMYK` image, including one behind an `/Indexed` palette, carries its colorants forward and is counted in `cmyk_native_image_pixels` instead, so what remains of that class is an image with **no ink to keep**. ANALYTIC SHADINGS left in `Pass 137.0`: an axial, radial or function-based shading whose ramp carries colorants now composites natively whether or not overprint is in force. MESH SHADINGS left in `Pass 137.1`, ONE COMMIT after this row was rewritten to say they were what remained — `Shade::Ink` gave them the carrier they lacked. `Separation`/`DeviceN` IMAGES left in `Pass 140.0`, directly and behind an `/Indexed` palette: they convert to their `DeviceCMYK` alternate now rather than to sRGB. WHAT IS LEFT: images and meshes with **no ink to keep** (an additive colour space, a `Separation`/`DeviceN` over a non-`DeviceCMYK` alternate, or a parametric mesh whose ramp carries no colorants), and the results of transparency groups. THIS ROW HAS NOW BEEN WRONG FOUR TIMES, each by standing still while the code moved, and each correction was written by somebody who had just read it and believed it — a description that enumerates a POPULATION is a claim that decays whenever the population changes, and nothing compiles it. ⇒ **A FALL HERE IS THE INTENDED OUTCOME, NOT A COUNTER GOING QUIET.** It measures ink identity LOST on the way to the compositor; when less is lost it reports less, and reading it as "how much shading work happened" turns four fixes into four apparent regressions) |
//! | `cmyk_native_image_pixels` | `cmyk_native_image_pixels` | "how much of this ink page KEPT its ink?" (pixels an image contributed with no conversion in either direction — a `DeviceCMYK` image, an `/Indexed` image over a `DeviceCMYK` base, a `Separation`/`DeviceN` image over a `DeviceCMYK` alternate, or an `/Indexed` image over such a base. This row said only the FIRST of those four until `Pass 140.0`. The complement of the row above, and not interchangeable with it: a bridged pixel has been through `CMYK → sRGB → CMYK`, and that first step is MANY-TO-ONE, so the ink that returns is not the ink that left) |
//! | `cmyk_groups_approximated` | `cmyk_groups_approximated` | "how many groups on an ink page had their RESULT composited in ink and their INTERIOR not?" (DIVERGENCE. **THIS KEY NARROWED IN `Pass 97.1g` and a reader comparing boards across that Pass must know it.** It used to count TWO populations: a KNOCKOUT group, whose §11.4.6 semantics are preserved but whose interior runs in sRGB — still counted — and EVERY NON-ISOLATED group, on the reasoning that all of them had §11.4.4's backdrop removal skipped. The second population is gone: a non-isolated group now gets its second content walk and its removal. What is left of it is the allocation-failure fallback alone, where the second buffer could not be had. ⇒ **A DROP IN THIS NUMBER ACROSS `97.1g` IS NOT ALL RENDERING IMPROVEMENT.** Measured on the print-conformance suite: 118 → 0, of which only 13 groups actually needed the walk; the other 105 were counted as approximations while rendering exactly right, because the old test asked "is this group non-isolated?" rather than §11.4.4 NOTE 2's "does its interior read the backdrop?". An ordinary isolated group is NOT counted — it gets a child colorant buffer and crosses no conversion at all) |
//! | `cmyk_unbridged_images` | `cmyk_unbridged_images` | "did an image reach a subtractive paint with no bridge and therefore not get painted AT ALL?" (should always be zero: the only route is a replayed display list, and a subtractive page is refused for recording outright. Counted rather than asserted because a claim of unreachability decays as the code around it changes, and a counter that stays zero costs one `u64` and one line of output. Non-zero here is a bug report, not a document property) |
//!
//! `images` and `forms` are *volume*, not shortfall — they are non-zero
//! on a perfectly faithful render and exist so a batch pipeline can tell
//! "this page has no images" apart from "this page's images all failed."
//! `dct_cmyk` is likewise pure volume: decision 006 verified that
//! YCCK-storage JPEGs decode without polarity ambiguity
//! (pixel-matching pdfium), so the counter is a neutral census and no
//! stderr note accompanies it. Its former companion warning ("check
//! the colours") cried wolf on known-good files and was retired by the
//! 006 split. `dct_cmyk_unverifiable` is the half that still deserves
//! attention: a 4-component JPEG with effective `ColorTransform` 0 and
//! no `/Decode` is the one shape whose polarity genuinely cannot be
//! verified (rule R30 — reported, never repaired), and any sighting is
//! a decision 006 §9 revisit trigger. It sits at the END of the line
//! because keys are appended, never reordered.
//!
//! The six `unsupported_*` tokens are the **by-reason breakdown** of
//! `unsupported` (`fonts_unsupported_by_reason`, keyed by
//! `pdfcer_render::text::UnsupportedFont::reason_key`): their sum equals
//! `unsupported`, and they are emitted in a fixed order even at zero so
//! the line stays diffable. They answer "*why* was text skipped?" without
//! re-instrumenting the loader (rule R20):
//!
//! | token | reason | meaning |
//! |---|---|---|
//! | `unsupported_type3` | `Type3` | a Type 3 font (ISO 32000-1 9.6.5, content-stream glyphs) that pdfcer could not build a model for. **This meant "Type 3 is deferred" until `Pass 126.0`, when Type 3 began rendering.** It now means only that Table 112's IRREDUCIBLE entries are missing -- `/CharProcs` (no glyph descriptions exist) or `/FontMatrix` (no mapping from glyph space to text space, and guessing the conventional `[0.001 ...]` would render a nonstandard font a thousand times too large). Everything else recovers; a font with no usable `/Encoding` in particular is NOT counted here, because 9.6.6.3 makes that a font whose every code resolves to no glyph -- a blank page by the standard rather than a feature pdfcer lacks. See `type3_glyphs` for the census of what DID render |
//! | `unsupported_noncmap` | `NonIdentityCmap` | `Type0` with a non-`Identity-H` CMap, deferred |
//! | `unsupported_vertical` | `VerticalWriting` | `Identity-V` vertical writing, deferred |
//! | `unsupported_composite_not_embedded` | `CompositeNotEmbedded` | `Identity-H` with no embedded program — supply the font |
//! | `unsupported_unknown_subtype` | `UnknownSubtype` | `/Subtype` absent/unrecognized |
//! | `unsupported_unusable_program` | `UnusableProgram` | an embedded program pdfcer could not parse |
//!
//! `unsupported_unusable_program` is the load-bearing one for the
//! embedded-font-rendering class: a non-zero count is the exact signal
//! that once caught the `0x00010000`-sfnt whitespace-trim misroute which
//! sent every embedded TrueType to the CFF parser.
//!
//! `codec_features` is a **sum**, because the underlying counter is a
//! map keyed by feature name (`DCT/arithmetic`, `DCT/12-bit`, …) and the
//! machine line's contract is `key=<non-negative integer>`. The per-name
//! breakdown — which is the part an operator actually acts on — goes to
//! stderr, where it cannot break a parser.
//!
//! Any non-zero shortfall counter also triggers a **human-readable
//! expansion on stderr** (substituted font names, sample operator names,
//! the specific codec an image needed) — detail that would bloat the
//! machine line, placed where it cannot break a parser.
//!
//! ### `round-trip`'s counters (Pass 3.0)
//!
//! Same two-half split: `round-trip <input> mode=<M> -> <output>` is the
//! narrative half (paths may contain spaces; `mode` is a name, not an
//! integer, and lives here for exactly that reason), then `"; "`, then
//! `key=<non-negative integer>` pairs in the fixed order below.
//!
//! | key | meaning |
//! |---|---|
//! | `identical` | did the mode's byte-identity promise hold? (see below — the promise differs per mode) |
//! | `in_bytes` / `out_bytes` | input and output file sizes |
//! | `appended` | bytes written past the input's original length; `0` for a no-op incremental save |
//! | `objects` | object definitions emitted by this save |
//! | `verbatim` | of those, how many were copied byte-for-byte from the retained source |
//! | `reserialized` | of those, how many were rebuilt from values — every one is a byte-level divergence, counted rather than rounded away |
//! | `reloaded` | did `pdfcer-core` parse back what it wrote? |
//! | `raster_compared` | was the semantic oracle able to run? (`0` when page 1 does not render, which is not a failure) |
//! | `raster_identical` | did page 1 re-render to identical pixels? |
//! | `delinearized` | did this save spend a live Annex F Fast Web View property? |
//!
//! **`identical=1` means three different things, by mode**, and this is
//! the distinction decision 007 W1 calls the likeliest source of a false
//! green or a false red:
//!
//! - `--mode incremental` — the output is byte-identical to the input,
//!   **whole file**. Zero edits means zero bytes.
//! - `--mode append-identity` — every byte below the input's original
//!   EOF is unchanged (§7.5.6), with a new revision appended.
//! - `--mode full` — every `File`-provenance object's **definition
//!   bytes** appear verbatim. Never whole-file: a full rewrite moves
//!   object offsets, so the cross-reference section must differ, and a
//!   whole-file comparison would fail on every input.
//!
//! ### The editing subcommands' counters (Pass 3.1)
//!
//! `set-info` and `rotate-page` share a counter tail, because they share
//! everything that matters: both go through the **same command log**
//! (`pdfcer_core::edit::EditSession`) that the GUI uses, and both save
//! through the same writer. There is no CLI-only mutation path, and that
//! is the point of the GUI-core separation rather than an accident of
//! this Pass.
//!
//! ```text
//! set-info      <input> mode=<M> -> <output>; \
//!               changed=<N> objects=<N> verbatim=<N> reserialized=<N> \
//!               promoted=<N> appended=<N> out_bytes=<N> info_created=<0|1> \
//!               undo_verified=<0|1> undo_identical=<0|1> delinearized=<0|1>
//! rotate-page   <input> page <P> mode=<M> -> <output>; \
//!               rotate=<D> changed=<N> objects=<N> verbatim=<N> \
//!               reserialized=<N> promoted=<N> appended=<N> out_bytes=<N> \
//!               undo_verified=<0|1> undo_identical=<0|1> delinearized=<0|1>
//! ```
//!
//! | key | meaning |
//! |---|---|
//! | `changed` | objects that currently differ from the base revision — the save-time diff, **not** a count of commands run |
//! | `promoted` | objects moved out of an object stream because they were touched (R38) — a representation change worth disclosing |
//! | `info_created` | `1` when the file had no `/Info` dictionary and one was created for the operator's metadata |
//! | `undo_verified` | `1` when `--verify-undo` ran the edit → undo → save check |
//! | `undo_identical` | `1` when that check produced a file byte-identical to the input |
//!
//! `changed=0` is a legitimate, successful outcome: asking for a
//! rotation a page already has, or a title it already carries, changes
//! nothing and therefore writes nothing. The output file is then a byte
//! copy of the input, `appended=0`, and a note goes to stderr. Silently
//! appending an empty revision instead would be the exact "zero edits
//! means zero bytes" violation the writer refuses to commit.
//!
//! ### `--verify-undo`, and why it is a real flag rather than a test hook
//!
//! With it, the tool performs the edit, then **undoes it and saves
//! again**, and checks that the second save is byte-identical to the
//! input. That is `ARCHITECTURE.md` §11.1's contract — the dirty set is a
//! diff against the base, never the union of commands run — evaluated
//! against *this operator's document* rather than against a fixture. A
//! batch pipeline that is about to edit ten thousand signed contracts can
//! use it as a pre-flight on a sample. It costs one extra save, so it is
//! off by default; a failure exits [`exit::NOT_BYTE_IDENTICAL`], because
//! it is a correctness result, not a crash.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use pdfcer_core::PdfError;
use pdfcer_core::document::Document;
use pdfcer_core::outline::{DestView, Destination, DestinationReader, RemoteTarget};
use pdfcer_core::pageops::{DocumentView, InsertPosition, PageOpError, SplitCriterion};
use pdfcer_core::signature::{SaveMode as CoreSaveMode, SignatureImpact};

mod cli;
mod clipboard;
mod tesseract;
use cli::*;
mod arg_types;
use arg_types::*;
mod dispatch;
use dispatch::*;
mod inspect;
use inspect::*;
mod ocr_cmd;
use ocr_cmd::*;
mod render_export;
use render_export::*;
mod navigation;
use navigation::*;
mod listing;
use listing::*;
mod print;
use print::*;
mod fields;
use fields::*;
mod edit_common;
use edit_common::*;
mod page_edit;
use page_edit::*;
mod text_edit;
use text_edit::*;
mod fonts;
use fonts::*;
mod security;
use security::*;
mod annot_parse;
use annot_parse::*;
mod extract;
use extract::*;
mod sign;
use sign::*;
mod dimension;
use dimension::*;
mod field_edit;
use field_edit::*;
mod annot_edit;
use annot_edit::*;
mod image;
use image::*;
mod objects;
use objects::*;
mod pages;
use pages::*;
mod stamp;
use stamp::*;
mod offpage;
use offpage::*;
mod structure;
#[cfg(test)]
mod tests;
use structure::*;

/// Exit-code assignments for pdfcer's scriptable contract.
///
/// These are stable, documented values — changing one is a
/// backwards-incompatible change to any script that branches on the exit
/// code, so treat them the way the public API surface is treated. New
/// failure modes get new codes rather than reusing an existing one with a
/// broadened meaning.
mod exit {
    /// Everything succeeded.
    pub const SUCCESS: u8 = 0;
    /// A generic runtime failure with no more specific code.
    pub const RUNTIME_ERROR: u8 = 1;
    // 2 is reserved by clap for CLI-usage / argument-parse errors.
    /// The input file could not be opened or read (not found, permission
    /// denied, I/O error). Maps from [`pdfcer_core::PdfError::Io`].
    pub const IO_ERROR: u8 = 3;
    /// The input is not a PDF, or its header is malformed. Maps from
    /// [`pdfcer_core::PdfError::MissingHeader`] /
    /// [`pdfcer_core::PdfError::MalformedVersion`].
    pub const NOT_A_PDF: u8 = 4;
    /// `round-trip`: the save completed, the output reloads, but it is
    /// **not byte-identical** to the input where the mode promised it
    /// would be.
    ///
    /// This is the Pass 3.0 headline gate's failure code. It is
    /// deliberately distinct from [`RUNTIME_ERROR`]: nothing crashed and
    /// nothing refused — pdfcer produced a working PDF that differs from
    /// its input, which is a violation of `ARCHITECTURE.md` §5's
    /// round-trip invariant and a *correctness* result, not an error.
    /// A corpus script needs to count these separately.
    pub const NOT_BYTE_IDENTICAL: u8 = 5;
    /// `round-trip`: the save produced bytes, but `pdfcer-core` could not
    /// load them back.
    ///
    /// Strictly worse than [`NOT_BYTE_IDENTICAL`] — the writer emitted a
    /// file that is not a valid PDF by pdfcer's own reckoning.
    pub const RELOAD_FAILED: u8 = 6;
    /// `round-trip`: the output reloads, but re-rendering page 1 at the
    /// same scale produces a **different raster** than the input does.
    ///
    /// The semantic oracle. Byte identity is a syntactic claim; this is
    /// the one that says the document still *means* the same thing.
    /// Available only because the render stack shipped before the
    /// writer.
    pub const RASTER_DIFFERS: u8 = 7;
    /// `round-trip`: pdfcer **refused** the requested save by name — e.g.
    /// a full rewrite of a §7.5.8.4 hybrid-reference file, which would
    /// destroy the file's pre-1.5 readability.
    ///
    /// A refusal is a correct outcome, not a defect, so it gets its own
    /// code: a corpus run must be able to tally "declined, by name" apart
    /// from "produced a wrong file".
    pub const SAVE_REFUSED: u8 = 8;
    /// An **edit** was refused by name before any save was attempted —
    /// a rotation that is not a multiple of 90 (ISO 32000-1 Table 30), a
    /// page index past the end of the document, a malformed `/Info`.
    ///
    /// Distinct from [`SAVE_REFUSED`] (which is about writing) and from
    /// [`RUNTIME_ERROR`] (which is about failing): the document was
    /// readable and pdfcer declined to perform the operation as asked.
    /// A batch script needs to tell "this file is unsuitable for this
    /// edit" apart from "this file is broken".
    pub const EDIT_REFUSED: u8 = 9;
    /// A redaction **apply** completed the removal but the diligence
    /// carrier sweep disclosed a residual it could not scrub (XFA, a
    /// structure-tree ActualText copy, an embedded file), and the operator
    /// did **not** pass `--acknowledge-residuals`. The output was still
    /// written (the covered content IS removed), but the non-zero code
    /// forces a script to see the disclosure — the refusal-acknowledgement
    /// gate (ui-spec §4.4): no path where partial reads as complete.
    pub const REDACTION_RESIDUALS: u8 = 10;
    /// The document **opened**, but only via cross-reference **recovery**
    /// (decision 013): its stored cross-reference table could not be parsed
    /// and pdfcer rebuilt it by scanning for `N G obj` headers
    /// (rebuild-by-scan). The content is available, but a batch script
    /// needs to tell "opened clean" from "opened via recovery" — a
    /// recovered document forces a full-rewrite save (incremental is
    /// refused) and its bytes were reconstructed, not read as authored.
    /// A distinct, documented status per the R20 counted-diagnostics
    /// tradition (fuzzy-never-sneaky).
    pub const OPENED_VIA_RECOVERY: u8 = 11;
    /// `verify-signatures`: at least one signature FAILED integrity — the
    /// bytes under it were altered after signing, or its signature value
    /// does not verify with its own certificate. The output was printed;
    /// this is the one exit code a script must treat as "do not trust
    /// this document".
    pub const SIGNATURE_FAILED: u8 = 12;
    /// `verify-signatures`: no signature failed, but at least one could
    /// not be verified — a subfilter, algorithm or curve pdfcer does not
    /// implement, a malformed CMS, a missing certificate. Named in the
    /// output. Distinct from [`SIGNATURE_FAILED`](Self::SIGNATURE_FAILED)
    /// because "pdfcer cannot say" is not "the document was tampered with".
    pub const SIGNATURE_UNVERIFIABLE: u8 = 13;
    /// The subcommand exists in the surface but is not implemented yet
    /// (Pass 0 stub). Distinct code so a script can tell "you asked for a
    /// feature pdfcer doesn't have yet" apart from a real failure.
    pub const UNIMPLEMENTED: u8 = 64;
}

/// pdfcer — a scriptable PDF toolkit (open-source Acrobat-Pro-parity engine).
/// The multi-line provenance banner behind `pdfcer --version`.
///
/// # Why this is a leaked `String` rather than a `const`
///
/// `clap`'s `long_version` wants a `&'static str`, and the banner is
/// *formatted* from [`pdfcer_core::build::BuildInfo`] rather than being a
/// literal. Leaking one small allocation, once, in a process that is about to
/// print it and exit is the honest trade; the alternatives are a `OnceLock`
/// whose value can never be dropped anyway, or duplicating the format string
/// as a `const` that would then have to be kept in step with `BuildInfo`'s
/// own `Display`.
///
/// # What it prints, and why the second timestamp earns its line
///
/// The crate version, the build time, the git revision, the *commit* time,
/// and the `iccce` line. Build time and commit time together say how stale
/// the source was when the binary was made — a build from today off a
/// six-week-old commit is a different situation from one off this morning's,
/// and only the pair distinguishes them.
///
/// The `iccce` line reports the colour engine's version, the pin the
/// manifest asked for, the resolved git revision and when that revision was
/// committed — all four halves of what the operator asked for on 2026-08-18.
///
/// **It said `not-linked-yet` from `Pass 199.2` to `Pass 223.0`, and that
/// was false for six days.** The dependency landed and the stamp went on
/// announcing that it had not. Worth remembering as a shape rather than as
/// an incident: the disclosure was accurate when written, was falsified by
/// an improvement to the very thing it described, and nothing failed —
/// because the code that would have noticed was waiting on a signal
/// (`DEP_ICCCE_PROVENANCE`) that its subject never emits.
fn build_banner() -> &'static str {
    let b = pdfcer_core::build::BuildInfo::current();
    // The FIRST line is the version alone, because clap already prints the
    // binary name in front of whatever this returns -- rendering `BuildInfo`
    // directly gave "pdfcer pdfcer 0.7.0", the product named twice. The
    // remaining lines mirror `BuildInfo`'s own `Display` on purpose, so a
    // stamp copied out of `--version` and one copied out of a crash report
    // read the same.
    let mut text = format!(
        "{}\n  built:     {}\n  revision:  {}\n  committed: {}\n  iccce:     {}",
        b.version, b.built_at, b.revision, b.committed_at, b.iccce
    );
    // Mirrors `BuildInfo`'s own Display, deliberately -- a stamp copied out
    // of `--version` and one copied out of a crash report must read the same.
    // This branch is the out-of-workspace case and never fires for a shipped
    // pdfcer, which links pdfcer-render and therefore links iccce.
    if b.iccce == "not-linked" {
        text.push_str(" (this build links pdfcer-core alone; iccce is pdfcer-render's dependency)");
    }
    if b.is_dirty() {
        text.push_str(
            "\n  NOTE: built from a MODIFIED working tree - this binary is not the commit it names",
        );
    }
    Box::leak(text.into_boxed_str())
}

/// Run the CLI on a worker thread with a generous stack.
///
/// clap's **debug-build** argument-tree validation (`debug_assert`, compiled
/// only under `debug_assertions`) recurses deeply enough that, for a command
/// tree this size, it overflows the small default **main-thread** stack on
/// Windows/MSVC (~1 MB) — a release build is unaffected (no `debug_assert`,
/// and optimized frames). Running the whole program on a spawned thread with
/// a 16 MB stack sidesteps it portably; on failure to spawn, we fall back to
/// the main thread rather than aborting.
fn main() -> ExitCode {
    match std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
    {
        Ok(handle) => handle.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => run(),
    }
}

/// Strip source-only markup from one shipped `--help` string.
///
/// In `clap`-derive a `///` doc comment IS the operator-facing help text, so
/// this crate's doc comments serve two readers at once: `cargo doc`, which
/// renders Markdown, and a terminal, which does not. The house style writes
/// for the first — a bold lead-in per subcommand, backticks around flags and
/// PDF keys, and the originating Pass ID in a trailing parenthetical — and the
/// second ships it verbatim: `**Send pages to a printer.**` with the asterisks
/// visible, and `(Pass 155.0)` naming an internal identifier that means
/// nothing outside this repository.
///
/// The transform is applied at startup rather than in the source because the
/// bold lead-in is load-bearing: `tools/check-cli-help-leads.py` uses `**` at
/// the start of a `///` line as its structural marker for "this is a summary",
/// and that is how it catches a summary spliced into the preceding variant's
/// doc block. Scrubbing the source would blind that gate; scrubbing the
/// rendered string keeps both readers correct.
///
/// # What it removes
///
/// * `**` and `` ` `` — Markdown emphasis and code spans, which no terminal
///   renders.
/// * Pass IDs inside a parenthetical — `(Pass 5.4)` goes entirely,
///   `(Pass 6.1, §12.5.6)` keeps the clause citation. Spec citations are kept
///   deliberately: they are true outside this repository and a technical
///   operator can look them up, which is the opposite of a Pass ID.
///
/// A Pass ID written into running prose rather than a parenthetical is left
/// alone here and rejected by `cli_help_ships_no_internal_markup`, because
/// rewriting a sentence is an authoring decision and a silent half-fix would
/// read as a clean one.
fn plain_help(s: &str) -> String {
    let out = drop_pass_id_parentheticals(s);
    out.replace("**", "").replace('`', "")
}

/// Whether one comma/semicolon-separated item inside a parenthetical is purely
/// an internal Pass reference — `Pass 5.4`, `` `Pass 182.0/183.0` ``,
/// `Pass 7`. Markup is stripped before the test so a backticked ID is not
/// missed.
fn is_pass_reference(item: &str) -> bool {
    let t = item.replace("**", "").replace('`', "");
    let t = t.trim();
    let Some(rest) = t.strip_prefix("Pass ") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '/')
}

/// Remove Pass references from every parenthetical in `s`, and remove the
/// parenthetical itself when nothing else was in it.
///
/// Parentheticals are found by depth so a nested pair cannot truncate the
/// scan early. The items inside are re-joined with `", "`, which normalises a
/// doc comment's hard-wrapped whitespace at the same time — clap has already
/// joined the source lines into one paragraph by the time this runs.
fn drop_pass_id_parentheticals(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '(' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut end = None;
        for (j, c) in chars.iter().enumerate().skip(i) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            // Unbalanced: copy the rest verbatim rather than guess.
            out.extend(&chars[i..]);
            break;
        };
        let inner: String = chars[i + 1..end].iter().collect();
        let kept: Vec<&str> = inner
            .split([',', ';'])
            .map(str::trim)
            .filter(|item| !item.is_empty() && !is_pass_reference(item))
            .collect();
        if kept.is_empty() {
            // Nothing but Pass IDs: drop the parenthetical, and the single
            // space that separated it from the preceding word, so the line
            // does not end in " ." or a double space.
            if out.ends_with(' ') {
                out.pop();
            }
        } else {
            out.push('(');
            out.push_str(&kept.join(", "));
            out.push(')');
        }
        i = end + 1;
    }
    out
}

/// Apply [`plain_help`] to a command's own help text, its arguments' help
/// text, and — recursively — every subcommand.
///
/// `mut_subcommand` is driven from a pre-collected name list because the
/// closure takes the subcommand by value, so the parent cannot be borrowed
/// while the iteration runs.
fn scrub_help(cmd: clap::Command) -> clap::Command {
    let about = cmd.get_about().map(|s| plain_help(&s.to_string()));
    let long_about = cmd.get_long_about().map(|s| plain_help(&s.to_string()));
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();

    let mut cmd = cmd.mut_args(|arg| {
        let help = arg.get_help().map(|s| plain_help(&s.to_string()));
        let long_help = arg.get_long_help().map(|s| plain_help(&s.to_string()));
        let mut arg = arg;
        if let Some(h) = help {
            arg = arg.help(h);
        }
        if let Some(h) = long_help {
            arg = arg.long_help(h);
        }
        arg
    });
    if let Some(a) = about {
        cmd = cmd.about(a);
    }
    if let Some(a) = long_about {
        cmd = cmd.long_about(a);
    }
    for name in names {
        cmd = cmd.mut_subcommand(name, scrub_help);
    }
    cmd
}
