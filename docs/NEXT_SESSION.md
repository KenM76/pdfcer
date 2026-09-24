# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-12, after `Pass 300.3` and the 530th filing.
**Amended:** 2026-09-23, after `Pass 326.2` and the 588th filing; **2 commits UNPUSHED** — see the 2026-09-23 **SINCE THE LAST HANDOFF** block at the top of STATE.
**Earlier amendment:** 2026-09-17, after `Pass 309.0`/`309.1` and `Pass 310.0`/`310.1`/`310.2`, filings to the 571st, **`v0.55.0` released**. See the newest **SINCE THE LAST HANDOFF** block at the top of STATE.
**Previously amended:** 2026-09-15 (again), after `Pass 307.0`, `Pass 308.0`/`308.2`, `Pass 308.1` — with which **`G020` is fully closed** — and `Pass 308.3` answering the new `G021`. Filings to the 560th. See **SINCE THE LAST HANDOFF** at the top of STATE —
everything below that block is carried forward unchanged and still true.

---

## READ THIS PARAGRAPH FIRST — RUN THE GATE SWEEP BEFORE YOU PUSH

`tools/run-gates.sh` was treated as a **release** gate. It is not; it is a
**push** gate, and this session paid for the difference.

`main` had been **red on GitHub since 20:18Z**, from a commit pushed earlier
the same day by a session that did not read CI's colour afterwards. The cause
was a string literal with a baked-in run of spaces — a lost line-continuation
backslash. Nothing about it could fail a test. Running the sweep before pushing
found it, plus two more:

| gate | what it found | how long it had been live |
|---|---|---|
| `check-public-fns-documented.py` | `preview_style_resolution` had **no doc comment** — a splice welded its 38-line doc block onto the function inserted above it | 1 day |
| `check-string-gaps.sh` | two literals with a lost backslash | 1 day / same day |
| `check-ci-job-names.py` | the `audits` job said `(20 checks)` and ran 21 | same day |

**None would ever have failed a test, been caught by clippy, or looked wrong in
a diff** — a doc block welded to the wrong function reads as correct, because
both functions have docs.

So: **sweep, then push, then read CI's colour from GitHub.** Rule 8 already
says to read the colour; it does not yet say to sweep, and that is the gap this
paragraph exists to close.

**AND SWEEP AFTER YOUR LAST EDIT, not merely before pushing** — added
2026-09-12, after the distinction cost a red CI run. The first two string gaps
were found by a sweep, fixed, and pushed. Then more code was written, the sweep
was not re-run, and the third gap reached `origin` and turned CI red.
**A gate run before your last edit is a gate that did not run.** The sweep is
seconds; the discipline is running it against the tree you are actually
pushing.

### How to run it on this machine, because the obvious way gets killed

**`run-gates.sh` and `cargo test --workspace --all-features` are both
OOM-killed here**, repeatedly, including per-crate. Three watchers and two
sweeps died on 2026-09-12.

⚠ **AMENDED 2026-09-14 — this is a TENDENCY, not a certainty, and reading it
as a certainty costs a sweep you could have had.** On 2026-09-14 the plain
`bash tools/run-gates.sh` ran **all 34 commands to completion** on this
machine, as did `cargo test --workspace` (≈7 min, the `pdfcer-cli` integration
binary alone 415 s) and `cargo clippy --workspace --all-targets -- -D warnings`.
Nothing was killed. What is *reliably* fatal is still **release-linking
`pdfcer-cli`**, which is a different act from testing it in debug.

⇒ **Try the sweep. If it dies, fall back to the split procedure below** — do
not start from the fallback on the strength of this paragraph, and do not
delete the paragraph either, because the failures it records were real. A
session that skips the sweep on the authority of a sentence written two days
earlier is exactly the failure the paragraph above it is about.

The fallback procedure, when it is needed:

1. Run the **23 non-cargo gates in one loop** — they are seconds each. Get the
   list from `python tools/check-ci-parity.py --list`.
2. Run the **cargo gates one at a time, in the background, serially**:
   `fmt --check`, `clippy --workspace --all-targets`, `clippy --all-features`,
   `test --workspace`, `test -p pdfcer-core --no-default-features`,
   `check --target wasm32-unknown-unknown`, `cd fuzz && cargo check --bins`.
3. **Do not hold `gh run watch` open** — it is what died most often. Poll
   `gh run list --branch main --limit 1` on a wakeup instead.

**THAT "IT BUFFERS" CLAIM WAS ALSO MINE, AND IT WAS ALSO FALSE.**
The sentence here said:

> ~~"`run-gates.sh` **buffers**, so a redirected log sits empty until it
> finishes. An empty output file is not a hung run."~~

It does not buffer. Redirected straight to a file it writes each `=== <cmd>`
banner as it goes, and a sweep OOM-killed mid-`cargo test` on 2026-09-12 left
24 lines of readable progress showing every gate that had already passed.
What sat empty was `bash tools/run-gates.sh 2>&1 | tail -40` — **`tail` cannot
emit a line until its input closes.**

⇒ Note that this is the SAME ERROR as the exit-code one below, from the same
pipeline, written into this file in the same session that corrected the other
half of it. **Redirect to a file; do not pipe.** A pipeline changes both what
you see and the status you read, and both failures look like a defect in the
tool.

**THE SENTENCE THAT WAS HERE WAS FALSE, AND CORRECTING IT IS THE POINT.**
It said:

> ~~"its final line reports failures **while exiting 0** — read the
> `run-gates: FAILED — N of 31` line, never the exit code."~~

`tools/run-gates.sh` ends `exit 1` on any failure (line 246) and `exit 0` only
on a clean sweep. It has always been correct. What reported 0 was **my own
pipeline** — `bash tools/run-gates.sh 2>&1 | tail -40` exits with `tail`'s
status, not the script's.

⇒ **A wrapped command's exit code is the WRAPPER's.** Run a check alone and
read its own status, or redirect to a file and grep the file — never both pipe
it and trust the code. The same mistake pushed a lint failure to `origin` an
hour later (`4608f7e`), from `cargo clippy … | grep … | head`.

Note the shape, because this project has met it before and it is the
expensive kind: **I attributed my own error to a defect in a tool, and wrote
the false attribution into the document a session reads FIRST.** `CLAUDE.md`
rule 8 records the same thing about "there is still no git remote configured" —
a fact about the environment that nobody had measured, reading as reassurance
for a day. `grep -n 'exit' tools/run-gates.sh` costs nothing.

---

## STATE

Workspace version `0.55.0`; the last release is **`v0.55.0`** (`229e8635`,
2026-09-17 — 9 commits since `v0.54.0`, which shipped the same day).
Verify with `gh release list` before repeating it — a previous handoff
carried a release number four versions stale for a day, and on 2026-09-17 this
very line was left saying `0.54.0` while a new block three screens above it
said `0.55.0`. **Nothing in this file checks itself**, and the stale half is
always the one further from where you are editing.

**`main` is pushed through the 587th filing (`5a901336`, CI green); `2039521c` and `b82b7aba` are local only** (2026-09-23 — superseding the next sentence). Previously: **`main` is pushed through the 571st filing**, CI green at `229e8635` and
`7419fb2f` — read CI's colour from GitHub yourself
(`gh run list --branch main --limit 1`); this line records what was pushed and
what the server said at that moment, never what it says now.

### `main` WAS RED FOR TEN HOURS AND TWO PUSHES LANDED ON IT

Green again as of `96958657` (2026-09-15 04:14Z), and the whole of the fault
is worth one paragraph because it is the exact failure the section at the top
of this file was written to prevent.

**What was red:** `check-register-entry-size.py`, on ONE row of
`docs/FEATURES.md` — the reflow row, at 1,223 characters against a 1,200 cap,
not carried in the baseline. Nothing to do with code. It went red on the push
at 2026-09-14 18:36Z and stayed red.

**What then happened:** the balloon-note fix (`Pass 304.0`) was pushed at
02:37Z the next morning **without reading CI's colour**, so it inherited the
red and added nothing to it. Rule 8 says to read the colour. The paragraph at
the top of this file says to sweep before pushing. **Neither was done for that
push, and the sweep would have caught it** — `run-gates.sh` reports this gate.

⇒ Three things, in the order they matter:

1. **A red `main` is not a property of your commit.** The run list is the only
   place that says whose fault it is, and `gh run list --branch main --limit 3`
   shows the two pushes before yours. Look at more than one row.
2. **`check-register-entry-size.py` goes red on a doc edit with no code in
   it**, which is exactly the class of change a session is least likely to
   sweep after. A librarian filing that adds one clause to a FEATURES row can
   turn `main` red, and nothing about it will look like a risk.
