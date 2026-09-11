# pdfce — Session log

Append-only. One section per session date. Never overwrite or reorder
a prior entry; corrections get a dated amendment footer appended to
the affected entry. Maintained by `pdfce-librarian`.

## 2026-09-11 (504th filing) — the scan said "exit 1" and was read as "stops"

**Shipped:** `8129dc1` — `scan-offpage`'s help, and the published
`v0.52.0` notes, now say that every file and every page is scanned
always, that an unreadable file is reported and the walk continues, and
that the exit code is a verdict at the END of the run. The one-shot form
is spelled out beside it.

**Why:** the operator read *"Exits 1 when something is off-canvas"* as an
early stop and asked how to make it scan to the end. The code was right,
a test would have passed, and the defect was entirely in the sentence.

★ **Nothing in this project's gates reads prose for what it will be
UNDERSTOOD to mean.** Worth remembering the next time a release note
describes an exit code.

## 2026-09-11 (503rd filing) — `v0.52.0` released, with the portable build

**Released:** tag at `cb7727e`, CI **green** at the tagged commit. Zip
`pdfcer-v0.52.0-windows-x64.zip`, **19,139,784 bytes**, SHA-256
`6ebe3671afb5b7700189292cc3c6c2e8a6d770a0f318a878e8eb2df0d1dd95eb`,
published on the release page with its checksum file. Portable folder
`builds/pdfcer-20260911-0005-cb7727e` on D:, 8 files, 35,384,512 bytes.
OneDrive slot **`pdfcer1`**; `pdfcer2` keeps `0.51.0`.
`verify-release.py` **nine of nine**.

**Contents:** `Pass 294.0` and `294.1` — `scan-offpage` and
`redact-offpage`, both taking files, folders and `--recursive`.

★ A local `v0.52.0` tag from an interrupted first attempt pointed at the
BUMP commit, one commit behind the batch feature. Caught by reading the
tag before building, confirmed unpublished with `git ls-remote --tags`,
then moved. **An unpushed tag is a local note; a pushed one is a
published claim** — the check that separates them costs one command.

## 2026-09-10 (502nd filing) — redact-offpage goes batch

**Shipped:** `Pass 294.1` (`d41be61`) — `redact-offpage` takes files,
folders and `--recursive`, matching the scan. `-o FILE` for one input;
`--out-dir DIR` for a batch, **mirroring the input tree** rather than
flattening, because two product folders can hold drawings with the same
file name — a collision this feature's own test copy hit on the first
try. An existing output is skipped and counted unless `--force`, so an
interrupted batch resumes.

**Owed:** still no `docs/core-api/` entry for the `offpage` module.

## 2026-09-10 (501st filing) — `v0.52.0` bumped for the off-canvas Pass

**Shipped:** the version bump to `0.52.0` (`4d5b226`) — `Cargo.toml` and `fuzz/`'s own
lockfile, which is a separate cargo workspace — `v0.51.0` learned that
the hard way, from a release binary whose banner read `-dirty`).

Carries `Pass 294.0`: `scan-offpage` and `redact-offpage`. Release notes
lead with the measurement on the operator's own drawings — 176 of 341
files draw outside the sheet.

## 2026-09-10 (500th filing) — off-canvas content: found, and cut away

**Shipped:** `Pass 294.0` (`04d0099`) — `pdfcer scan-offpage` (files, folders,
`--recursive`) and `pdfcer redact-offpage`. Asked for as ASAP work.

Content drawn outside the page box is still in the file: it prints on a
larger sheet, survives a page-box change, and its text is extractable.
The scan is a read-only census; the removal authors `/Redact` marks over
the four bands around the page box and applies them, so a PARTIAL object
is cut at the page edge by the same code that cuts it at the edge of an
operator's redaction box. **"Outside the page" is a region like any
other** — that observation is the whole Pass; no new geometry surgery
was written.

**Measured on `R:/Products`, 341 files:** 176 affected, 554 pages,
471,840 fully-off objects, 1,152 partial, 0 unreadable. One sheet had
15,927 fully-off objects — a second drawing at x = -600. The 176 are
copied to the operator's test folder.

**Verified end to end** on one: 234 paths dropped, 247 cut, 4,642
off-page glyphs removed; the output re-scans clean and page 1 renders
pixel-identical to the input.

**Owed:** no `docs/core-api/` entry yet (the new module is `pdfcer-core`
public surface); the full workspace suite was not re-run, deliberately,
at the operator's request for speed.

## 2026-09-10 (499th filing) — `v0.51.0` released

**Released:** tag at `1ccd31e`, CI **green** at the tagged commit. Zip
`pdfcer-v0.51.0-windows-x64.zip`, **19,117,523 bytes**, SHA-256
`b8e5741075e17c6c02b159249fb4de77a84d50df2d51f5f6251386a21274ed48`.
Portable folder `D:/builds/pdfcer-20260910-1611-1ccd31e`, 8 files,
35,300,188 bytes. OneDrive slot **`pdfcer2`**; `pdfcer1` keeps `0.50.0`
as the previous version. `verify-release.py` **nine of nine**.
`run-gates.sh` PASS, 30 commands.

**Contents:** six Passes — `290.0`/`290.1`, `291.0`, `292.0`, `293.0`,
plus `289.0`. Headline for an operator: a PDF Acrobat wrote could not
be opened, and now it can.

★ **Two builds were discarded before one was shippable**, both caught by
the binary's own version banner reading `-dirty`: the first was built
before the version bump was committed, the second while `fuzz/Cargo.lock`
— its own cargo workspace, its own lockfile — still carried `0.50.0`.
A release binary that says *"this is not the commit it names"* is not a
release binary. The banner did the work no checklist item would have.

## 2026-09-10 (498th filing) — the queue was a fifth finished work, and the rules now say who enforces them

**Shipped:** `93b7bbf` — 19 of `ROADMAP.md`'s 99 *Next up* items described a
Pass that had already shipped; moved verbatim to
`docs/history/roadmap-nextup-already-shipped.md`. `ROADMAP.md` 30,324 →
26,542; queue 99 → 80 items; register-size debt 136 → 117. Found by script
(a Next-up `Pass N.M` matched against every Shipped heading carrying a hash),
so it will find the next batch too.

Plus the rule-enforcement column (`8e426e9`): `tools/annotate-rule-gates.py` marks every
standing rule whose own text names a script in `tools/`. **44 of 187 do.** The
header records the other 143 as a backlog and explicitly declines to call them
judgment calls, which is a reading nobody has done.

**Owed:** `Backlog` (9,566 lines) is the remaining bulk and needs editorial
judgment. The 143 unenforced rules want a per-rule verdict: gate it, or bin
it.

## 2026-09-10 (497th filing) — the registers were the bottleneck

**Shipped:** `a12dca6`, `233a9ef` — the register trim. `ROADMAP.md`
168,036 → 30,277 lines, `SESSION_LOG.md` 99,597 → 1,446,
`ARCHITECTURE.md` 34,341 → 10,317; history moved verbatim to
`docs/history/`. New gate `tools/check-register-entry-size.py` caps
new entries (150/80/200 lines, 1,200 chars) with 136 pre-existing
entries carried as DEBT. Three filing gates taught to read the
archive. Full reasoning: the two commit messages.

**Why:** the operator said the project *"has slowed to a crawl"*, and
the measurement agreed — 372,011 lines of docs against 455,626 of
code, 2,722 register lines written that day against 5,844 code lines,
the same paragraph landing four times.

**Owed:** `ROADMAP.md`'s *Next up* (12,900 lines) and *Backlog* (9,566)
are the remaining bulk and need editorial judgment, not a script. The
136-entry baseline should shrink. The 231 standing rules still have no
"what enforces this" column — the operator's *"script it or bin it"*
applies there next.

**Note the shape of this entry:** it is 20 lines. That is the point.

## 2026-09-10 (496th filing)

