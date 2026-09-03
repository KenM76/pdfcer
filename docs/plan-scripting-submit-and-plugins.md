# PLAN — Making push buttons work: actions, submission, and an out-of-tree scripting/network plugin model

**★★ STATUS AMENDED 2026-08-30 — PHASE 1 IS BUILT AND SHIPPED.** The banner
below said *"Nothing here is built"* and that was true for four days. It is
kept legible rather than silently rewritten, because a plan that quietly
becomes a status report is a plan nobody can date.

- **§8 Phase 1 (action authoring) — SHIPPED**, as `Pass 182.0` (`bc49a8e`,
  `/ResetForm`) and `Pass 183.0` (`cff102a`, `/SubmitForm`, `/GoTo`,
  `/Named`, `/URI`, and the payload disclosure of §6.3/§6.3.1). **Two items
  of Phase 1 are NOT built**: `/AP` `/D` (the pressed appearance) and `/MK`
  icon/label layout.
- **§11 prerequisite 3 — DISCHARGED** before Phase 1, as it required:
  `Pass 133.0` (`afd8da8`) fixed the `/A`-versus-`/AA` hazard blindness of
  §2.7. Verified again on 2026-08-30 on the shipped binary — `inspect`,
  `list-fields` and `list-annotations` all disclose a pdfcer-authored submit.
- **§8 Phases 2–5 — UNBUILT and unchanged.** Nothing dispatches. `R54` is
  **not engaged** by Phase 1 and its decision-088 amendment is **not relied
  on**; `R12` is not engaged either, since no crate gained network code.
- **★ §1's O4/O5 ladder was written for DISPATCH, and three of its four rungs
  are unreachable from authoring.** Only rung 3 (payload disclosure) survived
  into Phase 1 — and it turned out to be the rung with the most in it. A
  future session reading §6 as a Phase-1 checklist will look for a whitelist
  and a redirect check that have nowhere to live yet.

**Original status, retained: DRAFT PLAN. Nothing here is built, filed, or
decided beyond the operator rulings quoted verbatim in §1.** Authored by
`pdfcer-engineer`, 2026-08-26, at the operator's request ("draft a full plan
now on what we have"). This document is engineer-owned, in the same class as
`docs/ocr-engine-survey.md` — a sourcing-and-design record that precedes a
decision record, not a substitute for one.

**This document does not itself amend any standing rule.** §3 names the rules
that have to be amended, by number, and states who owns each amendment. An
amendment happens in `ARCHITECTURE.md` §12 via `pdfcer-librarian`, on an
explicit operator ruling, and not before.

**★ UPDATED 2026-08-26, same day: three of §9's open questions were ruled on
by the operator within the hour** — see **O6/O7/O8** in §1. `R54` **is to be
amended** (Q1 = yes), the plugin boundary **is a message format** (§5.1
confirmed), and the `R53` reversal **is deferred** (Q2 = defer), with
**Phases 1–3 planned for delivery without touching it**. The librarian filing
that carries the amendment into the rule text was dispatched the same day;
until it lands, **`R54`'s text on the books is still the unamended one**, and
no code may rely on the amendment before it is filed.

---

## 0. Reader's orientation — the one-paragraph version

Making a push button work splits into three genuinely separate problems that
this plan keeps separate throughout: **(a)** authoring an action into the
file, **(b)** honouring an action when an operator presses the button, and
**(c)** executing embedded JavaScript. (a) and (b) are small and safe; (b) is
blocked on a rule written for (c). (c) is large, is prohibited scope today,
and is the subject of the plugin proposal. **The single most consequential
finding in this plan is that (b) is blocked by a rule nobody was thinking
about — `R54`, "no trigger event ever fires" — which is separate from the
JavaScript rule and bites a plain, script-free Reset button.**

**★ AMENDED 2026-08-30. (a) IS BUILT** — see the status banner at the head of
this document. This paragraph opened by saying pdfcer *"can already create
push buttons … that do **nothing** when pressed, because pdfcer authors no
action on them and fires no trigger for one"*, and **half of that is now
false**: `EditSession::set_button_action` authors `/ResetForm`,
`/SubmitForm`, `/GoTo`, `/Named` and `/URI` (`Pass 182.0`/`Pass 183.0`).
**Creation** still authors none, deliberately; **firing** still never
happens, so (b) is untouched and `R54` is not engaged.

Recorded here rather than rewritten away because of the shape it repeats: the
banner at the head of this file was updated in the same edit that left this
sentence alone. A correction that reaches a status block and stops short of
the prose is this project's most-repeated documentation defect, and the
sentence it stopped short of is always the one a reader actually reads.

---

## 1. The operator's rulings, verbatim and dated

All from 2026-08-26, in conversation, in this order. Quoted rather than
paraphrased because each one is an authorisation whose exact scope matters.

| # | Ruling | Scope it settles | Scope it does NOT settle |
|---|---|---|---|
| **O1** | *"would it work to create a separate project called `D:\Dev\pdfceJS` which developes javascript support, then in order to have Javascript support in pdfcer the folder `plugins\pdfceJS` would have to exist in pdfcer's folder?"* | A proposal, framed as a question — the **presence of an installed folder** is the capability gate. Assessed in §5; the answer is a qualified yes. | Not itself an authorisation to build. Explicitly prefaced *"plan only, we wont start it yet."* |
| **O2** | *"we'd let it do the server thing as long as it matches the same security that adobe already allows."* | Form submission is **permitted in principle**, with Acrobat's own security model as the acceptance criterion. | What Acrobat's model actually *is* — researched, §7. |
| **O3** | *"I assume that is still written into the separate script side so pdfcer still stays server free."* | An **assumption**, tested in §5.2. Mechanically sound; the plan nevertheless relocates the network capability out of the scripting plugin, for reasons in §5.2. | — |
| **O4** | *"We'll allow a submit to send filled data wherever the document's author said."* | **Default destination policy: open.** No host whitelist by default, no blocked hosts, the document author chooses. | Disclosure (settled by O5), payload scope, transport security. |
| **O5** | *"yes well we'll have support for allowing only submission to whitelists, and your default is good, plus we can show what is being sent as an option too."* | Three things: **(i)** an operator-owned whitelist-only mode exists as an option; **(ii)** the engineer-proposed default — *disclose the destination before anything leaves* — is **approved**; **(iii)** payload disclosure ("show what is being sent") exists as an option. | Which of (i)/(iii) is on by default; see §9 Q3. |

### 1.1 Rulings on this plan's own open questions (same day, after §9 was written)

| # | Ruling | What it settles |
|---|---|---|
| **O6** | *"change the rule"* | **§9 Q1 = YES. `R54` is to be amended** to permit trigger dispatch for an enumerated safe allow-list. This unblocks Phase 2 — a button press may fire a trigger whose action subtype is on the list, and every other subtype is refused by name. **Does not touch `R53`**, which O8 explicitly defers. |
| **O7** | *"make it a message format so a web version is easier to make"* | **§5.1 confirmed, and confirmed for the stated reason.** The plugin boundary is a **versioned message protocol**, not a binary artefact — which makes the in-process `.dll` option dead, not merely disfavoured, and makes the web fork a matter of supplying another implementation of the same protocol. The operator named the web-fork rationale himself; it is the *reason for* the choice, not a side benefit of it. |
| **O8** | *"defer for now and plan to deliver the first 3 phases without touching it"* | **§9 Q2 = DEFER.** The `R53` reversal is not sought now. **Phases 1, 2 and 3 are the delivery plan**, and each must stand up **without** any JavaScript execution capability. Phase 4 (`pdfceJS`) and Phase 5 (in-app download) stay planned-but-unscheduled. |