3. **The fix is a trim, not a baseline entry.** The baseline is DEBT and the
   intended direction is down. Both rows over cap on 2026-09-15 were trimmed
   by deleting reasoning that already lived in `ROADMAP.md` and the commit
   message — no fact was lost, which is the test for whether a trim is honest.

### ON BRANCH `ocrcer-engine` — 2026-09-24 (`Pass 327.0`, NOT on `main`, NOT pushed)

OCRcer wired in as an opt-in second OCR engine (`ocrcer` feature, default
OFF; `pdfcer ocr --ocr-engine ocrcer`; model `models/ocrcer/ocrcer.ocrw`, not
shipped). **Do not merge or push this branch as it stands:** `ocrcer-core` is
a LOCAL PATH dependency (`../OCRcer`, operator decision), and Cargo reads
every path dependency's manifest even when optional — so on GitHub CI, or any
clone without `D:\Dev\OCRcer` beside it, the whole workspace fails to
resolve, default build included. Merge only once OCRcer is published and the
dependency is a pinned git one. `pdfcer-gui` engine choice untouched — owed
there separately.

### SINCE THE LAST HANDOFF — 2026-09-23 (`Pass 311.0`–`326.2`, 572nd–588th filings)

**FIRST ACTION ON "continue": sweep, then push.** Two commits are local only:
`2039521c` (cli: `pdfcer print --line-width`, `--poster-cut-marks`,
`--poster-labels`) and `b82b7aba` (588th filing). `tools/run-gates.sh` was
started and **killed by the harness for low memory** mid-`cargo test
--workspace`; the one reported failure was `0xc0000142 STATUS_DLL_INIT_FAILED`
compiling a test binary — starvation, not a defect. Already green on
`2039521c`: `clippy -p pdfcer-cli --all-targets -D warnings`, `cargo test -p
pdfcer-cli --all-features` (first 20 groups seen; rerun it whole),
`cargo check --target x86_64-unknown-linux-gnu -p pdfcer-cli`,
`check-clap-help`, `check-cli-help-leads`, fmt. Use the split fallback
procedure at the top of this file if the sweep dies again. Then push and read
CI's colour.

**What shipped in this span** (all answering `pdfceGUI` requests G028–G041;
every one has a `reply_*_FIXED.md` in `FeatureRequests/pdfce_FeatureRequests/open/`):
- `Pass 311.0` G028 span-edit origin · `312.0` G032 line split on clear space ·
  `313.0` G031 per-run preview cap · `314.0` G029 move_objects moves text ·
  `315.0` G030 set-taking move_text_runs · `316.0`/`317.0` G034 writable text
  render mode, G037 glyph→surgery-run map · `318.0` G036 OCR layer
  find/replace/remove · `319.0` G038 run target width · `320.0` G035 merge runs.
- G033 (SVG/EMF keep text as text): `321.0`, `322.0`, `322.1` EMF, `323.0`
  bare CFF, `324.0` font collections (`.ttc`/`.otc` — also fixed a supplied TTC
  never rendering: `skrifa::FontRef::from_index`), `324.1` Type 1.
- `326.0` poster cut-mark band + label geometry (G040/G041), `326.1`
  `StrokeDisplay::Fixed { device_px }` (G039), `326.2` their CLI flags
  (unpushed). FEATURES: fixed stroke width has its OWN row (cli `[x]`, print
  only); the hairline row's cli stays a deliberate `—` — do not tick it.
- 584th filing corrected eight stale gui boxes per the 09-20 channel note;
  both 09-11 and 09-20 notes have replies in `open/`.

**Not started, next in line:**
- `Pass 325.0` — split `pdfcer-core` into a model crate + narrow feature crates
  behind a facade (Ken asked why it was ever one crate). Step 1 (dependency-edge
  measurement) is recorded in its Backlog entry; steps 2–7 remain. Memory-heavy
  builds make this machine a poor fit for big refactors — build per-crate.
- Next free Pass family: **327**.
- Check both FeatureRequests channels for anything newer than G041.

**Session gotchas:** `SendMessage` is disabled this session type — re-dispatch
a fresh librarian with full context instead of continuing one. The librarian
has no shell; paste exact hashes. Low memory: `CARGO_BUILD_JOBS=1
RUST_TEST_THREADS=2`, per-crate `-p`, long runs in background.

### SINCE THE LAST HANDOFF — 2026-09-17 (`Pass 309.x`, `310.x`, `v0.55.0`)

Everything after this block is carried forward unchanged.

**The operator reported redaction destroying content he never marked**, and he
was right in a worse way than the report suggested. Marking `INVOICE 4412` on
page 1 turned `INVOICE summary` on page 2 into `XXXXXXX summary`. Two
independent causes, both in `redact::carrier_residual_sweep`:

1. **No liveness test.** Every record of that sweep — decision records, doc
   comments, `docs/core-api/` — calls its target an *abandoned* content
   stream. Nothing in the code tested for that. The word lived only in prose,
   never in a predicate, so the sweep blanked live page `/Contents`,
   annotation `/AP /N`, form XObjects, Type-3 CharProcs and tiling patterns
   with equal confidence. Filed as `R247`'s sixth instance.
2. **A tokenised needle.** `redaction_evidence` expands each redacted run into
   the whole run PLUS every whitespace-delimited token of four characters or
   more, so redacting `INVOICE 4412` made the bare word `INVOICE` an
   independent search term.

Fixed by splitting the needle sets — tokens to invisible carriers, whole
redacted runs only to anything drawable — and by gating the drawable half
behind `redact::ResidualScope` (`MarkedOnly` / `HiddenCarriers` default /
`WholeDocument`), carried on `RedactOptions` and set on the session.
`Pass 310.2` then reconciled the sweep's detector with its actors: the detector
was ASCII-case-insensitive, the two blankers were exact-byte, so a case variant
was *found and not removed*. Both now share `text_match_ranges`. `R245`'s
eleventh instance.

### Three things from this session that will cost the next one if forgotten

1. **`has_unscrubbed_matches()` must never be folded into
   `has_disclosed_residuals()`.** They answer different questions. "pdfcer
   could not act" drives the CLI's non-zero exit and `--acknowledge-residuals`;
   "pdfcer was told not to act" must never fail a redaction, or every redaction
   of a phrase appearing twice in a document exits non-zero. The temptation to
   merge them is real — they are both "a match survived".
2. **A librarian with no shell cannot tell committed work from uncommitted
   work.** The 570th filing read `text_match_ranges` in the working tree,
   concluded the dispatch's "310.2 still open" was stale, and filed all three
   Passes under one commit. The source it read was real; the attribution was
   not. `check-cited-commits-exist.py` cannot catch this — the wrong hash was
   real and an ancestor of HEAD. Corrected as `R87`'s dated instance.
   ⇒ **Dispatch a librarian AFTER committing, or tell it explicitly what is
   uncommitted.**
3. **`gh release create <tag> <assets...>` is not atomic.** A transient
   `HTTP 500: Error saving asset` **rolled the whole release back** —
   `gh release view` answered "release not found" while the error URL carried a
   release id. Create the release bare, then `gh release upload --clobber` with
   retries. Two failures before the third attempt stuck.

**State:** `v0.55.0` tagged at `229e8635`, released, OneDrive slot `pdfcer2`
written (`pdfcer1` keeps 0.54.0 as previous). Gate sweep green — 34 commands,
420 `test result: ok`, zero failures. The packaged binary was smoke-tested from
a fresh folder against the operator's own scenario, not only in unit tests.

**Still open:** the GUI half. `pdfcer-gui` has not wired
`EditSession::set_residual_scope`; the heads-up is `E001` in the request
channel, and `FEATURES.md`'s redaction row is honestly `[ ] gui`.

### SINCE THE LAST HANDOFF — 2026-09-15, later (`Pass 307.0`, `308.0`, `308.2`)

Everything after this block is carried forward unchanged. Two requests arrived
from `pdfcer-gui` within minutes of each other and both shipped the same
session: `G019` (no derived tab order) and `G020` (`/MK` colours round-trip and
are painted by nothing). Replies are in the channel's `open/`; `308.1` is the
one piece still owed.

**AND THEN I TURNED IT RED MYSELF, AND THE MECHANISM IS WORTH MORE THAN
THE APOLOGY.** `bd8059f2` (`Pass 308.0`) failed CI on **`check-passes-filed.py`**
— not for anything in it, but because `729cf6db` (`Pass 307.0`), one commit
back, was still unfiled. Green again on the next commit, the filings.

