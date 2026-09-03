# pdfcer UI Preferences — Design System (egui/eframe terms)

**Author:** `pdfcer-ui-specialist` · **Date:** 2026-08-05
**Drives:** the operator's design-philosophy handoff,
`C:\Users\Ken\AppData\Local\Temp\claude\D--Dev-KenAgent\ede777ff-2525-4b85-b93f-95b9674b0040\scratchpad\pdfcer_gui_design_handoff.md`,
which the operator instructed be followed while the Tool Options dock
(Pass 34.1/34.2, `docs/ui_specs/tool-options-dock-and-ce-dimension-
properties.md`) is implemented.

**What this document is.** The handoff is a philosophy + process
document, not a layout spec — it names principles (calibrated not
templated, distinctive not AI-aesthetic, both themes equally,
information design first, structure encodes information, spend
boldness in one place, avoid generic defaults) and asks for a "process
before code" step (sketch palette/type/layout, review for uniqueness,
build to the plan). This document IS that sketch-and-review step, and
it is the design system every future pdfce-gui surface should read
from. It does not redesign the toolbar/ribbon (the handoff's "Option
B") — that is separately-scoped work this document supplies tokens
for, not a decision made here.

**What this document is not.** Several of the handoff's *mechanisms*
are literally CSS/web concepts — token-level custom properties,
`@media (prefers-color-scheme: dark)`, `data-theme` overrides,
`@font-face` data URIs, gradient heroes, rounded cards with accent
bars. pdfcer is native Rust + egui/eframe; none of those mechanisms
exist here as written. §2 below translates each one honestly to its
egui equivalent, and says explicitly where no clean translation exists
yet rather than quietly dropping the principle or pretending to
implement CSS inside egui.

**Terminology (CLAUDE.md rule 15, binding throughout).** Where this
document discusses dimension objects (the canvas-overlay palette, §4),
every one is a **ce dimension** — a `/Line`+`/IT /LineDimension`
annotation pdfcer itself authors — never a **pdf dimension** (a
pre-existing CAD-exported callout pdfcer did not create). This document
does not concern pdf dimensions anywhere.

---

## §0. Process record — palette / type / layout sketch, and the uniqueness review

Per the handoff's own "process before writing code" step: state the
plan first, review it for genericness, then let §§3–9 below be the
buildout of exactly this plan and nothing else.

**Palette sketch (6 named roles + a hue-bias note):**
`ACCENT_PRIMARY` (deep violet, ~260° hue, the one place boldness is
spent), `GOOD` (muted forest green, new — nothing today uses green),
`WARNING`/`CRITICAL` (not new colors — aliased to egui's own
`Visuals::warn_fg_color`/`error_fg_color`, already in use in this
codebase, see §3), a neutral scale **inherited from egui's own
`Visuals::light()`/`dark()`** rather than replaced (see §3's reasoning
for why replacing it is out of scope for v1), and a **second, disjoint
vocabulary** of canvas-overlay colors (amber/orange/teal/blue — see
§4) that already exists in the codebase and is being formalized, not
invented.

**Typeface sketch:** two ROLES (proportional for chrome/body/heading,
monospace for data/numeric readouts), currently both filled by egui's
own bundled defaults with zero custom font files loaded anywhere in
`pdfce-gui` (verified — see §7). Recommendation: keep it that way for
v1, name the two roles honestly, and treat "bundle a chosen, licensed
display face" as a real, separately-scoped, license-gated decision
(rule 13), not a silent given.

**Layout sketch (the handoff's "1–2 sentences" ask):** property
surfaces are two-column label/control grids (§9), panel hierarchy is
carried by text weight and a plain separator rule rather than by a
colored accent bar under headers, and nothing in pdfcer's chrome is
centered by default — labels sit left, controls fill the remaining
width, and an empty-state caption sits exactly where the populated
content would have sat rather than being recentered, so arming a tool
does not relocate anything the operator's eye was already resting on.

**Uniqueness review — the handoff's own avoid-list (Part 1 item 8),
checked one by one, not rubber-stamped:**

| Avoid-list item | Verdict | Why |
|---|---|---|
| Cream + serif + terracotta | **N/A** | No serif introduced; `ACCENT_PRIMARY` is violet, not terracotta; no cream — neutrals stay egui's own. |
| Hairline rules with dense columns | **Addressed** | Property rows use `egui::Grid` spacing (§9), not a rule drawn under every row. |
| Gradient heroes on white | **N/A** | pdfcer ships no hero/marketing surface; explicit recommendation in §9 to keep all chrome fills flat regardless. |
| Accent bars on rounded cards everywhere | **Addressed** | Panel section headers (§9) carry hierarchy by weight + one plain separator, not a colored bar. |
| Everything centered | **Addressed** | Property-row layout and the Tool Options empty state (§9) are both left/top-aligned, not centered — cited to the existing "shell should hold still" precedent so a tool arming doesn't relocate the caption. |
| Generic sans-serif with no type scale | **Fixed by this document** | §6 names a 5-role scale with concrete point sizes. This was a real, confirmed gap — zero custom `TextStyle` configuration exists in `pdfce-gui` today (grepped, not assumed). |
| Silent font fallbacks | **Honestly NOT fixed — flagged, not concealed** | §7: pdfcer's only "font" today is whatever egui bundles. Naming this plainly, with a scoped P2 recommendation, is the intended output of this review step — not a clean pass. |

The last row matters most: this section exists specifically so an
open gap gets written down instead of glossed over. A "process before
code" review that reports 7/7 clean passes would itself be a warning
sign.

---

## §1. The one distinction that governs every color decision here

pdfcer's colors fall into two domains that must never be reasoned about
with the same rule:

1. **Chrome colors** — panel fills, tab backgrounds, button colors,
   text colors, separators. These are the "surface of the app," and
   they are correctly **theme-aware**: they must route through
   `egui::Visuals`/`ui.visuals()`, never a bare `Color32` literal.
2. **Canvas-overlay colors** — node marks, ce-dimension selection/drag
   outlines, live-preview drafts, redaction ink, translucent masks.
   These draw **on top of the rendered PDF page**, and a PDF page's
   background is near-white in essentially every real document pdfcer
   opens, regardless of which theme the operator's app chrome is set
   to. These are correctly **theme-invariant** — a bare, named
   `Color32` constant is the RIGHT mechanism, not a bug to fix.

This distinction is not a new idea invented for this document — it is
already implicit in the codebase's own comments. `NODE_MARK_FILL`'s
doc comment (`main.rs:307-315`) states it outright: *"The page under a
node is white in every document pdfcer has drawn, and where it is not,
the `NODE_MARK_COLOR` border is what carries the mark."* That is domain
1 reasoning applied correctly to a domain-2 color; this document's job
is to name the distinction so the NEXT engineer who adds an overlay
color does not "fix" it into a `ui.visuals()` read and make it
invisible against the still-white page underneath a dark chrome theme.

**Consequence for the handoff's own audit (Part 2 §4):** the stated
"Gap" — *"verify all custom colors in your code respect theme
context"* — is right for domain 1 and **wrong if applied uniformly to
domain 2**. Every canvas-overlay literal should be reviewed for
whether it is *duplicated* (it mostly is — see §4) and *misnamed as an
accident* (mostly not — see §4's one real drift finding), not for
whether it "respects theme." Making canvas-overlay colors theme-aware
would be a regression, not a fix.

---

## §2. Mechanism translations — CSS concept → egui equivalent

| CSS/web concept (handoff's wording) | Why it doesn't map directly | egui translation | Where it lives |
|---|---|---|---|
| Token-level CSS custom properties | No cascading stylesheet exists; egui has no equivalent of a custom property that every consumer re-reads | A Rust module of named `const Color32` values (canvas-overlay, §4) **plus** two functions building customized `egui::Visuals` (chrome, §3) — "set once, everything downstream inherits" is achieved by overriding specific `Visuals` fields once at startup, and every widget that reads `ui.visuals()` picks up the change for free | New module, e.g. `crates/pdfce-gui/src/style.rs` (engineer's call on exact file boundary) |
| `@media (prefers-color-scheme: dark)` | Not a stylesheet media query; egui/eframe has its OWN theme-preference mechanism | `egui::Context`'s theme-preference handling (verify the exact 0.35 API against docs.rs before implementing — recommend confirming empirically by toggling the OS theme with the app running, since this agent has not run the app) | App init / `PdfceApp::new` |
| `data-theme` manual override | No DOM attribute exists | An in-app theme selector (View menu: System/Light/Dark) that calls into the same theme-preference mechanism above, letting the operator override the OS signal | New, optional (P2) — not required for the sidebar work |
| `@font-face` data URIs (used by the handoff to mean "no silent fallback") | No embeddable web font format; the honest native equivalent is embedding real font bytes so appearance doesn't depend on what's installed on the machine | `egui::FontData::from_static(include_bytes!(...))` + `egui::FontDefinitions` registration | Not done anywhere in `pdfce-gui` today (confirmed by grep — §7). Flagged as a real, scoped, license-gated (rule 13) P2 decision, not implemented here. |
| Gradient heroes on white | No hero/marketing surface exists in a native desktop tool | N/A — explicitly recommend keeping all chrome fills flat (no gradient) regardless of theme | — |
| Accent bars on rounded cards | No "card" component exists in egui by default | Panel section headers carry hierarchy by TEXT WEIGHT + a plain `ui.separator()`, never a colored bar (§9) | Panel headers, dock panes |
| Hairline rules with dense columns | egui has no CSS Grid; the closest primitive is `egui::Grid` | `egui::Grid::new(id).num_columns(2)` for every label/control property row, relying on Grid's own consistent SPACING rather than a rule drawn under each row | Property panels, §9 |
| Type scale (headings/body/captions) | No CSS `font-size` cascade | egui's `Style::text_styles: BTreeMap<TextStyle, FontId>`, keyed by the 5 built-in `TextStyle` variants (`Heading`/`Body`/`Button`/`Small`/`Monospace`) plus `RichText::strong()`/`.weak()` for emphasis WITHOUT a new size (§6) | App init, `ctx.style_mut(...)` |

---

## §3. Named palette — chrome (theme-aware)

**Neutrals: inherited, not replaced, for v1.** The handoff's "hue
bias" ask (Part 1 principle 5, Part 2 §4) is a genuine aesthetic
preference but is not backed by any standing rule the way, say, R84
is — replacing `panel_fill`/`window_fill`/`extreme_bg_color` across
the whole app is a large-blast-radius change with no correctness
requirement behind it. Recommend deferring it and spending the
handoff's own principle 7 ("spend boldness in one place, keep
everything else quiet") literally: touch only the fields that carry
the ONE deliberate accent, leave every neutral fill as egui's own
`Visuals::light()`/`Visuals::dark()` default. Revisit only if, once
`ACCENT_PRIMARY` + the canvas vocabulary are in place, the app still
reads as generic — that is a real possibility worth checking against
the running build, not something to decide from source alone.

| Role | Meaning | Light value | Dark value | Mechanism |
|---|---|---|---|---|
| `ACCENT_PRIMARY` | The one deliberate brand accent (principle 7) — active-tool toolbar highlight, primary button fill, hyperlink color, selection tint | `rgb(98, 71, 156)` | `rgb(168, 142, 224)` | Override `Visuals::hyperlink_color`, `Visuals::selection.bg_fill`/`.stroke.color` |
| `GOOD` (success) | Validation passed / operation completed cleanly — **new role, nothing in the codebase uses it today** | `rgb(46, 138, 86)` | `rgb(86, 178, 126)` | New named function/const pair, theme-selected at call site |
| `WARNING` | Needs attention, not yet resolved | *(no new value)* | *(no new value)* | **Alias, not a new color** — `ui.visuals().warn_fg_color`. Already used correctly in this codebase (e.g. `main.rs:13829`'s `warn_color`). |
| `CRITICAL` (error/refusal) | A refusal, a blocking failure | *(no new value)* | *(no new value)* | **Alias, not a new color** — `ui.visuals().error_fg_color`. Already the established pattern for refusal lines (`main.rs:7625`, `7637`, `8204`, `8851` all do exactly this). |

Both `ACCENT_PRIMARY` values share the same ~260° hue (verified by
computing HSL from the RGB triples above, not eyeballed) — the "same
theme, calibrated per background" translation of "not naive inversion"
(handoff principle 4): the dark-mode value is lighter/more saturated
so it doesn't recede against a near-black panel; the light-mode value
is dark enough to read against near-white. `GOOD`'s pair does the same
at ~146° hue. Neither collides with any canvas-overlay hue in §4
(teal ≈185°, the amber family ≈15-30°, node-mark blue ≈213°) — mostly
moot since chrome and canvas are different visual CONTEXTS (§1), but
worth confirming there's no coincidental confusion either.

**What was already right before this document existed:** 44 existing
call sites in `main.rs` already read `ui.visuals()` for chrome colors,
`error_fg_color` is already the refusal-line convention, and
`RichText::new(...).weak()` is already the established caption/
secondary-text pattern (`main.rs:4010`, `4478`, `7710`). This document
formalizes an already-largely-correct practice; it does not correct a
widespread chrome-color defect. The defect, where one exists, is in
domain 2 (§4).

---

## §4. Named palette — canvas overlay (theme-invariant by design, §1)

**The claim verified quantitatively.** The handoff's audit said "no
formalized palette... each tool/panel potentially invents its own
colors." Measured today: **25 `Color32::from_rgb(...)` literals**, 24
of them in `main.rs`. Reading every one in context (not just counting)
shows they reduce to **8 distinct semantic roles**, most declared as a
LOCAL `let` binding and re-declared identically at 2-7 different call
sites instead of being one named constant — real duplication, cheap to
collapse, and the collapse is almost entirely safe because the
duplicate values are numerically IDENTICAL, not merely similar.

| Token (proposed name) | Value | Meaning | Current call sites (values confirmed identical unless noted) |
|---|---|---|---|
| `OVERLAY_PREVIEW` | `rgb(210, 90, 40)` | Live preview / uncommitted draft — will vanish or become real on commit | `DIMENSION_DRAG_COLOR` (`main.rs:329`), `main.rs:9693`, `9707`, `9719` (`preview_orange`), `11202` (`preview_orange`), `13115`, `13826` (`preview_color`) — **7 sites, all the same value.** This is the single biggest, safest consolidation: one constant, reused everywhere a live gesture preview is drawn, across TextEdit/AddText/Measure/VectorEdit/Reflow. |
| `OVERLAY_PREVIEW_FILL` | `rgba(210, 90, 40, 70)` | The same preview hue at reduced alpha, for a translucent drag-handle affordance | `main.rs:9872` (reflow width-handle) |
| `OVERLAY_WORKING` | `rgb(200, 120, 40)` | Read-only structural annotation drawn over content that is a transient working state (a block boundary, a reflow target) | `main.rs:9640` (block-overlay dashed outline), `9718` (`amber`, reflow target highlight) — **2 sites, identical.** |
| `OVERLAY_SUBPATH_OUTLINE` | `rgb(210, 140, 40)` | Outline of a selected subpath inside an entered object | `SUBPATH_OUTLINE_COLOR` (`main.rs:278`) — **see the drift finding below; do not silently merge this into `OVERLAY_WORKING`.** |
| `OVERLAY_GUIDE` | `rgb(160, 90, 40)` | A secondary, lower-emphasis auxiliary guide (reflow's right-edge alignment guide) — deliberately dimmer than `OVERLAY_PREVIEW` | `main.rs:9837`, `9849` |
| `OVERLAY_NODE_STROKE` | `rgb(30, 110, 220)` | Outline of an editable node/anchor mark | `NODE_MARK_COLOR` (`main.rs:305`) |
| `OVERLAY_NODE_FILL` | `rgb(250, 250, 252)` | Fill of an unselected node mark | `NODE_MARK_FILL` (`main.rs:315`) |
| `OVERLAY_TEXT_SELECTION_FILL` | `rgba(90, 140, 220, 70)` | Text-selection highlight (same blue family as node marks — "blue = editable/selectable," consistently applied, a GOOD existing pattern) | `main.rs:9599` |
| `OVERLAY_CE_DIMENSION_SELECTED` | `rgb(40, 150, 160)` | A selected ce dimension's outline — teal, distinct from the page-object accent and the "inside an object" amber | `DIMENSION_SELECT_COLOR` (`main.rs:323`) |
| `OVERLAY_MASK_FILL` | `rgba(250, 250, 250, 220)` | Translucent near-white mask over content currently being edited (the original is hidden beneath, not deleted) | `main.rs:9683`, `9759`, `11203` — **3 sites, identical.** |
| `OVERLAY_PREVIEW_INK` | `rgb(20, 20, 20)` | Near-black draft text painted on top of `OVERLAY_MASK_FILL` | `main.rs:9691`, `9809`, `11204` — **3 sites, identical.** |

**One real drift finding — flagged, not silently resolved.**
`SUBPATH_OUTLINE_COLOR`'s own doc comment (`main.rs:275-277`) claims:
*"Amber, matching the... hue the measure and add-text previews already
use, because an entered object IS a transient working state."* But the
measure/add-text preview color is `OVERLAY_PREVIEW` at `(210, 90, 40)`,
and `SUBPATH_OUTLINE_COLOR` is `(210, 140, 40)` — the comment asserts a
shared vocabulary that the actual bytes don't quite deliver (green
channel 90 vs 140, a real and probably visible difference side by
side, even though the two rarely appear together on screen). This is
exactly the kind of near-miss a token module is supposed to force
someone to resolve, once, by name, instead of leaving it to silently
persist as an unverified claim in a comment. **Recommend:** the
engineer either (a) changes `SUBPATH_OUTLINE_COLOR`'s value to
literally equal `OVERLAY_PREVIEW` and updates the comment to state
that plainly, or (b) keeps it distinct, renames it to
`OVERLAY_SUBPATH_OUTLINE`, and corrects the comment to stop claiming
kinship it doesn't have. This is not mine to decide from static
reading alone — Pass 36.3 tuned this value against a real screenshot
(`main.rs:293-298`'s own account of the defect it fixed), and any
change to it should get the same photographic verification, not just
a textual consistency argument.

**One open simplification question, not a finding.** Is `OVERLAY_GUIDE`
(`160, 90, 40`) genuinely a necessary THIRD hue in the amber/orange
family, or would `OVERLAY_PREVIEW` at reduced alpha read the same way?
My judgment, stated as a recommendation rather than a rule: **keep it
distinct.** `NODE_MARK_COLOR`'s own doc comment (`main.rs:300-304`)
already established the project's reasoning that alpha/lightness-only
differentiation is harder to distinguish at small stroke widths than a
genuine hue+lightness shift — the same argument applies here. But this
is cheap to verify against the running app once the token module
exists, and I'd rather the engineer eyeball it than take my word for
it.

---

## §5. The exception: `markup_color` is not a token

`main.rs:977`'s default annotation pen color
(`egui::Color32::from_rgb(0xE0, 0x30, 0x30)`, a visible red) is
**deliberately excluded** from §4's table. It is not a fixed semantic
cue pdfcer assigns meaning to — it is a **sensible starting value for a
property the operator owns and can repaint** (Pass 6.1's own comment:
"a visible red pen and a 2-point stroke — sensible defaults an
operator can change before authoring"). A token system exists to make
semantic state legible; it must not swallow a user-adjustable
preference into that vocabulary, or a future engineer will "fix" the
operator's own chosen color back to a fixed constant. No change is
recommended here beyond documenting the category — the value itself
doesn't collide with any `OVERLAY_*` hue, so it's a safe default as-is.

This is the general rule for anything added later that resembles a
color literal: **ask whether pdfcer assigned the meaning, or the
operator did.** Only the former belongs in the token module.

---

## §6. Type scale

Zero custom `TextStyle` configuration exists in `pdfce-gui` today
(confirmed: no `text_styles` assignment anywhere in the crate) — every
size in the app is whatever egui's own defaults happen to be. This is
the one part of the handoff's audit (Part 2 §4 item 1, "no... type
scale") that is unambiguously and fully correct, with no existing
partial credit to note.

| Role | egui mechanism | Point size | Where used |
|---|---|---|---|
| Heading | `TextStyle::Heading` | 17.0, proportional | Dock panel section titles, per-tool title at the top of the Tool Options pane body (§A.3 of the ui-spec) |
| Body | `TextStyle::Body` | 13.5, proportional | Property values, prose, tooltip bodies |
| Property label | Body role + `RichText::strong()` | (no new size) | The label half of a label/control row (§9) — emphasis by WEIGHT, not a new size, the same mechanism `tab_title_for_tile` already uses for R84's active-tab cue |
| Caption | `TextStyle::Small` | 11.0, proportional | Disclosure/refusal strip secondary lines, the Tool Options empty-state hint, status-bar narrator text — via `RichText::weak()`, already the established pattern (`main.rs:4010`, `4478`, `7710`) |
| Button | `TextStyle::Button` | 13.5, proportional | Toolbar/dialog buttons — same size as Body; buttons are not a separate visual register |
| Data / numeric | `TextStyle::Monospace` | 13.0, monospace | Dimension values, coordinates, byte offsets, hex, file sizes |

Deliberately NOT introducing new `TextStyle::Name(...)` variants beyond
these 5 built-ins — every role above is reachable by pairing a built-in
`TextStyle` with `.strong()`/`.weak()`, which is less new mechanism
than it looks (rule-3-adjacent: don't add a widget-identity concept the
existing modifiers already cover).

---

## §7. Typefaces — the honest gap

Confirmed by grep (`FontDefinitions`/`FontData`/`set_fonts` across
`crates/pdfce-gui/src`): **pdfce-gui installs zero custom fonts.**
`ui_text.rs:5303-5304`'s own test comment already says so: *"pdfce-gui
installs no custom fonts (`grep -r FontDefinitions crates/pdfce-gui/src`
finds only this test)."* Every glyph in the app today comes from
egui's bundled default face, which (per this project's own egui RAG
finding, `D:\dev\rag\egui\epaint_0.35_text_stack_i18n_limits.md`) has
no CJK coverage and no system-font discovery.

**Recommendation for v1: name the two roles honestly rather than
pretend a typeface decision was made.** One proportional role (chrome/
body/heading, §6) and one monospace role (data/numeric, §6) — both
currently filled by egui's own bundled defaults. That satisfies the
handoff's "2+ typefaces, 2+ roles" ask truthfully: two ROLES exist and
are named; whether they're filled by a deliberately-chosen, licensed
display face or by whatever egui ships is a separate, real decision.

**A genuinely new gap this surfaces, not previously flagged:**
bundling a chosen font (e.g. Inter, IBM Plex Sans) means redistributing
a third-party licensed asset. `cargo-about` (rule 13's attribution
generator) scans **Cargo dependencies** — it will not see a font file
dropped in as a bundled asset via `include_bytes!`. If pdfcer ever ships
a custom font, `THIRD_PARTY_LICENSES.md` needs a manually-added line
for it, or the attribution story has a silent hole. Flagging this now
so it isn't discovered the hard way later. Not decided here — P2,
needs the operator's go-ahead the same way any dependency addition
does (rule 13).

---

## §8. Both themes, equally — the concrete mechanism, and the deliberate exception

**Mechanism (§2's translation, restated concretely):** egui/eframe has
its own theme-preference handling distinct from a CSS media query —
verify the exact 0.35 API surface against docs.rs before wiring it,
and confirm empirically (toggle the OS theme with the app running)
whether pdfcer already benefits from automatic OS-theme following today
with zero app code, since nothing in `main.rs` currently calls any
theme-setting API at all. The gap this document cares about is
narrower than "add theme detection": it is that **`Color32` literals
do not participate in whatever theme-switching mechanism exists**,
automatic or not — they're the same bytes regardless. That is the real
target of the fix, and §3/§4 already separate which literals need to
move (chrome) from which correctly don't (canvas overlay).

**Not naive inversion (principle 4), made concrete:** `ACCENT_PRIMARY`
is not `rgb(98,71,156)` in light mode and its RGB-inverted complement
in dark mode — it's a second value, independently chosen so it reads
correctly against a near-black panel, sharing the SAME hue (~260°, §3)
so it still reads as "the same accent," calibrated for lightness/
contrast the way principle 4 asks. `GOOD` (§3) gets the same treatment.
`WARNING`/`CRITICAL` need no separate work at all — they're aliases to
`ui.visuals().warn_fg_color`/`error_fg_color`, and egui's own
`Visuals::dark()` already ships a legible dark-mode value for both;
this is the version of "set once, inherit everywhere" that costs
nothing because the underlying mechanism already exists.

**The one deliberate exception, stated as a boundary rather than an
oversight:** every `OVERLAY_*` token in §4 is the **same value in both
themes**. This is correct given §1's reasoning, not a violation of
"both themes equally" — a rendered PDF page is near-white regardless
of the operator's chrome theme, so an overlay color calibrated against
"draws visibly on white" would be WRONG to reroute through the app's
dark-mode chrome palette. If pdfcer ever renders a genuinely
dark-background page, the existing two-tone construction (stroke +
fill, e.g. `OVERLAY_NODE_STROKE` + `OVERLAY_NODE_FILL`) already
degrades gracefully — `NODE_MARK_FILL`'s own doc comment names this
explicitly. Nothing new is needed for that edge case.

---

## §9. Component patterns

For each surface, the tokens it uses and the R84 redundant-cue
statement (state is never carried by color alone — CLAUDE.md rule and
`ROADMAP.md` R84 both bind this).

- **Dock tab.** Already compliant, cited rather than redesigned:
  `DockBehavior::tab_title_for_tile` (`dock.rs:399-416`) bolds the
  active tab's label text — a weight cue, not a fill-color-only cue.
  No change; new left-dock tabs (Pages | Tool Options, the ui-spec's
  §A) reuse this verbatim.
- **Panel section header** (e.g. Tool Options' per-tool title, a dock
  pane's top line). Heading role (§6) + one plain `ui.separator()`
  below it. **No colored accent bar** — per §0's uniqueness review,
  hierarchy is carried by weight and the separator's presence, not by
  introducing a decorative bar that the handoff's own avoid-list names
  by name.
- **Property row** (label + control). `egui::Grid::new(id)
  .num_columns(2)`, label column left (Body role + `.strong()`, §6),
  control column filling the remainder. No hairline drawn under
  individual rows — Grid's own spacing carries the rhythm. Exact
  column width is a spacing judgment best made against the running
  dock (per this agent's own standing precedent for not inventing
  exact pixel numbers from source alone) — not specified here.
- **Disclosure/refusal strip.** Caption role (`RichText::weak()`, §6)
  for informational disclosures; `WARNING`/`CRITICAL` alias colors
  (§3) PAIRED with a glyph (e.g. a small triangle/cross prefix) for an
  actionable refusal — never color alone, matching the existing
  `error_fg_color` convention this codebase already uses correctly.
- **Icon toggle.** Reuse the already-specified `icon_button` accessible-
  name wrapper and `toggle_label`'s bold-on-selected cue (this agent's
  prior "Icon set + toolbar spec" memory); for an ICON-ONLY toggle with
  no visible label to bold, that memory already names the needed
  addition — an outline-ring selected-cue — restated here as binding
  for any new icon toggle the Tool Options pane introduces.
- **Status note** (the fixed-height status/narrator bar,
  `STATUS_PANEL_HEIGHT_PTS`, `main.rs:266`). Caption role. Per the
  ui-spec's own §A.4 reasoning, tool-specific disclosures do NOT belong
  here — this surface is a shared, capped-height, project-wide
  convention for save/delete/copy narrator lines; nothing new is asked
  of it by this document.

---

## §11. Density — the dock panels are a dense surface

Added by the shell redesign's slice 1 (`docs/ui_specs/shell-redesign.md`
§2.2/§6), which asked for a named convention rather than a per-panel
judgement, *"cheaper to fix once, in the token module, than to re-derive
per new surface."*

### §11.1 What was actually costing the space — measured, not guessed

egui's shipped defaults are `item_spacing: vec2(8.0, 3.0)` and
`interact_size: vec2(40.0, 18.0)` (`egui-0.35.0/src/style.rs:1449,1454`).
So a row's pitch is **`interact_size.y` + `item_spacing.y`**, and the
measured pitches before this pass agree exactly:

| Surface | Measured pitch | Composition |
|---|---|---|
| Radio option rows (Forms) | 21 px | 18 + 3 (defaults) |
| `/Info` property grid rows | 25 px | 18 + 6 (its own `.spacing([12.0, 6.0])`) |

That matters because it names the lever. The airiness was **not** large
text and **not** the general `item_spacing` — it was the vertical gap
*added on top of* a widget height that is already near the floor.

### §11.2 The convention

**`DENSE_ROW_SPACING_Y = 2.0`**, applied once to the whole dock-pane
scope in `PdfceApp::panel_body`, so every dock panel inherits it and no
panel sets its own. The property grid's own row spacing is brought to the
same number, so a labelled property row and a plain control row sit on the
same rhythm instead of two.

egui's 3.0 default is a *general-purpose* value shared with the toolbar,
the ribbon, the status bar and the canvas overlays. It was never chosen
for a dense property surface, and the redesign spec says so in those
words. This narrows it **for the dock panels only** — deliberately not
globally, because the same number is doing a different job in a toolbar
row where controls need to be separable at a glance.

### §11.3 What density may NOT be bought with

Three things are ruled out here so a future pass does not quietly trade
them for pixels:

1. **`interact_size.y` stays at 18.0.** It is the minimum height of an
   interactive widget — i.e. the click target. Shrinking it is the single
   biggest available density win and it is refused: it trades pointer
   accuracy, which costs most for the operators least able to spare it.
   Density is taken from the GAP between controls, never from the
   controls.
2. **No text gets smaller.** §6's roles stand — Heading 17, Body 13.5,
   Caption 11. The redesign spec makes the same point independently.
3. **No explanatory line is deleted to save a row.** The canvas raster is
   screen-reader-illegible, and this project has repeatedly narrowed that
   gap by routing facts through real text widgets — disclosures,
   refusals, readouts. Those lines ARE the accessibility surface. A
   separator that separates nothing is fair game; a sentence never is.

### §11.4 One control per row is not a rule to defend

Where a numeric field and its unit/mode selector are stacked, pair them
on one row (the scale-entry and decimal-places rows already do this).
Where a section's controls are genuinely independent, leave them stacked
— pairing unrelated controls to save a row makes the operator read a row
as a unit that is not one.

`ui.separator()` earns its row only between sections that are
conceptually distinct. Two separators with one label between them, or a
separator immediately under a heading that already has one, are the
common cases worth removing.

---

## §10. Open items — not decided here

1. **A conflict between the handoff and a standing rule, surfaced
   explicitly, not routed around.** The handoff's Part 2 §2 and Part 4
   Q4 recommend auditing "Acrobat Pro's current ribbon layout" / "GUI
   structure (ribbon, panels, tool organization)" via
   `pdfcer-acrobat-librarian`, "so you can intentionally differ."
   `CLAUDE.md` rule 12 states, unambiguously, that the Acrobat
   feature-parity RAG "must never describe or inform copying Acrobat's
   GUI structure (menu paths, panels, dialogs); pdfcer's UI is designed
   independently by `pdfcer-ui-specialist`." These are the same request
   read two different ways, and rule 12 is the one binding today.
   **Recommendation: do not dispatch `pdfcer-acrobat-librarian` for a
   GUI-structure audit.** If Ken wants a literal comparative audit of
   Acrobat's ribbon as reference material, that requires HIM to grant
   a new, explicit exception to rule 12 — it is not something the
   engineer or this agent can quietly authorize by calling it something
   else. Until that happens, "intentionally differ from Acrobat's
   ribbon" is satisfied by this agent's own domain competence (the
   role this document's parent agent file exists for), not by a
   capability-only RAG whose charter exists specifically to keep GUI
   mechanics out of it.
2. **Ribbon specificity (handoff Part 4 Q2) — Ken's call, not made
   here.** How close pdfcer's eventual ribbon/toolbar restructuring
   should read to Acrobat's own layout vs. developing pdfcer's own
   distinctive structure is explicitly flagged, not decided, per this
   agent's charter.
3. **A candidate standing rule for the librarian.** "A canvas-overlay
   color's reference surface is the rendered page, not app chrome; an
   app-chrome color's reference surface is the theme" (§1) is exactly
   the kind of pattern this project's standing-rule ledger exists to
   capture, once it's been battle-tested against the actual
   consolidation in §4. Recommend the engineer file it with
   `pdfcer-librarian` once the token module ships, librarian-numbered
   in the usual way — not asserting a number here.
4. **The `SUBPATH_OUTLINE_COLOR` drift (§4)** needs a decision (merge
   into `OVERLAY_PREVIEW`, or keep distinct and correct its comment) —
   flagged, not resolved, since it touches a recently, deliberately
   screenshot-tuned value (Pass 36.3) that deserves the same
   verification discipline it shipped with.
5. **Font-asset licensing (§7)** — bundling a chosen display/body face
   is real, scoped, P2 work requiring rule-13 classification and an
   operator go-ahead; not decided here.