**Shipped:**
- Pass 293.0 (`56c5e55`) — a custom stamp can now be PLACED: one page's
  artwork onto another, as vector. `Pass 288.0` gave pdfcer stamp
  COLLECTIONS (container, names, category) but nothing could draw one
  page onto another, so pdfcer could read the operator's own signature
  stamps and could not stamp anything with them. New
  `EditSession::place_page_artwork(&source_view, source_page, page_index,
  rect) -> Result<PlacedArtwork, EditError>` imports the source page's
  content and resources as a **form XObject** behind a `/Stamp`
  annotation's `/AP /N`. New CLI verb `place-stamp <in> --from
  <collection.pdf> (--stamp NAME | --stamp-page N) --page N (--at X,Y |
  --rect x0,y0,x1,y1) -o <out>`. Closes the last of the five
  `pdfcer-gui` requests filed 2026-09-10.
  **Why a form XObject, not a raster**: what Acrobat writes, and
  architecturally forced by §12.5.5 + §8.10 — keeps the artwork vector,
  keeps it selectable/movable/deletable, never touches the page's own
  content stream (R47). A raster alternative (render the stamp page,
  place via `add_image`) was considered and rejected: not
  Acrobat-compatible, inflates a 5.6 MB CAD drawing per stamp, does not
  survive zooming, picks a resolution nobody asked for — the consuming
  shell had already declined the same alternative for the same reasons
  (reported through the existing decision-058 channel).
  **Disclosures on `PlacedArtwork`** (project rule 4, since the CLI
  invocation is the commit): `scale_x`/`scale_y`/`distorted` (§12.5.5
  maps `/BBox` onto `/Rect` with independent factors — the stretch is
  normative behaviour, not a pdfcer shortcut); `objects_imported`;
  `resources_renamed` (always `0` by construction, per §8.10, reported
  anyway); `source_annotations_ignored`; `source_widgets_ignored` (the
  dynamic-stamp caveat — a dynamic stamp places its design-time text,
  correct as a picture, wrong as a promise); `transparency_group_carried`.
  No `/Name` is written (Table 181 leaves it optional); whether Acrobat
  records one for a custom stamp is an open gap, flagged by name (`R250`).
  New `EditError::SourcePageOutOfRange` (135 variants now).
  Verified on the operator's own
  `%APPDATA%\Adobe\Acrobat\DC\Stamps\YTV_yyfVN1TzJ0_6oei-GB.pdf` — both
  signatures place and render, the same file `Pass 290.0` had to fix
  page-tree-resource handling for just to open. Nine new tests in
  `crates/pdfcer-core/tests/place_artwork.rs`, including an R47
  byte-comparison with an explicit fixture-can-fail assertion (`R225`);
  sabotaged twice, each turning exactly the expected test red.
  `cargo test --workspace`: **5,309 pass over 9 new tests this Pass
  (5,300 prior + 9)**; fmt/clippy `--all-features` clean;
  `check-core-api-verbs` PASS at **224 verbs (223 prior + 1)**.
  `FEATURES.md` gains a new row in the same filing — `core [x] / cli [x]
  / gui [ ]` — the requesting shell asked for the core API only.

**Decisions made this session:**
- None. The form-XObject-behind-an-annotation shape is §12.5.5/§8.10,
  already established elsewhere in `ARCHITECTURE.md` (annotation
  appearance placement); the rejected-raster-alternative rationale is
  reported through the existing decision-058 channel (the external
  `pdfce-gui` consumer had already declined the same alternative for the
  same reasons) rather than argued fresh here. No new crate boundary,
  library choice or invariant.

**Findings + decisions:**
- **A `pdfcer-acrobat-librarian` dispatch this session corrected a
  standing project premise.** Acrobat **Reader** — not only Pro — can
  place an existing custom stamp; it can only NOT author a new stamp
  category. The project memory that reads "Acrobat Reader is available;
  Pro is not" had been carried as implying no stamp-placement artifact
  is obtainable in this environment, and that inference does not follow
  from the fact. One Reader-placed-and-saved PDF would settle the
  `/Name` round-trip gap flagged above, open since `Pass 288.0`. New
  corpus file:
  `Acrobat_Features/markup__custom_stamp_placement_and_appearance_authoring.md`;
  `markup__stamp_text_size_and_resize_behavior.md` upgraded (d)→(a) in
  place — §12.5.5's anisotropic stretch turned an inference into a
  documented fact. This project's own memory entry on the Reader/Pro
  split is corrected accordingly (see this filing's memory update).
- **An empirical PDF-domain finding**, written to `C:\personal_rag\pdf\`:
  the operator's own Acrobat-authored stamp collection contains a page
  whose only content is a `/DCTDecode` RGB image with a **black
  background and no `/SMask`** — a faithful placement puts a black box
  on the page. pdfcer reproduces it exactly (pixel-identical against a
  direct render of the source page), so a future "the stamp looks wrong"
  report would be about the source file, not pdfcer. New lesson:
  `lesson_20260910_stamp_collection_page_with_black_background_dct_image_and_no_smask_places_as_a_black_box.md`,
  indexed in both `pdf\index.md` and the master `personal_rag\index.md`.

**Still in flight:**
- All five `pdfcer-gui` requests filed 2026-09-10 are now closed
  (`place_page_artwork` was the last).
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged —
  this Pass does not touch the owed ledger.
- The `/Name`-on-a-custom-stamp round-trip gap (`R250`) is still open,
  but is now potentially answerable via an Acrobat-Reader-placed
  artifact — see the finding above.

**For next session:**
- If an Acrobat-Reader-placed custom stamp PDF becomes available, check
  it for a `/Name` entry to close the `R250` gap.
- This filing had no shell; all commit-message detail beyond what
  `Read`/`Grep`/`Glob` could confirm against the live tree (byte-level
  test/verb counts, the two sabotage mechanisms, the exact new-test
  count) is relayed, not independently verified — see `ROADMAP.md`'s
  sourcing paragraph for `Pass 293.0` for the full list.

## 2026-09-10 (495th filing)

**Shipped:**
- Pass 291.0 (`0173a95`) — a shrunk or clipped stamp label now says so.
  `Pass 287.0` gave `StampFit` three values (`GrowToText`/`ShrinkToBox`/
  `ClipToBox`) but the consuming shell could offer only one of them,
  because the other two decide something the operator did not ask for
  with no channel back to report it. The disclosure channel that should
  have carried this, `AuthoredTextAnnot::applied_autosize`, is `None` for
  every stamp, always — it signals variable-text auto-size (`/DA 0 Tf`),
  and a stamp's fitted size is written as an explicit `/DA` size, so the
  field never fires for the case it was needed for. New
  `AuthoredTextAnnot::stamp_label_fit: Option<StampLabelFit>`, an enum
  (`AsRequested`/`BoxGrown`/`LabelShrunk`/`LabelClipped`,
  `#[non_exhaustive]`) rather than a bare size, since the same number
  means opposite things depending on whether it was requested or forced.
  New `EditSession::add_text_annotation_reporting` returns
  `TextAnnotOutcome`, added alongside the existing verb rather than
  widening its return type. CLI prints all three inference cases,
  nothing for `AsRequested`. Five new tests, `cargo test --workspace`:
  5,292 pass.
- Pass 292.0 (`c11c1aa`) — a placed stamp's label size can now be read and
  written. New read half: `annot::stamp_label_parameters_in` /
  `EditSession::stamp_label_parameters`, returning `{label, size,
  size_source}` with `size_source` one of `DeclaredInDa` /
  `RecoveredFromAppearance` / `DaUnreadable`. New write half:
  `TextAnnotStyle::font_size` (+ `stamp_fit`) on `set_text_annot_style`,
  refused by name on `/Text`. **A live data-loss defect was found and
  fixed on the way in**: the restyle route rebuilds a stamp's appearance
  from a spec read back out of the file, and `text_spec_from_dict`
  deliberately reports `label: None` for a `/Stamp` (correct in
  isolation — `/Contents` must not drive the face) — but the restyle's
  rebuild read that `None` as "no custom label" instead of calling the
  same recovery `resize_annotation` already used, so changing a stamp's
  COLOUR silently replaced its own custom text with the stamp name's
  default. Fixed by calling the shared recovery from both routes. CLI
  `set-text-annot-style --font-size POINTS [--stamp-fit grow|shrink|clip]`;
  `list-annotations` gains `stamp_label=`/`stamp_size=`/
  `stamp_size_from=`. Eight new tests, `cargo test --workspace`: 5,300
  pass; fmt/clippy clean; `check-core-api-verbs` PASS at 223 verbs.