**★ Scope note, so a future session does not over-read O6.** *"Change the
rule"* is singular and answers Q1. It is **not** authority to touch `R53`
(O8 defers that in the same breath), nor `R13` clause 5, nor `R12`'s
new-destination-class record — each of those is still owed separately, and
§3 keeps them itemised.

**★ Scope note on "deliver".** O8 says *"plan to deliver"*. The standing
instruction from the start of this conversation — *"plan only, we wont start
it yet"* — has **not** been lifted. Phases 1–3 are the agreed delivery
*target*; no implementation Pass has been authorised to begin.

---

**Reading note for a future session.** O4 read, at the time, like a
preference for minimal friction. It was not — O5 arrived unprompted and
added two restrictions the engineer had not proposed. **O4 is a ruling about
the DEFAULT, not about the ceiling.** Do not cite O4 as authority for
building a permissive-only design.

---

## 2. What exists today — measured, not assumed

Everything in this section was verified against the tree at the time of
writing, not recalled.

### 2.1 Push-button authoring: shipped

`EditSession::add_push_button` / `NewPushButton` (core), `add-push-button`
(CLI). Writes `/FT /Btn` with `Pushbutton` (bit 17) set, no `/V`, no `/DV`,
no `/AS`, a plain-stream `/AP` `/N` (not the state-keyed sub-dictionary a
check box gets), `/MK` `/CA` for the caption, `/BS`, `/F`. Merging a second
widget under one field name works and each widget keeps its own caption.
Undo reproduces the input byte-for-byte.

**Not reachable in `pdfcer-gui`** — field creation is unbuilt there; the
type palette shows push button absent rather than greyed.

### 2.2 Action authoring: did not exist, anywhere — **★ AMENDED 2026-08-30, this is the bullet Phase 1 retired**

Kept in the past tense with the measurement intact, following §2.3's
precedent, because §2 is a dated snapshot and its value is what was true when
the plan was written.

**As of 2026-08-30 there IS a code path that writes an `/A` entry**:
`EditSession::set_button_action`, on a push button's widget, authoring
`/ResetForm` (`Pass 182.0`, `bc49a8e`) and `/SubmitForm`, `/GoTo`, `/Named`,
`/URI` (`Pass 183.0`, `cff102a`). **Link-annotation authoring is still
absent** — `grep` for `add_link` / `NewLink` still returns nothing — and the
`/A`-writing primitive built here is the one a future link Pass would reuse,
exactly as §8 Phase 1 predicted.

Original measurement, 2026-08-26: *"There is **no code path in pdfcer that
writes an `/A` entry**, on a push button or on anything else. … This is not a
gap in the button feature; it is a capability the project has never had."*

Every push button pdfcer **creates** is still disclosed as inert on both
channels (`FieldAuthorDisclosures::push_button_inert`), which is why this
never silently surprised anyone. **The flag is unchanged; three sentences
explaining it said "pdfcer never authors one" and were corrected in
`cff102a`.**

### 2.3 Action *reading*: partially exists, and is good

- `outline.rs` resolves `/GoTo` (Table 199) and `/GoToR` (Table 200)
  destinations, including the remote-file case and its
  page-index-in-another-document trap.
- `forms.rs` classifies action subtypes and counts hazards. **★ AMENDED
  2026-08-26 by `Pass 133.0` (`afd8da8`) — this bullet said `/AA` action
  subtypes, and that WAS the defect**: the scan walked `/AA` only, so a
  submit in a widget's `/A` counted as nothing. It now walks all 17 carrier
  sites and follows `/Next` chains, and the hazard list is longer than the
  four named here — `GoToR`, `GoToE`, `Thread`, `Movie` and `Rendition` all
  reach a file or a URL. See `iso32000__ref__action_carriers.md`.
- `pageops/references.rs` rewrites destinations correctly across page
  insert/delete/reorder, and knows that a `/GoToR` page index must **not**
  be resolved against this document.

So pdfcer already understands the action model as *data*. What it lacks is
the authoring half and the dispatch half.

### 2.4 Reset: the verb already exists, without the action

`EditSession::reset_form(only: Option<&[String]>)` is shipped, with a
`reset_form_preview` planning sibling, a `CommandKind::ResetForm` undo
entry, and CLI `reset-form`. It correctly skips pushbuttons, signature
fields and read-only fields, and reports each skip by reason.

**This is the plan's cheapest win and it is nearly free:** authoring
`<< /S /ResetForm >>` onto a button means binding a button press to a verb
pdfcer already implements, tests and can undo.

### 2.5 JavaScript: posture A and posture B both shipped

- **Posture A** — recognise, classify, count, disclose, byte-preserve.
  Shipped.
- **Posture B** — native Rust reimplementation of an exact-match built-in
  whitelist. Shipped `Pass 7.2` (`form_script/`, CLI `list-scripts` /
  `recompute`). `AFSimple_Calculate` SUM/AVG/PRD/MIN/MAX changes `/V`; the
  `AF*_Format` family changes display only. Off by default (no `--apply` =
  plan only). Follows `/CO` order, with a checkable dependency-order
  fallback and cycle detection when `/CO` is absent or partial.
- **Posture C** — a sandboxed JS engine. **Prohibited scope** (`R53`).

**Consequence for scoping: the common case already works.** Totals, dates,
percentages and the standard formatting masks compute today without
executing anything. A JavaScript engine would serve the long tail of
hand-written scripts, not "my form doesn't add up."

### 2.6 Network: a primitive exists, deliberately unwired

`crates/pdfcer-fetch` is a workspace member (`Pass 77.0`). **No shell links
it.** A default build contains no network code at all, and the fail-closed
`no-network` CI job proves it for `pdfcer-core` + `pdfcer-render`
specifically.

This matters to the plan: **the "network capability as a separable,
opt-in, provably-absent piece" pattern is already built and proven in this
project.** Submission should ride it rather than invent a second route.

---

### 2.7 ★★★ A SHIPPED DEFECT FOUND WHILE BUILDING THE PROBE — pdfcer cannot see a submit button at all

> **★ FIXED 2026-08-26, `Pass 133.0` (`afd8da8`), the same day this was
> written.** Everything below is kept in the PRESENT TENSE as it was written,
> because the section's value is now the diagnosis rather than the status —
> and rewriting it to say "was" would erase the measurement that made the case.
> What shipped went considerably further than this section proposes: the
> repair was built from the whole carrier set (17 sites; `/A` + `/AA` cover
> 11) rather than from the widget, and it follows `/Next` chains, which this
> section does not mention and which is what makes a per-carrier scan unsafe
> rather than merely incomplete. **The three surfaces in the table below all
> disclose now.**

