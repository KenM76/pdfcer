# Next-up items whose Pass had already SHIPPED (archive)

**Moved out of `docs/ROADMAP.md`'s *Next up* on 2026-09-10.** Each of these describes a Pass that has a Shipped entry with a commit hash — the item was written when the work was scoped and nobody removed it when the work landed.

**19 of 99 queue items**, which is why the queue read as longer than it was. Kept verbatim in case an item described more than the Pass that shipped: if you find live scope in one, move that part back into *Next up* as its own item rather than restoring the whole entry.

---

<!-- Pass 256.0 -->
### `Pass 256.0` — ★★★★ **EDIT TEXT ACROSS SHOW OPERATORS — a `find` may span CONSECUTIVE show operators of one text object when they share font resource, size and baseline, so a producer that writes ONE GLYPH PER SHOW OPERATOR (and ordinary `TJ`-split kerning output) becomes editable; today's single-operator contract stays a SUBSET** — filed 2026-09-05 (439th filing, `pdfcer-gui` correction 2026-09-05 evening, ask (a)), ~~*Next up*, **NOT STARTED**~~ **SHIPPED `1343f0e` (444th filing) — see top of *Shipped*; criteria 5 and 7 AMENDED below** — head of family 256 (text editing across operator boundaries)

~~**Status: NOT STARTED.**~~ **Status: SHIPPED `1343f0e`, 2026-09-05 (444th
filing).** The one engine gap the operator's first real typo
exposed. The correction's own words for the shape: *"What a caret in a line
of one-glyph-per-operator text means is 'replace this span of the visual
line', which is N operators."*

**The inbound, and the shape worth keeping.** Two files, one typo. The
morning request read `list-fonts`' `verdict=blocked-identity` column on every
original face, saw every glyph decode through `to_unicode` (384) rather than
`encoding_agl` (98), saw that his own pdfcer-written `WinAnsiEncoding` lines
edit, and concluded `Identity-H` was the wall. Retracted the same evening by
its authors, in three commands: `pdfcer-core`'s own
`fixtures/synthetic/text/composite-editable.pdf` carries the identical
verdict line and edits
(`composite_refusal_reachable.rs::an_invertible_composite_run_is_editable_end_to_end`
— `Pass 29.0` refuses a composite run ONLY when `/ToUnicode` is absent or
non-injective); `--find "n" --replace "t"` on his page 2 succeeded in
`AAAAAA+Arimo-Bold`, one of the three faces named as blocked; and the
decompressed content stream is the diagnosis — `BT /F4 28.91 Tf 1 0 0 -1
8.03125 29 Tm (\x00\x17) Tj 16.0762787 0 Td (\x00\x11) Tj 8.0310822 0 Td
(\x00\x03) Tj …` — **one two-byte code, one `Td`, one `Tj`, per letter.** His
editable lines differed from the uneditable ones in TWO ways at once (a
reversible encoding AND whole-line show operators), and the variable with a
printed verdict column took the blame. **The tell they name against
themselves:** `edit-text` answered `NoMatch`, not `R-INV-*` — a font refusal
names the font; a locational refusal was read as a font refusal because the
font had already been found guilty. (The shape `R220`(e) records — a true
symptom carrying a false mechanism — this time caught by the requester
before it reached this document.)

**What ALREADY exists — verified by the engineer on the operator's own file;
NOT built here.** Ask **(b)** — *"pin the operator that letter is in and
replace ITS text … what is missing is a request shape that names WHICH
`n`"* — is the whole-operator pin, `Pass 145.0` (`0c48bbf`), with the named
constructors `EditRequest::whole_operator(page, span, replace)` /
`EditRequest::pinned(span)` (`Pass 152.0`, `06e4c27`) and the CLI spelling
`edit-text --pin-span START:LEN` with no `--find`. Measured on
`C:\Users\Ken\OneDrive\pdfTests\apartment work - signed.pdf` (**the only
apartment-work file in that folder at filing time, by `ls`** — the request's
`apartment work.pdf` is not there; the engineer measured on the `- signed`
copy and said so), page 2, `page_objects()` text object 12 = *"Final quality
walkthrough with clien"*, 36 runs, last run `bytes = ByteSpan { start: 7250,
len: 7 }`:

```rust
let mut req = EditRequest::find_replace(1, "", "nt");
req.pinned_span = Some(last_run.bytes);   // VectorObject::Text(t).runs[i].bytes
session.edit_text(&req, &EditOptions::default())?;
// base_font BAAAAA+Arimo-Regular, advance_delta +8.03, disposition Reflow,
// followers_repositioned 0; reopened: "Final quality walkthrough with client"
```

Two disclosures the shell's sentence must carry, both already in
`EditReport::disclosures`: `followers_repositioned: 0` — with one glyph per
operator there are no in-operator followers, and the NEXT operators sit where
the producer's own `Td` put them, so the line does NOT re-space after the
insert (the new `t` may crowd what follows) — and the tagged-PDF
`/ActualText` staleness note. **That un-respaced tail is exactly what this
Pass exists to fix:** (b) is the caret-sized answer today; (a) is the
line-sized one.

**Acceptance criteria** (drafted by this role from the engineer's reply; the
engineer amends by strike-through, never by rewrite):

1. **Span rule.** A `find` may match across CONSECUTIVE show operators
   (`Tj`, `'`, `"`, and `TJ` elements) of ONE text object (`BT…ET`) when the
   operators share the same font resource (`Tf` name) and size and the same
   baseline — no intervening `Tm`/`Td`/`TD`/`T*` that changes the
   text-space y; an x-only `Td` between operators is the producer's advance
   and is permitted. The grouping rule is DOCUMENTED in the module docs —
   Acrobat's own grouping is unpublished
   (`Acrobat_Features\text_edit__in_place_editing_mechanism.md`, recorded
   GAP), and that RAG's stated `must_have` is that pdfcer document its own.
2. **Where the replacement lands.** The replacement is re-encoded — through
   the run's OWN font encoding via the existing `R-INV` ladder; nothing about
   encoding changes in this Pass — into the operator holding the match END;
   the matched glyphs in EARLIER operators are removed from their operands.
   An operator emptied by that removal is either dropped together with its
   own `Td` (its advance folded into the next operator's step) or left as an
   empty string — **the engineer decides, and the choice is disclosed in the
   module docs and the report.** Round-trip: only the text object's operators
   are rewritten; the rest of the stream is byte-identical.
3. **Followers.** Every operator AFTER the match end in the same text object
   has its `Td`/`Tm` x-step shifted by the net advance delta, the same way
   the single-operator path repositions in-operator followers today
   (`followers_repositioned`), applied across operator boundaries. The line
   re-spaces; `Reflow` is the default disposition.
4. **Report.** `EditReport` gains `operators_spanned: u64` (**1** for
   today's single-operator edits, so the field is never absent) and counts
   the moved followers in the existing `followers_repositioned`. The CLI
   summary line prints both. Rule 4: disclosed, never gated — the edit
   renders exactly as saved content renders.
5. **`NoMatch` is still the answer when the run does not span** — a font or
   size change, a baseline change, another text object, or a `find` that
   straddles an `ET`. ~~Today's contract is a strict subset of the new one:
   every edit that succeeds at `8a18e53` succeeds with the IDENTICAL saved
   bytes after this Pass (an equivalence test on saved bytes, the
   `Pass 152.0` pattern).~~
   > ★ **AMENDED at ship time by the engineer (`1343f0e`, 444th filing).**
   > The identical-bytes clause holds for `Tm`-positioned and follower-less
   > edits. It does NOT hold where `Td`-positioned followers sit after a
   > single-operator edit on the same line: those were left in place before
   > — precisely the un-respaced tail the correction observed on the pin
   > (`followers_repositioned 0`) — and are now RE-SPACED by the advance
   > delta. Keeping the tail un-respaced for a one-operator edit while
   > re-spacing it for a spanned one is an inconsistency no operator would
   > accept. The `NoMatch` half of this criterion stands and is tested.
6. **Tagged PDF unchanged.** The `/ActualText` staleness disclosure
   (`text_edit/edit.rs`, the `R72` posture) fires exactly as it does for a
   single-operator edit; a multi-operator match that crosses an `MCID`
   boundary (`BDC`/`EMC` between the operators) is REFUSED by name in this
   cut, not merged — the structure tree's granularity is the operator's, and
   two marked-content sequences cannot become one silently.
7. **Tests.** A synthetic one-glyph-per-operator fixture — generator added
   under `tools/` (the `gen-*-fixtures.py` pattern), a composite
   `Identity-H` face so the fixture is the operator's shape and not a
   simple-font stand-in — ~~proving the `pdfcer-gui` repro: `edit-text --page 1
   --find "clien" --replace "client"` succeeds, `extract-text` reads back
   `client`, `operators_spanned == 5`, and the operator after the match moved
   by the advance of `t`.~~
   > ★ **AMENDED at ship time by the engineer (`1343f0e`, 444th filing).**
   > The fixture spells its word with the three-glyph composite donor's
   > `A`/`B`/`C` (`composite-per-glyph.pdf`: `A` `B` `C` as three `Tj`, a
   > follower `C`, a second line `B`), not `"clien"`; the `pdfcer-gui` repro
   > itself was run on the operator's REAL document instead —
   > `operators_spanned=5`, `followers_repositioned=4`, `extract-text` reads
   > *"…with client"* (the *Shipped* entry). The second and third cases
   > below shipped as specified (`composite-tj-split.pdf`,
   > `composite-font-change.pdf`).

   A second case: a `TJ` array whose elements split a
   word (`[(cli) -20 (en)] TJ`) — one operator, several elements — edits with
   `operators_spanned == 1`, proving element splits and operator splits are
   both covered. A third: the same fixture with a `Tf` change mid-word →
   `NoMatch`, by name.
8. **CLI.** `edit-text` gains the capability with NO new flag. The four
   `--help` sentences that state the old contract — *"Locates `--find`
   within one show operator on `--page`"* (`crates/pdfcer-cli/src/main.rs`
   `:6002`, `:6086`, `:22704`, `:23528` at `8a18e53`) — are rewritten to
   state the widened one (consecutive operators, same font/size/baseline,
   one text object); `check-clap-help.py` and `check-cli-help-leads.py`
   green. `docs/core-api/02-editing-and-saving.md`'s `edit_text` section and
   `03-capabilities.md`'s targeting paragraph (`R220`(a): the
   capability-shaped document is the primary landing place) say the same.
9. **Acrobat parity.** Acrobat edits at the level of a heuristically
   identified RUN — consecutive glyph-showing operators grouped by font
   resource, baseline and horizontal adjacency, algorithm unpublished — and
   recalculates positioning for the remainder of that run on the line; it
   never exposes the operator boundary
   (`Acrobat_Features\text_edit__in_place_editing_mechanism.md`). This Pass
   reaches that behaviour for the one-glyph-per-operator and `TJ`-split
   shapes; pdfcer's PUBLISHED grouping rule and its `operators_spanned`
   disclosure are the exceed-Acrobat points.

**Invariants:** round-trip/minimal-diff (criterion 2 — measured by
`tools/content-identity` on the fixture); GUI-core separation (`cargo tree -p
pdfcer-core`, no change expected); wasm32 check clean; `check-string-gaps`
and `check-outcome-disclosed` green.

**Not in scope, by name:** a `find` across TEXT OBJECTS (`ET … BT`); across
a font or size change inside a line (criterion 5, refused by name); merging
marked-content sequences (criterion 6); the `/ToUnicode` partial inversion
(`Pass 256.1`, *Backlog*); a caret-scoped request shape (exists — the pin,
above); `format_text` across operators — the same seam, filed when asked:
`FormatRequest` carries the same `(find, pinned_span)` pair through
`effective_find`, so the span rule will want to live in `find_anchor`, not
in `plan_edit`.

`docs/FEATURES.md`: one *Planned* row at the top of the section (the only
*Next up* item), `[ ] [ ] [ ] [x]`.

> ★ **Status note 2026-09-06 (443rd filing):** the engineer reports this Pass
> **IN BUILD** as of this filing (relayed; no commit to cite yet — the entry
> moves when one exists). And it is no longer the only *Next up* item:
> `Pass 142.2` (below) is queued directly after it.

> ★★★★ **Status note 2026-09-06 (444th filing): SHIPPED `1343f0e`** (authored
> `2026-09-05 20:22:25 -0400`). The build record, the walked criteria, the
> two amendments and the measurement on the operator's file are the
> `Pass 256.0` entry at the top of *Shipped*. The shipped span rule is
> TIGHTER than criterion 1's draft (`Tc`/`Tw`/`Tz` and the MCID join the
> grouping keys) and the follower rule is WIDER than criterion 3's (`Rec::Td`
> with `cum`/`absorbed`, new-line compensation, the `T*` exception, 1/10 000 pt
> rounding). `Pass 142.2` (below) is now the HEAD of *Next up*.


<!-- Pass 142.2 -->
### `Pass 142.2` — ★★★ **THE FONT PRE-FLIGHT TESTS THE TEXT ABOUT TO BE TYPED, NOT ONLY THE TEXT THAT IS THERE — a CANDIDATE string beside the locator, every face's `FontAcceptance` derived against IT with the embedded-subset floor applied, so a refusal names the FIRST character the face cannot hold; and the STANDARD 14 surveyed for the same string, marked `WouldBeAdded` beside the page's own `OnPage` faces, the encoding rule staying ENGINE-side** — filed 2026-09-06 (443rd filing, `pdfcer-gui` request of 2026-09-05), *Next up* AFTER `Pass 256.0`, ~~**NOT STARTED**~~ **SHIPPED `5f9beb3` (445th filing) — see top of *Shipped*** — family 142 (the font pre-flight: `142.1` minted `preview_font_resources`; `142.0`, the embedded-donor restyle, is *Backlog*)

> ★★★★ **Status note 2026-09-06 (445th filing): SHIPPED `5f9beb3`** (authored
> `2026-09-05 20:48:28 -0400`, 8 files, `+409/−14`). The build record, the
> walked criteria, the three amendments (1, 2, 6 — struck below) and the
> measurement on the operator's file are the `Pass 142.2` entry at the top
> of *Shipped*. The verb is named exactly as proposed. Criterion 9's sweep is
> discharged: `cfb5b5c` (the `bold:` hint and its `italic:` twin) and the
> `FEATURES.md` row (this filing).

~~**Status: NOT STARTED — the HEAD of *Next up* since the 444th filing
(`Pass 256.0` shipped `1343f0e`).**~~ Sourced from
`D:/Dev/FeatureRequests/pdfce_FeatureRequests/open/request_font_preflight_tests_the_text_that_is_there_not_the_text_about_to_be_typed.md`
(2026-09-05 18:06). Severity in the requester's words: *"a narrowing, not a
blocker … the difference between a chooser that is exact and one that is
honest."*