⇒ **The tip-deferral is the whole mechanism, and it is not a defect.** The gate
exempts the tip on purpose — a commit cannot cite its own hash, so its filing is
always a later commit. Which means a Pass commit's own CI run is **always green**
and the **next** push is what tells you whether it was filed. Push two Pass
commits back to back without the filing in between and the second one is
**guaranteed** red, with a message naming the first.

⇒ Three practical consequences, and the second is the one that bites:

1. **File between Passes, not at the end of the session.** I dispatched the
   librarian for `307.0` and pushed `308.0` before it returned.
2. **A green run on a Pass commit proves nothing about that commit's filing.**
   It is the one commit the gate is guaranteed not to check. Read the run
   *after* it.
3. The pre-push hook has the same deferral, so it will not stop you either — it
   refused my *third* push, correctly, and by then CI had already gone red.

Same session, same file, one paragraph apart: the block below is about a
docs-only commit reddening `main` because nothing about it looked like a risk.
This one is about a **code** commit reddening `main` for something that was not
in it at all. **Neither is visible in the diff you are about to push.**

**AND `main` WAS RED BEFORE THAT, FOR THE THIRD TIME IN EIGHT DAYS, THE SAME
WAY.** Red from `0b48b3e2` (the 555th filing, pushed 15:57Z) until `729cf6db`
fixed it. The gate was `check-core-api-verbs.py`; the cause was
`docs/core-api/index.md` still stating `03-capabilities.md`'s old line and
clause counts after that commit grew the file. **Doc-only, no code, nothing
about it looked like a risk** — which is now the third instance of exactly that
sentence in this file. The librarian filed it as a dated instance of `R197`
rather than minting a new rule, on the grounds that it is the same mechanism
and not a new shape. ⇒ **The sweep is the control, and it works. It caught this
one in the first minute of the session** — before any code had been written,
because it was run before starting rather than before pushing.

**`Pass 307.0` — `EditSession::page_tab_sequence`.** All six `/Tabs` states.
`/R` and `/C` computed from `/Rect` with `/Rotate` applied and
`/ViewerPreferences` `/Direction` honoured; `Absent` and an unknown name fall
back to array order **as a disclosed convention**; `/S` returns an **empty**
sequence rather than a guess. Plus `TabOrderBasis`, `TabExclusion`,
`AnnotFlags::TOGGLE_NO_VIEW`, two settings (`widget_tab_tail`,
`tab_row_tolerance`), the `pdfcer tab-order` subcommand, three synthetic
fixtures, 23 core + 14 CLI tests. Decision **158**.

**THE REQUEST'S MEMBERSHIP RULE WAS HALF WRONG AND THE CORRECTION IS
SOURCED.** `pdfcer-gui` asked for every annotation with the caller filtering,
and argued it well: filtering before ordering changes which annotations fall
into which row. Right about **subtypes**. Wrong about **flags** — §12.5.1 is
silent, but §12.5.3 says a `Hidden` or `NoView` annotation shall not *"allow it
to interact with the user"*, and tabbing is interaction. ⇒ *A clause being
silent is not the standard being silent.* The exclusion was three clauses away
from where everybody was looking, and the commissioned spec-corpus file found
it by being asked "what excludes them?" rather than "does §12.5.1 exclude
them?".