- Both close two of the three `pdfcer-gui`-channel requests still open as
  of the 494th filing (the shrink/clip-fit disclosure gap and the
  placed-stamp label size read/write gap); a reply is on file
  (`reply_2026-09-10-stamp-label-size-both-halves-and-the-fit-disclosure-
  SHIPPED.md`, relayed — not independently confirmed present, outside
  this session's accessible directories).

**Decisions made this session:**
- None. Both Passes add API surface inside decision 147's existing
  `/DA`-storage reading for a stamp's label — no new crate boundary,
  library choice or invariant.

**Findings + decisions:**
- **`R245` gains a seventh dated instance, in two parts.** Part (a):
  `TextAnnotOutcome::applied_autosize` was wired for one member of a
  size-inference family (`/FreeText` auto-size) and silently `None` for
  a sibling added later (`/Stamp` fit) — the rule arriving
  *retroactively*, through a later Pass growing a new family member onto
  an existing disclosure surface without re-checking coverage, rather
  than being incomplete from day one. Part (b): the stamp-label recovery
  was called by one of two rebuild routes and not its sibling — the
  founding shape exactly, and the more serious of the two instances,
  since the gap silently destroyed operator data rather than merely
  under-reporting. Named `R245` by the engineer's own commit message.
  No amendment to the rule's text; ceiling unaffected, `R245`, next free
  `R246`.
- **A related design principle flagged at n=1, not minted**: the
  requester's own framing for `Pass 292.0` — "a read with no write, and a
  write with no read, are both unbuildable surfaces, so an inspect/modify
  pair for the same property ships together" — is a real stated
  discipline for this Pass but not yet an independently-observed
  cross-Pass pattern. Worth a standing rule at a second, independently-
  arrived-at instance.
- This filing had no shell; all commit-message detail beyond what
  `Read`/`Grep` could confirm against the live tree (see `ROADMAP.md`'s
  sourcing paragraph for the full list) is relayed, not independently
  verified.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged —
  neither Pass this filing touches the owed ledger.
- Only **one** of the five `pdfcer-gui` requests filed 2026-09-10 remains
  open and unscoped: `place_page_artwork` (no verb draws one page's
  artwork onto another page as a form XObject); the requester ranked it
  lowest, and its `as_annotation` shaping question is undecided.
- The two flagged `personal_rag/pdf` findings from the 494th filing
  (Acrobat's own spacer-page habit; the population-scale empirical claim)
  are still unwritten — carried forward again.

**For next session:**
- Scope `place_page_artwork` into a Pass ID.
- Write the two flagged `personal_rag/pdf` findings.

## 2026-09-10 (494th filing)

**Shipped:**
- Pass 290.0 (`556878e`) — a page with no `/Resources` (on itself or any
  ancestor, or resolving to a dangling reference) now opens as an empty
  resource dictionary instead of refusing the whole page tree. Before this,
  `page_tree::resolve_page`'s `?` sat on a walk returning ONE `Result` for
  every page, so one blank spacer page cost the whole document on
  `render-page`, `extract-pages`, `set-page-size` and `extract-text` alike.
  Measured on the operator's own Acrobat-written signature-stamp collection
  (`YTV_yyfVN1TzJ0_6oei-GB.pdf` — a blank spacer page 1, then two perfect
  signature pages) and on pdfcer's own `fixtures/synthetic/minimal.pdf`,
  which has the identical shape. Sourced from Table 30's own `/Resources`
  row ("shall be an empty dictionary" when the page needs none) rather than
  invented; `/MediaBox` deliberately does NOT get the same treatment (no
  clause names a default box), pinned by a test. Disclosed via `Page::
  resources_defaulted`, appended to `render-page`'s and `extract-text`'s
  metrics lines. **Decision 150 minted** — extends decision 145/`R248`'s
  fail-clean kernel to a case where the "other reading" comes from the
  standard rather than from the file; new `ARCHITECTURE.md` §10.6.
- Pass 290.1 (`bce4703`) — `stamp_file::read` used to build its page list
  via `page_tree::pages(doc).map(..).unwrap_or_default()`, so a page-tree
  failure produced an EMPTY list — indistinguishable from `page_index:
  None`'s existing meaning ("this stamp's name points at a page that does
  not exist"). On the operator's real stamp file `pdfcer stamp-list`
  printed `page=MISSING` beside both of his genuine signatures.
  `StampCollection::page_tree_error: Option<String>` now carries the real
  cause; the CLI prints `page=UNKNOWN`, distinct from `page=MISSING`, and
  names the cause on stderr.
- Both close inbound `pdfcer-gui`-channel requests filed 2026-09-10
  (`request_one_resourceless_page_makes_the_whole_document_unopenable_
  and_acrobat_writes_those.md`,
  `request_a_page_tree_failure_is_reported_as_every_stamp_pointing_at_
  nothing.md`); a reply is on file
  (`reply_2026-09-10-a-resourceless-page-no-longer-costs-the-document-
  SHIPPED.md`). Channel state relayed, not independently `Glob`-confirmed
  this filing (no shell in this invocation).

**Decisions made this session:**
- **Decision 150** — a required page-tree attribute that is absent (or
  dangles) defaults to the value the standard itself names for that key,
  when one exists (Table 30's `/Resources` row), rather than refusing the
  page tree; a key with no stated default (`/MediaBox`) is unaffected and
  stays fatal. `ARCHITECTURE.md` §12 + new §10.6, sibling to §10.5
  (decision 145). Explicitly does NOT restate `R248` — the file supplies
  no reading here at all; the standard does, once, for a named key — which
  is why this is its own decision rather than a dated `R248` instance.

**Findings + decisions:**
- **A test that measured exactly what it claimed, reached through a
  defect it was fixing, not through the condition it named — filed as
  `R225`'s 18th dated instance, a new sub-shape.** `fontinfo`'s
  `an_unwalkable_page_tree_is_reported_not_rendered_as_no_fonts` and the
  CLI's `an_unwalkable_page_tree_is_flagged_rather_than_reported_as_empty`
  both obtained "an unwalkable page tree" by relying on `minimal.pdf`'s
  now-fixed `/Resources` defect, and both went RED when the defect was
  fixed. Every prior `R225` instance is a test that measured LESS than its
  name/doc comment claimed; this is the inverse. Both repointed at a new
  fixture, `fixtures/synthetic/xref-recover/page-tree-cycle.pdf` (a
  `/Pages` node listing itself in its own `/Kids`).
- **A first-draft justification was replaced mid-Pass, not merely
  reworded, by the dispatched spec librarian.** The claim "a page with no
  `/Contents` can never name a resource" is false — §7.8.3's third bullet
  lets a form XObject or Type 3 font inherit the page's `/Resources`, and
  the ISO 32000-2 erratum extends that to annotation appearance streams,
  which is exactly the stamp-page shape in play. The decision to default
  survived; the reason given for it did not.
- **A candidate finding at n=2, flagged rather than minted**: a value
  computed and discarded via `.unwrap_or_default()`, whose ABSENCE is then
  read as a content fact, is the same shape as `Pass 285.0`'s whole-buffer
  blank (a different subsystem — redaction, not stamp reading). Worth a
  standing rule if a third instance surfaces; not yet.
- **`docs/NEXT_SESSION.md` is now stale** — it still states "the queue is
  empty of inbound work" as of `Pass 288.0`; two more requests have since
  arrived and closed. Flagged for the engineer, not edited (that file is
  engineer-owned).
- Spec-librarian corpus additions from this session (relayed, not
  independently confirmed by this filing): `D:\Dev\Rag-Specialized\
  PDF_Spec\iso32000\iso32000__ref__page_required_attributes_absent.md`,
  amendments to `iso32000__s__7.7.3.md` and the ambiguity register
  (`PR-N1`/`PR-N2`), noting `/MediaBox` absent is a separate case
  (register `PB-A5`, not §7.7.3.4) — worth a `personal_rag/pdf` finding
  for Acrobat's own habit of writing a contentless, resourceless spacer
  page inside a user stamp collection, and for the PDF Association CTO's
  quoted empirical population claim ("a lot of PDFs out there fail this
  simple validation") — **not written this filing** (budget; flagged for
  next librarian session, not forgotten).

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14, 18 all carried forward, unchanged.
- Three of this morning's five `pdfcer-gui` requests remain open, in the
  requester's stated priority: placed-stamp label size read/write gap,
  the shrink/clip-fit disclosure gap (`applied_autosize` is `None` on
  every stamp), and `place_page_artwork` (no verb draws one page's
  artwork onto another). Not yet scoped to Pass IDs.
- The candidate `personal_rag/pdf` findings named above (Acrobat's own
  spacer-page habit; the population-scale empirical claim) are named but
  not yet written — carry forward as a small owed task, not a numbered
  owed-ledger item (a documentation debt, not a defect).

**For next session:**
- Scope the three remaining open `pdfcer-gui` requests into Pass IDs.
- Write the two flagged `personal_rag/pdf` findings.
- Treat `docs/NEXT_SESSION.md` as stale until the engineer refreshes it —
  do not carry forward its "queue is empty" claim.

## 2026-09-10 (493rd filing)

**Shipped:**
- Pass 289.0 (`9b7bc6c`) — a `/Text` sticky note or `/Stamp` naming a
  standard icon, with no `/AP`, is now painted from pdfcer's own icon
  artwork rather than left blank. Triggered by the operator's
  `Annotations_output.pdf` (PDFsharp 1.3): three annotations, no `/AP` on
  any of them, `R43` correctly left the page blank while Acrobat Reader
  showed content. §12.5.6.4 Table 172 and §12.5.6.12 Table 181 put a
  `shall` on **conforming readers**, not on the annotation, to provide
  predefined icon appearances — so drawing the icon discharges a duty the
  standard assigned to pdfcer, not synthesis. `R43` is narrowed, not
  repealed: `/Square`, `/Circle`, `/Line`, `/Ink`, `/Caret` stay governed
  by its original text (their `shall`, where one exists, addresses the
  annotation, not the reader). New counter `annots_icon_painted`,
  appended to `render-page`'s metrics line; old "not painted" stderr note
  rewritten rather than left to contradict the new pixels. 6 tests
  (`crates/pdfcer-render/tests/named_icon_without_ap.rs`) — a sabotage of
  the subtype restriction survived on the original 5 because a `/Square`
  control failed for the wrong reason (a different guard caught it first);
  a `/FreeText` fixture, the 6th test, separates the two failure modes.
  `tools/run-gates.sh` PASS 29/29 (relayed, no shell this filing).

**Decisions made this session:**
- **Decision 149** — the grammatical subject of a spec `shall` clause
  ("conforming readers shall…" vs "the annotation shall…") is the
  discriminator for whether a no-`/AP` look is forbidden synthesis or an
  obligation `R43` does not reach. `ARCHITECTURE.md` §12; `R43`'s own
  `ROADMAP.md` entry carries a matching narrowing note. No body-section
  edit — a rendering-policy change inside `pdfcer-render`'s existing
  annotation-paint loop, not a crate-boundary or invariant change.

**Findings + decisions:**
- **A corpus sentence sourced 2026-07-31 sat unused until 2026-09-10.**
  §12.5.2's "individual annotation handlers may ignore this entry and
  provide their own appearances" was filed into the spec RAG the same
  session `R43` was written, and never reached the decision it governed
  for five weeks / 492 filings. Second recorded instance of this shape —
  the first is the XFA-deprecation finding in `CLAUDE.md`'s Outstanding
  open items. Cross-referenced from decision 149; not yet a standing rule
  (n=2), flagged for `pdfcer-spec-librarian` if a third instance appears.
- A metrics-line gate (`check-metrics-line-contract.py`) used to locate
  the format string's end by naming the last key — a maintenance trap its
  own comment says had already gone stale before, failing loudly and
  unread across several Passes. Fixed to scan to the closing quote
  instead; caught a real omission (`annots_icon_painted` missing from the
  first draft of the published template) immediately. Filed as an `R243`
  dated instance.

**Still in flight:**
- Owed item 18 opened: `decision 145`'s disclosure obligation has a gap
  in the **recovery** path — the same `Annotations_output.pdf`'s
  `startxref` points 134 bytes short of its own `xref` keyword, recovery
  drops the resulting corrupted content-stream object with no anomaly
  recorded, and mis-describes it as "not in the file" when it is present
  and simply declined. Not scoped to a Pass yet.
- Items 4, 5, 10, 11, 13b, 14 carried forward unchanged.
- `docs/FEATURES.md`'s new row for this capability is `gui [ ]` — the fix
  lives in the shared `pdfcer-render` annotation-paint loop, which
  `pdfcer-gui`'s canvas calls directly, but that repo pins `pdfcer-render`
  as a **git dependency** (`branch = "main"`, not a path dependency), so
  it needs `cargo update -p pdfcer-render` in `D:\dev\pdfcer-gui` before
  the fix is actually reachable there. Flagged, not performed by this
  role.

**For next session:**
- Scope owed item 18 (decision-145 recovery-path gap) into a Pass.
- Confirm `pdfcer-gui` has pulled the updated `pdfcer-render` revision
  before treating the FEATURES row's `gui` box as answered either way.
- Consider whether `/FileAttachment`/`/Sound` icon artwork is worth
  building, now that the reader-`shall` pattern is established for them
  too (currently deliberately excluded — no artwork exists).

## 2026-09-10 (492nd filing)

**Shipped:**
- Pass 288.1 (`4b45a96`, committed, not yet pushed) — `stamp-pack` gains
  `--stamps-from <FILE>`, a name-list file (one name per line, `#`
  comments and blank lines skipped) so a 113-page stamp sheet doesn't need
  113 repeated `--stamp` flags. Names are not auto-derived from the
  artwork (the supplied sheets are pure vector, no text layer) — asking is
  better than a picker full of `Stamp001`. Verified against two real
  third-party files: a non-Adobe dynamic-stamp collection with generated
  internal names, and a 113-page pure-vector sheet round-tripping through
  `stamp-pack` → `stamp-list`.
- `tools/edit-source.py` (`aeeecb5`, committed, not yet pushed — the
  pre-push gate was blocking on this until it was filed) — a
  line-ending-agnostic exact-replacement tool. A multi-line `str.replace`
  against a CRLF file with an `\n`-typed pattern matches zero times
  **silently**; this happened three times in one session despite an
  existing written warning in `docs/NEXT_SESSION.md`. The tool refuses a
  non-exactly-one match and writes nothing unless every replacement
  succeeds; patterns are passed as file paths (not shell arguments) after
  this project separately lost content out of a pushed commit message to
  shell-argument mangling.

**Decisions made this session:**
- No new architectural decision — neither filed item touches
  `pdfcer-core`/`pdfcer-render`'s public surface or the object model.
  Decision ledger stays at `148`.

**Findings + decisions:**
- **A hazard written down and hit anyway is a missing tool, not a missing
  warning.** Filed as a dated instance of `R243` (not a new mint) —
  `R243`'s own text already covers "a documented obligation … is not a
  control"; this instance is the same mechanism one layer out (a warning
  failing to stop a repeated *manual* action, not two call sites failing
  to agree on a value). The remedy differs from `R243`'s usual one
  (extract into a shared function) because there is no function to
  extract from a human/agent re-typing an edit by hand — the remedy here
  is a tool that refuses the silent failure mode outright.
- `R250` (minted last filing, 491st, in the Shipped-section banner only)
  was owed its *Standing rules* master-list entry — discharged this
  filing.
- The CRLF/`str.replace` gotcha itself is flagged for
  `troubleshooting-librarian` (`personal_rag/claude_code` or
  `personal_rag/python`) rather than written by this role — it's a
  general Python-scripting-under-Claude-Code finding, not PDF-domain and
  not Rust/egui-ecosystem, so it sits outside every tier this role owns.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- Both `4b45a96` and `aeeecb5` are committed but not yet pushed; the
  push is gated on this filing landing (per the coordinator's mid-dispatch
  note) and was not performed by this role — no shell this filing to
  confirm push/CI state either way.
- The `Annotations_output.pdf` (PDFsharp) `/AP`-less annotation question
  — whether `R43` is being applied outside its territory for a *named
  standard icon* stamp with no `/AP` — is under investigation by the
  engineer via `pdfcer-spec-librarian`, explicitly **not resolved**; do
  not treat as closed.

**For next session:**
- Push `4b45a96` + `aeeecb5`, confirm CI colour with a shell.
- Dispatch `troubleshooting-librarian` for the CRLF `str.replace` lesson,
  if judged worth a personal_rag entry.
- Follow up on the `Annotations_output.pdf` `/AP`-less-stamp investigation
  once `pdfcer-spec-librarian` reports back.

## 2026-09-10 (491st filing)

**Shipped:**
- Pass 288.0 (`554897e`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — custom stamp
  collections, readable and authorable, compatible with Adobe's:
  `pdfcer_core::stamp_file`, `EditSession::set_named_pages` (verb 221),
  CLI `stamp-list`/`stamp-pack`. A collection is an ordinary PDF, one
  page per stamp; category = `/Info` `/Title`; stamp names live in the
  catalog's `/Names`→`/Pages` name tree; `#` marks a dynamic stamp
  (read, never authored). There is no separate interchange format —
  "export" is handing someone the PDF, and that is Acrobat's own
  answer too.

**Decisions made this session:**
- Decision **148** minted (`ARCHITECTURE.md` §12): the stamp-collection
  format — category in `/Info` `/Title`, stamp names in the catalog's
  name tree written in **lexicographic** order per §7.9.6 (not page
  order — Adobe's own `StandardBusiness.pdf` proves the two differ),
  `#` prefix for a dynamic stamp, `/PieceInfo` rejected as a red
  herring (present only alongside `/Illustrator` data, never as the
  naming mechanism).
- Standing rule **R250** minted, this role's own synthesis of a finding
  the engineer offered without a number: a Feature-RAG entry labelled
  `(c)` convergent-secondary is a pointer at what to go verify against
  a primary artifact when one is on disk (here, Adobe's own shipped
  stamp files), not a license to build from unchecked. Full text:
  `ROADMAP.md` *Standing rules*.

**Findings + decisions:**
- **The methodological point is the more durable finding.**
  `pdfcer-acrobat-librarian` reached the correct capability shape from
  convergent community sources and correctly flagged two gaps by name
  (where the category name is stored; whether `#` was real) rather
  than guessing. Both were closed by reading Adobe's own shipped stamp
  files directly — two file opens, not a research session — which is
  exactly what the RAG's own `(c)` label pointed at doing.
- `pdfcer-acrobat-librarian`'s two RAG files
  (`markup__stamp_text_size_and_resize_behavior.md`,
  `markup__custom_stamp_file_format.md`) are flagged for that role to
  consider upgrading the two now-closed gaps from `(c)` to `(b)
  observed` — not this role's corpus to edit (hard rule 6's sibling
  boundary, applied to a confidence label rather than content).
- §7.9.6 name-tree order is lexicographic, not page order — Adobe's
  own file proves it (`SBApproved` names page 0, `SBCompleted` names
  page 4). A test deliberately gives page 0 the alphabetically-last
  name so a page-order-emitting implementation fails it.
- A doc-comment orphan (splicing `set_named_pages` above
  `set_info_field` stranded its doc block) — the same shape as earlier
  the same session (`Pass 287.0`).
- A CRLF/LF `str.replace` matched zero times silently for the third
  time this session; a small line-ending-agnostic edit helper was
  built in a temp dir but **not added to the repo** — flagged for the
  engineer to judge whether it belongs in `tools/`.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- Whether `554897e` has actually reached `origin/main`, and current CI
  colour, are relayed from the dispatch only — not independently
  confirmed, no shell this filing.

**For next session:**
- Confirm `554897e`'s push/CI state with a shell.
- Flag `pdfcer-acrobat-librarian` for the `(c)`→`(b)` label-upgrade
  consideration above.
- Judge whether the line-ending-agnostic edit helper belongs in
  `tools/`.

## 2026-09-10 (490th filing)

**Shipped:**
- Pass 287.0 (`1bbb7c1`, committed, not yet pushed) — a stamp's text
  size is now a property (`StampStyle`/`StampFit`, `/DA` storage,
  `--stamp-font-size`/`--stamp-fit`), the box follows the text by
  default, and both size and a custom label are recovered from every
  pre-existing stamp's baked appearance — which also closes a standing
  defect where `resize_annotation` refused pdfcer's own stamps as
  foreign.
- Chore commit `e774a41` (lockfile bumps + `pdfcer-acrobat-librarian`
  agent-memory notes) — filed alongside Pass 287.0, same push gate.
- `v0.50.0` released (relayed from the dispatch — no shell this
  filing): ten Passes since `v0.49.0` (`277.0`→`286.0`), tagged,
  pushed, GitHub release published latest with zip+sha256,
  `verify-release.py` clean on every check, fresh-folder smoke test
  run, OneDrive slot `pdfcer1` (alternating scheme, `0.49.0` preserved
  on `pdfcer2`). 159 GB reclaimed from `target/` in the same window
  (154 GB of it in `target/debug/deps` alone, on a disk at 90% full).

**Decisions made this session:**
- Decision **147** minted (`ARCHITECTURE.md` §12): where the spec
  defines no key for a subtype's derived parameter, pdfcer borrows the
  key the standard already defines for the identical problem on a
  sibling subtype rather than inventing a private sidecar — `/DA` on
  `/Stamp`, sourced from §12.7.3.3's `/FreeText` entry, not
  `/PieceInfo`. Sourced via `pdfcer-acrobat-librarian` before the
  choice was made. A `StampFit` policy is settable but deliberately
  never recovered from an existing file — nothing stored records an
  author's intent, and inferring one from geometry would invent a
  decision nobody made.
- No new standing rule minted. The Pass's fake-test finding (reverting
  the fix left `stretching_a_stamp_keeps_its_text_size` green because
  the fixture stretched only width, and the old formula depended only
  on height) is filed as `R225`'s **17th** dated instance, not a new
  rule or an `R247` instance — it is a fixture that could not
  discriminate two implementations on the axis they actually disagree
  on, squarely `R225`'s family, not a doc comment (`R247`'s shape).

**Findings + decisions:**
- **Two compounding defects, which is why this shipped as a
  `StampStyle`, not a one-line bug fix.** A stamp's label was clipped
  to `/BBox` (right for a form field's box, wrong for a stamp's
  drawing gesture), and the repair scaled the text because the font
  size was `(rect_height * 0.42).clamp(8, 28)`, derived from the box
  and stored nowhere. Either alone is an annoyance; together the first
  mistake is unfixable — you cannot escape the clip by resizing,
  because resizing rescales the text with it.
- **The more valuable finding is the second one the first uncovered: a
  stamp's custom label was stored nowhere either.** `/Contents` is a
  comment *about* a stamp, not its words, so a rebuild always produced
  the stamp name's default label — which is why `resize_annotation`
  refused pdfcer's own stamps as foreign. The long-open
  `request_resize_annotation_refuses_a_pdfcer_authored_stamp_as_foreign.md`
  had been read as a geometry bug; it was a spec-completeness gap. The
  authorship test was correct; the spec it tested against was lossy.
- Both values are now recovered from the appearance itself
  (`EditSession::recover_stamp_parameters`) — the same both-ways trick
  `Pass 276.0` used for `/FreeText`'s `multiline`. Recovery is
  mandatory, not optional: every stamp already in every document has
  no `/DA`, so a stored property alone would silently change all of
  them on first touch.
- `resize_annotation` gained a **third** authorship arm (markup,
  `/FreeText`, now `/Stamp`) — `R245`'s shape on a family of three
  routes, closed rather than merely counted again.
- A dead function (`stamp_font_size_from_appearance`, superseded) was
  deleted rather than kept with a justifying comment — the second time
  this session clippy caught a function kept alive only by its own
  doc comment (the first was `Pass 286.0`'s "the honest raw record"
  field, filed at the 488th filing). `tf_size_in` unified into one
  shared implementation instead of two token scanners that could
  disagree.
- The fuzz target now drives the new `style` field from fuzz input
  rather than `..Default::default()`, on the reasoning that a new
  field satisfied only by its default is a new field nothing fuzzes.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged.
- `docs/NEXT_SESSION.md`'s queue item 2 (the stamp-resize request) is
  now closed by `Pass 287.0` — flagged for the engineer to update
  directly; that file is engineer-owned and was not edited here.
  Its "look for a fourth authoring family" note is answered as far as
  this filing can tell (three families now recognised, closed) but
  whether a fourth was actually searched for and not found, versus
  simply not searched, was not stated in the dispatch and is not
  asserted here either way.
- `e774a41` and `1bbb7c1` are committed but **not pushed** as of this
  filing (per the dispatch); pushing both, and confirming
  `v0.50.0`'s tag/push/release state independently, needs a shell —
  not available to this role this filing.

**For next session:**
- Push `e774a41` and `1bbb7c1`; the operator's own ordered plan
  (`Pass 142.0`, resize-page-contents, `Pass 259.0`, `Pass 10.11`) is
  otherwise untouched, per `docs/NEXT_SESSION.md`.

## 2026-09-09 (489th filing)

**Shipped:**
- Nothing — a librarian reconciliation filing, dispatched and ruled on by
  the engineer directly.

**Decisions made this session:**
- The engineer resolved the `R247` reservation directly (his own explicit
  call, not derived by this role): standing rule `R247` — *a doc comment
  stating a behavioural guarantee is an unenforced claim until a test
  exists that would fail if it were violated* — claims the number. The
  competing "alternate route" sabotage-fixture cause (`n=3`) is
  **withdrawn**, not deferred and not given its own number: its instances
  are already correctly filed inside `R225`'s own family, and a second
  number for one family would only hand a future reader two rules to
  reconcile mid-defect.
- No `ARCHITECTURE.md` §12 decision minted — a standing-rule numbering
  resolution is a librarian/engineer process call, not a crate-boundary/
  library/invariant redefinition, consistent with `R225`'s and `R246`'s own
  precedent (neither carries a §12 decision either).

**Findings + decisions:**
- `R247`'s founding instance is `Pass 285.0`'s `blank_show_strings` doc
  comment (already on record as `R225`'s 16th instance): a stated
  span-scoping safety guarantee that a scope-widening sabotage would have
  violated, caught only because the fixture was widened to separate the
  two implementations' output.
- `R247`'s second, lesser instance is `Pass 286.0`'s "the honest raw
  record" doc comment on a dead field, caught by `clippy` rather than a
  person — recorded to show the rule catches low-severity cases too.
- The two rules are distinguished on the record rather than merged:
  `R225` is a **test** whose own name over-claims relative to its fixture;
  `R247` is a **doc comment** publishing a guarantee no test enforces at
  all. A reader re-derives behaviour from a test only once they distrust
  it; they trust a doc comment *instead of* re-deriving — the entire point
  of documentation-first discipline — which makes the doc-comment case the
  more dangerous of the two.
- Cross-project derivation filed as its own new file (not a section of the
  existing sabotage-fixture file), because the two findings are found and
  repaired differently:
  `D:\dev\rag\rust\a_doc_comment_stating_a_behavioural_guarantee_is_unenforced_until_a_test_would_fail_without_it.md`.
  Dated footer also added to the existing sabotage-fixture file pointing
  forward to it, so a reader who lands there via the 16th-instance note
  does not read `R247` as still unclaimed.

**Still in flight:**
- Owed items 4, 5, 10, 11, 13b, 14 all carried forward, unchanged. Item 9
  (the `R247` reservation) is CLOSED this filing — resolved, not deferred.
- `docs/NEXT_SESSION.md`'s own OWED bullet on `R247` is now stale (it still
  reads "reserved-but-unclaimed... resolve it before a fourth candidate
  lands") — flagged for the engineer to update directly; that file is
  engineer-owned and was not edited here.

**For next session:**
- Nothing `R247`-specific remains. Next items are whatever `NEXT_SESSION.md`
  and the carried-forward owed list (above) already name.

## 2026-09-09 (488th filing)

**Shipped:**
- Pass 286.0 (`369d4de`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — closes owed item 17: a
  per-glyph producer's redacted text is no longer single characters.
  `RedactionReport::redacted_text` is now grouped per `/Redact` mark, not
  per show operator — `Surgeon::glyph` returns the region index a glyph
  landed in (was a bare `bool`), and `box_marks` folds a mark's characters
  into one string. `SW41177-obselete.pdf` (GPL Ghostscript 8.15) drew one
  glyph per show operator, so marking `3.5 TYP` used to yield
  `["3", ".", "5", " ", "T", "Y", "P"]` and a consuming absence proof
  refused a correct redaction on finding `"3"` on every page. Now yields
  `["3.5 TYP"]`.

**Decisions made this session:**
- None minted. Not a crate-boundary/library/invariant change — a bug fix
  in a shared field's granularity.

**Findings + decisions:**
- **The three-consumer analysis.** `redacted_text` has three readers (the
  absence proof, `carrier_info`, `residual_sweep`'s `redaction_evidence`)
  and joining characters into per-mark strings is safe for all three only
  because it strictly LENGTHENS entries — never shortens them — so every
  reader's match floor (`MIN_MATCH_LEN` = 4 characters) is cleared more
  reliably, not less. A change that split entries instead would not carry
  the same guarantee, and that asymmetry is the only reason a field with
  three readers could be changed in one Pass. Flagged as a trap in the
  handoff before the Pass was built; resolved in the safe direction.
- **Nothing new was inferred to make the fix** — `Surgeon::glyph` already
  computed which region a glyph landed in; it was discarding that as a
  bare `bool` one line before the caller needed it.
- **A kept, justified, unread field was deleted, not excused.** The old
  per-operator `removed_text: Vec<String>` field was first kept beside the
  new map with a doc comment calling it "the honest raw record"; `clippy`
  flagged it as unread and it was deleted. Recorded as a recurring
  self-deception shape: an unread field with a justification attached is
  not a record, it is dead weight with an excuse.
- **The fixture is the finding, again.** A producer drawing the run in a
  single `Tj` cannot distinguish old grouping from new (both report
  `["3.5 TYP"]`), so a test written on an ordinary producer would have
  been green before and after this Pass, measuring nothing. The new
  fixture emits one `Tm … (c) Tj` per character.
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  17's sentence replaced with the fix and the per-mark grouping named.
- `C:\personal_rag\pdf\`: a second dated footer added to the existing
  2026-09-09 lesson on this producer (the "joining half" is no longer
  unbuilt); subject-index and master-index bullets corrected in place.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14 all carried forward. Item 17 discharged this filing.
- **`R247` reservation flagged for a FOURTH consecutive filing** (475th,
  483rd, 486th, 487th, now 488th) — still unreconciled, two candidates
  contesting the slot (a second `///`-guarantee-with-no-enforcing-code
  instance; the "alternate route" sabotage-fixture cause at `n=3`). `R248`
  and `R249` were both minted past it deliberately. Nobody has yet sat
  down with time to resolve it.
- Whether `369d4de` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Resolve the `R247` reservation — this is now a fourth consecutive
  filing carrying the flag forward unresolved, the longest it has run.

## 2026-09-09 (487th filing)

**Shipped:**
- Pass 285.0 (`1366138`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — closes owed item 18
  (`Pass 284.0`): an abandoned content stream's drawn text is now blanked,
  not merely named. `blank_show_strings` parses the stream and touches only
  the operand spans of `Tj`/`TJ`/`'`/`"`, never the whole buffer, so a
  resource name sharing bytes with the redacted evidence is never
  corrupted into one that resolves to nothing. New counter
  `residual_content_streams_blanked`. Still declines and discloses (the
  sweep's existing floor): a non-parsing stream, and glyph-code text on a
  subset font.

**Decisions made this session:**
- None minted. `R225` gains a 16th dated instance (severity escalation,
  not a new cause) to its RAG file; `R249`/`R247` untouched.

**Findings + decisions:**
- **A scope-widening sabotage survived all eight tests in
  `redaction_residual_sweep.rs`.** The wrong implementation (blank every
  byte-occurrence of the evidence in the whole buffer, not only the
  show-operator spans) agreed exactly with the correct one on the shipped
  fixture, because the fixture's only occurrence of the word was inside a
  string operand — the one place both implementations blank. Outside that
  string the wrong implementation also corrupts resource names, which is
  precisely the failure the function's own doc comment names as the
  reason for the narrower scope. Fixed by widening the fixture (the word
  now also appears in a resource name) rather than by strengthening the
  assertion, which could not have discriminated on the old fixture no
  matter how it was written.
- **Escalation recorded explicitly, at the engineer's request**: every
  prior instance of this project's `R225` sabotage-fixture family is a
  test measuring less than its own *name* claimed. This is the first
  where the survived sabotage would have shipped a defect the project's
  own *documentation* claimed was impossible — judged a severity clause
  on the existing "scope or filtering" degenerate-value row, not a new
  cause and not a rule amendment. Filed as the RAG file's 16th dated
  instance; no mint, `R225`'s founding text unchanged, `R249` remains the
  standing-rule ceiling and `R247` remains reserved-but-unclaimed.
- **Test amended, not deleted, honest half preserved as a new control**:
  `Pass 284.0`'s `an_unreachable_content_stream_is_named_not_silently_left`
  became `an_abandoned_content_streams_drawn_text_is_blanked` (old
  assertions kept struck through in place); the disclosure half it used
  to carry survives as a new, separate test,
  `a_stream_that_cannot_be_blanked_is_still_named`, over a fixture the
  blanking function structurally cannot reach — without it, a future
  silent regression in the disclosure would pass unnoticed.
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  18's sentence replaced with the fix, the new counter named, and the two
  remaining declining cases named where item 18 used to be.
- `D:\dev\rag\rust\a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`
  gains a 16th dated footer (the escalation above); its `index.md` bullet
  extended in the same edit, along with a compact catch-up note for
  instances 12–15 which the index bullet had fallen behind on.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14, 17 all carried forward. Item 18 discharged this filing.
- The `R247` reservation is still unreconciled — flagged again, three
  filings running now.
- Whether `1366138` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Resolve the `R247` reservation before a fourth unrelated candidate
  makes the gap harder to reconcile — this is now the third consecutive
  filing carrying that flag forward unresolved.
- `D:\dev\rag\rust\index.md`'s summary bullet for the sabotage-fixture
  file had drifted behind its own source file (missing instances 12–15
  before this filing added a catch-up note) — worth a proper backfill
  next time an `D:\dev\rag\rust\` index-check runs, rather than leaving
  the catch-up note as the permanent form.

## 2026-09-09 (486th filing)

**Shipped:**
- Pass 284.0 (`ea4acb3`, pushed to `origin/main` per the dispatch — not
  independently verified, no shell this filing) — the queue's head owed
  item ("an orphaned `/Info`-shaped object survives a redaction") turned
  out to be one instance of a class: every redaction carrier finds its
  target by navigating the document graph, while the writer emits objects
  by enumerating the cross-reference table, and every object in the
  difference was re-emitted verbatim into a redacted file, never offered
  to a carrier. New fourteenth carrier `residual_sweep` closes it,
  scoping the sweep to the xref table's own listing rather than to a
  computed reachability walk. Closes owed item 16; also closes three
  carriers nobody had filed (a thread's own information dictionary, and
  two further XMP routes, §14.3.2 B/C) as a byproduct of sweeping instead
  of enumerating. New owed item 18: a non-metadata content stream
  carrying redacted text is named, not removed.

**Decisions made this session:**
- **Decision 146** (`ARCHITECTURE.md` §12, body §5.9): a destructive
  sweep obliged by an outcome-shaped requirement ("remove all traces of
  X") is scoped by the evidence the requirement itself names, never by a
  computed reachability walk — because reachability computations on a
  graph-shaped format fail silently rather than loudly, and the census
  probe built to measure this very fix reproduced that failure shape
  twice within the hour it was written.
- **Standing rule `R249` minted** from the engineer's own generalisation,
  which was offered unnumbered and left for this filing to judge. Minted
  past the still-reserved-but-unclaimed `R247`, same precedent `R248`
  itself set one filing ago. Full text in `ROADMAP.md` *Standing rules*.

**Findings + decisions:**
- **A census probe made the exact mistake it was written to catch,
  twice, within an hour** — `examples/unreachable_census.rs`, written
  immediately after reading §12.5.6.23, first counted every object
  stream as an orphan, then every cross-reference stream, before landing
  on the correct figure. Both wrong numbers (21% and an intermediate
  figure) were caught only by measurement, never by re-reading the
  clause — the final reported figure moved 21% → 12%. Recorded because a
  filing that keeps only the final 12% loses the lesson that a careful,
  purpose-built reachability computation, written by someone who had just
  argued against trusting reachability computations, made the trap-shaped
  error anyway.
- **Two implementation bugs in the new sweep, both caught by tests
  written for earlier Passes**, an argument against deleting a test whose
  subject you are changing: a staged span not indexing the base buffer
  (`stage()` allocates at `base_len + staging.len()`, so slicing the
  original bytes with a replaced stream's span read the wrong region);
  and two different empty answers collapsed into one report value ("no
  text redacted at all" vs. "text redacted but all of it below the match
  floor" both reported the same way in the first cut).
- **A PDF-domain empirical finding**, filed to `C:\personal_rag\pdf\`:
  measured over the operator's own 57-file drawing set, 12 files (21%)
  carry objects the cross-reference table lists but the document graph
  never reaches, 20 such objects total, 7 of which could carry drawn
  text — merge outputs are the worst offenders. Distinct from the spec
  text half (§14.3, filed by `pdfcer-spec-librarian`).
- **Two new `D:\dev\rag\rust\` findings**: the staged-span base-offset
  indexing bug (generalises to any base-plus-staging-buffer pattern), and
  the census-probe-reproduces-the-trap-it-measures finding (generalises
  as a caution about trusting a freshly-written verification computation
  more than the mechanism it is checking, when both share a structural
  blind spot).
- `docs/FEATURES.md`'s *Apply redaction* row amended in place: owed item
  16's sentence replaced with the fix (fourteenth carrier, two new
  counters, the three incidentally-closed carriers, the evidence-not-
  reachability argument in brief), and the new non-metadata-stream gap
  named where item 16 used to be.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11,
  13b, 14, 17, 18 (new this filing) all carried forward.
- The `R247` reservation is now flanked by three unrelated, already-
  decided-past-it rules (`R248`, `R249`) — worth resolving soon so the
  numbering gap does not become confusing in its own right.
- Whether `ea4acb3` has been pushed or released is relayed from the
  dispatch only, not independently checked — the engineer should verify
  directly.

**For next session:**
- Item 18 (non-metadata content stream carrying redacted text, named not
  removed) is queued, unstarted — a destructive act on a new object
  class, deliberately not folded into `Pass 284.0`.
- Resolve the `R247` reservation before a fourth unrelated candidate
  makes the gap harder to reconcile.

## 2026-09-09 (485th filing)

**Shipped:**
- Pass 283.1 (`d8fcb68`) — addendum to Pass 283.0: the disclosed,
  overridable malformed-PDF policy reached only
  `Document::from_bytes_with_options`; every real shell opens a **file**,
  not bytes. New `Document::load_with_options(path, password, options)`,
  and `pdfcer`'s own `open_document` now uses it instead of duplicating
  `Document::load`'s `std::fs::read`. The CLI's `--on-malformed` override
  is now reachable from a real invocation, not only from the bytes-based
  test harness.

**Decisions made this session:**
- None minted. This is a completeness fix inside decision 145's own
  mechanism (`ARCHITECTURE.md` §10.5, §12), addended in place rather than
  re-argued.

**Findings + decisions:**
- Sixth dated instance of standing rule `R245` (`ROADMAP.md` *Standing
  rules*): the rule's shape (a guard/key/disclosure shipped on one member
  of a parallel family, untested on the rest) recurs over a family of TWO
  ENTRY POINTS rather than verbs, and the withheld item is an *affordance*
  rather than a restriction — `docs/core-api/01-reading-and-model.md`
  §3.6b already names this explicitly. No amendment to `R245`'s text; the
  fix (a test that opens the same path twice, once directly and once
  through `load_with_options`) is exactly the family-wide test the rule
  asks for.
- Also `R151`-adjacent, noted rather than merged: the affordance had test
  callers all along, just not its intended production caller — `R151`'s
  canonical shape is zero callers, so this stays a distinct observation
  under `R245` rather than folding into `R151`'s text.
- Filed as a dated footer on the existing `D:\dev\rag\rust\` finding
  (`a_guard_or_key_added_to_one_sibling_verb_is_untested_until_a_family_wide_test_exists.md`),
  not a new file — the underlying mechanism is unchanged, only the family
  shape. `index.md` line updated in the same edit.
- `docs/FEATURES.md` row 169 (*Document & pages*) sentence amended to name
  `Pass 283.1`'s fix; no checkbox moved — the row was already correctly
  ticked for the capability, which now, post-fix, actually reaches a real
  invocation rather than only the bytes-based test harness.

**Still in flight:**
- Owed items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11, 13b,
  14, 16, 17 all carried forward unchanged.
- Whether `d8fcb68` has been pushed or released is not asserted here (no
  shell) — the engineer should check directly.

**For next session:**
- Same open items as the 484th filing's own "For next session" entry —
  nothing new surfaced by this addendum beyond the `R245` instance
  captured above.

## 2026-09-09 (484th filing)

**Shipped:**
- Pass 283.0 (`dce2223`) — a PDF with structural errors now opens instead
  of refusing: six defect classes (duplicate dictionary key, missing/
  unusable `/Length`, missing `endobj`, an unparseable object, an xref/body
  id disagreement, an unreadable object stream) are each recorded as a
  `LoadAnomaly` — what pdfcer decided, and what it discarded — with a new
  CLI `--on-malformed keep-last|keep-first|refuse` letting the operator
  take the other decision. Prompted by the operator's own file (a real
  drawing Acrobat opens and pdfcer refused, on a duplicate `/PageMode`
  key) and his own ruling that this must generalize to "all defects where
  it is possible to continue and open the file" — not a one-defect patch.

**Decisions made this session:**
- **Decision 145** (`ARCHITECTURE.md` §12, body section §10.5): a
  structural defect that leaves the object graph AMBIGUOUS rather than
  UNDEFINABLE is opened under a disclosed, overridable default; only a
  defect requiring pdfcer to INVENT a reading (no `/Root`; encryption with
  no working password) stays fatal. Argued as an extension of `R27`'s
  fail-clean kernel from the decoder layer to the loader layer, not a
  relaxation of it — `R27` was always about silence, never about refusal.
- **Standing rule `R248` minted**, argued rather than deferred, directly
  from the operator's own general-scope ruling (matching this project's
  `R35`/`R58`/`R67` precedent for minting from a decisive ruling rather
  than waiting for a second occurrence). Numbered past the still
  reserved-but-unclaimed `R247` deliberately, to avoid entangling this
  claim with that unrelated, unreconciled reservation (two other
  candidate triggers, neither decided).

**Findings + decisions:**
- **An analogy dressed as a citation, caught before shipping.** The first
  draft justified keeping the LAST value on a duplicate key by citing
  §7.5.6 (incremental-update object ordering) — an ordering the standard
  makes meaningful, borrowed to justify a decision about §7.3.7's
  dictionary-entry order, which the standard's own preceding sentence says
  "shall be ignored." `pdfcer-spec-librarian`'s answer corrected it before
  the code shipped: the real support is observed behaviour (qpdf, pdf.js,
  pdfium all keep-last, none refuses), not an internal spec analogy. Filed
  as a new `D:\dev\rag\rust\` methodology finding — close in spirit to
  `R246`'s reference-corpus reinfection finding, but about analogical
  reasoning rather than a stale figure.
- **A flag named for one member of the class it governed.** The first cut
  called the new CLI flag `--duplicate-keys` while its `strict` value also
  silently disabled two unrelated recoveries (`/Length`, `endobj`).
  Renamed to `--on-malformed` before shipping. Filed as a second new
  `D:\dev\rag\rust\` methodology finding.
- **A PDF-domain empirical finding, filed to `C:\personal_rag\pdf\`
  rather than duplicated into the spec RAG:** real-world readers (qpdf,
  pdf.js, pdfium) converge on keep-last for duplicate dictionary keys,
  where ISO 32000 itself leaves reader behaviour explicitly out of scope
  (pdf-issues #199). The spec-text half (the `shall not`, the erratum
  #3 precedent) is `pdfcer-spec-librarian`'s territory and is already
  filed there (new `iso32000__s__7.3.7.md`, supersession redirect,
  register entries `DK-A1`/`DK-A2`).
- `docs/FEATURES.md` gains a **new** row under *Document & pages*, kept
  deliberately separate from the existing xref-recovery row (different
  mechanism: object-level ambiguity resolution with disclosure/override,
  vs. xref-table rebuild-by-scan) — `[x]` core, `[x]` cli, `[ ]` gui,
  `[x]` Acrobat, with the exceed named explicitly: Acrobat opens such
  files too but does not disclose which value it kept or offer the
  alternative.

**Still in flight:**
- Items 4, 5, 9 (`n=3`, `R247` reservation unreconciled), 10, 11, 13b, 14,
  16, 17 all carried forward unchanged from the 483rd filing — this Pass
  originated from a fresh operator report, not from a prior owed item, and
  discharged none of them.
- Whether `dce2223` has been pushed or released is not asserted here (no
  shell) — the engineer should check directly.

**For next session:**
- The `R247` reservation is now flanked on both sides by unrelated,
  already-decided-past-it work (`R246` below it, `R248` above it) — worth
  resolving soon so the numbering gap does not become confusing in its own
  right.
- Owed items 16 and 17 (orphan `/Info`-shaped object; per-glyph
  absence-proof joining) remain queued, unstarted.
- Item 13b's measured blocker is now on record in `ROADMAP.md`: a stamp's
  label size (`(h * 0.42).clamp(8.0, 28.0)`) is derived at bake time and
  stored nowhere, so a re-bake has nothing to recompute from without a new
  stored field.

## 2026-09-09 (483rd filing)

**Shipped:**
- Pass 282.0 (`a83c6e6`) — `redact::carrier_info` (the `/Info`
  metadata-carrier redaction-diligence check) had two opposite
  defects, both reporting `scrubbed`: a one-directional match (a
  redacted run *longer* than the `/Info` string could never match) and
  no length floor (a single-character redacted piece on a per-glyph
  producer matched almost any string). Found while smoke-testing Pass
  281.0 on a file from the private corpus. Fixed by `redaction_evidence`
  matching whole runs **and** their whitespace-delimited tokens at a
  4-character floor, and a new `CarrierAction::CheckedClean` that
  distinguishes a present-and-clean `/Info` from no `/Info` at all
  (previously both reported `Absent`). Discharges the 482nd filing's
  owed item 15.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched) — a bug fix
  and a numeric-constant alignment with a consuming project, not a
  crate-boundary/library/invariant redefinition.
- Declined to mint a standing rule for the "alternate route"
  sabotage-fixture cause, despite it reaching its third occurrence in
  one calendar day (past this project's own `n=2` minting precedent).
  Argued in the roadmap entry: mint/decline decisions for this family
  belong to the engineer inside the finding Pass, not to a
  roadmap-filing pass; the next free rule number (`R247`) is already
  contested by an unrelated trigger and should be reconciled before a
  second cause is folded in; the RAG file's dated-footer mechanism
  already captures every occurrence at low cost. Flagged for the next
  session with time to reconcile `R247`.

**Findings + decisions:**
- The floor (`MIN_MATCH_LEN = 4`) was chosen to match the consuming
  project's own `MIN_VERIFIABLE_LEN`, deliberately — two independent
  redaction-evidence checks disagreeing on the floor would produce a
  file one project calls scrubbed and the other calls unverified.
- Third instance in one session (and third occurrence specifically of
  the "alternate route" sabotage cause) of a test whose fixture could
  not exhibit the defect its name claimed: the first version marked by
  search for a single word contained outright by the metadata string,
  so the *old, unchanged* containment rule decided the case
  regardless of the new token-split logic under test. Repaired by
  redacting a multi-word region instead. New dated footer (instance
  15) in `D:\dev\rag\rust\a_sabotage_can_only_be_as_discriminating_as_the_fixture_it_runs_on.md`.
- **★ A different, new redaction-diligence gap measured and NOT
  fixed:** the file that started this still has one survivor — its
  `/Keywords` lives in an `/Info`-shaped object (140) superseded by
  another (145) that the current trailer now names, but object 140 is
  still listed in the cross-reference table and is therefore
  re-emitted verbatim by the forced full rewrite. `carrier_info` only
  inspects the trailer's own `/Info`; no carrier covers an orphan.
  `prior_revisions action=dropped_by_rewrite` remains true and
  accurate — it is about superseded byte ranges, not objects the xref
  table still names. New PDF-domain lesson,
  `C:\personal_rag\pdf\lesson_20260909_a_superseded_info_shaped_object_still_xref_listed_survives_a_full_rewrite_untouched_by_a_trailer_scoped_scrub.md`.
  Filed as owed item 16; wants its own Pass and a reading of
  §12.5.6.23's "all content" against an xref-listed, trailer-orphaned
  object.
- The length-floor idea behind this Pass's fix is the same one named
  in the same-day Ghostscript lesson
  (`lesson_20260909_ghostscript_8_emits_one_glyph_per_show_operator_so_string_level_checks_see_single_characters.md`)
  for the GUI's content-stream absence proof; applied here to a second,
  independent consumer. That lesson's "joining" half (words from
  adjacent single-glyph shows) remains unbuilt — dated footer added,
  filed as new owed item 17
  (`request_redacted_text_carries_single_characters_on_a_per_glyph_producer_so_the_absence_proof_is_blind.md`,
  confirmed at the source, replied to, not built).

**Still in flight:**
- Items 4, 5, 10, 11, 13b, 14 carried forward unchanged.
- Item 9 (the "alternate route" sabotage cause) strengthened from
  `n=2` to `n=3`; still not minted, `R247` reservation still
  unreconciled.
- Item 15 (the `carrier_info` diligence gap) discharged by this Pass.
- Items 16 and 17 (new): the orphan `/Info`-shaped object, and the
  per-glyph absence-proof "joining" request, both above.
- Whether `a83c6e6` has been pushed or released is not asserted here
  (no shell) — the engineer should check directly.

**For next session:**
- Resolve the `R247` reservation conflict (the "alternate route"
  sabotage cause, now at `n=3`, vs. the unrelated `clap`-derive
  doc-guarantee trigger named 2026-09-08) with time to reconcile both
  properly, rather than guessing one onto the number.
- Owed item 16 (orphan `/Info`-shaped object surviving a full rewrite)
  is redaction-area and reachable on a real file — worth scoping into
  its own Pass before the next redaction-adjacent change.
- Owed item 17 (per-glyph absence-proof joining) has been open since
  earlier the same day; still unbuilt.

## 2026-09-09 (482nd filing)

**Shipped:**
- Pass 281.0 (`1177221`) — a hybrid-reference file (ISO 32000-1
  §7.5.8.4, classic xref table + `/XRefStm`) can now be fully rewritten,
  so redaction — which is forced to a full rewrite by `R35` — finally
  reaches it. The old refusal's own named remedy ("use incremental
  save") was the one thing a redaction is forbidden to take, so
  redaction was unreachable on every such file. Reported by
  `pdfcer-gui` against the operator's own SolidWorks-drawing-set file,
  asked about three times; discharges the 481st filing's owed item 13
  (hybrid half).

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched). This was an
  engineering fix and a correction to a refusal's own stated
  reasoning, not a crate-boundary/library/invariant redefinition.
- `R33` ("the writer never normalizes") gains a dated clarifying note
  in the Standing Rules body: it is UPHELD, not waived, by this Pass —
  the rewrite reproduces the file's own two-part partition rather than
  collapsing it to one section.

**Findings + decisions:**
- The old refusal's doc comment gave two true reasons and reached a
  false conclusion: the "merged view" it said would need re-deriving
  was pdfcer's OWN load-time merge (`merge_first_wins`), discarding a
  fact (which objects the `/XRefStm` established) that only needed to
  be *retained*, not re-derived. Re-deriving would in fact have been
  wrong — §7.5.8.4 states what a producer MAY hide, not what this file
  DID hide.
- A corpus-harness check (`tools/roundtrip`'s `is_section_object`) was
  complete on the day it was written only because the hybrid case
  could not arise; the moment the writer stopped refusing it, every
  hybrid file's xref-stream object was misreported as an unexplained
  change. Confirmed by probing stream-object numbers before fixing.
  Generalizable — new file in `D:\dev\rag\rust\`
  (`an_enumerating_check_complete_today_goes_silently_incomplete_the_day_a_refused_case_becomes_possible.md`).
- A test fixture corrupting an offset-bearing structure (the broken
  `/XRefStm`'s `/W` array) must corrupt to the SAME total width — the
  first attempt widened `/Length` too, shifted every byte offset,
  triggered the loader's rebuild-by-scan fallback, and silently tested
  a different failure. PDF-domain finding — new lesson in
  `C:\personal_rag\pdf\`
  (`lesson_20260909_same_length_corruption_is_the_only_honest_way_to_corrupt_an_offset_bearing_fixture.md`);
  the earlier same-day lesson recording the refusal
  (`lesson_20260909_excel_365_exports_hybrid_reference_pdfs_that_refuse_a_full_rewrite.md`)
  corrected in place with a dated footer, not deleted.
- Corpus measured before/after on the private corpus (name withheld
  per the operator's standing ruling): full-rewrite per-object-verbatim
  225/237 → 237/237; hybrid refusals 12/237 → 0/237; mutation-gate
  denominator 225 → 237; raster oracle 456/456 → 468/468. No
  shortfalls either direction. End-to-end proof through the binary on
  a real hybrid file: `redact-apply` went from refusing verbatim to
  `pages_redacted=4 marks_applied=12 glyphs_removed=170`.
- **★ Redaction diligence gap measured, NOT fixed this Pass:**
  `redact::carrier_info` drops an `/Info` string containing a redacted
  run only when the run is no LONGER than the string; a longer run is
  not detected, yet the carrier report still says `action=scrubbed`.
  Observed live on the smoke-test file (`/Keywords` kept a string
  sharing the redacted word while the report read "scrubbed").
  Pre-existing; this Pass makes it reachable on more files. Filed as
  owed item 15, flagged for priority attention as a redaction-area
  finding.

**Still in flight:**
- Items 4, 5, 9, 10, 11 carried forward unchanged.
- Item 14 (second instance of the file-channel-blindness cause,
  flagged not minted) carried forward unchanged.
- Owed item 13 split: the hybrid half (13a) is discharged by this
  Pass — a reply closing the request to `pdfcer-gui` is owed but not
  written by this filing (no shell); the `/Stamp`-resize half (13b)
  remains queued and unstarted.
- Item 15 (new): the `carrier_info` redaction diligence gap above.
- Whether `1177221` (or `26ef381`) has been pushed or released is not
  asserted — no shell this filing.

**For next session:**
- Send the reply to `pdfcer-gui` closing the hybrid-reference request.
- Scope `resize_annotation`'s `/Stamp`-as-foreign refusal (item 13b)
  into a Pass.
- Fix the `carrier_info` redaction diligence gap (item 15) — a
  correctness gap in the redaction area, not merely a report-wording
  issue.
- Reconcile `R221`'s instance count (item 10, long-carried).
- Watch for a third instance of the file-channel-blindness cause.

**★ Amendment, 2026-09-09 (483rd filing):** `ROADMAP.md`'s owed-item-13
text (mirrored into this entry's own **Shipped** bullet above, which was
already correctly worded) said the hybrid file was "measured on the
operator's own SolidWorks sheet." That is wrong about the producer: the
file is `SW41177 MATERIAL REQUIREMENTS.pdf`, exported by
`Microsoft® Excel® for Microsoft 365` (`/Producer` and `/Creator` both),
sitting inside a SolidWorks drawing set alongside two genuinely
SolidWorks-exported sheets (`SOLIDWORKS PDF Publisher`, 2022/2024) that
are **not** hybrid and rewrite cleanly — recorded the same day in
`C:\personal_rag\pdf\lesson_20260909_excel_365_exports_hybrid_reference_pdfs_that_refuse_a_full_rewrite.md`.
**Mechanism, not just the fix:** an inbound request's description of a
file ("SolidWorks-exported") is a claim, not a fact, and this project's
own empirical corpus already held the measured answer one grep away —
the error propagated from the dispatch that filed owed item 13, which
repeated the request's wording without checking it. `ROADMAP.md` line
~527 corrected in place with this same dated note; this entry's own
Shipped bullet needed no change. The `Pass 281.0` commit message
(`1177221`) also says "the operator's own SolidWorks drawing" and is
published history that cannot be corrected — read it with this
amendment attached.

## 2026-09-09 (481st filing)

**Shipped:**
- Pass 280.0 (`26ef381`) — a verb that answers "which characters will
  this text run accept?" before the first keystroke
  (`EditSession::run_repertoire`, `pdfcer run-repertoire`), so a shell
  can grey a key instead of a caller typing a whole word and losing it
  at commit. Acceptance is decided by calling the same accepting code
  `edit_text` calls (`R221`), never a parallel description of it.
  Discharges the 480th filing's owed item 12 — `pdfcer-gui`'s standing
  ask, offered twice.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched).
- Declined to mint a standing rule for the file-channel-blindness cause
  at its second recorded instance (see below) — flagged for the
  engineer's judgment rather than decided here.

**Findings + decisions:**
- An existing gate (`route_enumeration.rs`) caught this Pass's brand
  new verb as a fourth route needing find-resolution, within the hour
  of the verb being written — the first instance of this gate catching
  code that postdates it rather than rediscovering old code. Discharged
  by resolving the find and reporting which run was resolved
  (`RunRepertoire::text`), not by exemption. Dated footer + table row
  added to `D:\dev\rag\rust\a_behaviour_test_over_an_enumerated_list_cannot_fail_on_a_route_added_later_but_a_source_scan_can.md`.
- A sabotage of the `encode_char` call on the simple-font branch
  survived for a checked, honest reason (every candidate on that branch
  can only be refused under one rule no fixture reaches) rather than
  because the call is dead — documented at the call site and in
  `docs/core-api` instead of deleted or forced red. New RAG file:
  `D:\dev\rag\rust\a_sabotaged_call_can_survive_for_an_honest_reason_document_it_as_a_third_option.md`.
- `R221` gains another instance; its true current instance count
  remains unreconciled (480th filing's owed item 10, untouched here).

**Still in flight:**
- Items 4, 5, 9, 10, 11 carried forward unchanged (see `ROADMAP.md`'s
  owed-work ledger).
- Two new inbound requests from `pdfcer-gui`, read and queued, neither
  started: a hybrid-reference file's forced full-rewrite refusal makes
  redaction unreachable on such files; `resize_annotation` refuses a
  pdfcer-authored `/Stamp` as foreign (third such family, after
  `/FreeText` and `/Text`).
- A second instance, in two days, of the file-channel-blindness cause
  (a reply asserted two requests were unanswered when they had been
  answered 67 minutes earlier) — flagged, not minted; the
  `stat`-before-replying remedy was already written down and not
  applied twice now.
- Whether `26ef381` has been pushed or released is not asserted — no
  shell this filing.

**For next session:**
- Confirm `pdfcer-gui`'s fourth outbound reply by `Glob` (carried from
  the 480th filing).
- Reconcile `R221`'s instance count before its Standing Rules body
  gains another dated note.
- Scope the two new `pdfcer-gui` requests (hybrid-reference redaction,
  `/Stamp` resize) into Passes.
- Watch for a third instance of the file-channel-blindness cause before
  deciding whether it earns a standing rule.

## 2026-09-09 (480th filing)

**Shipped:**
- Pass 279.0 (`5b8ec61`) — a font-coverage refusal's named remedy could
  lead in a circle: `format-text --set-font Helvetica` on
  `ABCDEF+Helvetica` resolved back to the very subset that had just
  refused the character, reported success, and changed nothing, so the
  repeated edit refused word-for-word. Fixed by running every candidate
  face through the same resolution and acceptance path `set_font`
  itself uses (`R221`), rather than describing it separately. Discharges
  the 479th filing's owed item 8.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched).
- Declined to guess a reconciled instance ordinal for `R221` — the
  commit calls this "the third recorded instance," but `docs/ROADMAP.md`'s
  own Standing Rules `R221` entry already shows numbers well past three,
  with a documented history of prior mis-tracking (300th filing). Filed
  as owed work rather than asserted.

**Findings + decisions:**
- Two dated footers written to `D:\dev\rag\rust\`: a 14th instance of
  the sabotage/fixture-discrimination family — the first of the day on
  an *ordinary, pre-existing, correct* regression test rather than a
  deliberate sabotage, because the test's fixture could not collide
  with the defect's precondition — and a fresh instance of the
  non-unique-string sabotage-anchor cause, where a generic Rust idiom
  (`None => true`) matched an unrelated match arm before the intended
  one.
- The named-remedy-leads-in-a-circle defect is exactly the risk
  `pdfcer-gui` flagged this morning in the abstract ("we have not seen
  that happen and are not claiming it") — it happens, and it is the
  first name on the list. Confirmed and replied to.

**Still in flight:**
- `R221`'s true current instance count is unreconciled — needs research
  before a dated note can be added to its Standing Rules body.
- `pdfcer-gui`'s fourth outbound reply is relayed, not independently
  `Glob`-confirmed this filing (unlike the 479th filing's three).
- `pdfcer-gui`'s standing ask for a pre-keystroke "which characters can
  this run accept?" verb remains open, offered twice, unanswered.
- Items 4, 5 and 9 (PROVENANCE.md backfill, the `origin/main..HEAD`
  filing-boundary note, and the "alternate route" `R247` reservation)
  carried forward unchanged.
- Whether `5b8ec61` has been pushed or released is not asserted — no
  shell this filing.

**For next session:**
- Confirm `pdfcer-gui`'s fourth reply by `Glob` against
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`.
- Reconcile `R221`'s instance count before its Standing Rules body gains
  another dated note.
- Resolve the `R247` reservation and mint (or fold) the "alternate
  route" cause properly (carried from the 479th filing).

## 2026-09-09 (479th filing)

**Shipped:**
- Pass 277.0 (`fccd6cd`) — a sticky note (`/Text`) no longer refuses a
  resize by falsely claiming pdfcer had not drawn it; the refusal is
  now permanent and correctly argued (§12.5.6.4 + §12.5.3: a `/Text`
  annotation behaves as `NoZoom`/`NoRotate`, so `/Rect` is an anchor,
  not a size, and no scale factor has anything to act on), refused as
  a class (the `/Text` rule OR a `NoZoom` flag on any subtype), with
  no override. pdfcer's own `TextAnnotSpec::Sticky` doc comment named
  the wrong anchor corner (lower-left; spec says upper-left) and is
  corrected in place.
- Pass 278.0 (`c8a6697`) — a freehand `/Ink` stroke's nodes are
  editable now, per point (`move_ink_point`/`insert_ink_point`/
  `remove_ink_point`) and per whole stroke (`replace_ink_stroke`/
  `move_ink_stroke`/`remove_ink_stroke`), overturning a refusal that
  had argued from Acrobat's own lack of per-point ink editing at any
  version — a decision, not a not-yet, per the requester's own
  framing, overturned under the standing "parity is the floor, not
  the ceiling" ruling. `reshape_annotation` still refuses `/Ink` (its
  single-index shape cannot address a stroke) but now names these
  verbs. pdfcer's own polyline-only authoring (`m`/`l`, no curve
  operators) makes the point-drag preview exact, not approximate —
  §12.5.6.13 leaves the join style implementation-dependent, so both
  readings conform.

**Decisions made this session:**
- No new decision minted (`ARCHITECTURE.md` §12 untouched). Neither
  Pass redraws a crate boundary, picks a library, or redefines an
  invariant; `Pass 278.0` overturns a capability ruling made inside a
  Pass, not an architectural decision.
- A standing-rule candidate ("a refusal must name the property that
  makes the operation impossible, not the nearest fact that happens
  to be true") was considered for `Pass 277.0` and **declined at
  n=1**, consistent with this project's practice of waiting for a
  genuine second instance before minting.
- A second candidate was drafted and then **corrected before filing**:
  what first looked like a novel n=1 shape (an alternate,
  independently-justified guard masking the removal of the guard
  under test, `Pass 277.0`) turned out on checking to be the
  **second** instance of an "alternate route" sabotage-survival cause
  already recorded in `D:\dev\rag\rust\` from `Pass 155.1`
  (2026-09-07) — there between two derivation rules, here between two
  refusal guards. Now at `n=2`, this project's own stated minting
  threshold, but not minted this filing: the next free standing-rule
  number (`R247`) is already reserved for an unrelated trigger, and
  resolving that reservation needs more time than this filing had.
  Filed as owed work.

**Findings + decisions:**
- Two PDF-domain lessons written to `C:\personal_rag\pdf\`, checked
  against the index first and confirmed not already covered:
  `/InkList`'s join style is implementation-dependent (§12.5.6.13),
  and a `NoZoom` annotation's anchor is `/Rect`'s upper-left corner,
  not lower-left (§12.5.3) — the second lesson exists because pdfcer's
  own doc comment had this backwards.
- `R225` (sabotage survives on a non-discriminating fixture) gains a
  twelfth dated instance from `Pass 278.0`: removing the *last*
  element of a list made `.get(i)` return `None` under both the
  correct and the sabotaged code, so a naive report's default answer
  happened to be right by accident. Re-pointed at index 0 instead.
  New degenerate-value-table row: the last index of a collection is a
  degenerate fixture choice for any bounds-checked/`Option`-returning
  access.
- The consuming shell's own question on `Pass 278.0` — *"is this a
  decision or a not-yet, because from here they look identical?"* —
  is recorded as a suggested convention for `docs/core-api/`'s
  refusal documentation (engineer-owned, not edited here), not minted
  as a standing rule.
- Reply debt discharged and independently confirmed: the 478th
  filing's owed item 7 (three unconfirmed outbound replies) and this
  filing's own three replies are the same three files, confirmed to
  exist by `Glob` directly against
  `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\` rather than
  relayed.

**Still in flight:**
- The `format_text --set-font` subset-resolution question promised to
  the requesting project is still unmeasured: whether `set_font`
  resolves to an existing subset-embedded resource sharing the target
  `/BaseFont` before authoring a fresh standard-14 one. If it does,
  `Pass 274.0`'s font-remedy refusal can name a face that then fails
  the `R-INV-1` subset floor. One fixture, one test; flagged, not
  built.
- The "alternate route" sabotage cause's proper standing-rule number
  is unresolved (see *Decisions*, above) — needs the `R247`
  reservation checked before minting.
- 477th filing's owed items 4 (21 of 38 `fixtures/synthetic/text/`
  files undocumented in `PROVENANCE.md`) and 5 (`origin/main..HEAD` is
  not a filing boundary once a release has been pushed) remain open,
  carried forward unchanged.
- Whether `fccd6cd`/`c8a6697` have been pushed or released is **not
  asserted** — this filing had no shell. Check
  `git rev-parse origin/main` / `git describe --tags --abbrev=0`
  directly.

**For next session:**
- Resolve the `R247` reservation and mint (or fold) the "alternate
  route" cause properly.
- Build the `format_text --set-font` subset-resolution measurement
  (owed item 8, `docs/ROADMAP.md`).
- Backfill `fixtures/synthetic/text/PROVENANCE.md` (477th filing's
  item 4, still open).


> **Entries before 2026-09-09 are in [`history/session-log-before-2026-09-09.md`](history/session-log-before-2026-09-09.md)** — verbatim, still citation-valid, still read by the filing gates.
> Moved there 2026-09-10, when this file had reached 99,597 lines.
