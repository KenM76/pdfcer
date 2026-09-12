# 020 — Form field AUTHORING: the field-identity model, XFA scope, slice order, tab order, tagging, and the four unresolved research conflicts

**Status:** DECIDED (KenAgent decision consultant, 2026-08-03)
**Requested by:** `pdfce-engineer`, scoping operator priority item #4
("work on form building tools after if that makes sense")
**Roadmap bucket:** Forms (AcroForm) — the AUTHORING half
**Proposed Pass family:** **Pass 20.x** (verified free in `ROADMAP.md`;
`pdfce-librarian` finalizes numbering)
**Decision record number:** 020 (next sequential; 019 is the highest on disk)
**Supersedes:** nothing. **Amends:** nothing.
> ### ★ FORWARD POINTER 2026-09-12 — `PeriodInPartialName` no longer exists
>
> This record names `FormAuthorError::PeriodInPartialName` in several places as
> the refusal for a **partial name containing a period**. That variant was
> renamed to **`EmptyNameSegment`** in `Pass 299.0`, because its name and doc
> comment stated a rule it never enforced: its only raiser fires on an EMPTY
> segment (`a..b`, `.x`), and `"Text.2"` could never reach it. The
> period-in-a-partial-name rule is **`DottedPartialName`**.
>
> ★★ This record is left unedited below, because it is a dated account of what
> was decided on 2026-08-03 and the confusion is part of the account — the
> history shows the same misreading happening once before, in `Pass 20.6`,
> where reusing `PeriodInPartialName` for `A.B` produced *"contains an empty
> name segment"*, **which `A.B` does not have**. That was fixed by adding
> `DottedPartialName` and leaving the misleading name in place; the rename is
> the fix that did not happen then.

**AMENDED IN PLACE 2026-08-07** by an engineer ruling — **§0 below
supersedes every `forms <verb>` CLI shape this document specifies.**
Read §0 before acting on any CLI line in §6 or §11.
**AMENDED AGAIN 2026-08-07 (second ruling, same day) — see §0.1:** the
radio verb is **RULED `add-radio-button`**, and §6's deletion verbs
`remove-field` / `remove-widget` are **SUPERSEDED by `delete-field` /
`delete-widget`**. §0 ruled the CLI *shape*; §0.1 rules the *words*.
**Depends on:** decision 009 (JS posture), decision 019 §3.7 (the FF-I cut),
`ARCHITECTURE.md` §5 (round-trip), §12.7.3 spec RAG.

---

## §0 — AMENDMENT, 2026-08-07: the CLI verb shape is FLAT, not nested

**Ruled by `pdfce-engineer`, 2026-08-07. Filed by `pdfce-librarian` the
same day. This amendment is placed FIRST, ahead of the TL;DR, because
the shape it corrects appears eleven times below and a reader who meets
any of those before meeting this one will implement the wrong surface.**

### What changed

This document specified field creation as a **nested** CLI surface —
`pdfce-cli forms add-field --type text|checkbox|choice …`, with sibling
`forms remove-field`, `forms remove-widget`, `forms set-tab-order`,
`forms rename-field`. **[§0.1, 2026-08-07: the two deletion siblings lose
their VERB as well as their nesting — they are now `delete-field` /
`delete-widget`. This paragraph describes what the document originally
specified and is left as the historical record.]**

What shipped in Pass 20.1 / 20.2 / 20.3 / 20.0 is **flat**:
`add-text-field`, `add-check-box`, `add-choice-field`.

**The FLAT shape stands. Every nested `forms <verb>` form specified in
this document is SUPERSEDED.**

### Why — and this is not "the shipped thing wins because it shipped"

The reasoning is a measurement of the surface the new verbs had to join,
taken from `crates/pdfce-cli/src/main.rs`'s `Command` enum (lines
381–2414) rather than recalled:

- **52 subcommands. Every one of them is flat.**
- **Zero nesting** — `#[command(subcommand)]` occurs **0 times** in the
  `Command` enum. There is no nested verb anywhere in `pdfce-cli`.
- Two naming conventions already coexist and are both flat: **verb-first**
  (`list-fields`, `fill-field`, `extract-text`, `render-page`,
  `regenerate-appearances`, `export-data`, `import-data`) and
  **noun-prefixed** (`dimension-add`, `dimension-offset`, `dimension-delete`,
  `group-add`, `group-set-scale`, `layer-toggle`, `object-move`,
  `subpath-delete`, `node-move`). Even the noun-prefixed family is a flat
  hyphenated verb, not a parent command with children.
- `forms add-field` would therefore be **the only nested verb in the
  entire CLI**.

**The load-bearing point: this document specified the nested form before
the surface it had to join was examined.** Consistency with 52 shipped
flat verbs beats consistency with a prediction made in the abstract. Had
`pdfce-cli` been nested elsewhere — had `dimension` or `object` been
parent commands with children — the ruling would have gone the other way,
and `forms add-field` would have been right. It is the measurement that
decides it, not the fact of having shipped.

### This is REVERSIBLE and cheap to reverse — it is Ken's to overturn