Discovered 2026-08-26 by running pdfcer over the probe PDFs. **This is a
defect in shipped disclosure code, not a gap in the unbuilt feature**, and
it is the single most actionable item in this document.

`scan_javascript` / `FormJavaScript` exist, in their own doc comment's
words, *"to disclose what a document would run in Acrobat/Reader"*, and
`network_action_count` is documented as flagging *"`/AA` actions
referencing the network — `/URI`, `/SubmitForm`, or `/ImportData`"*.

**It scans `/AA` only. A widget's PRIMARY action lives in `/A`.**

A push button's submit, its URI, its launch — all of them sit in `/A`.
`/AA` on a widget holds the *additional* actions (`/E` `/X` `/D` `/U` `/Fo`
`/Bl`). So the counter that exists to say "this document would reach the
network" **misses the single commonest carrier of a network action in the
entire format.**

**Measured, not inferred.** On `probe_declared_http.pdf`, whose button
carries `/A << /S /SubmitForm /F (http://127.0.0.1:8765/declared-http) >>`
and which Acrobat then actually submitted to that address:

| surface | what it said |
|---|---|
| `list-fields` | `aa=0 … js_network_actions=0 js_launch_actions=0` |
| `inspect` | no action, submit, network, URI or launch line at all |
| `list-annotations` | `subtype=Widget … author="Go"` — no action |

**Three surfaces, none disclosing it.** An operator asking pdfcer "is this
document going to phone home?" is told *no* about a file that demonstrably
does.

★ **The shape is the one this project keeps meeting:** a check that
under-reports reads as a clean bill of health. It is the same class as the
ledger gate fixed the same day (§11.2) and as `R53`–`R57`'s own recorded
history. **Two independent lines of evidence agree** — the doc comment says
`/AA`, and a crafted `/A`-only file counts zero.

**Fix shape** (a real Pass, with tests and a filing — not a one-liner): walk
widget `/A` alongside `/AA`, and — since `/A` also appears on link
annotations, outlines and `/OpenAction` — decide deliberately which carriers
are in scope rather than patching the one that was noticed. Counting is
recognition only; it fires no trigger and needs no `R54` amendment.

**Do this BEFORE any of Phase 1.** Authoring submit actions into files while
pdfcer cannot report the ones already there would ship the write half of a
capability whose read half is blind.

---

## 3. The rules this collides with — named exactly

This is the section a future session should read before anything else,
because the collisions are not where they look.

### 3.1 `R54` — "no trigger event ever fires". **THE ACTUAL BLOCKER.**

> *"**R54** — no trigger event ever fires, on load or on any interaction.
> … Recognition is pure data modeling — there is no JS action dispatcher in
> pdfcer and none is added."*

**This forbids honouring a plain, script-free `/ResetForm` action.** It is
not a JavaScript rule. It says *no trigger fires*, full stop — and a mouse
press on a widget with an `/A` entry is a trigger.

★ **This is the finding that reframes the whole "make buttons work"
question.** The obvious assumption is that buttons are blocked by the
JavaScript prohibition. They are not. They are blocked by a rule written as
a *companion* to the JavaScript prohibition, whose text is broader than its
motivation. `R54`'s own stated justification is that trigger actions *can*
reference `/URI` // `/SubmitForm` // `/ImportData` // `/Launch` — i.e. the
rule is broad because *some* actions are dangerous, not because *all* are.

**Amendment needed: yes, and it is the narrowest one in this plan.** The
shape that matches `R54`'s own reasoning is a **dispatch allow-list**: a
trigger may fire if and only if its action subtype is on an explicitly
enumerated safe list, and every other subtype is refused *by name* rather
than ignored. That preserves `R54`'s intent exactly while unblocking (b).

**★ RULED 2026-08-26 (O6): the operator said *"change the rule."* The
amendment is authorised.** The allow-list shape above is what gets filed.
Two things a future session must not slide past:

1. **The amendment is not retroactive to the code.** Until `pdfcer-librarian`
   carries it into the rule text, `R54` on the books still reads *"no trigger
   event ever fires"*, and a Pass may not cite an amendment that has not
   landed.
2. **`R54`'s refusal half survives the amendment and is the load-bearing
   half.** An action subtype that is not on the list is **refused by name**,
   never silently ignored. A trigger that quietly does nothing is
   indistinguishable from a broken button, which is the failure mode the
   whole disclosure posture exists to prevent.

### 3.2 `R53` — "pdfcer never executes embedded PDF JavaScript"

> *"…adding one (posture C — a sandboxed JS engine) is **prohibited scope,
> not deferred scope**."*

The pdfceJS proposal (O1) **is posture C**. A plugin folder does not change
what the code does when it runs; it changes who ships it and whether it is
present. **Reversing a prohibition requires a new decision record that
supersedes decision 009 §5 openly** — not a workaround that leaves `R53`
on the books while routing around it.

★ **Do not let the plugin architecture launder this.** The strongest
argument *for* the proposal is honest and should be made on its merits
(§5.4): it converts an always-on capability into an operator-installed,
provably-absent-by-default one, which is materially better than the
in-tree engine decision 009 rejected. That argument may well win. It must
be *made*, not sidestepped.

**Owner: operator ruling required.** Not the engineer's to reverse.

### 3.3 `R13` clause 5 — "never executes anything it fetched"

Already flagged as an unresolved direct collision by decision 061 §7, and
already sitting in *Backlog* awaiting the operator. **An add-in is executed
code.** Whether a downloaded-then-loaded pdfceJS is "something it fetched"
depends on a ruling nobody has made.

★ **This plan is the case that forces that ruling.** Note the sequencing
consequence: if pdfceJS is only ever *hand-installed* by the operator
(copied into the folder, never downloaded by pdfcer), clause 5 arguably is
not engaged at all, because pdfcer fetched nothing. **That is a genuinely
cheaper first step and the plan adopts it** (§8, Phase 3) — hand-install
only, no in-app download, which defers the `R13` ruling rather than
forcing it.

**Owner: operator ruling required, but deferrable by design choice.**

### 3.4 `R12` as narrowed (decision 061) — the network rule

- `pdfcer-core`, `pdfcer-render`: **may never contain network code, under any
  future decision.** Untouched by this plan and untouchable.
- `pdfcer`, `pdfce-gui`, `tools/`: **may** carry a network client for
  **operator-initiated** fetching. No decision record needed for that class.

★ **But a submit is a new class, and the rule does not cover it.** Every
permitted case so far means *pdfcer goes where pdfcer's authors said* — a
model file, an update. A submit means *pdfcer sends the operator's data
where a FILE's author said*. The destination is attacker-controlled in the
threat-model sense. **This needs its own decision record naming the crate
and the feature**, per `R12`'s own unlocking clause. O2/O4 authorise the
capability; they do not write the record.

Note also `ARCHITECTURE.md` §1.1 **clause 2** (no network call that fires
without the operator asking at the moment it happens). A submit is
operator-initiated by construction — they pressed the button — so clause 2
is satisfied *on its face*. §6.2 explains why "they pressed the button" is
weaker than it sounds.

### 3.5 `R55` — JS carriers byte-preserved. **Untouched, and a constraint.**