**The operator's question, verbatim (2026-09-05):** *"if the character isn't
available in a pdf are we able to change to a different font?"* — **YES,
today.** The requester proved the whole route on his own file before writing
a word: `AAAAAA+Arimo-Bold` is an embedded `Identity-H` subset with no `€`;
`edit-text --page 2 --find "n" --replace "€"` refuses by name (exit 9, *"this
font has no glyph for '€', and pdfcer cannot add one to a font that is
already embedded"*); `format-text --page 2 --find "n" --set-font
Helvetica-Bold` then the same `edit-text` lands, and `extract-text` reads
back `4. I€terior Door Package`. Two things they record as thanks, not asks:
**`set_font` accepts a standard-14 name the page does not carry** and authors
the resource (`Pass 162.0`), so the escape hatch reaches all fourteen; and
**`set_font` scopes to the matched span** — one character re-encoded — so a
substitution can be as narrow as the character that could not be written.
What is missing is the QUERY that makes the shell's face chooser exact
instead of trial-and-error.

**The two gaps, checked in source at `bdefb09`:**

1. **The pre-flight tests the text that is there.**
   `EditSession::preview_font_resources(page_index, find, pinned_span)`
   (`crates/pdfcer-core/src/edit.rs:9848`; worker `text_edit/format.rs:3494`)
   derives every `FontAcceptance` by running `accept_font_target`
   (`format.rs:2387`) against the LOCATED text — its own doc says coverage is
   per-string (*"a face that covers `"Hello"` may not cover `"Hellö"`"*).
   Right for *"can this run be restyled into that face?"* (what `142.1` was
   built for); wrong for *"which face can hold the character I want to
   type?"*. Passing the prospective string as `find` fails to anchor
   (`font-preflight --page 2 --find "€"` → *"was not found in an editable run
   on the page"*) — a correct refusal reporting the wrong thing. **No free
   function does the job:** `InverseEncoding::has_char` (`encoding.rs:403`)
   and `CompositeEncoding::covers` (`encoding.rs:316`) are public, but neither
   applies the **embedded-subset floor** (`text_edit/edit.rs:1786–1788`: `if
   class.embedded && class.subset` → `Refused { trigger:
   RInvTrigger::TargetAbsent, character: Some(u), .. }`), so `has_char() ==
   true` does not imply the edit is accepted. `preview_font_resources` is the
   only query that applies both gates, and it is welded to an anchor lookup.
2. **The survey cannot reach a face the page does not carry.**
   `survey_page_fonts` (`format.rs:3122`) walks `/Resources /Font`. So the
   standard-14 half of the shell's chooser is OFFERED UNTESTED, and the shell
   says so at its call site rather than re-derive the encoding rule — which
   is exactly what `R221` forbids it to do, honoured. The pre-flight's own
   bold line says the same about itself: *"`--set-font` with a standard-14
   bold name … is NOT surveyed by this check"* (`Pass 179.1`, `2c93f6a`). A
   route that works is described in prose, in a footnote, because the survey
   cannot reach it.

**Acceptance criteria** (drafted by this role from the engineer's
disposition; the engineer amends by strike-through, never by rewrite):

1. **A `candidate` parameter, as a NEW verb beside the old.** Shape:
   `EditSession::preview_font_resources_for(page_index, find, pinned_span,
   candidate: &str) -> Result<FontPreflight, FormatError>` (the requester's
   proposed name, recorded; the engineer names it). `find` / `pinned_span`
   remain the LOCATOR, resolved through `effective_find` exactly as today
   (`Pass 147.0`). **`preview_font_resources` itself is UNCHANGED** — ~~its JSON
   is byte-identical for every existing call (an equivalence test, the
   `Pass 147.0` pattern), and `font-preflight` without the new flag prints
   what it prints today~~ **AMENDED at ship (445th filing):** its VERDICTS
   are identical (tested, `empty.entries == old.entries`) but `FontPreflight`
   gained `candidate: None` and `standard_14` (the fourteen tested against
   the located text), so its `--json` carries two new keys and the plain
   `font-preflight` prints a `standard-14` block it did not print before.
2. **Every `FontAcceptance` derived against `candidate`.** For each face on
   the page, `accept_font_target` runs against `candidate`, INCLUDING the
   embedded-subset floor gate (`edit.rs:1786–1788`) — so
   `FontAcceptance::Refused { character, .. }` names the FIRST character the
   face cannot hold, in the words the edit itself would refuse with. ~~An empty
   `candidate` is REFUSED BY NAME — `R221`'s fourth instance (`Pass 147.0`)
   was a coverage test over zero characters that reported every face
   `Accepted`; this verb does not get to repeat it.~~ **AMENDED at ship
   (445th filing):** an empty `candidate` means *the located text* and is
   identical to the old query (tested). The `R221` vacuity cannot recur by
   this route — the empty candidate resolves to the located text, and an
   empty located text with no pin is already refused by `effective_find`.
3. **The standard 14 in the survey.** `Std14::ALL` (`fontdata/mod.rs:230`,
   `[Std14; 14]`) coverage-tested against the same `candidate`, each entry
   carrying a `presence` marker — **`OnPage`** (already a resource of this
   page, in whatever form, surveyed as today) vs **`WouldBeAdded`** (authored
   on demand by `set_font`, `Pass 162.0`). The encoding rule lives
   ENGINE-side and the shell never re-derives it (the requester's `R221`
   point): the `/WinAnsiEncoding` pdfcer binds for the twelve Latin faces
   (`Pass 162.0` — `Euro` at `0o200`, `fontdata/mod.rs:97`,
   `PDF_Spec/fonts/font__std14_widths__helvetica.md`), and the built-in
   `FontSpecific` encodings of `Symbol` and `ZapfDingbats` (§9.6.6.1). A
   standard-14 face the page already carries appears ONCE, as `OnPage`, never
   twice.
4. **Doc.** The verb's doc comment states what `candidate` is, that `find`
   still locates, and that the page-face half and the standard-14 half are
   ONE list distinguished by `presence`; `docs/core-api/`'s pre-flight prose
   says the same (`R220`(a) — the capability-shaped document is the primary
   landing place). `check-public-fns-documented.py` green.
5. **CLI.** `pdfcer font-preflight --candidate TEXT` (with `--find` /
   `--pin-span` / `--page` as today) prints BOTH halves — the page's faces,
   then the fourteen — each face with its verdict and, for the fourteen,
   `on-page` vs `would-be-added`; `--json` carries the same fields. Exit is
   OK whatever the verdicts are (a page where every face refuses is an
   answer, `142.1`'s rule). `check-clap-help.py`, `check-cli-help-leads.py`,
   `check-string-gaps.sh`, `check-outcome-disclosed.py` green.
6. **Tests on `fixtures/synthetic`.** A `€` candidate against an
   embedded-SUBSET face → `Refused { character: '€' }` for that face; the same
   `€` against `Helvetica` → `Accepted`, `WouldBeAdded` when the page lacks it
   (`Euro` at WinAnsi `0o200` — cite Annex D in the test); ~~`Symbol` and
   `ZapfDingbats` REFUSE a Latin candidate by name~~ **AMENDED at ship (445th
   filing): `Symbol` ACCEPTS `€` — its built-in encoding carries `Euro` — and
   `ZapfDingbats` REFUSES it; the test asserts the truth, not the draft;** a face both on the page
   and in the fourteen is listed once as `OnPage`; the no-`candidate` path's
   ~~JSON is byte-identical to~~ VERDICTS are identical to
   `preview_font_resources`' (criterion 1, as amended). The once-only
   `OnPage` listing is not on this fixture (it carries no standard-14 face);
   measured on the operator's file instead.
7. **Rule 4.** A QUERY — nothing on the canvas changes; disclosed, never
   gated. The refusal words the shell receives are the edit's own, so a
   greyed row and the refusal it would have produced cannot disagree.
8. **Acrobat parity: NOT SOURCED.** The Acrobat RAG records the
   coverage-failure behaviour of a face change as an explicit unconfirmed GAP
   (`Acrobat_Features\text_edit__font_family_style_change_on_format.md`) and
   Acrobat's edit gate as *"the font must be installed locally"*
   (`text_edit__font_handling_on_edit.md`). A pre-typing coverage query over
   the page's faces AND the standard 14 has no recorded Acrobat counterpart;
   this is pdfcer's own route. `FEATURES.md` Acrobat column `?`.
9. **The hard-rule-11 sweep THIS Pass owes when it ships** (named now so it
   is not rediscovered): the `font-preflight` bold line's *"NOT surveyed by
   this check"* (`crates/pdfcer-cli/src/main.rs`, `Pass 179.1`) becomes
   half-true — the fourteen ARE surveyed for a `--candidate` — and
   `docs/FEATURES.md`'s *Implemented* pre-flight row's *"scoped to faces ON
   THIS PAGE … NOT a sound routing answer for a Bold button"* narrows the
   same way. Both are to be reworded, not deleted; `Pass 179.0`'s routing
   ladder is still a separate, unbuilt thing. **→ DISCHARGED (445th filing):
   the CLI sentence AND its `italic:` twin by `cfb5b5c` (engineer, 20:51);
   the `FEATURES.md` row by the filing.**

**Invariants:** GUI-core separation (`cargo tree -p pdfcer-core`, no change
expected); round-trip untouched (a query, `&self`); wasm32 check clean.

**Not in scope, by name:** a fifth `RefusalKind` bucket (recorded as a dated
NOTE on the `Pass 249.0` *Shipped* entry, not minted — the consumer says it
is not blocked and `RefusalKind` is deliberately exhaustive, so a new bucket
breaks every consumer's match); any change to `preview_font_resources`'
contract; coverage-testing an INSTALLED system font or a `--font-dir` donor
(`Pass 142.0`'s territory, *Backlog*); the automatic bold ladder
(`Pass 179.0`).

**Requester's priority, recorded:** *"We would take (a) first if you are
choosing … (b) removes an asterisk from a list that is otherwise correct."*
Minted as ONE Pass because both halves are one query and one CLI flag; the
engineer may ship (a) before (b) inside it and say so.

`docs/FEATURES.md`: one *Planned* row directly under `Pass 256.0`'s,
`[ ] [ ] [ ] ?` — Acrobat `?` because the RAG records a GAP, not an absence.


<!-- Pass 257.0 -->
### `Pass 257.0` — ★★★★ **SESSION VERBS PLAN AGAINST THE SESSION GRAPH, NOT THE BASE REVISION — `edit_text`, `format_text`, `preview_font_resources`/`_for`, `preview_style_resolution` and every other verb that reaches `plan_edit`/`plan_edit_target`/`plan_format`/`plan_format_target`/`preview_*` resolve fonts, resources and form XObjects through `self.view()`/`self.graph()`, so a `/Font` object `format_text` authored THIS SESSION is visible to the next `edit_text` on the same run instead of being "unresolvable"; the `Walk` that decodes the stream resolves the same way, so the unpinned voice stops saying `NoMatch` about text that is on the page** — filed 2026-09-06 (446th filing, `pdfcer-gui` request of 2026-09-05 20:43, ACK posted 2026-09-06), *Next up*, ~~**IN BUILD**~~ **SHIPPED `5e95805` (447th filing) — see top of *Shipped*** — head of family 257 (session-graph resolution; a correctness class over session verbs, so neither `256.x` nor `142.x`)

~~**Status: IN BUILD as of this filing** (the engineer's ACK on the channel:
*"ACCEPTED — in build now; Pass ID in the next ROADMAP filing"*; no commit to
cite yet — the entry moves when one exists).~~

> ★★★★ **STATUS 2026-09-06 (447th filing): SHIPPED `5e95805`** *"fix(core):
> text-edit planners resolve through the SESSION VIEW, not the base (Pass
> 257.0)"* — one commit after the mint. The *Shipped* entry at the top of the
> file carries the walk of the nine criteria; the amendments are struck and
> annotated in place below (5, 6, 7). Kept here for the origin, the
> requester's three measurements and the call-site census, which the
> *Shipped* entry cites rather than repeats.

**Origin.**
`D:/Dev/FeatureRequests/pdfce_FeatureRequests/open/request_edit_text_resolves_font_names_against_the_base_revision.md`
(10,066 B, 2026-09-05 20:43; read in full by this role and by the engineer).
Against `pdfcer-core` `v0.40.0` `03f6004`, re-read unchanged at `1c1d4c4`.
Severity in the requester's words: *"it makes the only remedy pdfcer has for a
subset-font refusal unreachable inside a session. No data is at risk — the
refusal happens before any mutation, as rule 4 requires."* ACK:
`open/reply_2026-09-06-base-revision-font-resolution-ACK-and-plan.md`.

**The defect, in the requester's one sentence:** `EditSession::format_text`
may create a new `/Font` object and correctly binds it against `self.graph()`
— *"not `self.base`"*, as its own comment says — but `EditSession::edit_text`
then plans with `plan_edit(&self.base, …)`, and `resolve_font_dict`
dereferences the run's `Tf` name THROUGH THAT BASE DOCUMENT. The object
exists only in the overlay, so the deref answers `None` and the edit is
refused. **The stream read is the session's (`current_page_content`, which is
what makes five sequential edits accumulate) and only the object graph that
the names inside it are resolved through is the base's** — two halves of one
read disagreeing about which revision they describe.

**Three measurements (theirs, all asserted in `pdfcer-gui`'s
`crates/pdfcer-gui/src/canvas/textedit/facewall.rs`, which goes RED the day
this ships — the engineer's ACK asks them to let it):**
1. One session, `subset-simple-embedded.pdf` (their `fixtures/subset-font-floor.pdf`
   is a byte copy): `edit_text` `ABC → ABCq` → `R-INV-1` (the subset has no
   `q`); `format_text` `FontSelector::new("Helvetica")` onto `ABC` → OK
   (authors a new `/Font`); `edit_text` `ABC → ABCq` again → REFUSED — pinned
   (`whole_operator` with a span re-measured from a fresh extraction after
   the swap): `Unsupported("the run's font resource is unresolvable in the
   target stream's resources")`; by `find`: `NoMatch("ABC")` — *"text to edit
   ("ABC") was not found in an editable run on the page"*, the UNTRUE one
   (locating by text decodes every show operator, decoding needs the font,
   the font will not resolve).
2. Control: the same pair with `to_incremental_bytes` + `Document::from_bytes`
   between the verbs SUCCEEDS, and `extract-text` reads the `q` back.
3. Bound: a face swap to a `/Font` the file ALREADY carries is editable at
   once, same session, no save — so the trigger is the NEWLY CREATED object,
   not the swap.

**Where it is (theirs, MEASURED here on the working tree at `56dde4d` by
`grep -n self.base crates/pdfcer-core/src/edit.rs`):** `plan_edit(&self.base,
…)` `edit.rs:9265`; `plan_edit_target(&self.base, …)` `:9366`;
`plan_format(&self.base, …)` `:9548`; `plan_format_target(&self.base, …)`
`:9658`; `preview_style_resolution(&self.base, …)` `:9801`;
`preview_font_resources(&self.base, …)` `:9875`; `&self.base` inside
`preview_font_resources_for` `:9922`; `plan_reflow_from_doc(&self.base, …)`
`:10040`. `text_edit/edit.rs`: `resolve_font_dict(doc: &Document, …)` derefs
`/Font` and the named entry through `doc.resolve`; `plan_edit_target` turns
`None` into the `Unsupported` sentence. `format_text`'s own
`bind_font_resource(&self.graph(), …)` is the correct sibling and the comment
that states the rule.

**Why a class, not one broken verb (theirs, accepted by the engineer):**
*"a session verb allocates an indirect object, and a later session verb
resolves a name that points at it"* — every planner reached through
`&self.base` has it. `reflow_block` already refuses this class by name
(*"Save and reopen to reflow after an in-session edit of the same page"*,
`edit.rs:9947`); `preview_font_resources` surveys the wrong revision after a
swap (not yet seen to bite).

**Shape chosen: the requester's (1).** The planners and the decoding `Walk`
take the session's view, fixing every consumer in one move rather than
teaching `resolve_font_dict` alone (their (2)); their (3), a named
`EditError` variant, is NOT taken — the refusal is removed, not renamed.

**Acceptance criteria:**
1. `edit_text`, `format_text`, `preview_font_resources`,
   `preview_font_resources_for`, `preview_style_resolution` and every other
   session verb that reaches `plan_edit` / `plan_edit_target` / `plan_format`
   / `plan_format_target` / `preview_*` resolve object references — fonts,
   resources, form XObjects — through `self.view()` / `self.graph()`, never
   `&self.base`.
2. The `pdfcer-gui` repro passes IN ONE SESSION: `format_text`
   (`FontSelector::new("Helvetica")` onto the `ABC` run of
   `fixtures/synthetic/text/subset-simple-embedded.pdf`, which creates a new
   `/Font` object) then `edit_text` `ABC → ABCq` SUCCEEDS — both by `find`
   and by a re-measured pin.
3. The control (save + reopen between the verbs) still succeeds and yields
   the same text.
4. A face swap to a font the page ALREADY carries stays editable in-session
   (unchanged behaviour — their measurement 3).
5. `reflow_block`'s *"save and reopen to reflow after an in-session edit"*
   refusal is RE-EXAMINED: if the same root cause, it is removed in this
   Pass; if not, its retention is recorded here with the reason. ★ Read
   before deciding: its rustdoc (`edit.rs:9941–9950`) gives a DIFFERENT
   mechanism — the reflow *"extracts + recognises the page fresh, needing
   provenance the staging buffer does not carry"*, so *"the base-relative
   byte offsets would not match the staged content"* — a provenance-offset
   mismatch, not a name-resolution miss, though it is planned through
   `plan_reflow_from_doc(&self.base, …)` (`:10040`) like the others. The
   criterion is satisfied either way; what it forbids is leaving the refusal
   in place unexamined.
   **→ 447th filing: MET, resolved toward REMOVAL.** The provenance-offset
   mismatch was downstream of the same root: with `plan_reflow_from_doc`
   reading the view (`pages_in`, `extract_page_view`,
   `trailer_entry(b"Encrypt")`) the offsets match the staged content, so
   BOTH refusals — content already edited this session (T-14) and page set
   changed this session (`Pass 186.0`) — are removed, and their tests flipped
   to composition tests (`edit.rs:47156`, `session_overlay_skew.rs:287`). The
   `Pass 251.0` refusal (a run APPENDED this session in `contents[1..]`; the
   plan re-emits `contents[0]` only) is RETAINED, reason recorded.
6. No `&self.base` remains as a PLANNER argument in `edit.rs` — ~~grep-
   assertable; a source-scan test in `tests/route_enumeration.rs`'s style is
   the acceptance instrument~~ (other `self.base` reads — the trailer's
   `/Encrypt` check at `:9908`, and the like — are not planner arguments and
   are not in scope). **→ 447th filing: MET, instrument AMENDED.** No
   source-scan test was written: every planner now takes `&DocumentView<'_>`
   and there is no `&Document → &DocumentView` coercion, so the engineer's
   sabotage (`preview_font_resources(&self.view(), …)` back to `&self.base`)
   is a COMPILE ERROR. A type that refuses the wrong argument is the stronger
   assertion; the grep remains available if a planner ever takes a
   `&Document` again. Residue measured at `6e2c439` (`grep -n '&self\.base'
   edit.rs`, ten lines): `document()` `:7690`; `writer::save_*` `:8745`,
   `:8770`, `:8862`, `:9054`, `:9129`, `:9175`; `signature::census` `:10350`;
   a rustdoc mention `:13975`; `page_tree::page_slots` `:18693` (the diff's
   BEFORE side) — none a planner.
7. The free functions `text_edit::edit_text(&Document, …)` /
   ~~`format_text(&Document, …)`~~ keep their signatures (they pass `&doc.view()`
   internally). **→ 447th filing: MET, naming corrected** — the free function
   is `set_format` (`format.rs:1296`); `format_text` is only the session verb.
   Every one-shot free function keeps `&Document`: `edit_text`
   (`text_edit/edit.rs:1549`), `set_format`, `apply_reflow`
   (`reflow_apply.rs:281`), `write_incremental`/`_with`/`_form`/`_form_with`,
   `add_text` (`addtext.rs:620`).
8. Tests: the three `pdfcer-gui` measurements as tests in
   `crates/pdfcer-core/tests/`; existing suites unchanged
   (`composite_refusal_reachable.rs`, `tounicode_partial_inverse.rs`,
   `font_preflight_candidate.rs` and the rest still green).
9. Channel: a SHIPPED reply naming the hash, so `pdfcer-gui` can delete the
   save-and-reopen sentence it shows today (*"pdfcer cannot type into a font
   it has just added to a file until that file has been saved and opened
   again"*) and let `facewall.rs` go red on purpose.

**Invariants:** round-trip/minimal-diff — resolving through the graph reads
the same objects the writer will emit, so the two cannot disagree; GUI-core
separation — no dependency change expected (`cargo tree -p pdfcer-core`
unchanged); rule 4 — a refusal is REMOVED, nothing is inferred.

**Not in scope, by name:** a named `EditError` variant for the old refusal
(their (3) — moot once the refusal is gone); `pdfcer-gui`'s consumption
(theirs); the automatic bold ladder (`Pass 179.0`); the embedded-donor
restyle (`Pass 142.0`).

**Acrobat parity:** none to match — Acrobat has no "base revision" a
session verb could resolve against; this is pdfcer's own session model
(`ARCHITECTURE.md` §11.1, the commit point is Save) being made
self-consistent.

`docs/FEATURES.md`: one *Planned* row at the TOP of the section (the head of
*Next up*), `[ ] [ ] [ ] —`.

<details><summary>Original <code>Pass 10.7</code> / <code>10.8</code> / <code>10.9</code> <em>Next up</em> entries (kept for the record — superseded by the <em>Shipped</em> block at the top of the file)</summary>


<!-- Pass 10.7 -->
### `Pass 10.7` — **PKCS#12 IMPORT + THE `Signer` SEAM — `Pkcs12Signer`: parse a `.pfx`/`.p12` (RFC 7292), VERIFY THE MAC FIRST as the password check, decrypt BOTH encryption eras, pair key↔leaf, order the chain leaf→CA; and the hash-in / signature-out `Signer` trait every later key source implements** — filed 2026-09-05 (436th filing), *Next up*, ~~**NOT STARTED**~~ **SHIPPED `7734261` (438th filing) — see top of *Shipped*** — first Pass of the signing arc (decision 136); ~~gated on the crate-stack decision~~ (cleared by decision 137 at the 437th filing; see *Dependency posture*)

**Origin.** The *Digital signatures* bucket's SIGN half, unscheduled since
the 396th filing split verification off as `Pass 10.1` (*"signing needs a
key source, a certificate source, a PAdES-level decision and a signing-time
`/ByteRange` patch in the writer, and is not scheduled"*). All four of those
are now decided or sourced: the key source is a `.pfx` (this Pass), the
certificate source is the same container, the level is B-B (`Pass 10.8`),
the patch is `Pass 10.9`. The operator approved the shape 2026-09-05
(recorded by the engineer in `docs/NEXT_SESSION.md` at `26f0257`, §2
*"Digital signing — the large arc ← BUILD SECOND (operator approved the
shape)"*; the only verbatim operator words on record are the batch ruling
quoted in the blockquote above — the shape itself is the engineer's
proposal, approved, not the operator's dictation).

**★ Why a `.pfx` first, and why the trait exists from the first commit.**
`D:\Dev\Rag-Specialized\Acrobat_Features\signatures__digital_id_sources.md`
(created 2026-09-05) sorts Acrobat's four digital-ID source classes by ONE
fact — *whether the raw private key is ever available to the signing
application*: **(a) a PKCS#12 file — yes, the app decrypts the container and
signs in-process, "the only source class where an application-level
implementer legitimately holds the raw private key in memory"**; (b) a
Windows-certificate-store entry — only if the key was provisioned
exportable; otherwise Windows signs via `CryptSignHash`/`NCryptSignHash` and
hands back a blob; (c) a roaming/cloud (CSC) identity — the TSP's HSM signs,
key never local; (d) a PKCS#11 token/smart card/HSM — the device signs, the
module is a bridge. Three of four are *"send a hash, receive a signature"*.
So `pdfcer-core` defines the primitive in that shape — a **`Signer`
trait** — and `Pkcs12Signer` is its FIRST IMPLEMENTATION, not a special case
generalised later (the RAG's own `must_have` design constraint, adopted).
B-B from a `.pfx` is the only fully self-contained build: no network, no OS
key store, no device — which is why it is first.

**Sourcing for every criterion below** (project rule 1 — read them before
writing a byte): `D:\Dev\Rag-Specialized\PDF_Spec\security\security__pkcs12_import.md`
(`P12-0`…`P12-14`, dated 2026-09-05), with `security__cms_signeddata_build.md`
`CB-5` for the trait's algorithm obligation.

**Acceptance criteria.**

1. **The `Signer` trait, in `pdfcer-core` (a `sign` module — working name;
   the engineer names it).** `sign(digest) -> signature` takes the
   ALREADY-COMPUTED digest, never the message — the CNG `NCryptSignHash` /
   PKCS#11 `C_Sign` shape, so `Pass 10.10`'s custodial impls fit without a
   second pipeline. `certificate_chain()` returns DER certificates leaf
   first. **Librarian's design note, sourced from `CB-5`, not a decision:**
   the trait must also NAME the algorithm its bytes are — RSA PKCS#1 v1.5 /
   RSASSA-PSS / ECDSA, curve, digest OID — because the CMS
   `SignerInfo.signatureAlgorithm` has to agree with the signature bytes,
   and for PKCS#1 v1.5 the `DigestInfo` wrap (EMSA-PKCS1-v1_5, RFC 8017
   §9.2) happens INSIDE the signer (a raw hash goes in). A trait that returns
   bytes without saying what they are cannot be wrapped correctly by
   `Pass 10.8`.
2. **PFX v3** (`P12-1`). Password-integrity mode is the target
   (`authSafe.contentType = data`, `macData` present); public-key-integrity
   mode (`signedData`, no `macData`) is REFUSED BY NAME (`P12-2` — rare; no
   fixture has it).
3. **Verify the MAC FIRST, as the password check** (`P12-2`; Appendix A HMAC
   with the Appendix B KDF, `id = 3`). Password in the KDF's BMPString /
   UTF-16BE null-terminated form for the MAC and the PKCS#12-KDF bags; raw
   bytes per PKCS#5 for PBES2 bags (`P12-11` — the wrong encoding looks
   exactly like a wrong password; try both before reporting one). A missing
   `macData` is handled (`P12-12`). **Wrong password → a NAMED refusal
   variant**, never a generic parse error and never a message that says
   which byte failed.
4. **Unwrap `authSafe` → `AuthenticatedSafe` = `SEQUENCE OF ContentInfo`,
   accepting BER** (`P12-14` — indefinite lengths live here; this is the
   opposite posture from `CB-8`'s DER-only signed attributes, so the ASN.1
   reader needs a BER mode, or a second reader). Walk `data` and
   `EncryptedData` ContentInfos both (`P12-3`); `EnvelopedData` refused by
   name.
5. **BOTH encryption eras decrypt** (`P12-9`/`P12-10`): **modern** PBES2
   (`id-PBES2`) / PBKDF2 / AES-CBC, **and legacy** `pbeWithSHAAnd3-KeyTripleDES-CBC`
   (key bag) + `pbeWithSHAAnd40BitRC2-CBC` (cert bags) under the SHA-1
   PKCS#12 KDF — *"the single most common legacy shape"*. The RC4 PBE
   variants (`pbeWithSHAAnd128BitRC4` …) are refused by name — rare, not in
   the fixtures, never silently fallen through.
6. **`pkcs8ShroudedKeyBag` → `EncryptedPrivateKeyInfo` → `PrivateKeyInfo`**
   (`P12-7`, RFC 5958): RSA (`rsaEncryption`) and EC (`id-ecPublicKey`,
   named curves **P-256 and P-384** — the two `Pass 10.1` verifies). Every
   other key type (P-521, Brainpool, Ed25519, DSA) refused by name. An
   unshrouded `keyBag` is accepted (rare, legal).
7. **Pair key ↔ leaf via `localKeyId`, falling back to a public-key match**
   (`P12-6`/`P12-8`); collect every `certBag`/`x509Certificate`; **order the
   chain leaf → CA**. A store with a key and no matching certificate, or
   certificates and no key, is refused by name (it is not a signing
   identity).
8. **Secrecy.** The decrypted key and the password are never logged, never
   persisted, never placed in an error string or a `Debug` impl; zeroized on
   drop (`security__pkcs12_import.md` §7).
9. **Large iteration counts are not an error** (`P12-13`) — no timing
   heuristic, no "this is taking too long" refusal.

**Fixtures — ALREADY ON DISK, commit `e6c0271` (this filing names it;
`fixtures/` is a `check-commits-filed` code prefix).** `fixtures/synthetic/signing/`:
`rsa2048-modern.pfx` (2,776 B; PBES2/PBKDF2/AES-256-CBC, MAC SHA-256),
`rsa2048-legacy.pfx` (2,634 B; **the SAME key + cert** under
`pbeWithSHAAnd3-KeyTripleDES-CBC` / `pbeWithSHAAnd40BitRC2-CBC`, MAC SHA-1),
`ecp256-modern.pfx` (1,299 B; EC P-256, PBES2/AES-256), `rsa2048.cer`
(957 B) and `ecp256.cer` (571 B) as DER leaves for chain-equality assertions,
`rsa2048.key.der` (1,217 B) and `ecp256.key.der` (138 B) as plaintext PKCS#8
for the **OpenSSL oracle only — never loaded through pdfcer**, and
`PROVENANCE.md`. Password `pdfcer` for every container (ASCII, so `P12-11`
has one answer). Category (a) synthetic under `LEGAL.md` §5, minted by
`tools/gen-signing-fixtures.py` (176 lines) with OpenSSL 1.1.1s — an
INDEPENDENT producer, which is what makes the files an oracle for the
importer rather than a mirror of it (pdfcer has no PKCS#12 writer, `P12-0`).
Eras verified with `openssl pkcs12 -info` at generation. Keys are NOT
deterministic across regeneration (`openssl req -newkey`), so no test may
assert a signature VALUE — round trips, chain equality and refusals only
(`PROVENANCE.md` says so). **Tests owed by this Pass:** (a) modern and legacy
RSA containers yield byte-identical `PrivateKeyInfo` and leaf; (b) each
store's extracted leaf equals its `.cer` byte-for-byte; (c) wrong password →
the named refusal, for both eras; (d) the EC store yields an ECDSA-capable
signer; (e) a sabotage test — a container whose `localKeyId` is stripped
still pairs by public key.

**★★ Dependency posture — THE CRATE STACK IS NOT DECIDED IN THIS FILING,
AND ITS RECORD IS OWED BEFORE THE `Cargo.toml` CHANGE IS COMMITTED.**
Decision 129 forbids reusing the in-crate, verify-only, NOT-constant-time
`crypto/bignum.rs` / `crypto/ecdsa.rs` with a private key — a
constant-time signing primitive is REQUIRED. The engineer's survey
`docs/signing-crate-survey.md` (dated 2026-09-05) EXISTS on disk at filing
time as an UNTRACKED file (measured: `git status --short` → `??`), and the
working tree ALSO carries an uncommitted `crates/pdfcer-core/Cargo.toml`
(+ `Cargo.lock`, +406 lines) adopting the survey's "LEAN" stack behind a
`signing` Cargo feature, default ON — `rsa 0.10.0-rc.18`, `p256`/`p384`
`0.14`, `signature 3.0`, `rand_core 0.10`, and the PKCS#12 plumbing
`sha1`/`hmac`/`pbkdf2`/`des`/`rc2` (`git diff crates/pdfcer-core/Cargo.toml`,
read by this role). **Project rule 13 puts the decision record + the
`docs/PRIOR_ART.md` rows BEFORE any `Cargo.toml` change; the change is
uncommitted, so the order can still be honoured — author decision `137`
(claimed by name in this filing; the ledger gate already counts it) and amend `PRIOR_ART.md`'s `rsa` / `cms` / `x509-cert` /
`num-bigint` rows in the SAME commit as, or before, that `Cargo.toml`.** This
filing neither mints it nor stages it (the filing commit stages the four
docs files by name). What the record must carry, per the survey's own scope
and the standing rows: the Marvin advisory (`RUSTSEC-2023-0071`) addressed
explicitly for the SIGNING path (the `rsa` row already narrows it to
private-key operations); `rsa`'s pre-1.0 (release-candidate) status as a
watch item; the wasm32 posture (RSA blinding needs an RNG; ECDSA via RFC
6979 does not); and whether RustCrypto `cms` BUILDS `SignedData` or the
in-crate DER writer does (`Pass 10.8`'s question). All named candidates are
MIT OR Apache-2.0 — no operator licence flag is needed (rule 13's copyleft
clause is not engaged); `THIRD_PARTY_LICENSES.md` regenerates with the
dependency change.

**★ DECIDED 2026-09-05 (437th filing) — decision 137, `ARCHITECTURE.md`
§12; this Pass is no longer gated.** The stack is the survey's LEAN one,
exactly as the paragraph above listed it, behind a default-ON `signing`
feature. Every item the paragraph said the record must carry, it carries:
**Marvin** — RUSTSEC-2023-0071 is open against every `rsa` version with no
patched release and is ACCEPTED for signing because the residual channel
is the PKCS#1 v1.5 DECRYPTION de-padding oracle (signing has no de-padding
step), the modexp channel closed 2026-01-07 on `crypto-bigint 0.7`, pdfcer
signs only through the blinded `Randomized*` paths, and the advisory's own
workaround names local systems safe; re-checked at every `rsa` bump;
`signing` OFF removes it from the tree. **Pre-1.0** — `rsa` is the one
pre-release pin (`pkcs1 0.8.0-rc.4` rides with it); every ECDSA crate is
the Jul-2026 stable wave. **wasm32** — `rsa/getrandom` OFF (it drags
`getrandom 0.4`, which fails on wasm32), so RSA signing refuses there with
`SignError::RandomUnavailable` as encryption authoring does; ECDSA (RFC
6979) signs on wasm32; `cargo check --target wasm32-unknown-unknown` of
core+render clean (measured by the engineer and re-measured by the
librarian). **`cms` vs in-crate** — in-crate: `cms`'s `builder` does not
compile against today's dependencies (11 errors, native and wasm32);
`sign/der_out.rs` (262 lines, X.690 §11.6 `SET OF` ordering, tested
against `asn1.rs`) writes the DER and `der 0.8` — transitive anyway — is
not used directly. **PKCS#12** — in-house on `asn1.rs` (`sign/pkcs12.rs`),
KDF from RFC 7292 B.2 verified against the `pkcs12` crate's published
vector; `pkcs12`/`pkcs5`/`pkcs8[encryption]`/`p12-keystore`/`p12` not
taken, each for a measured reason (no decryption, no PBES1, wasm32 fail,
stale generation). `PRIOR_ART.md` rows amended in the same filing;
`THIRD_PARTY_LICENSES.md` regenerated (+1,108/−42). The `Cargo.toml`
change was STILL uncommitted at the 437th filing, so rule 13's order
holds. Two things the librarian measured that the engineer should read:
`lib.rs` gates the whole `sign` module (trait included), so a `Pass 10.10`
shell signer needs `signing` ON; and `p12-keystore`'s `pbes1.rs` is NOT
credited in `sign/` — the survey asked for a credit IF structure was
borrowed, so the engineer states which.

**Disclosure (rule 4/11).** Nothing here is inferred; the CLI prints the
loaded identity — certificate subject, serial, key algorithm, chain length —
and never the password.

**Invariants to verify at ship:** `cargo tree -p pdfcer-core` gains no GUI,
network or OS-key-store crate (rule 2; decision 061's engine row); the
`--no-default-features` build (signing OFF) still compiles and still
VERIFIES signatures — the engineer's `Cargo.toml` comment says verification
is deliberately NOT gated, and a test should hold that true.

**`docs/FEATURES.md`:** one *Planned* row, placed at the TOP of *Planned*
(it is *Next up*), `[ ]` core · `[ ]` cli · `[ ]` gui · `Acrobat [x]`.

**Not in scope, by name:** writing a `.pfx` (import only, `P12-0`); any other
key source (`Pass 10.10`); the CMS build (`Pass 10.8`); the PDF write
(`Pass 10.9`); cloud/CSC identities (architecturally covered by the trait,
deliberately unscheduled — the Acrobat RAG's `nice_to_have`).


<!-- Pass 10.8 -->
### `Pass 10.8` — **BUILD THE CMS `SignedData` — CAdES, PAdES B-B: `version 1`, detached `id-data`, signed attributes content-type + message-digest + `signing-certificate-v2` (SHA-256), NO signing-time, the `0x31` retag over a DER-SORTED `SET OF`, one signer, the full chain, `crls` omitted** — filed 2026-09-05 (436th filing), *Next up*, ~~**NOT STARTED**~~ **SHIPPED `7734261` (438th filing) — see top of *Shipped*** — depends on `Pass 10.7` (a `Signer`)

**Scope.** Given (a) the digest over the `/ByteRange` spans, (b) a `Signer`,
(c) options — produce the DER `ContentInfo { id-signedData, SignedData }`
bytes for the `/Contents` hole. The bytes only; where they go is `Pass 10.9`.
pdfcer already PARSES and VERIFIES this object (`Pass 10.1`: `asn1.rs`,
`cms.rs`) — this is the BUILD direction, new.

**★ The "`cms` crate or in-crate writer?" question this Pass carried is
ANSWERED — decision 137 (2026-09-05, 437th filing): IN-CRATE.** RustCrypto
`cms`'s `builder` feature does not compile against the current dependency
resolution (11 errors on both `0.3.0-pre.1` and `-pre.2`, native and
wasm32 — `docs/signing-crate-survey.md` §0 finding 1, §5), and its
types-only half is a pre-release pin for ~150 lines of structs. The DER
encoder is `crates/pdfcer-core/src/sign/der_out.rs` (262 lines: definite
lengths, minimal INTEGER, **X.690 §11.6 `SET OF` ordering** — criterion 5's
sort — tested against the `asn1.rs` reader); `der 0.8` is in the tree
transitively and deliberately NOT used, so no foreign type crosses a `pub`
signature. Criterion 8's `openssl cms -verify` oracle and the
insertion-order sabotage test are what catch a §11.6 mistake a lenient
reader would not — the survey's own risk 3 — so they are load-bearing,
not optional. Working-tree note, measured: `sign/cms_build.rs` (204 lines)
already exists at the 437th filing; this Pass ships when the engineer says
so, against the criteria above.

**Sourcing:** `D:\Dev\Rag-Specialized\PDF_Spec\security\security__cms_signeddata_build.md`
(`CB-1`…`CB-11`) and `pades\pades__ref__creation_by_level.md` (`PC-1`…`PC-4`),
both dated 2026-09-05; Acrobat defaults from
`D:\Dev\Rag-Specialized\Acrobat_Features\signatures__signing_defaults_and_limits.md`.

**Acceptance criteria.**

1. **`SignedData.version = 1`** (`CB-1` — `eContentType = id-data` and
   `sid = issuerAndSerialNumber`, so the RFC 5652 §5.1 ladder lands on 1).
   `digestAlgorithms = { id-sha256 }`, one entry, equal to
   `SignerInfo.digestAlgorithm`. `certificates` = the FULL chain from
   `Signer::certificate_chain()` (PAdES requirement a); **`crls` OMITTED**
   (`CB-1` note, `CB-11` — B-LT revocation lives in the PDF-level `/DSS`,
   `Pass 10.6`, never here).
2. **Detached:** `encapContentInfo = { id-data, eContent ABSENT }` (`CB-2`).
3. **`SignerInfo` v1, `sid = issuerAndSerialNumber`** of the leaf (`CB-3` —
   the interoperable choice; `subjectKeyIdentifier` is not authored).
4. **Signed attributes — exactly these, DER-sorted (`CB-3`, `CB-6`):**
   `content-type` = `id-data`; `message-digest` = the `/ByteRange` digest as
   an OCTET STRING inside the attribute's `SET OF AttributeValue` (`CB-9` —
   the raw digest is never what is signed); **`signing-certificate-v2`**
   (RFC 5035, OID …16.2.47) with `certs[0]` = the leaf, `certHash` = SHA-256
   over the ENTIRE DER certificate, `hashAlgorithm` per the ASN.1 DEFAULT
   (SHA-256) — **`CB-7`: RFC 5035's prose sentence saying SHA-1 is a v1
   residue; follow the ASN.1**, and emit `hashAlgorithm` explicitly whenever
   it is not SHA-256. **NO `signing-time` attribute** (`PC-3` — PAdES
   `shall not be present`; the claimed time is the PDF `/M`). The same
   omission applies to the `adbe.pkcs7.detached` option — ONE attribute
   set, one code path, `/M` carries the claim in both. `signing-certificate`
   v1 (SHA-1) is never authored.
5. **The exact bytes signed — the `0x31` retag (`CB-4`):** the signature is
   over `DER(SET OF Attribute)` with the UNIVERSAL `SET` tag `0x31`, elements
   sorted ascending by their full DER encoding; the WIRE `signedAttrs` is the
   same length + content under the `[0] IMPLICIT` tag `0xA0`. The `Signer`
   receives `H(DER-with-0x31)`; for RSA PKCS#1 v1.5 the `DigestInfo` wrap is
   the signer's (criterion 1 of `Pass 10.7`) — `CB-5`'s one-shot-vs-low-level
   gotcha is decided ONCE, at the trait boundary.
6. **Algorithms (`CB-5`):** RSA **PKCS#1 v1.5 DEFAULT** — Acrobat parity:
   PSS is opt-in there (`bEnableRSAPSSSigning`, default OFF), and the
   `pdfcer verify-signatures` corpus is v1.5-heavy; **RSASSA-PSS as an
   OPTION** with RFC 4055 params (hash, MGF1, salt length) carried in
   `signatureAlgorithm`; **ECDSA** `ecdsa-with-SHA256` / `-SHA384`, the
   signature OCTET STRING wrapping DER `SEQUENCE { r, s }`. **SHA-256
   default; SHA-1 and MD5 are never authored** (PAdES §6.2.1 prohibits MD5;
   Acrobat has defaulted to SHA-256 since 9.1 and the RAG's `must_have` says
   SHA-1 need not be offered at all). SHA-384/512 as options if the engineer
   wants them; not required.
7. **DER throughout, definite lengths only** (`CB-8`); **exactly one
   `SignerInfo`** (`CB-10`).
8. **Oracle, BOTH directions, over all three fixture stores:** pdfcer's own
   `signature::verify` (it already knows the retag —
   `iso32000__ref__signature_verification.md`) must return integrity
   `Verified`; **and** `openssl cms -verify -binary -inform DER -content
   <spans> -CAfile fixtures/synthetic/signing/rsa2048.cer` (resp.
   `ecp256.cer`) must accept — an independent producer's verifier, so a
   shared misreading cannot pass. **Sabotage tests, each caught by exactly
   one named test:** insertion-order (unsorted) attributes → REJECTED by the
   verifier, proving the DER sort is load-bearing; `0xA0` used as the signed
   tag → rejected; a present `signing-time` → a test asserts its ABSENCE.
9. **Size reporting:** the builder exposes the blob length so `Pass 10.9` can
   size the hole (`SC-6`); the recommended measure is a probe build with the
   real chain and a dummy digest.

**Not in scope, by name:** the `id-aa-timeStampToken` unsigned attribute
(`Pass 10.11`); `crls`; multiple signers; the deprecated `adbe.pkcs7.sha1`
and `adbe.x509.rsa_sha1` encodings (`SC-1` — never authored).

**`docs/FEATURES.md`:** one *Planned* row at the top of *Planned*, all
pdfcer columns `[ ]`, `Acrobat [x]`.


<!-- Pass 10.9 -->
### `Pass 10.9` — **THE PDF-LEVEL SIGNING WRITE — an `EditSession` signing verb + `pdfcer sign`: a `/FT /Sig` field with `/SigFlags 3`, the Table 252 dictionary with `/SubFilter /ETSI.CAdES.detached` BY DEFAULT and `/M` REQUIRED, the two-pass `/Contents` hole, ALWAYS an incremental update, SELF-VERIFIED before returning; encrypted / certified / second-certification refusals BY NAME; the PAdES level PRINTED** — filed 2026-09-05 (436th filing), *Next up*, ~~**NOT STARTED**~~ **SHIPPED `7734261` (438th filing) — see top of *Shipped*** — depends on `Pass 10.7` + `Pass 10.8`

**Sourcing:** `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__ref__signature_creation.md`
(`SC-1`…`SC-8`, dated 2026-09-05) for the write; `pades__ref__creation_by_level.md`
(`PC-2`, `PC-3`, `PC-12`); Acrobat behaviour from
`signatures__signing_operation_options.md`, `signatures__format_and_level_choices.md`
and `signatures__signing_defaults_and_limits.md` (all 2026-09-05). The CLI
stub: `crates/pdfcer-cli/src/main.rs` ~line 1821, clap variant
`Sign { input, --cert, --output }`, doc string `[not yet implemented]`,
dispatched to `unimplemented_stub("sign")` at ~8948 — fill it, do not add a
second verb.

**Acceptance criteria.**

1. **Signature field + widget (`SC-4`).** A merged field/widget:
   `/FT /Sig`, `/T` unique in the document, `/V` → the signature dictionary,
   `/Subtype /Widget`, `/P` the page, `/F` print-only. **INVISIBLE by
   default: `/Rect [0 0 0 0]`, no `/AP`** — the batch/CLI case. Optional
   VISIBLE placement (`--visible x0,y0,x1,y1 --page P`) with a MINIMAL `/AP
   /N` (plain text: subject + `/M`) — the Acrobat-parity appearance composer
   (Name / Date / Location / Reason / DN toggles, graphic, watermark;
   `signatures__signing_operation_options.md`) is NOT this Pass (appearance
   is *"cosmetic, not evidentiary"*). Signing INTO an existing empty
   `/FT /Sig` field by name (`--field-name`) is supported; visible/invisible
   is per-signature, never document-wide.
2. **`/AcroForm`** created if absent; the widget in `/Fields`; **`/SigFlags
   3`** (`SC-5` — `SignaturesExist` + `AppendOnly`, the machine-readable
   R36). The page's `/Annots` gains the widget.
3. **The signature dictionary (Table 252; `SC-1`, `PC-2`):** `/Type /Sig`,
   `/Filter /Adobe.PPKLite`, **`/SubFilter /ETSI.CAdES.detached` DEFAULT**,
   `adbe.pkcs7.detached` as the option (`--format pkcs7`). **pdfcer's default
   DIVERGES from Acrobat's out-of-the-box default (legacy PKCS#7,
   `aSignFormat`; "CAdES-Equivalent" is opt-in there) — by design**, the
   Acrobat RAG's own `DISCHARGED [2026-09-05]` note and the standing memory
   rule (parity is a floor): pdfcer has no installed base to stay compatible
   with, and a fresh Acrobat's output *"is not conformant with the PAdES
   baseline profile"*. **`/M` REQUIRED** (`PC-3`), a PDF date string,
   CALLER-SUPPLIED — pdfcer reads no clock; **the CLI MAY derive it from the
   system clock when `--signing-time` is absent, AND PRINTS THAT IT DID**
   (`m_source=system-clock`) — rule 4/11, and the exceed-Acrobat opportunity
   both Acrobat RAG files name (Acrobat's no-TSA, clock-derived time is
   visually indistinguishable from a TSA time). **NO `/Cert`** (`PB-N1`).
   `/Name`, `/Reason`, `/Location`, `/ContactInfo` optional pass-throughs.
   `/ByteRange` exactly four integers.
4. **The two-pass hole (`SC-2`).** Reserve `L` hex characters (default
   ~16 KB; `--reserve`), `/Contents <000…0>` zero-filled; a FIXED-WIDTH
   `/ByteRange` placeholder; serialise the incremental update; locate the
   hole — **the `<` and `>` delimiters are INSIDE the gap** (the fixture
   defect `Pass 10.1` found); `/ByteRange [0 a b EOF−b]` with `len2`
   reaching EOF (`SC-3` — the ISO 32000-2 endpoint rule; a short range
   leaves an unsigned tail); digest span 1 ‖ span 2; CMS via `Pass 10.8`;
   hex-encode; **assert `2N ≤ L`, else REFUSE BY NAME stating both sizes —
   NEVER shrink the hole** (`SC-6`); zero-pad; back-patch in place — **no
   byte outside `(a, b)` changes** (a test compares the file before and after
   the patch outside the hole).
5. **ALWAYS an incremental update (`SC-7`, R36).** A full-rewrite save
   requested together with signing is refused by name. A document loaded via
   xref recovery forces a full rewrite (R67), so the writer already refuses
   incremental there — the signing verb SURFACES that refusal, it does not
   add a new one.
6. **`/Contents` is never encrypted and never line-wrapped (`SC-8`, §7.6.1)**
   — on an encrypted document the encoder skips the signature dictionary's
   `/Contents` string.
7. **Refusals, by name, sourced from Acrobat's own behaviour
   (`signatures__signing_defaults_and_limits.md`):**
   - **Encrypted document — PERMISSION-BIT-GATED, NOT BLANKET.** Signing is
     permitted when the document opens AND the encryption dictionary's
     permission bits allow it: signing INTO an existing field needs the
     fill-in-forms permission (the Table 22 bit that covers *"fill in
     existing interactive form fields (including signature fields)"* — cite
     the bit number from `PDF_Spec\iso32000\iso32000__s__7.6.*` at build
     time, it is not asserted here); CREATING a new signature field
     additionally needs the modify permission — a TWO-permission operation
     the refusal message must NAME (the RAG's `should_have`: *"name the
     actual constraint, not a generic 'cannot sign'"*). Any claim that
     encrypted PDFs cannot be signed is an oversimplification — refuse on the
     bits, not on `/Encrypt`.
   - **Certified document.** `/DocMDP /P 1` forbids ANY further signature;
     `/P 2` and `/P 3` permit a further APPROVAL signature only. Reuse the
     READ-side classification (`SignatureImpact`, `Pass 3.2`) — one
     permission vocabulary, not a second.
   - **A SECOND CERTIFYING signature is refused regardless of `/P`** — one
     certification per document, ever (Acrobat treats it as a hard,
     level-independent ceiling). Certification must also be the FIRST
     signature: `--certify` on a document that already carries any
     signature is refused by name (`SC-6` gotcha list; §12.8).
   - **Hole too small** (criterion 4). **Xref-recovered document**
     (criterion 5).
8. **Certifying vs approval.** Default is an APPROVAL signature. `--certify
   --mdp 1|2|3` writes `/Reference [<< /TransformMethod /DocMDP
   /TransformParams << /P n /V /1.2 >> >>]` and the catalog `/Perms /DocMDP`
   pointing back — the three-level ladder is FIXED (no composable bits;
   Acrobat RAG). `/FieldMDP` lock dictionaries are a later `should_have`,
   not here.
9. **Self-verify before returning.** After the back-patch, run
   `signature::verify` on the OUTPUT bytes: integrity must be `Verified`,
   coverage complete, the prior signatures (if any) still `Verified`. Anything
   else → the write is REFUSED, the output is not left on disk, and the
   failure is named — the verifier is the oracle (it already knows the
   retag).
10. **A second signature on an already-signed document** appends after the
    first, its `/ByteRange` reaching the new EOF; the first still verifies
    (test on a pyHanko-signed `Pass 10.1` fixture and on pdfcer's own).
11. **CLI surface — fill the stub.** `pdfcer sign <in> --cert <pfx>
    [--password <pw> | prompt, never echoed, never an env-var default]
    [--format cades|pkcs7] [--reason … --location … --contact … --name …]
    [--field-name F | --visible x0,y0,x1,y1 --page P] [--signing-time D:…]
    [--certify --mdp n] [--reserve N] [--dry-run] -o <out>`. **Stdout, one
    line, `key=value` (the project's own convention):** `sign … subfilter=
    level=B-B signer="<subject>" serial= alg= m=<D:…>
    m_source=caller|system-clock field= rect= byte_range=[…]
    contents_reserved= contents_used= mode=incremental verified=yes -> out`.
    **`level=` is the level ACTUALLY produced** (`PC-12`, rule 11) — always
    `B-B` in this Pass; `Pass 10.11` adds `B-T`. Exit codes per the existing
    ladder; a refusal exits non-zero with its name.
12. **Rule 4.** Nothing is inferred except a system-clock `/M` when the
    caller supplied none — and that is printed. The GUI half (the separate
    `pdfcer-gui`) discloses the same off-canvas; nothing is drawn on the page
    beyond the signature's own appearance.

**Invariants at ship:** `cargo tree -p pdfcer-core` unchanged in kind
(rule 2); round-trip — every object the signing verb did not touch is
omitted from the incremental section (rule 3; `tools/content-identity`
reports 0 content-stream changes for `sign`); wasm32 check of core + render
green (the engineer's `Cargo.toml` note says RSA signing REFUSES on wasm32
for want of an RNG while ECDSA does not — that refusal must be by name).

**`docs/FEATURES.md`:** one *Planned* row at the top of *Planned*, all
pdfcer columns `[ ]`, `Acrobat [x]`. **Sweep owed at ship (hard rule 11):**
`README.md` line 52 *"pdfcer verifies them but does not yet sign"* — TRUE at
this filing, FALSE when this Pass ships; the CLI stub's `[not yet
implemented]` doc string; `docs/core-api/` gains the verb.

**Not in scope, by name:** the appearance composer; `/FieldMDP`; timestamps
(`Pass 10.11`); `/DSS` / LTV (`Pass 10.6`'s dependency); any key source but
a `.pfx` (`Pass 10.10`); a `/DocTimeStamp` (B-LTA).

</details>

<details><summary>Original <code>Pass 5.4</code> <em>Next up</em> entry (kept for the record — superseded by the <em>Shipped</em> block above)</summary>


<!-- Pass 199.0 -->
### ~~`Pass 199.0`~~ — **`PCS 16.1` FAILS 15 OF 16 CELLS, AND THE BLEND ARITHMETIC IS CORRECT — THE sRGB→CMYK CONVERSION FEEDING IT IS NOT, AND THE FIX IS NOW UNBLOCKED: `iccce` ALREADY HAS THE CAPABILITY** — filed 2026-09-01 (358th filing), ~~**NOT STARTED**~~ ★ **PARTIALLY SHIPPED as `Pass 199.0`/`199.1` — SEE *Shipped*; RESIDUE SPLIT TO `Pass 199.2`, *Backlog* — 359th filing**

**★ Sourcing.** No shell available to this role this filing (hard rule 8).
The diagnosis below is **relayed from an ablation run this role did not
personally execute** — recorded as such throughout, per the dispatch's
own instruction, not independently re-run.

**★ ID assignment.** `CLAUDE.md` rule 5 makes the ID the engineer's act;
this filing's dispatch did not name one for this finding. Highest Pass ID
before this filing was `198.0` (above, *Shipped*). ⇒ **`199.0` is minted
here; ceiling `199.0`, next free `199.1` / new major `200.0`.**

#### The diagnosis

`PCS 16.1` (ICCBasedRGB blend modes) fails 15 of 16 cells. Ablation
(numbers relayed, not personally re-run by this role):

- **Control:** `find_traps` on Acrobat's own render of the same patch
  returns 2 traps — so 11 of pdfce's 12 own failures are real, 1 is
  instrument noise.
- **The backdrop is right everywhere** (≤15 levels of error); only the
  blend RESULT is wrong (up to 94 levels).
- **The failing cells share one property:** both blend operands are
  ICCBasedRGB and both cross pdfce's sRGB→CMYK bridge.
  `cmyk_bridged_pixels = 28,673` on this patch versus **0** on the
  passing DeviceCMYK sibling `PCS 16.4`, which applies the same 15 blend
  modes. That asymmetry is the diagnosis.
- **Root cause:** `crates/pdfce-render/src/cmyk_paint.rs::paint_solid_into_cmyk`
  converts authored sRGB into the group's blending space with
  `overprint::rgb_to_cmyk` — the round-trip / exactly-invertible max-GCR
  transform `cmyk_buffer.rs::snapshot_srgb_backdrop` names and documents,
  whose own doc states invertibility is the only criterion **because the
  value never reaches a screen in that form**. §11.6.6's conversion into
  the group's blending space is **terminal** — it DOES reach a screen. A
  category error, not a tuning problem: the wrong transform for the
  site, not a wrong constant inside the right one.
- **Ablation:** same binary, same synthetic page, only the transform
  varied. pdfce's transform: 92 levels of error. The file's own embedded
  profiles, at SATURATION intent: 3 levels. The naive (embedded-profile)
  arm reproduced the real patch cell-for-cell within ≤2 levels across ten
  modes, so the ablation measured the real thing, not an artefact of the
  ablation itself.
- **Three hypotheses REFUTED, each with its own ablation:** wrong
  blending space (`blends_in_wrong_space=0`; a probe confirmed native
  four-colorant blending); group/isolation/knockout handling (a
  24-combination synthetic matrix, identical output); the blend formulas
  themselves (same formulas + correct operands = 3 levels of error).

#### ★★ The ownership question is already answered — `iccce` already has the capability

An outbound feature request was drafted asking the sibling `iccce`
project to BUILD `sRGB→CMYK` conversion at a chosen rendering intent —
and reframed before filing, because a read-only survey of `D:\dev\iccce`
(v0.2.0, `HEAD` `3af2d87`) found `Chain::new(&src, &dst,
Intent::Saturation).convert(&[r, g, b])` returning four floats **today**,
at all four rendering intents, with a genuinely distinct `B2A2` table and
1.55e-4 device agreement against lcms2. Filed instead with four narrow
asks:
`D:\Dev\FeatureRequests\iccce_FeatureRequests\open\request_srgb_to_cmyk_with_an_intent_and_why_saturation_is_load_bearing.md`.

⇒ **The remaining work is pdfce's own integration, not a capability
`iccce` lacks.**

#### What pdfce owes, unblocked as of this filing

1. **Read `/RI`.** Verified by this role directly (live grep,
   `interpret.rs:2783`): `b"i" | b"ri" => {}` — a recognised no-op. pdfce
   parses the rendering-intent operator and discards it.
2. ★ **The intent is load-bearing, not a refinement.** Saturation gives
   ≤0.014 ink error; relative and perceptual give 0.02–0.68 — still
   failing. Picking the wrong intent does not merely lose precision, it
   still fails the suite.
3. **Read the PDF/X `/OutputIntent`'s `/DestOutputProfile` BYTES.**
   `iccce` has no identifier registry, by design — decision `064`'s
   boundary: pdfce owns compositing and what a colour component means,
   `iccce` owns conversion.
4. **Supply an sRGB SOURCE profile as bytes.** `iccce` has no built-in
   sRGB source.
5. ★★ **Route only the TERMINAL conversion sites — NOT a global
   search-and-replace.** `overprint::rgb_to_cmyk` MUST remain the return
   leg of `snapshot_srgb_backdrop` ↔ `composite_srgb`: mixing the
   calibrated and the invertible transforms across those legs previously
   cost a different patch **10 trap markers against a baseline of 2**
   (a regression precedent, not a hypothetical one).

#### Secondary lead — unmeasured, recorded as such

`PCS 13.0` (currently FAIL, 4 traps) reports `blend_modes_applied=0` with
`cmyk_bridged_pixels=6396` — same conversion path, no blending applied.
Plausibly free with the same fix. **Not measured this filing** — do not
treat as confirmed until re-checked after the fix lands.

#### A provenance gap, recorded honestly

The diagnostic run did **not** name the CMM behind its 3-level arm, which
matters: lcms2 forces black-point compensation for v4 perceptual/
saturation intents and `iccce` deliberately does not. The 3-level figure
may not be reproducible verbatim through `iccce` for that reason — verify
against `iccce`'s own output, not against the diagnostic's arm, before
citing 3 levels as pdfce's expected post-fix result.

#### Owed dependency — a reply pdfce owes `iccce`, not the reverse

`iccce`'s own
`request_can_you_hand_me_the_output_intent_and_an_intent.md`
(2026-08-25) has **no reply** in their `open/` folder as of this filing.
It asks exactly the `/DestOutputProfile` hand-off and per-paint `/RI`
questions items 3–4 above need settled. Outstanding pdfce-side work,
independent of whether this Pass starts first — see this filing's
`SESSION_LOG.md` entry, "For next session."

**`FEATURES.md`.** Rows for `/OutputIntents`-aware CMYK conversion,
`/ICCBased` real-profile resolution and rendering intent (`/RI`) amended
this filing — reworded from "gated on `iccce`" (a capability gap on
their side) to "gated on pdfce's own integration, `Pass 199.0`, filed
*Next up*" (the gap is now here). **No boxes ticked; nothing in this
Pass has shipped.**

> ★★★★ **`Pass 185.0` SHIPPED AND HAS LEFT THIS SECTION — `cec4069`,
> 2026-08-30 (348th filing). IT IS CLOSED AT 4 OF 4 CRITERIA DISCHARGED.**
> The entry below is kept **struck** rather than deleted, because it is the
> record of what was owed, of the eighteen-day-old RAG file that owed it, and of
> the ID decision. **Its full Shipped entry, criteria ledger, sabotage table and
> the finding above the Pass are at the top of *Shipped*.**
>
> **What shipped:** the job `ui-strings` / *"verify pdfce-gui strings live in
> ui_text.rs"* is now `audits` / **"repository audits (20 checks)"**, and
> `tools/check-ci-job-names.py` keeps the count honest — a job over 8 `run:`
> steps must declare `(N checks)`, and a declared count must be right.
> **Option (2), rename**, was taken; the split's per-gate attribution was **not**
> taken and is **accepted, not owed** — recorded in the RAG file so the next
> reader does not re-file it.
>
> ★★ **Criterion B was owed to the LIBRARIAN'S tree, not the engineer's**
> (`D:/dev/rag/rust/`, outside the repo), and is discharged **in the 348th
> filing rather than by the commit.** A Pass whose criteria span two owners does
> not close when the code lands.
>
> ★ **Still owed and un-minted, unchanged by this:** the `Pass 38.5` **C9** debt
> (`/StructParent` / `/OBJR`), and `Pass 184.0` **criterion E**.


<!-- Pass 185.0 -->
### ~~`Pass 185.0`~~ — **THE CI JOB NAMED FOR ONE OF ITS NINETEEN STEPS: SPLIT IT, OR RENAME IT TO WHAT IT HAS BECOME** — ★★★ **THIS WAS DIAGNOSED, FILED AND REMEDIED IN THE RAG ON 2026-08-12 AND THE REMEDY WAS NEVER EXECUTED; THE JOB HAS SINCE GROWN FROM 3 STEPS TO 19** — ★★ **SEVEN OBSERVED RED RUNS HAVE NOW BEEN MISATTRIBUTED IN PUBLIC, AND THE MISATTRIBUTION IMPUGNS `run-gates.sh`** — filed 2026-08-30 (345th filing), ~~**NOT STARTED**~~ ★ **SHIPPED `cec4069`, 2026-08-30 — SEE *Shipped***

**★ ID assignment.** `CLAUDE.md` rule 5 makes the ID the engineer's act; this
filing's dispatch **explicitly delegated the judgement** (*"Your call whether
it deserves a Pass"*). Highest Pass ID before this filing was `184.0`. ⇒
**`185.0` is minted here; ceiling `185.0`, next free `185.1` / new major
`186.0`.**

#### What is wrong today, measured

**Measured in this filing** from `.github/workflows/ci.yml` (`awk`) and from
GitHub (`gh run view --json jobs`):

| fact | value |
|---|---|
| job key | `ui-strings` |
| job `name:` | `verify pdfce-gui strings live in ui_text.rs` |
| **named steps in the job** | ★★ **19** |
| steps that name describes | ★★ **1** |
| gates it actually runs | `check-ui-strings.sh`, `check-disclosure-channel.sh`, `check-outcome-disclosed.py`, **`check-commits-filed.py`**, **`check-passes-filed.py`**, `check-bypass-paths.sh`, `check-core-api-verbs.py`, `check-clap-help.py`, `check-cited-verbs-exist.py`, `check-metrics-line-contract.py`, `check-ledger-numbers.py`, `check-suite-name-absent.py`, `check-cli-help-leads.py`, `check-one-commit-per-command.py`, `check-cited-commits-exist.py`, `check-settings-consumed.py`, `check-string-gaps.sh`, `check-theme-colors.sh`, `check-ci-parity.py` |

**GitHub's checks list, the PR status rollup and the commit status badge all
show JOB names and never step names.** ⇒ **Every one of those nineteen gates
fails in public under the label of the first one.**

★★ **Two red runs today, both misattributed** — `33325723019` (`cff102a`) and
`33328613196` (`5d87a5f`), both actually `check-commits-filed.py`. **Plus the
five recorded on 2026-08-12. Seven observed occurrences, zero exceptions.**

#### ★★★ Why this is a Pass and not a note

**The finding is eighteen days old and already carries its own remedy.**
`D:/dev/rag/rust/a_ci_job_name_describes_its_first_step_not_the_gate_that_failed.md`
(2026-08-12) names this job, quotes its YAML, and closes: *"**Never** leave a
multi-gate job named after one of its gates."*

⇒ **The RAG entry was correct, complete, actionable and inert.** It changed
nothing because **nothing was scheduled against it** — no Pass, no gate, no
owner. **This Pass is the scheduling.** ★ **The general shape, recorded because
this project has now hit it three times** (`oxidize-pdf`, XFA deprecation, this):
*an answer sourced in one document while another still asks the question.* **A
RAG finding with no work item behind it decays into a record of a problem.**

★ **The cost is not cosmetic.** Today it cost a diagnostic cycle **in the
expensive direction** — the engineer ran `check-ui-strings.sh` locally, got
clean, and briefly believed CI and his machine disagreed. **The instrument is
the thing a report is checked against**, so a misleading instrument does not
merely fail to help; it argues against a correct local result.

#### Acceptance criteria

> ★★★ **ALL FOUR DISCHARGED, 2026-08-30 (348th filing).** `A` by **option (2),
> rename** — the alternative (1) split was not taken and its per-gate
> attribution is **accepted, not owed**. `B` **in that filing, not by the
> commit**, because the RAG file is this role's to write. `C` **mechanised
> above the threshold** — appending a step to the job now fails
> `tools/check-ci-job-names.py`, measured; below the threshold it stays a human
> trigger by design. `D` honoured — **no standing rule minted.** Ledger and
> evidence at the top of *Shipped*.

- **A — Fix the attribution.** Either **(1) split** the unrelated gates into
  separate jobs — correct attribution in the one place everyone looks, at the
  cost of a checkout + toolchain install per job — or **(2) rename** the job to
  what it has become (`project gates`, or similar). ★ **The RAG file ranks
  split above rename and says why: rename is honest and free, but the red X
  still does not say WHICH gate.** **Either is acceptable; leaving it is not.**
- **B — If (2) is chosen, say so in the RAG file**, so the next reader does not
  re-file the split as still-owed.
- ★ **C — The trigger, which is the durable half.** *Whenever a step is
  appended to an existing job, re-read that job's `name:` and ask whether it
  still describes the whole job.* **This job went 3 → 19 steps without anyone
  asking once.**
- **D — No new standing rule.** `R209`'s gate-sweep discipline and the RAG file
  already cover the ground; what was missing was a **work item**, and this Pass
  is it.

#### ★ Scope note

**`.github/workflows/` is engineer-owned and was NOT edited by this filing** —
the misdirection was measured and filed, not fixed. **The RAG file received a
dated recurrence footer** (hard rule 4: a dated footer on the existing lesson,
never a second file).

> ★ **Both halves closed since.** `.github/workflows/ci.yml` was edited by the
> **engineer** at `cec4069`, as it had to be; the RAG file received a **second**
> dated footer — a *resolution* footer — in the 348th filing, recording that
> option (2) was chosen and that the count-gate is a **third fix category the
> 2026-08-12 list did not contain**. Still one file, still footers: writing a new
> RAG file about the inertness of a written RAG file would refute itself.


<!-- Pass 179.0 -->
### `Pass 179.0` — **BOLD BECOMES AUTOMATIC: A FALLBACK LADDER THAT BINDS A REAL FACE WHEN ONE EXISTS AND SYNTHESISES WHEN ONE DOES NOT, WITH NO OPERATOR INTERVENTION** — ★★★ **OPERATOR RULING, 2026-08-30; DECISION `106`. REVERSES THREE CLAUSES OF pdfce's OWN DOCUMENTED POSTURE AND AMENDS `R90`** — filed 2026-08-30 (340th filing), ~~**NOT STARTED**~~ ★★★★ **SHIPPED 2026-09-06, `72b7296` (451st filing) — see top of *Shipped*. Kept in place, struck-not-moved: criterion 7 AMENDED and criterion 5 gained an ordering decision at ship time; the measurement table below stays verbatim**

**The ruling, verbatim (Ken, 2026-08-30):**

> *"bold font should be automatically used if available, but otherwise
> synthetic should be supported, and the user shouldn't have to intervene."*

Given in reply to the engineer asking which of two remaining items to take
next — custom fonts (`Pass 142.0`) or the colour work. **So it is a ruling on
the font item, and it re-weights it.** The full record, including the three
overruled clauses quoted so the prior wording stays legible, is
`ARCHITECTURE.md` §12 **decision `106`**. This entry is the work.

**★ ID assignment.** `CLAUDE.md` rule 5 makes the ID the engineer's act; the
engineer's dispatch for this filing **explicitly delegated it** (*"Assign Pass
IDs and report them back to me — I will build against them"*). Highest ID
before this filing was `178.2`; **`179.0` and `179.1` are minted here, highest
ID now `179.1`, next free family `180`.** The numbering is **not** an ordering
— see `179.1`.

> **★ Carried forward, 2026-08-30 (342nd filing).** The 341st filing recorded
> a **second, differently-caused survivor set** with **no ID**, deliberately.
> The engineer assigned **`Pass 179.3`**, and `179.1` + `179.3` shipped
> together at `2c93f6a` — **both filed to *Shipped*, both discharged.**
> ⇒ **Highest Pass ID is now `179.3`; `179.2` and `179.3` are SHIPPED,
> `179.0` is NOT STARTED, and next free family is still `180`.** ★ **The
> numbering is still not an ordering, and it is now visibly so: `.1`, `.2`
> and `.3` all shipped before `.0` was begun.**

#### What is wrong today, measured

Read from live source and from the **shipped binary**
(`target/release/pdfce-cli.exe`), not from the docs:

1. **There is no "make this bold" verb.** Two separate controls:
   `--set-font <FACE>` and `--bold-synthetic`. The operator picks.
2. **`--bold-synthetic` REFUSES when a real bold face resolves**
   (`FormatError::RealFaceAvailable`, `format.rs:1099`), quoting the
   `--set-font` argument to retry with — **two calls with a refusal between
   them.**
3. **"Available" means fonts already on that page and nothing else.**
   `gate_synthesis` → `survey_page_fonts` (`format.rs:2563`, `:2997`). Branch
   1 prefers the run's own family, branch 2 accepts another family, branch 3
   finds nothing and synthesis proceeds.
4. **★ The standard-14 Bold sibling is not considered.** A page whose only
   font resource is `Helvetica` takes branch 3 and synthesises, though
   `Helvetica-Bold` needs **no embedding at all** and is already modelled
   (`fontdata::Std14::HelveticaBold`, `std14_by_base_font`).
5. `font-preflight` exists to tell an operator which of the three outcomes
   they would get — **itself the intervention step the ruling removes.**

#### ★★★ The measurement found a live defect — `Pass 179.1`, below

On `fixtures/synthetic/textedit/format_other.pdf` (one font resource,
`Helvetica`, `std14=1`), same file and same run, three commands:

| command | shipped answer |
|---|---|
| `font-preflight --find hello` | `real_bold=-`, then **`"bold: no real bold face of this run's family is accepted here — --bold-synthetic is the route"`** |
| `format-text --find hello --set-font Helvetica-Bold` | ★ **SUCCEEDS** — `set_font=Helvetica->Helvetica-Bold`, adding `/pdfceF1`, *"a standard-14 face … so no font program is embedded"* |
| `format-text --find hello --bold-synthetic` | proceeds, disclosing *"no real Bold face resolves … so pdfce **cannot make this change with a genuine typeface**"* |

⇒ **Both quoted strings are FALSE on every standard-14 page.** pdfce made the
change with a genuine typeface one command earlier. See `Pass 179.1`.

> **★★ AMENDMENT, 2026-08-30 (342nd filing) — THE TWO QUOTED STRINGS ARE
> CORRECTED ON DISK; THE TABLE ABOVE IS A DATED MEASUREMENT AND STAYS.**
> `Pass 179.1` + `Pass 179.3` shipped at `2c93f6a`. `font-preflight` now names
> **both** routes and says the standard-14 route is *"NOT surveyed by this
> check"*; `--bold-synthetic`'s disclosure no longer asserts pdfce *"cannot
> make this change with a genuine typeface"*. **The table above is left
> verbatim on purpose** — it is what the binary said on 2026-08-30 before the
> fix, and rewriting a dated measurement into agreement with today destroys
> the evidence the ruling was made on.
> **★★★ WHAT IS *NOT* FIXED, AND THIS IS THE WHOLE OF `179.0`:** the third
> row of that table is **unchanged behaviour**. `format-text --set-font
> Helvetica-Bold` still succeeds one command after `--bold-synthetic`
> synthesises on the same page, because `survey_page_fonts` still ignores the
> standard-14 siblings. **`179.1`/`179.3` corrected what pdfce SAYS; `179.0`
> is what pdfce DOES, and it is still `NOT STARTED`.**

#### The ladder — the engineer's read, not the operator's

The ruling fixes the **policy** (automatic, no intervention); the **rungs**
are pdfce's own choice:

| rung | source of the face | status |
|---|---|---|
| **1** | a real bold face already on the page | machinery **exists** — ~~currently **refuses** instead of binding~~ → ★ **corrected 2026-08-30 (342nd filing): since `Pass 179.2` it refuses only under `style_policy = refuse`; under the default `auto` it **passes the face over and names it**. Either way it does **not bind**, which is the rung that is missing** |
| **2** | ★ the **standard-14 Bold sibling of the run's OWN face** (`Helvetica`→`Helvetica-Bold`, `Times-Roman`→`Times-Bold`, `Courier`→`Courier-Bold`) | **the cheap win, and cheaper than first stated**: the *binding* half shipped in `Pass 162.0` (`std14_resource_dict` + `bind_font_resource`); only the **automatic selection** is new |
| **3** | a face supplied via `--font-dir` (decision 012) | ★★ **IS `Pass 142.0`** — see below. **Ships absent from this Pass** |
| **4** | synthetic, **disclosed off-canvas** per rule 4 | emission exists; ~~its disclosure *text* is wrong today (`179.1`)~~ → ★★ **DISCHARGED 2026-08-30 at `2c93f6a` (`Pass 179.1` + `179.3`, filed 342nd). The disclosure text is correct now; the rung itself is unchanged** |

**★★ Rung 3 is `Pass 142.0` wearing a different name.** `--font-dir` on
`format-text` supplies **non-embedded faces for rendering and measurement**
today (`main.rs:5332`); it does not put a font program **into the document**.
Binding a `--font-dir` face a run never referenced needs subsetting and
embedding (§9.6.4 ST1–ST4, deferral code **FF-C**) — exactly `142.0`'s
narrowed remainder. ⇒ **`179.0` ships with rung 3 absent and grows it when
`142.0` lands.** `142.0` is not a competitor to this Pass; it is the ladder's
rung-3 supplier.

**★ The re-weighting, stated precisely.** `142.0`'s Backlog entry carries the
consuming project's use report — *"Synthetic is enough. Drop `142.0` down the
queue."* — and that stands. The ruling **raises this automatic-ladder work
above `142.0`; it does not raise `142.0` itself.** Rung 2 delivers a **real**
bold for Helvetica / Times / Courier text with **no embedding**, which covers
a large share of the CAD and office documents this project actually sees —
i.e. much of what `142.0` was wanted for arrives without `142.0`.

#### ★ The one genuinely debatable rung, and it is OPEN

If the run is an embedded **Arial** and no Arial Bold exists, rung 2 does
**not** fire — `Helvetica-Bold` is a *different family*. Synthetic keeps the
letterforms and fakes the weight; a cross-family real bold keeps the weight
and changes the letterforms. Rung 2 is deliberately scoped to the run's **own**
family, matching `gate_synthesis`'s existing same-family-first preference.
**`pdfce-acrobat-librarian` was dispatched in parallel and did NOT close it**
(`Acrobat_Features/text_edit__synthetic_bold_italic_styles.md`, addendum
2026-08-30):

- **CLOSED — Acrobat's Bold toggle is a FONT-VARIANT SWAP, not a style flag.**
  *"'Bold' means a different font"*; *"It's not a separate setting from the
  font like in Word."* ⇒ rungs 1–3 are the shape Acrobat itself has.
- **CLOSED, and a free EXCEED — Acrobat's preference is ONE COMBINED toggle**
  for bold *and* italic. pdfce's `StyleSynthesis::{Bold, Italic, BoldItalic}`
  already has per-axis granularity, so pdfce can **bind a real Bold and
  synthesise Italic in the same operation.** Take it; record the divergence.
- **★ STILL GAP — cross-family substitution.** *"No source — Adobe or
  community — describes this branch at all."* Same-family-only is *"merely the
  more parsimonious reading … not a stated rule."* **Do not record it as
  Acrobat-confirmed.**
- **★★ STILL GAP — standard-14 siblings.** No source, any session. ⇒ **rung 2
  is NOT a parity claim** and must be argued from pdfce's own capability
  boundary (the decision-020 posture), not from Acrobat.
- Weak evidence that Acrobat's fallback-off behaviour is a **silent no-op or a
  greyed control** — the opposite of pdfce's explicit refusal. Under this
  ruling pdfce does **neither**: it acts, and it discloses.

#### Rule 4, which this looks like it collides with and does not

Decision 059 settled the shape: rule 4 mandates **non-silence**, never
**visible machinery**. ⇒ **The bound face renders exactly as saved content
will render; WHICH RUNG FIRED IS DISCLOSED OFF-CANVAS.** No accept/reject
gate, no provisional marking, no `font-preflight` round trip required of the
operator. **★ And the disclosure must name the rung**: *"Bold applied"* is
silence with a receipt — binding a real face, authoring a standard-14 sibling
into the file, and stroking the existing face are three different things to
have done to a document.

#### Acceptance criteria

1. **One verb reaches bold.** A caller asks for bold (and/or italic) **without
   knowing** whether a real face exists; `FormatError::RealFaceAvailable`
   stops being reachable from that verb. Core + CLI, same Pass (rule 11).
2. **Rung 1 binds instead of refusing** — the face `gate_synthesis` finds
   today is now applied, through the same `accept_font_target` coverage gate
   that `set_font` uses (`R221`; do not build a second acceptance test).
3. **★ Rung 2 fires**: `format_other.pdf` above, asked for bold with no face
   named, binds `Helvetica-Bold` and embeds nothing. **This is the Pass's
   discriminating fixture** — it is the exact input on which the shipped
   binary currently synthesises.
4. **Rung 4 still reachable, still spec-native** — `R90`'s emission half is
   untouched: `Tr 2`, user-space stroke width, matched stroking colour, `Tm`
   shear, never double-strike, provenance never written into the PDF.
5. **Per-axis independence is exercised** — a fixture where a real Bold binds
   and Italic is synthesised **in one operation** (the Acrobat exceed above).
6. **The disclosure names the rung**, and a test asserts the string reaches
   **stdout/stderr of the shipped binary**, not only that the core returns it
   — the `Pass 162.0` finding-1 trap (two of three save paths wired, every
   unit test green, the shell using the third).
7. **The explicit verbs survive as overrides.** `--bold-synthetic` must remain
   reachable for an operator who wants the stroke *despite* an available real
   face — the ruling removes the **obligation** to choose, not the
   **ability**. It no longer refuses; it discloses that it is overriding a
   real face.
8. `cargo fmt --check`, `cargo clippy -D warnings`, `bash tools/run-gates.sh`
   clean; `FEATURES.md` rows 149, 280 and the *Planned* ladder row updated in
   the shipping filing.


> **★★★★ SHIPPED 2026-09-06, `72b7296` (451st filing) — the eight criteria
> walked; full account at the top of *Shipped*.** 1 MET (`set_style` /
> `--bold` `--italic`; `RealFaceAvailable` unreachable from it). 2 MET (rung 1
> binds through `accept_font_target` via `plan_font`; on `format_twins.pdf`
> the `/Differences` twin is passed over BY NAME with its `R-INV-7` refusal).
> 3 MET, MEASURED on the shipped debug binary (`format_other.pdf --bold` →
> `set_font=Helvetica->Helvetica-Bold`, `rung=StandardFourteenSibling`,
> `/pdfceF1` added, nothing embedded, no `2 Tr`). 4 MET (embedded subset →
> rung 4, `2 Tr`, `synthetic_bold_width`). **5 MET with an ORDERING DECISION**
> recorded in code: a FULL standard-14 sibling (rung 2, both axes) is tried
> BEFORE a one-axis page face (rung 1, one axis) — `Times-Roman` +
> `Times-Bold` on page, bold-italic asked → `Times-BoldItalic`, nothing
> synthesised; `Verdana` + `Verdana-Bold` → real Bold at rung 1 + synthetic
> Italic in one operation. 6 MET (`format_text.rs` +4 assert the rung line and
> sentence on STDOUT of the shipped binary; exit `9` for the posture refusal).
> **7 MET, LAST SENTENCE AMENDED**: *"It no longer refuses"* was written under
> ruling 1 alone; under ruling 2 (*"make … refusing available as well"*)
> `--bold-synthetic` with a real face available still REFUSES
> (`RealFaceAvailable`) under `refuse`, proceeds-and-names under `auto`/`warn`
> (`Pass 179.2`, unchanged) — the engineer resolved the tension toward the
> later ruling; and NEW, the ladder reaching rung 4 under `refuse` returns
> `SynthesisRefusedByPosture` naming `--bold-synthetic`. 8 MET (fmt/clippy;
> `cargo test --workspace` green; non-cargo gates green individually —
> `run-gates.sh` as one process exceeds the 10-minute foreground limit; the
> `FEATURES.md` rows the criterion numbers 149/280 are `:196`/`:353` at
> `HEAD`). Cross-family substitution NOT taken; the debatable rung is resolved
> same-family-only by pdfcer's own choice, and the `Acrobat_Features` gap is
> NOT claimed closed. Rung 3 ships absent (`Pass 142.0`), as stated.
> `tests/style_ladder.rs` 8/8 run by the librarian at filing.

#### Not in this Pass, stated so it is not assumed

- **Rung 3** (`--font-dir` donor) — that is `Pass 142.0`.
- ~~**Any global preference.** The ruling removes the *need* to intervene; it
  does not ask for an Acrobat-style set-and-forget switch, and `R90`'s
  never-a-global-preference clause survives that far.~~
  **★★★ STRUCK 2026-08-30 (341st filing) — A SECOND OPERATOR RULING ARRIVED
  MINUTES AFTER THE FIRST AND ASKED FOR EXACTLY THIS**, and it **shipped** in
  `Pass 179.2` (`8671daa`) before `179.0` was started. Ken, verbatim: *"let's
  still make the current method of warning or forcing it manually or refusing
  available as well as the automatic silent one."* ⇒ `style_policy = auto |
  warn | refuse` is a **persisted global preference**, default `auto`, and
  `auto` **is** the Acrobat-style set-and-forget switch this bullet said the
  ruling did not ask for. **The bullet was true of ruling 1 and false of the
  pair.** `R90`'s never-a-global-preference clause is **narrowed** in the same
  filing — see *Standing rules*. **`179.0` now builds ON TOP of the posture
  selector**: it must make the ladder automatic **in every posture**, since a
  posture selects only what pdfce says about a fallback, never which face it
  picks.
- **`crates/pdfce-gui`** — paused (GUI-pause block); the `gui` column tracks
  `D:\dev\pdfceGUI`, which must be notified rather than built here.

---


<!-- Pass 143.0 -->
### ~~★★★★★ THE 08:31 INBOUND — A `pdfceGUI` REQUEST ARRIVED BETWEEN THE 305th FILING'S CHANNEL CHECK AND `Pass 143.0`'s COMMIT. **UNPARSED · NO PASS ID CLAIMED** — and it is `R219`'s shape again: `format_text` was made addressable by PIN ALONE and its sibling `edit_text` was not~~ — **DISCHARGED 2026-08-28 (307th filing): SCOPED AND SHIPPED AS `Pass 152.0` (`06e4c27`), THREE HOURS AFTER THIS BOX WAS OPENED — AND THE BOX'S OWN PREMISE WAS FALSE** — opened 2026-08-28 (306th filing)

> **★★★★★ DISCHARGE BANNER — READ BEFORE ACTING ON ANYTHING BELOW, BECAUSE
> THE BOX IS WRONG ABOUT THE ONE THING IT ASSERTS.**
>
> **SHIPPED as `Pass 152.0` (`06e4c27`).** The full account is the *Shipped*
> entry at the head of this file. In one screen:
>
> - **`edit_text` WAS ALREADY ADDRESSABLE BY PIN ALONE**, and had been since
>   `Pass 145.0`. The heading below — *"`format_text` was made addressable by
>   PIN ALONE and its sibling `edit_text` was not"* — is **false**, and this
>   role wrote it. `edit.rs:1643` calls `effective_find`; the behaviour is
>   pinned by `whole_operator_pin.rs::edit_text_gets_the_same_affordance`;
>   `pdfce-cli edit-text --pin-span` with no `--find` works, and that
>   subcommand's own `--find` help documents the empty case.
> - **So this was NOT `R219`'s shape.** `R219` is *a fix reached one route and
>   not its sibling*. Both routes were fixed. What differed was that one had a
>   **named constructor** and the other had a **sentence describing how to
>   spell it**.
> - **`Pass 152.0` therefore adds NO BEHAVIOUR.** It adds
>   `EditRequest::whole_operator(page_index, span, replace)` and
>   `EditRequest::pinned(span)` — a name for something that already worked —
>   plus a worked example in `docs/core-api/02-editing-and-saving.md`.
> - **Their option 2 — *honour the pin over a non-empty `find`* — was
>   DECLINED**, on the ground they flagged themselves: with a non-empty `find`
>   the pin narrows **which operator** and the find narrows **which characters
>   within it**, and that combination is real.
> - **The larger question the box raised is still open**: whether
>   `text_extract`'s synthesised inter-glyph spacing should be recoverable at
>   all. Nothing here answers it.
>
> **★★ Why this discharge is written at length rather than struck through.**
> The box was filed by this role from the requester's own framing, and the
> framing carried a **mechanism** — *"the sibling was never fixed"* — attached
> to a **real observation** — *"our pinned `edit_text` was refused"*. That is
> **`R220`(e) exactly**: the symptom is evidence, the mechanism is a
> hypothesis, and they arrived in one paragraph in one confident voice. The
> observation was true. The mechanism was wrong, and this role relayed it into
> `ROADMAP.md` **as this project's own statement about its own crate**, six
> weeks of documents deep, under a five-star heading. It was caught by the
> engineer running the CLI, not by anyone re-reading the box.

**★ NO ID IS MINTED HERE, DELIBERATELY.** Assigning a Pass ID is the
engineer's act (`CLAUDE.md` rule 5, and this role's own dispatch protocol).
This box exists so the item cannot be lost between filings, **not** to scope
it.

**Provenance and why it is at the front.** `ls -la
D:/Dev/FeatureRequests/pdfce_FeatureRequests/open/`, run by this role at
filing time:
**`request_a_pinned_edit_still_matches_on_find_and_the_find_is_extractor_prose.md`**,
from `pdfceGUI`, **2026-08-28 08:31** — *after* the 305th filing (07:29) and
*before* `4094e49` (09:06). **No reply answers it**; the newest outbound is
`reply_stroke_scaling_is_an_OPTION_operator_ruling_and_you_get_the_toggle.md`
at **05:20**. The operator's standing ruling — *"check the feature requests
and write these as the first thing to do before continuing work on other
things"* — is **still in force** (296th filing's discharge banner explicitly
kept the ruling while discharging its ordering), so this takes the front of
this section.

**The report, in the requester's own terms.** Driven on the operator's own
`SW41177.pdf`, page 1, at `(1140, 62)`, with the pin **set**:

```
pdfce-diag text-edit-caret  kind=Edit page=0 run=426 len=30
pdfce-diag edit-text-refused page=0 n=1
    detail=text to edit ("0.00                     0.030") was not found in an editable run
```

**Twenty-one spaces that are not in the content stream.** They are
`text_extract`'s inter-glyph gap filling — the machinery that turns two show
operators separated by a `Td` into one readable line — and **on a CAD drawing
that is most of the text there is.** The shell captured the run's text exactly
as the extractor reported it and handed it back as `EditRequest::find`. **It
cannot match, because nothing in the file spells it.**

**★★ AND THE REQUEST CARRIED A PIN THE WHOLE TIME.** `canvas::textedit::plan`
sets `pinned_span` from `pin::of_run` with the `EditTarget` from the same
provenance record — so the request named **an unambiguous byte span naming one
show operator**, and was refused on a string comparison against text the
**extractor synthesised**. ⇒ ***`find` and `pinned_span` are two answers to one
question, and when both are present the pin is the one with evidence behind
it.***

**★★★ THIS IS `R219`'s SHAPE, AND THE SIBLING ROUTE WAS ALREADY FIXED.** The
requester says so themselves: **`Pass 145.0` gave them
`FormatRequest::whole_operator(page, span)` — no find string at all — and they
consumed it the same night.** `format_text` and `edit_text` are **two verbs
over the same surgery**; one is addressable by pin alone and the other is not.
**A fix that reached one entry point and not its sibling taking the same
operands** is precisely what `R224`(a) says to scope by the **operand**, not by
the function being edited — and this is the **fourth** consecutive item in that
family (`145.0` → `147.0` → `148.0` → this).

**Owed at scoping time, not decided here:** whether `edit_text` gains a
pin-only form mirroring `FormatRequest::whole_operator`, or whether a present
`pinned_span` simply **outranks** `find` on the existing verb; and whether the
extractor's synthesised spacing should be recoverable at all, which is a
separate and larger question about what `text_extract` promises its consumers.

---


<!-- Pass 147.0 -->
### ~~★★★★★ `preview_style_resolution` STILL PASSES THE CALLER'S `find` STRAIGHT THROUGH — **`Pass 147.0` FIXED ONE OF THE TWO PURE QUERIES AND THE SIBLING TAKING THE SAME TWO OPERANDS WAS NEVER ASKED ABOUT** — found by the 299th filing's hard-rule-11 sweep, **UNPARSED · NO ID CLAIMED**~~ — **DISCHARGED 2026-08-28 (300th filing): SCOPED AND SHIPPED AS `Pass 148.0` (`f1a88e6`), FOUR HOURS AFTER THIS BOX WAS OPENED** — opened 2026-08-28 (299th filing)

> **★★★ DISCHARGE BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
>
> **SHIPPED as `Pass 148.0` (`f1a88e6`).** The full account is the *Shipped*
> entry at the head of this file. In one screen:
>
> - **The finding was reproduced before it was believed**, on
>   `fixtures/synthetic/textedit/format_family.pdf`. **Every particular held**,
>   including the *"worse than the pre-flight"* judgement — the empty-`find`
>   call answered `RealFaceResolves { "Times-Bold", "F3" }` where the explicit
>   one answered `{ "Calibri-Bold", "F2" }`, and `/F3` cannot show the run.
> - **The mechanical fix is the one this box proposed**: `effective_find` plus
>   the unpinned refusal in `match_run`'s existing words.
> - **The judgement this box declined to prescribe was MADE, and made this
>   way**: the unpinned refusal ships, because *"a third behaviour across five
>   entry points would be a third thing to remember"*.
> - **Acceptance criteria 1, 2 and 3 are met; 3 was EXCEEDED** — this box
>   proposed a `grep -c` equality and the Pass shipped
>   `crates/pdfce-core/tests/route_enumeration.rs`, a per-function source scan
>   with a corpus floor and a named exemption list.
> - **★ Criterion 4 is NOT met and is now owed doc work.** `f1a88e6` touches
>   **no documentation file** (`git show --stat`). `docs/core-api/02-editing-and-saving.md:339`
>   and `docs/core-api/03-capabilities.md` still say nothing about what an empty
>   `find` means here — and the function now **refuses** one, so line 339's
>   *"Pure query."* has gone from a near-miss to a real gap. **Engineer-owned.**
> - **Standing rule `R224` was minted from this box's process finding**, on the
>   revisit trigger the 299th filing set for itself.
>
> **Everything below is the finding as it stood on 2026-08-28 at 00:39 and is
> kept legible rather than edited** (hard rule 1). Its *"`effective_find` has
> exactly TWO call sites"* line carries the same denominator error corrected in
> the `Pass 147.0` entry's amendment footer: the true figure was **3 of 4**, not
> 2 of 3, because the `grep` was scoped to `format.rs` and `plan_edit` lives in
> `edit.rs`. **The defect claim was right; the ratio around it was not.**

**★ NO PASS ID IS CLAIMED HERE, DELIBERATELY.** Parsing a defect into Pass
entries is **the engineer's act** (`CLAUDE.md` rule 5; precedent: the 184th,
297th and 298th filings' inbound boxes). This box exists so the finding cannot
be lost between sessions, not to scope it. **Next free family is `148`**
(`python tools/check-ledger-numbers.py`, run this filing).

**How it was found:** not by a bug report and not by a grep for the changed
identifier — by asking hard rule 11's question, *"what else makes the same
CLAIM?"*, of a Pass that changed **what an operand means**. A grep for
`preview_font_resources` returns `preview_style_resolution` **zero** times.

---

#### THE FINDING

| field | value |
|---|---|
| function | `EditSession::preview_style_resolution(page_index, find, pinned_span, want)` |
| declared | `crates/pdfce-core/src/edit.rs:7388` → `crates/pdfce-core/src/text_edit/format.rs:2635` |
| reachable from | **a library caller** — `pdfceGUI` drives its Bold/Italic routing from this query. **No `pdfce-cli` subcommand** (`grep` over `crates/pdfce-cli/src/main.rs`: no hits) |
| commit path | **unaffected and correct** — `plan_format` resolves |
| blocking? | **Not reported by anyone yet.** It is the *same* defect `pdfceGUI` reported one function over, so a consumer following the same guidance reaches it the same way |

**`effective_find` has exactly TWO call sites** —
`format.rs:1514` (`plan_format`, the commit path) and `format.rs:3197`
(`preview_font_resources`, added by `Pass 147.0`). **There is no third.**
`preview_style_resolution` builds its locating `EditRequest` with
`find: find.to_owned()` (`format.rs:2654`) and passes the **raw** string to
`probe_synthesis` at three call sites (`2681`, `2688`, `2699`).

**Both halves of `Pass 147.0`'s defect are present, traced through source read
here:**

1. **PINNED + empty `find`.** `find_anchor` locates by the pin without reading
   `find`; `probe_synthesis` → `gate_synthesis` calls
   **`survey_page_fonts(doc, resources, recs, text)` at `format.rs:2395`** —
   the same call whose vacuity was `Pass 147.0`'s defect, at the *other* of its
   two call sites. `""` surveys every candidate as accepted, so
   `find_styled_face` returns whichever face it reaches first and the query
   answers **`RealFaceResolves` naming a face that cannot show the run** —
   precisely the state `Pass 144.0` shipped to end.
2. **UNPINNED + empty `find`.** `find_anchor` runs `s.text.contains(find)` and
   **every string contains the empty string**, so it matches the **first show
   operator on the page** and the answer is about an operator the caller never
   named.

**★★ WHY THE CONSEQUENCE IS WORSE HERE THAN IN THE PRE-FLIGHT.**
`preview_font_resources` returns a **list** a shell filters a combo box with; a
universal yes offers faces that then refuse. `preview_style_resolution` returns
a **routing decision** — `WouldSynthesize` vs `RealFaceResolves` — and a wrong
`RealFaceResolves` sends the Bold button to `set_font` **on a face that will
refuse**, i.e. **the operator gets no bold at all by either route.** That is
`R221`'s third-instance cost profile (a capability removed), not its
fourth-instance one (a richer-looking list).

---

#### WHAT THE FIX LOOKS LIKE, AND THE ONE JUDGEMENT IT NEEDS

The mechanical part is one line, and it is the **same** line: resolve with
`effective_find(anchor, find, pinned_span)` and pass the resolved string to
`probe_synthesis`, so **all three** entry points ask one function what an empty
`find` means instead of two of three. The unpinned case is then refused by the
same `match_run` sentence, exactly as `Pass 147.0` did.

**The judgement the engineer owns**, and the reason this box does not prescribe
a fix: `preview_style_resolution` has been shipped and consumed for longer than
the pre-flight, so **the empty-`find`-unpinned refusal is a behaviour change for
an existing caller**, not only a bug fix. Whether that lands in the same Pass,
and whether `pdfceGUI` is told before or after, is a scoping call.

**Acceptance criteria this box proposes** (engineer's to accept, amend or
reject):

1. `preview_style_resolution(page, "", Some(pin))` and
   `preview_style_resolution(page, real_text, Some(pin))` return **identical**
   `StyleResolution` for the same operator — the byte-identical oracle
   `Pass 147.0` used, which asserts *the two spellings mean the same thing*
   rather than *one of them improved*.
2. `preview_style_resolution(page, "", None)` is **refused by name**, with
   `match_run`'s existing sentence rather than a second spelling of it.
3. **A test that fails if a FOURTH entry point is added without resolving** —
   or, failing that, the enumeration written down: `grep -c effective_find`
   must equal the number of functions taking `(find, pinned_span)`. `R219`
   clause (e) asks for the enumeration's answers to be **measured**; this is
   the cheapest instrument for it.
4. `docs/core-api/02-editing-and-saving.md:339`'s row (currently just *"Pure
   query."*) and `docs/core-api/03-capabilities.md`'s style-routing prose say
   what an empty `find` means here — the same paragraph `Pass 147.0` added for
   the pre-flight.

---

#### ★ THE PROCESS FINDING, WHICH IS THE PART THAT GENERALISES

`Pass 147.0` **did** enumerate a sibling route and fixed both halves of it — the
pinned and unpinned calls **of the function it was fixing.** The sibling
**function** taking the same two operands went unasked **in the same commit that
was about exactly this hazard.**

⇒ ***A route enumeration scoped to the function being fixed is scoped to the
instance, not the class.*** When a Pass changes **what an operand means**, the
enumeration's unit is *"every entry point taking that operand"*, not *"every
branch inside this entry point"*. Filed as `R219` clause (e)'s first live catch
and `R221`'s fifth instance; both rules are **amended in place**, neither
re-minted. See the `Pass 147.0` *Shipped* entry's sweep section for the full
derivation.

---


<!-- Pass 142.1 -->
### ~~★★★★★ TWO MORE `pdfceGUI` FILES ARRIVED AT 22:50 AND 22:57 — **ONE OF THEM IS A LIVE DEFECT REPORT AGAINST `Pass 142.1`, SHIPPED FIVE HOURS EARLIER; BOTH ARE UNPARSED AND HAVE NO PASS ID; AND THE PAIR REFUTED A CLAIM THE 297th FILING WAS MAKING WHILE IT WAS BEING MADE**~~ — **DISCHARGED 2026-08-28 (299th filing): FILE 1 PARSED, SCOPED AND SHIPPED AS `Pass 147.0` (`8aa9cea`); FILE 2 WAS A NOTE AND OWED NOTHING** — opened 2026-08-27 (298th filing)

> **★★★ DISCHARGE BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
>
> **FILE 1 (22:57) is SHIPPED.** `Pass 147.0` (`8aa9cea`) takes **BOTH** of the
> requester's offered remedies, not one: `preview_font_resources` now calls
> **`effective_find(anchor, find, pinned_span)` — the same function
> `plan_format` calls** — so a pinned empty `find` resolves to the anchor
> operator's own characters and the two functions cannot disagree again; and an
> **unpinned** empty `find` is **refused by name**, reusing `match_run`'s
> sentence. `FontPreflight::text` reports the **resolved** string.
> `pdfce-cli font-preflight` gains `--pin-span START:LEN` with `--find`
> optional. Reply written to
> `open/reply_the_preflight_now_resolves_the_pin_and_refuses_a_bare_empty_find.md`,
> **2026-08-28 00:07**. **See the `Pass 147.0` entry at the head of *Shipped***
> for the delivery record, the sabotage result, the rule dispositions and the
> ledger.
>
> **★★ THE BOX SAID *"EITHER CLOSES IT"* OF THE TWO REMEDIES, AND THAT WAS
> WRONG — BOTH WERE NEEDED, AND THE ENGINEER SHIPPED ONLY ONE ON THE FIRST
> CUT.** Remedy (1) closes the **pinned** route; remedy (2) closes the
> **unpinned** route, where `find_anchor` runs `s.text.contains(find)` and
> **every string contains the empty string** — the same vacuous survey, about an
> operator the caller never named, reachable **by accident** rather than by
> following guidance. **The unpinned half is the MORE reachable one.** The error
> is left visible below rather than edited out, because it is the transferable
> finding: ***when a reporter offers two remedies for one symptom, check whether
> they cover different reachability ROUTES before choosing between them.***
>
> **★ THE MECHANISM BELOW IS LABELLED *"RELAYED, NOT VERIFIED HERE"* AND THAT
> LABEL IS NOW DISCHARGED, NOT STRUCK.** All four traced steps were reproduced
> in-crate by the engineer before the report was believed (`R203`), and
> **nothing in the report needed correcting.** The label was the correct
> epistemic state at filing time; striking it would erase the record of a claim
> carried honestly until it could be checked — the same treatment the 297th and
> 298th filings gave their own unverified relays.
>
> **★ THE ENGINEER-SIDE ITEM AT THE FOOT OF THIS BOX IS ALSO DONE.**
> `docs/core-api/`'s `preview_font_resources` prose now states what an empty
> `find` means with and without a pin, **and what it meant before
> `Pass 147.0`** (`git show 8aa9cea -- docs/core-api/02-editing-and-saving.md`,
> read in full by this role).
>
> **FILE 2 (22:50) needed nothing and still needs nothing.** Its four
> load-bearing contents were consumed by the 298th filing (`FEATURES.md`
> corrections, `R203`(d)) and are unaffected by this discharge.

**★ NO PASS ID IS CLAIMED HERE, DELIBERATELY.** Parsing a consumer request into
Pass entries is **the engineer's act** (`CLAUDE.md` rule 5; precedent: the 184th
and 297th filings' inbound boxes). This box exists so neither file can be lost
between sessions, not to scope them. **Next free family is `147`** (`python
tools/check-ledger-numbers.py`, run this filing).

**How they were found**, because that is the whole reason this box exists:
`ls -lt D:/Dev/FeatureRequests/pdfce_FeatureRequests/open/`, run by this role at
filing time. **Nothing in this repository can report a file outside it.**

---

#### ★★★★★ FILE 1 (22:57) — `preview_font_resources` REPORTS EVERY FONT AS ACCEPTED WHEN `find` IS EMPTY AND A SPAN IS PINNED

| field | value |
|---|---|
| file | `open/request_preview_font_resources_trusts_the_callers_find_where_format_request_now_resolves_it.md` |
| arrived | **2026-08-27 22:57** (`ls -lt`) |
| against | **`Pass 142.1`** (`2e6235c`), meeting **`Pass 145.0`** (`0c48bbf`) |
| their tip | `pdfce-core` at **`0c48bbf`** |
| blocking? | **No, and they say so by name** — their caller passes real text today |

**The finding, in one line.** `preview_font_resources(page, "", Some(pin))`
**locates correctly and reports every font on the page as `Accepted`.**

**Why anyone would call it that way: because `Pass 145.0` told them to.**
`FormatRequest::whole_operator(page, span)` ≡ `FormatRequest::new(page,
"").pinned(span)` — a pinned request no longer needs a `find`. **They consumed
it that night and deleted their per-operator `find` construction.** Feeding the
pre-flight the same way is the obvious next step, **and it is the trap.**

**Their traced mechanism — RELAYED, NOT VERIFIED HERE** (`crates/` is outside
this role's remit; `R203` cuts both ways and this is the honest label):

1. `find_anchor` with `pinned_span` set **never reads `find`** — it matches on
   `pin_names_operator` and continues past the text search. An empty `find`
   **locates the right operator, with no error.**
2. `survey_page_fonts(.., find)` passes that same empty string down.
3. `accept_font_target(.., text, ..)` tests coverage with
   `text.chars().zip(encoded.codes.iter())`.
4. **`"".chars()` yields nothing. Zero characters checked, zero refusals found.
   Every entry comes back `Accepted`.**

**★★ WHY THIS IS WORTH A PASS RATHER THAN A DOC LINE, in their words and this
project's terms.** `Pass 142.1` exists to stop a shell offering faces that
cannot work. A caller following `145.0`'s guidance gets a list where **every**
face is offered — **strictly worse than the `fontinfo` superset they deleted**,
which was at least a superset of the *page's* fonts by name, where this is an
**unconditional yes**. And **it is silent**: *"the list looks richer, not
broken. The operator picks a face, gets a refusal, and the control that was
built to prevent exactly that has become the thing producing it."*

**⇒ `R221`'s FOURTH instance, and the first found by a consumer rather than by
this project.** After `Pass 145.0`, **`FormatRequest` RESOLVES the text and
`preview_font_resources` TRUSTS the caller's** — **two functions taking the same
two operands and disagreeing about what an empty one means.** `Pass 142.1`'s own
commit message argued that the fix for `gate_synthesis` was to stop
**describing** when `set_font` succeeds and make it **call** the accepting code;
**the identical shape survived in the parameter list.**

**Two remedies offered; either closes it.**

1. **Resolve** the text from the anchor operator when `find` is empty and
   `pinned_span` is set — *"it already has the anchor two lines earlier
   (`recs.get(anchor_index)`), so the text is in hand."*
2. **Refuse** an empty `find` by name, the way `match_run` used to. **Their
   stated preference**: *"a refusal we can see beats a list of universal
   yeses."*

**★ They are carrying a workaround for it and REPORTED it rather than keeping
quiet** (decision 058, working as designed). `Reading::find` — the
longest-contiguous-glyph-stretch walk whose stated justification this project
already refuted — is **kept alive solely to feed the pre-flight**, so they are
*"carrying a mechanism we cannot fully explain in order to feed a parameter that
should not need feeding."* **The fix deletes code on their side**, which is why
it is filed as a request rather than a note.

**★★ WORK APPEARS TO BE IN FLIGHT ON THIS ALREADY, IN THE ENGINEER'S
UNCOMMITTED TREE, AS THIS BOX IS BEING WRITTEN — and the box stands anyway.**
`git status --porcelain`, re-run here at commit time, shows
`crates/pdfce-core/src/text_edit/format.rs`,
`crates/pdfce-core/tests/font_preflight.rs`,
`crates/pdfce-cli/tests/font_preflight.rs`, `crates/pdfce-cli/src/main.rs` and
`docs/core-api/02-editing-and-saving.md` all modified and **none of them by
this role**. **That the changes are THIS fix is an INFERENCE from the file
names, not a fact** — the diffs were not read here. **Uncommitted work has no
commit, no Pass ID and no acceptance criteria** (`R217`'s shape), so this box
is what stops it becoming a shipped capability with no roadmap entry — the
same reasoning the 297th filing gave for the box that became `Pass 146.0`.
**The engineer should claim a Pass ID before committing.** Nothing here is a
request to stop.

**★ Engineer's note when scoping:** whatever the fix, **`docs/core-api/`'s
`preview_font_resources` prose is part of it** — it will now be read by callers
arriving from `Pass 145.0`, and it currently says nothing about what an empty
`find` means here.

---

#### ★★ FILE 2 (22:50) — BOTH `142.1` AND `144.0` CONSUMED; A HOLE CLOSED THAT THEY HAD NOT REPORTED; THE PERFORMANCE REPORT DELIVERED

| field | value |
|---|---|
| file | `open/note_142_1_and_144_0_are_both_consumed_and_the_superset_is_deleted.md` |
| arrived | **2026-08-27 22:50** (`ls -lt`) |
| kind | **NOTE, not a request.** *"Nothing else is owed to us."* |

**Nothing here is owed engineering work.** It is filed because four of its
contents are load-bearing elsewhere:

- **★★★ IT REFUTES A CLAIM THE 297th FILING MADE WHILE IT WAS BEING MADE.**
  `preview_font_resources`' `accepted()` list *"is now the entire contents of
  both face combos — the Properties panel's and the Format ribbon's"*, and
  **`faces_on_page` is deleted, not left as a fallback** (*"a workaround whose
  cause is removed rots, and the next reader cannot tell a deliberate fallback
  from a forgotten one"* — their standing rule, and a good one). **The 297th
  filing left `gui` `[ ]` on that row, substantiated by a quote from the 21:54
  file.** `docs/FEATURES.md` is corrected by the 298th filing and **`R203` gains
  clause (d)**.
- **★★ pdfce CLOSED A HOLE THEY HAD NOT REPORTED.** Their request had described
  the `fontinfo` superset as *"usually exactly right, and when it is wrong the
  operator finds out by pressing a button and getting a refusal"* — the
  **visible** half. `base_font_ambiguous` was the **invisible** half: matching
  on the stripped `/BaseFont` meant that **on 87 % of embedding files a row
  could reach the wrong twin silently** — wrong font applied, **no refusal to
  show for it**. ⇢ Their generalisation, which belongs to both projects: **a
  superset is not a mild error when the elements are indistinguishable.**
- **★ `real_bold()` / `real_italic()` are DELIBERATELY not consumed**, on
  `R221`'s reasoning applied from their side: with `Pass 144.0` in, their
  existing route (ask for synthesis, take the face the refusal names) reaches
  the right answer, and *"adding a second routing rule while the first one works
  would be two descriptions of one decision."* **A deliberate non-consumption,
  not a gap** — recorded so a later filing does not read it as owed work.
- **★ The promised performance report, delivered and self-critical.** The
  pre-flight *"costs nothing measurable, because it shares the extraction"*: on
  `SW41177` (5,903 objects, a real title block) a fourteen-run Bold gesture ran
  **1,075 ms before** and **1,083 ms after** — inside the noise. True only
  because they caught a defect of their own first: the first draft called their
  `inspect()` — a full extraction with provenance capture, **392 ms** — while
  its only caller *had just run one*. **Two extractions per selection change to
  answer two halves of one question.** ⇢ *"A helper that fetches what it needs
  reads better and hides that somebody upstream fetched it already."*
- **They also refused something, and the refusal is `R9` applied correctly.**
  Refused pre-flight entries are **absent, not greyed with their reason** — *"a
  combo of twelve faces with nine greyed rows each carrying a per-character
  explanation is a control an operator cannot read"*; the empty case says so in
  one line. **pdfce's per-character sentence is still the right thing to
  produce**; it is simply not what a combo box should render.

---

#### ★★ THE PROCESS FINDING THIS PAIR PRODUCES — n = 4, AND IT IS NO LONGER ONLY ABOUT ARRIVAL

**Fourth consecutive session in which the channel changed underneath a check
that had already run.** The 296th, 297th and now the 298th filings each recorded
the same shape about their predecessor: **a channel check is a TIMESTAMP, not a
STATE.**

**★ What is new at n = 4, and it is worse than arrival.** In the previous three
instances the newer file merely **arrived**. Here it **refuted a claim the
filing was making at the time** — and the refutation was **in the directory
listing the filing had just run**. ⇢ ***A directory listing is not a reading.
`ls -lt` tells you a file exists; opening the one above the one you came for is
a separate act.*** Disposition: **`R203` amended in place, clause (d)** — see
*Standing rules*. No new rule; the decline is argued there.

---


<!-- Pass 146.0 -->
### ~~★★★★★ THE 21:54 INBOUND — A FOURTH `pdfceGUI` REQUEST ARRIVED AFTER THE 296th FILING'S CHANNEL CHECK AND BEFORE THIS SESSION'S REPLIES WENT OUT. IT IS UNPARSED, IT HAS NO PASS ID, AND NO PDFCE GATE WILL EVER NOTICE IT~~ — **DISCHARGED 2026-08-27 (298th filing): PARSED, SCOPED AND SHIPPED AS `Pass 146.0` (`9f6e732`)** — opened 2026-08-27 (297th filing)

> **★★★ DISCHARGE BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
>
> **The request in this box is SHIPPED.** `Pass 146.0` (`9f6e732`) delivers
> the requester's options **(1)+(2)** — `forms::Widget::border:
> Option<BorderSpec>`, `forms::Widget::visibility: Option<Visibility>` and
> `forms::Widget::annot_flags: AnnotFlags`, populated by `parse_acroform` —
> plus `pdfce-cli list-fields --widgets`. **Option (3), the separate
> `widget_properties()` query, was NOT taken**, on the requester's own
> argument that `caption` is already modelled in the parsed struct. Reply
> written to
> `open/reply_widget_border_and_visibility_are_readable_now.md`, 2026-08-27
> 23:13. **See the `Pass 146.0` entry at the head of *Shipped*** for the
> delivery record, the reading rules, the sabotage result and the ledger.
>
> **★ THE FOUR CLAIMS BELOW ARE LABELLED *"Not verified here"* AND THAT
> LABEL IS LEFT STANDING ON PURPOSE.** All four were subsequently **checked
> against the tree by the engineer and all four HELD** (the fourth with a
> refinement — the one site touching `/BS /S` was a dropped-property
> *detector*, which maps it to a WARNING and never back to a `BorderStyle`).
> The label was the correct epistemic state at filing time, and striking it
> would erase the record of a claim carried honestly until it could be
> checked — the same treatment the 297th filing gave `Pass 145.0`'s
> UNVERIFIED multi-operator claim.
>
> **★★ ONE THING IN THIS BOX IS NOW KNOWN TO HAVE BEEN WRONG WHEN WRITTEN,
> AND IT IS NOT ONE OF THE FOUR CLAIMS.** The final section — *"AND ONE
> THING BACK ON `Pass 142.1`"* — reports, quoting the 21:54 file, that
> `pdfceGUI` had **not yet consumed** `preview_font_resources`. **Two newer
> files in the same directory say otherwise**: `note_142_1_and_144_0_are_-
> both_consumed_and_the_superset_is_deleted.md` (**22:50**) and
> `request_preview_font_resources_trusts_the_callers_find_where_format_-
> request_now_resolves_it.md` (**22:57**). **Both `Pass 142.1` and
> `Pass 145.0` were consumed the night they shipped**, and
> `docs/FEATURES.md` is corrected accordingly by the 298th filing. **The
> inbound performance report promised at the end of this box has also
> arrived and is answered** (1,075 ms → 1,083 ms, inside the noise). This
> is the instance behind **`R203` clause (d)**; see *Standing rules*.


**★ NO PASS ID IS CLAIMED HERE, DELIBERATELY.** Parsing an operator or
consumer request into Pass entries is **the engineer's act** (`CLAUDE.md`
rule 5, and the 184th filing's inbound-box precedent). This box exists so the
request cannot be lost between sessions, not to scope it. **Next free family
is `146`** (`python tools/check-ledger-numbers.py`, run this filing).

**How it was found, because that is the whole reason this box exists.**
`ls -lt D:/Dev/FeatureRequests/pdfce_FeatureRequests/open/`, run by this role
at filing time. **Nothing in this repository can report a file outside it** —
the same standing reason the `pdfceGUI` and `iccce` inbox boxes exist, and the
same reason the 296th filing recorded its channel check with a command rather
than an assertion.

| field | value |
|---|---|
| file | `open/request_a_widget_border_can_be_written_and_not_read_so_a_properties_control_would_lie.md` |
| arrived | **2026-08-27 21:54** (`ls -lt`) |
| against | `Pass 134.0` (`edit_widget`), **now consumed** |
| their tip | `pdfceGUI` at `aa7109f`+ · **`pdfce-core` at `2e6235c`** — i.e. they wrote it **after `Pass 142.1` landed** and **before `144.0`/`145.0`** |
| blocking? | **No, and they say so by name** |

**Why it slipped the previous box.** The 296th filing checked the channel and
found **three** files at 17:18 and 18:21. This one arrived **at 21:54, during
the engineering session**, between that check and the three replies this
session wrote at **22:38–22:39**. **Third consecutive session in which the
channel changed underneath a check that had already run** — the 296th filing
recorded the same shape about *its* predecessor. **A channel check is a
timestamp, not a state**, and that is now n = 3.

#### The ask, in one line

**`WidgetEdit` can WRITE four widget properties; `forms::Widget` can READ
two.** They shipped the two that can be read (`rect`, `caption`) and
**refused to ship the other two** (`border`, `visibility`).

| property | write | read | they shipped |
|---|---|---|---|
| `rect` | `WidgetEdit::with_rect` | `forms::Widget::rect` | ✅ |
| `caption` | `with_caption` | `forms::Widget::caption` | ✅ |
| **`border`** | `with_border(BorderSpec)` | **nothing public** | ❌ |
| **`visibility`** | `with_visibility(Visibility)` | **only by a detour** | ❌ |

**What they grepped, offered so pdfce can check the claim rather than take
it** — their own framing, and it is `R203`/`R220`(c) discipline arriving from
the other side of the boundary: `forms::Widget` has no border field of any
kind; `annot_author::read_border_width` is **private**;
`annot_author::border_style` is a **writer**; and **nothing anywhere reads
`/BS` `/S`** (solid / dashed / beveled / inset / underline). **Not verified
here** — this role reads documents, and the claim is about `crates/`.

#### ★★★ WHY THEY DID NOT SHIP THEM WITH A DEFAULT, AND THIS IS THE PART WORTH THE ENGINEER'S ATTENTION

> *"A properties control has to show the current value. That is what makes it
> a properties control rather than a command. A border control seeded from a
> default would show* Solid, 1 pt *over a widget whose file says* Dashed, 3 pt
> *— and the operator's first press would write the invention into their
> document, silently replacing a border they never looked at."*

**They cite pdfce's own precedent for the refusal, correctly.** The text
colour swatch shows **a sentence** rather than a nearest-RGB approximation for
a run painted in `DeviceCMYK`, because *"a swatch showing DeviceCMYK ink as
its nearest RGB would write that RGB back on the next press"*. **Same failure,
same refusal** — and it is `CLAUDE.md` rule 4 read correctly from the outside:
a control seeded from a guess is an **inference presented as document state**,
which is the one thing rule 4 has never permitted through any of its two
narrowings.

**⇒ The gap is a WRITE-WITHOUT-READ ASYMMETRY in `pdfce-core`, not a wiring
omission in their shell**, and that makes it this project's finding under
decision 058 — the same shape as `Pass 142.1`'s pre-flight and the
`insert_pages` disclosure fields.

**★★ WORK IS ALREADY IN FLIGHT ON THIS, IN THE ENGINEER'S UNCOMMITTED TREE, AS
THIS BOX IS BEING WRITTEN — and the box stands anyway.** `git diff` run here
after this filing's last edit shows `crates/pdfce-core/src/forms.rs` **+351
lines**, adding exactly three public fields: **`border: Option<BorderSpec>`,
`visibility: Option<Visibility>`, `annot_flags: AnnotFlags`** — options (1) and
(2) below, plus the raw flags. **That is uncommitted work, so it has no commit,
no Pass ID and no acceptance criteria**, and this box is what stops it becoming
a shipped capability with no roadmap entry (`R217`'s shape). **The engineer
should claim `Pass 146.0` for it before committing**, or say why it is a
sub-Pass of something existing. Reported from `git diff`, not inferred.
**Nothing here is a request to stop.**

#### Three shapes they offer, with no preference between them

1. **`forms::Widget::border: Option<BorderSpec>`**, populated by
   `parse_acroform` — symmetric with `caption`, which pdfce's own doc says is
   modelled *"because modelling it is what lets a caption be listed, copied
   and compared"*. **A border is the same kind of fact.**
2. **`forms::Widget::visibility: Visibility`**, same place, *"so we are not
   deriving your enum from flag bits"* — they note `annot::page_annotations`
   plus `Annotation::flags` would work but would be **a second implementation
   of a mapping pdfce owns**, which is `R221`'s shape arriving unprompted from
   a consumer.
3. Or **`widget_properties(fqn, index) -> WidgetProperties`** — *"the
   pre-flight shape you used for `preview_font_resources`, which we like."*

#### ★ THEY ALSO REPORT A COST THIS PROJECT CAUSED, AND IT IS THE SECOND HALF OF `R220`

> *"It shipped a day late and that is ours. The reply sat unread in `open/`
> while our own panel told the operator that these properties* can only be set
> when a field is placed. To change one, delete this field and place a new
> one. *— which is a **destructive** instruction (it loses the name, the value
> and the tab position) for a capability you had already built."*

**Their own lesson, quoted because it generalises to this project verbatim:**
*"an absence claim about a crate we do not build has a shelf life, and that
one was true when written and false within hours."* **That is `R220` clause
(c) and `R203` stated from the consumer's side**, and it is the **fourth**
recorded instance in four days of an absence-or-universality claim about a
neighbouring project's capability going stale. **The claim did not merely
mislead — it recommended a destructive action.**

#### ★ AND ONE THING BACK ON `Pass 142.1`, WHICH IS A LIVE ENGINEERING FACT

They have **not yet consumed `preview_font_resources`** (`gui` stays `[ ]` in
`FEATURES.md`, correctly) and it is next for them. Two uses named, and the
second carries a consequence **pdfce did not draw**:

- the **face chooser's list**, replacing today's `fontinfo` name-join superset;
- **`real_bold()` / `real_italic()` for routing Bold** — and *"note the
  consequence you may not have drawn: routing from `real_bold()` makes the
  Bold button **independent of `gate_synthesis`**, so it will do the right
  thing on `format_family.pdf` whether or not `Pass 144.0` has landed."*
  **`Pass 144.0` has since landed**, so they get both.

**★★ A PERFORMANCE REPORT IS INBOUND AND IS ALREADY PARTLY ANSWERED.** They
say they *"will report what the pre-flight costs on a page with many fonts,
since the face list is drawn per frame while a combo is open and yours is a
per-resource acceptance test."* **`Pass 144.0` removed an O(n²) sibling scan
from exactly that query** — fine at ~20 resources, a hang at the ~50,000 a
crafted `/Font` dict can carry. They should be told it is fixed **before**
they spend a session measuring it, and their measurement is still wanted on
the linear cost.

#### The `iccce` channel — CHECKED and CLEAR

`ls -lt D:/Dev/FeatureRequests/iccce_FeatureRequests/open/`, run by this role
at filing time. **19 files (`ls -1 | wc -l`); the newest is pdfce's own
reply**, `reply_the_profile_census_and_your_33_node_constant.md`, **2026-08-27
14:22** — **unchanged since the 296th filing's check**, which reported the same
count and the same newest file. Nothing inbound is unread.
Recorded explicitly for the same reason as above: **an "it's empty" claim
about a directory outside this repository has no falsifier here.**

---


<!-- Pass 142.1 -->
### ~~`Pass 142.1`~~ — **SHIPPED 2026-08-27, commit `2e6235c`. See the combined `Pass 142.1` + `144.0` + `145.0` entry at the top of *Shipped*.** Retained below as the scoping record (append-only discipline) — **THE FONT-RESOURCE PRE-FLIGHT — PROMOTED FROM *Backlog* TO HERE 2026-08-27 (296th filing), BECAUSE THE REQUESTER ASKED FOR IT BY NAME AND IT IS THE PREREQUISITE FOR `Pass 144.0`** — ★★ two refinements added, and **refinement (2) is the valuable one** — ~~NOT STARTED~~

> **★★ STATUS BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
> **All five provisional acceptance criteria were MET as written**, and both
> named refinements landed:
> **(1)** entries are keyed as `set_font` matches, produced by the **same code
> path** — the test was **extracted** into `accept_font_target`, which
> `plan_font` now calls too, so there is one implementation rather than two
> (`R221`); a `/BaseFont` collision is **reported** via `base_font_ambiguous`
> and a resource-key `selector`, never silently deduplicated.
> **(2)** each entry states whether a real Bold and a real Italic **would be
> ACCEPTED** — `FontPreflight::real_bold()` / `real_italic()` — with the
> self-case handled (an entry that *is* the family's real bold reports
> **itself**, since reporting `None` would tell a shell to synthesize bold on
> top of a real bold face).
>
> **★ One thing this entry did NOT anticipate and the Pass shipped anyway:**
> a new fixture was required. **Nothing in the corpus carried two resources
> with one `/BaseFont`** — the 87 % case this entry cites from the requester's
> survey had **no witness in pdfce's own fixtures**.
> `fixtures/synthetic/textedit/format_twins.pdf` is that witness (corpus **8 →
> 9** files, every pre-existing fixture regenerated byte-identical).
>
> **★★ And the requester has since reported, unprompted, that
> `real_bold()`/`real_italic()` routing makes their Bold button INDEPENDENT of
> `gate_synthesis`** — so it does the right thing on `format_family.pdf`
> whether or not `Pass 144.0` has landed. That consequence was not drawn by
> this entry and is worth carrying: the pre-flight is not merely a
> pre-check for the gate, it is an **alternative route around it**.

**The scoping record stays in *Backlog*** (filed 293rd filing) — the shape of
the query, the `&self`/side-effect-free discipline it shares with
`preview_style_resolution`, and why the answer must be about the **session's
staged content** rather than the base document. **Read that entry before
scoping; this one carries only what has changed since.**

**Why it moved.** `pdfceGUI`,
`reply_synthetic_is_enough_and_142_1_is_the_one_we_want.md`, 2026-08-27
17:18: **"`142.1` — the font-resource pre-flight — is the one we want."**
They are building the Font group now (`format_text` on an existing run:
size, colour, face, bold, italic) and *"the face control is the one with a
real design problem, and it is exactly the one your pre-flight solves."*

**★ THEY ARE NOT BLOCKED, AND THEY SAID SO** — *"We are telling you because
decision 058 says to, not because it is blocking."* They are shipping today
by building the list themselves from `fontinfo::FontInventory`
(`FontRecord::pages`, `FontRecord::base_font`). This is a **quality** ask,
not an unblock — do not scope it as a rescue.

#### The gap, in their table

| what they can answer today | what they cannot |
|---|---|
| which `/BaseFont` names are resources on page N | **whether `set_font` will accept a given one of them** |

#### ★★ THE JOIN PROBLEM, and it is the reason a naive implementation would be wrong

`fontinfo` is keyed on the font **dictionary**. `set_font` matches on
**`/BaseFont`** with the §9.6.4 subset tag stripped. **One page can carry two
dictionaries with the same `/BaseFont`** — two independent subsets of one
face — which the survey behind their Fonts panel found in **87 % of embedding
files**.

⇒ A list built from `fontinfo` is **a superset that is usually exactly
right**, and *"when it is wrong the operator finds out by pressing a button
and getting a refusal."* **A pre-flight that re-derives the key rather than
asking `set_font`'s own matcher reproduces the defect it exists to
remove** — `R221`, and the same trap as `Pass 144.0`.

#### The two refinements they named

1. **The list keyed the way `set_font` matches** — *"the strings that WILL
   resolve, not the dictionaries that exist."*
2. **Per entry, whether a real Bold and a real Italic of that family also
   resolve on the page.**

**★★ (2) IS THE MORE VALUABLE OF THE TWO, IN THEIR WORDS**, and the reason
is specific rather than general: it is **the fact that decides whether their
Bold button routes to `set_font` or to `set_synthetic`** — *"which today we
cannot know before pressing, and your `gate_synthesis` complement means the
two verbs are exhaustive but only AFTER one of them has refused."*

They name it as the question the previous reply left them unable to ask, and
*"it is still unaskable."*

**⇒ And `Pass 144.0` proves the two verbs are NOT exhaustive** (see that
entry). Refinement (2) computed honestly — *does a real Bold that would
actually be ACCEPTED resolve here?* — **is most of `Pass 144.0`'s missing
predicate.** Build this first; make `144.0` a caller.

#### Acceptance criteria, provisional — ADDITIVE to the *Backlog* entry's

1. Entries are keyed **as `set_font` matches**, produced by the **same code
   path** `set_font` uses, not a reimplementation (`R221`). Two dictionaries
   with one `/BaseFont` collapse to **one** entry, or the collision is
   reported explicitly — never silently deduplicated to the wrong one.
2. Each entry states whether a **real Bold** and a **real Italic** of that
   family **would be accepted** on this page — accepted, not merely
   name-matched. **Name-matching is the `Pass 144.0` defect.**
3. The answer is about the **session's staged content**, per the Backlog
   entry's `preview_style_resolution` precedent.
4. `pdfce-cli` surface per rule 11.
5. **It still does not depend on `Pass 142.0`** and remains worth shipping
   if `142.0` is never built — that judgement from the Backlog entry is
   **confirmed** by the requester's answer, not superseded by it.

#### ★ ONE THING THEY RETRACTED BEFORE SENDING, AND THEY LEFT IT IN ON PURPOSE

Their §4 held an ask — *"`FormatReport` has no `synthesis` field"* — which
was **wrong**, caught by them pre-send: it is `format.rs:913`, `pub
synthesis: StyleSynthesis`, beside `synthetic_bold_width` and
`synthetic_italic`, with `disclosures: Vec<String>` carrying the prose.

**They left it struck rather than deleted, and their reason is the finding:**

> *"we wrote an absence claim about your crate into a document whose whole
> subject was you having done the same thing, within the hour. Your `R220`
> is not a rule about carelessness — the pull toward 'I looked and did not
> see it, therefore it is not there' is strong enough to survive reading a
> note about itself."*

**A consuming project has adopted `R220`'s clause (c)** — an absence claim
about `pdfce-core` is grepped against source **before the file is saved**,
not before it is sent. Recorded here because it is evidence about the rule's
transferability, and because it is a **fourth** same-week instance of the
absence-claim pull, this one caught in time.

---


<!-- Pass 144.0 -->
### ~~`Pass 144.0`~~ — **SHIPPED 2026-08-27, commit `cfa2c44`. See the combined `Pass 142.1` + `144.0` + `145.0` entry at the top of *Shipped*.** Retained below as the scoping record (append-only discipline) — **`gate_synthesis` NAMES A REAL FACE THAT THEN REFUSES, AND ON THAT PAGE BOLD IS UNREACHABLE THROUGH EITHER VERB** — ★★★ **REPRODUCED BY THE ENGINEER BEFORE FILING, ON PDFCE'S OWN FIXTURE — CONFIRMED, NOT CLAIMED** — ★★ `R221`'s THIRD INSTANCE, hours after the mint, in a different subsystem — ★ **`R90` IS NOT WEAKENED BY THIS** — filed 2026-08-27 (296th filing), ~~NOT STARTED~~

> **★★★ STATUS BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
> **All seven provisional acceptance criteria were MET**, including the two
> non-code items in this entry's own *"TWO THINGS OWED THAT ARE NOT CODE"*
> section: §3.6 was corrected, and `pdfceGUI` was told on the channel
> (`reply_gate_synthesis_fixed_your_test_goes_red_on_purpose.md`, 2026-08-27
> 22:39) that **their characterisation test goes red on purpose** and that
> **their retry must switch from `real_font` to the new `selector` field.**
>
> **Three branches shipped exactly as scoped**, and the family preference is
> expressed by **the ORDER of two searches** rather than by a separate rule —
> so the preference cannot drift from the acceptance test. Branch 3 (synthesis
> proceeds when nothing is accepted) is pinned by a test **authored to fail if
> a future change turns it into a refusal**, as criterion 3 required.
>
> **★ WHAT THIS ENTRY GOT RIGHT AND UNDERSTATED.** Criterion 5 named **one**
> stale doc claim (`name_claims_bold`'s *"never to refuse an edit"*). The
> change found **four**, and only that one had been reported by anybody:
> **(b)** `gate_synthesis`'s own doc claimed a `Times-Bold` request *"is
> refused with that same face named"* when `!is_self` **skips** it, so it is
> never named; **(c)** the `RealFaceAvailable` **format string** named a remedy
> it had not checked (`R222`, firing on its first live change); **(d)** §3.6's
> *"every page is covered"* (`R220` clause (d)). **Four claims, one function
> pair, zero reported by any tool.** Claim (a) is `R223`'s first instance — see
> *Standing rules*.
>
> **★★ A DEFECT THIS ENTRY DID NOT NAME, FIXED IN THE SAME CHANGE:** an
> **O(n²) per-entry sibling scan over an attacker-controlled `/Font` dict** in
> the pre-flight. Fine at the ~20 resources a real page carries; **a hang at
> the ~50,000 a crafted one can** — in a query the requester has since
> confirmed they call **every frame** while a combo box is open.
>
> **★ TWO IN-REPO `pdfce-gui` TESTS CHANGED EXPECTATION ON PURPOSE.** They
> asserted `Times-Bold` and now assert `Calibri-Bold` — and **they passed
> throughout the defect**, because they asserted the **name** the gate produced
> rather than **that the named face worked**. The property now lives in
> `crates/pdfce-core/tests/synthesis_gate.rs` where it can be a property
> instead of a string.

**Provenance.** `pdfceGUI`,
`request_gate_synthesis_names_a_face_that_cannot_cover_the_run.md`,
2026-08-27 18:21. Found by *"a driven test of your own fixture."*

**★★★ IT WAS REPRODUCED BEFORE IT WAS BELIEVED.** The engineer ran all three
commands on `pdfce-cli` at **`703a38e`**, against pdfce's own fixture
`fixtures/synthetic/textedit/format_family.pdf`. **A consuming project's bug
report is a claim** (`R203`); this one was **measured** first, and the entry
records it that way deliberately — the alternative is filing a Pass on
someone else's word, which is how a wrong premise becomes a work order.

#### The three commands, measured

```
--find "hello world" --bold-synthetic
  refused: a REAL bold face is available as 'Times-Bold' (resource /F3)
           … change the run's family to 'Times-Bold' instead.

--find "hello world" --set-font Times-Bold        <- the remedy it names
  refused: R-INV-7: character U+006F 'o' has no code in 'Times-Bold's
           encoding; code 111 is already assigned by /Differences

--find "hello world" --set-font F2                <- never mentioned
  set_font=Times-Roman->Calibri-Bold              <- SUCCEEDS
```

⇒ **`gate_synthesis` refuses synthesis and names `/F3`. `/F3` then refuses
for coverage. `/F2` — a fully-covering bold on the same page — is never
mentioned.** On this page, bold is reachable **only** by an operator who
already knows to try a face pdfce never names.

#### Cause, read from source (`format.rs:2132`–`2185`), and it matches the reporter's own diagnosis

`gate_synthesis` decides *"a real face is available"* with **two name-based
tests and no coverage test**:

1. `family_stem(&base) != want` ⇒ **`continue`**. `/F2` is `Calibri-Bold`;
   stem `Calibri` ≠ `Times`, so **`/F2` is never even considered as a face
   the refusal could name.**
2. `name_claims_bold(&base)` (`synth.rs:404`) — `n.contains("bold") ||
   "black" || "heavy" || "semib"`. **A string test on `/BaseFont`.**

**Neither asks whether the face can show THIS RUN'S TEXT**, and that answer
is **already computable**: `set_font` computes it moments later and refuses
on it (`R-INV-7`, the `/Differences` encoding-coverage check).

**The family preference itself is right and must survive the fix.** An
operator asking for bold on Times wants Times-Bold, not Calibri-Bold. The
defect is not the preference; it is that the preference is applied **without
asking whether the preferred face can show this text.**

#### ★★ `R221`'s THIRD INSTANCE — the rule was minted this morning and this is a different subsystem

**`R221`:** *a predicate that decides whether a capability applies is
computed by the code that provides the capability — ask the real function,
never pattern-match a parallel description of when it would succeed.*

`name_claims_bold` **is** a parallel description of when `set_font` would
succeed, written by hand, at a different call site, at a different time. It
drifts from `set_font` exactly as `R221` predicts. The rule's two minting
instances were both in the **colour/overprint** subsystem; this one is in
**`text_edit`**, which is what makes it worth recording rather than merely
noting.

**★ AND IT INVERTS `R221`'s RISK ANALYSIS, WHICH THE RULE ASKS FOR
EXPLICITLY.** `R221`'s worked example (`Space::yields_cmyk`) could show that
*"neither error direction can paint a wrong colour."* **Here one direction
is harmful:** a **false positive** — *"a real face is available"* when it
cannot cover the run — makes the capability **unreachable through either
verb**, which is worse than the slow path and worse than a wrong pixel,
because the operator has no route at all. `R221` is amended in place to
carry that (see *Standing rules*).

#### ★ `R90` IS NOT WEAKENED BY THIS, and this paragraph exists because a future session will read the fix as loosening a refusal

`R90` says synthesis is a **fallback for when no real face RESOLVES**, never
an alternative to one. **The fix does not touch that.** It corrects the
predicate for the word **"resolves"** — from *"exists with a matching family
name"* to *"would actually be accepted by `set_font` for this run"*. That is
`R90` applied **more accurately**, not less. Any change that lets synthesis
run while a genuinely usable real face is present is a regression, not this
Pass.

#### Shape of the fix, in the reporter's terms and the engineer's alike

`gate_synthesis` should treat a real face as **available** only if `set_font`
would actually accept it **for this run**. Three branches, and the third is
the one that must not be lost:

1. **Family match passes coverage** ⇒ refuse synthesis, name it. *(Today's
   behaviour, unchanged.)*
2. **Family match fails coverage, another resource passes** ⇒ refuse
   synthesis, **name that one instead**.
3. **No resource passes** ⇒ **synthesis is genuinely the only option and
   must proceed.** This is the first branch's own reasoning (*"No font
   resources to search: nothing better exists, so the fallback is genuinely
   the only option. Proceed."* — `format.rs:2145`) applied to the right
   predicate.

**Build `Pass 142.1` first.** Its pre-flight computes the same predicate for
a shell; making `144.0` a caller of it is the difference between one answer
and two that drift — see the ordering argument in the priority box above.

#### Acceptance criteria, provisional

1. `gate_synthesis` refuses synthesis **only** when a real face both matches
   the request's style **and** would be accepted by `set_font` for the run's
   actual text. The predicate **calls the accepting code**; it does not
   restate its conditions (`R221`).
2. Where the family match fails coverage and a non-family resource passes,
   the refusal **names the resource that would work**, not the one that
   would not. `format_family.pdf` is the pinning fixture: `--bold-synthetic`
   must either succeed or name `/F2`.
3. Where **no** resource passes, synthesis **proceeds** — pinned by a test
   authored to fail if the fix turns branch 3 into a refusal.
4. **`R90`'s gate is unchanged for every input that works today** — pinned,
   same discipline as `Pass 142.0`'s criterion 2.
5. **`name_claims_bold`/`name_claims_italic`'s doc comment
   (`synth.rs:391`–`403`) is corrected in the SAME change** — it claims the
   heuristic is *"never used to refuse an edit"* and `gate_synthesis` is a
   refusing caller. Fixing the predicate without fixing the comment leaves
   the comment accidentally true; fixing the comment without the predicate
   leaves the defect. `R222` — the format string at `format.rs:1017`–`1022`
   carries the same remedy claim and moves with them.
6. **`docs/core-api/03-capabilities.md` §3.6 (`:1229`) is corrected**, and
   **`pdfceGUI` is told on the channel** — see the two owed items below.
7. `pdfce-cli` surface per rule 11: nothing new is needed, but the
   `--bold-synthetic` refusal text changes and its test must move with it.

#### ★★ TWO THINGS OWED THAT ARE NOT CODE, AND THE SECOND IS TIME-SENSITIVE

- **The §3.6 correction is ENGINEER-OWNED, not this role's.** Per the
  2026-08-18 ruling, `docs/core-api/` belongs to the engineer. **Reported,
  not edited.** The false sentence is `docs/core-api/03-capabilities.md:1229`
  (*"So between the two verbs, **every page is covered**"*). **The
  neighbouring guidance at `:1248` — *"do not grey out a bold button"* — is
  STILL TRUE and must NOT be corrected with it**; the requester agrees the
  button should be offered. Correcting the wrong sentence of the two is the
  available mistake here.
- **★ A FIX BREAKS A DOWNSTREAM TEST ON PURPOSE, AND THEY ASKED FOR THAT.**
  `pdfceGUI`'s Bold button retries with the face the refusal names, and they
  wrote a **characterisation** test naming pdfce's revision whose docstring
  says it will start failing on its *"nothing was applied"* assertion when
  this is fixed. **Their words: *"That failure is the good news."***
  **Whoever ships this must tell them on the channel** — a downstream red
  build that nobody was warned about is indistinguishable from a regression.

#### ★ WHAT NO TEST ASKED, and it is the same shape as the finding that produced `R220`

pdfce's own `fixtures/synthetic/textedit/PROVENANCE.md` documents **both
halves separately and correctly**: `/F2` as *"a fully-covering target"*, and
`/F3` as one that *"does NOT cover `o`, so `--set-font F3` is REFUSED by
name"*. **Nothing asked what happens when the SYNTHESIS GATE picks between
them.** Two documented facts, each accurate on its own, with the defect
living in **the join** — the same shape as `R220`'s *"documented, accurate,
gate-green and unfindable"*. A fixture authored to be exactly this shape did
not catch it, because the tests were written per-fact rather than
per-interaction.

#### ★ WHAT THE REPORTER DELIBERATELY DID NOT BUILD, and it was the right call

> *"We did not build a shell-side search for a different bold resource on the
> page. It would work, it would take about twenty lines, and it would be this
> project second-guessing your font selection — decision 058's exact case.
> Told rather than built."*

**Decision 058 working as intended, from the other side of the boundary.**
Recorded because the cheap wrong answer here is for pdfce to shrug and let
shells each grow their own font-selection heuristic; three shells would then
disagree about which face is "available" on the same page.

---


<!-- Pass 145.0 -->
### ~~`Pass 145.0`~~ — **SHIPPED 2026-08-27, commit `0c48bbf`. See the combined `Pass 142.1` + `144.0` + `145.0` entry at the top of *Shipped*.** Retained below as the scoping record (append-only discipline), **and its UNVERIFIED label is part of that record — see the banner** — **A PINNED `FormatRequest` SHOULD BE ABLE TO SAY "THE WHOLE OPERATOR"** — ★★ **THE THREE WRONG ANSWERS ARE THE ENTRY, because each one LOOKED right and each failed for a DIFFERENT reason** — ★ **AN INVARIANT QUESTION IS OWED EITHER WAY, and only pdfce can answer it** — ★★★ **CORRECTED WITHIN THIS SAME FILING: THE SECOND WRONG ANSWER'S CAUSE WAS RELAYED FROM THE REPORTER AND IS FALSE (0 of 256 fixtures); THE REAL MECHANISM IS `/ToUnicode` MULTI-CHARACTER MAPPING, AND IT CHANGES THE DOCS HALF FROM A FALSE STATEMENT TO A TRUE ONE** — filed 2026-08-27 (296th filing), ~~NOT STARTED~~

> **★★★★ STATUS BANNER — READ BEFORE ACTING ON ANYTHING BELOW. TWO CLAIMS THIS
> ENTRY RECORDS AS OPEN ARE NOW ANSWERED BY MEASUREMENT.**
>
> **(1) THE `operator_span`-SLICE INVARIANT HOLDS.**
> `crates/pdfce-core/tests/operator_span_invariant.rs` walks `fixtures/`:
> **4,289 files · 1,623 with text · 18,559 runs · 669,436 glyphs · 29,246
> `operator_span` groups → 0 non-contiguous groups, 0 groups that fail to
> index the run's text cleanly.** Sabotage-checked (excluding one glyph per
> group from the coverage computation turns the probe red). **This entry's
> "if it does not hold, their shipped workaround is resting on luck" branch
> did NOT fire** — the workaround is sound, and the invariant is now a
> **published guarantee** (`ARCHITECTURE.md` §12, **decision 094**) plus a test
> that re-runs on every `cargo test`.
>
> **(2) THE MULTI-OPERATOR CLAIM — recorded below as the reporter's account
> and explicitly UNVERIFIED — IS CONFIRMED. `2,420 of 18,559 runs (13.0 %)`
> carry glyphs from more than one show operator.** **Common, not exotic.**
> Their third table row was right, and **this is what makes the affordance
> load-bearing rather than a convenience**: on 13 % of runs a `find` rebuilt
> from `TextRun::text` names a range across several operators, which
> `match_run` cannot express. The UNVERIFIED label below is **left standing on
> purpose** — it was the correct epistemic state at filing time, and striking
> it would erase the record of a claim being carried honestly until it could be
> checked. **The probe settled both questions, as this entry's *"ONE PROBE, TWO
> ANSWERS"* note predicted; it was built once.**
>
> **All five provisional acceptance criteria MET.** Criterion 1's guard held —
> **an empty `find` with NO pin is still refused by the same name**, and both
> CLI verbs refuse it with a message naming the **flag** that would fix it,
> which core cannot know about. Criterion 3 was **not** discharged by shipping
> the affordance, exactly as this entry insisted it must not be.
>
> **★★ THE SUBTLE DEFECT A SHALLOWER FIX WOULD HAVE SHIPPED, not named
> below:** `plan_font` checked encoding coverage against `req.find`. On a
> whole-operator request that is `""` — and **every face covers the empty
> string** — so a family change to a face that **cannot show the run** would
> have been **ACCEPTED**. The affordance would not have broken coverage
> checking; it would have **hollowed it out silently**. `plan_font` now takes
> the resolved text, pinned by a named test.
>
> **★ AND TWO DEFECTS IN THE PASS'S OWN NEW CODE WERE FOUND BY RUNNING THE
> BINARY, NOT BY THE 4,463-TEST SUITE** — `cmd_edit_text` parsed `--pin-span`
> and never attached it (the refusal read *"empty find text"*, the very thing
> the feature removes), and a shipped refusal string carried fourteen baked-in
> spaces past a green `check-string-gaps.sh`. Both live in the **shell layer
> between the flags and the core API**, which a core-API test cannot reach by
> construction.

**Provenance.** `pdfceGUI`,
`request_a_pinned_format_request_should_be_able_to_say_the_whole_operator.md`,
2026-08-27 18:21. *"Filed as a documentation nit first; driving it promoted
it."*

**The gap.** `FormatRequest` has no way to express *"restyle the whole pinned
operator."* `FormatRequest::new(page, find)` requires `find` even when the
caller has **already located** the operator by `pinned_span`, so a caller
that has the thing in its hand must still **describe** it.

**Symptom that sent them looking**, quoted because it is the shape an
operator reports rather than the shape an engineer debugs:

> *"eleven pieces of text went bold and the twelfth refused"* — on a page
> where nothing is unusual.

#### ★★ THE THREE WRONG ANSWERS, and this table is the most valuable thing in the report — ⚠ **but the RESULTS are theirs to report and the CAUSES were theirs to guess: row 2's cause is FALSE (corrected below, measured), row 3's is UNVERIFIED**

| attempt | result |
|---|---|
| `find: ""` with a pin | `Unsupported("empty find text")` from `match_run` |
| `find` = `TextRun::text` | **`NoMatch`** — **`TextRun::text` can contain characters that are not in the file.** ⚠ **The MECHANISM filed here first was wrong and is corrected below — see *"The second row's cause was relayed, not measured"*.** The claim as originally filed, kept legible: ~~"Extraction **synthesises** a space wherever a `TJ` offset exceeds the word-gap threshold, so a title-block cell reads `"FINISH         "` while the buffer holds `FINISH` and kerning numbers"~~ |
| `find` = the glyph-covered bytes only | **Still `NoMatch` on some runs** — reported cause: **a `TextRun` can span several show operators.** ⚠ **UNVERIFIED — see *"The third row is unverified"* below; recorded as the reporter's account, not as a pdfce-confirmed fact.** As filed: `layout` closes a run on **geometry**; a producer closes an operator *"on whatever its writer felt like"*. The pin named the first `Tj`; the find named a range across three of them, which `match_run` cannot express |
| `find` = the glyphs sharing the pinned operator's own span | ✅ **works** — and is what they shipped |

**Three refusals, each with accurate prose, each with its cause somewhere
else entirely.** That is the cost being reported, not the final workaround —
they are content with the workaround.

#### ★★★ CORRECTION, SAME FILING (296th) — **THE SECOND ROW'S CAUSE WAS RELAYED, NOT MEASURED, AND IT IS FALSE. THE HEADLINE SURVIVES; THE MECHANISM DOES NOT — AND THE REAL MECHANISM CHANGES THIS PASS'S SCOPE.**

**What was filed first**, struck rather than deleted (append-only
discipline), because the shape of the error is the transferable part:

> ~~"`find` = `TextRun::text` → `NoMatch`, because **`TextRun::text`
> contains characters that are not in the file** — extraction **synthesises**
> a space where a `TJ` offset exceeds the word-gap threshold, so a
> title-block cell reads `"FINISH         "` while the buffer holds
> `FINISH` and kerning numbers."~~

**Why it is false, from source.** A derived word space is **never inside a
glyph run**. `text_extract/layout.rs`'s `Break::Word` arm calls
`close_run()` and *then* `emit_derived(' ', TextOrigin::DerivedWordSpace)`,
which pushes a **separate one-character `TextRun` with
`glyphs: Vec::new()`**; `text_edit/model.rs:548` keeps the two apart on the
edit side as well (`DerivedWordSpace => {}`). **A `TextOrigin::Glyphs` run's
`text` therefore contains only real glyph characters.**

**★ WORLD-SOURCE, so this correction is itself checkable (hard rule 10's
corollary — a correction is a claim).** Not read: **measured**, by the
engineer, over **every run extracted from 256 fixture PDFs** via
`pdfce-cli extract-text --json`, counting glyph runs whose character count
differs from their glyph count:

| over 256 fixture PDFs | runs |
|---|---:|
| `derived_word_space` runs (always emitted separately) | 5 |
| **glyph runs containing a synthesised space** — *the claimed cause* | **0** |
| glyph runs where `len(text) != len(glyphs)` | 1 |

⇒ Zero of 256. The reporter's `"FINISH         "` string is almost certainly
**their own concatenation** of adjacent runs — a glyph run, a
`DerivedWordSpace` run, another glyph run — which produces exactly that
string without any run ever containing it.

**★★ AND THE SINGLE OFFENDER IS THE REAL MECHANISM, WHICH NEITHER SIDE
HAD.** `fixtures/synthetic/text/identity-h-tounicode.pdf` holds one run
whose `text` is **8 characters over 6 glyphs**. Not synthesis —
**`/ToUnicode` maps one glyph to SEVERAL characters** (ISO 32000-1
§9.10.3): an `ffl` ligature is one glyph and three characters; a surrogate
pair is one glyph and two `char`s.

**So the headline claim is TRUE and the mechanism was WRONG**, and the two
mechanisms have **different consequences** — which is why this correction
re-scopes the Pass rather than merely tidying it:

- **Word-gap synthesis** would insert a character present in **no** operator
  — a character pdfce invented.
- **A ligature** maps a character **range** onto a single glyph that **is**
  in the operator. So a `find` built from `TextRun::text` fails on the
  **buffer bytes**, not on locating the operator — and the failure is
  **invisible on unligatured test text** and **routine on real typeset
  copy**. A `find`-based locator will therefore look correct in every
  synthetic fixture a shell writes for itself.

**★ Against `CLAUDE.md` rule 4, restated on the corrected facts** (the
original note argued from the false mechanism and is superseded): the
`/ToUnicode` expansion is **not an inference at all** — it is the file's own
mapping, faithfully applied, so rule 4 does not bite there. The derived word
space **is** an inference, and it is disclosed **as an entire separate run
carrying `TextOrigin::DerivedWordSpace`** — a stronger disclosure than the
original note credited, not a character hidden inside somebody else's run.
**The `R220` findability complaint survives in weakened form**: the fact a
caller actually needs — *one glyph may map to several characters, so
`text.chars().count()` is not `glyphs.len()`* — is documented nowhere the
caller looks. See the docs half below, which is re-scoped accordingly.

#### ★★ THE THIRD ROW IS UNVERIFIED, AND IS RECORDED AS UNVERIFIED

*"A `TextRun` can span several show operators"* is the reporter's account
and **has not been measured by this project.** It could not be, from
outside: **`GlyphProvenance::operator_span` is not exposed by
`pdfce-cli extract-text --json`** — the emitted glyph objects carry
`code`/`rung`/`sourced`/`start`/`len`/`x`/`y`/`advance`/`size`/`direction`/
`invisible` and **no span**.

**The method that would settle it, written down because it is owed either
way:** an **in-crate probe over a corpus, counting runs whose glyphs carry
more than one distinct `model.provenance(...).operator_span`.** Zero such
runs over a real corpus is evidence the claim is false as stated (or at
least not reachable on ordinary producers); a non-zero count names the
witness.

**★ ONE PROBE, TWO ANSWERS.** That same probe settles the
`operator_span`-slice invariant of acceptance criterion 3 below — the
question *"do the glyphs sharing one `operator_span` always slice a
contiguous, matchable range out of the run's text?"* is answered by the
same enumeration. Build it once.

#### The ask, as filed

> **`find: ""` on a request that carries `pinned_span` means the whole pinned
> operator.**

Their estimate: *"Two lines in `match_run`, we would guess."* Recorded as
**their estimate**, unverified here.

**What it buys, in their words:** *"It would let a caller that has already
located an operator stop having to describe it, which is the thing that went
wrong three times above — and it would delete a function from our side whose
whole job is reconstructing a string the engine already has in its hand."*

#### ★★ THE ALTERNATIVE THEY OFFER, AND THE QUESTION THAT IS OWED EITHER WAY

If `find` stays mandatory, they ask instead for a **documented invariant**:

> *"the glyphs sharing one `operator_span` always slice a contiguous,
> matchable range out of the run's text."*

> *"We believe that is true today and we have no way to know whether it is
> guaranteed."*

**★ THIS IS A QUESTION ONLY PDFCE CAN ANSWER, AND ANSWERING IT IS OWED
WHICHEVER BRANCH IS TAKEN.** The two outcomes are not symmetric:

- **If the invariant holds** — it is undocumented load-bearing behaviour and
  must be **written down and pinned by a test**, or the next refactor of
  `layout` breaks a downstream project silently.
- **If it does not hold** — **their shipped workaround is resting on luck**,
  and they need to be told on the channel, promptly, because it is in a
  build their operator is using.

⇒ **Do not close this Pass by shipping `find: ""` and skipping the invariant
question.** Adding the affordance makes the workaround unnecessary going
forward; it does **not** tell them whether what they already shipped is
sound. That is a separate answer and it is owed on the channel either way.

#### ★ THE DOCS HALF, WHICH STANDS WHATEVER IS DECIDED — **ENGINEER-OWNED, REPORTED NOT EDITED**

Per the 2026-08-18 ruling, `docs/core-api/` is the engineer's. Two doc gaps,
**neither of which is wrong** — both are *findable only by someone who
already knows*, which is `R220`'s exact diagnosis:

1. **`FormatRequest::new(page, find)`** describes `find` as *"the text to
   locate within one show operator's decoded run"* and **does not say it
   stays required when the operator is already located by a pin.**
2. **`TextRun::text`'s own docs do not say its character count can differ
   from its glyph count.** ★★ **RE-SCOPED BY THIS FILING'S CORRECTION, AND
   THE ORIGINAL WORDING WOULD HAVE SHIPPED A FALSE STATEMENT.** As filed
   this read: ~~"`TextRun::text`'s own docs do not say it may contain
   DERIVED characters. That fact lives on `TextOrigin` — one level away
   from where a reader looking for 'what text is in this run' would
   land."~~ **Writing that into `docs/core-api/` would have published the
   relayed mechanism as pdfce's own statement about pdfce's own type** —
   and a `TextOrigin::Glyphs` run's `text` contains **no** derived
   characters (measured: 0 of 256 fixtures). What the docs actually owe is
   the **true** fact: **one glyph may map to SEVERAL characters via
   `/ToUnicode` (ISO 32000-1 §9.10.3) — so `text.chars().count()` is not
   `glyphs.len()`, and a caller building a `find` string from a run cannot
   assume a 1:1 correspondence with the content-stream buffer.** The
   derived-character route is real but lives in **separate runs** tagged
   `TextOrigin::DerivedWordSpace`, which is worth one sentence beside it so
   a caller concatenating runs knows what it is concatenating.

#### Acceptance criteria, provisional

1. `FormatRequest` can express *"the whole pinned operator"* — via `find:
   ""` + `pinned_span`, or via a named constructor if the empty-string
   overload reads as a footgun. **`Unsupported("empty find text")` must stay
   the answer for an empty `find` with NO pin**, or a caller that forgot to
   pin gets silent whole-operator behaviour instead of a refusal.
2. The multi-operator case is **stated**: a `TextRun` spanning several show
   operators restyles **the pinned operator only**, and the disclosure says
   so — off-canvas, `CLAUDE.md` rule 4; printed by `pdfce-cli`, rule 11.
3. **The `operator_span`-slice invariant is ANSWERED** — documented and
   test-pinned if true, contradicted with a counter-example if false. Not
   optional, and not satisfied by criterion 1. **Method, from this filing's
   correction: an in-crate probe over a corpus counting runs whose glyphs
   carry more than one distinct `model.provenance(...).operator_span`** —
   which **also** settles whether a `TextRun` can span several show
   operators at all (the third row of the table above, recorded as
   UNVERIFIED). One probe, two answers.
   **And in the same breath, the multi-operator claim is either CONFIRMED
   or RETRACTED on the channel** — it is currently the reporter's account,
   carried here as unverified. If the probe finds no such run, they are
   relying on a mechanism that does not exist; that does not make their
   workaround wrong, but it changes what they should be told.
4. The two doc gaps above are closed **in the same change** — a capability
   entry titled by the operator's question, per `R220` clause (a).
5. `pdfce-cli`: whether `format-text` grows a way to say *"the operator at
   this point"* is a scoping question, not settled here; rule 11 applies if
   the answer is yes.

#### ★ ONE PROCESS NOTE, RECORDED BECAUSE IT IS THE CHANNEL RULE WORKING

The reporter put this finding and `Pass 144.0`'s in **one file first, then
split them**, citing the channel README's one-topic-per-file rule —
*"a merged request gets partly dropped in triage."* A font-selection defect
and a locator API have nothing in common but the session that found them.
**The split is why both got a Pass ID instead of one getting a paragraph.**

---

**`Pass 124.2` — the suite-name scrub, `docs/ROADMAP.md`'s half — operator-ordered, 2026-08-25, opened this filing (two-hundred-and-fifty-second), closed the two-hundred-and-fifty-third.** Ken's ruling (quoted in elided form in `(bt)`'s own closure, below, *Open operator questions*) answers `(bt)` and goes further than it asked — see that entry's own nested closure for the full ruling. Five agents scrub in parallel from a shared private contract (`D:\Dev\pdfce-private\suite\SCRUB_SPEC.md`, outside this repository by design): this role owns `docs/ROADMAP.md` only — `SESSION_LOG.md`, `ARCHITECTURE.md`/`FEATURES.md`/the surveys, and `crates/`/`tools/`/`docs/NEXT_SESSION.md` are scrubbed by the other four filings/the engineer, not here.

**Acceptance criterion, rewritten 253rd filing because the original was self-defeating: `python tools/check-suite-name-absent.py` exits `0`.** The original criterion (this paragraph, two-hundred-and-fifty-second filing) named the two forbidden terms literally inside the acceptance criterion meant to guarantee their absence — a checkable rule that was itself the rule's own first violation, and a reader could not then distinguish a real occurrence from the gate hunting them. `tools/check-suite-name-absent.py` (new, shipped by the engineer between the two filings) solves the bootstrapping problem: the two needles are stored **base64-encoded** in the script and decoded at run time, so the rule and its enforcement can coexist in one repository without contradicting each other. This is not obfuscation for its own sake and not secrecy — the private map (`D:\Dev\pdfce-private\suite\`) names the suite in full; it is the only way for a rule and its enforcement to coexist without contradicting each other. The script checks tracked file **contents** (`git grep -I -i`, binaries excluded — a binary OCR model matches a naive case-insensitive byte grep on its weights, and a model's weights are not a mention) **and** tracked file **names** (a scrubbed file still named after the suite fails the check, since the name is published in every directory listing and diff regardless of what the file's contents say). It prints `path:line` but **never the offending line's text**, because CI logs on a public repository are themselves public. Exit codes: `0` clean, `1` occurrences found, `2` could not run.

This filing (253rd) closes the two occurrences the script found in this document itself: this acceptance criterion (just rewritten above) and the verbatim quotation of Ken's ruling in `(bt)`'s closure, below, which is now elided — see that entry's own note on why the elision is deliberate. That brings `docs/ROADMAP.md`'s own portion to zero. Also folded in here, completed by the engineer between the two filings: five patch identifiers in `crates/pdfce-render/tests/page_blend_space_source.rs` now resolve through **`PDFCE_SUITE_DIR`** plus a `pdfce-manifest.txt` that sits beside the corpus, outside the repository — no path and no suite file name remains in tracked code, verified both ways (variable set: all 5 tests pass; variable unset: all 5 skip, each announcing its own skip on stdout). `tools/suite-cell-probe.py`'s hard-coded corpus default became an environment lookup returning `None`, so the path must now be given explicitly when the variable is unset. `cargo fmt --check`, `cargo check -p pdfce-render --tests` and `cargo clippy -p pdfce-render --tests --all-features` are all clean; `cargo test` was run only for the one affected test binary, **not** the full workspace — the operator is running SolidWorks on a 16 GB machine this session and asked for light resource use, so this is not a full-suite-green claim. `check-suite-name-absent.py` is itself a new gate; see item 7 of the OWED table above for its place in the project's own gate count (amended this filing).

**★ A correction this filing owes its own predecessor's report, not this document** (the 252nd filing's chat report, never written to disk): it flagged three `crates/pdfce-render/src/color.rs`/`overprint.rs` doc-comment sites as *"almost certainly still say [the suite name]"* and owed to the engineer under hard rule 11. They do not — the engineer had already scrubbed all of `crates/`, `tools/`, `.claude/agent-memory/pdfce-engineer/` and `docs/NEXT_SESSION.md` before that filing started, so `crates/` was at zero the whole time, and the concern was a reasonable inference from a file the prior filing could not see (no shell), now closed-and-verified rather than left owed.

**This filing has no shell** (librarian invocation, hard rule 8) — it does not run `check-suite-name-absent.py` itself and does not assert its result beyond what the engineer relayed in dispatch: that the script, before this filing's two fixes, reported exactly these two lines and nothing else. Status: this document's own portion is now at zero occurrences of the two needles, pending the engineer's own run of the script to confirm; the Pass as a whole is still **NOT SHIPPED** until the engineer verifies the combined result across all five filings and commits.

**`Pass 119.0` SHIPPED, `cc57080` — see top of *Shipped*, below.** The
pointer entry that lived here (two-hundred-and-eighth filing) is removed
per this project's fully-delete-on-ship convention (`Pass 92.0`/`93.0`
precedent, reconfirmed 177th filing) — the enduring record is the fresh
Shipped entry, not relocated *Next up* prose. **`Pass 113.0`/`113.1`/
`113.2` (`transform_objects`/`transform_preview`/CLI `object-transform`)
also SHIPPED, `e5be7d5`** — see top of *Shipped*, below; the "next in
line again" sentence this section carried for `113.0` is now stale and
removed with it. Nothing named next in line by this filing — see
Backlog's `114.0`-onward move/resize/rotate carriers and the `119.x`/
`120.x` items `SESSION_LOG.md`'s two-hundred-and-eleventh filing still
lists as queued.


<!-- Pass 122.2 -->
### ★★★★★★ THE SUITE STANDING BOARD — **26 pass AT MINIMUM, of 51 patches** (harness-reported `24 / 11 / 16`, UNCHANGED since `Pass 122.2` — this is a measurement-only step, no code shipped; CORRECTED 2026-08-21 by the 225th filing, AGAIN 2026-08-24 by the 243rd, AND AGAIN 2026-08-24 LATER THE SAME DAY by the 244th — `PCS 1.1`'s DISPUTE IS RESOLVED: a third, independent oracle (Adobe Acrobat) agrees with pdfce's COMBINED render, not its individual one, so `PCS 1.1` flips FAIL → PASS; see `Pass 122.4`'s answered investigation and `Pass 122.5` (Backlog) for the mechanism it exposed, which generalises to 24 of 51 patches — AND CORRECTED AGAIN 2026-08-24, STILL THE SAME DAY, BY THE 245th FILING: `pdfce-spec-librarian` reports the 244th filing's clause citation, its "no page group" premise, and its Acrobat-attribution wording were all wrong, and that under ISO 32000-1 pdfce's CURRENT behaviour is CONFORMING, not defective — see the correction block immediately below, which supersedes the citations (not the measurements) in every block beneath it) — ★★★★★ **THE HARNESS IMPLEMENTS ONE OF THE SUITE'S TWO PASS CRITERIA, AND THE OTHER HAS BEEN REPORTED `clean` FOR ITS ENTIRE LIFE — SO EVERY FIGURE BELOW THIS LINE, INCLUDING ALL THE HISTORICAL ONES, IS OVER-COUNTED BY THE SAME FAMILY** — see the correction blocks immediately below — ★★★ **RE-MEASURED 2026-08-21 (224th filing, `Pass 97.1e`/`97.1f`, `a277931`+`ff4b4bf`) AGAINST A BINARY BUILT FROM `06aaad3` IN A WORKTREE; the previous figure (**26 / 14 / 11**) is kept below as history and was NOT stale — it was correct at its own commit, and this is a SHIP moving it, not a correction** — ★★ **AND THE COMPOSITION OF THE REMAINING FAILs IS NOW THE BOARD'S MOST USEFUL FACT: every one of them is an OVERPRINT, SPOT or ICC patch. Not one is a blending-space failure any more** — ★★ **FIGURE CORRECTED TWICE OVER 2026-08-19 (189th filing): once by a SHIP and once because the PREVIOUS FIGURE WAS ALREADY STALE** — the ONE architectural item that unblocks the largest cluster — opened 2026-08-18 (hundred-and-sixty-sixth filing)

#### ★★★★★★★★ CORRECTED BY `pdfce-spec-librarian`, 2026-08-24 (245th filing, `pdfce-librarian`, NO COMMIT) — FOUR ERRORS IN THE 244th FILING, ONE OF THEM A VIOLATION OF A STANDING OPERATOR INSTRUCTION; `(bs)` WITHDRAWN, REPLACED BY A SETTING

**This block corrects CITATIONS AND FRAMING, not the 244th filing's
MEASUREMENTS.** The three-way render comparison (pdfce individual vs.
pdfce combined vs. Adobe Acrobat) still stands and `PCS 1.1` still flips
FAIL → PASS; the standing figure `26` is unchanged. What was wrong is the
clause cited for WHY, the premise about what exists, one attribution, and
— most importantly — that a question got opened for Ken that a standing
rule already answers.

**1 — `(bs)` is WITHDRAWN, not answered; it should never have been
opened.** The operator has twice, explicitly, removed exactly this class
of question from the engineer's queue: *"any contradictions or
ambiguities in the specs get an option in settings with the default as
YOUR best guess. do not ask me for the default as you know more about
this than I ever will"* (2026-08-19), and *"for you two questions, make
things work both ways as options. default it to your best guess as to
what would be normally expected"* (2026-08-20) — already carried as
standing rule **R169** (settings register) and its sibling **R206**.
This item fits R169's own definition exactly (see point 3 below: the
spec is genuinely, informatively ambiguous, not silent and not
determinate). See the dated closure appended to `(bs)` itself, in *Open
operator questions*, for the retraction in full and the replacement
setting. **Noted for the record because it is the interesting part, not
the embarrassing part:** the rule was in memory and was still not
applied, because the item arrived describing itself as an architectural
decision ("this changes compositing for a large class of files") rather
than as a spec ambiguity — the costume, not the rule, is what failed.

**2 — the cited clause is wrong, and so is the premise it was cited
for.** The 244th filing said §11.7.2 governs and that the individual
suite patches have **no page group**. Both wrong. **The page group
exists unconditionally.** ISO 32000-1 §11.4.7: *"All of the elements
painted directly onto a page… shall be treated as if they were contained
in a transparency group P… This group is called the page group,"*
treated as an isolated group whether or not `/Group` is present in the
page dictionary — `/Group` supplies *attributes* to an
already-mandatory group, it does not create one. **The correct
statement is that the page group's `/CS` is undeclared, never that there
is no page group** — replace "no page `/Group`" with that phrasing
wherever the 244th filing's own text used it. **The governing clauses
are §11.4.7 and §11.6.3**, each independently: *"If not otherwise
specified, the page group's colour space shall be inherited from the
native colour space of the output device."* §11.7.2 governs *declared or
inherited* group spaces and never addresses this case at all.

**3 — under ISO 32000-1, pdfce's CURRENT behaviour is CONFORMING, and
the planned change is a deliberate move off that, not a bug fix.**
**1.7 is determinate and against the change**: device-native, `shall`,
no hedge, and `/OutputIntent` does not appear anywhere in 1.7's
compositing text (measured: a ±4-line proximity scan of every "output
intent" line in the 1.7 corpus against `blend`/`composit`/`transparen`
returns exactly one hit, §8.6.5.5's ICC sentence, inspected and
excluded). §14.11.5's *"informational purposes only… free to disregard"*
for `/OutputIntent` survives verbatim into 2.0. **2.0 opens the question,
and only informatively**: §11.4.7 inherits from the device *"actual,
assumed or simulated"* and says the processor *can* choose; **Annex P
(informative)** says page groups inherit *"from the output device, or
from the output intent"* with no ranking and no condition; §11.4.7 NOTE
3 names PDF/X-4's `OutputIntent` as the *"implied default page blending
colour space."* The one body-text rung, §10.8.3 step (a), is a `should`
and selects a *colourant set*, not a blending space. ⇒ two conformant
2.0 processors can render the same file in two different blending
spaces and both cite Annex P — the textbook case for a setting, which is
why the default is the engineer's to pick and not Ken's. **Record
explicitly: if pdfce stayed on 1.7 semantics it would be conforming
today, and the Acrobat divergence would be a deliberate, disclosable
deviation to RECORD rather than repair.** The setting below moves pdfce
off that deliberately, and rule 4's off-canvas disclosure obligation
rides with it: name the blending space and its provenance on a status
line / CLI line; draw nothing on the page.

**4 — the citation I filed is right for a MINORITY of the 24-patch
population, and wrong for most of it.** PDF/X-1a and PDF/X-3 **forbid
live transparency**, so a conforming X-1a/X-3 file has no transparency
group to begin with and its overprint is an **opaque-model** question —
**§8.6.7 and Table 148**, never §11.7.4.3 / Table 149. Most of the
24-patch population is `_x3`/`_x1a`. The same n-colorant buffer is
needed either way, but the citation differs, and the citation is what
belongs in the doc comment that implements the fix — do not write the
transparency clause into opaque-model code. Two sharpenings from the
same reading:
- **§8.6.7 already prescribes today's degenerate branch**: *"It also
  shall not apply if the device's native colour space is not
  `DeviceCMYK`; in that case, source colours shall be converted… and all
  components participate in the conversion, whatever their values."*
  pdfce's current sRGB behaviour is literally what this clause names —
  **conforming but degenerate**, never "unspecified."
- **Overprint in an additive space is structurally unrepresentable, not
  merely unsimulated.** §11.7.4.3's second bullet: *"the value of
  `B(cb, cs)` shall be `cs` for all colour components specified in the
  current colour space, otherwise `cb`."* In sRGB every source colour is
  already converted to all three components, so every component is
  "specified," so `B = cs` everywhere — no shader work fixes this; only
  an n-colorant buffer does. This forecloses a whole class of
  cheaper-looking fix attempts and belongs in `Pass 122.5`'s own text.
- **§11.7.4.3's OPM-1 predicate names "the current colour space and
  group colour space," never the output device** — a `DeviceCMYK` page
  group satisfies it on an RGB display. Recorded in the corpus as
  `SP-A3`, dormant since 2026-08-08; this dispatch is the case that
  fires it.

**PDF/X CONFORMANCE — a NEGATIVE result, recorded as a negative, not a
gap.** ISO 15930 is paywalled and all three free routes failed. **No
ISO 15930 clause number is asserted anywhere in the corpus and none may
be added from recall.** From free secondary sources: no source
establishes that any PDF/X part *requires* a page group (the suite's publisher's own
conformance list accepts an *undefined* blend space — affirmative
evidence the other way), and none requires CMYK compositing; X-4
requires only *agreement* between a declared group `/CS` and the
`OutputIntent`'s colour class. For X-3: it permits a device colour space
only if the `OutputIntent`'s profile is that same space, so in a
conforming X-3 file `DeviceCMYK` and the CMYK `OutputIntent` agree **by
construction, writer-side** — which is why the convention works with no
reader-side rule at all. **Corollary for `Pass 122.5`: for the X-3
patches the decision is STRUCTURAL (how many colorants), not
colorimetric — the trap can be made correct with a nominal CMYK and no
ICC transform.**

**5 — attribution correction, a third occurrence of the same
over-claim.** Today's entries call the reference renderer "Acrobat Pro."
Adobe ships one binary and gates paid features at runtime; a window
title or folder name establishes nothing about licence tier
(`pdfce-librarian`'s own memory records this exact over-claim twice
before: 2026-08-08 from a folder name, 2026-08-18 from a window title).
What IS established: the binary self-identifies as Acrobat Pro 25.1 and
is a sound *rendering* reference; the 51 refs in
`D:\Dev\temp\acro-refs` were captured 2026-08-18 via `PrintWindow` +
`PW_RENDERFULLCONTENT`. Phrase it **"Adobe Acrobat (rendering reference;
licence tier unestablished)"** — corrected at each point it appears in
the 244th filing's own text, below.

**Deliverables — named by path, per the rule that a RAG deliverable is
not handed off until a pdfce doc names it:**
- `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__ref__page_group_absent_blending_space.md`
  (claim IDs `PGB-1`…`PGB-14`, `PGB-N1`/`PGB-N2`, `PGB-A1`…`PGB-A4`)
- `D:\Dev\Rag-Specialized\PDF_Spec\pdfx\pdfx__ref__transparency_blending_space.md`
  (new `pdfx__` prefix; `PX-1`…`PX-11`, `PX-N1`…`PX-N3`)
- Dated footers added to `iso32000__s__11.7.2.md`, `__11.4.md`,
  `__14.11.5.md`, `__8.6.7.md`, `__11.7.md`,
  `iso32000__ref__spot_colour_overprint.md`, plus that RAG's `index.md`
  and `LEGAL_NOTE.md`.

**Two open items carried forward so they are not re-derived, and not
solved twice, differently:**
- **`PGB-A2`** — *which* `OutputIntent`, when a file carries several, is
  unstated; same shape as the existing `SEP-A1`.
- **`PGB-A4`** — the answer is edition-dependent (1.7 vs. 2.0); consider
  a single `pdf_semantics_edition` knob rather than three separate ones
  when `Pass 122.5` is designed.
- **`GCS-A1`**, the prediction recorded in `iso32000__s__11.7.2.md` that
  Annex P might define colour-space equivalence, is now **answered in
  the negative** — it does not.

---

#### ★★★★★★★ CORRECTED AGAIN, LATER THE SAME DAY, 2026-08-24 (`Pass 122.4`'s ANSWER — MEASUREMENT ONLY, NO COMMIT, two-hundred-and-forty-fourth filing) — A THIRD ORACLE SETTLES THE DISPUTE: `PCS 1.1` FLIPS FAIL → PASS, STANDING MOVES TO 26, AND THE MECHANISM GENERALISES TO 24 OF 51 PATCHES

**★ SUPERSEDED 2026-08-24 (246th filing, `Pass 122.5`, `270b9d0`) — see the
top of *Shipped*.** `Pass 122.5` shipped the fix this block's mechanism
predicted: harness board `24/11/16` → `27/8/16`, hand-adjudicated minimum
`26` → `28`. Kept legible rather than rewritten, per hard rule 1.

**`Pass 122.4` asked: does the combined render disagree with the individual patch render it is assembled from? ANSWERED: YES — and the operator's original reading of the combined page was right all along.**

**Three-way measurement, `PCS 1.1`'s OPM-1 swatch, same binary, 2026-08-24:**

| render | result |
|---|---|
| pdfce, INDIVIDUAL patch `PCS1_011_Overprint-Mode_x3.pdf` | solid, filled, **contrast 18.4**, 79×78 px |
| pdfce, COMBINED document (the suite's combined X-4 file) page 1 | none detectable (a faint outline only) |
| **Adobe Acrobat** (rendering reference; licence tier unestablished), individual patch | none detectable |

⇒ **Acrobat agrees with pdfce's combined render. pdfce's individual-patch
render is the outlier, and it is wrong.** `PCS 1.1` PASSES.

**Corrected standing moves `25 → 26`.** The harness-reported figure is
UNCHANGED at **`24 pass / 11 FAIL / 16 UNRESOLVED`** — this step shipped
no code, so the harness still scores the individual-patch file exactly as
it did this morning. The correction is two hand-adjudications layered on
top of the harness's own bucketing, neither of which the harness itself
has been taught yet: `PCS 5.0` pulled out of its 16 UNRESOLVED, `PCS 1.1`
pulled out of its 11 FAIL. `24 + 1 (PCS 5.0) + 1 (PCS 1.1) = 26`.

**★ THIS SUPERSEDES `Pass 122.2`'s OWN "25" FROM EARLIER THE SAME DAY** —
see the amendment on that entry, immediately below, rather than a silent
overwrite. The 243rd filing's reasoning was sound given what it had: it
compared the individual render against the operator's own reading of the
combined one and correctly refused to pick a side without an outside
oracle. This filing supplies that oracle.

**THE MECHANISM — found in one measurement, and it is NOT an
overprint-LOGIC defect, it is a compositing-SPACE defect:**

    individual patch (_x3):  blend_space_subtractive=0   cmyk_buffer=0   (composited in sRGB)
    combined document (_X4): blend_space_subtractive=2   cmyk_buffer=1   (composited natively in ink)

`PCS1_011_Overprint-Mode_x3.pdf` is a standalone PDF/X-3 file whose page
group carries **an undeclared `/CS`** — the page group itself exists
unconditionally (ISO 32000-1 §11.4.7), corrected 2026-08-24, 245th
filing, above — so pdfce never allocates a colorant buffer and
composites `DeviceCMYK` overprint content additively, on screen. The
combined file declares a `/CS` and
gets the ink buffer. Same artwork, two different compositing spaces,
visibly different output — and Acrobat's individual-patch render shows
this does NOT happen there, so the additive path is pdfce's own defect,
not a property of the file.

**★★ THE SCALE OF IT — the reason this is the headline and not a
`PCS 1.1` footnote.** Swept all 51 suite patches for the same signature
(`overprint_requested>0` with `cmyk_buffer=0`): **24 of 51 REQUEST
overprint and receive NO colorant buffer; only 13 get one.** The 24
include **every single remaining suite FAIL and all four `MARK?`
patches**:

`PCS1_010`, `PCS1_011`, `PCS1_050`, `PCS1_082`, `PCS1_090`, `PCS1_091`,
`PCS1_150`, `PCS1_151`, `PCS1_152`, `PCS1_190`, `PCS1_191`, `PCS1_192`,
`PCS2_020`, `PCS2_030`, `PCS2_031`, `PCS2_040`, `PCS2_041`, `PCS2_080`,
`PCS2_081`, `PCS2_120`, `PCS3_132`, `PCS3_133`, `PCS3_205`, `PCS3_206`.

⇒ This board's own long-standing note — *"the ONE architectural item
that unblocks the largest cluster"* — now has a **named, measured
mechanism**, not just a cluster label. This is very likely a single fix,
not a family of per-feature ones.

**THE OPEN QUESTION — SPEC-GOVERNED, AND NOW ANSWERED BY
`pdfce-spec-librarian` (245th filing, above): it is a genuine, informative-
only ambiguity, so the answer is a SETTING per standing rule R169, not a
question for Ken.** §11.4.7/§11.6.3 (not §11.7.2, corrected above) key
the page group's blending colour space on the native device colour
space **"if not otherwise specified,"** and ISO 32000-2's Annex P
(informative) permits inheriting from the `OutputIntent` instead, with
no ranking between the two. `cmyk_buffer=0` is literally 1.7-conforming
today; a PDF/X-3 file whose entire purpose is CMYK overprint, rendered
additively, nonetheless produces output Acrobat disagrees with. **Filed
as `Pass 122.5` (Backlog).** Open operator question `(bs)` was opened
for this and is now **WITHDRAWN** — see its dated closure in *Open
operator questions* — because R169 already dictates the answer: ship
both readings as a setting, default to the best-sourced guess of normal
expectation, do not ask.

**A second, smaller correction, found while re-verifying the
artefacts.** `D:\Dev\temp\suite-out\final1.png` (dated 2026-08-17) shows
`PCS 8.2` **with check marks and without its images** — the opposite of
what pdfce produces today in both the individual and combined renders
(images present, check marks absent). **That artefact is STALE**;
date-qualify it if it is ever cited again. Re-verified `PCS 8.2` in the
combined document today: images present, no check marks, consistent with
the individual patch — `8.2` is NOT part of the combined-vs-individual
discrepancy; only the overprint-buffer population above is.

**★ METHODOLOGY FINDING, worth carrying forward: when an oracle and an
instrument disagree, the disagreement is data, not noise to be resolved
by authority.** `Pass 122.2` recorded the harness-vs-operator dispute
over `PCS 1.1` as genuinely open in both directions rather than picking
a winner — that refusal is what made it worth checking a third way hours
later. Had either side been declared right on the spot, the 24-patch
root cause would still be undiscovered.

**Owed, opened by this filing:** `Pass 122.5` (Backlog). Open operator
question `(bs)` was opened here and **withdrawn the same day, 245th
filing** — see above and its dated closure in *Open operator questions*.

---

#### ★★★★★★ CORRECTED AGAIN 2026-08-24 (`Pass 122.2`, `f6457ee`, two-hundred-and-forty-third filing) — ONE FAULT FIXED, ONE FAULT REPLACED BY AN HONEST REFUSAL, AND THE 225th FILING'S OWN FLOOR FIGURE DOES NOT REPRODUCE

**Corrected standing: `25 pass of 51`, not the 225th filing's `26`.** Harness
now reports **`24 pass / 11 FAIL / 16 UNRESOLVED`** (24+11+16=51). The
one-patch gap between the harness's `24` and the corrected `25` is `PCS 5.0`
— verified correct by hand but not yet adjudicable by the harness itself
(no detector shipped, see below), so it stays inside `MARK?`/UNRESOLVED on
the board's own count.

**Arithmetic against the prior board (`29/10/12`), so every patch is
accounted for:** one `PCS 1.0` flip (pass→FAIL) takes `29→28` / `10→11`;
four reclassified check-mark patches (formerly false `clean`, now `MARK?`)
take `28→24` / `12→16`.

**Why the 225th filing's `26 pass AT MINIMUM` does NOT reproduce.** That
figure counted `PCS 1.1` as a flip FAIL → PASS from *"the combined render
shows no cross at any contrast."* Re-examined 2026-08-24, cropped and
magnified 6×: the **individual** patch render — what this harness actually
scores — shows a solid, filled, high-contrast teal X at contrast 17.4, not
an outline. **The two observations may both be true of two different
artefacts** (the combined multi-patch page vs. the single-patch render), and
this filing does not overrule the only independent oracle this harness has
ever had. `PCS 1.1` stays FAIL. **`Pass 122.4`** (Backlog) exists to chase
whether the combined and individual renders genuinely disagree — which
would be a defect in its own right.

**★ SUPERSEDED THE SAME DAY, 2026-08-24 (244th filing) — see the
correction block above this one.** `Pass 122.4` ran the measurement this
paragraph called for and found a THIRD oracle — Adobe Acrobat (rendering
reference; licence tier unestablished), not just the harness or the
operator's own reading — and it agrees with pdfce's combined render, not
its individual one. `PCS 1.1` does not stay FAIL; it flips to PASS, and
this entry's own corrected standing (`25`) is one low as a result. The
reasoning above was sound given what it had measured — no Acrobat
reference existed yet — it simply did not have the measurement that
settled it.

**Fault 1 (contrast floor) FIXED — but the 225th filing's OWN DIAGNOSIS of
it does not hold, and that is a finding in itself.** `CONTRAST_MIN`
12.0 → 6.0, calibrated against the operator's cell readings: four "clear
fails" at 10.7/10.2/7.8/7.4, two "faint outline only" at 4.1/4.1 — an empty
interval 4.1–7.4, 6.0 in the middle. The 225th filing's prescription — *"the
floor has no area term… make the threshold a function of mark size"* —
**measured false before being implemented**: every trap on `PCS 1.0` and the
`PCS 16.0` calibration patch is the SAME pixel size (36–38 px) at this
harness's render scale. An area term would have changed nothing. **A fix
aimed at a misdiagnosed cause is more dangerous than no fix, because it
consumes the suspicion** — it would have shipped with a plausible reason to
stop looking and zero effect on the false `clean`. Net board effect: exactly
one flip, `PCS 1.0` pass → FAIL.

**Fault 2 (positive criterion) — NOT a detector. A false `clean` is removed
and replaced by an honest `MARK?` verdict, counted UNRESOLVED, never folded
into `pass` or `FAIL`.** ★ **AND THE 225th FILING'S OWN LIST WAS WRONG: FOUR
PATCHES, NOT SEVEN.** `PCS 150`/`151`/`152` were included by a `grep` for
the phrase "check mark" — but those three ReadMes state the suite's
NEGATIVE criterion and mention "check mark" only while describing what the
FAILURE CROSS is drawn out of. **A grep for a phrase finds a mention, not a
criterion.** Re-verified against Acrobat renders: **`PCS 15.0`/`15.1`/`15.2`
genuinely PASS**, discharging owed item 4 below. The harness now reads each
patch's own extracted ReadMe text at runtime, so this list cannot drift from
the corpus again.

**Ground truth for the four real check-mark patches, by hand (Acrobat
renders, 2026-08-24), since no detector ships:**

| PCS | Acrobat shows | pdfce shows | verdict |
|---|---|---|---|
| `8.2` DeviceN (4 col.) | two olive marks ~46×56 px, upper-right of each image, plus a smaller inline one | inline mark only | **FAIL** |
| `8.01` DeviceN (6 col.) | two dark-green marks on the images plus ~15 more along the spot-colour gradient bar | none | **FAIL** |
| `8.1` DeviceN (5 col.) | same family | same absence | **FAIL** |
| `5.0` Font Substitution | a black glyph, embedded modified Symbol font | rendered correctly | **PASS** |

**★★ THE TRAP THIS RAISED, CAUGHT MID-SESSION.** A first detector keyed on
the mark's OWN COLOUR (olive, from `PCS 8.2`). Run against `PCS 8.01` it
reported the mark PRESENT — matching the green stop of that patch's own
spot-colour gradient bar — while BOTH real marks were absent: a false green
produced BY the fix for a false green. **The mark's colour is not a
constant of the criterion**; a future detector must key on presence
relative to a reference render, not on a hue. Thrown away rather than
shipped. RAG:
`C:\personal_rag\pdf\lesson_20260824_check_mark_detector_must_key_on_presence_not_hue.md`.

**Owed, DISCHARGED by this Pass:** the harness repair and the re-measure.
**Owed, still:** a reference-render-based (not hue-based) detector for the
four `MARK?` patches; `Pass 122.4` for the combined-vs-individual render
question. Full record: the `Pass 122.2` *Shipped* entry, top of *Shipped*.

---

#### ★★★★★ CORRECTED 2026-08-21 (two-hundred-and-twenty-fifth filing) — **BY AN ORACLE, NOT BY A RE-MEASUREMENT, AND THAT DISTINCTION IS THE WHOLE FINDING**

**Corrected standing: `26 pass of 51` AT MINIMUM.** The harness reports
`29 / 10 / 12`. Four patches move **pass → fail** and one moves **fail →
pass**; three more are **unchecked**, which is why the corrected figure is a
floor rather than a number.

**Source:** `docs/suite-operator-review-2026-08-21.md` — the operator read the
annotated render **cell by cell**. **These are the first independent judgements
of pdfce's suite output that have ever been taken.** Engineer-owned; **this
role does not edit it** — it is the calibration set for repairing the harness.

| PCS | harness said | corrected | why |
|---|---|---|---|
| `1.0` CMYK Overprint Test | pass | **FAIL** | cells `d`,`e`,`i`,`j` carry large crosses at contrast **9.8**, below a floor with no area term |
| `1.1` CMYK Overprint Mode | FAIL, 1 cross | **PASS** | no cross at any contrast; a faint outline only |
| `19.0` DeviceN Overprint (Black) | FAIL, 1 cross | FAIL, 1 cell (`d`) | `b` is outline-only — the operation is right, the edge rounding differs |
| `19.1` DeviceN Overprint (Yellow) | FAIL, 4 crosses | FAIL, **2** cells | `a`,`c` pass; 4 was an over-count |
| `8.2` DeviceN Support (4 col.) | pass | **FAIL** | both check marks absent |
| `8.1` DeviceN Support (5 col.) | pass | **FAIL** | check marks absent |
| `8.01` DeviceN Support (6 col.) | pass | **FAIL** | check marks absent |
| `5.0` Font Substitution | pass | PASS | confirmed present and correct |
| `15.0` / `15.1` / `15.2` optional content | pass | **UNCHECKED** | same positive criterion, not on the combined pages, nobody has looked |

**TWO INSTRUMENT FAULTS.**

**(1) The harness implements ONE of the suite's TWO pass criteria.** The suite
marks failure with a **negative marker** (a cross a correct renderer makes
vanish — implemented thoroughly) **and** with a **positive marker** (*"if a
check mark is visible … then DeviceN is respected; if no check mark appears
then DeviceN colour was transformed to CMYK (= ERROR)"*). **The harness has no
notion of a mark that should be there and is not.** Seven of 51 patches score
this way — PCS `050`, `080`, `081`, `082`, `150`, `151`, `152`. ⇢ **A gate
that looks for the wrong thing does not report *"I cannot tell"*; it reports
`clean`, which is indistinguishable from a pass.**

**(2) The contrast floor has no area term.** `CONTRAST_MIN = 12.0` implements
the suite's *"a **clear** X … judged by a human at 0.5 m"* as a fixed number
**regardless of mark size**. `PCS 1.0`'s cells `d`/`i` are ~**3× the linear
size** of the calibration patch's at contrast **9.8**; box 11's cells sit at
**1.3–3.1** and genuinely are invisible. **Lowering the number drags box 11 in;
the threshold has to be a function of the mark's size.** The operator also
distinguished **outline-only** from **filled** twice by name (*"just an issue
with the layer edge"*, *"the math for the edges of the x differs slightly with
rounding"*) — a distinction the harness cannot currently express.

**★★★ WHY RE-MEASURING COULD NEVER HAVE CAUGHT THIS, which is what makes it
different from every other correction on this board.** The box's own caveat
below says a document has no access to staleness and **"only re-measuring
does"**. That is true of a **stale** figure and false of a **mis-scored** one:
every figure this project has filed came from **the harness scoring itself**,
its thresholds calibrated against **one** patch (`PCS 16.0`, 2026-08-17) whose
answer was already known from a code change. **Re-running an instrument that
agrees with itself produces the same number forever.** What was missing was an
**ORACLE**, and one arrived today from outside the machine.

**★★ SECOND OCCURRENCE, SAME INSTRUMENT, SAME NUMBER.** The 148th filing
records `suite-check.py`'s **first run reporting `29 of 51`**, corrected before
publication when **13 reference-strip patches** turned out not to be adjudicable
by an X-detector. **Four days apart; the first was caught before it was filed,
the second was published and propagated for a day.** `R210` is minted on that
pair (*Standing rules*).

**WHAT IS UNAFFECTED, so the correction is not over-read.** The
blending-colour-space census (**107 wrong → 0**) counts **blend operations**,
not traps. Patches `16.0`/`16.1`/`16.2` and 36/37 are cross-criterion and the
operator confirmed the first three read correctly. **The re-scoping fact —
every remaining FAIL is an overprint, spot or ICC patch — survives**, and the
check-mark family sharpens it: `8.2`, `8.1` and `8.01` fail for **image
overprint** (`Pass 122.1`), which is the same cluster.

**HISTORY IS NOT REWRITTEN.** The seven positive-criterion patches have scored
`clean` since the harness was written, so **no** figure below this line ever
excluded them — the `25 / 18 / 8`, `26 / 14 / 11` and `29 / 10 / 12` boards are
**all** over-counted by the same family, and **their deltas remain valid while
their levels do not**. The correction is stated once, here, and governs
everything below.

**Owed:** `Pass 122.2` (teach the harness the positive criterion and give the
floor an area term, calibrated against the table above), then **re-measure and
re-file**, then **check `15.0`/`15.1`/`15.2`**. **★ DISCHARGED 2026-08-24 —
see the correction block above this one.** Shipped in a different shape than
requested here: no area term (measured and refused, see above), `MARK?`
rather than `MISSING-MARK`, and this block's own **26 pass AT MINIMUM** does
NOT reproduce — kept below, unrewritten, as the record of what this filing
believed at the time.

---

**This box exists so the corpus figure has ONE home that is not a
per-Pass entry.** Before this filing the number lived only inside
`Pass 93.0`'s and `Pass 85.5`'s prose, which is how "22/51" ended up being
quoted three filings after it stopped being true. **Carry the denominator
every time (hard rule 10): the figure is `X of 51`, never bare `X`.**
**★ And the box did not prevent the failure it was built to prevent —
see the correction immediately below.** One home for a figure stops it
being quoted *inconsistently*; it does not stop it being *stale*, because
staleness is a relation to the world and a document has no access to
that. **Only re-measuring does.**

#### ★★★★★ RE-MEASURED 2026-08-21 (two-hundred-and-twenty-fourth filing) — A SHIP, NOT A CORRECTION, AND THE BASELINE WAS BUILT

**Standing at `HEAD` (`ff4b4bf`), from `tools/suite-check.py`. The
comparison column is a binary compiled from `06aaad3` in a git worktree —
not a number quoted from this board**, which is the discipline the 189th
filing's Half 2 made a rule of and the reason a 2-trap regression inside
`97.1e` was visible at all.

| outcome (**of 51 patches**) | baseline `06aaad3` | `97.1e` | **`97.1f` = `HEAD`** |
|---|---:|---:|---:|
| pass | 26 (51.0 %) | 28 (54.9 %) | **29 (56.9 %)** |
| FAIL | 14 (27.5 %) | 11 (21.6 %) | **10 (19.6 %)** |
| UNRESOLVED | 11 (21.6 %) | 12 (23.5 %) | **12 (23.5 %)** |
| render errors | 0 | 0 | **0** |
| trap marks, **total over the 51** | 55 | 45 | **41** |

**Arithmetic check (hard rule 10):** `26+14+11 = 28+11+12 = 29+10+12 = 51`
in every column, so no patch is unaccounted for. **Three patches changed to
`pass`** (`PCS1_162`, `PCS3_164`, `PCS1_161`) and **one moved FAIL →
UNRESOLVED** (`PCS1_1611`, having lost its last trap; strip correlation
**0.986**), which is `14 − 10 = 4` FAILs leaving and `11 → 12` UNRESOLVED
gaining one. **`PCS3_161` improved 14 → 11 traps and still FAILs.**

★★★ **THE COMPOSITION FACT, which is what this board should be read for
now: all 10 remaining FAILs are overprint, spot or ICC patches.** The
blending-colour-space cluster this board tracked since the 166th filing is
**closed** — the census went **107 of 107 wrong → 0 of 107**. The next
movement on this board comes from `Pass 85.5`'s **n-channel spot**
remainder, not from more compositing work. Full record: the
`a277931` + `ff4b4bf` entry at the top of *Shipped*.


#### ★★★★ CORRECTED 2026-08-19 (hundred-and-eighty-ninth filing) — BOTH HALVES, AND THE SECOND ONE MATTERS MORE

**Standing at `HEAD` (`0eec220`), from the harness at `972ddbb`:**

| outcome | count | of 51 |
|---|---:|---:|
| pass | **26** | **51.0 %** |
| FAIL | **14** | **27.5 %** |
| UNRESOLVED | **11** | **21.6 %** |
| render errors | **0** | **0 %** |

**Half 1 — the ship.** `Pass 85.4b` (`972ddbb`, the four non-separable
blend modes) moved **25 → 26 pass, 15 → 14 FAIL**. `PCS1_160` flipped
FAIL → ok; `PCS3_164` went 4 traps → 1; `PCS3_161` went 15 → 14.

**Half 2 — ★★ THE FIGURE THIS BOX CARRIED WAS ALREADY WRONG BEFORE THAT
SHIP.** This board read **25 / 18 / 8**. The harness, run on the
**pre-change** binary built from the previous commit **in a worktree**,
reports **25 / 15 / 11**. Neither the engineer nor this role caused it.

⇒ **THE ARITHMETIC IS THE FINDING** (hard rule 10, exactly): `18 + 8 = 26`
and `15 + 11 = 26`, and **`pass` is 25 in both.** So **no patch changed
outcome** between the filed figure and the measured one — **three patches
moved across the FAIL/UNRESOLVED boundary and nothing else moved.** That
is a **classifier** change, not a render change. It is consistent with the
167th filing's two harness findings (*"the instrument may be wrong before
the renderer is"*) and **inconsistent with any regression**. **Filed as a
reading, NOT as a verified cause** — the two figures come from different
runs at different commits and nobody has diffed the classifier. Owed item
**20**.

⇒ **AND THE PROCESS LESSON, which is why the engineer flagged it rather
than silently using his own number:** he built the baseline from the
previous commit **instead of quoting this board**, *"because comparing
against a documented number instead of a measured one is how a regression
hides."* Had he quoted **25/18/8**, the ship would have been reported as
**18 → 14 FAIL**, a −4 that never happened, and the three-patch
classifier drift would have been **absorbed into the credit for the
change** — invisible, permanently. **A/B against a rebuilt baseline is the
only form of this measurement that can be wrong in a detectable way.**

**★ WHERE THE STALE FIGURE STILL LIVES, dispositioned rather than
globbed** (the 188th filing's liveness ruling — a frozen figure in a dated
record is the record working):

| where | disposition |
|---|---|
| this board | **CORRECTED HERE** |
| `85.4b` row + `Pass 85.0–85.5` heading | **CORRECTED HERE** |
| `docs/FEATURES.md` | **CORRECTED HERE** (this role's file) |
| `docs/compositor-plan.md` §1 (*"Baseline re-measured 2026-08-18 at `e618d67`: 25 pass · 18 FAIL · 8 UNRESOLVED"*) | **REPORTED, owed item 21** — engineer-owned, and it is a **LIVE PLAN**, not a dated record: its whole §1 premise is *"16 of the 18"*, and 18 was never the FAIL count at that commit |
| `docs/NEXT_SESSION.md:217` | **REPORTED, owed item 22** — engineer-owned by rule; overwritten next session anyway |
| `ARCHITECTURE.md` §2 soft-mask cell, §12 decision 070 | **LEFT** — dated sub-entries, frozen by construction |
| every `Shipped` / `SESSION_LOG.md` occurrence | **LEFT** — append-only dated records |

**★ THE CLUSTER TABLE BELOW NO LONGER RECONCILES AND IS NOT SILENTLY
PATCHED.** It sums to **18** by cause; the FAIL count is now **14**.
`PCS1_160` has left the transparency-group cluster (5 → 4), which accounts
for one; **the other three are the FAIL→UNRESOLVED reclassification, and
nobody has said WHICH three.** Re-clustering from the harness is owed item
**20** and belongs with the classifier question, because guessing which
three left would manufacture exactly the kind of consistent-looking wrong
set hard rule 10 exists to expose. **Read the cluster table as *the last
clustering anyone actually ran*, dated 2026-08-18, not as current.**

**Previous standing, for the delta:** 22 / 21 / 8 (43.1 %) at `Pass 93.0`
(`a342354`). **Acrobat Pro's own score on the same 51 is 1 FAIL**
(`PCS 16.1`, `ICCBasedRGB`) — the ceiling is **50, not 51**, and pdfce is
**26 of a reachable 50 (52.0 %)**.

<details><summary>★ SUPERSEDED — the figure this box carried from 2026-08-18 to 2026-08-19, kept legible rather than rewritten</summary>

**Standing at `HEAD` (`ac15158`) — RELAYED from the engineer's own
measurement against Acrobat Pro reference strips, not re-measured here:**

| outcome | count | of 51 |
|---|---:|---:|
| pass | **25** | **49.0 %** |
| FAIL | **18** | **35.3 %** |
| UNRESOLVED | **8** | **15.7 %** |
| render errors | **0** | **0 %** |

**The `18 / 8` split is the half that was wrong; `25 pass` was right.**

</details>

#### History of this board, kept in place — every line below was true when written and is dated for that reason

**Standing at `Pass 85.5` (`ac15158`, 2026-08-18):** 25 / 18 / 8 as filed
— **the `18 / 8` half is now known to have been a classifier artefact**,
see the correction above. **Previous standing before that:** 22 / 21 / 8
(43.1 %) at `Pass 93.0` (`a342354`); **Δ at `85.5`: +3 pass, −3 FAIL,
unresolved unchanged.**

**★ RE-CONFIRMED UNCHANGED 2026-08-18 (hundred-and-sixty-seventh filing,
`cb20770`, soft masks).** The board is **still 25 / 18 / 8 of 51.** That is
not an oversight in this filing — it is the commit's own reported result.
**Strip correlation moved on all three measurable soft-mask patches**
(`PCS1_1610` 0.515 → 0.575, `PCS1_168` 0.661 → 0.725, `PCS1_169` 0.884 →
0.905, against reference-engine 0.966 / 0.981 / 0.983) **and none of them
crossed the pass threshold.** **What changed is the CHARACTER of the
soft-mask cluster, not its count: those 4 FAILs are now PARTIAL rather than
untouched**, exactly as the overprint cluster became partial at `bf75351`
— and the distinction matters for scoping, because "4 FAILs" reads the same
either way. **A correlation figure and a pass count are the same fact in
two forms** (hard rule 10): quote the correlations for soft masks, not the
board.

**The 18 remaining FAILs, clustered by cause (RELAYED)** — ⚠ **DATED
2026-08-18 AND NO LONGER RECONCILING: the FAIL count is now 14. Do not
quote this table as current** (owed item **20**). `PCS1_160` has left the
transparency-group row (5 → 4); the other three departures are the
unexplained FAIL→UNRESOLVED reclassification and are **not** attributed
here, because attributing them without re-running the harness would
manufacture a consistent-looking wrong set:

| cluster | patches | named |
|---|---:|---|
| overprint | **7** | `PCS011`, `PCS190`, `PCS191`, `PCS192`, `PCS020`, `PCS030`, `PCS040` |
| transparency groups | **5** | `PCS1_160`, `PCS1_161`, `PCS1_162`, `PCS3_161`, `PCS3_164` |
| soft masks | **4** *(PARTIAL since `cb20770` — moved, none passing)* | `PCS1610`, `PCS1611`, `PCS168`, `PCS169` |
| shading | **1** | `PCS060` |
| ICC | **1** | `PCS130` |
| **total** | **18** | **= the FAIL count above** |

**★ ONE DISCREPANCY, FLAGGED RATHER THAN SILENTLY RESOLVED — for the
engineer to confirm.** The dispatch labelled the overprint cluster **"6"**
and then **named seven patches**. Seven is what makes the clusters sum to
the FAIL total (**7+5+4+1+1 = 18**); six would sum to 17 and contradict the
headline. **This filing records SEVEN**, on the arithmetic, and flags it
here because **the arithmetic is the only reason to prefer one over the
other** — the names were not re-verified against the harness output.
This is hard rule 10 doing exactly its job: the cluster table and the
outcome table are the same fact in two forms, so the set-property became a
single-claim property and one division caught it.

**★★ THE ONE ARCHITECTURAL ITEM, and it is now a MEASURED requirement
rather than a guess: a REAL n-CHANNEL BUFFER — one plate per colorant, RGB
synthesised only at display.** It is what the **7-patch overprint cluster**
needs, and plausibly what part of the 5-patch transparency-group cluster
needs too (§3's obligation (2): blending in a `DeviceCMYK` blending colour
space, which the suite patches declare). **DO NOT RE-ATTEMPT THE CHEAP
VERSION.** A page-sized spot-ink multiplier plate was built and ablated in
`ac15158`: **−1 trap of 17, 0 patches of 51 flipped, and `PCS2_030`
regressed 3 → 6 unexplained.** Working copies are kept **outside the tree**
so the next attempt starts from that measurement. Full account: the
`ac15158` Shipped entry and the rewritten `85.5` row, below.

**★★★ THE SOURCING RECORD FOR IT IS NOW ON DISK AND IS NAMED HERE SO IT IS
FINDABLE FROM THE ROADMAP, NOT ONLY FROM `git log`:
`docs/overprint-architecture-survey.md`** (`8eb0668`, 2026-08-18). **The
n-channel buffer is no longer a preference, it is the DECIDED next
architecture, on THREE INDEPENDENT CONFIRMATIONS**: (1) pdfce's own
ablation (`ac15158` — the spot-multiplier plate, built, measured, reverted);
(2) a **seven-engine** research survey finding unanimous convergence on one
plane per colorant, with **no published alternative and no published
approximation that covers spot colorants**, and a peer-reviewed Artifex
paper stating that collapsing colour before compositing *"is not possible"*
specifically because of overprint; (3) **pdfce's own spec RAG's 2026-08-08
"stage 10" note**, which said the same thing before either of the other two
existed. The survey also records the **unstandardised final-collapse step**
(vendors disagree materially; Acrobat does not document its method — so it
is **settings-shaped**, per the operator's "make spec ambiguity a setting"
rule), three places pdfce can **EXCEED** Ghostscript and Poppler, and the
binding constraint on all follow-up: **every engine surveyed is GPL/AGPL
and stays a BEHAVIOURAL reference only** (`LEGAL.md` §6.1, project rule 8).
**A third of that evidence was already inside this project and had not been
connected to the other two** — which is the reason the pointer is written
into the roadmap rather than left in the commit.

**No Pass ID minted for it here** — next free Pass family is **97**, and
the engineer assigns. The work is carried on the `85.5` row until it is
scoped.


<!-- Pass 81.1 -->
### ~~Pass 81.1~~ — **SHIPPED 2026-08-27, commit `4eaea20`. See the `Pass 81.1` entry at the top of *Shipped*.** Retained below as the scoping record, **and it must be read with the banner**: this entry's own heading and its acceptance criterion 1 name a carrier the Pass did **not** use

> **★★★ STATUS BANNER — READ BEFORE ACTING ON ANYTHING BELOW.**
> **`Pass 81.1` is SHIPPED**, and it shipped in a **different shape** from
> the one specified here. **This entry says `opacity` on `MarkupSpec`.
> What shipped is `MarkupOptions`, a separate options struct**, passed to
> two new verbs `add_markup_with` / `add_text_annotation_with`. The
> divergence is deliberate and is argued in full in the *Shipped* entry:
> `MarkupSpec` is an enum of **eight** geometric variants (so the field
> would be eight copies), and — decisively — `MarkupSpec` describes what
> the **appearance draws**, whereas §12.5.2 Table 164's `/CA` is the alpha
> with which the **annotation is composited onto the page**, which is a
> whole-annotation property and not a geometric one.
>
> **Two further things below are narrower than what shipped.** This entry
> scopes **geometric markup only**; the Pass shipped **both authoring
> routes**, because Table 164 is the *markup-annotation* entry list and a
> sticky note is a markup annotation. And the refuse-vs-clamp asymmetry
> this entry records as an observation is now **decision 092**.
>
> **What below is UNCHANGED and still binding:** the `/CA` ALONE
> specification, the Highlight-`ExtGState` warning, the `None`-omits-the-key
> rule, and the refusal (rather than clamp) on an out-of-range value. All
> four shipped exactly as written here.
>
> **★ One CITATION below is wrong and is corrected here rather than left
> to propagate.** This entry twice attributes the no-clamp ruling to
> **`R27`** — *"(R27: a clamp silently produces a document the caller did
> not ask for)"*. **`R27` does not say that.** Read from *Standing rules*
> this filing, `R27` is *"Unsupported codec sub-features fail clean and
> are counted BY NAME"* — a **decoder** rule. The shared kernel (*fail by
> name, never substitute a guessed value*) is real, which is why the
> citation felt right, but the ruling's actual home is **decision 092**
> (`ARCHITECTURE.md` §12), minted this filing precisely because the
> principle had none and was being borrowed from the nearest rule that
> sounded like it.

**What the consumer asked for:** an `opacity` field on `MarkupSpec` so a
markup can be authored at reduced opacity — §12.5.2 Table 164's `/CA`,
Optional, **default 1.0**, a number in `0.0..=1.0`.

**`/CA` ALONE, and the word *alone* is the specification.** No `ExtGState`
is added to the appearance stream, no `/ca` is written, no blend mode is
touched. `/CA` applies to the annotation's appearance **as a group**,
which is precisely the semantics wanted, and it is **one key**.

#### ★ HIGHLIGHT'S `ExtGState` MUST NOT BE GENERALISED FOR THIS

pdfce's Highlight markup already writes an `ExtGState` into its appearance
stream. **It is there for the MULTIPLY BLEND MODE** — that is what makes a
highlight tint the text beneath it instead of covering it — and blend mode
and constant alpha are **different properties that happen to live in the
same dictionary.**

**Reusing that code path for opacity would be a plausible-looking mistake
with two consequences:** it would put a blend mode on shapes that must not
have one (a Square at 50 % opacity is *not* a multiply-blended Square), and
it would make the opacity **operation-scoped** inside the stream rather
than **annotation-scoped**, which is a different rendered result wherever
the appearance overlaps itself. **Recorded here because the shortcut is
right there and looks like reuse.**

#### Ordering — **SATISFIED 2026-08-14: `Pass 81.0` SHIPPED (`a84bdc3`) hours after this entry was filed**

**`Pass 81.0` first**, and it landed first. Until `pdfce-render` read
annotation `/CA`, this Pass would have produced files that **look right in
Acrobat and wrong in pdfce** — the shell penalised in its own viewer for
writing the correct thing. **That constraint is now discharged**; this
Pass is free to proceed whenever it is picked up.

#### ★ THE READ PATH CLAMPS AND THE WRITE PATH MUST REFUSE — that asymmetry is DELIBERATE, not a contradiction

`Pass 81.0` **clamps** an out-of-range `/CA` to `0.0..=1.0` rather than
refusing, on the stated ground that *"a producer writing `1.5` means
opaque, and refusing to place the annotation to defend a range check would
lose content."* **That is the right rule for reading someone else's
file. It is the wrong rule for writing pdfce's own** — a caller passing
`1.5` has a bug, and silently clamping it produces a document the caller
did not ask for (R27). **Lenient in what it accepts, strict in what it
emits**; recorded here because a reader who sees the clamp on the read
side will otherwise "fix" the refusal on the write side.

**Also inherited from `81.0`, and it constrains this Pass:** the model
stores `Option<f64>` precisely so that **absent** and **explicitly 1.0**
stay distinguishable through a round-trip. **A writer that normalises
either into the other breaks that**, which is why acceptance criterion 1
below omits the key for `None` rather than writing `1.0`.

#### Acceptance

1. `opacity: Option<f64>` on `MarkupSpec`'s variants; `None` **omits
   `/CA`** (which is exactly `1.0`, so writing it would be a no-op key —
   §5's *never normalize*).
2. **Refuse by name**, do not clamp, on a value outside `0.0..=1.0` (R27:
   a clamp silently produces a document the caller did not ask for).
3. **No `ExtGState` is written** — assert it against the saved appearance
   stream bytes, so the Highlight shortcut cannot be taken later without a
   test failing.
4. Highlight's existing multiply-blend `ExtGState` is **unchanged**, and a
   test pins that a Highlight authored with `opacity` carries **both** —
   its blend `ExtGState` **and** an annotation-level `/CA`.
5. A **render** test proving the written `/CA` is honoured by
   `Pass 81.0`'s reader — the two halves must be checked against each
   other, not each against its own expectation.
6. `pdfce-cli`'s markup subcommands gain `--opacity` (rule 11).
7. `cargo fmt --check`, `cargo clippy -- -D warnings` clean.

**Terminology (rule 15):** nothing here touches **ce dimensions** or **pdf
dimensions**.

> ### ★ `Pass 75.0` — **SHIPPED 2026-08-18** (`e13f8ed` + `6af5655` + `6b797db`).
> The reusable parsed handle (display list) is **no longer *Next up***; its
> full entry — measurements, the seven acceptance criteria one by one, the
> two engineer divergences, `MAX_DISPLAY_LIST_BYTES`, and the poster-printing
> bug found while confirming the CLI column — is at the top of ***Shipped***.
> **This pointer exists because 27 lines across three documents cite
> `Pass 75.0`**, several in append-only history, and a reader arriving from
> one of them at *Next up* would otherwise conclude it was never built.
> The number-collision note (`d24c1df`'s message calls itself `Pass 75.0`
> and is **VOID**) travels with the Shipped entry.

---


<!-- Pass 71.0 -->
### Pass 71.0 — **OCR**, promoted from the Backlog bucket 2026-08-12 (hundred-and-twenty-sixth filing) on the operator's engine decision — **SLICES 1–4 SHIPPED (`9f2af1d` types, `ed05033` the sandwich WRITER, `49af8fb` the `ocrs` ENGINE, `4b82641` + `40c377a` the END-TO-END PROOF and the WEIGHTS); the PIPELINE IS COMPLETE END TO END IN CORE, THE WEIGHTS NOW SHIP, AND STILL NOTHING IS OPERATOR-REACHABLE, because there is no `pdfce-cli ocr` and no GUI surface** — **★ NO LONGER BLOCKED ON AN OPERATOR DECISION as of 2026-08-13: `(bl)` IS ANSWERED YES**

> **★★★ SLICE 4 SHIPPED 2026-08-13 (hundred-and-forty-sixth filing) —
> `4b82641` the smoke harness, `40c377a` the weights. THIS HEADING'S
> PREVIOUS TEXT HAS BEEN CORRECTED: it read *"the MODEL WEIGHTS ARE NOT
> IN THE REPOSITORY"*, which `40c377a` falsified.** The two `.rten` files
> are committed at `crates/pdfce-core/assets/models/ocrs/`,
> **12,240,008 B over 2 files = 6.12 MB each**, **CC-BY-SA-4.0**,
> SHA-256-pinned, with a hand-authored `PROVENANCE.md` **and** an
> `about.hbs` entry so the licence reaches binary recipients and not only
> repository readers. **A build can now recognise text.** Full record,
> including the gate that caught the missing `about.hbs` citation and the
> false positive retracted by arithmetic: **top of *Shipped*.**
>
> **★★ WHAT SLICE 4 DOES NOT ESTABLISH — do not read it wider.**
> **RECOGNITION QUALITY IS UNPROVEN.** Both documents available are the
> **wrong input** — vector PDFs that already contain text, which are
> out-of-distribution in the **opposite** direction from a bad scan.
> **62 words returned is a COUNT, not an accuracy result**, and measured
> output on both documents was **poor**. **A real quality claim needs a
> genuine scanned page and this project has no rights-cleared one**
> (`LEGAL.md` §5, rule 7).
>
> **`FEATURES.md` STILL `core [ ] · cli [ ] · gui [ ]`** — the row's
> *sentence* changed because it asserted the weights were absent; **no
> box moved**, because nothing is operator-reachable.
>
> **★ STILL OWED ON THIS PASS, and item 1 is a direct instruction from
> the operator that has not been carried out:** the operator said
> *"do both"* — bundle the weights **AND** build the **downloader**.
> **The downloader has not landed** (`git log -S download` over
> `crates/` since 2026-08-13: zero hits). Then `pdfce-cli ocr`.

> **★★★ SLICE 3 SHIPPED 2026-08-13 (hundred-and-forty-second filing) —
> `49af8fb`, `pdfce_core::ocr::engine_ocrs`.** `OcrsEngine` implements
> `OcrEngine`, behind the Cargo feature **`ocrs`** (named after the
> **crate**, not the capability, so the second engine lands as a sibling
> without a rename), **default ON**, forwarded from **every** shell
> including `pdfce-gui`'s manifest. **20 crates added, ZERO copyleft**
> (`ocrs` + the 11 `rten` crates are MIT OR Apache-2.0; `flatbuffers`
> beneath them is Apache-2.0 only), `THIRD_PARTY_LICENSES.md` regenerated
> via `cargo-about`. **The wasm32 gate was VERIFIED empirically at
> adoption, not cited from the survey.** **3,690 tests, +2.**
>
> **★ IT REPORTS NO CONFIDENCE AT ALL** — `ocrs`'s output type is a char
> and a rectangle, so `reports_confidence()` returns **`false`**, and
> **that is a fact about the world, not a stub.** The first real
> implementation of the trait landed on the side a convenience default
> would have got wrong. Full record: top-of-*Shipped*.
>
> **★★ THIS SLICE MOVED NO `FEATURES.md` BOX EITHER, AND THE REASON IS
> THE REMAINING WORK:** the **model weights are not in the repository**,
> so **no build can recognise text**. `from_model_dir` compiles, runs and
> returns a named **`ModelMissing`** naming the file and the directory —
> a clean refusal, not a stub.
>
> **★ WHAT IS ACTUALLY LEFT ON THIS PASS, in order:**
>
> 1. **The WEIGHTS — and the open item is the COMMIT, not the licence.**
>    `(bl)` is answered **YES** and must not be re-raised. What has
>    **not** been put to the operator is that **committing ~12 MB of
>    `.rten` binary into a PUBLIC repository's history is permanent**.
>    The engineer raised it in the 2026-08-13 session summary as a
>    heads-up. **Until he responds, the files are neither authorised nor
>    forbidden — they have not been asked about.**
>    **★ DISCHARGED 2026-08-13 (hundred-and-forty-sixth filing), by the
>    operator: *"Yes put the OCR in"*, then *"do both"*.** He was told
>    plainly that a 12 MB binary in a public repository's history is
>    **permanent** and asked for it anyway. **Shipped as `40c377a`.** The
>    *"do both"* half — **the downloader — is still owed.** The four
>    carve-outs below were all honoured: pinned + SHA-256'd, hand-authored
>    `PROVENANCE.md`, no adaptation performed or cleared, and publishing
>    remains an operator act. The four carve-outs
>    below still bind (pin + hash the exact artifact, hand-authored
>    `PROVENANCE.md`, no clearance for an adaptation, publishing is still
>    an operator act).
> 2. **`pdfce-cli ocr`** (rule 11). This is what makes the capability
>    reachable from a shell at all, and it is the box that moves first.
> 3. **The second engine** — `ocr-rs`/PaddleOCR, Apache-2.0, **50+
>    languages**, which is the operator's own stated ranking criterion.
>    **No WASM**, so it is a sibling feature the wasm32 build omits, never
>    a replacement.
> 4. **The review surface (rule 4, as narrowed by decision 059)** —
>    **off-canvas**, and on this engine it must state that *nothing was
>    scored*, which is the hardest case rule 4 has met.
> 5. **Language selection**, a property of the engine chosen at build
>    time as well as at run time.

> **★★ SLICE 2 SHIPPED 2026-08-13 (hundred-and-forty-first filing) —
> `ed05033`, `pdfce_core::ocr::layer`.** `build_layer_content` (pure) and
> `add_ocr_layer` (incremental save): ISO 32000-1 §9.3.6 Table 106 **mode
> 3** invisible text, `q…Q`-wrapped, one `BT…ET` per page, vertical fit by
> font size against Helvetica's real **0.718/0.207** AFM metrics,
> horizontal fit by **`Tz`** (§9.3.4, a **percentage**). **Additive only** —
> one appended content stream, one Standard-14 font dict, one rewritten
> page dict; **the scan is never re-encoded**, and a test asserts the input
> file is a **byte prefix** of the output. **3,688 tests, +21.**
>
> **This slice did NOT move any `FEATURES.md` box, deliberately.** A
> caller must **supply** the recognised words; a writer nobody can feed is
> not a capability (`R151`). **What slice 3 changes is exactly that.**
>
> **★ SLICE 3 IS ALREADY IN FLIGHT IN THE WORKING TREE — measured by
> `git status --short` in this dispatch, not inferred.** Uncommitted at
> filing time: **`crates/pdfce-core/src/ocr/engine_ocrs.rs` (untracked)**,
> plus modified `Cargo.toml` on **all four crates** and **+195 lines of
> `Cargo.lock`**. **Nothing about its state is claimed here beyond the
> file list** — not that it compiles, not that it is tested, not that it
> is nearly done. **It is named so a resuming session does not start it
> twice.**
>
> **Slice 3, the remaining work, is ENGINEERING with no decision in front
> of it:** bind `ocrs` behind a Cargo feature (`docs/ocr-engine-survey.md`,
> `NEXT_SESSION.md` §1), ship + **hash** + **attribute** the weights, build
> the rule-4 / decision-059 **off-canvas** review surface, and ship
> `pdfce-cli ocr`. **The four carve-outs below still bind** — `(bl)`
> removed the licence obstacle and nothing else.

> **★★ THE LICENCE BLOCK IS LIFTED — 2026-08-13 (hundred-and-thirty-sixth
> filing). Operator, verbatim and in full:**
>
> > *"yes to the license. keep going."*
>
> He was answering **`(bl)`** as this project has been carrying it — *may
> a **CC-BY-SA-4.0** model file ship inside pdfce's **MIT** single-folder
> portable distribution?* **YES.** That is the whole of what he said and
> it is recorded at that length.
>
> **What remains on this Pass is ENGINEERING, not a decision:** bind the
> engine behind a Cargo feature, ship + **hash** + **attribute** the
> weights, build the rule-4 review surface, and ship `pdfce-cli ocr`.
>
> **★ FOUR THINGS THE ANSWER DOES NOT DECIDE — carried HERE, not only in
> the questions list, because a reader who lands on `Pass 71.0` is the
> one who needs them:**
>
> 1. **Not authority to publish or release.** Project rule 8 is
>    untouched — pushing and releasing stay separate operator acts. **The
>    repository is public**, so a bundled weight file is *published* the
>    moment it is committed.
> 2. **Not an engine choice.** That was the separate 2026-08-12 answer
>    (*"…or heck, just build for both"*) → **both engines, behind Cargo
>    features**, ranked on multi-language coverage. `(bl)` removes the
>    licence obstacle in front of **the pure-Rust one only**.
> 3. **Not clearance for an ADAPTATION.** Survey §3.3: fine-tuning,
>    quantizing, retraining, or **converting the weights to another
>    runtime's format** plausibly creates **Adapted Material**, which must
>    then be released under CC-BY-SA-4.0 or a compatible licence. That
>    binds the **derived model**, not pdfce's source. **A future Pass that
>    touches the weights owes its OWN operator decision.**
> 4. **Not the end of the attribution obligation — its BEGINNING.**
>    `cargo-about` reads the **Cargo dependency graph**; a model file is
>    not a Cargo dependency, so it **will not be seen, will not be
>    attributed, and nothing will fail**. The compliance artifact is
>    authored by hand: `PROVENANCE.md` naming the licence + a citation in
>    `about.hbs`, which is what `tools/check-shipped-assets.py`
>    (`e3fb7e0`) already enforces. **Enforcement is not acceptance.**
>
> **★ PIN AND HASH THE EXACT ARTIFACT.** `ocrs-models` has **no LICENSE
> file** (the CC-BY-SA declaration lives only on the Hugging Face card),
> and the two channels are **not byte-identical**: detection **2,510,284 B
> (S3)** vs **2,523,564 B (HF)** = **13,280 B smaller**; recognition
> **9,716,568 B (S3)** vs **9,716,444 B (HF)** = **124 B larger**, under
> **different filenames**, totals agreeing to within 0.1% (12.23 vs
> 12.24 MB over 2 files). **"The ocrs models" is not one thing.**
>
> Licensing mirror: `LEGAL.md` §6.7. Question mirror: *Open operator
> questions* → `(bl)`.

**Operator's decision, verbatim, and it is what promoted the bucket:**

> *"use whichever one is best for everyone including other languages, or
> heck, just build for both."*

**Read as: BOTH engines, behind Cargo features, with multi-language coverage
as the stated priority.** The features mechanism that makes "both" affordable
shipped **one commit earlier** as `Pass 70.0` (`fbcb946`) — see the
top-of-*Shipped* entry.

**Bucket history.** Created at project bootstrap as *"OCR —
recognize-text-in-scanned-page. Needs a decision on OCR engine binding"*.
**That decision is now made** for the *engine* half. **The LICENCE half is
not** — see open operator question **`(bl)`**. The bucket text is retained
below in *Backlog* as the scoping record, per append-only discipline.

> **★ AMENDED 2026-08-13 (hundred-and-thirty-sixth filing): the licence
> half IS now made too.** `(bl)` is **ANSWERED YES** — *"yes to the
> license. keep going."* **Both halves of the original bucket's "needs a
> decision on OCR engine binding" are therefore closed**, one on
> 2026-08-12 (which engine) and one on 2026-08-13 (may its weights
> ship). The sentence above is left standing because it was true at its
> date; this marker is the correction.

#### Slice 1 — SHIPPED (`9f2af1d`)

`crates/pdfce-core/src/ocr/` — recognised words with page positions become an
**invisible, selectable text layer over an untouched scan**. Sourced to
**ISO 32000-1 §9.3.6 Table 106 mode 3** (*"neither fill nor stroke text
(invisible)"*), which the spec corpus names as the OCR mechanism **by name**.
`ContentBuilder::set_render_mode` added. The **y-flip has exactly one home**
(`words_to_page_space`). **`confidence: Option<f32>`, with `None` load-
bearing**: an unscored word counts as needing review exactly like a
low-scored one, and no confidence yields `None`, never `0.0`. Full record:
top-of-*Shipped*, item 4.

#### Slices NOT built — this is the whole rest of the capability

1. **Bind an engine.** Both, per the operator: `ocrs`/`rten` (pure Rust,
   **the only wasm32-passing route**, Latin-only, **CC-BY-SA-4.0 weights**,
   **no confidence at any level**) and `ocr-rs`/PaddleOCR-on-MNN
   (**50+ languages**, **Apache-2.0 weights**, 3.2 MB models, zero DLLs on
   Windows MSVC, **no WASM**). Each behind its own default-OFF Cargo
   feature, following `Pass 70.0`'s convention — **including a forwarding
   block in EVERY shell**, because forgetting one removes a capability
   without breaking the build.
2. **Model FILES — ★ RESHAPED BY `af5580e`, SAME FILING.** This slice was
   written as *"ship the model files, BLOCKED on `(bl)`"*. **`af5580e`
   changed the shape of the problem rather than solving it:
   `ocr::models` (`crates/pdfce-core/src/ocr/models.rs`) makes pdfce
   **LOOK FOR** operator-supplied models — a named path first, then
   `models/<engine>` beside the executable, then user data — **exactly
   the `--font-dir` pattern already in the codebase**, instead of pdfce
   **SHIPPING** them or downloading them.
   - **A named path that does not exist is an ERROR, never a silent
     fallback** — a fallback would run a *different* model while
     reporting success, and **the output is text either way, so nobody
     could tell.**
   - **When nothing is found, every searched path is printed.** *"Models
     not found"* is unactionable.
   - **Per-engine directories**, because the two engines' weights carry
     **different licences** and a merged folder would force the
     `PROVENANCE.md` that `check-shipped-assets.py` requires to describe
     a mixture.
   - **This NARROWS `(bl)` but does not close it.** An operator who
     supplies his own CC-BY-SA-4.0 weights is not pdfce redistributing
     them; **the question remains live for any build that BUNDLES a model
     set**, which is still the only way OCR works out of the box.
     **★ CLOSED 2026-08-13 (hundred-and-thirty-sixth filing): the bundled
     case is PERMITTED — `(bl)` answered YES.** The resolver is **not
     superseded by that answer**: a named path, then `models/<engine>`
     beside the executable, then user data remains the lookup order, and
     an operator-supplied model set remains the path that requires no
     redistribution at all. **What changes is that a build MAY now ship
     the files into that first-party directory**, so OCR can work out of
     the box.
   `tools/check-shipped-assets.py` (`e3fb7e0`) will refuse a model
   directory whose `PROVENANCE.md` states no terms; **that is enforcement,
   not permission** — **and as of 2026-08-13 the permission exists
   separately, which is precisely what makes the enforcement useful
   rather than moot.**
   > **★ AMENDED 2026-08-13 (hundred-and-forty-fourth filing, decision
   > 061) — A FETCH ROUTE IS NOW PERMITTED, AND THE `af5580e` WITHDRAWAL
   > WAS MADE UNDER A READING OF `R12` THAT NO LONGER HOLDS.** `R12` is
   > narrowed to the engine only, so a downloader may live in the
   > shells. **This does not supersede the resolver above** — a named
   > path, then `models/<engine>` beside the executable, then user data
   > remains the lookup order, and **fetch fills that first-party
   > directory rather than becoming a fourth lookup step.** The
   > downloader's shape is fixed by decision 061: the sibling crate
   > **`pdfce-fetch`** (pinned URL + SHA-256, **not built**), depended on
   > **optionally** by each shell, **never** by `pdfce-core`. See the
   > *Backlog* entry *"`pdfce-fetch`: THE ONE FETCH PRIMITIVE"* for the
   > full constraint table. **Note the second-order effect the operator
   > raised earlier and which this answers more completely than the
   > packaging argument did:** with a downloader permitted, the weights
   > need **neither a git commit nor bundling**.
3. **The review surface (rule 4).** OCR output is an inference on **every
   word**. The needs-review set must be visible and rejectable before it
   becomes document state, and **the uncertainty must be STATED where the
   engine cannot score** — a Latin-only engine with no confidence is the
   hardest case rule 4 has met so far.
4. **`pdfce-cli ocr`** (rule 11, same Pass as the GUI flow).
5. **Language selection**, which is the operator's stated priority and is a
   property of the engine chosen at build time, not only at run time.

**Read `docs/ocr-engine-survey.md` FIRST** — 116,991 bytes, written
2026-08-12, the sourcing record for every claim above, and **named here
because a research deliverable is not handed off until a pdfce doc names
it.** **`Surya` is recorded there as a trap** (modified Open RAIL-M weights,
$5 M revenue cap — field-of-use restrictions cannot be bundled in an MIT
app); do not re-evaluate it on its accuracy numbers.

**Behavioural reference:** OCRmyPDF's "sandwich" approach (MPL-2.0,
study-only — `PRIOR_ART.md`). **Empirical finding already captured:**
`C:\personal_rag\pdf\lesson_20260812_ocr_text_layer_bt_et_per_line_poppler_tz.md`.

---