Recorded explicitly so nobody treats it as settled beyond appeal: there
is **no release, no git remote, and no users** (project rule 8 —
publishing still awaits Ken's explicit go-ahead). Renaming three clap
variants and their tests is a small, mechanical change. If Ken prefers
the nested form, the cost of reversing is hours, not a migration.

### What this amendment does NOT do

- It does **not** rename any shipped verb. `add-text-field`,
  `add-check-box`, `add-choice-field` keep their names.
- It does **not** mint a standing rule or a decision record. It is an
  amendment to this record, not a new one.
- It does **not** rule on any verb name that §6 does not itself specify.
  See the mapping table below for exactly where the ruling stops.

### Mapping — how each §6 CLI line reads after this amendment

Read strictly. The left column is what §6 says; the right column is what
it now means. Where the right column says **NOT RULED**, the engineer has
not picked a name and one must not be invented.

| §6 slice | Nested form as specified (SUPERSEDED) | Flat form that now applies |
|---|---|---|
| F1 | `forms add-field --type text …` | `add-text-field …` — **SHIPPED** |
| F2 (checkbox) | `forms add-field --type checkbox …` | `add-check-box …` — **SHIPPED** |
| F2 (radio) | repeated `add-field --type radio --name G --export-value V` | ~~**NOT RULED.**~~ **[§0.1, 2026-08-07: NOW RULED — `add-radio-button`.]** One call per radio MEMBER, per the radio caveat below. |
| F2 (deletion) | `forms remove-field --name <fqn>`, `forms remove-widget --name <fqn> --index N` | ~~`remove-field`, `remove-widget`. **Note the verb is REMOVE, not DELETE** — §6 says `remove-*` and this amendment does not change it. `delete-field` is not a name this document or this ruling authorises, notwithstanding that the CLI elsewhere uses `object-delete` / `node-delete` / `dimension-delete`; reconciling `remove` vs `delete` across the surface is a separate, unruled question.~~ **[SUPERSEDED by §0.1, 2026-08-07 — the separate question is now RULED the other way: `delete-field --name <fqn>`, `delete-widget --name <fqn> --index N`. `delete` is the house word; forms is a verb-first domain.]** |
| F3 (listbox/combobox) | `--type listbox\|combobox …` | `add-choice-field …` — **SHIPPED** (both list and combo behaviours live behind the one verb, via `--combo`-family flags) |
| F3 (push button) | `--type pushbutton [--caption <s>]` | ~~**NOT RULED** by name. The flat pattern implies `add-push-button`, but that is an inference from `add-check-box`, not an engineer ruling.~~ **[ARCHITECTURE.md §12, 2026-08-08 (thirty-second filing) — NOW RULED: `add-push-button`, chosen (spec's own two-word term, `add-button` is ambiguous with check box/radio, hyphenated like its siblings), not inferred. `ROADMAP.md`'s `Pass 20.3` COMPLETION ADDENDUM is canonical where this document and it now differ.]** |
| F4 | `forms set-tab-order --mode …` / `--order …` | `set-tab-order …`. Reads cleanly flat; still BLOCKED on the `pdfce-spec-librarian` dispatch (§3.4.4) regardless of naming. |
| F6 | `forms rename-field` | `rename-field`. Reads cleanly flat. |
| F7 | `pdfce-cli merge --on-field-collision …` | **Unaffected** — already flat, `merge` is a shipped top-level verb. |

### The radio caveat — flagged, not smoothed over

§6's F2 does **not** specify a group-creating verb. It specifies radio
grouping *"through the F1 merge primitive, not a radio-specific path —
repeated `add-field --type radio --name G --export-value V`."* One call
per **member**; the group is what the merge produces.

That matters for naming, because the obvious-sounding flat name
**`add-radio-group` would misdescribe the operation** — it names a group
where §6 authors a member. A name faithful to §6 is member-shaped
(`add-radio-button`, or `add-radio-field` by analogy with
`add-choice-field`). ~~**No name is ruled here.** Whoever scopes F2 picks
one, and should pick it against §6's per-member merge model rather than
against the word "group".~~
**[§0.1, 2026-08-07: a name IS now ruled — `add-radio-button`. The
reasoning above is unchanged and is precisely the reasoning that
selected it: member-shaped, not group-shaped. See §0.1 Ruling 1.]**

### Bare `add-field` mentions elsewhere in this document

Beyond the §6 CLI lines marked inline below, this document says
`add-field` in narrative and acceptance prose — §4.1, §5.1, §6-F1's
acceptance list, §6-F3's push-button paragraph, §8.2's `--comb` limit.
Those are **generic references to "the field-creation verb"**, not
surface specifications, and they are left as written rather than edited
eleven times into unreadability. **Blanket rule: every bare `add-field`
in this document reads as "the flat creation verb for the type under
discussion"** — `add-text-field`, `add-check-box`, `add-choice-field`,
`add-radio-button` (**named by §0.1, 2026-08-07**), or the still-unnamed
push-button verb. Flags spelled on those lines
(`--comb`, `--export-value`, `--option`) are unaffected by this
amendment; only the verb changes.

### This CLOSES §11's standing open question

§11's bullet — *"Whether `pdfce-cli`'s existing forms subcommands should
move under a `forms` parent (`list-fields` → `forms list`)"* — is
**CLOSED: no.** The same measurement settles both halves. The six shipped
forms subcommands stay flat and top-level, and no `forms` parent is
created for the authoring verbs either, so there is nothing left for them
to migrate toward. `ROADMAP.md`'s open-operator-question **(q)**, which
was the same question filed as a pointer, is closed with it.

---

## §0.1 — SECOND AMENDMENT, 2026-08-07: the radio verb is RULED, deletion is `delete-*`, and the CLI's word order is DOMAIN-PARTITIONED

**Ruled by `pdfce-engineer`, 2026-08-07. Filed by `pdfce-librarian` the
same day, into this same §0 rather than into a parallel record, because
these are exactly the two names §0 above deliberately left open.** §0
ruled the CLI **shape** (flat, not nested). §0.1 rules the **words**.

### Ruling 1 — the radio verb is `add-radio-button`

**RULED.** This replaces the `NOT RULED` marker that stood in four
locations across the project's documents.

The grounds matter more than the name, because the name is the small
part:

- **It matches its three shipped siblings' `add-<thing>` shape** —
  `add-text-field`, `add-check-box`, `add-choice-field`.
- **`button`, not `group`, is faithful to §6's design.** §6 authors one
  radio **MEMBER** per call through the F1 merge primitive; the group is
  what the merge produces. `add-radio-group` names a group where §6
  authors a member. The radio caveat above is unchanged and is precisely
  the reasoning that selected this name.

**It is ratified on the MERITS, not because an in-flight engineering
fork had already typed it.** That distinction is the reusable part of
this entry, and it is recorded deliberately.

A prior librarian, filing §0, found `add_radio_button` live in
`crates/` and **refused to bless the name from source** — leaving
`NOT RULED` standing rather than ratifying whatever happened to be in
the working tree. **That refusal was correct, and it is the reason this
is now a decision rather than a fait accompli.** Adopting a name because
a fork typed it is exactly the accretion §0's ruling exists to stop:
one more verb per slice, each one settling the convention by arriving
rather than by being chosen. The name is adopted here because it
independently satisfies the two tests above; had it failed them, the
fork would have been renamed to match the ruling, not the ruling bent to
match the fork.

### Ruling 2 — deletion is `delete-field` / `delete-widget`

**§6's `remove-field` / `remove-widget` are SUPERSEDED.** The house word
for deletion is **`delete`**, and the deletion verbs are **verb-first**:
`delete-field`, `delete-widget` — **not** `field-delete` / `widget-delete`.

**Measured, not recalled.** From `pdfce-cli --help`: **five shipped
verbs use *delete*** — `delete-pages`, `dimension-delete`,
`object-delete`, `subpath-delete`, `node-delete` — and ***remove*
appears only in prose descriptions, never as a verb name.**
Independently confirmed by this filing against
`crates/pdfce-cli/src/main.rs`'s `Command` enum: **51 variants, five
carrying `Delete`, and ZERO named `Remove*`.**

This reverses the F2-deletion row of §0's mapping table above, which
recorded `remove` vs `delete` as "a separate, unruled question." It is
now ruled, and the row is struck accordingly.

### The finding worth keeping: the CLI is NOT inconsistent — it is DOMAIN-PARTITIONED

This is the part that outlives both verb names, and the reason it is
written as its own section rather than buried inside one verb's
rationale: **it answers the next naming question without another
ruling.**

`pdfce-cli` *looks* inconsistent about word order — `object-delete` but
`delete-pages` — and is not. **It is partitioned by domain, and each
domain is internally consistent:**

| Domain | Word order | Verbs |
|---|---|---|
| vector / dimension | **noun-first** | `object-list`, `object-delete`, `node-move`, `node-delete`, `subpath-delete`, `dimension-add`, `group-add`, `layer-toggle` |
| page | **verb-first** | `extract-pages`, `insert-pages`, `delete-pages`, `reorder-pages` |
| forms | **verb-first** | `list-fields`, `fill-field`, `add-text-field`, `export-data`, `import-data` |

**Forms is a verb-first domain.** That single fact is what makes
`delete-field` right and `field-delete` wrong, and it is what made
`add-radio-button` right without needing an argument of its own.

**The general rule, stated so it is greppable:** *a new `pdfce-cli` verb
takes its word order from the DOMAIN it joins, not from the CLI as a
whole.* An apparent inconsistency across domains is not a defect to be
normalised away; normalising it would break the consistency that
actually exists.

~~**No rule number is minted for this.** Whether the domain-partition
finding deserves a standing rule of its own is the operator's call, and
is flagged rather than pre-empted. Ceilings stay **R160 / decisions 031 /
Pass family 43**.~~

> **✅ SUPERSEDED 2026-08-07 (later the same day) — THE NUMBER IS MINTED.
> This finding is now STANDING RULE R161.** The struck sentence above is
> preserved, not deleted; it was accurate when filed and the flag it
> raised is what produced the ruling.
>
> **`ROADMAP.md`'s *Standing rules* section is CANONICAL for R161**, per
> the *Update protocol*'s same-filing propagation duty — this decision
> record is append-only and this block is a pointer to the live text, not
> a second copy of it.
>
> **R161's binding statement** (the wording ruled, unchanged from the
> section above): *"A new verb takes its word order from the domain it
> joins, not from the CLI as a whole."* Carried with it: the three-domain
> partition table above; **`delete` is the house word and `remove` is
> not** (five `Delete` variants, zero `Remove*`); **zero nesting**
> (`#[command(subcommand)]` occurs zero times inside the `Command`
> enum); and the corollary that **the CLI's apparent inconsistency is not
> a defect and must not be "corrected."**
>
> **Why it earned a number when Rulings 1 and 2 above did not:** *there
> is a live failure mode it guards against.* A finding that only
> describes the CLI can live in a decision-record amendment; one that
> must **stop** a plausible, well-intentioned future action needs a
> number, **because the person about to take that action will not be
> reading decision 020.** Two supporting arguments also held — it is a
> standing constraint on all future work, and it would be rediscovered or
> contradicted if it lived only here.
>
> **Ceilings are now R161 / decisions 031 / Pass family 43** (**R162**
> next free). Rulings 1 and 2 above still mint nothing.
>
> **One measurement correction, recorded rather than silently fixed
> (R87).** This section's Ruling 2 states *"51 variants, five carrying
> `Delete`, and ZERO named `Remove*`."* **The variant count is a
> miscount.** Re-measured by parsing the `Command` enum out of `git show
> <rev>:crates/pdfce-cli/src/main.rs` across eleven commits: the sequence
> is **50** (`8e799e9`, `e137277`) → **52** (`bca60c9` through
> `69ab966`, the entire window in which §0 and §0.1 were both filed) →
> **53** (`834d256`, HEAD at R161's filing, which added
> `AddRadioButton`). **51 appears at no commit.** ***The load-bearing
> half of the sentence is CORRECT and was corroborated independently:
> five `Delete`, zero `Remove*` — at all eleven commits.*** §0's own
> "52 subcommands" figure is right; only §0.1's 51 is wrong, and neither
> ruling depended on it.

### What §0.1 does NOT rule

- ~~**F3's push-button verb name remains `NOT RULED`, deliberately.** The
  flat pattern implies `add-push-button` and Ruling 1's sibling-shape
  argument would very likely reach it — but F3 was not the question in
  front of the engineer, and a name arrived at by inference is the exact
  thing §0 and §0.1 both refuse. Its `NOT RULED` markers stand.~~
  **[RULED 2026-08-08, ARCHITECTURE.md §12 (thirty-second filing) —
  `add-push-button`, chosen on its own merits (the spec's own term,
  `add-button` would collide with check box/radio), not inferred from
  this entry. See `ROADMAP.md`'s `Pass 20.3` COMPLETION ADDENDUM, which
  is canonical where it and this document now differ.]**
- ~~**Nothing is minted** — no Pass ID, no standing rule, no new decision
  record. This is an amendment to decision 020, exactly as §0 is.~~
  **[AMENDED 2026-08-07, later the same day — the two RULINGS still mint
  nothing, but the DOMAIN-PARTITION FINDING was minted as **standing rule
  R161**. Ceilings are now **R161 / decisions 031 / Pass family 43**
  (R162 next free). See the superseded-block above and `ROADMAP.md`
  *Standing rules*, which is canonical. ~~F3's push-button verb name is
  still NOT RULED and must not be inferred from R161.~~ **[RULED
  2026-08-08 — `add-push-button`; see the *What §0.1 does NOT rule*
  bullet above for the pointer.]**
- **No Pass is shipped or implied.** An engineering fork was live in
  `crates/` building F2 when this was filed; its outcome is neither
  anticipated nor recorded here, and this filing carries no commit.
- **Reversible on the same terms as §0** — no release, no git remote, no
  users (project rule 8). Renaming is hours, not a migration, and both
  rulings are Ken's to overturn.

---

## TL;DR — the six answers

| # | Question | Answer | Confidence |
|---|---|---|---|
| Q1 | `/Kids` object graph vs. flat name-keyed list | **Agree with the research's substance, reject its implied remedy.** The shipped flat `Vec<Field>` is a correct *read projection* and stays. What must be a graph is the **write path**: one `resolve_field_path(graph, fqn)` choke point over the raw object graph, through which every authoring write passes. Shipping a flat *write* model does not cost "a refactor later" — it emits documents pdfce's own shipped reader cannot represent. | **high** |
| Q2 | XFA scope | **Dynamic XFA: `out_of_scope`, agreed.** **Static-XFA hybrid: REFUSE field creation by name** — decided here, not deferred, and decided from pdfce's own capability boundary rather than from the unresolvable Acrobat GAP. Nothing in Q2 needs to go to Ken. | **high** |
| Q3 | Slice order / P0 floor | **F0 correctness-first (no operator surface, rule-11 exempt by the 19.0 precedent), then F1 = text field + the full collision branch.** A lone text field never exercises the merge branch, so the merge branch *is* the P0 floor, not a follow-on. GUI last (R83). Signature + barcode fields explicitly cut. | **high** |
| Q4 | Tab order | **Agree with the recommendation, correct its mechanism.** `/Tabs` is a **mode, not a snapshot** — under `S`/`R`/`C` pdfce reorders **nothing**, because nothing is stored to reorder. Only the explicit/manual case needs a rule: append to end, disclosed. Plus a case the research missed: under `/Tabs S`, an untagged new field has **no defined position at all**. | **high** |
| Q5 | Tagged documents | **Split it.** The sourced accessibility win for *forms* is **`/TU`, not the tag tree** — `/TU` ships P0, mandatory-or-explicitly-declined. Writing a `/StructElem` stays with **FF-I**, cut rationale unchanged. This does not re-open FF-I; it removes a reason to. | **high** |
| Q6 | Four unresolved items | **None blocks. None escalates.** Combine-Files: the two accounts are one behavior with a documented fallback, not a contradiction — resolve by making it an operator choice. Encrypted: structurally inapplicable to pdfce *and* the bit-6 gate would be **dead code today** (R96 hit, verified). Radio deletion GAPs: reframed out of existence — they were never acceptance criteria. | **high** |

**The single sharpest finding in this document** is not any of the six.
It is §1.2.6: **decision 009's structural guarantee — "fill touches only
`/V` // `/AP` // `/AS`, never the `/AcroForm` dict, so every JS carrier
re-emits byte-verbatim" — cannot survive field creation.** You cannot add
a top-level field without writing `/AcroForm/Fields`. A guarantee that
has held for two shipped Passes stops holding the moment this family
starts, and nothing in the codebase will notice. See §7.2.

---

## 1. Context

### 1.1 The request, and the hedge inside it

Operator priority item #4, verbatim: *"work on form building tools after
if that makes sense."* `ROADMAP.md` records the hedge as the operator's
own and instructs re-evaluation rather than treating item #4 as an
unconditional commitment. This decision scopes the family so that
*whether to start it* is a decision Ken can make against a real plan
instead of a vague bucket — it does **not** assert that starting it now
is correct. See §10.1, which is the one item in this document that
genuinely must go to Ken.

pdfce has shipped form **filling** (Pass 7.0/7.1): the `/AcroForm`
parser, field↔widget merge, per-type value decode, text/checkbox/radio
fill, choice fill, appearance regeneration, flatten, FDF/XFDF. It has
shipped **no authoring at all**: there is no way to place a field on a
page that had none.

### 1.2 What the code actually has today — audited this session, not assumed

A full code survey was run over the worktree
(`D:\Dev\pdfce\.claude\worktrees\agent-a90dfe853c88ca161\crates\`). Every
claim in this subsection is read off the source, not recalled. This
matters because three of the six answers below turn on facts that
contradict what the parity research assumed.

#### 1.2.1 The shipped model is FLAT — but it already preserves the half that matters

```rust
pub struct AcroForm {
    pub fields: Vec<Field>,      // TERMINAL fields only, field-tree DFS order
    ...
}

pub struct Field {
    pub id: ObjId,
    pub fully_qualified_name: String,
    pub partial_name: Option<Vec<u8>>,
    ...
    pub widgets: Vec<Widget>,    // ← the one-to-many that matters
    pub merged: bool,
    pub shares_parent_name: bool,
}
```

There is **no `kids` field, no `parent` field**, and no `HashMap`/
`IndexMap` anywhere in the form path — lookup is `fields.iter().find(...)`,
a linear scan, in at least six places. The `/Kids` graph is walked once by
`walk_field` at parse time and then **discarded**; non-terminal (grouping)
field dictionaries exist only inside the recursion and never reach the
output.

**But `Field.widgets: Vec<Widget>` survives.** That is the field→widget
one-to-many, and it is exactly the relationship a radio group and a
repeated page-number field are made of. Every write path already fans out
over it: `regen_field_appearance` allocates one fresh `/AP` stream **per
widget**; `set_button_state` validates the requested state against the
union of `on_states` across all widgets and sets each widget's `/AS`
individually; `flatten_fields` burns every widget onto whatever page it
lives on and queues each for its own `/Annots` removal.

This single fact reframes Q1 entirely. The research's premise — "a flat
list of widgets-with-string-names" — is **not** what pdfce shipped. pdfce
shipped a flat list of *fields*, each holding its widgets. The merge
branch ("same name + same type → the new widget becomes a kid of the
existing field") is therefore **already representable in the shipped
read model**. What is not representable is everything *above* the
terminal field.

#### 1.2.2 What the flat projection has already lost

Enumerated from the survey, each with its authoring consequence:

| Lost | Authoring consequence |
|---|---|
| Non-terminal grouping fields never surface | Cannot author a dotted `parent.child` name — §12.7.3.2's own identity model is unauthorable |
| No `parent` link on `Field` | Cannot write `/V` to the ancestor that declared it; cannot rename a subtree |
| A `/Kids` node mixing field-kids and widget-kids drops the widget-kids **silently** | Authoring can *produce* this shape by accident; the reader will then lose part of what was just written |
| `remove_fields_from_form` never prunes a parent left with empty `/Kids` | Last-member deletion (Q6c) leaves an orphan |
| `parse_acroform` collects `/Fields` roots via `filter_map(Object::as_reference)` — **direct inline dicts are silently dropped** | An authoring path that ever writes an inline field dict writes a field pdfce itself cannot see |

That last row is a self-inflicted-invisibility trap and it is cheap to
avoid: §12.7.3.1 requires a field dictionary to *be* an indirect object
anyway ("Each field … shall be defined by a **field dictionary, which
shall be an indirect object**"). It becomes an acceptance criterion in
§6.F1 rather than a hazard.

#### 1.2.3 `kid_is_field` is a heuristic, and it is a hard constraint on what pdfce may write

```rust
fn kid_is_field<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> bool {
    let Some(d) = graph.resolved(id).as_dict() else { return false; };
    d.contains_key(b"T") || d.contains_key(b"FT") || d.contains_key(b"Kids")
}
```

If **any** kid satisfies this, the walk treats the node as non-terminal,
recurses into the field-kids, and **pushes nothing for the node itself**.

**Consequence, binding on every authoring write:** a widget kid pdfce
creates must carry **no `/T`, no `/FT`, no `/Kids`**. Put `/T` on a radio
group member and pdfce's own reader promotes it to a separate terminal
field, and the group semantics — mutual exclusivity, shared `/V` — are
gone. The document would still be legal PDF; it just would not be the
thing the operator asked for, and pdfce would be the one that broke it.
This becomes **R101**.

#### 1.2.4 `/Encrypt` documents are refused outright by every authoring path

The survey found this stated plainly as a hard constraint every new-object
path inherits, alongside `suppressed_object_count() > 0 →
ObjectCreationWouldExposeHiddenObjects`.

**This is an R96 hit, found prospectively rather than after shipping.**
The parity research names a `must_have`: field creation must consult the
`/P` bit-6 ("create/modify form field definitions") permission. Implement
that today and it is **dead code that looks live** — the coarser
`/Encrypt` refusal fires first, unconditionally, and no encrypted document
can ever reach a bit-6 test. R96's own words: *"a guard clause placed
after a filter the guarded case cannot pass is dead code that looks
live."* See §3.6.2 for what to do instead.

#### 1.2.5 `fill_text_field` carries an inlined duplicate of the shared regen loop

`set_choice_value`, `import_form_data` and `regenerate_appearances` all
call the shared `regen_field_appearance`. `fill_text_field` does **not** —
it inlines its own copy of the same per-widget loop.

This is R92's exact shape ("a predicate that hand-duplicates the shape of a
data structure it inspects drifts silently the moment the structure gains
a field or case"), and it is not hypothetical here: authoring will add
per-field properties (`/MK` border/background, comb layout driven from
`/MaxLen`, auto-size behavior at creation) that the shared helper will
learn and the inlined copy will not. It goes into **F0** as a
consolidation, before any authoring depends on either copy.

#### 1.2.6 The decision-009 byte-verbatim guarantee does not extend to authoring

`ROADMAP.md`'s Pass 7.0 entry states the guarantee explicitly:

> *"Decision 009's posture A … is honored: fill touches only `/V` //
> `/AP` // `/AS`, never the `/AcroForm` dict, so every JS carrier (`/CO`,
> `/AA`, `/Names /JavaScript`) re-emits verbatim."*

That guarantee is **structural** — it holds because of *which objects fill
writes*, not because of a test. Field creation must append to
`/AcroForm/Fields`. There is no way around it: a new top-level field is a
new entry in that array, and `/DR` may need a font added for the new
field's `/DA`. Both live in the `/AcroForm` dictionary.

So a property that has held, by construction, across two shipped Passes
stops holding the moment this family starts — and because it held *by
construction* rather than by assertion, **there is no test that will go
red.** This is precisely R93's failure shape (a claim that was true, is
silently false, and every later Pass relies on it without re-checking),
caught before rather than after. §7.2 turns it into a required test.

#### 1.2.7 There is no structure-tree model at all

`/StructTreeRoot` appears in `pdfce-core` only as a boolean detection or a
disclosure string — in `redact.rs`, `pageops/assemble.rs` (documented as
deliberately not carried), `text_edit/addtext.rs` (R73 tagged-page
detection), and a CLI counter. **No `/StructElem`, no `/ParentTree`, no
`/K` traversal exists.** A newly created field has nothing to hook into.
This is decisive for Q5.

#### 1.2.8 The canonical new-object recipe already exists

`EditSession::add_text_annotation` (and its siblings `add_markup`,
`add_redaction`, `add_text`, `add_dimension`) establishes the pattern:
guards → build → `alloc_number()` → `stage_bytes` → `ObjectWrite { before:
None, after: Some(..) }` (which is what makes undo-of-a-creation
expressible) → `annots_append(page_id, …)` → one `commit`. On the
AcroForm side, `remove_fields_from_form` already demonstrates patching
`/Fields` in both its indirect-array and direct-array shapes, and
`acroform_id()` resolves whether `/AcroForm` itself is indirect or inline.

**Field creation is the mirror image of code that already exists and is
R46-proven.** It does not need a new authoring architecture. That is a
large de-risking fact and it is why the slice plan below is as short as
it is.

### 1.3 The spec facts that constrain every option

From `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__12.7.3.md`
(verbatim quotations of ISO 32000-1 as recorded there):

- **§12.7.3.1** — *"Each field … shall be defined by a **field
  dictionary, which shall be an indirect object**. The field dictionaries
  may be organized hierarchically into one or more **tree structures**.
  Many field attributes are **inheritable** …"*
- **Table 220 / `Parent`** — *"A field can have **at most one parent**; it
  can be included in the `Kids` array of at most one other field."*
- **Table 220 / `Kids` — the merge switch** — *"In a **non-terminal
  field**, `Kids` shall refer to **field dictionaries**. In a **terminal
  field**, `Kids` ordinarily shall refer to **one or more separate widget
  annotations**. **However, if there is only one associated widget
  annotation, and its contents have been merged into the field dictionary,
  `Kids` shall be omitted.**"*
- **§12.7.3.2 — the FQN is not stored** — *"The fully qualified field name
  is not explicitly defined but shall be **constructed** from the partial
  field names of the field and all of its ancestors … separated by a
  PERIOD (2Eh)."*
- **Table 220 / `FT`** — a non-terminal field *"does not logically have a
  type of its own; it is merely a container for inheritable attributes."*
- **§12.5.1 / Table 30 `/Tabs`** (via `iso32000__s__12.5.2.md`) — page
  entry, PDF 1.5, values `R` (row), `C` (column), `S` (structure order
  §14.7); *"This is interaction-order only."*

Three consequences fall straight out and are worth stating because each
one kills an otherwise-tempting design:

1. **The FQN is derived, not stored.** There is no key you can set to
   "name a field." Identity is a property of the tree's shape. Any
   authoring model that treats the name as a settable string on a flat
   record is modeling something the file format does not have.
2. **`Kids` requiredness is a *function of widget count*.** One merged
   widget ⇒ `Kids` must be **absent**. Two widgets ⇒ `Kids` must be
   **present** and the widgets must be **separate objects**. So adding a
   second widget to a merged field is not an append — it is a **structural
   split of an existing object**. This is the Shape A→B *promotion* the
   research does not name, and it is where all the invariant risk lives
   (§7.1).
3. **A non-terminal field has no type.** So a name can collide with
   something that is neither "same type" nor "different type" — a third
   branch neither Acrobat's UI nor the research has, because neither
   exposes hierarchy authoring (§3.1.3).

### 1.4 What the parity research got right, and the two places it needs correcting

The 2026-08-03 `Acrobat_Features` findings are good work and I am adopting
most of them. Two corrections, both material:

- **The Q1 recommendation names the right *goal* and the wrong *object*.**
  "`pdfce-core`'s internal field model must be built around the field/
  `/Kids` object graph … from day one" reads, against the shipped code, as
  "replace `Vec<Field>`." That would be a large refactor of fuzz-tested
  (fuzz target 13, ~1.3M runs), corpus-proven read code, for no read-side
  benefit — and it would not by itself fix anything, because the actual
  defect class is on the write side. §3.1 restates the goal in terms of
  the write path.
- **The `/P` bit-6 `must_have` is currently unbuildable as a live gate**
  (§1.2.4). The research could not know this; it catalogs Acrobat, not
  pdfce's internals. §3.6.2 overrides it.

---

## 2. Options considered

### O1 — Flat write model: `add_field` appends to `/AcroForm/Fields`, name is a string

**Rejected, and not on "we'd refactor later" grounds.** It emits malformed
documents *immediately*.

Two top-level fields with the same `/T` produce two fields with the same
fully-qualified name. §12.7.3.2 makes the FQN the identity; there is no
disambiguator. pdfce's own reader copes with this only accidentally —
`fields_named()` returns an *iterator*, and the setters
(`fill_text_field`, `set_choice_value`, `set_button_state`) filter by FQN
and write to **every** match. That fan-out is a defensive coping mechanism
for malformed third-party input. Using it as the *intended* result of
pdfce's own authoring would mean pdfce deliberately produces the shape its
reader treats as damage.

Cost if shipped first, stated plainly because the brief asked: it is not
a refactor cost, it is a **corrupt-output cost, plus a data-loss cost on
every document authored in the interim, plus a migration problem with no
migration path** — you cannot retroactively decide which of two same-named
fields the operator meant. The reversal expense is not in the code; it is
in the files.

### O2 — Replace the read model with a full graph (`FieldNode` tree, `Vec<Field>` deleted)

**Rejected.** Correct in principle, wrong in cost/benefit.

It rewrites `parse_acroform`/`walk_field` and every consumer
(`list-fields`, all three setters, `regenerate_appearances`,
`flatten_fields`, `FormData::from_acroform`, the fuzz target, the R85
oracle) to buy nothing the read path needs. Read/fill/flatten are correct
today over the projection, including multi-widget fan-out. R46/R34/R85 are
all proven against the current shapes. Churning them to enable authoring
puts proven code at risk to serve unproven code.

The research's real requirement — *the merge-vs-refuse branch should fall
out of the model, not be special-cased on top of it* — is satisfied by a
**single write-side resolver**, not by rewriting the reader.

### O3 — Graph resolver on the write path, flat projection retained on the read path — **CHOSEN**

The `/Kids` graph is the file. pdfce keeps two views of it:

- **Read projection** (`AcroForm.fields: Vec<Field>`) — unchanged, correct,
  shipped, fuzz-tested. The right shape for "show me the fields / fill
  this one / flatten these."
- **Write resolver** (new, authoring-only) — walks the raw graph, retains
  non-terminal nodes, and answers exactly one question: *what, if
  anything, does this fully-qualified name currently name?*

Every authoring write goes through the resolver. There is exactly one
place the collision branch lives, which is what the research asked for.

Additive, not disruptive: `Field` gains `parent: Option<ObjId>` (needed for
ancestor-`/V` writes and subtree rename) as a new `pub` field, and nothing
existing changes shape.

### O4 — Author fields, but only ever as flat top-level names (refuse dots)

**Rejected.** pdfce already *reads and fills* dotted-FQN forms, and
`parse_fdf` already flattens an incoming `/Kids` hierarchy into dotted
names for import. Refusing to author what it can already consume is an
arbitrary asymmetry, and it makes round-tripping an FDF into a
pdfce-authored form impossible at exactly the point it is most useful.
Hierarchy authoring is cheap once the resolver exists (§3.1.4).

---

## 3. Decision

### 3.1 Q1 — the data model

#### 3.1.1 The verdict, stated as the thing that is actually binding

**The research's recommendation is ACCEPTED in substance and REPLACED in
mechanism.** The binding rule is not "the model is a graph." It is:

> **Field identity is the fully-qualified name; the fully-qualified name
> is derived from the object graph, not stored; therefore every authoring
> write must resolve the name against the graph *before* deciding what to
> write, and must be able to attach a widget to an existing node without
> creating a second node.**

This becomes **R100**. It is stronger than "use a graph" because it names
the failure it prevents (a second node) rather than the representation it
prefers.

#### 3.1.2 The resolver

One function, one choke point:

```rust
/// Resolve a fully-qualified field name against the live object graph.
///
/// This is the ONLY entry point through which any authoring write may
/// learn what a name currently denotes (R100). It walks the raw
/// `/AcroForm/Fields` tree — including non-terminal grouping nodes, which
/// the read projection (`AcroForm.fields`) deliberately discards — because
/// §12.7.3.2 derives the FQN from the tree's shape, so only the tree can
/// answer the question.
pub fn resolve_field_path<G: ObjectGraph + ?Sized>(
    graph: &G,
    fqn: &str,
) -> Result<FieldPath, FormAuthorError>;

pub enum FieldPath {
    /// No node bears this name. `deepest` is the lowest existing ancestor
    /// (None ⇒ create from the root); `remaining` are the segments that
    /// must be created beneath it.
    Vacant { deepest: Option<ObjId>, remaining: Vec<String> },
    /// A TERMINAL field bears this name.
    Terminal { id: ObjId, ft: Option<FieldType>, kind: Option<ButtonKind>, shape: FieldShape },
    /// A NON-TERMINAL grouping node bears this name. It has no type of its
    /// own (Table 220) and cannot become a fillable field.
    Grouping { id: ObjId },
}

pub enum FieldShape { MergedSingleWidget, KidsWidgets { n: usize } }
```

#### 3.1.3 The collision branch — three ways, not two

| `FieldPath` | Requested type | Outcome |
|---|---|---|
| `Vacant` | any | **CREATE.** New terminal field + widget, plus any intermediate non-terminal parents `remaining` requires. |
| `Terminal` | `(ft, kind)` **matches** | **MERGE.** Attach a new widget to the existing node. Shape A→B promotion if it was merged (§3.1.5). This is the radio-group mechanism and the repeated-field mechanism — one code path, per the research's correct insistence. |
| `Terminal` | `(ft, kind)` **differs** | **REFUSE** — `FormAuthorError::FieldTypeCollision { fqn, existing, requested }`. |
| `Grouping` | any | **REFUSE** — `FormAuthorError::NameIsGroupingNode { fqn }`. |

**The fourth row is not in the research, and it is real.** If `Address.City`
exists, then `Address` names a non-terminal container. A request to create a
terminal text field named `Address` is neither a same-type merge nor a
different-type collision — the existing node *has no type* (Table 220:
*"a non-terminal field does not logically have a type of its own"*).
Acrobat's UI has no such branch because it never exposes hierarchy
authoring; pdfce does (§3.1.4), so pdfce needs it.

Both refusals are **reachable and must be proven firing** (R96). The
reachability tests are trivially constructible — create a text field
`Name`, then request a checkbox `Name`; create `A.B`, then request a
terminal `A` — which is exactly why there is no excuse for shipping them
untested. Contrast Pass 19.4's R91 refusal, which was correct, wired, and
structurally unreachable.

#### 3.1.4 Dotted names are paths, always; a partial name may never contain a period

The research flagged a GAP: does Acrobat's Name box interpret a dotted
string as hierarchy or as one flat `/T`? **pdfce does not need the
answer.** It adopts the spec's own model as its own documented choice:

- `--name a.b.c` ⇒ non-terminal `a`, non-terminal `a.b`, terminal `c`,
  reusing any of those that already exist.
- A partial name (`/T`) containing a PERIOD (2Eh) is **refused** —
  `FormAuthorError::PeriodInPartialName`. There is no escape hatch and none
  is needed: §12.7.3.2 reserves the period as the path separator, so a `/T`
  containing one has no unambiguous FQN. Refusing is the only honest
  reading, and an escape hatch would author exactly the ambiguity the spec
  avoids.

Disclosed at creation: *"`a.b.c` creates a 2-level hierarchy (`a` → `a.b`
→ terminal `c`)."* Fuzzy-never-sneaky — the operator is told what the dot
did.

#### 3.1.5 Shape A→B promotion — the load-bearing new primitive, and the one the research does not name

Table 220 permits the merged form **only** while there is exactly one
widget. Attaching a second therefore requires a **split**, not an append:

1. Allocate a new widget object. Move the annotation keys off the field
   dict onto it: `/Subtype /Widget`, `/Rect`, `/AP`, `/AS`, `/MK`, `/F`,
   `/BS`, `/Border`, `/DA` *if it was the widget's*, `/P`, `/OC`, `/StructParent`.
2. Remove `/Subtype`, `/Rect`, `/AP`, `/AS`, `/MK`, `/F`, `/BS`, `/Border`,
   `/P` from the field dict. Field keys (`/FT`, `/T`, `/TU`, `/TM`, `/Ff`,
   `/V`, `/DV`, `/AA`, `/Opt`, `/MaxLen`, `/Q`, `/DA`-as-field-default)
   stay.
3. Write `/Kids [<widget1> <widget2>]` on the field dict, `/Parent` on each
   widget.
4. **Retarget the page's `/Annots`**: the entry currently references the
   merged dict, which is no longer an annotation. It must now reference
   widget1. **Miss this and the page's `/Annots` points at a dict with no
   `/Subtype /Widget`** — and `dict_is_widget`'s defensive "or has `/Rect`
   or `/AP`" fallback may partially mask it, producing a document that
   half-works in pdfce and misbehaves elsewhere. This step is the single
   easiest thing in the whole family to forget.
5. Set the new widget's `/P` back-reference and add it to the target page's
   `/Annots` via the existing `annots_append`.

Promotion writes an object the operator *did* logically change, so it is
legitimate under §5.1's `save_full` per-object contract — but see §7.1 for
the R94 provenance hazard it carries.

#### 3.1.6 Never collapse B back to A

When deletion drops a Shape-B field to one remaining widget, pdfce
**leaves it as Shape B**. Collapsing would rewrite two objects for a purely
cosmetic normalization. `ARCHITECTURE.md` §5.6 — *"Never normalize"* —
already forbids this class of tidying; this is that rule applied to a shape
it did not anticipate. Becomes **R102**.

#### 3.1.7 What changes in the shipped Pass 7.x code

**Nothing changes shape. Two things are added, three are fixed.**

*Added:*
- `Field.parent: Option<ObjId>` — a new `pub` field. Needed so a write can
  reach the ancestor that declared an inherited `/V` (today writes always
  go to the terminal, and an inherited-`/V` group is written to the wrong
  dict). Additive; check against
  `D:\dev\rag\rust\rust-style-guide-and-api-guidelines.md` per rule 10.
- The resolver + authoring module (`forms_author.rs`), consuming the raw
  graph, not `AcroForm`.

*Fixed in F0 — all four are pre-existing, all four become reachable the
moment authoring can generate the shapes:*
- `fill_text_field`'s inlined duplicate of `regen_field_appearance` (R92).
- Mixed `/Kids` (field-kids + widget-kids) silently dropping widget-kids.
- `remove_fields_from_form` not pruning an empty-`/Kids` parent.
- `/Fields` roots collected reference-only (harmless for reading
  spec-conformant files, fatal if authoring ever writes inline).

**The corpus cannot catch any of this.** `ROADMAP.md`'s Pass 7.0 entry
records: *"corpus max ≈ 63 fields/file … 1× on depth but **no corpus file
nests fields at all**."* pdfce's hierarchy handling has, in effect, never
been exercised against real data — and authoring's entire purpose is to
start generating precisely those shapes. That is the argument for F0.

### 3.2 Q2 — XFA

#### 3.2.1 Dynamic XFA — `out_of_scope`. Agreed, four independent reasons

1. No AcroForm exists as of Acrobat 8.1+, so there is no parity target.
2. Structural XFA editing requires LiveCycle/AEM Designer — outside
   Acrobat, therefore outside the parity brief by definition.
3. Measured demand: `/XFA` in **2 of 2,500** organic files (0.08%),
   decision-008 census.
4. pdfce already ships the honest posture: `XfaPresence` — recognized,
   never parsed.

#### 3.2.2 Static-XFA hybrid — DECIDED HERE: refuse field creation by name

The research flags this as an unresolved GAP requiring follow-up. **It does
not need follow-up, because the answer does not depend on Acrobat.**

A static-XFA hybrid carries **two parallel representations of one form**.
pdfce can write the AcroForm half. pdfce cannot write the XFA half — that
is a template-authoring engine, categorically out of scope. Adding a field
to one half only produces a document where an XFA-aware viewer shows *N*
fields and a non-XFA viewer shows *N+1*.

**That is a silent divergence between what two viewers show the same
user** — the exact failure class fuzzy-never-sneaky exists to forbid. It is
worse than refusing, and it is worse than whatever Acrobat does, because
its harm is invisible at authoring time.

So: **`/XFA` present ⇒ field CREATION refused by name.** Not fill, not
flatten, not read — creation only, because creation is the only operation
that introduces the divergence.

- Cheap: `XfaPresence` already exists in `AcroForm`.
- **Reachable and testable** (R96): a static-XFA-hybrid synthetic fixture is
  straightforward, and the gate must be proven firing.
- **Has a named remedy rather than being a dead end:** the honest escape is
  to *demote the hybrid* — drop `/XFA` from `/AcroForm`, leaving a plain
  AcroForm that pdfce can author freely. That is a real, separate,
  disclosable operation (`forms strip-xfa`), explicitly **not in this
  family**, named here so the refusal has a documented path forward.

Deciding it this way **closes the GAP without resolving it** — pdfce's
behavior is derived from its own capability boundary, so what Acrobat does
becomes merely interesting.

#### 3.2.3 The standing "verify XFA deprecation status" open item

`CLAUDE.md` and `ARCHITECTURE.md` both carry it. **This decision does not
need it answered** — both branches are decided without it. It should not
gate this family. Whether to retire it outright is Ken's (§10.5); I
recommend re-scoping it to "before any XFA *read/fill* work," where it
would actually bear on something.

### 3.3 Q3 — slice order and the P0 floor

#### 3.3.1 The floor is not "a text field." It is "a text field plus the collision branch."

The tempting P0 is: place one text field, done. **Reject it.** A single
text field on an empty document exercises exactly the code path that has no
interesting decisions in it — and leaves the merge/refuse branch, the whole
subject of Q1, for a later slice that will then be building on an authoring
path whose write model was never asked the hard question.

The collision branch is not a feature layered on creation. It is the *first
thing creation must do*, on every single call, before it knows what to
write. Splitting it out means shipping an `add-field` that appends
blindly — O1 — for one slice. Even one slice of that authors documents
that cannot be un-authored.

So **F1 = text field creation *through the resolver*, with all four
`FieldPath` outcomes live and tested**, which necessarily includes Shape
A→B promotion (because merging into a merged single-widget field is the
common merge case).

#### 3.3.2 Why text-only, and why not checkbox in P0

Text is the only type whose appearance is a **single baked stream through
the already-shipped §12.7.3.3 generator** —
`annot_author::build_field_text_appearance`, reused verbatim from Pass 6.2
via `regen_field_appearance`. Zero new appearance machinery.

Checkbox/radio need a **keyed `/AP /N` sub-dictionary** — one stream per
named state — plus a check glyph (ZapfDingbats, or drawn paths), plus
`/AS`, plus the `/Off` convention. The survey confirms the split is already
structural in the shipped code: `regenerate_appearances` handles only
`Text` and `Choice`, with `_ => continue`, and **buttons never regenerate
appearances at all**. A checkbox pdfce creates without authoring its state
sub-dictionary would be a field that draws **nothing** — the survey says so
in as many words: *"If a checkbox has no pre-authored `/AP`, nothing is
drawn."*

That is a real new capability (the first button-appearance *generator* in
the codebase), and it belongs in its own slice where it can be tested as
one, not smuggled into the slice that is proving the identity model.

#### 3.3.3 The slice order, and why F0 exists

Build order: **F0 → F1 → F2 → F3 → F4 → F5**, with F6/F7 as fast-follows.
Full acceptance criteria in §6.

**F0 (correctness, no operator surface) has direct precedent**: decision
019's Pass 19.0 was exactly this — *"CORRECTNESS ONLY, no new operator
surface"* — and §4.3 of that decision is titled *"Why the correctness slice
comes before the feature slices."* The same argument applies with more
force here, because unlike 19.0 (which consolidated three trackers that all
worked), F0 addresses code paths that **have never run against the shapes
authoring will create**.

**F0 is exempt from rule 11** (each feature Pass ships its CLI subcommand)
on the 19.0 precedent: it adds no capability, so there is nothing to
expose. State the exemption in the Pass entry so the omission reads as
reasoned rather than missed — the same discipline Pass 19.3 used when it
noted rule 11 did not apply to it.

**GUI is last (F5)**, per both the 19.x precedent (19.0 core → 19.1/19.2
core+CLI → 19.3 GUI) and R83: the type palette may only offer types core
can actually create, so it cannot be designed until F1–F3 fix the set.

#### 3.3.4 Explicitly cut from this family

- **Signature-field creation — DEFERRED to the Signatures Pass (Pass 10).**
  Structurally trivial (an empty `/FT /Sig` widget), but placing a
  signature field pdfce cannot sign, in an app with no signing subsystem,
  is an affordance for a capability that does not exist — R83's spirit if
  not its letter. It also interacts with the certification gate in ways
  that only make sense once signing exists. **See §10.3** — if Ken
  specifically wants "add a signature field for someone *else* to sign,"
  that is a legitimate use case and it changes this answer.
- **Barcode fields — OUT OF SCOPE, and the rejection is well-grounded, not
  a shrug.** The research found *no sourcing at all* on the creation floor.
  More decisively: a barcode field's content is populated by its
  JavaScript calculate action, and **decision 009 prohibits executing
  embedded PDF JavaScript, permanently.** A barcode field pdfce created
  would therefore never populate — an affordance for a capability pdfce has
  ruled out on principle. It is a genuine parity subtraction and §10.4
  flags it to Ken as such.
- **Field auto-detection ("Prepare Form")** — already filed as a separate
  later Pass in the Pass 7.0 residuals. Unchanged. It is a *hint generator*
  under fuzzy-never-sneaky and its output feeds `add-field`; it needs this
  family finished first, not merged into it.
- **The session-scoped "use current properties as new default" template**
  (research: `should_have`) — a UI convenience with no file-format
  consequence. Fast-follow F7, not P0.

### 3.4 Q4 — tab order

#### 3.4.1 The rule

> **`/Tabs` is a MODE, not a snapshot.**
>
> 1. Under `/Tabs /R`, `/Tabs /C`, or `/Tabs /S`, pdfce **reorders
>    nothing** on field insertion. The order is *computed by the consumer*
>    from the mode; there is no stored sequence to keep in sync. A new
>    field lands in its natural row/column/structural position by
>    construction.
> 2. Under an **explicit/manual** order — no `/Tabs` entry, or a mode pdfce
>    cannot compute — the new field's widget is **appended to the end** of
>    the page's `/Annots` array, and pdfce **discloses** that the new field
>    is last in tab order.
> 3. pdfce **never writes `/Tabs` as a side effect of field creation.** A
>    page that had no `/Tabs` keeps none (Table 30: optional).
> 4. Setting `/Tabs`, and authoring an explicit order, are **separate,
>    explicit operations** (F4) — never implied by placing a field.

Becomes **R104**.

#### 3.4.2 Why this differs from the research's recommendation, and why the difference matters

The research recommends: *"a newly-created field is always correctly
RE-SORTED into its natural geometric/structural position automatically."*
The **effect** is right. The **mechanism** as worded is wrong and would
cause a real bug.

Read literally, "pdfce re-sorts" invites an implementation that sorts the
page's `/Annots` array by widget rect on every insertion. That would:

- **rewrite the page object on every field placement**, reordering
  references to annotations pdfce did not logically touch — a
  minimal-diff violation (R32/R46) for zero benefit;
- **change the z-order/paint order of unrelated annotations**, since
  `/Annots` array order is also paint order — a *visible* change to the
  document, caused by a *non-visible* feature;
- and produce a diff that grows with the number of annotations on the page,
  in a project whose incremental-save default exists precisely to keep
  diffs proportional to the edit.

The correct observation is stronger than "re-sort": under `S`/`R`/`C`
**there is nothing to sort**, because the order is not stored. The research
reached the right destination by describing a road that should not be
built. The corrected rule does less work and is more correct.

#### 3.4.3 The case the research missed: `/Tabs /S` + an untagged new field

Structure order (§14.7) derives tab order from the **tag tree**. Per §3.5,
a pdfce-authored field is **untagged** (there is no structure-tree writer —
§1.2.7). Therefore, on a page with `/Tabs /S`, a newly created field has no
position in the tab order **at all** — not "last," *undefined*. Different
viewers will do different things.

This is not a hypothetical: `/Tabs /S` is Acrobat's own recommended default
for well-tagged forms (sourced in `forms__tab_order.md`), so tagged forms —
the ones most likely to carry `/Tabs /S` — are exactly the ones where this
bites.

**Acceptance criterion (F1, not deferred to F4):** detect `/Tabs /S` on the
target page + an untagged new field, and **disclose** — *"this page uses
structure tab order; the new field is not in the structure tree and its
tab position is undefined. Set an explicit tab order, or use row/column
order for this page."* Detection is cheap (the `/StructTreeRoot` boolean
already exists in `addtext.rs`'s R73 path).

#### 3.4.4 A spec-RAG gap that gates F4 — verified this session

`D:\Dev\Rag-Specialized\PDF_Spec\` covers `/Tabs` **only** as a passing
mention in `iso32000__s__12.5.2.md` (values `R`/`C`/`S`, "interaction-order
only"). Grepping the corpus:

- **Table 30's own `/Tabs` row is not in the RAG** (`iso32000__s__7.7.3.md`
  contains no `Tabs`).
- **PDF 2.0's additional `/Tabs` values are not covered** — the delta file
  `iso32000__delta__pdf20_pass1.md` contains no `Tabs` at all.
- **§14.7 structure-order derivation is not covered** for this purpose.

**F4 must not start without a `pdfce-spec-librarian` dispatch** for Table 30
`/Tabs`, §14.7 structure-order derivation, and the ISO 32000-2 delta.
Implementing row/column sort algorithms from training-data memory would
violate project rule 1 outright. Named as a hard prerequisite in §6.F4.

### 3.5 Q5 — tagged documents

#### 3.5.1 This is not a re-opening of FF-I, and here is the test for that

The brief is right to guard this. FF-I was cut on 2026-08-03 (decision 019
§3.7) with a specific rationale: *"a **partial** structure-tree writer …
is judged worse than none — a document that looks tagged-and-consistent
but silently drifts out of sync with its own structure tree is a harder
failure to detect than one that visibly discloses 'structure not
updated'."*

The test for whether a proposal re-opens that cut is: **does it build a
partial structure-tree writer?** Writing a `/StructElem` for a new form
field does. It requires allocating a `/StructParent`, inserting into the
`/K` tree at a chosen position, and updating `/ParentTree` +
`/ParentTreeNextKey` — a real structure-tree writer, arriving as a
side-quest of a forms Pass, which is precisely the shape decision 019 said
needs its own decision record. **So: not in scope, stays with FF-I,
rationale unchanged.**

#### 3.5.2 But the accessibility win for forms is `/TU`, and it is not the tag tree

This is the decisive fact, and it is directly sourced in
`forms__field_property_model.md` (primary source: WebAIM):

> *"Screen readers read form fields through the **interactive-field layer
> directly, not through the tag tree**, making `/TU` the practical,
> load-bearing accessible-name mechanism for forms specifically."*

`/TU` is one optional text string on Table 220, **on an object pdfce is
creating from nothing.** There is no existing structure to drift out of
sync with, no round-trip risk, no partial-writer problem. It is the whole
of the sourced accessibility benefit, at essentially zero cost and zero
risk.

The research's own `must_have` says it: *"any pdfce field-creation UI must
make `/TU` entry a first-class, prompted step — not an optional
afterthought — given how load-bearing it is for accessibility and how easy
Acrobat's own model makes it to skip."*

#### 3.5.3 The decision, split three ways

1. **IN SCOPE, P0, mandatory-or-declined:** every field pdfce authors
   carries `/TU`, or the operator has **explicitly declined** it and the
   declination is recorded in the operation's disclosure. CLI: `--tooltip
   <text>` or `--no-tooltip` — omitting **both** is an error, not a silent
   default. This is deliberately stricter than a warning: a silently-absent
   `/TU` is invisible to the person who created it and load-bearing for
   the person who cannot see the form. Becomes **R105**.
2. **OUT OF SCOPE, stays FF-I:** writing `/StructElem` / `/ParentTree`
   entries. Unchanged rationale.
3. **What ships instead of a partial writer:** on a document with
   `/StructTreeRoot`, field creation **discloses** — *"this document is
   tagged; the new field is NOT in the structure tree."* This is the
   existing R73 posture applied to a new operation, and it is already
   **strictly better than Acrobat**, whose own workflow leaves new fields
   untagged and says nothing (sourced, `forms__authoring_limits_and_refusals.md`).

#### 3.5.4 One note for whenever FF-I is picked up

FF-I now has a **second consumer** (form-field `/StructElem` authoring)
alongside its original one (text-edit `/ActualText`/MCID). That raises its
eventual value and slightly changes its scope. It is a data point to record
against FF-I — **not** a reason to pull it forward, and this decision does
not.

### 3.6 Q6 — the four unresolved items

#### 3.6.1 Combine Files: auto-RENAME vs. LINK — it looks like a contradiction and reads as one behavior

Do not resolve it by more research; do not paper over it. **Read the two
sources against each other:**

- `core_ops__merge_combine_files.md` (already built, already shipped-on):
  Acrobat auto-renames duplicate fully-qualified names with a per-source
  prefix (`Doc0_`/`Doc1_`) — *"if this is disabled or fails"*, linking
  occurs.
- The second source: same-named fields across merged documents *"are now
  merged into one field."*

**That is not a contradiction. That is a default plus its own documented
fallback**, and the first source states the fallback in its own text. The
second source describes the fallback branch. Two accounts of one behavior
with two branches. Version/entry-point variation may add noise, but the
reconciliation does not require it.

**pdfce's posture, which is correct under either reading:** neither
behavior should be silent. Today, unconditional auto-rename would *break
the legitimate case* — merging two copies of one form template, where the
operator **wants** the fields linked, is exactly the mechanism radio groups
and repeated fields are built on. Silently renaming destroys it.

So: **make it an operator choice, and disclose either way.**
`pdfce-cli merge --on-field-collision rename|link|refuse`, defaulting to
`rename` (preserving already-shipped behavior — this is a fast-follow
against shipped code, not a behavior change), with:

- same-name + same-type collisions **disclosed** (*"N field name(s) will
  merge into one field"* under `link`, *"N field(s) renamed"* under
  `rename`);
- same-name + **different-type** collisions **warned or refused** — the
  research's own `should_have`, and an exceed: *"Acrobat's own merge path
  does not appear to check this."*

**Filed as fast-follow F7, against the already-shipped merge. Does not
block F0–F5.** Changing already-shipped merge semantics inside a
field-authoring family would be scope creep in the wrong direction.

#### 3.6.2 Encrypted documents: structurally inapplicable — and the bit-6 gate is dead code today

Two layers, and both are settled here.

**Layer 1 — the Acrobat conflict is not pdfce's conflict.** The two sourced
accounts differ on Acrobat's *UI workflow ordering* (entering Prepare Form
directly does not prompt for the permissions password; entering via Edit
first does). The research already reaches the right conclusion: pdfce has
no modal tool entry points, so *"this Acrobat quirk is structurally
inapplicable to pdfce's own architecture."* Agreed, adopted, closed. The
stricter second account ("add fields before securing, or remove security")
is best read as describing a user who does not have the permissions
password — which pdfce models as "refuse, disclosed" regardless.

**Layer 2 — and this is the part the research could not see.** The
research's `must_have` is that field creation consult the `/P` bit-6
permission. **Implement that today and it never executes**: `/Encrypt`
documents are refused outright by *every* authoring path in `EditSession`,
before any forms code runs (§1.2.4). A bit-6 gate sitting behind that
refusal is R96 verbatim — *"a guard clause placed after a filter the
guarded case cannot pass is dead code that looks live."*

**Decision:** do **not** build a bit-6 gate in this family. Instead:

- Record in the authoring module's file-level doc that encrypted documents
  are refused by an earlier, coarser gate, that the finer bit-6 distinction
  is **owed**, and that it becomes reachable only when Pass 5 (Encryption)
  lands the ability to open an encrypted document at all.
- Add the owed refinement to Pass 5's own scope, where it can be built
  *and* proven firing.

This generalizes R96 in the prospective direction — R96 says *detect* dead
guards; the lesson here is *don't write one*, record the debt where it
becomes payable. Becomes **R103**.

**What this family does inherit for free, and must test as firing:**
`suppressed_object_count() > 0 → ObjectCreationWouldExposeHiddenObjects`,
and `check_certification()` — the DocMDP gate. On certification: the
research finds no DocMDP tier permitting *new field creation*
post-certification (even the "form fill-in and signatures" tier covers
filling already-defined fields via FieldMDP). So the authoring gate is
**strictly stricter than fill's**: fill permits `/P >= 2`; **creation
refuses at any `/DocMDP` tier, and on any `/FieldMDP`.** That is a
different, reachable, testable gate, and it is the one to build.

#### 3.6.3 Radio-group member deletion: the two GAPs are not acceptance criteria, so they cannot block

The brief asks whether "resolve empirically against a real Acrobat install
once fixtures exist" is acceptable as a deferred acceptance criterion, or
must block. **Neither — the framing is wrong, and the research itself
already supplies the correction.**

`forms__radio_group_authoring.md` recommends pdfce adopt the
spec-consistent behavior and disclose it *"as pdfce's own design choice
rather than an unverifiable 'matches Acrobat' claim."* Once you do that,
**the Acrobat answer stops being load-bearing.** pdfce's acceptance
criterion becomes "pdfce's documented rule holds, proven by test" — which
needs no Acrobat install, no fixture, and no deferral. The empirical check
becomes a curiosity, not a gate.

**The rules, decided now:**

- **Mid-group member deletion.** Remove the widget from the field's
  `/Kids` and from its page's `/Annots`; delete the widget object. The
  remaining members are otherwise untouched.
  **Plus a consequence the research does not name:** if the removed
  widget's on-state equals the field's `/V`, the field's value now points
  at a state **no remaining widget can display** — a malformed field.
  pdfce therefore sets `/V /Off` and every remaining kid's `/AS` to
  `/Off`, and **discloses that the group's selection was cleared by the
  deletion.** Silently leaving a dangling `/V` would be the sneaky
  outcome; silently clearing it without saying so would also be.
- **Last-member deletion.** Also remove the now-empty parent field from
  `/AcroForm/Fields` (and from its own parent's `/Kids`, recursively, if
  that leaves a grouping node childless). No invisible orphan lingers.
  This requires fixing `remove_fields_from_form`'s missing empty-parent
  prune (§1.2.2) — which is in F0 for exactly this reason.
- **One rule, not a radio special case.** The same rule applies to any
  field type whose last widget is deleted. Consistent with the "one merge
  mechanism, not a radio-specific path" principle the research is right
  to insist on.
- **No Shape-B→A collapse** on the way down (§3.1.6 / R102).

All three are pdfce's own documented design choices, flagged as such, and
provable by pdfce's own tests.

> **✅ IMPLEMENTED 2026-08-07 — `817b268` (Pass 20.2 COMPLETE).** All four
> rules above are built and test-proven: mid-group deletion with the
> **dangling-`/V` clear-and-disclose** (`FieldDeletion.selection_cleared`,
> reported in prose **and** as a machine-readable field — §3.6.3's
> "silence either way is sneaky" honoured in both directions);
> **last-member deletion delegating to `delete_field`** rather than
> reimplementing it, so the two paths cannot disagree about what *gone*
> means; **one rule, not a radio special case**
> (`FieldDeletion.emptied_parents` carries the recursive grouping-node
> prune, F0's fix reaching its first real caller); and **no Shape-B→A
> collapse** (R102 — a 3→1 group keeps its `/Kids` parent).
>
> **Two naming corrections this section does NOT reflect above, and which
> `ROADMAP.md` is canonical for:** the verbs shipped as **`delete-field` /
> `delete-widget`**, not `remove-*` — ruled by **§0.1** the same day
> (`delete` is the house word; forms is a verb-first domain, R161) — and
> they are **two verbs, not one with an optional `--index`**, because an
> index whose absence silently means *delete everything* is a footgun.
>
> **The test that proves this is the origin of standing rule R162.** Its
> dangling-reference assertion reads `/Fields` and every page's `/Annots`
> **raw, deliberately not through `parse_acroform`** (R159 — a repairing
> reader cannot witness a write path), and it **proves its own instrument
> first** by re-deriving the pre-deletion document and asserting three
> widgets were named there. Build record: `ROADMAP.md`'s COMPLETION
> ADDENDUM on the `Pass 20.2 + Pass 20.3` *Shipped* entry.

---

## 4. Rationale — the reasoning behind the reasoning

### 4.1 Why "expensive to reverse" understates the flat-write-model cost

The brief describes the model choice as *"the single decision that is
expensive to reverse."* True, but the framing invites a cost-of-refactor
comparison, and that comparison would come out closer than it should.

The real cost is not in the code. **A flat write model produces documents
whose damage is unrecoverable in principle.** Two top-level fields sharing
an FQN cannot be told apart afterward — not by pdfce, not by Acrobat, not
by the operator, because the file format provides no disambiguator
(§12.7.3.2). There is no migration, because there is no fact to migrate
*to*. Every document authored during the interim is permanently ambiguous.

That reframes the decision from "which is the better architecture" to
"which one is *correct*," and only one is.

### 4.2 Why the shipped flat reader is not the same mistake

Because it is a **projection of a graph that exists**, not a *replacement*
for one. The graph is the file; the reader derives a view of it; nothing is
lost that reading needs. Authoring is different in kind: it does not derive
a view, it **decides what the graph becomes.** A projection cannot make
that decision, because the information it dropped (non-terminal nodes,
parents) is exactly the information the decision needs.

This is why O3 is not a compromise between O1 and O2. Read and write have
genuinely different information requirements, and giving each the shape it
needs is the correct answer, not a hedge.

### 4.3 Why the corpus's silence is the argument for F0

`ROADMAP.md` records that **no corpus file nests fields at all**, and that
the field-count guard has >7,900× headroom while the depth guard has 1×.
Read plainly: pdfce's hierarchy-handling code has been *written* but never
meaningfully *exercised*. It is fuzz-tested for crashes (fuzz target 13,
~1.3M runs) — which proves it does not panic, not that it is correct.

Authoring's entire purpose is to start generating those shapes. Building
authoring on top of an unexercised reader means the first real test of the
reader is a document the operator is depending on.

This is R93's discipline (*"a code comment asserting a behavior is not
evidence the behavior holds"*) applied to a whole subsystem: Shape-B
handling *looks* correct, and reading it will not tell you. Pass 17.1/17.2
already found *"real, previously-silent data loss in `flatten_fields`"* —
in this exact area, by building an oracle rather than by reading code. F0
is that lesson applied before the fact rather than after.

### 4.4 Why `/TU` clears the FF-I bar and `/StructElem` does not

The distinction is **whether there is anything to stay in sync with.**

FF-I's cut rationale is about *drift*: a partial writer updates some of a
structure tree, the rest silently diverges, and the document looks
consistent while being wrong. That failure requires a pre-existing
structure the writer is partially maintaining.

`/TU` has no such structure. It is a string on an object pdfce is creating
from nothing, read directly by assistive technology through the
interactive-field layer, *bypassing the tag tree entirely* (WebAIM,
sourced). There is nothing to drift from. It is not a small piece of the
FF-I problem; it is **not that problem**.

This is why the answer is a split rather than a judgment call about how
much accessibility work is worth doing.

### 4.5 Why deciding the Acrobat GAPs from pdfce's own boundary beats researching them

Three of the four Q6 items, plus the static-XFA question, plus the
tab-order insertion question, are all "the research could not determine
what Acrobat does." The instinct is to research harder or defer.

But in every one of those cases, **pdfce has a determinate answer that
does not depend on Acrobat**:

- static XFA: pdfce cannot write the XFA half → refuse (§3.2.2);
- tab order on insertion: under a computed mode there is nothing stored →
  reorder nothing (§3.4.2);
- radio member deletion: the spec-consistent outcome is well-defined →
  adopt it, disclose it as pdfce's own (§3.6.3);
- encrypted: pdfce has no modal entry points → the quirk cannot occur
  (§3.6.2).

The generalizable move: **when parity research hits a GAP, check whether
pdfce's own architecture already determines the answer before treating the
GAP as a blocker.** A parity claim needs Acrobat's behavior; a *design
choice, disclosed as one*, does not. Decision 008's GAP-not-guess
discipline forbids *inventing* Acrobat's behavior — it does not require
waiting for it when pdfce can answer on its own terms.

### 4.6 Why the collision branch cannot be a follow-on slice

Because it is not a branch *in* creation — it is the branch that decides
*what creation is*. Every `add-field` call must ask "what does this name
currently denote?" before it can know whether it is creating a node,
attaching a widget, or refusing. Deferring it does not defer a feature; it
ships a version that skips the question, and skipping the question is O1.

---

## 5. Standing rules proposed (binding; `pdfce-librarian` assigns final `Rnn`; current ceiling **R96**)

> **RENUMBERED by `pdfce-librarian`, continuation 71, 2026-08-03.** This
> section originally proposed the six rules below as **R97–R102**, against
> a ceiling of R96 believed current at drafting time. That ceiling was
> stale by the time this decision was filed: continuation 70, running
> concurrently, had already claimed **R97–R99** for three unrelated Pass
> 8.1 findings (`redact_apply.rs` extraction, the confirmation-dialog
> real-outcome rule, and dock-pane action ordering — see `ROADMAP.md`
> Standing rules). The six rules below are therefore renumbered
> **R100–R105** throughout this document (prose and the Appendix A JSON
> block both updated in place); the real ceiling immediately before this
> decision was **R99**, not R96. See `ROADMAP.md` Standing rules for the
> filed R100–R105 text (verbatim from this section) and Appendix A's new
> `renumbered_by_librarian` field for the machine-readable mapping.

- **R100 — Field identity is the fully-qualified name, and every authoring
  write resolves that name against the object graph before it writes.**
  §12.7.3.2 derives the FQN from the field tree's shape rather than storing
  it, so only the tree can answer what a name denotes. All field-creation,
  rename, and widget-attachment writes pass through one resolver
  (`resolve_field_path`); none may append to `/AcroForm/Fields` without it.
  Two same-FQN sibling fields are a malformed document pdfce must never
  author — pdfce's own reader treats that shape as damage to cope with
  (`fields_named()` fan-out), not as an intended result.
- **R101 — A widget kid carries no field keys.** A `/Kids` entry pdfce
  authors as a *widget* must not contain `/T`, `/FT`, or `/Kids`.
  `pdfce-core`'s own `kid_is_field` promotes any such kid to a separate
  terminal field, silently destroying the group semantics (radio mutual
  exclusivity, shared `/V`) the widget was created to have. Verified
  against the shipped parser, not inferred.
- **R102 — pdfce never normalizes field shape.** Shape A→B promotion occurs
  **only** when a second widget makes the merged form illegal under Table
  220; Shape B **never** collapses back to A when deletion leaves one
  widget. `ARCHITECTURE.md` §5.6 ("never normalize") applied to a shape it
  did not anticipate — cosmetic re-tidying of an object the operator did
  not logically change is a minimal-diff violation regardless of how much
  nicer the result looks.
- **R103 — A guard whose precondition is already refused by a coarser
  earlier gate is not built; the refinement is recorded as owed to the Pass
  that removes the coarser gate.** The prospective form of R96. Verified
  instance: the `/P` bit-6 field-creation permission is unreachable while
  `/Encrypt` documents are refused outright by every authoring path, so it
  is filed against Pass 5 (Encryption) rather than written now as a gate
  that cannot fire.
- **R104 — `/Tabs` is a mode, not a snapshot.** Under `/Tabs /R`, `/C` or
  `/S`, pdfce reorders nothing on field insertion — the order is computed
  by the consumer and there is no stored sequence to maintain. Re-sorting
  `/Annots` to "realize" a computed order rewrites references pdfce did not
  logically touch **and changes annotation paint order**, a visible change
  caused by a non-visible feature. Under an explicit/manual order the new
  widget is appended to the end and that fact is disclosed. `/Tabs` is
  never written as a side effect of field creation.
- **R105 — Every field pdfce authors carries `/TU`, or an explicitly
  recorded operator declination.** For form fields, `/TU` — not the
  structure tree — is the accessible name assistive technology actually
  reads (WebAIM, sourced). It costs one optional string on an object pdfce
  is creating anyway, and its absence is invisible to the sighted operator
  who created the field. Omitting both `--tooltip` and `--no-tooltip` is an
  error, never a silent default.

---

## 6. Pass slicing — proposed **Pass 20.x** (`Pass 20` verified free)

Every slice below ships `cargo fmt --check` + `clippy -D warnings` clean
workspace-wide, `cargo tree -p pdfce-core -p pdfce-render` GUI-free, and
zero new Cargo dependencies unless flagged. Every `pub` item added to
`pdfce-core` is checked against
`D:\dev\rag\rust\rust-style-guide-and-api-guidelines.md` (rule 10).

### F0 — Field-hierarchy correctness + the authoring substrate (core only; CORRECTNESS ONLY, no operator surface)

**Rule 11 exempt** — adds no capability, therefore no subcommand. State the
exemption in the Pass entry (19.3 precedent) so it reads as reasoned.

Build the fixtures the corpus does not contain, run the **existing** paths
over them, fix what breaks:

- Synthetic fixtures via `tools/gen-form-fixtures.py` (+ `PROVENANCE.md`,
  LEGAL §5): (a) a terminal field with 3 kid widgets across 2 pages;
  (b) a 2-level non-terminal hierarchy producing `Personal.Address.Zip`;
  (c) a radio group with 3 kids and distinct on-states; (d) a node with
  **mixed** field-kids and widget-kids; (e) a static-XFA hybrid.
- Run `list-fields`, `fill-field`, `set-button-state`,
  `regenerate-appearances`, `flatten`, `export-data`/`import-data`, and
  `render-page` over every fixture.
- **Fixes owed** (all pre-existing, all made reachable by authoring):
  1. consolidate `fill_text_field`'s inlined regen loop onto
     `regen_field_appearance` (R92);
  2. mixed `/Kids` must not silently drop widget-kids;
  3. `remove_fields_from_form` must prune a parent left with empty `/Kids`,
     recursively;
  4. `Field.parent: Option<ObjId>` added and populated by `walk_field`.

**Acceptance:** every existing forms operation round-trips over all five
fixtures with no data loss; `list-fields` reports `Personal.Address.Zip`
correctly; flatten burns **all three** widgets of the multi-widget field
onto their correct pages; R85 preview-equals-saved oracle extended to cover
the multi-widget fill case; R46 unaffected (no writer path touched);
fuzz target 13 re-run over the new shapes.

### F1 — Text-field creation + the full collision branch (core + CLI) — **the P0 floor**

- `forms_author.rs`: `resolve_field_path`, `FieldPath`, `FormAuthorError`
  (`thiserror`, per rule 10 — never stringly-typed).
- `EditSession::add_field(&mut self, spec: &FieldSpec) -> Result<ObjId, EditError>`,
  following `add_text_annotation`'s recipe exactly: guards →
  `alloc_number` → `stage_bytes` → `ObjectWrite { before: None, .. }` →
  `annots_append` → one `commit` (one undoable command per created field).
- All four `FieldPath` outcomes live. **Shape A→B promotion** per §3.1.5,
  including the `/Annots` retarget.
- The creation floor per `forms__field_creation_minimums.md`: `/FT /Tx`,
  `/T`, `/Rect`, complete `/DA` (Helvetica, size 0 = auto, black),
  complete `/MK` (thin solid border, no fill), and a **baked `/AP`** via
  the shipped `build_field_text_appearance`. `/NeedAppearances` is
  **never set** — always-bake, per `forms__appearance_generation_and_needappearances.md`.
- `/DR` font-resource merge: reuse an existing `/Helv` if present; add if
  absent; **never rename or renumber existing resources** (§5.6).
- `/AcroForm` created (plus the catalog entry) if the document has none.
- `/TU` mandatory-or-declined (R105).
- Dotted-name path semantics + period-in-`/T` refusal (§3.1.4).
- `/Tabs /S` + untagged-field disclosure (§3.4.3); tagged-document
  disclosure (§3.5.3).
- Certification gate: refuse at **any** `/DocMDP` tier and on any
  `/FieldMDP` (§3.6.2) — stricter than fill's `/P >= 2`.
- `/XFA` present ⇒ refuse (§3.2.2).
- CLI: ~~`pdfce-cli forms add-field --type text …`~~ **[SUPERSEDED by §0,
  2026-08-07 — the flat verb `add-text-field` stands; SHIPPED under that
  name]**. Flags unchanged: `pdfce-cli add-text-field --name <fqn> --page N
  --rect x0,y0,x1,y1 (--tooltip <s>|--no-tooltip) [--value <s>]
  [--max-len N] [--multiline] [--required] [--readonly] [--font-size N]
  -o <out>`.

**Acceptance:**
- Round-trip: create → incremental save → reload → `list-fields` shows the
  field with the correct FQN → `render-page` paints its border and any
  initial value as **real glyph pixels through the Pass 6.0 read path**
  (the R44 proof shape).
- **Both refusals proven FIRING, not merely present** (R96): a dedicated
  test asserts `FieldTypeCollision` fires for checkbox-over-text, and a
  second asserts `NameIsGroupingNode` fires for a terminal request over
  `A` where `A.B` exists. A test that only exercises the happy path does
  not discharge this.
- **Merge proven**: two `add-field` calls with the same name and type
  produce **one** field with **two** widgets — verified by `list-fields`
  widget count, by `fill-field` setting both widgets' appearance, and by
  byte-inspecting that `/AcroForm/Fields` grew by exactly one entry.
- **Shape A→B promotion proven**: after the second call, the page's
  `/Annots` references the new widget object, **not** the field dict; the
  field dict has no `/Subtype`; both widgets carry `/Parent`.
- **R101 proven**: no authored widget kid carries `/T`, `/FT`, or `/Kids`
  (byte-grep test).
- **§7.2's JS-carrier test** (below) — mandatory, this slice.
- R85 oracle covers `add-field`.
- `/Fields` entry is an **indirect reference** (§1.2.2 trap).
- Every authored field dict is an indirect object (§12.7.3.1).

### F2 — Checkbox + radio creation, and field/widget deletion (core + CLI) — **✅ COMPLETE 2026-08-07 as Pass 20.2** (`bca60c9` check boxes; `69ab966`+`834d256`+`817b268` radio + deletion)

- The first **button appearance generator**: a keyed `/AP /N` sub-dictionary
  with one stream per named state, `/Off` always present, `/AS` set.
  Check glyph drawn as paths or via ZapfDingbats (a Standard-14 face
  already reachable through `basefont_to_std14`) — decide in-slice, cite
  the choice.
- Default on-state export value `Yes` (sourced default), overridable.
- Radio grouping **through the F1 merge primitive**, not a radio-specific
  path — repeated `add-field --type radio --name G --export-value V`.
  ~~**[§0, 2026-08-07: the flat radio verb name is NOT RULED. This line
  authors one radio MEMBER per call; `add-radio-group` would misdescribe
  it. See §0's radio caveat before picking a name.]**~~
  **[§0.1, 2026-08-07: NOW RULED — the verb is `add-radio-button`. The
  member-per-call reading above is unchanged and is what selected it.]**
- ~~`remove-field` / `remove-widget`~~ **`delete-field` / `delete-widget`
  [§0.1, 2026-08-07]** implementing §3.6.3's three rules.
- CLI: ~~`forms add-field --type checkbox|radio …`, `forms remove-field`,
  `forms remove-widget`~~ **[SUPERSEDED by §0, 2026-08-07 — flat verbs.]**
  Checkbox SHIPPED as `add-check-box [--export-value V]
  [--no-toggle-to-off]`. Radio: **`add-radio-button` (RULED by §0.1,
  2026-08-07)**; `[--radios-in-unison]` unaffected. Deletion:
  ~~`remove-field --name <fqn>`, `remove-widget --name <fqn> --index N` —
  **`remove`, not `delete`**, per §0's mapping table.~~
  **[SUPERSEDED by §0.1, 2026-08-07: `delete-field --name <fqn>`,
  `delete-widget --name <fqn> --index N` — `delete` is the house word
  (five shipped `*-delete`/`delete-*` verbs, zero `remove-*`), and forms
  is a verb-first domain.]**

**Acceptance:** a 3-member radio group authored by three CLI calls behaves
mutually-exclusively on `set-button-state` and renders correct on/off
states; deleting the **selected** member clears `/V` to `/Off`, sets every
remaining `/AS` to `/Off`, and **discloses the cleared selection**;
deleting the last member removes the parent field from `/AcroForm/Fields`;
a 3→1 deletion leaves Shape B intact (R102, byte-verified); `regenerate-appearances`
still skips buttons (unchanged `_ => continue`).

> **✅ F2's ACCEPTANCE IS MET IN FULL — 2026-08-07, Pass 20.2 COMPLETE.**
> Every clause above is built and verified **through the CLI and by
> RENDERING**, not only by a green suite: three `add-radio-button` calls →
> `fields=1 widgets=3 button=radio flags=0x8000` → the **unmodified**
> `fill-field` selects Green → three rings with the middle one filled;
> `delete-widget --index 0` on the **selected** member →
> `selection_cleared=1` with the prose disclosure, `value=Off widgets=2`,
> **rendered with the deleted ring gone and neither survivor filled** →
> `delete-field` → `fields=0`. **The rendered check carries the weight at
> the deletion step: `value=Off widgets=2` is also what two filled rings
> would report.**
>
> **Two additions this acceptance list did not name, both recorded as
> pdfce's own choices:** the check-glyph decision it left open in-slice was
> made in `bca60c9` (**vector-drawn**, because a ZapfDingbats route needs a
> second appearance generator and **R92 forbids it**), and the radio widget
> is **drawn round** — a pdfce design choice, not a parity claim, since
> §12.7.4.2 distinguishes radio from check box by `/Ff` bit 16 alone. **A
> group drawn as square boxes is a form that lies about its own
> behaviour.**
>
> **Deferred, not missed:** F2's `--defaults-from` is deferred to **F6**,
> where the property-editing family lives. **Positional-`/Opt` (§8.3) is
> DISCHARGED as a reasoned refusal** — see that limit's own ✅ block.

### F3 — Choice fields + push buttons (core + CLI)

- Listbox/combo: `/Opt` authoring (export↔display pairs), `/Ff` Combo /
  Edit / MultiSelect / Sort, `/I` and `/TI`.
- **Empty `/Opt` is ALLOWED and disclosed** — the research found no Acrobat
  blocking behavior, so pdfce picks one explicit rule rather than guessing:
  a zero-option choice field saves, and creation warns *"this choice field
  has no options and cannot be filled until options are added."*
- Push button: no `/V` ever written; an action-less push button is a valid
  inert placeholder. **Actions are recognized/round-tripped, never
  executed** — decision 009 posture A, unchanged. `add-field --type
  pushbutton` therefore authors **no** action in this slice; binding one is
  a separate question decision 009 already constrains.
- CLI: ~~`--type listbox|combobox|pushbutton …`~~ **[SUPERSEDED by §0,
  2026-08-07 — flat verbs.]** Listbox + combobox SHIPPED as the single verb
  `add-choice-field [--option export=display]... [--multi-select]
  [--editable]`. Push button: ~~verb name **NOT RULED** (the flat pattern
  implies `add-push-button`, but §0 does not rule it)~~ **[RULED
  2026-08-08, ARCHITECTURE.md §12 — `add-push-button`. `NewPushButton` /
  `EditSession::add_push_button` shipped in `baeb624`, decision 020 §6's
  F3 CLOSED.]**; `[--caption <s>]` unaffected.

### F4 — Tab order (core + CLI) — **BLOCKED on a spec-librarian dispatch**

**Hard prerequisite (§3.4.4):** dispatch `pdfce-spec-librarian` for Table 30
`/Tabs`, §14.7 structure-order derivation, and the ISO 32000-2 `/Tabs`
delta. Verified absent from the spec RAG this session. Implementing
row/column sort from memory violates project rule 1.

**[§0, 2026-08-07: `forms set-tab-order` → flat `set-tab-order`. Reads
cleanly flat; the block below is otherwise unchanged, and F4 stays
BLOCKED on the spec dispatch regardless of naming.]**

- `set-tab-order --mode structure|row|column --page N`
- `set-tab-order --order f1,f2,f3 --page N` (explicit; realized as
  `/Annots` position, which is also paint order — **disclose that**).
- R104 enforced: creation never writes `/Tabs`.

### F5 — GUI authoring surface (gui)

**Dispatch `pdfce-ui-specialist` before building** (rule: non-trivial UI).
R83: the type palette exposes **only** the types F1–F3 shipped — no
greyed-out signature or barcode entry, no placeholder. R84 for the
selected-tool state. `/TU` is a prompted field, not an optional one
(R105). The tagged-document and `/Tabs /S` disclosures surface as
non-modal strip text, matching the existing refusal-strip convention.

**R86 applies** (if in force by then): observed working in the running
application, against a purpose-built non-default fixture.

### F6 / F7 — fast-follows (not gating)

- **F6** — `--defaults-from <field>` (the research's session-template
  `should_have`), and ~~`forms rename-field`~~ → flat **`rename-field`**
  **[§0, 2026-08-07 — reads cleanly flat]** (needs `Field.parent` from F0
  for subtree renames).

  > **★★ AMENDED 2026-08-07 (`3d345aa`) — `rename-field` IS BUILT
  > (`Pass 20.6`, PARTIAL); ~~`--defaults-from` IS NOT. F6 is HALF DONE.~~**
  >
  > **★★ AMENDED AGAIN 2026-08-07 (`247b8fa`, thirty-first filing) — F6 IS
  > CLOSED. `--defaults-from` IS BUILT, on all four creation verbs
  > (`add-text-field`, `add-check-box`, `add-radio-button`,
  > `add-choice-field`), and `Pass 20.6` is COMPLETE.** *(Status note only —
  > this decision record's CONTENT is unchanged and nothing here is
  > re-decided.)* **What it copies:** `/MaxLen` (text), `/Opt` (choice), the
  > on-state from `widgets[0]` (check box). **A radio template copies
  > nothing** — on-states live per WIDGET while the flag names a FIELD.
  > **★ AND A CONSEQUENCE FOR THIS RECORD'S OWN CROSS-TYPE RULING:** with
  > every property shared across the four types being a **boolean** (all
  > excluded — the CLI's presence flags make *absent* and *explicitly false*
  > one token, so copying them could ADD a property but never turn one off)
  > and every copyable property being **type-specific**, **THERE IS NO COMMON
  > SUBSET.** *"Copy the common subset and disclose the drop"* therefore
  > reduces to ***"copy nothing and disclose it"*** — a template of a
  > different type contributes **literally nothing**. The behaviour is
  > coherent and shipped; **the DESCRIPTION was wrong, and only became
  > visibly wrong when someone tried to implement the sentence.** Full form:
  > `ROADMAP.md`'s `Pass 20.6` COMPLETION ADDENDUM §A.
  > **`/TU` is excluded and that exclusion is load-bearing** — R105 exists so
  > an accessibility name is never a silent default, and a copied tooltip
  > satisfies R105's mechanism while defeating its purpose (as does a copied
  > *declination*). **`/AA` is excluded because F3 rules that push-button
  > creation authors no action**, and copying it would author actions through
  > the back door.
  >
  > **THREE corrections to this bullet, all of them load-bearing:**
  >
  > **(a) This bullet is the WHOLE of F6, and other documents have been
  > reading four items into it.** `ROADMAP.md` (three places) and
  > `FEATURES.md` (one) wrote F6 as *"rename, reflag, move, resize"*.
  > **`move`, `resize` and `re-flag` appear NOWHERE in this decision
  > record** —
  > `git grep -n -iE "resize|reposition|move.*field|re-flag|change.*flag"`
  > over this file returns **16 hits, every one of them** the
  > `remove-*`→`delete-*` supersession, §3.3.2/§5's *"move the annotation
  > keys off the field dict"* promotion step, or a deletion rule. **They
  > are UNSCOPED, not deferred.** Closing F6 will therefore **not** close
  > the operator's *"creation/**editing**"* in the sense he is likeliest to
  > mean it, and that gap is invisible from the plan unless it is written
  > down. **Owed: an engineer scope call — a new slice, an amendment to
  > this record, or a named refusal.**
  >
  > **(b) *"needs `Field.parent` from F0 for subtree renames"* is TRUE but
  > its implied reason is WRONG.** A subtree rename is **ONE object
  > write**. §12.7.3.2 derives the FQN by **descending** from
  > `/AcroForm /Fields` and joining each node's `/T`, so rewriting one
  > node's `/T` re-derives that node's identity **and every descendant's**
  > without touching a single descendant object — they were never storing
  > the thing that changed. `Field.parent` is what lets the verb **COUNT**
  > the affected descendants so §3's rule-4 disclosure can report them
  > (`FieldRename::descendants_renamed`). **Required by the disclosure,
  > not by the mutation.** Verified: `Personal.Address` → `Location`
  > yields `Personal.Location.Zip` + `Personal.Location.City`,
  > `Personal.Name` untouched, `descendants_renamed=2`.
  >
  > **(c) The collision case this bullet never decided is now RULED:
  > a rename onto an occupied name REFUSES** — `FormAuthorError::RenameCollision`.
  > Not a merge, not an auto-suffix. **The asymmetry with `add-*` is the
  > argument:** a same-type add merges because the caller supplied a *type
  > and a name* and §12.7.3.2 makes same-FQN nodes one field, so merging
  > answers the request as stated; a rename supplied **two identities**, so
  > merging destroys one that was never offered up, with nothing in the
  > request saying how to reconcile two sets of `/V`, `/Ff` and `/Kids`.
  > Auto-suffixing is refused on rule 4 — `Name_2` is a name the operator
  > did not choose. **The branch is one `if`, so overturning is cheap.**
  > Full form: `ARCHITECTURE.md` §12's twenty-eighth-filing entry, and
  > `ROADMAP.md`'s `Pass 20.6 (PARTIAL)` entry §3.
  >
  > **Also added by that commit and not anticipated here:**
  > `FormAuthorError::DottedPartialName`, a refusal **separate from**
  > `PeriodInPartialName`. `A..B` is malformed; `A.B` is a **well-formed
  > path that simply is not a PARTIAL name**. Reusing the period error told
  > an `A.B` caller their input *"contains an empty name segment"*, which
  > it does not — sending them to hunt a typo that is not there.
- **F7** — `pdfce-cli merge --on-field-collision rename|link|refuse` with
  disclosure both ways, plus the cross-type-collision warning (§3.6.1).
  Against the already-shipped merge; deliberately outside this family.

---

## 7. Risks to the two load-bearing invariants

### 7.1 Round-trip / minimal-diff — four distinct exposures

**(a) Shape A→B promotion mutates an existing object — R94 hazard.**
Promotion rewrites a field dict that came from the file. R94: *"a repair
that mutates a value must invalidate any 'these-bytes-are-verbatim'
provenance attached to it."* If promotion changes the dict's value while a
`Provenance::File` verbatim fast path still applies, the writer emits the
**old** bytes — a document whose bytes contradict its own corrected value,
self-inflicted by the fix. The `EditSession` overlay
(`state: BTreeMap<ObjId, Object>`) should handle this correctly, but
"should" is R93's word. **Owed: an explicit test that a promoted field dict
serializes its post-promotion value in both save modes.**

**(b) `/AcroForm/Fields` append — see §7.2. The largest exposure.**

**(c) `/DR` font-resource merge.** If `/DR/Font` lacks the face the new
`/DA` names, pdfce must add it — mutating the AcroForm dict again, and
adding a resource. Rules: reuse an existing name if the face matches; add
under a fresh name if not; **never rename, renumber, or reorder existing
`/DR` entries** (§5.6 never-normalize). A renamed `/DR` entry silently
breaks the `/DA` of every pre-existing field that referenced it — a
document-wide appearance regression caused by adding one field.

**(d) Page `/Annots` append.** Lowest risk: `annots_append` already exists,
handles absent/inline/shared-indirect array shapes, and is R46-proven
through Pass 6.1's markup authoring. **Reuse it; do not write a second
one.** Writing a parallel append path would be R92's shape.

### 7.2 The decision-009 byte-verbatim guarantee — a shipped property that stops holding silently

Restating §1.2.6 because it is the sharpest item in this document.

Pass 7.0 guarantees that JS carriers (`/CO`, `/AA`, `/Names /JavaScript`)
re-emit byte-verbatim, and it holds **structurally** — because fill never
writes the `/AcroForm` dict. Field creation must write `/AcroForm/Fields`.
The guarantee therefore stops holding, and because it was structural rather
than asserted, **no existing test will go red.** This is R93's exact
failure shape, caught prospectively.

**Mitigation, mandatory in F1:**

1. Re-emit the `/AcroForm` dictionary with **only** the `/Fields` array
   (and `/DR`, if F1's font merge required it) changed — every other key
   byte-preserved.
2. A test that authors a field into
   `fixtures/synthetic/forms/demo-form.pdf` extended with `/CO`, an
   `/AA` hook, and a `/Names /JavaScript` tree, saves, and **byte-greps**
   that all three carriers are unchanged.
3. The `/AcroForm`-absent case (create the dict + the catalog entry) gets
   its own test — that path touches the **document catalog**, which is
   about as load-bearing as an object gets.
4. Record in `ARCHITECTURE.md` §12 that decision 009's structural guarantee
   is now **test-enforced for the authoring path** rather than structural,
   so a future reader does not inherit the stronger claim.

### 7.3 GUI-core separation — low risk, one named trap

The trap is coordinates. A canvas-placed field wants its rect from a mouse
drag, and it is tempting to pass an egui type into core. **Binding: the
core API takes a PDF-user-space `Rect` (four `f64`) and a page index;
every canvas↔user-space transform stays in `pdfce-gui`.** Appearance
generation, font resolution, and operator-supplied fonts (decision 012) are
already core-side. Verify with `cargo tree -p pdfce-core -p pdfce-render`
on F1 and F5 (rule 2), since F1 touches `pdfce-core`'s `Cargo.toml` only if
a dependency were added — and none is.

---

## 8. Honest limits (named up front)

1. **Auto-size (`/DA` size 0) at creation is only as good as the shipped
   generator.** `vartext.rs` implements the size-0 rule, but the research
   records a version-specific Acrobat quirk where auto-size fails to persist
   across save/reopen. F1 owes a specific regression test for
   auto-size persistence; pdfce's behavior is pdfce's own, not a parity
   claim.
2. **Comb fields.** `/MaxLen` is modelled but the comb layout is not driven
   from it in the shipped fill path. `add-field --comb` would therefore
   author a field whose appearance does not comb. **Either implement comb
   layout in F1 or refuse `--comb` by name.** Do not ship a flag that sets
   `/Ff` bit 25 and produces an uncombed appearance — that is an affordance
   without a capability (R83).
3. **`/Opt` for button fields (Table 227)** is parsed into `Field.options`
   today but never consulted on the write side, and positional on-state
   names (`/1`, `/2`, …) are not mapped to export values anywhere. F2 must
   either implement or explicitly refuse positional-`/Opt` radio authoring;
   it must not author a group whose export values pdfce cannot itself
   resolve.
   > **✅ DISCHARGED 2026-08-07 — `69ab966` (Pass 20.2 COMPLETE). THE
   > REFUSAL BRANCH WAS TAKEN, and this item is CLOSED, not outstanding.**
   > `add_radio_button` **refuses to extend a positional-`/Opt` group by
   > name**, for the reason this limit states: Table 227's `/Opt` makes the
   > `/AP /N` keys array **indices**, and pdfce parses `/Opt` but has never
   > consulted it on the write side — so it can compute neither what index
   > a new member would take nor what the existing members export. **The
   > refusal is unreachable on pdfce-authored groups**, which are always
   > named. **File this as a CHOSEN REFUSAL, not an omission or a gap** —
   > §8.3 names refusing as an equal outcome to implementing, and the
   > alternative it actually forbids (authoring a group whose export values
   > pdfce cannot itself resolve) never became reachable.
   > **Two adjacent refusals shipped with it, both reasoned:** duplicate
   > export values are refused **unless the group is `RadiosInUnison`**
   > (members are told apart by on-state name alone, §12.7.4.2.1, so two
   > sharing one make `/V` unable to say which was chosen — which is
   > precisely what `/Ff` bit 26 requests deliberately, so the refusal
   > names the flag); and a check box may not join a radio group or the
   > reverse (both are `/FT /Btn`, so the **KIND** comparison F1 already
   > had is what catches it — no new mechanism).
4. **Inherited-`/V` writes remain terminal-only** until `Field.parent`
   (F0) is actually *used* by the setters — F0 adds the field, it does not
   rewire the three setters. A group whose `/V` is declared on a
   grandparent will still be written at the terminal. Named, not fixed
   here; fixing it is F6-adjacent and needs its own round-trip evidence.
5. **No non-terminal field can be created empty.** `resolve_field_path`'s
   `Vacant` branch creates intermediate parents only as a side effect of
   creating a terminal beneath them. There is no `add-group` operation and
   none is proposed — a childless grouping node has no purpose.
6. **The `/Tabs` sort algorithms are not designed here** — F4 is
   deliberately blocked on spec sourcing (§3.4.4), so this decision states
   the *rule* (R104) and not the *implementation*.
7. **Barcode and signature field creation are absent**, by decision
   (§3.3.4). Both are genuine Acrobat capabilities. This is a parity
   subtraction, stated as one.

---

## 9. Where pdfce exceeds Acrobat (this family specifically)

Each is sourced against a documented Acrobat behavior, not asserted:

1. **`/TU` is mandatory-or-declined** (R105). Acrobat makes it trivially
   skippable and its own accessibility checker then flags the result.
2. **Tagged-document disclosure.** Acrobat leaves new fields untagged and
   says nothing; its own remediation guidance treats tagging as a separate
   manual step. pdfce says so at creation (§3.5.3).
3. **`/Tabs /S` + untagged-field disclosure** (§3.4.3) — no evidence
   Acrobat surfaces this at all.
4. **Cross-type collision checking on merge** (F7). *"Acrobat's own merge
   path does not appear to check this."*
5. **Merge-vs-rename is an operator choice with disclosure both ways**
   (§3.6.1), rather than a silent default that destroys legitimate field
   linking.
6. **Always-bake `/AP`, never `/NeedAppearances`** — vendor-consensus best
   practice, adopted as pdfce's own contract rather than chasing Acrobat's
   version-inconsistent flag handling.
7. **A first-class scriptable CLI for form authoring at all** (rule 11).
   Acrobat has no equivalent.

---

## 10. For Ken personally — do not decide these solo

### 10.1 Should item #4 start now at all? (the real question)

Item #3 ("finish off all the text handling stuff") is **partially** done:
FF-H is complete end-to-end, but **FF-C** (font subsetting/embedding) and
**FF-B** (cross-block/cross-page reflow) remain unscheduled — and decision
019's own build order is FF-H → FF-C → FF-B. `ROADMAP.md` states plainly:
*"do not treat 'text-handling' as closed until those two ship or are
explicitly deferred by the operator."*

Starting a new feature family while item #3 is open is a **resequencing**,
and the engineer has already flagged one such judgment call this
session (open question (l), redaction-apply ahead of form-building). Two
undirected resequencings in a row is where a priority list stops meaning
anything.

**Ask Ken:** (a) FF-C and FF-B — ship, or explicitly defer? (b) Does the
"if that makes sense" hedge survive contact with this plan — is a
six-slice authoring family what he wanted, or something smaller?

### 10.2 A competing claim on the same slot — RESOLVED, NOT OPEN

**Corrected by the engineer, 2026-08-03.** This section was written against
the tree as it stood when the survey began, and it is now out of date.

It said the GUI had *no redaction-apply flow at all* — that the app warned
*"⚠ N UNAPPLIED redaction mark(s) — this document is NOT redacted"* and
then offered no in-app way to fix it. That was true, and it was the right
thing to raise. It stopped being true at commit `9a68999` (Pass 8.1),
which shipped the apply flow: a `DockPanel::Redact` review pane, a
confirmation carrying a measured report, and a runtime absence proof that
refuses to write if the removed text survives anywhere in the finished
bytes.

So the competing claim is **discharged, not overruled**. The half-shipped
security feature is whole, which is precisely why this slot is free to be
argued about at all. Nothing else in this decision depends on the
correction — §10.2 was the only place the stale fact was load-bearing —
but it is fixed in place rather than deleted, because "the agent that
scoped this believed redaction was unfinished" is context worth keeping
when re-reading the sequencing argument in §10.1.

What remains genuinely open from the redaction family is narrower and is
already filed: canvas drag-marking (ui-spec §2.2/§2.6) and Sanitize (§6).
Neither is a security hole; both are named follow-ups.

### 10.3 Signature-field creation — deferred, but the use case is real

§3.3.4 defers it to Pass 10 because pdfce cannot sign. But "add a signature
field so **someone else** can sign this" is a completely legitimate
workflow that does not require pdfce to sign anything. If Ken wants it, it
is a small slice (an empty `/FT /Sig` widget) and it belongs in F3.
**Ask.**

### 10.4 Barcode fields — a parity subtraction, confirm it is acceptable

§3.3.4 cuts them, well-grounded (no sourcing on the creation floor;
JS-driven population that decision 009 permanently forbids). But they are a
real Acrobat Pro feature and this is a deliberate subtraction from
feature-for-feature parity. **Confirm.**

### 10.5 The standing XFA open item

This decision makes it non-blocking (§3.2.3). Recommend re-scoping it to
"before any XFA read/fill work" rather than leaving it as a general
standing item. **Ken's to retire or re-scope — not mine.**

**Not for Ken** (decided here, on the merits): the data model, static-XFA
refusal, slice order, tab-order rule, `/TU` vs. structure tree, and all
four Q6 items. None of them needs an operator answer to be correct.

---

## 11. What this decision explicitly does NOT decide

- The `/Tabs` row/column/structure **sort algorithms** — spec-sourced in
  F4, deliberately not designed here.
- **Field auto-detection** ("Prepare Form") — separate Pass, unchanged.
- **Whether to bind actions to push buttons** — decision 009 constrains it;
  F3 authors none.
- **The GUI's placement interaction design** — `pdfce-ui-specialist`'s, at
  F5.
- **FF-I's scope or timing** — §3.5.4 records a data point, nothing more.
- ~~**Whether `pdfce-cli`'s existing forms subcommands should move under a
  `forms` parent** (`list-fields` → `forms list`). New authoring commands
  are proposed as `forms <verb>`; whether to migrate the six shipped ones
  is a CLI-surface question for the librarian/engineer, not this decision.~~
  **CLOSED 2026-08-07 by §0 — the answer is NO.** The engineer ruling that
  superseded the nested authoring verbs settles this half too: no `forms`
  parent is created, so the six shipped forms subcommands stay flat and
  top-level with nothing to migrate toward. This bullet is struck rather
  than deleted because "this decision deliberately left the CLI surface
  open" is context worth keeping — it is *why* the divergence went four
  flaggings without being settled. `ROADMAP.md`'s open-operator-question
  **(q)** is the same question and is closed with it.
- **`Field.parent`'s use by the three setters** — added in F0, wired later
  (§8.4).

---

## 12. References

**pdfce**
- `docs/ARCHITECTURE.md` §5.1 (three save contracts), §5.6 (never
  normalize), §5.7 (mutation writer/promotion), §11.1 (command log),
  §12 (decision log)
- `docs/ROADMAP.md` — Pass 7.0/7.1 Shipped entries; Forms (AcroForm)
  Backlog bucket (amended 2026-08-01); FF-I Backlog entry; ★★★ Operator
  priority sequence; standing rules R32, R34, R35, R44, R46, R49, R73,
  R83, R84, R85, R86, R87, R92, R93, R94, R96
- `docs/decisions/009-forms-javascript-posture.md` — posture A, the
  byte-verbatim JS-carrier guarantee (§7.2)
- `docs/decisions/018-edited-state-is-what-the-canvas-renders.md` — R85/R86
- `docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md` — §3.7 (the
  FF-I cut), §4.3 (correctness slice first), Amendment F (R96's origin)
- `docs/ui_specs/pass-7-form-fill.md`
- Code audited this session (worktree
  `.claude/worktrees/agent-a90dfe853c88ca161`):
  `crates/pdfce-core/src/forms.rs`, `edit.rs`, `fdf.rs`,
  `annot_author.rs`, `vartext.rs`, `writer/save.rs`,
  `crates/pdfce-cli/src/main.rs`

**Spec RAG** (`D:\Dev\Rag-Specialized\PDF_Spec\`)
- `iso32000/iso32000__s__12.7.3.md` — §12.7.3.1 hierarchy/inheritance,
  §12.7.3.2 FQN construction, Table 220 (incl. the `/Kids` merge switch),
  Table 221
- `iso32000/iso32000__s__12.7.2.md` — `/AcroForm` document level
- `iso32000/iso32000__s__12.7.3.3.md` — variable-text appearance generation
- `iso32000/iso32000__s__12.7.4.md` — per-type Btn/Tx/Ch/Sig + flag tables
- `iso32000/iso32000__s__12.5.2.md` — annotation common entries; `/Tabs`
  R/C/S (the only `/Tabs` coverage in the corpus — see §3.4.4)
- **Verified absent, gating F4:** Table 30's `/Tabs` row in
  `iso32000__s__7.7.3.md`; any `/Tabs` content in
  `iso32000__delta__pdf20_pass1.md`

**Acrobat parity RAG** (`D:\Dev\Rag-Specialized\Acrobat_Features\`)
- `forms__field_naming_hierarchy_and_collisions.md` (2026-08-03) — the
  merge/refuse branch; the Combine-Files tension
- `forms__field_creation_minimums.md` (2026-08-03) — the per-type floor
- `forms__radio_group_authoring.md` (2026-08-03) — implicit grouping; the
  two deletion GAPs
- `forms__tab_order.md` (2026-08-03) — four modes; the insertion GAP
- `forms__authoring_limits_and_refusals.md` (2026-08-03) — certification,
  encryption tension, XFA, untagged-by-default
- `forms__appearance_generation_and_needappearances.md` — always-bake;
  2026-08-03 creation-time addendum
- `forms__field_property_model.md` — `/TU` as the accessible name (WebAIM)
- `forms__button_fields.md`, `forms__field_auto_detection.md`,
  `core_ops__merge_combine_files.md`

---

## Appendix A — JSON decision block (drives implementation)

```json
{
  "decision_id": "020",
  "slug": "form-field-authoring",
  "title": "Form field AUTHORING: identity model, XFA scope, slice order, tab order, tagging, and four research conflicts",
  "date": "2026-08-03",
  "status": "decided",
  "confidence": "high",
  "roadmap_bucket": "Forms (AcroForm)",
  "proposed_pass_family": "Pass 20.x",
  "pass_family_verified_free": true,

  "q1_data_model": {
    "decision": "graph-resolver-on-write, flat-projection-on-read",
    "research_recommendation": "accepted in substance, replaced in mechanism",
    "binding_rule": "Field identity is the fully-qualified name; the FQN is derived from the object graph, not stored; therefore every authoring write resolves the name against the graph before writing, and must be able to attach a widget to an existing node without creating a second node.",
    "read_model_changes": "none in shape; AcroForm.fields stays Vec<Field>",
    "additive_only": ["Field.parent: Option<ObjId>"],
    "new_module": "crates/pdfce-core/src/forms_author.rs",
    "resolver": {
      "fn": "resolve_field_path(graph, fqn) -> Result<FieldPath, FormAuthorError>",
      "variants": ["Vacant{deepest,remaining}", "Terminal{id,ft,kind,shape}", "Grouping{id}"],
      "shape": ["MergedSingleWidget", "KidsWidgets{n}"]
    },
    "collision_branch": [
      {"path": "Vacant", "outcome": "create terminal + widget + any intermediate non-terminal parents"},
      {"path": "Terminal", "type_match": true, "outcome": "merge: attach widget; ShapeA->B promotion if merged"},
      {"path": "Terminal", "type_match": false, "outcome": "refuse FieldTypeCollision"},
      {"path": "Grouping", "outcome": "refuse NameIsGroupingNode"}
    ],
    "third_branch_is_new": "Grouping — not in Acrobat, not in the research; arises because pdfce authors dotted hierarchies and a non-terminal field has no type (Table 220)",
    "shape_a_to_b_promotion": {
      "mandatory": true,
      "spec_basis": "Table 220 — Kids shall be omitted ONLY when a single widget is merged; two widgets require separate objects",
      "steps": ["allocate widget object", "move annotation keys off field dict", "strip annotation keys from field dict", "write /Kids + /Parent", "RETARGET page /Annots to widget1", "append widget2 via annots_append, set /P"],
      "easiest_to_forget": "the /Annots retarget in step 5"
    },
    "never_collapse_b_to_a": true,
    "dotted_names": "always path separators; a partial name containing PERIOD is refused (PeriodInPartialName); no escape hatch",
    "cost_if_flat_shipped_first": "not a refactor cost — corrupt output immediately: two same-FQN sibling fields have no disambiguator under 12.7.3.2, pdfce's own reader treats that shape as damage (fields_named fan-out), and no migration exists because no fact exists to migrate to"
  },

  "q2_xfa": {
    "dynamic_xfa": "out_of_scope",
    "dynamic_reasons": ["no AcroForm as of Acrobat 8.1+ so no parity target", "structural editing needs LiveCycle/AEM, outside Acrobat", "0.08% of organic corpus (decision-008 census)", "pdfce already ships XfaPresence detect-only"],
    "static_xfa_hybrid": "REFUSE field creation by name — decided here, not deferred",
    "static_reason": "pdfce can write the AcroForm half but not the XFA half; a one-sided add makes an XFA-aware viewer show N fields and a non-XFA viewer N+1 — a silent divergence between what two viewers show, which fuzzy-never-sneaky forbids",
    "scope_of_refusal": "creation only; fill/flatten/read unaffected",
    "gate_reachable": true,
    "gate_test_required": "static-XFA-hybrid synthetic fixture; assert the gate FIRES (R96)",
    "named_remedy": "a separate 'forms strip-xfa' demotion operation — explicitly NOT in this family",
    "standing_open_item": "does not gate this family; recommend Ken re-scope it to 'before any XFA read/fill work'",
    "escalate_to_ken": false
  },

  "q3_slices": {
    "build_order": ["F0", "F1", "F2", "F3", "F4", "F5"],
    "fast_follows": ["F6", "F7"],
    "p0_floor": "text field creation THROUGH the resolver with all four FieldPath outcomes live — the collision branch IS the floor, not a follow-on",
    "why_not_text_only": "a lone text field never exercises the merge branch, which is the entire subject of Q1; deferring it ships the flat-write model for one slice, and even one slice authors documents that cannot be un-authored",
    "why_not_checkbox_in_p0": "text reuses the shipped 12.7.3.3 generator with zero new appearance machinery; checkbox needs a keyed /AP /N state sub-dictionary + check glyph — the first button appearance GENERATOR in the codebase (regenerate_appearances today does `_ => continue` for buttons)",
    "F0": {"name": "field-hierarchy correctness + authoring substrate", "surface": "core only", "rule_11_exempt": true, "exempt_precedent": "Pass 19.0", "fixes": ["consolidate fill_text_field's inlined regen loop (R92)", "mixed /Kids must not drop widget-kids", "remove_fields_from_form must prune empty-/Kids parents recursively", "add Field.parent"], "fixtures": ["3-kid widget field across 2 pages", "2-level hierarchy Personal.Address.Zip", "3-member radio group", "mixed field-kids + widget-kids node", "static-XFA hybrid"]},
    "F1": {"name": "text-field creation + full collision branch", "surface": "core + CLI", "cli": "pdfce-cli add-text-field --name <fqn> --page N --rect x0,y0,x1,y1 (--tooltip <s>|--no-tooltip) [--value] [--max-len] [--multiline] [--required] [--readonly] [--font-size] -o <out>", "cli_amended_2026_08_07": "verb was specified as 'forms add-field --type text'; SUPERSEDED by section 0 — flat verb 'add-text-field' stands, flags unchanged", "cli_superseded_original": "pdfce-cli forms add-field --type text --name <fqn> --page N --rect x0,y0,x1,y1 (--tooltip <s>|--no-tooltip) [--value] [--max-len] [--multiline] [--required] [--readonly] [--font-size] -o <out>"},
    "F2": {"name": "checkbox + radio creation, field/widget deletion", "surface": "core + CLI"},
    "F3": {"name": "choice fields + push buttons", "surface": "core + CLI"},
    "F4": {"name": "tab order", "surface": "core + CLI", "blocked_on": "pdfce-spec-librarian dispatch for Table 30 /Tabs, 14.7 structure-order derivation, ISO 32000-2 /Tabs delta — VERIFIED ABSENT from the spec RAG this session"},
    "F5": {"name": "GUI authoring surface", "surface": "gui", "requires": "pdfce-ui-specialist dispatch first"},
    "cut": {
      "signature_field_creation": "deferred to Pass 10 (Signatures) — placing a field pdfce cannot sign is an affordance without a capability; ASK KEN, the 'someone else signs' use case is real",
      "barcode_fields": "out of scope — no sourcing on the creation floor, and population depends on JavaScript that decision 009 permanently forbids executing; a genuine parity subtraction, flagged to Ken",
      "auto_detection": "unchanged, separate later Pass",
      "session_default_template": "F6 fast-follow"
    }
  },

  "q4_tab_order": {
    "rule": "/Tabs is a MODE, not a snapshot",
    "under_computed_modes": "pdfce reorders NOTHING on insertion — there is no stored sequence to maintain",
    "under_manual_order": "append to end of page /Annots, and DISCLOSE that the new field is last",
    "never": ["write /Tabs as a side effect of field creation", "re-sort /Annots to realize a computed order"],
    "why_research_mechanism_corrected": "'re-sorted automatically' invites an implementation that sorts /Annots on every insertion — which rewrites references pdfce did not logically touch (minimal-diff violation) AND changes annotation paint order, a visible change caused by a non-visible feature",
    "missed_case": "/Tabs /S + an untagged new field ⇒ the field has NO defined tab position at all (not 'last' — undefined), because structure order derives from the tag tree and pdfce-authored fields are untagged",
    "missed_case_acceptance": "detect and disclose in F1, not deferred to F4",
    "spec_gap_verified": true
  },

  "q5_tagging": {
    "verdict": "split — not a re-opening of FF-I",
    "test_applied": "does the proposal build a partial structure-tree writer? /StructElem does; /TU does not",
    "in_scope_p0": "/TU mandatory-or-explicitly-declined on every authored field",
    "in_scope_reason": "for FORM FIELDS the accessible name assistive tech reads is /TU, read through the interactive-field layer, BYPASSING the tag tree (WebAIM, sourced) — one optional string on an object pdfce creates from nothing; nothing to drift out of sync with",
    "out_of_scope": "/StructElem, /ParentTree, /ParentTreeNextKey — stays with FF-I, cut rationale unchanged",
    "ships_instead": "on a /StructTreeRoot document, disclose 'this document is tagged; the new field is NOT in the structure tree' — the existing R73 posture, already strictly better than Acrobat's silence",
    "ff_i_note": "FF-I now has a second consumer; a data point to record, NOT a reason to pull it forward",
    "codebase_fact": "no /StructElem, /ParentTree or /K traversal exists anywhere in pdfce-core — /StructTreeRoot is only ever a boolean or a disclosure string"
  },

  "q6_conflicts": {
    "combine_files": {
      "verdict": "not a contradiction — a default plus its own documented fallback",
      "evidence": "core_ops__merge_combine_files.md itself states linking occurs 'if this is disabled or fails'; the second source describes that fallback branch",
      "pdfce_posture": "make it an operator choice with disclosure both ways",
      "cli": "pdfce-cli merge --on-field-collision rename|link|refuse (default rename, preserving shipped behavior)",
      "plus": "warn or refuse on cross-type collisions — an exceed; Acrobat's merge path does not appear to check type",
      "filed_as": "F7 fast-follow against already-shipped merge",
      "blocks": false
    },
    "encrypted_documents": {
      "layer_1": "the Acrobat UI-ordering conflict is structurally inapplicable — pdfce has no modal tool entry points",
      "layer_2": "the /P bit-6 gate the research calls must_have would be DEAD CODE TODAY — /Encrypt documents are refused outright by every authoring path in EditSession before any forms code runs (R96, verified in the code survey)",
      "decision": "do NOT build a bit-6 gate in this family; record it in the module doc as owed, and add it to Pass 5 (Encryption) where it becomes reachable and provable",
      "what_is_built_instead": "the DocMDP certification gate, which IS reachable — and is STRICTER than fill's: creation refuses at ANY /DocMDP tier and on any /FieldMDP (fill permits /P >= 2)",
      "also_inherited_free": "suppressed_object_count() > 0 -> ObjectCreationWouldExposeHiddenObjects",
      "blocks": false
    },
    "radio_deletion_gaps": {
      "verdict": "acceptable as neither a blocker nor a deferred acceptance criterion — they were never acceptance criteria",
      "reframing": "the research itself recommends pdfce adopt the spec-consistent behavior and disclose it as pdfce's own design choice; once it does, the Acrobat answer is not load-bearing and pdfce's criterion ('pdfce's documented rule holds, proven by test') needs no Acrobat install",
      "mid_group_rule": "remove the widget from /Kids and its page /Annots; IF the removed widget's on-state equals the field's /V, set /V /Off and every remaining kid's /AS to /Off, and DISCLOSE that the selection was cleared — a dangling /V pointing at a state no widget can display is a malformed field the research does not name",
      "last_member_rule": "also remove the now-empty parent field from /AcroForm/Fields, recursively up through childless grouping nodes",
      "one_rule_not_a_radio_special_case": true,
      "requires": "the empty-parent prune fix in F0",
      "blocks": false
    }
  },

  "standing_rules_proposed": [
    {"id": "R100", "text": "Field identity is the fully-qualified name, and every authoring write resolves that name against the object graph before it writes."},
    {"id": "R101", "text": "A widget kid carries no field keys — no /T, /FT, /Kids — because pdfce's own kid_is_field promotes any such kid to a separate terminal field, destroying group semantics."},
    {"id": "R102", "text": "pdfce never normalizes field shape: Shape A->B promotion only when a second widget makes the merged form illegal; Shape B never collapses back to A."},
    {"id": "R103", "text": "A guard whose precondition is already refused by a coarser earlier gate is not built; the refinement is recorded as owed to the Pass that removes the coarser gate. (Prospective form of R96.)"},
    {"id": "R104", "text": "/Tabs is a mode, not a snapshot — pdfce reorders nothing under S/R/C, appends and discloses under an explicit order, and never writes /Tabs as a side effect of field creation."},
    {"id": "R105", "text": "Every field pdfce authors carries /TU, or an explicitly recorded operator declination. Omitting both --tooltip and --no-tooltip is an error, never a silent default."}
  ],
  "standing_rule_ceiling_at_time_of_writing": "R96",
  "renumbered_by_librarian": {
    "when": "2026-08-03, continuation 71",
    "reason": "collision — continuation 70 concurrently claimed R97-R99 for three unrelated Pass 8.1 findings before this decision's R97-R102 was filed; the real ceiling immediately before this decision was R99, not the R96 this document was drafted against",
    "mapping": {"R97": "R100", "R98": "R101", "R99": "R102", "R100": "R103", "R101": "R104", "R102": "R105"},
    "note": "all prose + JSON occurrences in this file updated in place to the right-hand column; standing_rule_ceiling_at_time_of_writing above is left as the honest historical value, not corrected"
  },

  "invariant_risks": {
    "round_trip": [
      {"id": "a", "risk": "Shape A->B promotion mutates a Provenance::File object — R94 hazard (bytes contradicting the corrected value)", "mitigation": "explicit test that a promoted field dict serializes its post-promotion value in BOTH save modes"},
      {"id": "b", "risk": "SHARPEST — decision 009's byte-verbatim JS-carrier guarantee held STRUCTURALLY (fill never touches the /AcroForm dict); field creation MUST write /AcroForm/Fields, so it stops holding and NO EXISTING TEST GOES RED", "mitigation": "re-emit /AcroForm with only /Fields (and /DR if merged) changed; byte-grep test over /CO, /AA, /Names /JavaScript; separate test for the /AcroForm-absent case (touches the catalog); record in ARCHITECTURE.md §12 that the guarantee is now test-enforced, not structural"},
      {"id": "c", "risk": "/DR font-resource merge renaming or renumbering existing entries would silently break the /DA of every pre-existing field referencing them", "mitigation": "reuse matching face; add under a fresh name; never rename/renumber/reorder (§5.6)"},
      {"id": "d", "risk": "a second /Annots append path", "mitigation": "reuse the existing R46-proven annots_append; writing a parallel one is R92's shape"}
    ],
    "gui_core_separation": {"risk": "canvas placement passing an egui type into core", "rule": "core takes a PDF-user-space Rect (four f64) + page index; all transforms stay in pdfce-gui", "verify": "cargo tree -p pdfce-core -p pdfce-render on F1 and F5"}
  },

  "reachable_refusals_that_must_be_tested_as_FIRING": [
    "FieldTypeCollision (checkbox over an existing text field)",
    "NameIsGroupingNode (terminal request over 'A' where 'A.B' exists)",
    "PeriodInPartialName",
    "XFA present (static hybrid fixture)",
    "certification: any /DocMDP tier, any /FieldMDP",
    "ObjectCreationWouldExposeHiddenObjects (inherited)"
  ],
  "refusal_deliberately_NOT_built": {"what": "/P bit-6 permission gate", "why": "unreachable behind the unconditional /Encrypt authoring refusal — R96", "owed_to": "Pass 5 (Encryption)"},

  "honest_limits": [
    "auto-size persistence across save/reopen is pdfce's own behavior, not a parity claim — owes a regression test",
    "comb layout is not driven from /MaxLen in the shipped fill path — either implement in F1 or refuse --comb by name (R83)",
    "button /Opt (Table 227) parsed but never consulted on write; positional on-state names unmapped — F2 must implement or refuse",
    "inherited-/V writes remain terminal-only; Field.parent is added in F0 but not wired into the three setters",
    "no add-group operation — non-terminal nodes are created only as a side effect of creating a terminal beneath them",
    "the /Tabs sort algorithms are not designed here (F4 is spec-blocked)",
    "barcode and signature field creation absent by decision — a stated parity subtraction"
  ],

  "escalate_to_ken": [
    {"id": "10.1", "priority": "highest", "question": "Should item #4 start now at all? Item #3 is only PARTIALLY done — FF-C and FF-B are unscheduled and ROADMAP says do not treat text-handling as closed until they ship or are explicitly deferred. Starting a new family is a resequencing, and it would be the second undirected one this session (after redaction-apply, open question (l)). Also: does the 'if that makes sense' hedge survive a six-slice plan?"},
    {"id": "10.2", "priority": "resolved", "question": "SUPERSEDED 2026-08-03 by the engineer: this said the GUI had no redaction-apply flow. Pass 8.1 (commit 9a68999) shipped it — review pane, measured-report confirmation, and a runtime absence proof that refuses to write if the removed text survives. The competing claim is discharged, not overruled. Remaining redaction work is canvas drag-marking and Sanitize, both filed follow-ups, neither a security hole."},
    {"id": "10.3", "priority": "medium", "question": "Signature-field creation is deferred to Pass 10 because pdfce cannot sign — but 'add a signature field for SOMEONE ELSE to sign' is a legitimate workflow that needs no signing. Want it? It is a small addition to F3."},
    {"id": "10.4", "priority": "medium", "question": "Barcode field creation is cut (no sourcing; JS-driven population that decision 009 permanently forbids). This is a real parity subtraction — confirm acceptable."},
    {"id": "10.5", "priority": "low", "question": "The standing 'verify XFA deprecation status' open item is made non-blocking by this decision. Retire it, or re-scope it to 'before any XFA read/fill work'?"}
  ],
  "not_for_ken": ["the data model", "static-XFA refusal", "slice order", "the tab-order rule", "/TU vs structure tree", "all four Q6 items"],

  "dispatches_required": [
    {"agent": "pdfce-spec-librarian", "before": "F4", "topic": "Table 30 /Tabs entry; §14.7 structure-order derivation; ISO 32000-2 /Tabs delta values — verified absent from the spec RAG"},
    {"agent": "pdfce-ui-specialist", "before": "F5", "topic": "field placement interaction, type palette (R83), /TU prompting (R105), disclosure strips"},
    {"agent": "pdfce-librarian", "on": "acceptance of this decision", "topic": "file Pass 20.x entries under Next up; add R100-R105; add the ARCHITECTURE.md §12 dated entry cross-referencing this record"}
  ],

  "new_cargo_dependencies": 0
}
```