All `/JS` strings/streams, `/AA` dicts, `/CO`, the `/Names /JavaScript`
tree and `/OpenAction` must round-trip byte-identical. Nothing in this plan
strips or bakes a script. A plugin that *ran* a script would still leave
the script in the file, exactly as posture B does.

### 3.6 Incidental finding — `R56`'s text is stale against rule 4

`R56` currently reads *"every recomputed value is a reviewable hint the
operator accepts or overrides (rule 4)"*. That is rule 4's **pre-narrowing**
language, and decision 059 (2026-08-13) rejected the accept/override framing
**by name**. The behaviour `R56` governs is almost certainly still correct
(posture B is off by default and plans before it applies); it is the
*wording* that now cites a superseded reading of rule 4.

**Not fixed here** — rule text is librarian-owned. Flagged for filing.

---

## 4. Buttons are not JavaScript — the scoping insight

**Most push buttons in the wild carry no script at all.** Submit, reset,
print, next-page, previous-page and open-a-URL are plain declared actions
(§12.6.4, Tables 199–217). They need no interpreter.

This has three consequences that shape the whole phasing in §8:

1. **Do not build pdfceJS to make buttons work.** The two problems were
   conflated in the originating question and separating them is most of the
   value of this plan.
2. **The network capability must NOT live inside the scripting plugin**
   (contra O3's assumption) — otherwise a plain submit button, the common
   case, would require installing a *scripting engine*. See §5.2.
3. **A useful, shippable button feature exists entirely inside pdfcer's
   current rules, minus one narrow `R54` amendment.** Reset and in-document
   navigation need no plugin, no network, no interpreter, and no new
   dependency.

---

## 5. Architecture

### 5.1 The plugin boundary — three candidates, one recommendation

| | In-process library (`.dll`) | **Separate process (recommended)** | Wasm module |
|---|---|---|---|
| Isolation | **None.** Runs in pdfcer's address space with pdfcer's file handles and privileges. A bug in the engine is a pdfcer compromise. | **Process boundary.** Can be run with reduced privileges; a crash is contained and reportable. | **Strongest.** Capability-based by construction. |
| Rust ABI problem | Severe — no stable Rust ABI, so a hand-versioned `extern "C"` surface is mandatory and fragile. | **None.** The interface is a message format, not a symbol table. | None. |
| Implementation language | Must match/pin the toolchain. | **Any.** pdfceJS could wrap any engine in any language. | Anything targeting wasm. |
| Licence isolation | **No.** Linking into pdfcer's process is the classic derived-work case; a copyleft engine is unusable. | **Yes, arguably** — arm's-length separate program. Widens the viable engine set. **Operator's call, not the engineer's.** | Yes. |
| Crosses to the web fork | No. | No (but see below). | **Yes.** |
| Cost | Low. | Low–moderate (a protocol, a supervisor, a handshake). | High (a wasm runtime is a large dependency). |

**Recommendation: separate process, with the interface specified as a
versioned message protocol rather than a binary artefact.**

★ **The protocol-not-binary framing is what rescues the web fork.** If the
boundary is "these messages, this handshake", then the browser shell
supplies its own implementation of the same protocol — and in a browser, a
JavaScript engine is already present and free. Design the boundary this way
on day one and the web fork costs nearly nothing; bolt it on later and it
is a rewrite. This is the single highest-leverage design decision in the
document, and it costs nothing today.

**Rejected: in-process `.dll`.** It gives the weakest sandbox precisely
where the strongest is wanted, and it forecloses the licence question
permanently.

**★ RULED 2026-08-26 (O7): *"make it a message format so a web version is
easier to make."*** The recommendation is adopted, and the operator supplied
the web-fork rationale himself rather than accepting it as a side benefit.
Consequences that are now settled rather than recommended:

- **The in-process `.dll` option is dead**, not merely disfavoured. Do not
  re-propose it on performance grounds.
- **The protocol is the deliverable, and it is versioned from its first
  byte.** A protocol that ships unversioned acquires an implicit version
  anyway — the one its first consumer happened to observe — and pdfcer would
  then be maintaining compatibility with an accident.
- **The protocol must be specified without reference to any host language,
  process model or transport.** The moment it names a pipe handle, a Rust
  type or a file path, the browser implementation stops being a matter of
  supplying another endpoint. Specify messages and a handshake; leave
  transport to the shell.
- **This binds `pdfceNet` as well as `pdfceJS`.** Both plugins speak a
  protocol; neither is a linked artefact. That is what keeps §5.6's
  engine-side guarantees (no network, no process spawning in
  `pdfcer-core`/`pdfcer-render`) true by construction rather than by
  vigilance.

### 5.2 Two plugins, not one — `pdfceJS` and `pdfceNet`

O3 assumed the network code would live inside the scripting plugin. **It
should not**, for the reason in §4.2: a submit button needs no script.

| Plugin | Supplies | Required for |
|---|---|---|
| **`pdfceNet`** | An HTTP client and nothing else. Sends a prepared payload to a prepared destination; returns a status. | Plain `/SubmitForm` buttons. Scripted submits (together with `pdfceJS`). |
| **`pdfceJS`** | A JavaScript engine and the Acrobat-API binding surface. | Custom scripts beyond the built-in whitelist posture B already covers. |

Installed independently. Present-or-absent is the capability gate (O1's
mechanism, which is good and is kept). Each is absent from a default build,
so **O3's goal — "pdfcer stays server free" — is preserved and strengthened**:
a stock pdfcer has neither, and `cargo tree` on the shipped binary proves it.

`pdfceNet` should be a thin shell over the existing `pdfcer-fetch`
primitive rather than a second network implementation.

### 5.3 The protocol — effects, not just edits

An earlier framing in conversation ("the plugin hands back a list of
proposed changes") was **too narrow, twice over**, and the corrected shape
is:

The plugin returns a list of **effects**, each of which pdfcer either
applies as an ordinary undoable `EditSession` command, performs, or
refuses by name. Effect kinds:

| Effect | pdfcer's response |
|---|---|
| `SetFieldValue` | An ordinary undoable edit. Same path posture B's recompute already uses. |
| `SetFieldProperty` (visibility, read-only, required, colour) | Ditto. |
| `Message` / `Prompt` | **The gap in the earlier framing.** Real forms use alerts constantly for validation. Needs a UI channel; in `pdfcer`, printed. |
| `RejectKeystroke` / `ReplaceKeystroke` | **The second gap.** Acrobat's keystroke event inspects and can rewrite each character *as typed* — that is how a phone field refuses letters mid-entry. Requires the plugin consulted per keypress with the power to say no. Latency-sensitive; a local pipe round-trip is affordable, a process spawn per keystroke is not (hold one warm process per document). |
| `Submit` | Refused unless `pdfceNet` present **and** §6's posture ladder permits. |
| `Navigate` | In-document: performed. Remote/external: §6. |
| `DocumentStructure` (insert/delete pages, watermark, spawn from template) | **The honest tension, §5.5.** |

**Not supported, deliberately: timers / background scripts** (`setInterval`
and friends). A request-response protocol has no place for a script that
never stops. Near-zero use in real forms; unbounded cost.

**The plugin never touches the file, the filesystem, or the network
directly.** It receives a script plus a bounded view of field state, and
returns effects. That keeps every dangerous verb on pdfcer's side of the
boundary, where the posture ladder and the undo log already live.

### 5.4 The honest argument for the proposal, stated plainly

Decision 009 rejected posture C for a sandboxed engine **in the tree, in
the binary, always present**. The plugin model changes three facts that
decision reasoned from:

1. **Absent by default and provably so** — the capability does not exist in
   a stock build; its presence is a filesystem fact anyone can check
   without reading code or build flags.
2. **Out of process** — decision 009's stated objection was that a JS engine
   *"re-imports the exact attack surface Adobe's broker process contains."*
   A separate process with a narrow effect protocol **is** the broker
   architecture, rather than re-importing the thing it defends against.
3. **The dangerous verbs never move** — submit, navigate, structure changes
   and messages are pdfcer-side effects subject to pdfcer's gates. The plugin
   gets an interpreter, not capabilities.

That is a real change in the facts, not a costume. **It may still be
refused, and that refusal would be legitimate.** The decision belongs to
the operator either way.

### 5.5 The tension no architecture removes

**The narrower the plugin's vocabulary, the safer it is and the less
Acrobat-equivalent it is.** These trade directly and no shape escapes it.

It bites hardest on `DocumentStructure` effects. Acrobat's JS API can
insert and extract pages, stamp watermarks, flatten, add annotations and
spawn pages from templates. Supporting those means the script's vocabulary
becomes approximately *pdfcer's entire command surface* — which is the
opposite of a small, auditable list.

**Recommended resolution: a second tier.** `DocumentStructure` effects are
refused unless the operator grants them, per document, and the grant is
disclosed. This mirrors Acrobat's own trusted-function split (§7) rather
than inventing a pdfcer-specific concept.

### 5.6 Where the engine's rules still bind

`pdfcer-core` and `pdfcer-render` gain **nothing** from this plan — no
network, no process spawning, no plugin loading, no protocol client. The
supervisor, the protocol client and `pdfceNet` live in the shells. The
wasm32 CI gate and the `no-network` gate both stay green untouched, and
that is a hard acceptance criterion, not an aspiration.

---

## 6. The submit posture ladder

Derived from O2/O4/O5. **Four independent controls**, not one setting.

### 6.1 Rung 1 — destination policy (default: **open**, per O4)

| Mode | Behaviour |
|---|---|
| **Open (default)** | Any destination the document names is permitted. |
| **Whitelist-only** (O5) | Only operator-listed hosts permitted. A refusal **states the host it wanted to reach and why it was blocked** — never a dead button. |

**The whitelist is operator-owned and never document-supplied.** No entry
in the PDF may add to it, and no destination may vouch for itself.

★ **Redirect following is the trap to build in from day one.** A whitelisted
host that responds with a redirect elsewhere would carry the data with it.
**The whitelist check runs on every hop, not just the first.** Trivial now;
a security bug later.

### 6.2 Rung 2 — destination disclosure (**approved default: on**, per O5)

Before any byte leaves, the destination is stated. Not a block — a
statement. `pdfcer` prints it; the GUI shows it.

★ **Why this is load-bearing and not paranoia: the button's caption is
written by the document's author too.** A button reading "Save Draft",
"Print" or "Continue" can carry a submit to any host. So `ARCHITECTURE.md`
§1.1 clause 2's *"the operator asked for it at the moment it happened"* is
satisfied only formally — the operator asked for *what the caption said*.
Stating the destination is what makes the consent real. This is the
phishing shape, and it is not hypothetical.

### 6.3 Rung 3 — payload disclosure (option, per O5)

**This is where pdfcer exceeds Acrobat** (§7). Acrobat discloses the
destination and never the payload scope.

Requirements:

- **Not a raw dump.** The payload is either a structured data format
  (FDF/XFDF/HTML/XML) or, under the `SubmitPDF` flag, *the entire file* as
  a binary. A hex view helps nobody.
- **Field values as a plain name→value list.**
- **Whole-file submissions stated honestly** — "the entire document,
  including N attachments and its metadata" — rather than pretending to
  render bytes.
- **★ Hidden fields are the killer case and the feature's real
  justification.** A form can carry fields with the Hidden annotation flag
  set, holding author-supplied values, that submit exactly like visible
  ones. **Payload disclosure is the only way an operator can ever see
  them.** State them separately and by name: *these N values are being sent
  and were never shown to you.*

### 6.3.1 ★★ What the spec ingestion established — this rung is now the most valuable item in the plan

§12.7.5.2 was ingested 2026-08-26 (`iso32000__s__12.7.5.2.md`, both editions,
from staged primaries). It **confirmed the hidden-field case and then found
five worse ones.** Every item below is sourced, not inferred, and each one is
invisible to an operator today:

1. **Hidden fields ARE submitted, and the reason is structural rather than an
   oversight.** The standard is silent — but `Hidden` is an **annotation**
   flag, every submit selector addresses **field** dictionaries, and the only
   field-level withhold flag that exists is `NoExport`. The silence is not a
   gap to be interpreted; the two things are simply on different objects.
2. **`Password` field values are submitted.** The flag's NOTE constrains
   *storage*, not transmission.
3. **The BASELINE payload already ships the source document's local PATH
   (FDF `/F`) and the trailer `/ID`.** Not under an exotic flag — by default.
   `ExclFKey` (bit 12) is the *only* privacy-narrowing bit in the entire flag
   word.
4. **A `FileSelect` text field submits a LOCAL FILE.** A form can name a file
   on the operator's disk and carry it out. (The clause additionally
   contradicts itself on whether contents or path are sent — ambiguity
   `SF-A4`.)
5. **`IncludeAppendSaves` (bit 7) turns a submit into a SAVE.** An
   incremental update `shall` be performed immediately before transmission,
   and the payload is **every byte since the document was opened** —
   re-sending what earlier submits already sent, **signatures included**.
   A submit that writes to the operator's file is not a shape anyone would
   guess from the word "submit".
6. **`SubmitPDF` (bit 9) ignores `/Fields` entirely** — there is no
   partial-PDF submission. Whole file or nothing.

**Consequence for the design: rung 3 is promoted.** §9 Q3 proposed payload
disclosure "off by default, one gesture away". **That proposal is now
withdrawn by the engineer as too weak** — see §9 Q3 as amended. Four of the
six facts above are undetectable by any other means, and three of them
(3, 4, 5) are not about form data at all.

**`NoExport` has explicit precedence** over `/Fields` + `Include/Exclude`,
and must therefore be applied **last** when computing what a submit would
send. Getting that order wrong makes the disclosure itself wrong.

### 6.4 Rung 4 — transport (open question, §9 Q4)

An author may name a plain unencrypted destination, sending filled data in
the clear. Options: allow silently / allow with the disclosure saying so /
refuse in whitelist mode only / refuse always. **Acrobat's behaviour here
is one of the six things the parity research could not establish.**

**★ And the standard has nothing to say either — measured, not assumed.**
The spec ingestion recorded sixteen explicit negatives (`SF-N1`…`SF-N16`).
The load-bearing ones for this plan:

- **No clause restricts `/F` to a network address.** A submit destination
  may be a local path.
- **`https` appears ZERO times in ISO 32000-1.**
- **No consent rule, no privacy rule, no TLS rule, no redirect rule, no
  timeout rule, no size limit, and no failure-handling rule exists in
  either edition.** All measured as 0-hit searches, not inferred from
  absence of memory.

★ **The consequence is worth stating plainly, because it inverts a natural
assumption:** every safety control in §6 — the whitelist, the destination
disclosure, the payload disclosure, the per-hop redirect check — is a
**product decision with a named conformance cost**, never a conformance
requirement. There is no clause to cite in their defence. That does not make
them wrong; it makes them **ours**, and it means they must be justified on
operator-protection grounds and disclosed as deviations, not presented as
"what the standard requires."

### 6.5 What a submit must never do

- **Never fire without an operator gesture.** No submit on open, on close,
  on page change, on field change, or from a document-level script at load.
- **Never follow a submit with an automatic second request.**
- **Never render or execute the server's response.** A submit sends; it
  does not open a returned document. (Acrobat's FDF response machinery can
  update a form from the reply — **explicitly out of scope**, and this is a
  deliberate divergence, recorded as such in §7.)

---

## 7. Acrobat's actual model — parity assessment

Sourced 2026-08-26 into
`D:\Dev\Rag-Specialized\Acrobat_Features\forms__submit_actions_and_network_trust.md`.

**★ Sourcing caveat, carried forward deliberately: every direct fetch
against Adobe's own servers failed this session (timeouts / connection
resets).** Every fact below is search-engine synthesis of Adobe pages,
tagged `ADOBE-SNIPPET` in the RAG file, **never independently read**. Treat
as good working knowledge; **do not treat as verified** before it grounds a
`must_have` acceptance criterion.