**Two settings, because two things are genuinely open**, per Ken's standing
"make spec ambiguity a setting" rule: `widget_tab_tail` (`TAB-A1` — ISO 32000-2
contradicts itself about `/W`'s tail, measured unreported across three errata
channels with positive controls) and `tab_row_tolerance` (1.0 pt — the standard
states none, so the number is pdfcer's).

**`Pass 308.0` + `308.2` — `/MK` `/BG` and `/BC` are baked into the `/AP`.**
Read and write halves had both shipped; **nothing painted them**, so writing
`/BG` changed the dictionary and nothing a person could see. R43 is why: pdfcer
paints the baked `/AP` and never reconstructs from `/MK`. `WidgetChrome`
threaded through all four builders, `needs_regen` gains both colours, and
`AppearanceOutcome` gives the three states one value.

**A MISMATCH THAT HAD BEEN HARMLESS FOR MONTHS BECAME LOAD-BEARING THE
MOMENT SOMETHING READ IT.** Push-button creation wrote `/MK` `/BG` and `/BC` as
**DeviceRGB triples** while the artwork painted **DeviceGray**. Same colour,
different operator, completely inert — for as long as nothing derived one from
the other. Make the builder read `/MK` and the ownership test (*"would pdfcer
draw exactly these bytes?"*) answers **no, for a button pdfcer drew itself**.

⇒ *Two representations of one fact can disagree indefinitely at no cost, and
the cost arrives in full the moment a third thing starts deriving one from the
other.* Note this is the **inverse** of `Pass 306.0`'s finding two entries
below: there, a correct compensation hid the thing it compensated for; here
nothing was compensating and nothing was looking. **Consequence to carry:** a
push button created by an earlier build now reports `RecordedNotPainted`
instead of redrawing. A disclosure, not damage; re-setting either colour
rebuilds it.

**The defaults were the load-bearing half, and they pull opposite ways.** A
text field draws no box at all (an absent `/BG` that produced a white rectangle
would repaint every text field in every document pdfcer touches); a push
button's default is **not** "nothing" but the plate grey (a default of nothing
would erase every plate). Both live inside the builder rather than at the call
sites, and the test file asserts the **unchanged** half as hard as the changed
half. ⇒ *Threading a new parameter through an existing builder owes a test that
the parameter's ABSENCE is byte-identical to before.*

**A precision defect a test assertion found and review would not have.**
`MkColor` stores `f32`; `f64::from` widens the **binary** value, so `0.2` was
about to be written into content streams as `0.20000000298023224`, once per
component. Nothing renders differently. Fixed by round-tripping through `f32`'s
shortest-round-trip `Display`.

**Gate status: every gate run and green, and `run-gates.sh` was not needed.**
What worked, and it is the split procedure below rather than the sweep: the 21
non-cargo gates in one loop; `fmt --check`; `clippy --workspace --all-targets
--all-features`; `test -p pdfcer-core --lib` (2,085) and `--test '*'` (150
binaries); `test -p pdfcer-cli` (49 binaries); `test -p pdfcer-render -j 1` (53
binaries — **`-j 1` is what made this one finish**, it was `LNK1102`-killed at
the default `-j`); `test -p pdfcer-core --no-default-features` lib and doctests
(187, `--test-threads=1`); wasm `check`; `fuzz check --bins`.

**`LNK1102: out of memory` struck again and `-j 1` DID fix it this time** —
which contradicts the 2026-09-12 note below saying `-j 1` did not help. Both
observations are real; the difference is that the earlier one had a second
cargo job live beside it. ⇒ *One cargo invocation at a time, and `-j 1` for the
render crate.*

~~**Still owed: `Pass 308.1`**~~ — **SHIPPED `86ede66b`, same day. `G020` is
fully closed.** All five `New*` specs carry `chrome: WidgetChrome` with
`with_background` / `with_border_color`; the CLI gains `--background` and
`--border-color` on all five `add-*` verbs, spelled as `edit-widget`'s. The
dictionary and the artwork are written from ONE value, in the create branch and
the merge branch alike.

**AND THE FIX WAS A KEY REMOVED, NOT A KEY ADDED, WHICH IS THE PART TO
CARRY.** `add_text_field` and `add_choice_field` had always written `/MK`
`/BC [0 0 0]` while handing the appearance builder nothing — the dictionary
claimed a black frame the `/AP` never drew. Inert for months. **`Pass 308.0`
made `edit_widget` regenerate FROM `/MK`**, so the first *resize* of a
pdfcer-created text field would have materialised a frame that had never been
in the document. Nothing in `308.0` looked wrong; it simply started reading a
key that had never been true.

⇒ *Stop claiming it, do not start drawing it.* Painting the declared frame was
the other available fix and was rejected: *"a text field draws no box by
default"* is a stated invariant of the shared builder that FILL also uses, and
adopting Acrobat's floor would have repainted every newly created text field
for a change `G020` never asked for. Recorded as a road not taken, because the
fork will be met again.

⇒ **The rule, and note it is the INVERSE of `308.0`'s two entries above:** two
representations of one fact can disagree indefinitely at no cost, and the cost
arrives in full the moment a third thing starts deriving one from the other.
`308.0` met it from the paint side (DeviceRGB in `/MK`, DeviceGray in the
stream); this met it from the record side (a key describing artwork nobody drew).
**Both came from the same act — writing a dictionary and a stream from two
literals instead of one value.** If you are about to write a `/MK` entry and a
content-stream operator in the same function, that is the moment.

⚠ **Reporting change `pdfcer-gui` was told about:** a created text or choice
field now reports `border_color: None` where it reported `Some(Rgb(0,0,0))`,
and carries no `/MK` at all when no colour was asked for. Check box, radio
button and push button are byte-identical in both halves.

**A spliced doc block was found in `pdfcer-cli/src/main.rs`** —
`EditWidgetArgs`'s comment welded onto `parse_mk_colour`'s, the exact failure
`check-doc-block-spliced.py` exists for, **in a crate that gate does not
scan.** Unspliced in the same commit. The gate's coverage is narrower than its
name suggests; a CLI splice is found only by reading.

### AND `G021` ARRIVED MINUTES AFTER THAT NOTICE — `Pass 308.3`, `20bfb259`

`pdfcer-gui`, wiring the swatches `308.0` unblocked: *"a `/MK` colour can be
set and changed, but never removed."* Correct, and **the bug was in a TYPE.**

`WidgetEdit`'s two colour fields were `Option<MkColor>` — and that `Option`
already meant *this edit does not mention the key*. So `Some(MkColor::None)`
was Table 189's empty array and **absent had no spelling at all**. The key has
three reachable states, the READ model
(`forms::Widget::background`) always distinguished all three, and the write
model could reach two. Nobody wrote a wrong line: *the type could not express
the state, so the state was silently unreachable.*

⇒ **When a read model distinguishes N states and the write model is an
`Option`, check that N ≤ 2 before believing the round trip is closed.** This is
the **third** defect on these same two keys — `/BG` was readable-and-unwritable
and `/BC` writable-and-unreadable until `Pass 249.1`/`262.2` closed them. Those
fixes closed the KEYS. Nobody checked the STATE SPACE.

**On a push button it is a different RENDERING, not a different byte**:
absent means the plate grey, `[]` means no plate, so an operator who chose *no
background* could not get the plate back. `/BC` is currently only a byte
(absent and empty both stroke black) and stops being one the moment any default
there is not black.

`MkColorEdit { Set(MkColor), Remove }` names the three states.
`with_background` / `with_border_color` keep their signatures and every call
site; `without_background` / `without_border_color` are new; `resolved()` is
the single reader shared by the dictionary writer and the regenerator, so they
cannot drift. CLI: `edit-widget --background unset`, deliberately a different
word from `none`.

**The requester's preferred option was DECLINED with reasons, and the reason
generalises.** They asked for `with_background(Option<MkColor>)` on read/write
symmetry. That makes the field `Option<Option<MkColor>>` — **the same mistake
one layer deeper**, three states carried by two anonymous wrappers where the
reader must know which `None` is which. A named enum instead. The reply offers
a thin `with_background_state(Option<MkColor>)` wrapper **if** their
copy-chrome pattern recurs; that is a conditional offer, not a commitment, so
do not ship it unprompted.

**AND THE CHANNEL FINDING, which is worth more than the code one.** They
had a working workaround — write `/MK` directly, around `EditSession` — and
**refused it**, reporting the refusal per decision 058: it would have bypassed
the undo stack, the appearance regenerator and the R33 minimal-diff writer.
Verbatim: *"The boundary is drawn in the right place; it is just missing a
verb."*

⇒ *A missing verb at a well-drawn boundary is a CHEAP bug. A boundary crossed
to work around a missing verb is an expensive one, and it does not look like a
bug at all* — it looks like working code, and the unenforced invariants surface
much later as "undo behaves oddly on one property". **Second instance in two
days** (`G017` was the first shape of it). When a shell reports a missing verb
instead of routing around one, that report is worth answering the same day.

**A stale claim retired, and its twin found by the librarian's sweep.**
`WidgetEdit::border_color`'s doc still carried the long "honest limit"
paragraph — pdfcer does not paint `/MK`, painting it would need a second
generator against R92 — which `Pass 308.0` had falsified that morning by doing
it with neither. Retired in the Pass commit; the librarian's sweep then found
the **same claim surviving** in `docs/core-api/01-reading-and-model.md`, and
flagged rather than edited it because that file is the engineer's. Closed on
receipt, in the filing commit. ⇒ *A claim copied into two documents is retired
from one of them; grep for the SENTENCE, not the file you remember writing it
in.*

**AND THE SENTENCE I WROTE HERE AN HOUR LATER WAS WRONG, WHICH IS WHY IT
IS STRUCK RATHER THAN EDITED.** It said:

> ~~"`run-gates.sh` ran all 34 commands to completion again, nothing
> OOM-killed — the second consecutive clean full sweep on this machine, so
> treat the fallback procedure above as the exception it now is."~~

Two clean sweeps in a row, and I generalised from two. **The THIRD sweep the
same evening stalled** — parked in `cargo test --workspace`'s compile phase
with no progress line for many minutes while the machine was under memory
pressure (the harness killed several background waiters for the same reason).
Killed, and the **split procedure below carried it to green without trouble**.

⇒ **The honest statement is a COIN FLIP, and the split procedure is the
dependable path.** Try the sweep — it is one command and it sometimes works —
but budget for it stalling and do not plan a session around it. Note the
failure MODE is not always a kill: it can simply stop making progress, which
looks like a slow compile until you check the clock. `stat` the log; if its
mtime is minutes old while `rustc` processes are alive, it is stuck.

⇒ **And note the shape, because it is this file's recurring one.** The
paragraph above this block corrects "it buffers"; the one above that corrects
"it exits 0"; `CLAUDE.md` rule 8 corrects "there is no git remote". Every one
is **a confident environmental claim generalised from too few observations and
written into the document a session reads FIRST.** Two data points are not a
trend. The sweep-tendency paragraph at the top of this file gets this right —
it says *tendency*, tells you to try anyway, and refuses to delete the
evidence on either side. Match that register when you amend it.

One `Pass 308.1` failure worth keeping: `cargo fmt --all --check`, on the new
test file alone. The 24 non-cargo gates and fmt were re-run **after** the last
edit, which is the discipline the top of this file asks for.

### SINCE THE LAST HANDOFF — 2026-09-15 (`Pass 306.0`)

Everything after the 2026-09-14 block below is carried forward unchanged. Two
things shipped in one session, both from one message the operator sent in
conversation — not through either request channel:

> *"I am now able to edit the text on sw41177. When I edit the line #3 after I
> am done the whole line shifts position. I also can't move this line to
> position. I assume this is due to all the text of all the lines being part of
> a larger block. Since we've got the reflow text figured out maybe we can add
> a tool to make each reflowed area its own text object so each line can be
> moved and manipulated on its own."*

He reported a symptom and proposed an architecture. They turned out to be two
independent things, and **both were real** — the symptom was a defect his
proposal would not have fixed, and the proposal was a capability the defect had
nothing to do with. Shipping only one of them would have looked like a fix and
left the other live.

**1. The defect: a same-baseline re-anchor BEHIND the edit was treated as the
line's tail.** `same_line` answers *"same baseline?"* and `reposition_followers`
was reading it as *"the rest of the line?"*. Those come apart when a producer
writes a line's pieces out of visual order, and SolidWorks does — the note's
TEXT first, its BULLET second, to the LEFT, on the same row:

```text
100.00423 Tz 5.66931 -1 Td  <TOLERANCE :->Tj     ← the anchor
 99.94655 Tz -5.66931 0 Td  <3.>Tj               ← same row, 28 pt LEFT
 99.82585 Tz 5.66931 -1 Td  <X/XX: …>Tj          ← the next line
```

Shortening the anchor rewrote `-5.66931 0 Td` → `-9.9185 0 Td`, moving the
bullet 21 pt left, and compensated the line below so **everything downstream
stayed exactly where the producer put it**. Fixed with
`re_anchors_before_anchor` + `FOLLOWER_ORIGIN_EPSILON`; the same edit now
reports `followers_repositioned = 0` and both `Td` operators are byte-identical
to the input.

**THE COMPENSATION IS WHY IT SURVIVED, and that is the lesson to carry.**
Because the walk put the next line back, the damage was confined to one glyph
pair the operator had not selected. A render diff shows almost nothing; a
geometry assertion on the block passes; the byte diff is small and plausible;
the file round-trips perfectly. ⇒ *A correct compensation can hide the thing it
is compensating for.* The inverse of `Pass 305.0`'s finding two days earlier
(bytes wrong, pixels right) — here the **bytes look reasonable and one small
piece of geometry is wrong**. Neither is visible to the other's test.

⚠ **The guard's first cut was wrong in a way only the existing suite caught**:
it measured from the ANCHOR, but on a `Pass 256.0` span the anchor is the
**last** operator of the run, so a per-glyph producer's three-operator span read
its own interior steps as "behind the edit" and respaced nothing.
`text_edit_span.rs::a_growing_replacement_respaces_the_followers_and_keeps_the_next_line_put`
failed in the sweep and nowhere earlier. The reference is now the **first**
edited operator. ⇒ *A guard added to a single-item code path needs asking what
the multi-item path calls "the item".*

Fixed in passing, same file: **`same_line`'s doc comment had been welded onto
`reposition_followers`**, and `same_line` itself had none. This is a second live
instance of the exact failure this file records for `preview_style_resolution`,
and it was invisible for the same reason — both functions read as documented.
**`check-public-fns-documented.py` covers only `pub` fns, so the private half of
this failure mode has no gate at all.**

**2. `Pass 306.0` — `split_text_object`, the tool he asked for.** At each cut,
`ET BT <the run's own six-coefficient Tm>` is inserted before the run's show
operator and **nothing else changes** — every original operator keeps its bytes,
its order and its paint position. `--granularity run|line`, or explicit
`--before N`. After a split each piece is an ordinary text object: move it,
recolour it, delete it, reflow it. Verified on his file through the CLI:
`cuts=17 … undo_verified=1 undo_identical=1`.

**THREE SITES IN THIS CRATE HAD RECORDED THE COST AS A BLOCKER AND NOBODY
HAD PRICED IT.** `plan_move_text_run`'s docs, `text_edit/format.rs` and
`text_edit/reflow_apply.rs` all say, in nearly the same words, *"`q`/`Q` are not
admitted inside `BT`…`ET` (§8.2 Table 51), and splitting the `BT`…`ET` would
discard `Tm` (§9.4.1)"*. The sentence is **true and it is not an obstacle**:
`Tm` costs one operator to restate and the run already carries its own matrix.
What makes it cheap is the other half of §9.3/§9.4.1 — `BT` resets ONLY
`Tm`/`Tlm`; `Tf`, `Tc`, `Tw`, `Tz`, `TL`, `Ts`, `Tr`, colour and the CTM are
graphics state that `ET`/`BT` do not touch. So there is **no preamble to emit
and no `restore_ops` to compute** (contrast `reflow_apply`, which rebuilds a
text object and therefore owes R88's restore). Decision **157**.

**It also unblocks reflow on CAD output.** `reflow_apply` refuses a block that
shares a `BT`…`ET` with other content, which on a SolidWorks sheet is every
block. Split first and that refusal goes away — worth remembering before
anyone re-scopes reflow as "deferred for interleaved blocks".

**I WROTE A CLAIM INTO THE DOCUMENTATION AND THEN THE MEASUREMENT REFUTED
IT.** The doc comment said *"the rendering is unchanged, and that is testable
rather than asserted: split, render, compare — the rasters must hash equal."*
Measured on `SW41177.pdf` page 1, splitting before ONE run of the 237-run label
object and re-rendering the whole page:

| scale | differing px | worst delta | where |
|---|---|---|---|
| 1× | 11 of 1.9 M | 16/255 | scattered over the WHOLE sheet |
| 2× | 71 of 7.8 M | 64/255 | scattered over the WHOLE sheet |
| 4× | 301 of 31 M | 64/255 | scattered over the WHOLE sheet |

**Scattered over the whole sheet is the diagnostic**, and it is what turned a
worrying result into an explained one: a structurally wrong cut puts its
differing pixels AT the cut, and these bound the whole object. Nor is it a moved
glyph — the count tracks rendered AREA while the worst delta stays at one
antialiasing step. Cause: `pdfcer-render` keeps `Tm`/`Tlm` as
`tiny_skia::Transform`, **f32**, so a deep `Td` chain accumulates a rounding per
step; the absolute `Tm` the split emits is that chain summed in **f64**. The
glyphs land ~one f32 ulp CLOSER to what the file says.

⚠ Two things follow, and the second is a trap:

1. **Chain DEPTH predicts the drift, not the number of cuts.** The 18-run notes
   object split by `Line` — 17 cuts, more than the single-cut probe — is
   **bit-identical**, because its chains are three steps deep. Seven single-cut
   probes across the 237-run object: five bit-identical, two not.
2. **`pdfcer-render`'s f32 text matrix is a finding about the RENDER crate.**
   That crate already has `gstate::Mat64` and already uses it for the CTM for
   exactly this cancellation reason; the text matrix never got the same
   treatment. It is a `ROADMAP.md` Backlog item now. **Do NOT "fix" the drift by
   making the split emit a deliberately less accurate matrix** — the split is
   the more correct of the two.

⇒ And the methodology point, which this file already carries twice in other
clothes: **the claim went into the docs before the measurement, and the
measurement said no.** The doc comment now carries the numbers AND states that
bit-identity was claimed first and was wrong — a reader who finds only the
corrected claim cannot tell it was ever in doubt.

**Filing:** 554th, decision **157**, `FEATURES.md` row added. The librarian also
found the *"next free `R257`"* ceiling had been carried **stale across roughly a
dozen filings** — `R257` was minted at the 547th — and corrected it to `R258`.
Note the shape: a counter nothing checks drifts silently, exactly like the
`docs/core-api/` verb count did before `check-core-api-verbs.py` existed.

**Gate status at the end of this session: every gate run and green**, but
**not by `tools/run-gates.sh`** — it was OOM-killed twice, and the second time
took five watcher jobs down with it. What worked, and what a future session
should copy:

| gate | result |
|---|---|
| the 26 non-cargo gates, in one loop | all clean |
| `fmt --check`, `clippy --workspace --all-targets`, same `--all-features`, wasm `check`, `fuzz check --bins` | all clean |
| `test -p pdfcer-core --no-default-features` | 151 binaries, 0 failures |
| `test -p pdfcer-core --no-default-features --doc -- --test-threads=1` | 187 doctests |
| the 18 core suites that touch text editing / reflow / vector edits | 148 tests |
| `test -p pdfcer-render` | 753 tests |
| `test -p pdfcer-cli` | 479 tests |

**THE OOM IS IN LINKING, NOT COMPILING, AND THAT CHANGES THE REMEDY.** The
handoff's existing advice — fall back to the split procedure, drop to `-j 1` —
did **not** help: `-j 1` was killed at the same phase as `-j 8`. What every
killed run has in common is `link.exe` on many test binaries at once, and the
failure is always `STATUS_DLL_INIT_FAILED` (`0xc0000142`), which **reads like a
broken toolchain** — rustc even suggests repairing Visual Studio — and is not.

⇒ Three things that actually worked:

1. **Run ONE cargo invocation at a time and nothing else beside it.** Every
   kill here happened while a second job was live; the machine had 4–6 GB free
   at the time, so this is a spike, not exhaustion.
2. **Run the doctest phase separately, `--test-threads=1`.** `cargo test -p
   pdfcer-core --no-default-features` died in doctests three times and passed
   187 of them in 156 s once split out. A doctest links its own binary, so the
   doctest phase is the densest linking in the whole sweep.
3. **Go per crate.** `--workspace` never completed; the three crates run
   individually all did, on the first try each.

⚠ **`CARGO_PROFILE_*_DEBUG=0` is a TRAP here** — it does cut linker memory, and
it also invalidates the whole dependency graph, so it starts a from-scratch
rebuild of `iccce`, `skrifa` and everything else. Strictly worse than the
problem. Reverted immediately; noted so nobody re-derives it.

And the reason this section is long: the top of this file says a gate run
before your last edit is a gate that did not run, and **that discipline held
here** — the full sweep caught a regression in
`text_edit_span::a_growing_replacement_respaces_the_followers_and_keeps_the_next_line_put`
that none of the targeted suites reached, the fix went in, and then the sweep
would not run again. Getting the coverage anyway took the table above. *A gate
runner that cannot finish on the machine it runs on is not a green sweep, and
saying "the sweep failed" would have been a lie in the other direction.*

### SINCE THE LAST HANDOFF — 2026-09-14

Everything after this block is from 2026-09-12 and is carried forward
unchanged. Three things happened since, in order:

**1. `Pass 304.0` — a CAD note split across show operators is editable.**
From the operator directly, in conversation, not from either request channel:
*"Check if you can edit one of the notes with the numbers for the balloons"*,
then *"Test it and if you can't edit it make it so a user can edit it as they
would expect to."* It could not be. SolidWorks emits one visual line as several
show operators and perturbs `Tz` and `Td`'s vertical by float round-trip noise
between them; `spannable` compared `Tz` with `==` and `same_line` compared the
baseline with `==`, so no route reached the text. Two measured tolerances, the
vertical one **scaled by the text matrix's y-scale** — which is the half that
was got wrong on the first cut, because the drift arrives pre-multiplied and a
flat threshold fixes one note on a page and not the note beside it. All six
balloon-bearing notes on his sheet now edit and survive save-and-reopen.

⚠ **This one has NO topic key, deliberately.** An earlier draft labelled it
`G017` in code comments and its commit message. **`G` is `pdfcer-gui`'s
namespace** and they filed their own, unrelated `G017` the same morning. The
commit was amended before it was pushed, so nothing in git carries it. *Do not
mint a key in someone else's namespace for work that came from the operator.*

**2. `Pass 305.0` — `G017`, `move_text_run` and the in-form family.** The
request's own words: a text run could be deleted and not moved, while every
other part kind had both halves. Shipped with its in-form twin **and** the
three in-form deletes the request's second row found missing, which takes that
family from six verbs to ten. Reply in `open/reply_G017_…`; the request is
theirs to close with a `done_G017_…`.

**3. `docs/core-api/` counts moved.** 227 → **232** public `EditSession`
methods. `tools/check-core-api-verbs.py` caught it, as designed.

**One defect worth carrying forward for its SHAPE**, found during `G017`:
a token-gap search used `prev.tokens.end + 1`, but `TextRun::tokens.end` is
EXCLUSIVE. That made a `Td` read as one-operand-malformed and a `Tm` as five,
so both were classified "nothing to rewrite" and both were then moved by an
INSERTED operator instead of a rewritten one. **The page came out right and the
bytes came out long.** Every geometry assertion passed; only the byte-shape
assertions failed. ⇒ *A geometry-only test suite cannot see a correct edit
written the wrong way* — and on this project's round-trip/minimal-diff rule,
the wrong way is a defect. Assert the bytes as well as the pixels.

### THE TORONTO-MAP ARC IS CLOSED

Three Passes, one evening, one request — the operator's *"Acrobat can read and
zoom in on this pdf much much faster than we are capable of … the footprint in
ram for ours is enormous by comparison"*, then *"can you fix those things
without breaking the other things that our rendering engine does well"*.

| Pass | what | worth |
|---|---|---|
| `300.0` | an image whose unit square misses the viewport is skipped before the decode (§8.9.5.2), the twin of the form cull | **nothing on this file**, and that is the finding — 1,090 of 1,182 images culled and neither time nor RAM moved |
| `300.1` | a discarded full-page scan per group, moved inside the `if` that reads it | 4% |
| `300.2` | a transparency group composites over its own `/BBox`, not the whole page | **783 s → 8 s at 4×**, 54.6 s → 2.45 s at 1×, rasters hash-identical |
| `300.3` | the token vector sizes itself from each stream's own measured density instead of doubling | Toronto slack **176.8 MB → 32.7 MB**, parse ~0.40 s → ~0.23 s; seven real files improved, none regressed |

**READ `300.2`'s COMMIT (`6ff57ab`) BEFORE OPTIMISING ANYTHING IN THIS
CRATE.** The slow path it fixed had been correctly *located* and wrongly
*diagnosed* three times across a month, by three sessions, and fixed zero
times. Every one of them named the per-group `Pixmap::new`. Timed:

    whole render                                 54.94 s
    with the composite skipped                    2.33 s
    with the allocation pooled instead           53.18 s

The allocation is 1.8 s. The `draw_pixmap` one line below it is 52.6.
**Proximity in the source is not proximity in cost.** And the misdiagnosis is
why it survived: the allocation is the half that *would* have needed the
coordinate-system rewrite, so all three sessions correctly concluded the fix
was invasive and correctly deferred it — sound reasoning from the wrong
object. The real fix moves no coordinates at all.

The general form is in `D:\dev\rag\rust\a_plausible_explanation_that_predicts_the_right_order_of_magnitude_is_not_a_diagnosis.md`.

### THE MEMORY HALF IS DONE TOO — `Pass 300.3` — AND IT NEEDED NO API BREAK

**This section twice named a fix that turned out to be the wrong one, so
read the correction before the conclusion.** It said, in order: the peak is
image decoding (wrong — `extract-text` rasterises nothing and peaks the same);
then ~~"the fix is **shrinking `ContentToken`**, in `pdfcer-core`. Unscoped, no
Pass number, nobody has started it."~~ Also wrong, and expensively so: that is
a breaking change to a type published in `docs/core-api/`, with 64 match sites
here and unknown numbers in `pdfcer-gui`.

What a measurement said instead:

```text
peak after start            4.6 MB
peak after load            32.7 MB
peak after parse          301.7 MB
  largest form   2,291,669 tokens = 139.9 MB used
                 4,194,304 reserved = 256.0 MB   (116.1 MB slack)
  sum of ALL forms' tokens         = 241.9 MB
```

Every form is dropped as soon as it is interpreted, so **only one is ever
live** — the peak is `32.7 + 256.0 + the decoded buffer`, closing to within a
megabyte. The 241.9 MB sum this section used to quote is not the peak and
never was. It was **one `Vec` rounding 2.29 M up to a power of two.**

`Pass 300.3` (`865ed7b`) fixes it where it lives: the token vector measures
the stream's own token density as it parses and reserves from that, rather
than doubling. Toronto's slack 176.8 MB → 32.7 MB, parse ~0.40 s → ~0.23 s,
seven real files of different shapes all improved and none regressed, 372
synthetic fixtures byte-for-byte unaffected.

**THE GUARD THAT CAUGHT THE DRAFT, and the reason the harness covered
small files at all.** The first version set the minimum capacity to 64 "to
save a series of small allocations" — invented, unmeasured. The 372 synthetic
fixtures are all small streams and priced it at once: reserved **0.7 MB →
1.6 MB**. It fixed 176 MB on one large file and made every small file worse.
A capacity heuristic is a claim about a POPULATION, and the population that
matters is the one you did not tune on. General form in
`D:\dev\rag\rust\a_heuristic_tuned_on_the_motivating_case_needs_a_counter_sample_before_it_ships.md`.

### SHRINKING `ContentToken` IS GATED ON KEN'S APPROVAL — DO NOT START IT

**Operator instruction, 2026-09-12, verbatim:** *"Put the shrinking token type
the list you use for this sort of thing and note that I must approve it being
changed first."*

It is a `ROADMAP.md` **Backlog** item and an **open operator question**. It is
NOT owed work, it is NOT in flight, and a session that finds the measurements
below compelling still may not begin it. **Default if unanswered: do not
change it.**

What it is: `ContentToken` is 64 B (`ContentTokenKind` 48 + `ByteSpan` 16),
and `Object` alone is 40 B. Two independent levers — `ByteSpan` to two `u32`
(64 → 56, capping a content buffer at 4 GB), and splitting the common numeric
operand out while boxing the rare composite ones (64 → 32). Both reach 24 B.
Worth roughly **70 MB more** on the Toronto map, on top of what `Pass 300.3`
already recovered.

Why it is gated, and every one of these is a reason on its own:

* **It breaks a published API.** `ContentTokenKind` is `pub`,
  `#[non_exhaustive]`, and specified in `docs/core-api/01-reading-and-model.md`
  — the contract `pdfcer-gui` builds against. 64 match sites here (46 on
  `Operand`), unknown numbers there.
* **It may make text-heavy files SLOWER.** Boxing composites means one heap
  allocation per `TJ` array and per inline dict. A vector-heavy map wins
  outright; a text-heavy document may not, and **nobody has measured that.**
  Given this arc's record, taking that measurement is a prerequisite and not a
  formality.
* **It touches the round-trip invariant** (rule 3, `ARCHITECTURE.md` §5):
  token spans are what re-emit untouched objects byte-identically.
* **It is not required for the win it was proposed for.** `Pass 300.3` closed
  the memory problem without it.

### HOW TO BENCHMARK HERE, because the obvious way cannot run

~~**`pdfcer-cli` will not release-link on this machine** — four builds
OOM-killed, including at `-j 2`.~~ **⚠ AMENDED 2026-09-17 (v0.54.0 release
filing) — this is a TENDENCY, not a certainty, the same shape as the
`run-gates.sh` correction above.** `cargo build --release -p pdfcer-cli -j 2`
linked cleanly on this machine on 2026-09-17, exit 0, in 13m 12s. The four
prior OOM kills were real; so is this clean link. Try the release build
first — if it dies, fall back to the throwaway release test below rather
than assuming it will always fail.

Take timings through a throwaway release test in `pdfcer-render` instead
(`cargo test -p pdfcer-render --release --test <name> -- --nocapture`); it
builds in a couple of minutes and can call `render_page` directly. Hash
`out.pixmap.data()` in the same test and you get the A/B and the
byte-identity proof from one run. Delete the file before committing.

### TWO METHODOLOGY LESSONS FROM THIS ARC

Two paragraphs that stood here were DELETED rather than struck, because they
had become false: they described the group-buffer work as "unstarted" and
named the per-group allocation as the time cost. `Pass 300.2` shipped the fix
and measured the allocation at 1.8 s of 54.9 — see the table above. A handoff
that contradicts itself is worse than one merely out of date, because the
reader cannot tell which half is current.

**And the one that cost the most time in the arc:**
the regression baseline built before touching the code rendered **114 fixtures
and stopped at `fontinfo` alphabetically** — it did not contain `images`,
`transparency`, `overprint` or `shading`, the four directories the change was
most likely to break. It would have certified the change while testing none of
it. **A baseline that omits the directories your change touches certifies
nothing.** The real verification was a stash / rebuild / re-render of all 364
synthetic fixtures, byte-compared: identical.

### What shipped: one inbound batch, seven Passes, in one evening

Every one answers a request from `pdfcer-gui`.

| Pass | commit | what |
|---|---|---|
| `296.0` | `69d4d67` | **a deep-zoom region render REFUSES instead of killing the worker** — `RenderError::RasterizerLimit`, the crate's only `catch_unwind` |
| `296.1` | `141c989` | **a coverage refusal carries its remedy faces as DATA** — `Refusal::remedy_faces`, page-verified |
| `296.2` | `90576a8` | **`Display for Object` and `for Name`** — scalars exact, containers named not expanded |
| `296.3` | `5943beb` | **a PATTERN redaction reports the text it could not read** — and pdfcer's own CLI `--pattern` was silent too |
| `296.4` | `8d2f6bb` | **`page_composites_in_ink`** — ask before rendering, not after |
| `296.5` | `4f6f5a5` | the rasteriser's panic text out of the error MESSAGE |
| `296.8` | `f392b19` | `BlendSpaceFrom::token()` public |

Plus `f16e266` + `5917ece`, the pre-push gate fixes above (filed as fixes, not
Passes — `ROADMAP.md` has a commit-hash-heading precedent for that).

Filings 508–512. Decisions **151**, **152**, **153**; rules **R252**, **R253**,
**R254** minted. `R245`'s 8th dated instance.

---

## THE INBOUND QUEUE IS EMPTY — AND THAT SENTENCE HAS BEEN WRONG TWICE

**Every `pdfcer-gui` request is answered, shipped and confirmed consumed**, and
each consumption note is in the channel. Five `request_*` files remain in
`open/` only because **archiving is the GUI side's step** — they close their own
exchanges within minutes and write the `INDEX.md` rows themselves. Do not
archive on their behalf; you will duplicate work in flight.

**Two previous handoffs said "the queue is empty" and were stale within
hours.** `ls -lt` the inbound directory before believing any sentence in this
file — including this one.

`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`

### The channel now has a TOPIC KEY — use it

Adopted 2026-09-11, recorded in that folder's `README.md`. Every file in one
exchange carries the same key as a filename prefix: `reply_G042_…`,
`done_G042_CONSUMED.md`. **`G###` is minted by pdfcer-gui, `E###` by this
side** — two counters so a collision is impossible without coordination, which
nothing in a folder outside git can provide. The key names the **exchange**,
not the file: one defect filed twice gets one key.

It exists so *a `reply_G*` with no `done_G*` is an answer nobody acted on* is
checkable in four lines of shell. Their audit found five shipped fixes still
described as broken — one **in the operator's manual** — for two to three days
each.

---

## WHAT THIS SESSION ESTABLISHED THAT OUTLIVES ITS PASSES

### R254 / decision 153 — a value the crate already computes does not earn `pub` by DEMAND

It earned it by existing. Keeping it `pub(crate)` "until someone asks" hands
the discovery cost to **the one party who structurally cannot see the gap**.

I read `R151` ("an uncalled API is a cost") as licensing that, and it does
not: R151 audits whether a *published* capability gets *called*. The librarian
declined both homes I proposed and minted a new rule; it also cut my claimed
five instances to **three** on mechanism. **A shared symptom is not a shared
mechanism** — that scepticism was right three times running this session, and
it is worth asking for explicitly in a dispatch.

### R253 / decision 152 — the SAFE rendering must be the DEFAULT one

`Pass 296.0` put a third-party panic string in an error's `Display` and told
callers not to match on it. A consuming shell routes `Display` onto the page on
purpose — so the default path would have painted
`range start index 442613758592 out of range for slice of length 1088737`
across a site plan. **A variant safe only for a consumer who writes a named arm
is unsafe for every consumer who has not read its doc comment.**

### A measurement that refused to become a constant

`Pass 296.0`'s requester preferred a published max scale "because it names the
number". `examples/region_panic_ceiling.rs` bisected six page geometries and
got **three values ordering with nothing** — the largest sheet the most
fragile, an A1 and a business card sharing a boundary A4 never reaches.

⇒ **When the measurement does not support a constant, publishing one anyway is
an invented number wearing a measurement's clothes.** The guarantee became the
refusal; the constant is published only as a FLOOR below the lowest row,
checked against the table at compile time (`const _: () = assert!(…)`).

### A consumer's WORKAROUND is a defect report

Twice in one evening, both under decision 058: the `search_text` double
extraction (`296.3`) and the named arm hiding the panic text (`296.5`). **The
shell filed rather than worked around six times in two days.** That frequency
is evidence about where the boundary is drawn, not about them.

### A diagnostic's EXCERPT is not its finding

`5917ece` exists because I fixed the gap `check-string-gaps.sh` quoted and not
the second one on the same line, past its ~100-character truncation.
**Re-run the check; do not act on the printed excerpt.** (Already a
cross-project lesson at `C:\personal_rag\claude_code\lesson_20260807_truncated_read_of_wrapped_sentence.md`.)

---

## HABITS

- **`tools/edit-source.py` for every multi-line source edit.** I used ad-hoc
  python heredocs instead and it cost two of the three gate defects above —
  eaten backslashes and a doc-block splice. The machinery existed and was not
  reached for.
- **`git commit -F <file>`, never `-m`.** Unbroken.
- **Sabotage every new test.** Every test this session was falsified before
  being believed; two sabotages found that a single break turned *two* tests
  red, which is what a contract pinned in two places should do.
- **Never chain a reverting git verb.** A hook blocks it, correctly — run
  `git checkout --`, `git reset`, `git restore` **alone**.

---

## OWED (carried forward, plus this session's)

- ~~no `docs/core-api/` entry for the `offpage` module~~ — **CLOSED
  2026-09-11**, `bfa981b`, as §13 of `03-capabilities.md`. Struck rather than
  deleted so a reader who remembers it owed can see it moved.
- **`redact-offpage` residuals: ~~17 files / 23 objects~~ → 7 files / 12
  objects**, all of the `partial` kind. `Pass 297.0` (`536ef3b`) closed the
  fully-off half by making `wholly_covered` test the UNION of the bands, not
  one band — measured before and after on the operator's own 17 affected
  drawings.

  **What remains is NOT a cut that leaves a sliver** — that was this file's
  wording and it was wrong, corrected 2026-09-12 (`7a22c52`). The cut is
  COMPLETE: `covered_cells` snaps **outward**, so an image overhanging by 1 pt
  has its off-page sample columns cleared and the samples out there are blank.
  What clearing cannot do is move the **placement**, so the bbox still crosses
  the edge and `scan-offpage` — which classifies by GEOMETRY — still counts it.
  **The scan is reporting its own output**, the same shape as the empty text
  husk `Pass 294.2` fixed, one type over.

  ⇒ The fix is to stop counting an image whose off-page cells carry no ink, and
  it is **not** free: it needs the samples, and decoding every image during a
  scan is what made `redact-offpage` take ten minutes on one file
  (`Pass 294.1`). **It wants a measurement — how many placements, how much
  decode — before any code.** Anyone hunting a cutting defect here will find
  nothing wrong; that is the trap this paragraph exists to spring.
- **6 tests silently SKIP and report as passed** — ~~26~~ → ~~16~~ → ~~10~~ →
  ~~8~~ → **6**, four paydowns on 2026-09-12 (`f0d1dc7`, `e41892a`, `d291a03`,
  `05b3a80`).

  **THE SIX ARE TWO DIFFERENT KINDS — do not read them as one pile:**
  - **four** in `widget_adoption.rs` (the census and three preview tests) are
    **deliberate**: they assert the REAL AcroForm's own composition, so
    converting them would measure an invented fixture rather than the verb;
  - **two** in `stamp_collection.rs` read **Adobe's own installed stamp files
    from `%APPDATA%`** — the operator's machine, not a corpus. There may be no
    other way to test "we read Adobe's real files", but it means those two
    **can never run in CI on any machine**, which is a different problem from
    the corpus one and wants its own answer.
  `Pass 298.0`'s guard was sabotaged to prove its test could fail and the test
  **stayed green**. **The only fix is a synthetic fixture, and that is a
  constraint rather than a preference**: 13 of the 16 need
  `fixtures/external/pdfbox`, which `fixtures/README.md` marks *"NOT
  blanket-safe … never bulk-import"*, and `fetch-corpora.sh` deliberately omits
  it; 3 need `qpdf`, never fetched either. **"Fetch the corpus in CI" is
  REFUSED, not un-chosen** — it is the obvious two-line idea and it would bulk
  import a corpus `LEGAL.md` §5 rules out. `tools/check-skippable-tests-declared.py`
  keeps the count honest; `f0d1dc7` shows the pattern
  (`synthetic_orphaned_session()` in `widget_adoption.rs`, 14 skips → 4).
  Four of the remaining ten are deliberate: `widget_adoption.rs`'s census
  and preview tests assert the REAL AcroForm's composition, and converting
  them would measure an invented fixture rather than the verb. Do not "finish
  the job" on those.

  **The criterion that decides convertibility**, from doing it twice: *are
  the numbers the SUBJECT or the SETTING?* `merge_document.rs`'s "12 fields
  over 13 widgets" is a property of the fixture — a synthetic source with the
  same composition tests the same thing, so all eight converted. The preview
  tests assert the corpus's own composition, which is the subject, so they
  cannot.

  **`clippy::dead_code` proves a conversion is complete**, better than a
  SKIP count: when the last test stops using it, the corpus path constant
  becomes unused and the compiler names it. `merge_document.rs`'s `ACROFORM`
  is gone for that reason.

  ~~THE NEXT THREE ARE BLOCKED ON A FIXTURE NOBODY CAN GENERATE~~ —
  **RESOLVED the same day** (`d291a03`), by the operator asking whether a
  suitable PDF could be found online. It could: `LEGAL.md` §5 already names
  **veraPDF's open corpus** as approved source (b), so it was a documented
  decision rather than a search. `fixtures/verapdf/object-streams.pdf` is the
  smallest VALID file of the 78 in that corpus carrying an `/ObjStm`.

  **The analysis that said "blocked" was still right and is still worth
  keeping**: pdfcer's writer only DEcompresses (`writer/save.rs:1018`), and no
  `fixtures/synthetic/**` file contains an `/ObjStm`. What changed was not the
  facts but the question — *generate one* is blocked; *use a cleared one* was
  never blocked and was already permitted.

  **`fixtures/verapdf/` is the first non-MIT file in this tree** — the
  corpus is **CC BY 4.0**, redistribution permitted with attribution, which
  `fixtures/verapdf/PROVENANCE.md` carries. **OWED, and it is the operator's
  call:** whether `LEGAL.md` should gain an explicit line recording that, the
  way §6.7 does for the CC-BY-SA-4.0 OCR weights. Flagged by the librarian,
  not edited — that file is operator-governed.

  **And one of those three never needed a corpus at all.** It needed *an
  encrypted document*, and `fixtures/synthetic/encryption/` has held eight the
  whole time. ~~Before hunting any more fixtures, re-check the remaining 8~~ —
  **DONE the same hour** (`05b3a80`): two more converted, neither needing
  anything new.

  **And the re-check found a defect the note itself had created.**
  `structure_inspect`'s object-stream test was **still skipping for want of the
  fixture added an hour earlier** — its path was repointed, but a SECOND guard
  clause further down (`if l.object_streams.is_empty() { SKIP }`) was the real
  gate. ⇒ **Repointing a fixture is the visible half; a decline further down is
  invisible in the diff and keeps the test dead.** When you convert a test,
  grep its whole body for the skip idiom, not just its path.
- **NEW — above ~1e8 scale a region render succeeds again** with an underflowed
  page-space span. Nothing panics; whether those pixels mean anything is its
  own measurement. Told the shell rather than letting them discover it.
- **NEW — `check-reexport-closure.py` checks FIELDS, not method return types.**
  A verb returning an un-re-exported type is the same class. Widen it against a
  measurement, not a guess.
- **`R221`'s recorded instance count is wrong** and a commit message made it
  worse. **Do not copy an ordinal from a commit message.**
- ~~**`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.~~
  **BUILT 2026-09-13.** Red on one state only: a request scoped in
  `ROADMAP.md` with no answer in its channel. Green at baseline (10 open,
  10 answered). In CI it announces `SKIPPED` per channel by name — they live
  outside the repo — rather than passing silently (`R255`).
- **NEW, AND READ IT BEFORE AUDITING ANYTHING IN `D:\Dev\FeatureRequests\`:
  pdfcer answers TWO channels, not one.**
  - `pdfce_FeatureRequests` — the `pdfcer-gui` shell (6 open requests);
  - `iccce_FeatureRequests` — the ICC colour-management partner (4 open).

  `ROADMAP.md` cites files from **both**, and cites most of them BARE (no
  directory), so a scan scoped to one channel reports confident nonsense about
  the other. This cost four successive wrong answers in one hour: an audit of
  the register's reply citations reported 19 missing, then 15, then 10, and
  the true number is **0**. Every correction found a new convention rather
  than a real gap.

  The three conventions that broke it, all of them legitimate:
  1. `archive/` **prefixes with a date**: `reply_2026-09-09-x.md` in `open/`
     becomes `2026-09-09-reply-x.md` once archived — the date moves to the
     front and `reply_` becomes `reply-`.
  2. **Archiving can RENAME the subject.** `reply_G010_renamed_to_...` is
     filed as `2026-09-12-G010-period-in-partial-name-reply.md`. No
     normalisation matches that; only reading the file does.
  3. Some register citations **were never filenames** — a librarian paraphrase
     of a reply's content, or a forward-looking name for a reply not yet
     written, both of which read exactly like a path.

  ⇒ **A citation of a file owned by another project is a citation of a name
  that project may change.** Neither side is wrong: the register records the
  name as filed, the channel renames on archive. No gate was minted for it —
  nothing mechanical can tell which of two ungoverned directories a bare name
  belongs to. The habit instead: **name the channel directory whenever the two
  could be confused, and quote a reply's saved filename, never a paraphrase.**
- ~~**`check-public-fns-documented.py`'s denominator is `pub`** … staged fix~~
  — **MEASURED AND DECLINED 2026-09-11.** Widening it to private functions
  would mean a **1,837-row** baseline outside test modules, which is an
  instrument nobody reads. My first measurement said 381 and was wrong —
  an artifact of cutting each file at its first `#[cfg(test)]` line — and I
  nearly shipped the widening on it. The answer instead is
  `tools/check-doc-block-spliced.py`, which detects the splice directly (one
  doc block containing the same heading twice) and needs no denominator at
  all. It found **four live splices** and **five baseline rows that were
  misfiled text rather than missing text**.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded.**
  `LEGAL.md` §5 makes this a licensing statement.
- **Backup bundle is well over 150 commits behind `HEAD`.**
- **143 of 187 standing rules are unenforced** — the operator's own next piece
  of work: *"script it or bin it."*
- **`personal_rag/pdf` entry on the operator's stamp file** (black-background
  `/DCTDecode` with no `/SMask`) — verify it landed from the 496th filing.

### The operator's own ordered plan, still the front of the queue

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).

---

## BUILD ENVIRONMENT

**This machine runs out of memory on whole-workspace cargo work.** See the
procedure at the top; it is the single most time-costly thing about this
session.

**`target/debug/deps` grows without bound** — cargo never garbage-collects
it. `du -sh target/debug/deps` every session; 36 GB at one recent measurement.
Before any delete, both checks: `git ls-files target` returns 0 and
`git check-ignore -q target` passes.

**A stale `types.py` in the job temp directory shadowed the standard
library** and broke every `python` invocation whose script lived there, with a
traceback naming `enum`, not the shadowing file. If `python` starts failing on
`import pathlib`, look for a stdlib name in the working directory first.
