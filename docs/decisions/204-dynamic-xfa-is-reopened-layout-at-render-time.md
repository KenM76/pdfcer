# Decision 204: dynamic XFA is reopened; layout is computed at render time and never rewrites the file on open

Recorded at the 1271st filing, 2026-10-28, on the operator's instruction (Ken: "looks like it is time to add dynamic xfa support"). Reverses decision 020 §3.2.1 (dynamic XFA out of scope) and answers open operator question (p) in favour of support. Static-XFA hybrid field *creation* stays refused (decision 020). Trigger context: `Pass 646.25` (`G237`, `5f401310`) shipped detection, `pdfcer_core::xfa::form_kind`.

## Decides

1. **Dynamic XFA is in scope.** Arc `Pass 647.0`-`647.7` (packets/template reader; positioned layout and pageSet pagination; flowed `tb`/`lr-tb` layout; normal data merge; render path; CLI `xfa-layout` and `render-page`; explicit materialize verb; table/row layout, blocked on a spec slice).
2. **Round-trip invariant (ARCHITECTURE §5).** Opening a dynamic form must not rewrite its structure. Layout is computed at render time, as decision 203 does for `/AP`-less markup. Writing the generated pages and fields into the file is a separate explicit verb that commits at Save.
3. **Fuzzy-never-sneaky (rule 4).** The computed layout is inferred content: it renders exactly as saved content would, with no on-canvas marking, and is disclosed off-canvas (diagnostics, stderr, report). Content a dynamic form drops on overflow is disclosed too; the spec only warns about it.
4. **No external code relied on (operator).** pdfcraft (`github.com/storytold/pdfcraft`, MIT OR Apache-2.0, local clone `D:\Dev\pdfcraft\upstream`, crate `crates/xfa`, about 11k lines) has a dynamic-XFA engine. Allowed uses only: (a) behavioural and scope reference; (b) an out-of-tree differential oracle built in pdfcraft's own target directory, never a dependency of any shipped crate; (c) adapting its synthetic fixture generators as test inputs with MIT attribution (`LEGAL.md` §5 permits synthetic fixtures). No code copying. Its design materializes pages on open, which pdfcer rejects under point 2.
5. **Defaults.** Spec gaps in the extracted XFA slice (gap IDs G-X/S/T/L/B in `adobe_ext__xfa_{xdp_packaging,static_vs_dynamic_rendering,template_layout_dom,layout_algorithm,data_binding_normal}.md`) are the arc's default-decisions list; where two answers are defensible, both are supported behind a setting (R169).
6. **Pace.** The operator asked for slow progress this week; nothing is scheduled. Spec slices still owed from `pdfcer-spec-librarian`: SOM expressions, then table/row layout.

## Why

The operator's call, against the measured demand (`/XFA` in 2 of 2,500 organic files, 0.08%, decision 008 census) and XFA's deprecation in ISO 32000-2: the demand argument that put it out of scope is outweighed by the operator's wish. A dynamic form opens in pdfcer as its placeholder page; computing layout at render time fixes that while keeping the file untouched (the same ground as decision 203) and `pdfcer-core`/`pdfcer-render` GUI-free.