### 7.1 The model

Acrobat runs **two independent gates**:

1. **JS-execution privilege.** `submitForm()` is **not** trusted-function
   gated — ordinary document script may call it. (Contrast
   `importDataObject` / `importTextData(path)`, which are.)
2. **Enhanced Security's cross-domain check** — the gate that matters:
   - **Same domain as the document's origin → silent.**
   - **Different domain → disclosed warning naming the URL, with
     allow/block/cancel and an optional per-host "remember".**
   - Bypassed if the destination host publishes a cross-domain policy file,
     **or** if the operator has granted that host "Privileged Location"
     trust.

★ **The sharpest finding: ordinary "I trust this document" trust does NOT
unlock cross-domain access.** Only the separate, **host-scoped** grant does.
Document trust and network trust are different things in Acrobat's model —
a cleaner separation than expected, and one pdfcer should copy.

### 7.2 Where pdfcer would sit

| Aspect | Acrobat | This plan | Verdict |
|---|---|---|---|
| Destination chosen by document author | Yes | Yes (O4) | **Parity** |
| Same-origin submit silent | Yes | **No — always disclosed** | **Exceeds** (pdfcer has no "origin" for a local file; silence would be unjustifiable) |
| Cross-origin warning naming the URL | Yes | Yes (O5/§6.2) | **Parity** |
| Per-host persistent grant | Yes | Yes (whitelist, §6.1) | **Parity** — *corrects an earlier engineer claim that the whitelist would exceed Acrobat; Acrobat already has an operator-owned per-host grant.* |
| Destination may vouch for itself | Yes (policy file) | **No** | **Exceeds** — a hostile host writes a permissive policy |
| Payload scope disclosed | **No** | Yes (§6.3) | **Exceeds** |
| Hidden-field values disclosed | No | Yes (§6.3) | **Exceeds** |
| Redirect re-checked per hop | Unestablished | Yes (§6.1) | **Exceeds or parity** |
| Document trust ≠ network trust | Yes | Yes (adopted) | **Parity, copied deliberately** |
| Response updates the form | Yes | **No** (§6.5) | **Divergence, deliberate** |
| Timers / background scripts | Yes | **No** (§5.3) | **Divergence, deliberate** |

### 7.2a ★★ MEASURED AGAINST THE LOCAL ACROBAT, 2026-08-26 — this supersedes the search-sourced claims above where they disagree

A probe harness was built (four hand-authored AcroForm PDFs, each with one
pre-filled text field and one full-page push button carrying a different
action; a `127.0.0.1`-only listener recording method, headers and body).
**Everything in this subsection was observed**, with screenshot or
wire-capture evidence, and outranks §7.1's `ADOBE-SNIPPET` material.

**Observed, high confidence:**

1. **A declared `/SubmitForm` to a host raises a modal "Security Warning"**
   before anything leaves. Confirmed by screenshot and by reading the
   dialog's text through UI Automation, so the wording below is verbatim,
   not transcribed from pixels:

   > The document is trying to connect to:
   > `http://127.0.0.1`
   >
   > Do you trust 127.0.0.1? If you trust the site, choose Allow. If you do
   > not trust the site, choose Block.

2. **★ THE WARNING NAMES SCHEME + HOST ONLY — NOT THE PORT, NOT THE PATH.**
   The actual destination was `http://127.0.0.1:8765/declared-http`. The
   dialog said `http://127.0.0.1`. **This corrects §7.1's "warning naming
   the URL"** — it names the *host*.
3. **★ And Acrobat plainly HAS the full URL, because its own button tooltip
   shows it.** Hovering the push button displayed *"Send the data to
   http://127.0.0.1:8765/declared-http"*. So the least prominent surface
   (a hover tooltip) is more informative than the security prompt. That is
   a deliberate design choice by Adobe, and it is the one this plan
   consciously departs from (§6.2).
4. **"Remember this action for this site for all PDF documents" is TICKED
   BY DEFAULT.** One Allow writes a permanent host grant — verified by
   observing `tHostPerms` gain `127.0.0.1:2` in the operator's profile, and
   restored afterwards to its prior value.
5. **Three-way Allow / Block / Cancel**, Allow carrying the focus ring.
6. **The warning says NOTHING about the payload** — no field count, no
   whole-file indication, no mention of hidden fields. §7.1's claim that
   Acrobat never discloses payload scope is confirmed by observation.

**Captured on the wire.** With `/Flags 4` (`ExportFormat`), the submission
was `POST`, `Content-Type: application/x-www-form-urlencoded`, body:

    Payload=TOKEN-DECLARED-HTTP&Go=

Two things worth keeping: **the push button itself is submitted, as an
empty-valued field (`Go=`)** — a field §12.7.4.2.2 says never has a value —
and Acrobat presents a **spoofed Safari user-agent** while separately
sending `Acrobat-Version: 25.1.0`. Its `Accept` header advertises
FDF/XFDF/XDP/PDF, i.e. it expects the server may reply with data that
updates the form (§6.5 declines to implement that).

With `/Flags 0` the submission was `Content-Type: application/vnd.fdf`,
**155 bytes**. The raw FDF body was **not** captured before the harness was
retired, so **the spec's claim that the baseline FDF payload carries the
source document's PATH and trailer `/ID` remains SPEC-SOURCED, not
observed.** It is a small payload for a claim that large; that is a reason
to measure it, not a reason to doubt it. Do not upgrade it to "verified"
without the bytes.

**Not established, and why.** HTTP-vs-HTTPS treatment and scripted-vs-
declared treatment were not measured. Acrobat's dialog **ignores synthetic
keystrokes and `BM_CLICK` entirely**; UI Automation can *read* it but its
controls are custom-drawn panes with no Invoke or Toggle pattern, so they
cannot be pressed programmatically. The working technique — **read the true
control rectangle via UI Automation, click it with the mouse** — was found
too late in the session to run the remaining variants. Also learned:
**Acrobat will not open an `http://` URL given on the command line** (it
fetches the bytes, then fails with a filename-syntax error), which killed
the same-origin route.

### 7.3 The unestablished items

Carried from the parity research, unresolved: (1) the factory-default state
of Trust Manager's Internet Access setting; (2) whether `getURL()` sits on
the JS gate, the network gate, or neither; (3) HTTP-vs-HTTPS differential
treatment; (4) the cross-domain policy file's exact scope beyond FDF
submission; (5) whether scripted `submitForm()` gets identical cross-domain
treatment to a declared action; (6) current Trust-Manager preference key
names.

**(1), (3) and (5) should be spot-checked against the Acrobat Reader on
this machine before they ground acceptance criteria.** (3) is directly
§9 Q4.

---

## 8. Phasing

Ordered so that **every phase ships value alone** and each later phase is
independently killable.

**★ RULED 2026-08-26 (O8): Phases 1, 2 and 3 are the delivery plan.**
Phase 4 (`pdfceJS`) and Phase 5 (in-app download) are deferred — planned,
documented, unscheduled. **The test each of the first three phases must pass
is that it works with no JavaScript execution capability present at all**,
because under O8 there will not be one. That is a design constraint on
Phases 1–3, not merely a description of them: nothing in them may be
specified as "…and the rest is handled by a script."

### Phase 1 — Action authoring, safe subset. No plugin, no network, no interpreter. **★ SHIPPED 2026-08-30 (`bc49a8e`, `cff102a`), except `/AP` `/D` and `/MK` layout.**

**What the operator actually said, and it is not what this phase assumed.**
This phase was written expecting `/SubmitForm` to be authored *"as data"*
pending Phase 2. The operator's ruling — *"make the submit and other options
that don't need javascript available for buttons with the safeguards like we
had planned"* — arrived as a single instruction covering the whole list, and
`Pass 183.0` shipped it in one Pass rather than the two this phasing implied.

**The safeguards that survived into authoring**, since §6's ladder is a
dispatch ladder: the full destination is stated (rung 2's *content*, at
authoring time), the **payload** is computed and disclosed (rung 3, §6.3.1 in
full), `http` is allowed and said rather than blocked (rung 4's proposed
default), and every undecidable destination or non-conforming flag word is
**refused by name**. Rung 1's whitelist has nothing to gate: pdfcer sends
nothing.

**One thing this phase did not anticipate and Phase 2 inherits:** authoring a
`/GoTo` on a **widget** made `pageops::references::census_dangling` — which
walked `/Link` annotations only — under-report by construction. Fixed in the
same commit, `DanglingReport::non_link_annotations`. Any later carrier this
plan adds (`/Screen`, a link-annotation Pass) should check the same census
before shipping.


Author `/A` on push buttons (and, for free, the same primitive serves future
link annotations):

| Action | Notes |
|---|---|
| `/ResetForm` | With Table 238 `/Fields` and Table 239's `Include/Exclude` flag. |
| `/GoTo` | In-document destinations; rides the existing reference-rewrite machinery so page ops keep it correct. |
| `/Named` | `NextPage`, `PrevPage`, `FirstPage`, `LastPage`. |
| `/URI` | **Authored** (it is only data), never followed by pdfcer. Disclosed. |
| `/SubmitForm` | **Authored** in Phase 1 (data), **honoured** only in Phase 2. **★ UNBLOCKED 2026-08-26** — §12.7.5.2 is ingested. **Table 236 has exactly FOUR entries: `/S`, `/F`, `/Fields`, `/Flags`** (see the correction in §11.1). The flag word (Table 237 / 2.0 Table 240) is: `Include/Exclude` 1 · `IncludeNoValueFields` 2 · `ExportFormat` 4 · `GetMethod` 8 · `SubmitCoordinates` 16 · `XFDF` 32 · `IncludeAppendSaves` 64 · `IncludeAnnotations` 128 · `SubmitPDF` 256 · `CanonicalFormat` 512 · `ExclNonUserAnnots` 1024 · `ExclFKey` 2048 · **bit 13 is unnamed in both editions** · `EmbedForm` 8192. **Format precedence is specified as a strict `shall` chain: `SubmitPDF` ≻ `XFDF` ≻ `ExportFormat`**; `/Flags 0` means FDF-by-POST, which is a **decision, not an absence** — do not treat a missing `/Flags` as "unspecified format". Nine flags carry *"shall be used only when…"* gates and **none states a reader recovery rule**, so pdfcer must choose and disclose its own behaviour for each violated gate. One genuine internal contradiction to refuse or disclose by name: `SubmitPDF`+`GetMethod` (`/Flags 264`) is contemplated by bit 9 and forbidden by bit 4. |
| `/JavaScript`, `/Launch` | **Refused by name, permanently.** `/Launch` collides with `R13`; `/JavaScript` would author something pdfcer refuses to run. |

Also Phase 1: `/AP` `/D` (the pressed appearance) and `/MK` icon/label
layout, so a button looks like a button when held.

**Rules touched: none.** Authoring is writing bytes, not firing triggers.
Ships to core + CLI + the GUI request channel.

### Phase 2 — Honouring actions in-app. **Needs the `R54` amendment (§3.1).**

A dispatch allow-list: hit-test a widget, resolve its `/A`, and either
perform it (reset → the existing verb; navigate → the viewer) or refuse it
by name. Everything not enumerated is refused.

Submit lands here **only if `pdfceNet` is installed**, behind the full §6
ladder. Requires the `R12` decision record for the new destination class
(§3.4) and the §9 answers.

### Phase 3 — `pdfceNet`, hand-installed only.

Thin shell over `pdfcer-fetch`. **No in-app download** — the operator copies
the folder in. This deliberately avoids engaging `R13` clause 5 at all
(§3.3), which is why it is cheap.

### Phase 4 — `pdfceJS`, hand-installed only. **Needs the `R53` reversal (§3.2).**

Separate process, versioned message protocol, effect vocabulary from §5.3,
`DocumentStructure` behind a second-tier grant (§5.5). Largest phase by
far; buys the least *per unit of work*, because posture B already covers
the common case (§2.5).

### Phase 5 — in-app plugin download. **Needs the `R13` clause 5 ruling.**

Optional and last. Everything works hand-installed without it.

---

## 9. Open questions for the operator

Each is a ruling only the operator can make. **Defaults are proposed for
every one**, per standing practice, so none of these blocks planning.

| # | Question | Proposed default |
|---|---|---|
| ~~**Q1**~~ | ~~**Amend `R54`**…~~ | **★ ANSWERED 2026-08-26 (O6) — YES.** See §1.1 and §3.1. Filing dispatched. |
| ~~**Q2**~~ | ~~**Reverse `R53`**…~~ | **★ ANSWERED 2026-08-26 (O8) — DEFER.** Phases 1–3 are the delivery plan and must stand without it. See §1.1. |
| **Q3** | Whitelist mode and payload disclosure (O5) — which, if either, is **on by default**? | **★ PROPOSAL AMENDED 2026-08-26 after the spec ingestion.** Was: *"payload disclosure off by default, one gesture away."* **Now: a SUMMARY LINE is always shown** — how many values, whether the whole file goes, whether any hidden or password field is included, whether a local file or the document's own path is being sent — **with the itemised list one gesture away.** §6.3.1 is the reason: four of the six payload facts are undetectable by any other means, and three of them are not form data at all. Whitelist stays **off** (matches O4); destination disclosure stays **on** (O5 approved). |
| **Q4** | Unencrypted (`http://`) destinations — allow silently, allow with the disclosure saying so, or refuse? | **Allow, and say so in the disclosure.** Consistent with "disclose, don't block". |
| **Q5** | Whole-file submissions (`SubmitPDF`) — permitted like any other, or a separate confirmation? | **Permitted, but disclosed distinctly** — "the entire document" is categorically different from "these six values". |
| **Q6** | Copyleft JS engine in `pdfceJS`, given the separate-process boundary? | **Engineer will not decide this.** Legal reading, operator's alone (`LEGAL.md` §6). Not needed before Phase 4. |
| **Q7** | Does a submit's *response* ever update the form (Acrobat can)? | **No** (§6.5). Deliberate divergence. |

---

## 10. Rejected alternatives, recorded so they are not re-proposed

| Rejected | Why |
|---|---|
| **In-process `.dll` plugin** | Weakest sandbox exactly where the strongest is needed; forecloses the licence question; imports the Rust ABI problem. §5.1. |
| **Network client inside `pdfceJS`** | Would make the common case (a script-free submit button) depend on installing a scripting engine. §5.2 — and it is the one point where this plan departs from the operator's own stated assumption (O3). |
| **Plugin returns edits only** | No channel for messages/prompts or keystroke rejection, both of which real forms depend on. §5.3. |
| **Timers / background scripts** | No place in a request-response protocol; near-zero real-world use; unbounded cost. §5.3. |
| **Authoring `/JavaScript` or `/Launch` actions** | Authoring something pdfcer refuses to run; `R13` collision. §8 Phase 1. |
| **Wasm plugin runtime now** | Large dependency for a capability nobody has asked for yet. Revisit only for the web fork — where the protocol boundary (§5.1) already covers it. |
| **Treating the plugin folder as a workaround for `R53`** | It is not one. §3.2. The argument must be made openly or not at all. |

---

## 11. Prerequisites

1. **§12.7.5.2 Submit-Form Action spec ingestion — ★ DONE 2026-08-26.**
   `iso32000__s__12.7.5.2.md`, both editions, read from staged primaries.
   Phase 1's `/SubmitForm` authoring and §6.3 are **unblocked**. Findings
   are folded into §6.3.1, §6.4 and §8 Phase 1.

   **★★ 11.1 — TWO CORRECTIONS, AND THE SECOND ONE IS THE INSTRUCTIVE PART.**

   The dispatch that commissioned this work asserted that Table 236
   contained `/F`, `/Fields`, `/Flags`, `/CharSet` **and the PDF-1.7
   `/URL`/`/URLType` machinery**. Both extras are wrong:

   - **`/URL` and `/URLType` are NOT Table 236 entries.** They are the last
     two rows of **Table 235, the certificate seed value dictionary**
     (§12.7.4.5; 2.0 Table 238). They print immediately before §12.7.5 in
     both editions — a **page-break interleave**. The tell is inside the row
     itself: it cites *"the `Ff` attribute's `URL` bit"*, and **Table 236 has
     no `Ff`**.
   - **`/CharSet` is PDF 2.0 only** (2.0 Table 239; an ISO-ratified erratum
     deletes its `inheritable`). In ISO 32000-1, `CharSet` is the Type 1
     font-descriptor key — a different thing entirely.

   **Table 236 has exactly four entries: `/S`, `/F`, `/Fields`, `/Flags`.**

   ★ **Why this is recorded rather than quietly fixed: the false claim came
   from the corpus's own exclusion notice**, and the engineer's dispatch
   repeated it verbatim in good faith, which would have propagated it into
   acceptance criteria. **A gap notice describing what it does not contain is
   an unverified claim like any other** — it was written by someone who had,
   by definition, not ingested the clause. The error survived because a
   plausible-looking key list is exactly the kind of thing a reader nods at.
   Two keys were invented by a page break and by a name collision, and only
   reading the primary caught it.
2. **Acrobat spot-check — ★ PARTLY DONE 2026-08-26.** The cross-domain
   warning, its exact wording, its host-only scope, the default-ticked
   remember box and the HTML-format payload are now **observed** (§7.2a).
   **Still owed: HTTP-vs-HTTPS treatment, scripted-vs-declared treatment,
   and the raw FDF body.** The technique that works is recorded in §7.2a and
   in the engineer's memory; the harness lived in
   `%TEMP%\pdfcer-submit-probe\` and is disposable — rebuild it rather than
   assuming it survives.

   ★ **Two operator-environment courtesies, both learned the hard way:**
   Acrobat's sign-in modal and its crash-recovery modal each swallow clicks
   aimed at the page, and **a swallowed click is indistinguishable from a
   refusal** — the first probe run read as "Acrobat silently blocked the
   submit" when the click had simply never arrived. And an `Allow` with the
   remember box ticked **writes a permanent host grant into the operator's
   profile**; this session's was removed and the prior value
   (`version:2|ikea.com:2`) restored and verified. Any future run owes the
   same restoration.

3. **★ FIX §2.7 FIRST — pdfcer's network-hazard counter is blind to `/A`.**
   A shipped defect, not part of this plan's new work, and a prerequisite to
   Phase 1 for the reason given there.
4. **Decision records owed** before any code: the `R54` amendment (Q1), the
   `R12` new-destination-class record (§3.4), and — only if Phase 4 is
   taken — the `R53` reversal (Q2).
5. **`pdfcer-ui-specialist` dispatch** before any GUI surface for the §6
   ladder. The disclosure is the feature; getting its placement wrong is
   how rule 4 got narrowed twice already.
6. **`pdfcer-librarian` filing — ★ DONE.** O1–O8 filed as dated operator
   rulings, `R54` amended (decision 088), `R56`'s stale rule-4 citation
   corrected, `Pass 131.0`–`131.4` minted into *Backlog*, `FEATURES.md`
   rows added, 268th and 269th `SESSION_LOG.md` filings appended.

---

## 12. The one thing to carry out of this document

**Making a push button work is not a JavaScript problem.** The feature
everyone reaches for — a Reset button that resets — is blocked by a
one-line standing rule about trigger dispatch, needs no plugin, no network
and no interpreter, and binds a button press to a verb pdfcer already
implements, tests and can undo.

Everything else in this plan is optional, later, and independently
killable.
