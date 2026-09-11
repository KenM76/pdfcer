# pdfcer — Architecture

This document is the logic. The Rust code is the syntax that enacts it.
Per the user's standing global rule: a competent engineer (human or LLM)
should be able to reconstruct pdfcer's design from this file (plus
`ROADMAP.md`, `LEGAL.md`, the PDF-spec RAG, and the Acrobat feature-
parity RAG) without reading a line of code.

## 1. Project goal (verbatim framing from the founding conversation, 2026-07-23)

An open-source, non-monetized, full-feature-for-feature replacement for
**Adobe Acrobat Pro**. The initial application is a native desktop GUI
that does **not** rely on running a web server, a browser runtime, or
any local network listener — everything happens in one native process.
It must run from a single folder, including all of its dependencies
(no installer, no registry writes, no system-wide runtime dependency).

pdfcer also ships **CLI capabilities** from the start (`pdfcer`, see
§3 and §7) — batch/scriptable operations (merge, split, stamp, convert,
sign, validate) invokable without opening the GUI at all. This is
addressed by the user (2026-07-23) as an explicit project requirement,
not just a developer convenience: Acrobat Pro itself has no equivalent
first-class CLI (only in-GUI Action Wizard batch sequences), so a real
CLI is a genuine parity-plus feature for anyone scripting document
workflows.

A **later fork** (not this codebase's job yet, but a design constraint
on this codebase **today**) will turn the same core logic into a web
application. Every architectural decision below is chosen to keep that
fork cheap when the time comes, without over-building for it now.

**Competitive/prior-art landscape confirmed clear** (see
`docs/PRIOR_ART.md`, researched 2026-07-23): no existing open-source
project, web or desktop, currently combines pdfcer's full target
feature breadth in one native application. The closest attempts (Open
PDF Studio, KillerPDF) each have confirmed major gaps and neither uses
a native Rust PDF engine. This validates the project's premise.

### 1.1 Network posture — THREE separate clauses, not one (★ RE-SCOPED 2026-08-13 by the operator, decision 061)

**★ READ THIS HEADING CHANGE AS A WARNING.** This section was titled
*"Privacy posture"* and stated a single, absolute posture. **It was
conflating three different promises**, and on 2026-08-13 the operator
corrected the third of them as **too broad**. The three are separated
below because collapsing them is what produced the error: the first two
are unchanged and permanent, and only the third moved.

**The operator's correction, verbatim and in full (2026-08-13):**

> *"the no network rule was made too broad. all I meant by that is the
> software itself didn't rely on network technology to function which
> would bloat it and slow things down the way it does for other pdf
> software. it is fine to have download update or download addin
> capability."*

**The line that replaces the absolute is his own:** *what the software
needs to **RUN**, versus what the operator can **ASK** it to fetch.*

#### Clause 1 — pdfcer does not OPERATE over network technology (KEPT; this is the ORIGIN of the rule)

No web server, no browser runtime, no local network listener. Everything
happens in one native process (§1). This is the clause the whole posture
grew out of, and it is unchanged.

**★ AND HERE IS THE MOTIVATION, WHICH THIS SECTION NEVER RECORDED UNTIL
NOW — the omission is the most instructive part of the whole
correction.** The reason is **architecture and performance**, in the
operator's own words: *"which would bloat it and slow things down the way
it does for other pdf software."* Every competitor that ships a browser
runtime or a local service pays for it in install size and startup
latency, and pdfcer's single-native-process design is what buys that back.

§1.1 recorded the **posture** and not the **reason**, and a posture with
no stated reason has nothing to check a later restatement against. That
is very likely how it drifted into an absolute: an agent restating
*"pdfcer doesn't need the network"* with no reason beside it has no way to
tell which broader readings are still faithful, and the broadest reading
always feels like the safe one. See the `R194` proposal in `ROADMAP.md`
*Standing rules* for the generalised shape and its two sibling instances.

#### Clause 2 — PRIVACY (KEPT, UNCHANGED, and explicitly NOT relaxed by the 2026-08-13 correction)

No telemetry, no usage analytics, no crash reporting, no
licence-verification callback, no silent phone-home. Every document a
user opens is processed entirely locally, in-process, with no data ever
leaving the machine unless the user explicitly initiates it themselves
(e.g. emailing a file — an action pdfcer doesn't perform on their behalf
anyway). If a feature genuinely needs network access **that the operator
did not ask for at the moment it happens** (an update checker that runs
at startup, say), it must be **off by default and explicitly opted
into**, disclosed plainly in the UI and in `README.md`, never silently
enabled.

**★ THE OPERATOR SAID NOTHING ABOUT THIS CLAUSE, AND THAT SILENCE WAS
DELIBERATELY NOT READ AS PERMISSION.** The engineer refused to widen it,
and the refusal is recorded here rather than merely acted on, because the
general form is worth more than this instance: **an operator narrowing
one clause is not consent to widen a neighbouring one.** A correction is
scoped to what it names. Where two obligations were written as one
sentence, un-collapsing them is the *first* step of applying the
correction, not an optional tidy-up afterwards.

#### Clause 3 — the BLANKET BAN on any network-client crate (★ THIS is what was too broad, and it is NARROWED, not removed)

The old text: *no HTTP/TLS/socket client crate may enter **any** pdfcer
crate.* That turned *"does not need the network to function"* into
*"cannot fetch anything, ever"* — a different and much stronger claim
than the one clause 1 was ever about. **Download-update and
download-addin capability are explicitly permitted** as of 2026-08-13.

The replacement is a **narrowed gate, not the absence of one**:

| subject | posture | why |
|---|---|---|
| **`pdfcer-core`, `pdfcer-render`** — the ENGINE | **network-free, permanently, gate-enforced** | Parsing and rendering a file must never *require* a network. **Second, independent justification:** both crates must cross into the **wasm32/web fork** (§3), where a native HTTP stack does not exist — so this half of the gate is load-bearing twice over and does not rest on the posture alone. |
| **`pdfcer`, `pdfce-gui`, `tools/`** — the SHELLS | **may carry a network client** for operator-initiated fetching | This is what the operator authorised: model downloads, update downloads, add-in downloads. The fetch happens *because the operator asked*, which is the side of clause 2's line that was never in dispute. |

**Enforcement, as it now stands on disk:** the fail-closed `no-network`
CI job, narrowed to the engine at **`197f0a5`** (2026-08-13). Its job
name was part of the problem and was fixed in the same commit —
*"verify no HTTP/TLS client in any pdfcer crate"* asserted the over-broad
claim to anyone reading a green run, and now reads *"verify the ENGINE
needs no network (core + render)"*. **A green CI run is no longer
evidence that pdfcer as a whole makes no network calls**, and the name has
to say so. Per `R192`, the job's comment block now **enumerates** what it
cannot see (a raw `std::net` socket needs no dependency; build scripts
and proc macros are outside `cargo tree`'s default edges; the shells are
out of scope by design; and nothing in a Cargo-graph check can
distinguish an operator-initiated fetch from a silent one — that last one
is clause 2's obligation and is enforced by review and by the
decision-record requirement, not by this job).

Adding a network client to a **shell** no longer needs a decision record.
Adding one to `pdfcer-core` or `pdfcer-render` is **not** unlockable by a
future decision record; that bar is unchanged from the original R12.

#### Precision clause (2026-07-30, decision 003 §3.4) — STILL TRUE, STILL USEFUL, and note that it is a different KIND of amendment

pdfcer (as of this writing) makes no network requests and contains **no
HTTP client and no TLS stack** — verifiable by any reader of the
generated `THIRD_PARTY_LICENSES.md` — but the shipped GUI binary does
link the `webbrowser` crate (and its `url` parser dependency), because
eframe 0.35 hardcodes egui-winit's `links` feature and it cannot be
disabled downstream. That code opens the OS default browser and makes no
request itself; it is inert unless pdfcer emits an `OpenUrl` event. When
it fires, the request belongs to the user's browser, not to pdfcer. State
the posture in exactly these terms (decision 003 §6.3's copy) — "no
network code at all" would be false.

**★ THE TWO AMENDMENTS ARE NOT THE SAME ACT, and conflating them would
lose the lesson.** Decision 003 §3.4 was a **correction of WORDING**: the
claim was very slightly false as phrased, the intent was untouched, and
nothing about what pdfcer may do changed. The 2026-08-13 correction is a
**correction of SCOPE**: the wording was accurate, and the *subject set
it quantified over* was wrong. The second is the larger and rarer act —
it changes what the project is permitted to build, it changes a CI gate,
and it un-withdraws withdrawn work (`af5580e`). A precision clause can be
filed as a footnote; a scope correction earns a decision record, and this
one has **061**.

**★ DOWNSTREAM CLAIM THAT IS TRUE TODAY AND BECOMES FALSE ON THE FIRST
DOWNLOADER, tracked so it is not discovered by a user.** `README.md`'s
*"Privacy, platform and signing"* block states *"pdfcer does not use the
network. It contains no HTTP client and no TLS stack — you can confirm
this yourself in `THIRD_PARTY_LICENSES.md`."* That is **accurate at HEAD**
and must **not** be pre-emptively softened, which would make it less true
than it is. It must be rewritten **in the same Pass that first links a
network client into a shell**, per the claim-bearing-copy rule. Filed as
a Backlog obligation in `ROADMAP.md`.

#### Clause 3, continued — ★ THE GUARANTEE IS NOT LOST; IT BECOMES A BUILD CONFIGURATION

The strongest sentence §1.1 used to carry was **"no HTTP client exists in
this binary, verifiable by anyone reading `cargo tree`."** The obvious
objection to the narrowing is that this was given up.

**It was not.** The permitted capability is **strippable** — the operator
bound it so in the follow-on instruction that scoped it (*"if we add
these they should be modular in the same ways as out other features. if
someone doesn't include the crate in the package then that removes the
feature"*), and decision 061 §3 applies the `Pass 70.0`
strippable-capability convention to it in full. So anyone who wants the
old guarantee **builds without the feature and verifies it exactly as
before** — same command, same evidence — and `THIRD_PARTY_LICENSES.md`
agrees, because `cargo-about` generates it from the shipping graph.

**Nothing was given up; an option was gained.** Recorded here, not only
in §12, so a future session re-reading the narrowing finds the answer to
*"we gave up a guarantee"* **already made** rather than re-litigating it.

The fetch code lives in a sibling crate — working name **`pdfcer-fetch`**,
**PLANNED, NOT BUILT**, no such directory on disk — which the shells
depend on **optionally** and which `pdfcer-core`/`pdfcer-render` **never**
depend on at all. That is what keeps this section's enforced half
checkable by `cargo tree` and the wasm32 fork reachable. Full shape and
its `pdfcer-print` precedent: **decision 061 §2**.

#### Applied a second time — digital SIGNING (2026-09-05, decision 136)

The same boundary decision 135 drew for revocation (fetch in a shell,
validate in core) is drawn for the **private key**: `pdfcer-core` defines
the `Signer` trait as *hash in, signature out* and implements it for
exactly one custodian it can legitimately hold in memory — a PKCS#12 file
(`Pkcs12Signer`). A Windows certificate-store key (CNG), a PKCS#11 token,
or a cloud/CSC identity signs **in a shell**, behind the same trait; the
engine never gains an OS key store, a device driver or a TSA client, and
the RFC 3161 round trip that makes a B-T signature is likewise a shell's
(`Pass 10.11`). **Note for that Pass:** its `--tsa-url` is the first
network client a shell links, so the README claim tracked above falls due
then, not before. Full statement: §5.13 and §12 decision 136.

## 2. Language & toolkit decision (made 2026-07-23, by the user)

| Decision | Choice | Why |
|---|---|---|
| Systems language | **Rust** | Single self-contained native binary (no runtime to bundle), memory safety for a file-format parser that will be fed adversarial/malformed input from the public internet, first-class WASM target for the future web fork, mature crate ecosystem for compression (`flate2`, `weezl` for LZW), fonts (`ttf-parser`, `allsorts`), image codecs, and crypto (`rsa`, `aes`, `sha2`, `rustls`-adjacent primitives) that pdfcer will need anyway. |
| GUI toolkit | **egui + eframe** (recommended default — see §2.1) | `eframe` is egui's application shell and already targets **both native (winit+wgpu/glow) and WASM+canvas from the same codebase** — this is the single biggest lever for making the later web fork cheap. Immediate-mode fits a tool-heavy, many-panel editor (canvas + thumbnails + inspector + toolbars) well; prior art includes rerun.io and many CAD-adjacent Rust tools built the same way. |
| Rendering backend | `wgpu` (falls back to `glow`/OpenGL if needed) | Cross-platform, matches eframe's default, no separate native-toolkit dependency to bundle. |

### 2.1a — Toolchain pin & lockfile policy (Pass 0 task)

- **Toolchain**: pin a specific stable Rust release via `rust-toolchain.toml`
  at the workspace root, created at Pass 0. Don't float on "whatever
  stable is installed" — reproducibility matters for a project other
  people will eventually build. Bump deliberately (dated decision-log
  entry), not silently.
- **MSRV** (minimum supported Rust version): not yet decided — set it
  at Pass 0 once the toolchain is pinned, document it in `Cargo.toml`'s
  `rust-version` field, and re-check it against `docs/PRIOR_ART.md`'s
  candidate dependencies (some crates there have their own MSRV floors
  that could force pdfcer's own MSRV higher than expected).
- **`Cargo.lock`**: **commit it.** This is an application workspace
  (produces `pdfce-gui`/`pdfcer` binaries), not a pure library —
  the Rust ecosystem convention for binaries is to commit the lockfile
  for reproducible builds, unlike libraries which typically don't.
  Don't `.gitignore` it.
  **★ UPDATED 2026-09-03 (`Pass 247.0`, `da3b2f8`, 399th filing;
  decision 128/130).** `pdfce-gui` is removed from this workspace — it
  produces **`pdfcer` only** now. The GUI binary is built by the
  separate `D:\dev\pdfcer-gui` project. `Cargo.lock` shrank 488 → 159
  packages (329 removed, 0 added, 0 version changes) the same commit;
  still committed, same reasoning.

### 2.1 — egui vs iced: still confirm at Pass 0

The user's decision was "Rust core + native GUI (egui/iced)" — the
specific pick between the two was left to engineering judgment. This
document recommends **egui/eframe** for the WASM-parity reason above.
**pdfcer-engineer**: treat this as a strong default, not yet a closed
decision — confirm it explicitly with the user at the start of Pass 0
(the first real coding session) before the workspace is scaffolded,
since reversing it later means rewriting the entire GUI crate.

## 3. Workspace layout (Cargo workspace, to be created at Pass 0)

```
D:\Dev\pdfcer\
  Cargo.toml                  <- workspace root, [workspace] members below
  crates\
    pdfcer-core\                <- COS object model, tokenizer, xref (table + stream),
                                   object streams, incremental-update writer, filters,
                                   fonts, color spaces, encryption/decryption, digital
                                   signature verification, content-stream interpreter
                                   (produces a display-list / draw-op stream, NOT pixels).
                                   ZERO windowing/GUI/rendering-backend dependencies.
                                   THIS is the crate that forks to WASM later.
                                   **`font_embed.rs` (Pass 21.0, FF-C, decision 021,
                                   commit `48c6b77`; body-section sync 2026-08-04
                                   continuation 77):** plain-data contract
                                   (`FontEmbedPlan`/`SubsetGlyph`/`DescriptorMetrics`/
                                   `OutlineKind`) plus `build_objects` — emits the new
                                   `/Type0`+`/CIDFontType2`+`/FontDescriptor`+
                                   `FontFile2`+`/ToUnicode` PDF objects from a plan
                                   `pdfcer-render` fills in. No font-PROGRAM parser
                                   lives here — `fontdata/` stays metrics-only, even
                                   after this Pass. See §4 for the full contract and
                                   why the split runs this way.
                                   **`fontinfo.rs` (`Pass 67.0` phase A, 2026-08-12,
                                   `7aa5c2c`+`fa2414e`; read-only, no §12 entry —
                                   additive, no crate boundary or invariant change):**
                                   the font-INVENTORY half — per distinct font object
                                   (dedup by object id), `/BaseFont`, subtype,
                                   encoding, embedded/subset status, raw+decoded
                                   `/FontFile*` byte size, `fsType`, `/ToUnicode`
                                   presence, and a **stated-reason**
                                   `Removability` verdict (nine variants: one
                                   `Removable`, eight named refusals). Nothing here
                                   mutates a document.
                                   **`font_unembed.rs` (`Pass 67.0` phase B,
                                   2026-08-12; §12 decisions 046/047):** the font-
                                   REMOVAL half — consumes `fontinfo::Removability`
                                   rather than deriving its own verdict (one
                                   classifier only, so the report and the action
                                   cannot disagree). `EditSession::unembed_fonts`/
                                   `unembed_preview`/`unembed_refusal` (`edit.rs`)
                                   plan-then-commit the removal of
                                   `/FontFile`/`/FontFile2`/`/FontFile3` from a
                                   `/FontDescriptor` for `Removable` fonts only;
                                   every other verdict refuses by name with its
                                   reason shown (R124). Strips the §9.6.4 subset
                                   tag from `/BaseFont`+`/FontName` together by
                                   default (decision 046; overridable,
                                   `SubsetTagPolicy::Keep`) and removes `/CIDSet`/
                                   `/CharSet` (decision 047). Two sharing hazards
                                   handled explicitly: a `/FontFile*` reached by a
                                   second, non-removed font's descriptor is not
                                   freed; a `/FontDescriptor` reached by a second
                                   font DICTIONARY blocks the removable font
                                   outright rather than editing a shared object.
                                   Only a **full rewrite** reclaims the freed bytes
                                   — an incremental save appends, so it cannot
                                   shrink the file (§7.5.6).
                                   **`font_embed_missing.rs` (`Pass 67.0` phase
                                   E, 2026-08-12; §12 decisions 048–053):** the
                                   REVERSE of `font_unembed.rs` — a font
                                   `/BaseFont` names but no `/FontFile*` stream
                                   carries. Two structural shapes, chosen per
                                   font: **Attach** adds one `/FontFile2`/
                                   `/FontFile3` key to an EXISTING
                                   `/FontDescriptor`, changing nothing else;
                                   **Synthesise** writes the `/FontDescriptor`,
                                   `/Widths`, `/FirstChar`/`/LastChar` and
                                   `/Encoding` §9.6.2.2 permits a standard-14
                                   font to omit, from pdfcer's own compiled
                                   Adobe Core-14 metric tables — the SAME
                                   metrics a reader was already substituting
                                   with, so **glyph positions cannot move**;
                                   only letterforms change. Never derives its
                                   own donor-resolution policy — the shell
                                   (`FontEnvironment::resolve_for_embedding`,
                                   `pdfcer-render`) resolves a name to bytes and
                                   hands core the bytes; `pdfcer-core` sniffs the
                                   program's own framing itself, unchanged
                                   crate-boundary discipline from `font_embed.rs`.
                                   `EditSession::embed_preview`/`embed_refusal`/
                                   `embed_fonts` (`edit.rs`) plan-then-commit,
                                   mirroring `unembed_*`'s three-function shape.
                                   Refuses composite/CID (`Identity-H` codes ARE
                                   glyph indices — decision 052) and Type 3
                                   (`/CharProcs` already in the document) by
                                   name; refuses a donor whose `OS/2 fsType`
                                   reads Restricted/Ambiguous (decision 053);
                                   refuses ANY font reached through a shared
                                   `/FontDescriptor` (decision 050, asymmetric
                                   with `font_unembed.rs`'s own sharing rule —
                                   see §12). **Font programs are deflated on
                                   the way in** (`filters::flate::encode`, new
                                   this Pass) — ≈46% of original size.
                                   **`Document::next_object_number` gained a
                                   fourth source this Pass** — see §5.7 below
                                   and standing rule `R189`; not specific to
                                   font embedding, found by this Pass's own
                                   pixel-identity sweep oracle.
                                   **`export/dxf.rs` (`Pass 52.0`/`52.3`,
                                   2026-08-09, `3c4aca4`→`1f4839d`; §12
                                   decision 035, claimed by citation):**
                                   a WRITE-only path out of pdfcer-core's
                                   `vector::PageObjects` model into ASCII
                                   DXF — deliberately not governed by §5's
                                   round-trip invariant (foreign output
                                   format, not PDF-to-PDF), hand-written
                                   with no new dependency, zero
                                   GUI-symbol imports. See §12 for the
                                   three data-model forks it decided.
                                   **`Pass 52.2` (`d2d03a5`→`0466281`,
                                   2026-08-09) adds `suggest_scale_for_groups`/
                                   `dimension_groups_on_page` — the
                                   PAGE-SCOPED sibling of the document-wide
                                   `suggest_scale`, because a document-global
                                   inference consumed by a page-scoped export
                                   is silently wrong on multi-page sheets.
                                   Landed in `0466281`, not `d2d03a5` — see
                                   §12's decision-035 correction footer,
                                   dated 2026-08-09. GUI half (File ▸ Export
                                   ▸ Export DXF…) also `0466281`; the family
                                   is now COMPLETE across core/cli/gui.
                                   **`outline.rs` (`Pass 55.3`, 2026-08-10,
                                   `1862b1f`; §12 decision 036):** a
                                   READ-only §12.3.3 tree walker —
                                   `/First`/`/Next`/`/Parent` chains, no
                                   array anywhere in the source encoding —
                                   into an owned `Vec<OutlineItem>`.
                                   `/Count`'s sign carries open/closed
                                   state; its MAGNITUDE is transitively
                                   visible descendants, not an immediate-
                                   child count (a common misreading this
                                   module deliberately does not make).
                                   Cycle-guarded (`MAX_OUTLINE_DEPTH`) —
                                   a `/Next` chain has no array bound, so
                                   a malformed backward pointer describes
                                   an infinite list a naive walker would
                                   hang on. Distinct from
                                   `pageops::outline` (outline authoring/
                                   carryover across page operations,
                                   pre-existing), a deliberately different
                                   job with different simplifications.
                                   **`attachments.rs` (`Pass 55.4`,
                                   2026-08-10, `1862b1f`):** both standard
                                   §7.11.7 attachment paths (document-level
                                   `/Names /EmbeddedFiles` and page-level
                                   `/FileAttachment` annotations) unified
                                   into one `Vec<Attachment>`, plus
                                   `AttachmentNotes::may_be_encrypted` — a
                                   deliberately over-broad warning for the
                                   §7.6.5 `/EFF`+`DefEmbeddedFile` case
                                   (an otherwise-unencrypted document can
                                   carry PER-FILE-ENCRYPTED attachments
                                   with no password prompt; the filter
                                   chain runs and returns garbage that
                                   looks like a successful read). `name`
                                   is raw/verbatim; `safe_name()` is a
                                   separate sanitising accessor found
                                   necessary by TESTING, not reasoning — a
                                   NUL in `/F` was already U+FFFD before
                                   any sanitiser saw it, and
                                   `"\u{202E}gnp.exe"` (RTL override) reads
                                   as `exe.png`. `extract_attachment`/
                                   `attachment_bytes` exist here with NO
                                   CLI or GUI caller as of this Pass — an
                                   R151 instance, named not silently
                                   carried; see `ROADMAP.md`'s `Pass 55.4`
                                   Shipped entry.
                                   **`find_text` (`Pass 55.0`, 2026-08-10,
                                   `04c7820`) lives in `edit.rs`,** not a
                                   new module — it is the search-to-quad
                                   scan Pass 8's redaction verb already
                                   contained, extracted so it can run
                                   without mutating anything. Sharing the
                                   scanner with `add_redaction`'s search
                                   path is a correctness property: a
                                   second, independent implementation of
                                   glyph-span-to-quad geometry could drift
                                   from the first in the one direction
                                   that matters — a redaction covering a
                                   different box than the search that
                                   found it. A cross-check test compares
                                   `find_text`'s quad against
                                   `redact-mark`'s `/QuadPoints` for the
                                   same match so a future split of the two
                                   paths has to fail it. No encryption or
                                   certification gate — a signature
                                   freezes a document, it does not forbid
                                   reading it. Limits stated rather than
                                   silently absent: `/ActualText` runs are
                                   unmatched (no per-glyph geometry to
                                   locate a hit against); matching is
                                   per-TEXT-RUN, so a phrase a producer
                                   split across two `Tj` calls is missed;
                                   page content only.
                                   **`layers.rs` (`Pass 55.5`, 2026-08-10,
                                   `4810b49` registration, `6b806a9`
                                   CLI+GUI; §12 decision 036):** a
                                   READ-only §8.11 optional-content (OCG)
                                   tree reader — `[Layer]` plus the
                                   `[OrderNode]` presentation tree
                                   `/OCProperties /D /Order` declares.
                                   Reuses `annot.rs`'s
                                   `optional_content_default_off` rather
                                   than a second resolver, so this
                                   module's notion of "starts visible"
                                   cannot drift from the renderer's own —
                                   two independent resolvers disagreeing
                                   would mean a layers panel claiming
                                   "on" about content the page actually
                                   hides. **`/Order`'s default in `/D` is
                                   the empty array, and that default is
                                   itself a `shall`** (§8.11.4.3): a
                                   producer that omits `/Order` gets a
                                   strictly-conforming reading of NO
                                   groups presented, not an inferred full
                                   list. Depth/cycle-guarded
                                   (`MAX_ORDER_DEPTH`) the same way
                                   `outline.rs` is. **No write path** —
                                   toggling a layer's visibility is
                                   session state a viewer holds, with no
                                   file-format footprint unless the
                                   operator explicitly saves; pdfcer has
                                   neither a renderer visibility override
                                   nor a save path for one, so this
                                   module is deliberately view-only (R83)
                                   until both exist.
                                   **`signature.rs` gains
                                   `byte_range_coverage` (`Pass 10.0`,
                                   2026-08-10, `2676d4d`):** measures each
                                   signature's declared `/ByteRange`
                                   against the file's real length —
                                   arithmetic only, no PKCS#7, no trust
                                   chain, no cryptography of any kind.
                                   §12.8.1 makes whole-file coverage a
                                   `should` not a `shall`, so a short
                                   range is reported as CONFORMING
                                   (merely under-protecting), while an
                                   overlapping/out-of-order range violates
                                   Table 252's "exact byte range" and IS
                                   reported malformed — the function's own
                                   doc comment states it "cannot tell you
                                   a signature is VALID — only what it
                                   would be valid over." Reached through
                                   the existing `forms::parse_acroform`,
                                   not a third field walk (project rule
                                   2). CLI surface: `pdfcer
                                   list-signatures` (§7). See `ROADMAP.md`
                                   `Pass 10.0` Shipped entry (seventy-
                                   sixth filing) for the fixture design
                                   and the two-warning-branch correction.
                                   **★ `Pass 10.1` (`22421b6`, 2026-09-03;
                                   §12 decision 129): the cryptographic
                                   half now exists beside it.**
                                   `signature.rs` re-exports `verify` /
                                   `verify_all` from the new
                                   `signature_verify.rs`, which returns a
                                   `SignatureVerdict` of THREE independent
                                   facts, never a bool — `integrity`
                                   (`Verified { digest_algorithm,
                                   signature_algorithm }` / `DigestMismatch`
                                   / `SignatureInvalid` / `Unverifiable {
                                   reason }`), `coverage` (the `Pass 10.0`
                                   answer, folded in) and `trust` (only
                                   `NotChecked` exists). Its arithmetic and
                                   parsing are in-crate, no new dependency:
                                   `asn1.rs` (bounds-checked DER reader,
                                   definite lengths only), `cms.rs`
                                   (SignedData / SignerInfo / X.509 v3),
                                   `crypto/{bignum,sha1,rsa,ecdsa}.rs`.
                                   `signature.rs`'s own header still says
                                   *"This module verifies nothing"* and its
                                   line 145 names the stage `Pass 10.2` —
                                   true of that file's OWN code, wrong
                                   about the file as a whole; both owed a
                                   rewording (398th filing, engineer's).
                                   CLI: `pdfcer verify-signatures`
                                   (§7).
                                   **`annot.rs`/`layers.rs` (`956ef4d`,
                                   2026-08-10):** `annot::oc_refs` widened
                                   to `pub(crate)`; `layers::group_refs`
                                   (a second, independent implementation
                                   of the same §8.11.3.3 "one dict or an
                                   array" resolution) deleted, five
                                   `layers.rs` call sites converted to the
                                   shared function — see §12's plain dated
                                   entry for why this is a correctness
                                   property, not a tidiness pass.
                                   **`formcsv.rs` (`Pass 62.0`, 2026-08-11,
                                   `a64b5fd`; §12 dated entries below):**
                                   two-column `name,value` RFC 4180 CSV, a
                                   third interchange format alongside
                                   `fdf.rs`'s FDF/XFDF, hand-rolled rather
                                   than reusing either reader (CSV shares
                                   no syntax with FDF or XFDF). Export
                                   runs every value through a
                                   formula-injection neutraliser (a
                                   leading `=`/`+`/`-`/`@` gets a leading
                                   apostrophe) before writing — an
                                   unneutralised cell would let a form
                                   value pdfcer did not author reach a
                                   spreadsheet's live formula engine and,
                                   via `=WEBSERVICE(...)`, the network:
                                   the same capability **R12** refuses in
                                   pdfcer's own tree, reached by a longer
                                   route. Neutralisation is counted AND
                                   NAMED (R181's shape) and REVERSED on
                                   import, so a round trip through a
                                   spreadsheet does not accumulate
                                   apostrophes. A malformed row (wrong
                                   column count) or an empty file REFUSES
                                   rather than risking a destructive
                                   partial import. Format detection in
                                   both shells is by CONTENT — FDF's `%`
                                   header, XFDF's opening `<`, CSV as the
                                   untried residue — not by file
                                   extension.
    pdfcer-render\               <- Takes pdfcer-core's draw-op stream + resources
                                   (fonts, images, color spaces) and rasterizes to an
                                   in-memory pixel buffer via `tiny-skia` (CPU-only,
                                   pure Rust, no GPU/windowing context — see
                                   docs/PRIOR_ART.md, resolved 2026-07-23).
                                   Depends on pdfcer-core. Still GUI-framework-agnostic
                                   (no egui/eframe dependency) — a headless render
                                   (e.g. "render page 3 to PNG") must work with zero
                                   windowing system present, which is also what makes
                                   the eventual web fork (canvas-based rendering) and
                                   any future CLI/batch tooling possible.
                                   **Implementation note (Pass 1, amended 2026-07-30):**
                                   the content-stream *interpreter* (`gstate`/
                                   `interpret` modules, incl. §8.10.1 Form-XObject
                                   execution and §8.9 image drawing) lives HERE, not
                                   in pdfcer-core as this diagram's original wording
                                   ("content-stream interpreter... produces a
                                   display-list/draw-op stream") implied — pdfcer-core
                                   supplies the lossless content-token model only
                                   (`content.rs`); pdfcer-render walks those tokens and
                                   paints directly, with no separate draw-op IR
                                   in between. Recursive `Do` dispatch into nested
                                   Form XObjects is therefore a pdfcer-render-time
                                   concern (`MAX_XOBJECT_DEPTH`, §10.1), distinct from
                                   pdfcer-core's parse-time recursion guards (page-tree
                                   depth, xref/ObjStm cycles).
                                   **★ Implementation note (measured 2026-08-07,
                                   `76200e9`) — THE CLIP'S REPRESENTATION IS THIS
                                   CRATE'S COST CENTRE.** The graphics-state clip is
                                   an `Option<tiny_skia::Mask>`: a PAGE-SIZED coverage
                                   buffer, one byte per device pixel. On a
                                   129,515-path CAD sheet the clip machinery was
                                   **95% of render time** — painting every path costs
                                   **0.87 s** against an 18.04 s total — while read +
                                   parse + page tree together were **~0.005%**. Two
                                   semantics-preserving fixes landed in
                                   `interpret.rs`: a per-paint `clip.clone()` became a
                                   borrow (~108 GB of memcpy for one page, scaling
                                   with page AREA), and `intersect_clip`'s multiply is
                                   bounded to the path's device bounds — an IDENTITY,
                                   since outside them the fresh mask is zero. Output
                                   byte-identical (SHA-256). ~~**`Mask::new` alone
                                   remains 10.1 s of the remaining ~18 s**; reducing
                                   it is a REPRESENTATION change (most clips are
                                   `re W n` rectangles needing no mask at all) and was
                                   deliberately not folded into that commit.~~
                                   **★ CORRECTED 2026-08-07 (`4475fe6`) — BOTH CLAUSES
                                   IN THAT STRUCK SENTENCE WERE WRONG.** `Mask::new`
                                   is **1.02 s, not 10.1 s** (the 10.1 s came from an
                                   ablation that measured construction PLUS use — an
                                   **R164** instance), and only **612 of 24,128 clips
                                   — 2.5% — are rectangles**, so the rectangle
                                   special-case was **declined on measurement, not
                                   built**. The real 1× distribution was **`q`/`Q`
                                   gstate clone 6.80 s**, `mask.fill_path` 5.24 s,
                                   multiply 2.26 s, `Mask::new` 1.02 s. **The clone
                                   was the cost, and `4475fe6` removed it by making
                                   the clip an `Arc<Mask>`** — sound because a clip is
                                   never mutated in place (`intersect_clip` builds a
                                   FRESH mask and assigns it; the old one is only
                                   read), so `q` needs a reference, not a buffer, and
                                   no copy-on-write is required because there is no
                                   write. **`Arc` rather than `Rc` is a deliberate
                                   architectural choice: it keeps
                                   `GraphicsState: Send`**, which is what leaves
                                   off-thread page rendering reachable without a
                                   second type change; the cost is one atomic
                                   increment per `q`. Result: `q`/`Q` clone
                                   **6.80 s → 0.01 s**, 1× **17.47 → 10.18 s**, 2×
                                   **214.71 → 51.52 s**, and the 1×→2× cache cliff
                                   **14.1× → 5.1×** (reduced, not gone). Output
                                   byte-identical on the CAD sheet **and on 52
                                   synthetic fixtures** — that page has zero images
                                   and 242 text elements, so it cannot witness a
                                   regression in image sampling, glyph rasterization
                                   or annotation appearance, and a
                                   "no pixel anywhere changes" claim needs witnesses
                                   spanning the surfaces that produce pixels.
                                   ~~**What remains is still a REPRESENTATION change,
                                   but a differently-shaped one:** clips are 100%
                                   single-subpath, mean 7 segments, mean bounding box
                                   **0.663% of the page**, so the mismatch is between
                                   clip EXTENT and mask EXTENT — not between clip
                                   SHAPE and mask SHAPE.~~
                                   **★★ CORRECTED 2026-08-07 (`6b33789`) — THAT
                                   STRUCK SENTENCE IS WRONG BY 100×, AND IT IS THE
                                   SECOND WRONG FIGURE IN THIS SAME BLOCK.** Mean
                                   clip bounding box is **66.36% of the page, not
                                   0.663%** — a fraction printed as a percent. The
                                   sheet's first clips cover **87%, 65%, 100%, 81%,
                                   95%**; individual and accumulated bboxes both give
                                   66.36%, so it is not an accumulation artifact.
                                   **There is no EXTENT mismatch to exploit** — a
                                   mask sized to a 66%-of-page clip is a page-sized
                                   mask in all but name — and the follow-on
                                   optimisation this sentence was the premise for is
                                   **RETIRED** in `ROADMAP.md`'s *Next up*, not
                                   merely annotated. **Two further, independent
                                   refutations, either fatal on its own:** tiny-skia
                                   requires the clip mask and the pixmap to be the
                                   SAME SIZE and **enforces it SILENTLY** —
                                   `RasterPipelineBlitter::new` returns `None` on a
                                   mismatch (`pipeline/blitter.rs:36-44`), a
                                   `log::warn!` and a **dropped paint**, so a smaller
                                   mask produces WRONG output rather than fast output;
                                   and the saving does not exist anyway —
                                   `Mask::fill_path` costs **10.3 µs on a 64×64 mask
                                   vs 8.3 µs page-sized**, being dominated by three
                                   raster-pipeline compilations per call rather than
                                   by rasterization, while `scan::path_aa::fill_path`
                                   **already** bounds itself to `path.bounds()`.
                                   `Mask::new` at page size is **24.6 µs**, so its
                                   ~1.02 s is real and **irreducible without changing
                                   the representation**. **The clip-representation
                                   line of attack is CLOSED**; what survives of the
                                   census is SHAPE (single-subpath, 7 segments), not
                                   SIZE. The `intersect_clip` doc comment that
                                   asserted clips *"mostly cover a few percent"* was
                                   **corrected in place the same day it was written**,
                                   and **the bound it justifies remains an IDENTITY
                                   worth keeping** — it skips the ~34% outside the new
                                   path, a third of the work rather than two orders of
                                   magnitude. `clip_bbox` is a **`GraphicsState`
                                   field** rather than a thread-local for a reason
                                   found the hard way: any clip-derived quantity
                                   tracked outside the graphics state is monotonically
                                   wrong, because **`Q` reinstates a LARGER clip** and
                                   a tracker that only ever shrinks never widens on it.
                                   See §12's 2026-08-07 twenty-second entry.
                                   **Consequence for anyone optimising here: tiling
                                   and threading would today be aimed at 5% of the
                                   cost.** See §12's 2026-08-07 twentieth and
                                   twenty-first entries and `ROADMAP.md`'s two
                                   *fix — RENDER PERFORMANCE* entries.
                                   **★ AND THERE IS NOW A MEASURED FLOOR UNDER ALL
                                   OF IT (2026-08-07, `fa17d54`,
                                   `render-profile --ablate-sweep`): 0.49–0.53 s
                                   while pixels vary 64×.** The floor is
                                   **SCALE-FLAT**, therefore **per-operation** — it
                                   is the cost of walking **148,517 content-stream
                                   operators** and building their paths, and it is
                                   identical at every scale. **Pixels are
                                   essentially free here.** The complete map at 1×:
                                   interpreter floor **0.5 s** · painting
                                   **~0.8 s** · mask sampling **free, at the noise
                                   floor** · **clip construction ~8.4 s = 86%** —
                                   the last of which **reproduces the per-phase sum
                                   above (5.24 + 2.26 + 1.02 = 8.52 s) within 4% by
                                   a DIFFERENT METHOD**, which is the second
                                   measurement **R166** requires before a figure may
                                   order work. **The binding constraint on how this
                                   crate may be optimised is therefore stronger than
                                   "tiling addresses 5%": tiling and threading
                                   cannot go BELOW the floor at all**, because they
                                   render fewer pixels and not fewer operators.
                                   **A low-resolution proxy is likewise bounded
                                   below by ~2.6 s** — at 0.25× the full render is
                                   **2.57 s, not 0.67 s**, because clip construction
                                   drops only ~4× for a 16× pixel reduction. Anyone
                                   reaching for a proxy or progressive refinement as
                                   the answer to interactive speed should read that
                                   sentence first. See §12's twenty-fourth entry.
                                   **★★ AND THE 86% IS NOW BROKEN DOWN
                                   (2026-08-07, `110b8c9`, per-phase timing
                                   rather than ablation — a timer removes
                                   nothing, an ablation removes other things
                                   with it, R164). At 1× over 24,128 clips:
                                   `Mask::new` 1.03 s (42.7 µs, 11.8%) ·
                                   `fill_path` 5.22 s (216.4 µs, 59.9%) ·
                                   the multiply 2.46 s (102.0 µs, 28.3%) =
                                   8.72 s (361.2 µs per clip). Sum + floor =
                                   9.26 s against a 9.49 s render — THE
                                   ARITHMETIC CLOSES**, and the ~0.23 s
                                   residual is the corrected painting figure.
                                   **★ TWO FIGURES IN THIS BLOCK ARE
                                   CORRECTED BY THAT RUN.** (i) `fill_path`
                                   at **10.3 µs / 8.3 µs** above is what a
                                   SMALL path costs; in this workload it is
                                   **216.4 µs — wrong by ~22×**. **The
                                   conclusion that pair supported is still
                                   TRUE and still fatal to the clip-sized-mask
                                   idea:** buffer size does not drive the
                                   cost. What drives it is the PATH — **an
                                   anti-aliased scanline fill costs what the
                                   path's EDGES cost, not what the buffer
                                   costs** — and the original experiment
                                   varied the buffer while holding the path
                                   fixed, so it was right about the ratio and
                                   wrong about the magnitude. (ii) **Painting
                                   is ~0.27 s, not ~0.8 s**: the 0.81 s was
                                   the whole `clip-build`-ablated render,
                                   floor PLUS painting (**R164**, third
                                   instance that day); ablating `paint` alone
                                   moves the total 9.28 → 9.32 s, inside
                                   noise. **Consequence: tiling and threading
                                   address UNDER 3%, not 5% — the ordering is
                                   unchanged and the margin grew.**
                                   **★★ THE CONSTRAINT THAT SHAPES ANY FUTURE
                                   OPTIMISATION OF THIS CRATE: THE PER-CLIP
                                   COST DISTRIBUTION IS UNIFORM.** 85.0% of
                                   clips fall in 256–512 µs and 14.4% in
                                   512–1024 µs; **p90 and p99 are both under
                                   1024 µs**; only **108 of 24,128** exceed a
                                   millisecond and only **36** fall below
                                   256 µs — **99.85% inside a single 4× band.
                                   There is no tail and no head.** So there is
                                   **no pathological special case to find and
                                   fix**, and **anything that helps must
                                   change the work done for ALL 24,128
                                   clips.** That forecloses fast paths as a
                                   category (items 1 and 1′ in `ROADMAP.md`
                                   were both special-case proposals, both
                                   killed by a census; this is the third
                                   census and it kills the category), and it
                                   is why the live candidate is
                                   **deduplication of already-built clip
                                   masks — BLOCKED, deliberately, on a census
                                   of how many of the 24,128 are
                                   re-applications of an already-built clip
                                   path. Measure the repetition BEFORE
                                   building anything** (`R166` applied
                                   prospectively).
                                   **★★ THE CENSUS IS RUN AND THE BLOCK IS
                                   DISCHARGED 2026-08-07 (`1992d13`) — AND
                                   THE PREMISE SURVIVED, THE FIRST OF THREE
                                   THAT HAS.** **24,128 applications over 40
                                   distinct build keys = 603.20 per key;
                                   24,088 repeats = 99.83%.** **But the mean
                                   hides the shape: top-1 = 97.3%, top-2 =
                                   99.8%, and 37 of the 40 keys are applied
                                   EXACTLY ONCE**, so **a 2-entry cache
                                   serves 99.8% over ~1.9 MiB** (38.3 MiB ÷
                                   40 = 0.958 MiB per mask), not the 40-entry
                                   38.3 MiB the working set implies. **And a
                                   hit is worth the WHOLE operation, not 72%
                                   of it: a second key including the INCOMING
                                   clip returns 40 distinct (path, incoming
                                   clip) pairs — IDENTICAL to the 40 build
                                   keys — so every re-application is under
                                   the SAME incoming clip, the FINAL mask is
                                   identical, and a hit can SHARE THE
                                   EXISTING `Arc`: 361 µs/clip (8.72 s ÷
                                   24,128), not the 259 µs (6.25 s ÷ 24,128)
                                   a build-only cache would save.** `q`/`Q`
                                   was checked and does **not** already solve
                                   it — restore is free since `4475fe6`, but
                                   **every `W`/`W*` calls `intersect_clip`
                                   regardless**. **Both identity choices
                                   UNDERSTATE repetition by construction**
                                   (bit-exact coordinates; `Arc`-pointer
                                   incoming identity), so **99.83% and 40 are
                                   LOWER BOUNDS**. ⚠ **The ~10 s → ~1.7 s
                                   projection is ARITHMETIC over separately
                                   measured parts (99.83% × 345.6 µs mean =
                                   8.34 s over 24,128 removed, against 1×
                                   totals of 9.28–10.18 s), one instrument
                                   each, and is EXPLICITLY UNVERIFIED — no
                                   cache exists and nothing has been
                                   re-rendered** (`R166` still governs).
                                   **★★ AND THE SCALING LAW EXPLAINS WHY
                                   FEWER PIXELS BUY SO LITTLE.** Per 4×
                                   pixels: `Mask::new` 4.3×, 7.9×
                                   (**superlinear**) · `fill_path` 1.98×,
                                   2.11× (**~2× — it tracks the LINEAR
                                   dimension**) · the multiply 4.0×, 4.4×
                                   (**area-bound**). **The scanline
                                   converter's cost follows the path's
                                   PERIMETER and the number of scanlines it
                                   spans, not the buffer it writes into** —
                                   which is why `fill_path` dominates at every
                                   scale and is **still 56% of the entire
                                   render at 0.25×**. **This is the MEASURED
                                   mechanism behind the "proxies underdeliver"
                                   claim above, which until now rested on a
                                   total rather than on a law.** ⚠ One figure
                                   is **UNRECONCILED**: 56% at 0.25× with
                                   `fill_path` = 1.25 s implies a 0.25× total
                                   of ~2.23 s, against the 2.57 s recorded
                                   above — **13% apart, outside this machine's
                                   5.8% spread, and no denominator was
                                   stated.** Neither is retired here; the
                                   qualitative conclusion (both are ~3.5×
                                   above the 0.67 s naive pixel scaling
                                   predicts) holds either way.
                                   See §12's twenty-fifth entry.
                                   **★★ THE CACHE IS BUILT, AND THIS
                                   CRATE IS NOW FLOOR-BOUND (2026-08-07,
                                   `ce57ed5`, **`Pass 45.0`**).**
                                   `crates/pdfcer-render/src/clip_cache.rs`
                                   (414 lines) caches the mask **AFTER**
                                   intersection, keyed on the build inputs
                                   **plus which clip it is intersected
                                   with**, bounded to **4 entries, LRU**,
                                   and **owned by the `Interpreter`** so it
                                   dies with the content stream —
                                   deliberately **not global and not
                                   `thread_local`**, because rendering moved
                                   to a worker in **Pass 44.0** and masks are
                                   keyed partly on device size.
                                   **Result, two instruments, both filed:**
                                   engineer end-to-end **1× 32,313 →
                                   907 ms (35.63×)** and **2× 447,862
                                   → 1,425 ms (314.3×)**; the
                                   `render-profile` harness, render phase
                                   only, **1× 10.68 → 0.79 s
                                   (13.52×)** and **2× 58.52 →
                                   1.30 s (45.02×)**. **The first pair is
                                   DAY-CUMULATIVE over three fixes; the second
                                   is THIS COMMIT ALONE**, and they differ by
                                   a near-constant **117 ms at 1× / 125 ms
                                   at 2×** of process start and PNG encode.
                                   **Output BYTE-IDENTICAL — SHA-256
                                   `9250a89f…`, the SAME hash as the
                                   32.3 s render, plus an unchanged aggregate
                                   over 115 synthetic fixtures.**
                                   **★★ THE ABA HAZARD IS THE
                                   LOAD-BEARING DESIGN DETAIL, not the
                                   speed-up:** incoming-clip identity is
                                   **pointer identity**, which can lose hits
                                   and cannot invent one — but a **bare**
                                   pointer would be **unsound**, since a
                                   dropped mask's address can be reused and a
                                   stale entry would then match a pointer that
                                   means something else, **returning the wrong
                                   clip and painting a silently wrong
                                   picture**. **Each entry holds a strong
                                   `Arc` to the incoming mask**, pinning the
                                   address for as long as the entry can match.
                                   **No timing would have shown that failure
                                   — a wrong-mask hit is FASTER.**
                                   **Measured hit rate 24,087 + 41 = 24,128 =
                                   99.83%, EXACTLY the census ceiling**, the
                                   41st build being the single eviction 4
                                   slots make over 40 distinct keys; residual
                                   clip cost **41 × 362 µs = 14.8 ms
                                   = 1.9% of the render**, down from
                                   **8.72 s = 86%**.
                                   **★★ SO THE BINDING CONSTRAINT ON
                                   THIS CRATE CHANGES: the floor is now the
                                   COST.** 0.49–0.53 s against a 0.79 s
                                   render — **62–67% of what is left**,
                                   **maximum further speed-up at 1× =
                                   0.79 ÷ 0.51 = 1.55×**, and the only
                                   remaining target is the **operator walk**
                                   (148,517 operators = **3.43 µs each**).
                                   **★ AND THE ~1.7 s PROJECTION ABOVE IS
                                   DISCHARGED BY MEASUREMENT, WHICH ALSO
                                   ADJUDICATED AN EARLIER CORRECTION:** floor
                                   0.51 + painting **0.27** = **0.78 s**
                                   against **0.79 s** measured (**1.3% apart,
                                   CONFIRMED**), while floor 0.51 + painting
                                   **0.87** = **1.38 s** (**75% high,
                                   REFUTED**) — so the **`R164` painting
                                   correction, made on reasoning alone and
                                   never independently measured, is now
                                   confirmed**, and the projection was
                                   conservative by 2.2× because it rested
                                   on the uncorrected residual.
                                   See §12's twenty-sixth entry.
                                   **`font\subset.rs` (Pass 21.0, FF-C, decision 021,
                                   commit `48c6b77`; body-section sync 2026-08-04
                                   continuation 77):** `plan_subset` parses an
                                   operator-supplied donor face via the existing
                                   skrifa parser (no second font-program parser — R21
                                   unchanged), checks OpenType `OS/2 fsType` embedding
                                   permission (R109) BEFORE calling `subsetter::subset`
                                   (`subsetter` strips `OS/2`), and produces a
                                   plain-data `FontEmbedPlan` for `pdfcer-core::
                                   font_embed` to emit. `SubsetError` (R27-shaped,
                                   one variant per distinct cause) and
                                   `MAX_DONOR_BYTES` (64 MiB, a judgement call, not a
                                   corpus measurement — the census that measured real
                                   embedded font programs cannot apply to an
                                   operator-supplied donor face; see the constant's
                                   own doc comment) live here. P0 floor: `glyf`
                                   (TrueType-outline) donors only — CFF donors are
                                   refused by name (decision 021 §10, C-3).
                                   **`interpret.rs` — content-stream `BDC`/`EMC`
                                   `/OC` and XObject `/OC` honored (`Pass 56.0`,
                                   2026-08-10, `71592d3`).** Closes the gap named
                                   since Pass 1/Pass 6.0 ("§8.11 is a RAG GAP") and
                                   deferred again at Pass 12.M2 ("out of scope for
                                   annotation-only dimensioning"). **§8.11.3.1's
                                   invariant, load-bearing for every future change
                                   to this interpreter: hidden means NOT DRAWN, not
                                   NOT RUN.** Suppression happens at exactly the
                                   paint call (`skip_paint = ... || self.oc_hidden()`,
                                   two call sites) — every operator that mutates
                                   graphics state, clip, or text position still
                                   executes normally inside a hidden marked-content
                                   section. A hidden section's `W`/`W*` clip still
                                   bounds whatever unlayered content paints after
                                   it; a hidden glyph's show operator still
                                   advances the text position by its full width.
                                   Getting this backwards would make page LAYOUT
                                   depend on which optional-content layers happen
                                   to be on. Marked content is tracked ONLY for
                                   `/OC` — every `BDC` is stacked so `EMC` stays
                                   balanced regardless of tag, and a surplus `EMC`
                                   pops nothing rather than underflowing (which
                                   would un-hide still-open hidden content). An
                                   `/OC` operand that is not a resolvable indirect
                                   `/Properties` reference (§8.11.3.2 requires one)
                                   is SHOWN and counted tolerated — the same
                                   shown-by-mistake-is-recoverable posture as
                                   Pass 6.0's annotation-level `/OC` tolerance.
                                   New diagnostic `oc_sections_hidden`
                                   (`oc_hidden=<N>` on `render-page`'s pinned
                                   stdout contract, appended last; GUI diagnostics
                                   expander), deliberately NOT folded into the
                                   `unsupported` headline sum — see §12's
                                   `Pass 56.0` entry for why. Shares
                                   `annot.rs::optional_content_default_off`/
                                   `oc_is_hidden` with the annotation-level path
                                   and the Layers panel (`layers.rs`) — one
                                   resolver, so all three cannot disagree about
                                   which groups a `/D` array names. Decision 037
                                   (unregistered-OCG reading of `/BaseState /OFF`)
                                   is **ANSWERED BY MEASUREMENT as of 2026-08-11
                                   (ninety-first filing, `04f8acd`)** — the literal
                                   "every OCG-shaped object" reading is falsified
                                   against the installed Acrobat; pdfcer's shipped
                                   "registered only" reading is confirmed (this
                                   content-stream path shares `annot.rs`'s
                                   `optional_content_default_off`, so it was never
                                   a second answer to reconcile — see §12's
                                   ninety-first-filing entry). Decision 038 (Table
                                   101 vs §8.11.4.5 b), both `/ON` vs.
                                   opposite-array processing) remains CLAIMED, NOT
                                   YET AUTHORED, and applies equally to this
                                   content-stream path — it consumes the same
                                   `oc_off_set()` the annotation path already did.
                                   **★ Two §8.11 defects in the above, found and
                                   fixed the same day (`5c4ff08`, `57f0c8f`,
                                   2026-08-10):** `draw_image` was missing the
                                   `oc_hidden()` gate entirely (a self-regression
                                   of `71592d3` itself — the two `skip_paint`
                                   call sites named above covered paths and
                                   glyphs, not images; fixed by gating
                                   `draw_image` before decode, the one point
                                   every image path converges); `oc_is_hidden`
                                   was not reading Table 99 `/P` (every OCMD
                                   evaluated as `AnyOn` regardless of its actual
                                   policy — `/P /AllOff` with every member off
                                   is the inverse case, spec-visible not
                                   spec-hidden); `optional_content_default_off`
                                   was not reading §8.11.2.3 `/Intent` (a
                                   `Design`-only group hid content in a `View`
                                   render). All three now fixed at the shared
                                   resolver — see §12's seventy-ninth-filing
                                   entry.
                                   **`layer_state.rs` — `LayerVisibility`, the
                                   OPERATOR's session-scoped override (`Pass
                                   57.0`, 2026-08-10, `6ab72ec`).** Distinct
                                   from `annot.rs::optional_content_default_off`
                                   (what the DOCUMENT wants hidden): this is
                                   what the OPERATOR currently wants hidden,
                                   and it REPLACES the document's default
                                   configuration rather than merging with it —
                                   see §12's seventy-ninth-filing entry for the
                                   full contract and why a merge was rejected.
                                   Held by the GUI shell's own state
                                   (`layer_overrides`/`layers_generation`), not
                                   by `EditSession` — session-only, never saved,
                                   never marks the document dirty. `RenderOptions.layers:
                                   Option<LayerVisibility>` is the seam;
                                   `RenderPolicy` stays `Copy` because the set
                                   travels by reference, owned by the caller.
                                   **`annot.rs` — `apply_view_usage`,
                                   §8.11.4.4/.4.5 `/AS`+`/Usage` auto-state
                                   (`Pass 56.0`, 2026-08-10, `6171313` —
                                   completes §8.11, no remaining
                                   unimplemented piece on the render side).**
                                   `RenderOptions.view_magnification:
                                   Option<f32>` defaults to `None` — the
                                   §8.11.4.5 `shall not` ("printing and
                                   aggregating applications shall not apply
                                   the changes based on usage application
                                   dictionaries") is enforced by that
                                   default, not by caller discipline: a
                                   caller that has not decided whether it is
                                   a viewer gets the print-correct answer,
                                   and applying usage requires an explicit
                                   `Some(scale)`. GUI passes the operator's
                                   ZOOM (`render_worker.rs`), never the
                                   raster scale — those differ by
                                   `pixels_per_point`, and the raster scale
                                   would make layer visibility depend on the
                                   monitor. `render-page --scale` supplies
                                   it by default; `--print-state` opts out.
                                   Aggregation across `/AS` array entries is
                                   a GLOBAL CONJUNCTION (OFF dominates,
                                   order-independent) — the opposite algebra
                                   to the `/D` `/ON`/`/OFF` arrays in the
                                   same clause, where order is load-bearing
                                   (decision 038). See §12's two entries
                                   dated 2026-08-10 (eighty-second filing)
                                   for the `Option`-default enforcement
                                   rationale and the absent-usage-category
                                   (`DA-A13`) interpretation in full.
                                   **`layers.rs` — `LayerDiagnostics::
                                   auto_managed_groups` (`21910fa`,
                                   2026-08-10, eighty-third filing):**
                                   shipping `apply_view_usage` (immediately
                                   above) created a disagreement this field
                                   closes. The Layers panel enumerates the
                                   `/D`-INITIAL state; the canvas now paints
                                   the USAGE-ADJUSTED one; for a zoom-banded
                                   group these differ whenever the current
                                   magnification falls outside its band, so
                                   the panel could read "shown" while the
                                   group's content is absent from the page,
                                   with nothing to tell an operator that
                                   from a defect. Neither half is wrong —
                                   §8.11.4.5 makes a viewer's applied state a
                                   FUNCTION OF MAGNIFICATION, so "the" state
                                   of an auto-managed group is not a
                                   property of the document at all, only of
                                   the moment it is asked. `auto_managed_groups`
                                   names every OCG carrying a `/Usage` entry
                                   any `View`-event category can act on, and
                                   is surfaced (not merely available) in
                                   both shells: the GUI Layers panel prints
                                   a small label (`ui_text::
                                   layers_auto_managed`) whenever the count
                                   is nonzero, and CLI `list-layers` counts
                                   it and, when nonzero, prints an stderr
                                   note naming `--print-state` as the way
                                   to see what a printing/aggregating
                                   application would use instead.
                                   Deliberately EXCLUDED from
                                   `LayerDiagnostics::is_faithful()` — the
                                   file is a faithful transcription either
                                   way; the disclosure is about a state that
                                   moves, not a defect in reading it. See
                                   §12's 2026-08-10 (eighty-third filing)
                                   entry for the full ruling.
                                   **`shading.rs` (`Pass 85.0`, two
                                   slices — `33ea830` model, `9839d6f`
                                   paint — 2026-08-17; §12 decision
                                   065):** resolves a shading
                                   dictionary, classifies by
                                   `ShadingType`, loads `/ColorSpace`
                                   and `/Function` (both §8.7.4.4
                                   arities), pre-samples a 256-entry
                                   colour ramp. Axial (type 2) and
                                   radial (type 3) paint per-pixel into
                                   the clip region, anchored to current
                                   user space per Table 77. Radial
                                   circles paint on their CIRCUMFERENCE
                                   (`|P−c(s)|=r(s)`), never as filled
                                   discs — decision 065 records why the
                                   disc reading (ISO 32000-1's "within")
                                   cannot be right regardless of
                                   edition. Function-based (type 1) is
                                   modelled — its colour ramp resolves —
                                   but not painted; mesh types 4–7
                                   (`Pass 85.1`) are not yet ingested
                                   from spec. `sh` moved out of the
                                   anonymous `MP`/`DP`/`d0`/`d1`
                                   deferred-op bucket into six named
                                   counters (`shadings`/
                                   `shadings_via_sh`/
                                   `shadings_paintable`/
                                   `shadings_painted`/`shadings_refused`/
                                   `shadings_mesh`) plus a per-
                                   `ShadingType` breakdown, landed in
                                   the SAME change that added the
                                   module (`33ea830`) — `Pass 84.0`'s
                                   own lesson (counters computed with no
                                   shell reading them) applied
                                   immediately rather than repeated.
                                   **Page-group compositing + blend
                                   modes (`Pass 90.1`, `bd244d9`,
                                   2026-08-17; §12 decision 066):** two
                                   corrections against the SAME §11.3/
                                   §11.4 machinery `Pass 90.0` had only
                                   counted. (1) The page pixmap is no
                                   longer filled opaque white before
                                   painting — it composites as an
                                   ISOLATED group over a fully
                                   transparent backdrop, flattened to
                                   white ONCE at the end
                                   (§11.4.7's `Composite` +
                                   `(1−ag)·W+ag·Cg`), because filling
                                   white hands every blend function
                                   `cb=1.0` and only four of Table 136's
                                   sixteen modes satisfy `B(1.0,cs)=cs`.
                                   (2) `GraphicsState::blend_mode`
                                   (saved/restored by `q`/`Q` for free)
                                   and `gstate::blend_mode_from_name`
                                   (Table 136/137, single mapping point)
                                   are threaded to every paint site
                                   including image painting, which had
                                   `BlendMode::SourceOver` hard-coded.
                                   The four non-separable modes (Hue/
                                   Saturation/Color/Luminosity) are
                                   measured wrong in tiny-skia 0.11.4
                                   and REFUSED rather than mapped —
                                   decision 066 records the general
                                   dependency-verification policy this
                                   refusal instantiates; full write-up
                                   at `D:\dev\rag\rust\tiny_skia_0.11_non_separable_blend_modes_wrong_by_up_to_107_255.md`.
                                   **★★ THE "REFUSED" CLAUSE IS FALSE
                                   FROM 2026-08-19 (`Pass 85.4b`,
                                   `972ddbb`, hundred-and-eighty-ninth
                                   filing) AND IS KEPT ONLY AS HISTORY.**
                                   The four modes now ship: Table 137 is
                                   transcribed into
                                   `pdfcer-render/src/blend_nonsep.rs` and
                                   applied both at a paint and — the half
                                   that actually moves the suite — at a
                                   transparency group's composited result
                                   through `Canvas::layer`, because every
                                   non-separable mode in that corpus sits
                                   at a `Do` and none at a paint. **The suite
                                   25 → 26 pass, 15 → 14 FAIL of 51.**
                                   **The dependency defect is unchanged
                                   and tiny-skia is still not routed these
                                   four** — decision 066 is AMENDED, not
                                   reversed: it refused trusting the
                                   dependency, never the feature, and its
                                   verification bar is what produced the
                                   independent-oracle check that validated
                                   the replacement. See §12, decision 066's
                                   dated amendment.
                                   `/Group` is now read on form
                                   XObjects (previously not at all).
                                   **Transparency GROUP compositing
                                   (`Pass 85.4c`, `0d6f4ac`, 2026-08-17;
                                   §12 decision 068):** a group renders
                                   into its OWN page-sized offscreen
                                   buffer, with the graphics state RESET
                                   to initial (`Normal` blend, alpha 1.0)
                                   for the group's contents — the outer
                                   blend mode/constant alpha/soft mask
                                   belong to the group's RESULT once
                                   composited (§11.4.5), not to the
                                   objects inside it — then composites as
                                   a unit via `Pixmap::draw_pixmap`
                                   carrying the outer blend mode and
                                   alpha. **★ CORRECTED same day (`Pass
                                   85.4d`, `b15d7ff`; decision 068
                                   amended, sub-decision 3): the buffer
                                   is NOT taken unconditionally.** A
                                   fresh buffer starts fully transparent
                                   — ISOLATED semantics (§11.4.7) — but
                                   `/I` defaults FALSE, so most groups
                                   are non-isolated and must blend
                                   against the PAGE's accumulated
                                   backdrop, which unconditional
                                   buffering got wrong (878 ms measured
                                   on the suite's page 2 vs. a 230 ms
                                   pre-`85.4c` baseline, 142 buffers,
                                   found by asking why the per-buffer
                                   cost was what it was). The buffer is
                                   now taken only when the outer blend
                                   mode is non-Normal or outer alpha < 1
                                   (the composited RESULT must exist
                                   before the outer state can apply to
                                   it), or the group is itself isolated
                                   (`/I true`); otherwise the group's
                                   contents paint INLINE, which for a
                                   non-isolated group under neutral outer
                                   state is §11.4.5's exact answer, not
                                   an approximation of it, reached
                                   cheaper. **Page-sized rather than
                                   BBox-sized, deliberately** (decision
                                   068): the contents draw under the SAME
                                   CTM as the page, so no per-group
                                   coordinate translation is threaded
                                   through paint sites or the clip mask;
                                   cost is ~4 bytes/pixel/nesting-level
                                   for groups that DO buffer, and a
                                   misalignment bug would read as a
                                   visible rendering artefact rather than
                                   fail silently. `groups_composited` and
                                   `groups_knockout_approx` are counted
                                   AND PRINTED on `render-page`'s stable
                                   line; `groups_flattened` survives only
                                   as the allocation-failure fallback — a
                                   non-isolated group taking the inline
                                   fast path under neutral outer state
                                   counts as `groups_composited`, because
                                   that IS §11.4.5's result for it, not a
                                   flattening approximation. The suite's
                                   PCS 16.0 (non-knockout blend-mode panel)
                                   renders CLEAN on both the original fix
                                   and the correction; `groups_flattened`
                                   on that file 187 → 0, unaffected by
                                   the condition correction. **NOT
                                   correctly composited: `/K` knockout
                                   groups** (§11.4.6 — each element
                                   should composite against the group's
                                   INITIAL backdrop, not the accumulated
                                   result; pdfcer composites them as
                                   ordinary groups today, approximated
                                   and counted via
                                   `groups_knockout_approx` (47) rather
                                   than silently wrong — PCS 16.1 still
                                   shows its crosses, matching the
                                   counter). `/SMask` soft-mask groups
                                   (36 occurrences, suite) remain
                                   entirely unread. Both are blocked on
                                   a `pdfcer-spec-librarian` dispatch for
                                   §11.4.6/§11.5.2–.3, in flight as of
                                   this filing — deliberately not
                                   implemented from training-data recall
                                   (project rule 1), after the spec
                                   corpus itself flagged §11.4.6 as a GAP
                                   naming shape/opacity separation as a
                                   property a single-alpha buffer model
                                   may not be able to represent at all.
                                   Now this project's top-priority suite
                                   render-fidelity gap, narrowed from
                                   "all group compositing" to "knockout
                                   groups + soft masks" (`ROADMAP.md`
                                   *Next up*). **★ CORRECTED (`Pass
                                   85.4e`, `fee42e8`, 2026-08-17; decision
                                   068 amended again, sub-decision 4):
                                   the §11.4.6/§11.5.2–.3 spec dispatch
                                   landed and fixed two defects this
                                   entry's own citations and buffering
                                   condition had.** Citation fix: `/K`,
                                   `/I`, `/CS` are Table **147**, not 96
                                   (Table 96 is the COMMON group-
                                   attributes table every subtype
                                   shares); Table 147 in ISO 32000-1 is
                                   Table 145 in ISO 32000-2. Buffering
                                   fix: knockout groups now buffer
                                   UNCONDITIONALLY — `85.4d`'s
                                   outer-state-neutrality test was never
                                   meant to gate knockout at all, since a
                                   knockout group has no initial
                                   backdrop to composite against when
                                   painted inline, isolated or not; a
                                   knockout group under a neutral outer
                                   state had still been taking the
                                   inline fast path built for the
                                   non-isolated/non-knockout majority.
                                   **The dispatch also reframes the
                                   population this gap covers**, far
                                   beyond explicit `/K true`: §9.3.8's
                                   `/TK` defaults `true` (every text
                                   object is knockout by default),
                                   §11.7.4.4 makes `B`/`B*`/`b`/`b*` and
                                   text render modes 2/6 knockout (its
                                   own NOTE 2 names the double-border
                                   symptom on a semi-transparent
                                   fill-then-stroke), and §11.6.7 makes
                                   shading patterns knockout — no `/K`
                                   key needed for any of the three.
                                   **Still not correctly composited**:
                                   the buffer this fix guarantees is the
                                   CONTAINER only; §11.4.6's actual
                                   per-element-against-initial-backdrop
                                   compositing rule remains unimplemented,
                                   `groups_knockout_approx` still counts
                                   every knockout group as approximated.
                                   **Representability is now known**:
                                   isolated knockout is bit-exact
                                   representable in this single
                                   premultiplied-alpha buffer; non-
                                   isolated knockout (the common case,
                                   `/K` defaulting false the same way
                                   `/I` does) is not, pending buffer-
                                   model work. Full derivation:
                                   `ROADMAP.md`'s `Pass 85.4e` Shipped
                                   entry.
                                   **★★ SOFT MASKS SHIP 2026-08-18 (`cb20770`, hundred-and-
                                   sixty-seventh filing; decision 070) — THE "REMAIN ENTIRELY
                                   UNREAD" CLAUSE ABOVE IS NOW FALSE AND IS KEPT ONLY AS
                                   HISTORY.** `ExtGState /SMask` `/Alpha` and `/Luminosity`
                                   mask groups are BUILT and APPLIED (§11.6.5), multiplied into
                                   the clip so every existing paint site honours them. `/TR` is
                                   read, counted and disclosed but NOT evaluated
                                   (`soft_mask_tr_ignored`) — it is where a mask gets inverted,
                                   so an ignored one can leave visible exactly what a document
                                   meant to hide. **The knockout half of this cell is
                                   UNCHANGED**, and soft masks now share its remaining defect
                                   rather than being a separate gap: folding into the clip
                                   attenuates each element INSIDE the scope, whereas §11.4.5
                                   applies the mask to the group's RESULT. Construction is
                                   correct (mask groups and folded clips dumped to PNG, both
                                   correct soft gradients); application is not. Strip
                                   correlation moved on all three measurable suite soft-mask
                                   patches and NONE passes — suite standing UNCHANGED at
                                   25/18/8 of 51. Full derivation: decision 070 in §12, and
                                   `ROADMAP.md`'s `cb20770` Shipped entry.
                                   **★ FIXED 2026-08-18 (hundred-and-
                                   sixty-eighth filing, `75fa497`) — a
                                   routing bug this same commit-set
                                   introduced is closed.** `/BC` was
                                   converting to sRGB through an inline
                                   naive complement while painted content
                                   inside the same mask group used the
                                   calibrated route above, so a
                                   `DeviceCMYK` mask disagreed with itself
                                   across its own bounding box. `/BC` now
                                   takes the same `Rgb::from_cmyk` route
                                   for all three component counts, pinned
                                   by a test that asserts the two routes
                                   genuinely diverge for at least one input
                                   (`color.rs`,
                                   `a_soft_mask_backdrop_converts_by_the_painted_content_route`).
                                   Does not change this cell's remaining
                                   defect (mask applied per-element, not to
                                   the group's RESULT) or `LUM-A1` (still
                                   open — a different question, which
                                   CMYK→luminosity FORM, not which
                                   CMYK→sRGB ROUTE two call sites use).
                                   Full derivation: `ROADMAP.md`'s
                                   `75fa497` Shipped entry.
                                   **★ AMENDED 2026-08-18
                                   (hundred-and-sixty-sixth filing, `Pass
                                   85.5`, `bd9d5ef`+`bf75351`+`ac15158`) —
                                   THE PARAGRAPH BELOW IS KEPT VERBATIM
                                   AND ITS OBLIGATION (1) IS NOW HALF
                                   WRONG. OVERPRINT SIMULATION SHIPPED
                                   WITHOUT THIS BUFFER, WITHOUT `iccce`,
                                   AND WITHOUT A REPLY TO
                                   `request_cmyk_buffer_destination_and_width.md`.**
                                   It reads "overprint is inexpressible in
                                   an RGB buffer, not merely unimplemented
                                   … an RGB buffer has no CMYK components
                                   to select between." **True of a
                                   PERMANENT CMYK pipeline; false of a
                                   PER-PAINT one.**
                                   `pdfcer-render/src/overprint.rs` holds
                                   Table 149 as pure logic; the paint path
                                   rasterises an overprinting paint to a
                                   coverage mask with the SAME rasteriser
                                   a normal paint uses, reconstructs the
                                   backdrop's CMYK from the composited
                                   RGB, blends per pixel, writes back.
                                   **It works because
                                   `rgb_to_cmyk`/`cmyk_to_rgb` are EXACT
                                   inverses (4,913 colours, 1e-5) and
                                   because each output channel depends on
                                   a DISJOINT pair of inputs**, so
                                   overprinting one ink moves exactly one
                                   channel and leaves the rest at their
                                   round-tripped — hence original —
                                   values. The split is not recovered
                                   (`C=.5 M=.4 Y=.4 K=0` reads back
                                   `C=.167 M=0 Y=0 K=.4`) and does not
                                   need to be. **Named limit:** two inks
                                   overprinting in sequence over a rich
                                   backdrop can differ from a true
                                   separated pipeline. **Measured: the suite
                                   22 → 25 of 51 patches.** **What the
                                   amendment does NOT touch:** obligation
                                   (2) — blending IN a `DeviceCMYK`
                                   blending colour space — is untouched
                                   and still needs the buffer, the `iccce`
                                   boundary (decision 064) still owns the
                                   final conversion, and the cost figures
                                   below still stand. **What the remaining
                                   7 overprint FAILs need is a
                                   per-COLORANT buffer, not a CMYK one:**
                                   Table 149's SPOT row preserves the
                                   backdrop in both modes, and a
                                   flattened-RGB backdrop cannot say
                                   whether it was spot or process.
                                   **★ AMENDED 2026-08-28 (301st
                                   filing) — the CONCLUSION stands and
                                   the stated MECHANISM has gone stale.**
                                   Since `Pass 97.1e` a subtractive page
                                   composites in four `f32` colorant
                                   planes, so the backdrop is no longer
                                   "flattened RGB" — and it still cannot
                                   say whether the ink was spot or
                                   process, because a `Separation` paint
                                   goes through its TINT TRANSFORM into
                                   those same four planes. Read the
                                   sentence above as *a four-PROCESS-plane
                                   backdrop cannot say*. **Two further
                                   facts, measured 2026-08-28 and filed
                                   to *Backlog* before any code:** Table
                                   149's spot-component RULES are already
                                   written, doc-commented and unit-tested
                                   but **UNREACHABLE** (`Component::Spot`
                                   is only ever matched, never
                                   constructed outside tests;
                                   `cmyk_group_rules` returns four rules;
                                   `CmykBuffer` holds four planes) — so
                                   the work here is **the planes and a
                                   caller, not the rules**; and
                                   `Pass 143.0` (`DeviceGray` over a spot
                                   backdrop) is **NOT blocked on any of
                                   it**, because the spot ink is already
                                   in the four planes by paint time.
                                   **The cheap approximation was BUILT AND
                                   MEASURED: a page-sized spot-ink
                                   multiplier plate moved 17 traps to 16,
                                   flipped 0 patches of 51, regressed one
                                   patch 3 → 6 unexplained, and was
                                   reverted (`ac15158`).** See decision
                                   069 and `ROADMAP.md`'s `85.5` row.
                                   **The durable lesson is about THIS
                                   DOCUMENT, not about overprint: a
                                   blocker stated confidently went
                                   unretested for as long as it was
                                   written down, and it took someone
                                   trying it to find out.**
                                   **PLANNED, NOT DECIDED, NOT STARTED —
                                   a CMYK+alpha compositing buffer
                                   (recorded 2026-08-18, hundred-and-
                                   sixty-third filing, because it was
                                   present in `ROADMAP.md` twice and
                                   `FEATURES.md` once and absent from
                                   this document — the one whose job is
                                   the logic).** Two spec obligations
                                   converge on the SAME buffer and are
                                   why this is filed as one item, not
                                   two. **(1) Overprint is inexpressible
                                   in an RGB buffer, not merely
                                   unimplemented.** §11.7.4.3's
                                   `CompatibleOverprint` blend selects
                                   PER COMPONENT — "the value of the
                                   blend function shall be the source
                                   component c_s for any process
                                   (DeviceCMYK) colour component whose
                                   … value is nonzero; otherwise … the
                                   backdrop component c_b" — and an RGB
                                   buffer has no CMYK components to
                                   select between. `Pass 85.5`
                                   (`ROADMAP.md` *Next up*) is gated on
                                   this. **(2) The SAME buffer is also
                                   where blending happens correctly at
                                   all when a group's blending colour
                                   space is DeviceCMYK** — §11.3/§11.4
                                   define blending IN the group's BCS,
                                   and the suite corpus's own patches
                                   declare `TBCS: DeviceCMYK`; pdfcer
                                   blends in RGB today (decision 068),
                                   so `Difference`/`Exclusion` and the
                                   non-separable modes are computed in
                                   the wrong space wherever the BCS is
                                   CMYK, independent of overprint
                                   entirely. **Shape, as currently
                                   understood:** a CMYK+alpha buffer for
                                   the page group when the blending
                                   colour space is DeviceCMYK, page-sized
                                   like decision 068's RGBA group buffer
                                   (same reasoning: no per-group
                                   coordinate translation); blend modes
                                   and `CompatibleOverprint` run IN that
                                   buffer; ONE conversion to display RGB
                                   at the end. **The `iccce`/pdfce
                                   boundary (decision 064) already
                                   applies: iccce owns the final CMYK→RGB
                                   conversion, pdfcer owns everything that
                                   happens inside the buffer before that
                                   conversion runs** (overprint,
                                   knockout, blend-mode selection — none
                                   of it is iccce's). **Cost, measured
                                   (iccce's own bench, 2026-08-12,
                                   variance-noted): 1.29–1.48 Mpix/s for
                                   iccce's compiled transform.** An A4
                                   page at 150 DPI is 2.1 Mpix ⇒ **≈1.5 s
                                   over 2.1 Mpix = ~0.7 µs/pixel**; at
                                   300 DPI, 8.4 Mpix ⇒ **≈6 s over
                                   8.4 Mpix, same per-pixel rate**.
                                   pdfcer renders a full suite page in
                                   ~0.6 s today, so this conversion alone
                                   would cost 2.5×–10× the entire render
                                   — fine for export, too slow for
                                   interactive preview, which is the
                                   reason an f32/u8 buffer surface
                                   (not f64) has been asked of iccce
                                   rather than assumed: f64 at 8.4 Mpix
                                   is 268 MB in / 201 MB out per page.
                                   **What is NOT conformance-driven, and
                                   is therefore a product decision, not a
                                   spec one:** §8.6.7 says "if
                                   overprinting is not supported, the
                                   value of the overprint parameter shall
                                   be ignored" — pdfcer is CONFORMANT
                                   TODAY without this buffer — and
                                   ISO 32000-1 never describes overprint
                                   PREVIEW on a non-separating device (0
                                   hits in 756 pages of corpus). The
                                   justification for building it anyway
                                   is Acrobat-parity: Acrobat enables
                                   Overprint Preview automatically for
                                   PDF/X, so a PDF/X-4 file's EXPECTED
                                   on-screen appearance includes it.
                                   **No decision-log entry minted for
                                   this** (§12 stays at decision 068,
                                   next free 069) — nothing here has been
                                   CHOSEN yet in the sense §12 records:
                                   no buffer type has been committed to
                                   code, no tradeoff between the shapes
                                   above has been picked over an
                                   alternative, and decision 068's own
                                   page-sized-buffer choice was corrected
                                   the SAME DAY it was made (`Pass
                                   85.4d`) once real measurement arrived
                                   — recording a CMYK equivalent as
                                   DECIDED before a line of it exists
                                   would very plausibly need the same
                                   kind of correction. This paragraph is
                                   the planned-direction record §2/§3
                                   exist for; `ROADMAP.md` `Pass 85.5`
                                   carries the Pass; decision 064 already
                                   carries the iccce boundary this plan
                                   sits inside.
                                   **★ FORWARD POINTER, added 2026-08-18
                                   (pdfcer-librarian, hundred-and-
                                   seventy-fifth filing) — the "iccce
                                   owns the final CMYK→RGB conversion"
                                   half of this plan now has a concrete
                                   call, not just a boundary.**
                                   `Chain::with_destination(&src,
                                   Destination::None, intent)` —
                                   iccce's built-in sRGB destination,
                                   constructed from published BT.709-6/
                                   W3C/Bradford-D50-PCS constants, no
                                   shipped `.icc` — was already shipped
                                   when the cost figures above were
                                   measured; iccce's own reply
                                   (`reply_cmyk_buffer_destination_and_
                                   width.md`, 2026-08-18) corrects a
                                   stale statement of its own that had
                                   said otherwise. **Does not change
                                   this paragraph's cost figures or its
                                   NOT DECIDED/NOT STARTED status** — the
                                   buffer this call will eventually sit
                                   inside still does not exist. **Does
                                   change what "gated on iccce" means
                                   going forward:** the gate was never on
                                   iccce having *a* destination, only on
                                   this buffer being built; full
                                   correction and the `Destination::None`
                                   safety obligation: `ROADMAP.md`'s
                                   `Pass 85.5` gap-inventory and
                                   `iccce`-coordination Backlog entries,
                                   both corrected this filing.
                                   **★★★ AMENDED 2026-08-21
                                   (two-hundred-and-sixteenth filing,
                                   `Pass 97.0`,
                                   `7160819`/`9b49ca0`/`86a7b70`;
                                   decision 077) — THREE OF THIS CELL'S
                                   STANDING CLAIMS ARE NOW FALSE AND ARE
                                   KEPT ABOVE ONLY AS HISTORY.**
                                   **(1)** *"pdfcer composites them as
                                   ordinary groups today, approximated
                                   and counted via
                                   `groups_knockout_approx` (47)"* and
                                   *"`groups_knockout_approx` still
                                   counts every knockout group as
                                   approximated"* — **§11.4.6 knockout
                                   is IMPLEMENTED.** `KnockoutTarget` in
                                   `canvas.rs` carries **four planes
                                   where a `Pixmap` has one**: frozen
                                   initial backdrop, running result,
                                   `α_g`, `f_g`. Each element is
                                   rasterised into a reused scratch at
                                   full opacity so its alpha returns as
                                   **pure coverage `f_s`**, with `q_s`
                                   taken out of the paint — because
                                   §11.4.8 scales the destination by
                                   `(1 − f_s)` where the ordinary
                                   formula has `(1 − α_s)`. §11.4.6
                                   NOTE 6's nesting rule is honoured: a
                                   non-isolated group inside a knockout
                                   group inherits the **OUTER** group's
                                   initial backdrop. Suite `PCS1_161`
                                   **14 traps → 2** (2/16 cells correct
                                   → 14/16); `PCS2_120`
                                   still passes; no patch regressed.
                                   **(2)** *"non-isolated knockout (the
                                   common case) is NOT representable,
                                   pending buffer-model work"* — **a
                                   STATED BLOCKER, FALSIFIED BY BUILDING
                                   IT.** It was true of the single
                                   premultiplied-alpha `Pixmap` the
                                   claim was made about, and the claim
                                   silently generalised from *that
                                   buffer* to *any buffer pdfcer could
                                   have*. Same shape as `Pass 85.5`'s
                                   `iccce` gate and `85.4b`'s "needs
                                   `Pass 97.0`'s buffer": **three
                                   recorded blockers in this project
                                   now, each retired by someone trying
                                   it.** The general lesson is `R199`'s
                                   — a recorded blocker is a dated
                                   reading, not a standing fact — and
                                   this instance adds the sharper half:
                                   **a representability claim is a claim
                                   about a NAMED data structure, and
                                   naming the structure is what stops it
                                   generalising.**
                                   **(3)** *"the mask applied
                                   per-element, not to the group's
                                   RESULT"* — **fixed** (`Pass 97.0d`).
                                   The mask is lifted out of the
                                   contents' clip at a group `Do` and
                                   applied **once** to the composite;
                                   folding into the clip remains correct
                                   for an **elementary** object
                                   (§11.6.4.1 makes the mask value that
                                   object's `q_m`) and multiplies **once
                                   per object** inside a group, so the
                                   error grew as `M^n` over `n`
                                   overlapping objects and was invisible
                                   on single-object fixtures.
                                   Reference-strip correlation
                                   `PCS1_1610` 0.576 → **0.962**,
                                   `PCS1_168` 0.725 → **0.978**,
                                   `PCS1_169` 0.905 → **0.986** (mean
                                   over three, 0.735 → 0.975). Counter
                                   `soft_masks_on_group_result`; the one
                                   unliftable case (a `W n` clip between
                                   the `gs` and the `Do`) keeps the old
                                   behaviour on the **existing**
                                   `soft_masks_reset_stale` rather than
                                   getting a third name for one
                                   condition, and reads **zero** on all
                                   three patches.
                                   **UNCHANGED AND NOW THE LOAD-BEARING
                                   GAP:** this cell's `DeviceCMYK`
                                   **blending**-space obligation, stated
                                   in the paragraph above as *"(2) the
                                   SAME buffer is also where blending
                                   happens correctly at all when a
                                   group's blending colour space is
                                   DeviceCMYK"*. **That paragraph is
                                   correct and is now the whole
                                   remainder.** `Pass 97.0` corrected
                                   every group-model mechanism it set
                                   out to correct and **the suite board
                                   did not move — 26 pass · 14 FAIL · 11
                                   UNRESOLVED of 51, before and after**
                                   (trap count 67 → 55, all 12 of them
                                   `PCS1_161`'s). Derived by hand on
                                   `PCS1_162`'s `Difference` cell:
                                   complement, `|cb′ − cs′|`, complement
                                   back = `DeviceCMYK 1 0 1 0`, the
                                   green the trap surround requires;
                                   pdfcer renders `(237,1,140)`, **pdfium
                                   `(202,29,108)` — both blend in RGB
                                   and both are wrong, differently**, so
                                   pdfium is a **peer, not an oracle**,
                                   for anything §11.3.4 governs. **Every
                                   suite transparency patch declares
                                   `/Group /CS /DeviceCMYK` on the
                                   PAGE**, including `PCS3_161` whose own
                                   objects are `ICCBased` RGB. Full
                                   derivation:
                                   `docs/compositor-plan.md`'s head
                                   amendment (`:8–179`, the plan of
                                   record) and `ROADMAP.md`'s
                                   `7160819`/`9b49ca0`/`86a7b70` Shipped
                                   entry.
                                   **★ UPDATED 2026-08-21 (`cbb1ede`,
                                   `90739d7` — `Pass 97.1c`/`97.1d`,
                                   decision 078): THE GAP IS NOW READ,
                                   MEASURED AND DISCLOSED, AND STILL
                                   NOT CLOSED.** `page_blend_space`
                                   (§11.4.7) and `do_form` resolve the
                                   space, honouring **Table 147's rule
                                   that a NON-ISOLATED group ignores its
                                   own `/CS` and inherits the parent's**
                                   — which is *why* `PCS3_161` blends in
                                   `DeviceCMYK`, and which discharged
                                   that patch's "unconfirmed" flag by
                                   counter (**15 of its 15 blends
                                   wrong**). §11.3.4's arithmetic
                                   (`BlendSpace`,
                                   `Blend::apply_subtractive`,
                                   `PixelCmyk`) is written and tested
                                   but **unwired**. Counters
                                   `blend_space_subtractive` (census)
                                   and `blends_in_wrong_space`
                                   (shortfall) ship on the CLI's stable
                                   line. **Measured: the suite 107/107
                                   blends wrong (100.0 %, 13 of 51
                                   files); `fixtures/external` 2/49
                                   (4.1 %, 15 of 3,735 rendered), both
                                   hits being veraPDF transparency
                                   conformance fixtures.** The colorant
                                   buffer is the only missing piece and
                                   is a **conformance** deliverable, not
                                   an appearance one.
                                   **STILL NOT DONE, so this cell is not
                                   closed:** only explicit `/K true`
                                   groups are treated as knockout — the
                                   three larger tiers this cell itself
                                   enumerates (§9.3.8 `/TK`, §11.7.4.4
                                   `B`/`b`, §11.6.7 shading patterns)
                                   are untouched; `f_g` is approximated
                                   by `α_g` for a group used as an
                                   *element* of a knockout group (exact
                                   whenever that group's own elements
                                   are opaque); `/AIS` is not
                                   distinguished; and **`/TR` is still
                                   read, counted
                                   (`soft_mask_tr_ignored`) and NOT
                                   evaluated**, unchanged.
                                   **★★★ AMENDED 2026-08-21
                                   (two-hundred-and-twenty-fourth filing,
                                   `Pass 97.1e` + `Pass 97.1f`, `a277931` +
                                   `ff4b4bf`; decision 079) — THE BUFFER
                                   THIS CELL PLANNED IS BUILT, AND FOUR OF
                                   ITS STANDING CLAIMS ARE NOW FALSE. THEY
                                   ARE KEPT ABOVE AS HISTORY.**
                                   **(1)** *"PLANNED, NOT DECIDED, NOT
                                   STARTED — a CMYK+alpha compositing
                                   buffer"* — **SHIPPED**, and it is
                                   **not** CMYK+alpha: it is **four
                                   plane-major `f32` colorant planes plus
                                   alpha**, and — after `97.1f` — four more
                                   for knockout, carrying shape `f_g` and
                                   alpha `α_g` on **SEPARATE** planes
                                   because §11.4.8 reads shape where
                                   §11.4.4 reads alpha.
                                   `crates/pdfcer-render/src/cmyk_buffer.rs`
                                   + `cmyk_paint.rs`; knockout arithmetic
                                   in
                                   `compositor::composite_element_knockout_cmyk`
                                   / `remove_backdrop_cmyk`.
                                   **(2)** *"pdfcer blends in RGB today
                                   (decision 068), so
                                   `Difference`/`Exclusion` and the
                                   non-separable modes are computed in the
                                   wrong space wherever the BCS is CMYK"* —
                                   **FALSE on a subtractive page since
                                   `a277931`.** The census went **107 of
                                   107 wrong → 0 of 107** on the
                                   suite. It remains **true on an additive
                                   page and that is correct, not a
                                   shortfall**: §8.6.6.4 makes reverting to
                                   the alternate space the **specified**
                                   behaviour on an additive device
                                   (decision 079).
                                   **(3)** *"(2) the SAME buffer is also
                                   where blending happens correctly at all
                                   … That paragraph is correct and is now
                                   the whole remainder"* — **discharged.**
                                   The remainder is now the
                                   **spot/n-channel** half only: **every
                                   remaining suite FAIL is an overprint,
                                   spot or ICC patch, and not one is a
                                   blending-space failure.** The suite **26 →
                                   29 pass of 51**, trap marks **55 → 41**.
                                   ★ **CORRECTED 2026-08-21 (225th
                                   filing): BOTH LEVELS ARE
                                   OVER-COUNTS — `tools/suite-check.py`
                                   implements one of the suite's two
                                   pass criteria and has reported
                                   `clean` for the other since it was
                                   written (seven patches). Corrected
                                   standing: **26 pass of 51 at
                                   minimum**. The DELTA (+3) and the
                                   trap counts are unaffected; the
                                   levels are not. See `ROADMAP.md`'s
                                   *suite standing board* and
                                   `docs/suite-operator-review-2026-08-21.md`.**
                                   **(4)** *"the `iccce` boundary (decision
                                   064) still owns the final conversion"* —
                                   **unchanged as a BOUNDARY and no longer
                                   a BLOCKER.** §11.4.7's collapse ships
                                   against pdfcer's own conversion tables
                                   today. Two conversions exist and **they
                                   are for different jobs**: a
                                   **calibrated** lattice for the TERMINAL
                                   conversion, and an **exactly
                                   invertible** max-GCR pair for a ROUND
                                   TRIP. Using the accurate one on a round
                                   trip left `PCS1_161` at **10 trap
                                   marks**; the invertible one took it to
                                   **4**. When `iccce` lands it replaces
                                   the terminal one, not both.
                                   **Cost figures above are UNAFFECTED and
                                   untested against the shipped buffer** —
                                   they price *iccce's* transform, which is
                                   not what runs today. No new performance
                                   measurement was taken this session; do
                                   not read the ship as a measurement of
                                   the cost paragraph.
                                   **STILL NOT DONE, so this cell is still
                                   not closed:** a **non-isolated ORDINARY
                                   group on a subtractive page is
                                   composited as if isolated** (backdrop
                                   dropped, §11.4.4's removal skipped,
                                   counted `cmyk_groups_approximated`) —
                                   the arithmetic exists, the **second
                                   content walk** does not; **images with
                                   NO INK to keep bridge through sRGB**
                                   (`cmyk_bridged_pixels`,
                                   `cmyk_unbridged_images`) — ★ **amended
                                   2026-08-26 (`Pass 130.1`, `5dd4083`);
                                   this clause read "**images and shadings
                                   bridge through sRGB**" and that is now
                                   FALSE for a `DeviceCMYK` image, direct
                                   or behind an `/Indexed` base, which
                                   composites its authored colorants with
                                   no conversion in either direction and is
                                   counted on the sixth counter,
                                   `cmyk_native_image_pixels`**; **spot
                                   colorants are still flattened** (four
                                   planes, not runtime `N`); and everything
                                   the previous amendment listed — implicit
                                   knockout (§9.3.8 `/TK`, §11.7.4.4
                                   `B`/`b`, §11.6.7 shading patterns),
                                   `/AIS`, and **`/TR` still read, counted
                                   and NOT evaluated** — is **unchanged**.
                                   ★★ **AMENDED AGAIN 2026-08-27
                                   (`Pass 137.0` `523ca6d` +
                                   `Pass 137.1` `d1ce4ac`) — this clause
                                   read "MESH shadings, and images with NO
                                   INK to keep, bridge through sRGB", and
                                   the MESH half is now FALSE too.**
                                   `Pass 137.0` widened the analytic native
                                   route to run regardless of overprint
                                   state; `Pass 137.1` gave a mesh its own
                                   colorant carrier (`Shade::Ink`,
                                   `MeshColorants`) because a mesh has no
                                   `ColorRamp` for that widening to reach.
                                   Only images (and meshes) with genuinely
                                   **no ink to keep** — an additive colour
                                   space — still bridge; a
                                   `DeviceCMYK`-direct source under
                                   `/OPM 1` overprint is a separate,
                                   narrower exclusion (Table 149's
                                   value-dependent row), unchanged by
                                   either Pass.
                                   ★★★ **AMENDED A THIRD TIME 2026-08-27**
                                   (`Pass 140.0` + `Pass 140.1`, `70c5919`)
                                   — **the clause above was FALSE when it
                                   was written, and this Pass is what made
                                   it true.** A `Separation`/`DeviceN` image
                                   had ink and bridged anyway, because it
                                   resolved through its tint transform to
                                   sRGB and never to its `DeviceCMYK`
                                   alternate; so did an `/Indexed` duotone
                                   over such a base, and so did a path
                                   FILL of the same colour. All three now
                                   keep their ink (`Space::to_cmyk`,
                                   `Space::yields_cmyk`,
                                   `Interpreter::authored_cmyk`). ★ The
                                   round trip they took is not the identity:
                                   the outbound leg is `Rgb::from_cmyk`
                                   (calibrated, carries a rendering intent)
                                   and the return leg is
                                   `overprint::rgb_to_cmyk` (naive
                                   maximum-GCR, the exact inverse of a
                                   DIFFERENT function, `cmyk_to_rgb`).
                                   ★★ Note the shape rather than only the
                                   correction: this clause has now been
                                   amended three times and was wrong in the
                                   interval after each of the first two — a
                                   sentence that enumerates a POPULATION
                                   decays whenever the population changes,
                                   and nothing compiles it.
    pdfcer-print\                 <- Printing: job planning + spooling. Shipped with
                                   `Pass 55.2` (2026-08-10) but never documented in this
                                   tree until the eighty-fifth filing — a filing gap this
                                   entry closes. Depends on NEITHER pdfcer-core NOR
                                   pdfcer-render — a printing crate that also rendered
                                   would need the whole render stack to be testable for
                                   failures (a wrong DEVMODE, an upside-down DIB, a job
                                   left open) that have nothing to do with PDF content.
                                   Rasterization stays in the CALLING SHELL (via
                                   pdfcer-render); this crate only plans placement/
                                   resolution and, on Windows, spools bytes to a real
                                   device. **The planning arithmetic (`plan_job`,
                                   `job_resolution`, `imposition::{plan_n_up,
                                   plan_booklet, plan_poster}`) takes a PLATFORM-FREE
                                   `DeviceGeometry` (dpi + printable area), never
                                   `PrinterCaps` directly** — `PrinterCaps` is
                                   `cfg(windows)` (a real Win32 driver's report); taking
                                   it as the planner's input would have moved the most
                                   test-worthy code in the crate (six tests: render-scale
                                   folding, an asymmetric-resolution device rendering at
                                   its smaller axis, an out-of-range page skipped not
                                   refused) behind a `cfg` the Linux/macOS CI jobs never
                                   build — green locally, silently uncovered on every
                                   other platform. **★ CORRECTED 2026-08-11 (`Pass
                                   64.0`, decision 041) — `DeviceGeometry:
                                   From<&PrinterCaps>` is REMOVED.** That infallible
                                   `From` impl had no orientation parameter, so it
                                   always read `PrinterCaps`' printable area BEFORE any
                                   `DEVMODE` existed — the device's own DEFAULT
                                   orientation, portrait on nearly every real printer —
                                   and `plan_job` never saw the operator's orientation
                                   choice at all: every LANDSCAPE job was planned
                                   against the PORTRAIT sheet (measured: scale 0.727
                                   where correct is 0.941). The only route from
                                   `PrinterCaps` to `DeviceGeometry` is now
                                   `DeviceGeometry::from_caps(caps, requested,
                                   first_page_pt)`, which cannot be called without
                                   stating the requested orientation; the un-rotated
                                   view is unreachable by construction. See §12's new
                                   decision 041 entry for the full record. **★ CORRECTED
                                   2026-08-11 (`f2ac2af`, decision 043) — `from_caps`
                                   itself, plus `PrintError`/`Printer`/`PrinterCaps`,
                                   were `#[cfg(windows)]` from `Pass 64.0` onward despite
                                   holding no Win32 handle and despite `from_caps`'s own
                                   doc comment saying it was deliberately un-gated for
                                   Linux/macOS CI — the crate did not compile for ANY
                                   non-Windows target, `wasm32` included, until this
                                   commit un-gated the plain data and added the two
                                   `list_printers`/`printer_caps` non-Windows stubs its
                                   siblings already had. See §12's decision 043 entry.**
                                   **Shared by
                                   BOTH pdfcer and pdfce-gui**, deliberately — the
                                   alternative (each shell
                                   computing its own page placement) is how a GUI print
                                   comes to land differently from a CLI print of the same
                                   document at the same settings, a divergence nobody
                                   thinks to compare. `imposition.rs` (N-up/booklet/
                                   poster, `Pass 59.0`, 2026-08-10) lives here for the
                                   same reason — each changes the SHAPE of a print job
                                   (many-pages-to-one-sheet, one-sheet-folded, one-page-
                                   to-many-sheets), which the one-`Placement`-per-page
                                   model cannot express, so each is its own planning path
                                   rather than a scale-mode variant. See §12's 2026-08-10
                                   (eighty-fifth filing) entry for the crate-boundary
                                   decision record.
    pdfcer-fetch\                 <- ★★ BUILT 2026-08-13, `7393473` (`Pass 77.0`). This
                                   block previously read "PLANNED, NOT BUILT — no such
                                   directory exists on disk"; that was true when decision
                                   061 fixed the crate's SHAPE before it existed, and it
                                   is now false. The shape below was honoured key for key
                                   at creation — see `ROADMAP.md`'s `Pass 77.0` entry for
                                   the key-by-key check.
                                   ★ WHAT IS TRUE AND WHAT IS NOT YET, at `b943ea1`:
                                   the crate exists, its five public items are
                                   `PinnedArtifact`, `verify_bytes`, `sha256_hex`,
                                   `fetch_verified` and `MAX_ARTIFACT_BYTES`, and
                                   `cargo tree` shows `pdfcer-core`/`pdfcer-render` cannot
                                   see it (zero hits each). But **NO CRATE DEPENDS ON IT
                                   AT ALL** — `grep -rn "pdfcer_fetch" crates/` outside the
                                   crate returns zero hits and neither shell's
                                   `Cargo.toml` names it. So the boundary stated below is
                                   currently enforced TRIVIALLY, and the `cargo tree`
                                   result is not yet evidence about it: nothing has
                                   exercised it. That changes when the `pdfcer`
                                   subcommand lands (owed; `ROADMAP.md` *Next up*).
                                   Working name; kept.
                                   PURPOSE: pinned-URL download plus SHA-256
                                   verification — the one network primitive the shells
                                   are permitted (§1.1's 2026-08-13 narrowing). Serves
                                   OCR model fetch-and-verify, update download and
                                   add-in download.
                                   BOUNDARY, and it is the load-bearing part:
                                   `pdfcer` and `pdfce-gui` depend on it OPTIONALLY;
                                   **`pdfcer-core` and `pdfcer-render` never depend on it
                                   at all, under any future decision.** That is what
                                   keeps §1.1's engine half enforceable by `cargo tree`
                                   and keeps the wasm32 fork reachable.
                                   PRECEDENT, exact: `crates/pdfcer-print/Cargo.toml`'s
                                   own description reads *"Platform code, shared by both
                                   shells (docs/ARCHITECTURE.md §3). Deliberately NOT in
                                   pdfcer-core or pdfcer-render, which must stay
                                   platform-free."* This crate is that sentence with
                                   **network** in place of **platform**. A non-core
                                   sibling shared by both shells is established practice
                                   here, not a new pattern.
                                   WHAT IS NEW: it is the FIRST STRIPPABLE CAPABILITY
                                   THAT DOES NOT LIVE IN `pdfcer-core`. Every existing
                                   one (`jpx`, `ocrs`) sits in core and is forwarded
                                   outward to the shells; this one has nowhere to sit in
                                   core, so the forwarding runs the other way — each
                                   shell owns its own default-ON feature that pulls the
                                   crate in. `crates/pdfcer-core/Cargo.toml`'s
                                   strippable-capability convention is written entirely
                                   in terms of core features forwarded outward and does
                                   not yet describe this shape; extending that header is
                                   filed under *Backlog* in `ROADMAP.md` as the
                                   engineer's edit. ★ STILL OWED at 2026-08-14: read in
                                   that filing's dispatch, the core header is unchanged.
                                   `7393473` documented the new shape in `pdfcer-fetch`'s
                                   OWN `[features]` block, which is better than nothing
                                   and is not the same thing — the next person adding a
                                   strippable capability reads the CORE header, because
                                   that is where the convention is declared.
                                   FEATURE + LICENCES, as built: `default = ["download"]`,
                                   `ureq = { version = "3", optional = true }`; the
                                   stripped path returns `FetchError::FeatureUnsupported`
                                   by name, and `verify_bytes` sits deliberately OUTSIDE
                                   the gate so an operator-supplied file can still be
                                   checked against the manifest. Whole TLS stack
                                   permissive (ureq MIT/Apache-2.0, rustls
                                   Apache-2.0/ISC/MIT, ring Apache-2.0 AND ISC —
                                   conjunctive, rustls-webpki + untrusted ISC,
                                   webpki-roots CDLA-Permissive-2.0, a DATA licence
                                   accepted into `about.toml` by the OPERATOR on
                                   2026-08-13 after `cargo about generate` failed on it).
                                   DELIBERATELY ABSENT: anything that EXECUTES what it
                                   fetched (R13 clause 5, unresolved against the
                                   operator's "download addin" instruction — the crate
                                   moves bytes to disk and stops, and says so in its
                                   module docs), plus mirror fallback and "latest
                                   version": a pinned artifact has one source.
    pdfce-gui\                  <- ★★ REMOVED 2026-09-03, `Pass 247.0` (`da3b2f8`,
                                   399th filing; decision 128, extended by decision
                                   130). Dropped from `[workspace] members`; its
                                   egui/eframe/wgpu/winit/accesskit/rfd dependency
                                   tree is GONE from this workspace (`cargo tree`
                                   diff: 304 → 167 distinct crates, 137 removed, 0
                                   new). The native desktop shell now lives OUTSIDE
                                   this repository, as the separate project
                                   `D:\dev\pdfcer-gui` (renamed from `pdfceGUI` —
                                   the mechanical rename of `Pass 247.1` had made
                                   this read "renamed from `pdfcer-gui`", restored
                                   by hand in the 400th filing;
                                   operator ruling on open question (cd),
                                   `ROADMAP.md` `Pass 247.1`), consuming
                                   `pdfcer-core`/`pdfcer-render` as a dependency into
                                   this tree rather than as a workspace member —
                                   §3's own GUI-core separation invariant is what
                                   makes that possible at all, and the *zero GUI
                                   deps* CI job stays, trivially green, because the
                                   invariant is a property of `pdfcer-core`/
                                   `pdfcer-render`, not of having a shell in the
                                   tree (decision 128 item 3).
                                   **The design history this node carried —
                                   `Pass 44.0`'s background-render threading split,
                                   `Pass 58.0`'s `theme.rs` chrome/document-colour
                                   boundary, `Pass 58.1`'s `main.rs` module split —
                                   is PRESERVED, not deleted; it documents shipped
                                   reasoning that outlived the crate it was written
                                   about.** Full text as it stood immediately before
                                   removal: `git -C D:\Dev\pdfcer show
                                   cce414e:docs/ARCHITECTURE.md` (the fork point) —
                                   same pinned-commit citation convention as
                                   `docs/core-api/`'s 26 re-pointed citations
                                   (decision 130).
                                   ~~The native desktop shell. egui/eframe application,
                                   window chrome, file dialogs (rfd crate), menus,
                                   docking layout (egui_dock or hand-rolled), the
                                   `fn main()` entry point and packaged executable.
                                   Depends on pdfcer-core + pdfcer-render.~~
                                   **★ THREADING LIVED HERE, AND ONLY HERE
                                   (Pass 44.0, 2026-08-07, `7926a78`).**
                                   `render_worker.rs` (582 lines, `wc -l`
                                   at `bea3cb1` 2026-08-18) owns the
                                   background rasterization thread, the
                                   channel, the `RenderCancel` token and the
                                   generation counter that discards a
                                   superseded result. **`pdfcer-core` and
                                   `pdfcer-render` remain thread-AGNOSTIC** —
                                   they gained only the PROPERTY that makes
                                   this legal (`ObjectGraph: Send + Sync`) and
                                   the MECHANISM it needs (`RenderCancel`, a
                                   plain `Arc<AtomicBool>`); neither spawns a
                                   thread, owns a runtime, or knows a worker
                                   exists. **That split is deliberate and is
                                   what keeps the wasm fork a shell swap:** the
                                   web target has no `std::thread`, so a core
                                   crate that spawned one would not compile
                                   there, while a core crate that is merely
                                   `Send + Sync` compiles unchanged and lets
                                   the web shell reach for a Web Worker
                                   instead. **The session is
                                   `Arc<EditSession>`** because a worker must
                                   outlive the call that started it and
                                   `DocumentView` borrows its graph; every
                                   mutation passes through
                                   `OpenDoc::session_mut`, which cancels the
                                   in-flight render and JOINS the thread before
                                   handing out `&mut`, making `Arc::get_mut`
                                   infallible by construction. **The one place
                                   the UI thread blocks on rendering is a
                                   12 ms bounded wait in `spawn`** — 72% of one
                                   16.7 ms frame at 60 Hz — so a page that
                                   rasterizes in milliseconds returns inline and
                                   never touches the asynchronous path.
                                   **`theme.rs` (`Pass 58.0`, 2026-08-10,
                                   `2387a58`):** the first module in the
                                   crate that sets a `egui::Style` at all —
                                   before this Pass the app ran on egui's
                                   stock appearance plus ~26 scattered
                                   `Color32` literals across a 27,000-line
                                   file. `Palette` (named semantic roles,
                                   never colour names) + `Metrics` + three
                                   presets (Quiet/Airy/Dark) +
                                   `Theme::apply`, called once per frame via
                                   `ctx.all_styles_mut` (both egui 0.35
                                   light/dark `Style`s, never `set_style` —
                                   writing only one makes the app's look
                                   depend on the OS theme). Canvas-drawn
                                   overlay colours (node marks, snap
                                   guides, dimension previews) are stashed
                                   in `ctx.data_mut` under an app-owned
                                   `egui::Id`, since `egui::Style` has no
                                   field for pdfcer's own overlay vocabulary.
                                   **★ LOAD-BEARING BOUNDARY: chrome is
                                   themed, DOCUMENT colour is not.**
                                   `PdfceApp::markup_color`/`prop_color` and
                                   one pure-black comparison deciding a
                                   colour operator are written INTO the PDF
                                   (`/C`, appearance-stream colour
                                   operators) — a theme must never touch
                                   them, or a restyle would silently change
                                   what colour gets committed to a saved
                                   file. Marked `// DOCUMENT COLOUR:` at
                                   each of the three sites; `tools/check-
                                   theme-colors.sh` (new gate, same shape as
                                   `check-ui-strings.sh`) forbids raw colour
                                   literals outside `theme.rs` but honours
                                   that marker as the deliberate exception.
                                   `Settings::theme` (`pdfcer-core`) is a
                                   plain `String` token, not `theme::Preset`
                                   — core must never gain GUI vocabulary
                                   (this section's own invariant); a test
                                   in `theme.rs` cross-checks core's default
                                   token against `Preset::default()` so the
                                   two cannot drift silently. **No visual
                                   redesign shipped this Pass** — the three
                                   presets exist so the operator can choose
                                   a direction later; that choice is an open
                                   operator question, not yet answered. See
                                   §12 for the full decision record and
                                   `D:\dev\rag\egui\` for the generalized
                                   "centralize + gate" pattern this is the
                                   third pdfcer instance of (strings, icons,
                                   now colour).
                                   **`main.rs` split (`Pass 58.1`,
                                   2026-08-10, `255cf86`→`3a699cf`→
                                   `fc137e2`):** 27,647 → 25,511 lines, pure
                                   moves (no logic/signature/behaviour
                                   change, same 2,901 tests before and
                                   after), staged as three separately-
                                   revertable commits — `canvas_overlay.rs`
                                   (749 lines: canvas-drawn overlays),
                                   `panels_structure.rs` (520 lines: several
                                   dock panel bodies), `ribbon_ui.rs` (1,121
                                   lines: ribbon widget drawing). No crate-
                                   boundary change — purely an intra-crate
                                   module split, recorded here because two
                                   real defects surfaced FROM the move (see
                                   §12): a doc comment silently attached to
                                   the wrong function for two panels'
                                   worth of distance, and a textual test
                                   gate whose subject moved with the code
                                   it was gating.
    pdfcer-cli\                 <- The command-line batch shell — crate `pdfcer-cli`,
                                   BINARY `pdfcer` (no dash; `Pass 247.1`, `4db298d`).
                                   The rename script's "`pdfce-cli` means the tool"
                                   rule had turned this directory name into `pdfcer\`;
                                   `ls crates/` says `pdfcer-cli`, restored by hand in
                                   the 400th filing. Subcommand parsing
                                   (clap crate), one subcommand per batch operation
                                   (merge/split/rotate/extract, Bates stamp, convert
                                   to PDF/A, sign, validate PDF/A or PDF/UA conformance
                                   and print a report, render-page-to-PNG for scripted
                                   thumbnailing). `fn main()` entry point, packaged as
                                   its own executable alongside pdfce-gui in the same
                                   single-folder distribution. Depends on pdfcer-core +
                                   pdfcer-render, same as pdfce-gui — ZERO GUI/windowing
                                   dependencies of its own, see §7. Doubles as a fast,
                                   windowless way to exercise pdfcer-core in tests.
                                   **`printing.rs` (`Pass 55.2`, 2026-08-10,
                                   `ff873bc`+`1862b1f`; §12 decision 036):**
                                   the one genuinely platform-bound
                                   capability pdfcer needs, and deliberately
                                   NOT in `pdfcer-core`/`pdfcer-render` — a
                                   `windows` dependency there would end the
                                   WASM-fork premise as surely as an `egui`
                                   one. **Core rasterises, the shell
                                   spools.** `windows` 0.62 was ALREADY in
                                   the workspace tree, pulled transitively
                                   by eframe/winit, MIT-OR-Apache-2.0,
                                   already in `THIRD_PARTY_LICENSES.md` —
                                   verified with `cargo tree` before adding
                                   a direct dependency line, not assumed.
                                   Declared under
                                   `[target.'cfg(windows)'.dependencies]`
                                   so Linux/macOS CI still compiles this
                                   crate (a compile signal, R9/R10, never a
                                   support claim); the page-placement
                                   geometry (pure math, no platform dep) is
                                   the one part NOT `cfg(windows)`-gated, so
                                   it is unit-tested on every CI runner.
                                   **Ships enumeration (`list-printers`) and
                                   a page-fit preview (`print-preview`) —
                                   deliberately does NOT spool a job.**
                                   Printing is outward-facing (paper,
                                   shared device hardware) and irreversible;
                                   the spooling half needs the operator's
                                   explicit go-ahead before it is written
                                   against a real printer. Preview reports
                                   the PRINTABLE area, not the physical
                                   sheet — a Letter page against this
                                   session's default printer's printable
                                   region fits at 0.9725, the whole
                                   argument for using it: a naive fit to
                                   the sheet scales 1.0 and lets the
                                   hardware silently crop 8.4 pt off every
                                   edge. `Fit` and `ShrinkOversized` are
                                   kept as genuinely different operations
                                   (a test fails if they are collapsed).
                                   Clip is reported by NAME, not silently —
                                   Acrobat clips without saying so; pdfcer
                                   names the affected pages, warns on
                                   stderr, and still exits 0 (the PREVIEW
                                   succeeded).
    pdfcer-web\ (future, not     <- The web fork. Same pdfcer-core + pdfcer-render,
      built in this phase)         compiled to wasm32-unknown-unknown, eframe's
                                   web target, served as static files (no server-side
                                   PDF processing — everything still runs in-browser,
                                   preserving the "not a web app in spirit" privacy
                                   posture even in the fork).
  docs\                        <- This file, ROADMAP.md, LEGAL.md, SESSION_LOG.md
  .claude\agents\               <- pdfcer-engineer, pdfcer-librarian,
                                   pdfcer-spec-librarian, pdfcer-ui-specialist
  tests\                       <- Integration tests: parse→render→compare fixture PDFs
  fixtures\                    <- ONLY synthetic or clearly-licensed-for-redistribution
                                   test PDFs (see LEGAL.md §Test corpus sourcing).
                                   Never a scanned/downloaded real-world PDF of unknown
                                   provenance.
```

**Invariant (do not violate):** `pdfcer-core` and `pdfcer-render` must
compile with zero GUI/windowing crates in their dependency tree. This
is checked, not just hoped for — `cargo tree -p pdfcer-core` and
`cargo tree -p pdfcer-render` should never show `egui`, `eframe`,
`winit`, `wgpu` (a headless CPU rasterizer like `tiny_skia` in
`pdfcer-render` is fine; a *windowing* dependency is not). This is the
single invariant that keeps the future web fork a "swap the shell
crate" job instead of a rewrite.

**★★ AND `cargo tree` IS NOT SUFFICIENT FOR ONE SPECIFIC HAZARD — THREADING
(added 2026-08-21, decision 080).** A windowing crate is caught because it
**fails to compile** for `wasm32-unknown-unknown`. **A threading crate is
not: both `std::thread::spawn` and `rayon` `cargo check` CLEANLY for
`wasm32-unknown-unknown`** (measured 2026-08-21, rustc 1.97.1, rayon
1.12.0). `std::thread` **exists** on that target — it type-checks and links;
only the runtime has no threads to give it. **So the wasm32 leg of CI's
`cross-check` job stays green while the web build acquires a runtime
failure**, and a thread pool can enter `pdfcer-core` without any gate
objecting. Two obligations follow, and they are cheap:

1. **Any threading dependency is declared under
   `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`**, so the wasm
   build cannot name it and a parallel loop that ignores the runtime
   max-cores setting is a **build error**, not a browser crash.
2. **A dependency's DEFAULT features are checked for one.** `lopdf` enables
   `rayon` **by default** (`PRIOR_ART.md`, `4fca888`), so adding it without
   `default-features = false` puts a thread pool in `pdfcer-core` **with no
   parallel code written at all.**

Full reasoning, the operator's runtime max-cores design, and the
byte-identical-output acceptance criterion: **decision 080** (§12) and
`ROADMAP.md`'s `Pass 122.0`.

**★ ABOUT TO BE VALIDATED BY AN INDEPENDENT IMPLEMENTATION, 2026-08-13
(decision 058, hundred-and-thirty-seventh filing).** The operator has
paused GUI production in this repo and a **separate GUI project is being
built in `D:\dev\pdfcer-gui` in another session**, which — his words — *"if
successful will likely replace the current one and may have its dev
folder merged into this one."* **A second shell consuming `pdfcer-core` /
`pdfcer-render` from OUTSIDE this repository is exactly the scenario this
invariant was written for**, arriving early and in a different costume
than the web fork it was aimed at. **That is a stronger test than any
`cargo tree` check**, because `cargo tree` proves only that no windowing
crate is in the graph; an independent implementation proves the boundary
is *sufficient* — that a shell can be built against this surface without
needing anything on the far side of it to move. **The reading of the
result, fixed in advance so it is not decided by whoever is inconvenienced
by it:** if that project needs **nothing** to move in core, the separation
is **real and demonstrated rather than asserted**; **anything it does need
is a place the boundary was drawn wrong**, and is recorded as a finding
about this repo rather than quietly accommodated. **`cargo tree` remains
the cheap daily gate** and is not superseded — the two checks answer
different questions. **Second-order consequence, and the one with teeth:
`pdfcer-core`'s public API now has a REAL external consumer** (§8, project
rule 10) — doc-comment completeness and API-guideline compliance stop
being hygiene and become somebody else's unblocking, by a party who
**cannot ask a question here**. Full record: `ROADMAP.md`'s **GUI pause**
block at the head of *In progress*. **Nothing about `D:\dev\pdfcer-gui`
beyond the operator's own sentence is known to this repo; nothing further
is asserted about it here.**

**Empirically checked against a mobile target for the first time,
2026-08-11 (investigation only — no Android build committed, no
decision to target Android made; full measurements and effort
breakdown: `ROADMAP.md`'s Backlog entry, hundred-and-thirteenth
filing).** `cargo check --target aarch64-linux-android` compiles
`pdfcer-core`, `pdfcer-render`, `pdfcer-print` and `pdfcer` with
**zero errors and zero source changes** — the first time this
invariant has been exercised against a target outside the {Windows,
Linux, macOS, `wasm32-unknown-unknown`} set decision 043 already
names. `pdfce-gui` fails with **13 errors, all from `rfd`** (the
native file-dialog crate) — zero from `egui`, `eframe`, `winit` or
`glow` — so the one failure that does occur lands exactly at the
shell boundary this invariant draws, not inside it. Per decision 043's
own distinction between a dependency-graph check and a buildability
check, this is that second, stronger check's **first clean result
against a genuinely new target class** (the `pdfcer-print` incident
decision 043 records was that same check's first RED one). `eframe`
itself carries first-class Android support
(`NativeOptions::android_app`, `android-game-activity`/
`android-native-activity` features) — this is not a gap the invariant
happened to dodge, it is the shell crate's own target list already
including the platform being checked. One real constraint, not merely
a gap: `eframe`'s `accesskit` feature (which `pdfce-gui` enables for
native accessibility) and the `android-native-activity` feature are
**mutually exclusive under `target_os = "android"`**
(`eframe-0.35.0/src/lib.rs:148-153`, a `compile_error!` citing
AccessKit's own winit-integration limitation) — any future Android
attempt has to choose `android-game-activity` or drop the stated
accessibility goal for that target, not both.

## 4. Core data model (target contract — implemented incrementally per ROADMAP)

This is what `pdfcer-core` will expose once Pass 1+ lands. Written now
as the target so early implementation work has a north star; update
this section the moment the real API diverges (the doc is the logic —
if code and doc disagree, that's a bug in one of them, fix it same-day).

**Current state as of Pass 0 (2026-07-23):** `pdfcer-core` exposes ONLY
the header-probe surface — `PdfVersion { major, minor }`, `PdfError`
(`thiserror`, `#[non_exhaustive]`), `probe_header(&[u8])`,
`probe_file(&Path)`, and the `HEADER_SCAN_WINDOW` const (1024, the
byte window the `%PDF-` marker is scanned within). None of the
`Document`/`Object`/`Page`/`StreamData` model below exists yet; it is
the Pass 1+ target. The user deliberately kept Pass 0's core this thin
to defer the from-scratch-vs-`oxidize-pdf` foundation decision (§12
entry (b), 2026-07-23) — do not treat the contract below as implemented.

**Forward pointer (2026-07-30):** that decision is now CLOSED — build
from scratch (§12 entry 2026-07-30,
`docs/decisions/001-oxidize-pdf-adopt-vs-build.md`) — and it binds six
Pass-1 obligations on this model: `ByteSpan` provenance, a lossless
content-stream token model, the ONE-object-model invariant, fail-clean
filter contract, unwrap-deny lints, and no output fingerprint. The
engineer integrates the full design text here at Pass 1.

- `Document` — owns the COS object graph, trailer, xref, and the
  original byte buffer (for lazy/unmodified-object passthrough on
  write — see the round-trip invariant below).
- `ObjectId(u32 /* number */, u16 /* generation */)`
- `Object` — enum: `Null | Bool | Integer | Real | String(Str) | Name |
  Array(Vec<Object>) | Dict(Dictionary) | Stream(Dictionary, StreamData) |
  Reference(ObjectId)`
- `StreamData` — lazy: holds the raw (still-encoded) bytes plus the
  filter chain; decoding happens on demand and is cached, never eager
  for every stream in a large document.
- `Page` — resolved view over a page dictionary: `MediaBox`, `Resources`,
  content stream(s) concatenated, inherited attributes resolved per
  §7.7.3.4 of the spec (page tree attribute inheritance).
- `Document::open(path) -> Result<Document, PdfError>`
- `Document::save(path) -> Result<(), PdfError>` — full rewrite.
- `Document::save_incremental(path) -> Result<(), PdfError>` — **the
  default save mode.** Appends a new xref section + updated/new objects
  only; every object pdfcer did not touch is left byte-identical in the
  file. This is not an optimization, it's a correctness requirement:
  Acrobat's own digital-signature model depends on incremental updates
  (a signature covers a byte range; anything after that range is a
  later revision). pdfcer must support this from day one, not bolt it on
  after signatures are implemented.
- `Document::render_page(index, dpi) -> Pixmap` (in `pdfcer-render`,
  takes a `&Document`).

**IMPLEMENTED (2026-08-02, Pass 17.0, commit `3a56b55` — was a forward
pointer, now current reality):** `render_page`/`render_page_with` stay
`&Document`-taking thin wrappers (unchanged signatures), but
`pdfcer-render`'s real internal surface is generalized to accept
`&pdfcer_core::view::DocumentView` (a promoted, top-level home for the
former `pageops::assemble::DocumentView`) so it can render either a
plain `Document` or a live `EditSession` overlay — this is what the
canvas now actually renders (`self.session.view()`, not
`self.session.document()`). Full design, including the `StreamSource`
byte-source abstraction (`Contiguous | Split { base, staged }`) and the
two implementation deviations found while building it
(`image_codec::decode_image` generalization; `DocumentView::bytes()` is
`Option<&[u8]>`, not `&[u8]`): §12 entries "2026-08-02 — Decision 018"
and its same-day continuation-56 follow-up, below.

**IMPLEMENTED (2026-08-03, Pass 18.5, commit `9998a6b`):** the vector
object model (`pdfcer_core::vector`, introduced incrementally from
decision 011/Pass 9a onward, not otherwise itemized in this section)
gains two hit-testing/content-detail additions. **Invariant:**
`hit_test_point` is defined as the structural head
(`hits_front_to_back(..).next()`) of the new
`hit_test_point_all -> Vec<HitResult>` (`hits_front_to_back(..).collect()`)
— the two cannot disagree because there is one private iterator
underneath both; see §12's continuation-60 entry for the full rationale
and the cross-project generalization filed to
`D:\dev\rag\rust\define_singular_query_as_head_of_plural_query.md`.
**`TextObject`** gains `preview: TextPreview` (sourced-text-only, no
derived spacing; four-variant enum, not `Option<String>`) and
`font: Option<TextFont>` (`size` is the literal `Tf` operand, not the
rendered glyph size) via a new `FontResolver` seam
(`NoFonts`/`DocumentFonts`, zero GUI dependency, `decompose(...)`'s
public signature unchanged). **`ImageObject`** gains `pixel_size`. Full
build record: `ROADMAP.md`'s Pass 18.5 Shipped entry.

**IMPLEMENTED (2026-08-03, Pass 18.6, commit `1b38e34`):** `TextObject`'s
bounding box is no longer the pen-start point of each show operator
inflated symmetrically by the largest `Tf` size in the run (a square
centred on the run's start, for the common single-`Tj` case). It is now
the summed §9.4.4 advance widths across the run for the horizontal
extent, and the resolved font's ascent/descent for the vertical extent,
computed via a new `TextBoundsBasis` four-variant enum
(`FontMetrics | MetricAdvancesNominalHeight | EstimatedAdvances |
EmBox`) — deliberately four bases, not the two the originating ui-spec
(§E) asked for, because a Type 3 or descriptor-less CIDFont has real
advances but only a guessed height, and a non-standard-14 font with no
`/Widths` has estimated advances; collapsing either into `FontMetrics`
would silently misrepresent the confidence of the box. `EmBox` is the
prior (pre-Pass-18.6) geometry, kept as the guaranteed fallback for
`NoFonts`/unresolvable-font/non-finite-`Tf`-size objects — never
silently upgraded to a basis the data doesn't support. New `Vertical`
ascent/descent resolver, a four-rung fallback ladder: `/Ascent`+
`/Descent` (§9.8 Table 122) → `/FontBBox` `ury`/`lly` (§7.9.5) →
compiled-in standard-14 descriptor metrics (§9.6.2.2) → nominal 1.0/
−0.25 em, flagged. Composite (Type 0) fonts resolve through the
**descendant** font's descriptor (§9.8.1 — a Type 0 dict itself never
carries one); Type 3 always takes the nominal rung (its own descriptor
numbers, when present, live in `/FontMatrix` glyph space, not text
space). **Invariant:** `advance_tx(w0, tfs, tc, tw, th)` is now the ONE
implementation of §9.4.4's displacement formula, shared verbatim by
`text_extract::page::show_code`, `redact::glyph`, and this bbox
computation — a fourth call site was about to become a third
independent implementation before this consolidation. Two latent
decompose-walk bugs, invisible under the prior ±1-em-inflated geometry,
were found and fixed in the same Pass: `'`/`"` did not perform their
`T*` line move (§9.4.3 Table 109), and `Tc`/`Tw`/`Tz`/`Ts` were not
tracked in the decomposer's `GState` at all. Zero new Cargo
dependencies — reuses `text_extract::font::ExtractFont`'s existing
dictionary-only resolver (rule R21: no glyph-shaping crate in
`pdfcer-core`, a hit-test runs per click). Full build record:
`ROADMAP.md`'s Pass 18.6 Shipped entry (top of Shipped).

**IMPLEMENTED (2026-08-03, Pass 21.0, FF-C, decision 021, commit
`48c6b77`; §3/§4 body-section sync filed 2026-08-04, continuation 77 —
flagged owed at ship, discharged here):** `pdfcer-core` gains
`font_embed.rs` — the FIRST pdfcer-core surface that emits a *new*,
operator-supplied font program into a PDF, distinct from every prior
font-touching module which only ever READ existing font resources.
Public: `FontEmbedPlan` (plain-data contract: `SubsetGlyph`,
`DescriptorMetrics`, `OutlineKind` — `TrueType` emittable at P0,
`Cff` refused by name, decision 021 §10 C-3), `build_objects(&plan) ->
Result<EmbeddedFontObjects, FontEmbedError>` (allocates a `/Type0`
font dict + `/CIDFontType2` descendant + `/FontDescriptor` +
`FontFile2` stream + `/ToUnicode` CMap — always `Identity-H`, forced
independently by both `subsetter` stripping `cmap` and ISO 32000-1
§9.9's `shall`). **Round-trip: R107 — this module only ever
ALLOCATES fresh object ids, never rewrites an existing `/FontFile*`/
`/FontDescriptor`/`/Font` dict**, so FF-C needs no new §5 exception;
incremental save stays the default.

`pdfcer-render` gains `font::subset` — `plan_subset(donor_bytes, ...)
-> Result<FontEmbedPlan, SubsetError>`, parsing the donor via the
existing skrifa parser (no second font-program parser added anywhere
— R21 unchanged) and calling `subsetter::subset`. Reads the donor's
`OS/2 fsType` BEFORE subsetting, since `subsetter` strips `OS/2`
(R109: `SubsettingNotPermitted` on bit 8, `EmbeddingNotPermitted` on
bit 9, both correctly inert on `OS/2` v0/v1). `MAX_DONOR_BYTES` = 64
MiB, a judgement call rather than a corpus measurement (`ARCHITECTURE.md`
§10.1 wants a bound on attacker-influenced bytes; the project's own
`tools/fontfile-census` measured EXISTING embedded font programs, which
ISO 32000-1 §9.9 forbids reusing as an FF-C donor — so that census
cannot justify this constant, and the number is stated as argued, not
measured; see the constant's own doc comment).

**Why the split runs core/render rather than living entirely in one
crate (decision 021 §3.2):** subsetting is a *write* concern, so
`pdfcer-core` looks like its natural home — but producing a subset
first requires *parsing* the donor (coverage from `cmap`, advances
from `hmtx`, descriptor metrics, the `fsType` bits), and that parser
already exists in `pdfcer-render`. Putting `subsetter` in `pdfcer-core`
would give a crate with no font-program parser two of them, purely to
avoid a plain-data seam. So the seam **is** the design: `pdfcer-render`
parses and subsets, `pdfcer-core` emits the PDF objects, and
`pdfcer-core` gains **zero** new dependencies from this Pass.
`pdfcer-core` still has no font-program parser after Pass 21.0 —
`fontdata/` (§4, standard-14 metrics) remains compiled-in metrics
only, unchanged.

**Composite-run editability is explicitly NOT part of this contract.**
Pass 21.0 can only ADD composite text; R110 (Standing rules) governs
whether an already-present composite run can be EDITED, and as of this
entry `ShowSlot::code` (§4's `Page`/text-decode model) is still `u8`
and cannot hold a multi-byte CID — composite runs Pass 21.0 adds are
locatable and correctly refused (R-INV-4), not yet rewritable. Do not
read this §4 entry as FF-C being complete; see `ROADMAP.md`'s Pass
21.1 In-progress entry.

> **[SUPERSEDED 2026-08-05 by §4.1(F) below — read the two together.**
> `ShowSlot::code` has been **`u32` since Pass 21.1**, and **Pass 29.0
> (`a104536`) lifted the blanket composite refusal entirely**. The
> paragraph above is retained because it states *why* the narrowing
> existed, and because §4.1(F) records that the reason had become
> **self-justifying** — the refusal was cited as the ground for keeping
> the types single-byte, and the single-byte types were cited as the
> ground for keeping the refusal. **R-INV-4 still exists and still
> fires**, but only on the two font properties no amount of pdfcer work
> can fix. **]**

---

## 4.2 Published model guarantees — properties consumers may rely on

*(Added 2026-08-27, 297th filing, `Pass 145.0` / `0c48bbf`. Created by
**decision 094**, §12.)*

A **published model guarantee** is a property of `pdfcer-core`'s data model
that this project states to consumers as **stable**, backed by a test, and
that a future refactor may therefore **not** break silently. It is a
deliberately small list, and the bar for adding to it is high — every entry
is a constraint on work nobody has done yet.

**Why the list exists at all.** `D:\dev\pdfcer-gui` (and any future shell)
builds against `pdfcer-core` without building it. A property they *observe* to
be true is not a property they may *rely* on, and decision 058 puts the burden
of saying which is which on **this** project, not on them. Without a list, the
only options are (a) they re-derive the property defensively and get a second
implementation of something pdfcer owns (`R221`), or (b) they rely on it
silently and a refactor breaks their build with no failing test on our side.

**The entry format is fixed: the property, the measurement that established
it, the test that pins it, and the refactor it forbids.** A guarantee without
its measurement is a claim; a guarantee without its forbidden refactor does
not tell a future engineer what they may not do, which is the only reason to
write it down.

### 4.2.1 The `operator_span`-slice invariant

**The guarantee.** *The glyphs of a `TextRun` that share one
`GlyphProvenance::operator_span` are **contiguous within the run**, and the
range they occupy **slices cleanly out of the run's text**.*

**A caller may therefore** locate a show operator's glyphs inside a run, take
their span, and use it as a locator — which is exactly what
`FormatRequest::whole_operator` / `pinned` now do inside `pdfcer-core`, and
what `pdfcer-gui` had already shipped from outside before this guarantee
existed.

**The measurement that established it** (`Pass 145.0`, 2026-08-27), over
`fixtures/`:

| quantity | value | per-item form |
|---|---:|---|
| files walked | 4,289 | — |
| files with text | 1,623 | 37.8 % of 4,289 |
| runs | 18,559 | 11.4 per text-bearing file |
| glyphs | 669,436 | 36.1 per run |
| `operator_span` groups | 29,246 | 1.58 per run · 22.9 glyphs per group |
| **non-contiguous groups** | **0** | 0 of 29,246 |
| **groups not indexing text cleanly** | **0** | 0 of 29,246 |

**The test that pins it.** `crates/pdfcer-core/tests/operator_span_invariant.rs`
— it re-runs on every `cargo test`, and it is **sabotage-checked**: excluding
one glyph per group from the coverage computation turns it red, so the two
zeroes are measurements rather than a probe that examines nothing.

**The refactor it forbids.** `text_extract/layout.rs`'s run segmentation may
**not** be changed in a way that lets one `operator_span` group's glyphs
become non-contiguous inside a run — **even if no pdfcer feature notices**.
That last clause is the whole point: pdfcer's own code could absorb such a
change without a failing test, and a shell's locator would break. **The test
is the enforcement; this entry is why it may not be deleted when it looks
redundant.**

**A neighbouring fact that is NOT a guarantee, stated so the two are not
conflated.** A `TextRun` is **not** a show operator, and **2,420 of 18,559
runs (13.0 %) carry glyphs from more than one**. That is a *measurement of
real files*, not a promise about the model — a different corpus could move the
percentage, and a producer could emit one operator per run throughout. What is
guaranteed is the slice property above; what is measured is that the
multi-operator case is **common enough that any locator must handle it**.

**Related, and also not a guarantee:** `text.chars().count()` is **not**
`glyphs.len()` — `/ToUnicode` maps a code to a **string** (ISO 32000-1
§9.10.3), so one glyph may carry several characters. Documented in
`docs/core-api/01-reading-and-model.md` §8.4.0. Measured at **1 of 191
synthetic fixture runs**, and **that ratio is the trap**: near-zero on
synthetic text, routine on real typeset copy, so a `find`-based locator looks
correct in every fixture a shell writes for itself.

### 4.2.2 The review-state chain guarantee — pdfcer reports the graph and never resolves currency

*(Added 2026-09-06, 460th filing, `Pass 253.1` / `fe746ec`. Created by
**decision 139**, §12.)*

**The guarantee.** *A review status authored by `EditSession::add_review_state`
is a separate `/Text` annotation whose `/IRT` points at the **deepest existing
status by the same `/T` on that target's chain**, not at the target itself
whenever such a status exists — and `ReviewStateAdded::attached_to` and
`::chain_depth` report exactly which node it landed on. pdfcer states no
opinion about which status on a target is the **current** one.*

**A caller may therefore** rebuild each author's status history by walking
`/IRT` and grouping on `/T`, and may rely on the chain being a chain — one
node per status per author, in authoring order — rather than a star. **A
caller must therefore** supply its own currency rule; there is no
`current_state()` and there will not be one without a new decision record.

**Why the graph shape is a guarantee and not an implementation detail.**
§12.5.6.3 closes with a `shall`: *"Additional state changes shall be made by
adding text annotations **in reply to the previous reply** for a given user."*
★ **A star and a chain render identically in every viewer**, so a consumer
that assumed `add_review_state(target, …)` attached to `target` would produce
conforming-looking, history-destroying output that no screenshot, render diff
or operator report could surface. The two report fields exist to make the
shape **checkable from outside**, which is what turns it into something a
shell may rely on rather than observe.

**Why there is no resolver — measured, not preferred.** The standard defines
**no ordering** over the statuses on a target (`current state` and `most
recent` do not occur in either edition in an annotation context); `/M` is
**optional** (Table 164) and empirically ties, because a batch of statuses set
in one session share a timestamp to the second. Any resolver would therefore
be pdfcer inventing a rule and presenting it as a reading of the file — the
`R27` failure mode, one layer up. The requester asked for exactly this split,
verbatim: *"Give us the annotations and the keys; we will pick."*

**The read/write asymmetry this implies, and it is deliberate.**
`Annotation::state` / `::state_model` are `Option<String>` — the **open** set,
verbatim, because neither key carries a *"shall be one of"* in either edition,
so a value outside Table 171's vocabulary is *unhandled, not illegal*.
`edit::ReviewState` is a **closed** seven-variant enum with **no `Other`** —
the set pdfcer **authors**. *Read the open set; author the closed one.*

**The test that pins it.** `crates/pdfcer-core/tests/review_features.rs` —
`a_second_status_by_the_same_author_chains_onto_the_first` and
`a_different_author_starts_their_own_chain`, plus
`the_state_keys_are_text_strings_not_names` and
`the_state_model_is_derived_from_the_state` for the encoding half.
**Sabotage-checked:** replacing the per-user chain with a star fails 1;
writing `/State` as a name fails 3.

**The refactor it forbids.** Attaching a status to the target unconditionally
(the shape the request and the `Pass 253.1` *Backlog* entry both proposed);
adding an `Other(..)` arm to `ReviewState` (it would let pdfcer author a
vocabulary the standard does not define); typing
`Annotation::state`/`::state_model` as that enum (it would repair a producer's
value on the way in); and adding any `current`/`latest`/`effective` status
accessor without a new decision record superseding **139**.

## 5. Round-trip / non-destructive-editing invariant

Analogous to the tail-bytes / lazy-round-trip discipline the user's
other format-RE project (SWFormat) established for SOLIDWORKS files —
same principle, different format:

- Any object pdfcer did not logically modify is re-emitted **byte
  identical** (for full rewrite) or **omitted entirely** (for
  incremental save, since the old bytes are simply not touched).
- Never "normalize" a PDF's internal structure as a side effect of an
  unrelated edit (e.g. don't silently rewrite every xref table to xref
  streams just because pdfcer opened the file). Minimal-diff editing is
  a hard requirement — Acrobat users expect that adding one comment to
  a 400-page contract does not perturb the other 399 pages' bytes,
  and forensic/signature-validity expectations depend on it.
- Corollary: **redaction is the one deliberate exception.** True
  redaction must actually remove the covered content from the object
  stream (not just draw a black box on top) — see ROADMAP backlog
  item "Redaction — true content removal". This is a documented,
  intentional violation of the minimal-diff rule for exactly the
  objects the user asked to redact, and only those.
- **Forward pointer (2026-07-30):** the mechanical enactment of this
  invariant — `ByteSpan` provenance on every parsed object and a
  lossless, span-provenanced content-stream token model — is specified
  by the six Pass-1 obligations in the §12 entry of 2026-07-30
  (decision record `docs/decisions/001-oxidize-pdf-adopt-vs-build.md`
  §6.1); full design text lands in this document with Pass 1.
- **PDF-1.5 extension (2026-07-30, Pass 1.1 item 1 — continuation
  8):** objects parsed out of object streams (§7.5.7) carry
  `Provenance::ObjectStream { container, index }` rather than a file
  `ByteSpan` — for these, byte-identical passthrough is
  **expressible-or-consciously-absent**: a compressed object has no
  contiguous file bytes to re-emit, so any writer that touches one
  must either promote it to an uncompressed object or rewrite its
  container stream. The contract is documented on the `Provenance`
  type itself in `pdfcer-core`; `file_span()` returns `Some` only for
  `Provenance::File`. See the §12 continuation-8 entry of 2026-07-30.

### 5.1 The invariant stated precisely — three contracts, never one

*(Added 2026-07-31, Pass 3.0. Before this Pass §5 was prose; it is now
a measured gate. Decision 007 W1/R32 names conflating these "the single
likeliest source of a false green or a false red".)*

The invariant is **not** one claim. It is three, and each save mode
promises exactly one of them:

| Save mode | What is byte-identical | Assertion shape |
|---|---|---|
| `save_incremental`, **empty dirty set** | the **whole file** — output *is* input | `output == input` |
| `save_incremental`, non-empty dirty set | **every byte below the original EOF** | `output.starts_with(input)` |
| `save_full` | **every object definition** of a `Provenance::File` object | per object, **never** per file |

A full rewrite **cannot** be byte-identical file-wide: object offsets
move, so the cross-reference section must differ. A test asserting
file-level identity for `save_full` fails universally; a test asserting
only reloadability passes vacuously. Both mistakes look like diligence.

Two corollaries that are easy to get wrong and expensive to discover
late:

- **Zero edits means zero bytes.** An empty dirty set produces the
  input file, not "the input plus an empty revision". Appending a
  revision to a document the operator did not change is itself a §5
  violation.
- **The dirty set is a save-time diff against the base revision**,
  never the union of every command run (§11.1). §7.5.6 requirement 1
  is the spec-side reason: an update section *"shall contain entries
  **only for** objects that have been changed, replaced, or deleted"*
  — a restriction, not merely permission to omit.

**Measured, not asserted** (Pass 3.0, 2,914-file corpus): whole-file
identity 2,898/2,898 loadable files (100%); prior-bytes-intact on
append 2,898/2,898 (100%); per-object verbatim on full rewrite
2,897/2,898, the one exception being a hybrid file pdfcer **refuses by
name** (see below). `tools/roundtrip` is the executable gate; it
re-runs on every writer-touching Pass.

### 5.2 Redaction forbids incremental save

*(Added 2026-07-31, Pass 3.0, closing decision 007 W2. This was
trust-critical and undocumented, and incremental is the DEFAULT mode.)*

**Incremental save structurally preserves superseded content.** §7.5.6
requires that *"changes shall be appended to the end of the file,
leaving its original contents intact"* — so the old bytes of every
replaced object remain in the file **by construction**. A redaction
saved incrementally therefore leaves the redacted content trivially
recoverable by anyone who reads the earlier revision.

Binding rule (**R35**): redaction — and any operation whose contract is
*removal* — **must force a full rewrite and must refuse incremental
save.** This is enforced in the writer, not left to the Redaction Pass
to remember, and the Redaction Pass owes a test that greps the saved
bytes for the removed content.

See also §11.2, which covers the *undo* half of the same exception:
once a redaction is written, no later session can undo it, because
there is no data left in the file to restore.

**Correction (2026-07-31, Pass 3.1):** this section's original framing
implied that forcing a full rewrite closes the stale-copy path for
**promoted compressed objects**. It does not — object streams carry
through **verbatim in both save modes** (§5.6), so a promoted object's
superseded value survives inside its untouched container even after a
full rewrite. R35's refusal of incremental save is necessary but NOT
sufficient for redaction; see §5.7 for the full amendment and the
binding consequence for the Redaction Pass (container
rewrite/decomposition).

### 5.3 `/ID` discipline on save

*(Added 2026-07-31, Pass 3.0, closing decision 007 W6/R39.)*

§14.4 says `ID[0]` *"shall not change when the file is incrementally
updated"* and `ID[1]` is *"a changing identifier based on the file's
contents at the time it was last updated."* Read naively, the second
half conflicts head-on with byte-identical round-tripping.

It does not actually conflict, and the reasoning matters because it
will be re-litigated:

1. **If nothing changed, nothing was "updated"** — §14.4's trigger
   never fired.
2. `/ID` is `should`-strength for unencrypted files, and **no `shall`
   anywhere requires regeneration**. §14.4 states what `ID[1]` *is*,
   not when a writer must recompute it.

Binding rule: pdfcer regenerates `ID[1]` **exactly when a save writes at
least one changed object**, and never otherwise. `ID[0]` changes only
when pdfcer creates a document it regards as new (a from-scratch write,
or an explicit "Save As new document") — never on incremental save or
plain full rewrite. This is also an R41 matter: a gratuitously
regenerated `/ID` is an observable *pdfcer touched this file* signal on
a file pdfcer did not change.

Load-bearing beyond tidiness: `/ID[0]` is an input to §7.6.3.3's
encryption-key derivation, so an error here becomes a Pass 5 decryption
failure that presents as a crypto bug.

### 5.4 Linearization is invalidated by any save, and never repaired

*(Added 2026-07-31, Pass 3.0, closing decision 007 W5. Citation
corrected 2026-07-31, Pass 3.2 filing: this section's rule is
**R42** — the original "R36" citation collided with decision 007's
R36, "save mode is chosen by contract and disclosed," which the
writer/`document.rs`/`linearization.rs` code comments cite and keep.
See the dated reconciliation note at R42 in `ROADMAP.md` Standing
rules. The warn-before-save behavior below remains ALSO covered by
R36's disclosure clause; the never-repair/never-strip/never-patch-`L`
rule is R42.)*

Annex F.1 is normative and blunt: *"Incremental update shall still be
permitted, but the resulting PDF is **no longer linearized** and
subsequently shall be treated as ordinary PDF."* An append lands past
the first-page cross-reference table and the hint streams, so the
linearization is stale afterwards.

That is spec-sanctioned and unavoidable — but it is an observable
property change the operator did not ask for (the file opens more
slowly over a network). Under the *fuzzy, never sneaky* rule, pdfcer:

- **detects** linearization on load (Annex F.3.3's 1024-byte parameter
  dictionary, with `L`-versus-file-length as the liveness check);
- **warns** before a save that would spend a live Fast Web View
  property;
- **never strips** a stale `/Linearized` dictionary (that would be a
  normalization, and Annex G.7's reader-side revalidation depends on
  it being present);
- **never patches `L`.** `L` is not the property — the object ordering
  and hint validity are. A file whose `L` was "fixed" after an append
  *claims* to be linearized while its hints point into a stale layout,
  which is strictly worse for a network reader than an honestly
  de-linearized one.

Re-linearization belongs to the Optimization backlog bucket, not to any
save path.

### 5.5 Signatures and the redaction conflict

*(Added 2026-07-31, Pass 3.0, closing decision 007 W7.)*

§12.8.1 NOTE 1: *"If a signed document is modified and saved by
incremental update, the data corresponding to the byte range of the
original signature is preserved."* A **full rewrite destroys every
existing signature**, because a signature covers a byte range that a
full rewrite necessarily disturbs.

So signature presence forces incremental — which collides head-on with
§5.2's rule that redaction forces a full rewrite. **"Redact a signed
document" is a genuine either/or, not an oversight**, and it must be
surfaced to the operator as an explicit choice, never resolved
silently. Naming it here means neither the Redaction Pass nor the
Signatures Pass can claim surprise.

Structural consequence already in force: pdfcer never re-serializes a
signature dictionary, *even identically*. Its `/Contents` is a
fixed-width placeholder referenced by byte offsets, so re-emitting it
is a hazard regardless of whether the bytes come out the same. The
answer is structural rather than a special case — signed objects are
`Provenance::File` objects and ride the verbatim copy path like any
other.

### 5.6 Never normalize — the rule that has no spec backing, and needs none

*(Added 2026-07-31, Pass 3.0. Decision 007 R33/W4.)*

§7.5.6 contains **no requirement** that an appended update section
match the form of the section it supersedes. That is a recorded
NEGATIVE RESULT in the spec RAG, not an oversight — which is precisely
why the rule has to be pdfcer's own.

pdfcer emits whatever the base file's **newest** cross-reference section
already used, and never chooses:

- a classic §7.5.4 table stays a classic table;
- a §7.5.8 cross-reference stream stays a stream;
- a §7.5.8.4 **hybrid** file is appended to as a classic section
  carrying `/XRefStm` **forward** (form A — the only shape that
  satisfies §7.5.6 requirement 3's *"all the entries except the `Prev`
  entry … whether modified or not"*, since `/XRefStm` is such an
  entry);
- object streams are carried through a full rewrite **intact, with
  zero promotions**: a type-2 entry names a container and an index,
  neither of which is a byte offset, so re-emitting the container
  verbatim leaves every type-2 entry still correct;
- the `%PDF-M.N` header line and its §7.5.2 binary-comment line are
  copied byte-for-byte, so no save can raise a file's version —
  **copied FROM THE `%PDF-` MARKER since 2026-08-07; any bytes BEFORE
  it are dropped by a full rewrite. See the narrowing at the end of
  this section — it is the one deliberate exception §5.6 has.**
- **(added 2026-08-08, `8672cbc`) — the rule now also applies ONE LEVEL
  BELOW section-form matching.** Even *within* an unchanged classic
  §7.5.4 table, the per-entry two-byte terminator (`SP LF` / `SP CR` /
  `CR LF` — §7.5.4 permits all three, no preference stated) is matched
  from the base file rather than fixed to `SP LF`. `xref_entry_eol`
  defaults to `MatchSource`, read once per save by
  `xref::observed_entry_eol` from the last `xref` section's first entry.
  Same reasoning as the rest of this section: a fixed spelling changes
  bytes in every entry of a table nobody logically touched. See §12's
  2026-08-09 decision-log entry (the `8672cbc`/`365856f` pair) for the
  full mechanism and the evidence-tier argument for the default change.

~~**A full rewrite of a hybrid file is refused by name** rather than
flattened. §7.5.8.4 describes a hybrid as a three-part unit a writer
creates *"at the same time"*; rebuilding it from a merged view requires
re-deriving the hidden-object set and re-checking the clause's
recursive visibility rule. Normalizing it to a single section instead
would silently destroy the file's pre-1.5 readability. Refusing is the
R27 fail-clean posture applied to the write side: name it, count it,
do not guess.~~

**★ SUPERSEDED 2026-09-09, `Pass 281.0` (`1177221`) — a full rewrite of
a hybrid file is now PERFORMED, not refused; R33 is UPHELD, not
waived.** The struck paragraph is kept legible because a reader who
remembers "hybrid full rewrite is refused" needs to see that it moved,
not wonder whether they misremembered it. What changed: the loader now
**retains** which object numbers the `/XRefStm` established
(`Document::hybrid_partition`) instead of discarding that fact into
`merge_first_wins`'s single flattened map — the one thing the old
paragraph's "merged view" problem actually depended on. Nothing is
**re-derived**: re-deriving the hidden-object set from §7.5.8.4's
recursive visibility rule would answer what a producer *may* hide, not
what this file *did* hide, and a file may legally hide less than the
maximum — re-deriving the ceiling would be the normalization this
section forbids, wearing a different disguise. `write_hybrid_tail`
reproduces the three-part unit §7.5.8.4 itself describes (main classic
table with the hidden objects and the stream object marked free at
generation 65535, a main trailer with no `/XRefStm`/`/Prev`, the real
xref stream, an update classic table naming only the stream object, an
update trailer carrying `/XRefStm` and `/Prev`) — the file is not
flattened and its pre-1.5 readability is not destroyed. **What still
refuses, and for the reason the old paragraph actually needed:** a
file whose `/XRefStm` itself does not parse. There, pdfcer genuinely
cannot say which objects the stream was hiding, and the R27 fail-clean
posture — name it, count it, do not guess — still applies, now to a
narrower case. See `docs/ROADMAP.md`'s `Pass 281.0` Shipped entry for
the corpus evidence (225/237 → 237/237 full-rewrite verbatim) and the
Standing Rules `R33` dated note.

#### ★ 5.6.1 THE ONE DELIBERATE EXCEPTION — a full rewrite DROPS bytes before `%PDF-` (added 2026-08-07, `fa4f83c`)

**§5.6 stands, and is narrowed at exactly one point.** A **full
rewrite** emits `%PDF-` at **byte 0** and discards any preamble — the
BOM, whitespace or junk that pdfcer's 1 KiB header probe tolerates on
the way in. `save_incremental` and identity-append are **unchanged**
and still carry a preamble through; they promise whole-file identity
and a byte-prefix respectively (§5.1) and **do not call `header_span`
at all**, which an identity assertion in the same test pins.

**This reverses a tested contract, so the reasoning is recorded in
full rather than summarised.**

**What §5.6 said, and it was not wrong.** *Do not normalize what the
operator did not ask about.* A leading preamble is such a thing, the
probe tolerates it, and pdfcer's emitted offsets were **absolute from
byte 0 exactly as §7.5.4/§7.5.5 require** (*"the byte offset … from
the beginning of the file"*). Every offset in the output was verified
to match its true position. **pdfcer's writer was correct.**

**What overturned it, and it is a MEASUREMENT, not a preference.** A
minimal 3-object file with **correct absolute offsets** and 19 bytes
of junk before `%PDF-` is unreadable to veraPDF — *"can not locate
xref table"* — and the identical file with the junk removed parses
clean (`failedToParse="0"`). **veraPDF reads offsets as
HEADER-RELATIVE whenever a preamble exists**, whatever the producer
intended. So the property is not a quirk of one corpus file's
convention: **every preamble-preserving file pdfcer ever wrote was
unreadable to an independent conformance reader.**

**Why dropping is the right answer rather than a capitulation.** The
spec RAG (`iso32000__s__7.5.md`) records the offset base as *"a real,
load-bearing ambiguity"* that **ISO 32000-1 does not resolve** — the
spec position is byte 0, and it gives readers on the other side no
guidance at all. Preserving the preamble **picks pdfcer's side of an
unsettled argument** and ships files only that side can open.
**Dropping it makes the two readings COINCIDE**: with the header at
byte 0, *absolute* and *header-relative* are **the same number**, and
the output is unambiguous to every reader. It also stops re-emitting
a **§7.5.2 violation** (*"The first line of a PDF file shall be a
header"*) the operator never asked pdfcer to keep.

**Why only a full rewrite may do it.** §5.1's table is the whole
licence: `save_full` promises per-object-definition byte identity, a
reloadable file and an identical raster — **explicitly not whole-file
identity**, because offsets legitimately move. Removing a preamble is
inside that promise and outside the other two.

**The generalisable form, stated because the next ambiguity will not
be about headers:** *where a format spec leaves a question genuinely
unresolved, emit the form under which the competing readings coincide
— not the form under which your own reading is correct.* Put to the
engineer as a candidate standing rule (**R165**) and **deliberately
not minted** by the filing that found it; see `ROADMAP.md`'s *third
defect the veraPDF gate found* entry, *Ledger*.

**[★ AMENDED 2026-08-07, fifteenth filing — `R165` IS MINTED. The
clause above is left exactly as filed; it is no longer current
status.]** The generalisable form quoted above is now **standing rule
`R165`**, ruled in by the operator against the filing librarian's own
recommendation and on that librarian's own counter-argument. **Ceiling
R164 → R165; R166 next free.** Two consequences bind **this section**:

- **§5.6.1 is R165's WORKED EXAMPLE, and is named as such in the rule.**
  The conflict resolved here — R165 pulling one way, §5.6's *do not
  normalize what the operator did not ask about* pulling the other — is
  the model for the next such conflict. **§5.6 remains the default;
  R165 is the exception that must be ARGUED for**, per case, with the
  ambiguous clause cited and the §5.1 save-mode contract checked.
- **R165 does not widen this exception by one byte.** Its own limit
  binds it to cases where the spec is **genuinely silent or
  self-contradictory**, and the paragraph below (*what §5.6 is NOT
  narrowed toward*) is unaffected: the trailing-space header, the
  cross-reference form, object streams and the hybrid refusal are all
  still outside it. **Binding text: `ROADMAP.md`, *Standing rules*,
  `R165`.**

**Also note what §5.6 is NOT narrowed toward.** No other normalisation
is licensed by this: not the `%PDF-1.4 ` with a trailing space (still
copied verbatim), not the cross-reference form, not object streams,
not the hybrid refusal. The exception is **the preamble, on the full
rewrite, only.**

### 5.7 The mutation writer, promotion, and the stale-copy reality

*(Added 2026-07-31, Pass 3.1 — the first Pass with real mutations.
Records both the mutation-writer design and a CRITICAL correction to
§5.2's original framing and decision 007 W3's mitigation. Corrections
are recorded forward; the archived 007 record is not edited.)*

**Design: one writer path, dirty set as an argument.** Pass 3.1
extended the Pass 3.0 writer rather than adding a mutation sibling:
`save_full` (like `save_incremental`) now takes a `&DirtySet` —
replacements (object number → new definition) plus a trailer patch,
with `changes_content()` distinguishing content-bearing edits from
metadata-only ones. `DirtySet::empty()` reproduces Pass 3.0's identity
behavior exactly, making identity a **strict pinned subset** of the
mutation writer, not a parallel code path that could drift. The dirty
set itself is produced by `EditSession` (§11.5) as a save-time diff
against the base revision, per §11.1. `/ID[1]` is derived per §14.4 in
`writer/fileid.rs`, exactly when a save writes at least one changed
object (§5.3); `/ID` is **never synthesised when absent**, in either
mode — the spec RAG's synthesise-on-full-rewrite recommendation was
declined (R41: stamping an `/ID` into a file that never had one is an
observable "pdfcer touched this" signal); a real Save-As path may
revisit.

**Promotion (R38) in practice.** A touched
`Provenance::ObjectStream` object is promoted to an uncompressed
object superseded by a type-1 xref entry; its container is left
byte-untouched. Coverage honesty: promotion is **fixture-covered, not
corpus-covered** — 75 corpus files hold 2,197 compressed objects, but
page objects are uncompressed in all of them, so the corpus rotation
gate never exercises promotion; the round-trip harness reports both
numbers so the gap cannot silently pass for coverage.

**The stale-copy reality — CORRECTION.** Decision 007 W3's mitigation
and §5.2's original framing claimed a full rewrite "closes the
stale-copy path" for promoted compressed objects. **FALSE.** Object
streams carry through **verbatim in BOTH save modes** — incremental
save never touches them by construction, and `save_full` re-emits
containers intact with zero promotions (§5.6, deliberately, because
rewriting a container perturbs every other object inside it). So a
promoted object's old value survives inside its untouched container
under *either* save mode. Binding consequence (documented at the
creating code as well): **the Redaction Pass must rewrite or decompose
every container stream that holds a redacted object.** R35 (refuse
incremental) is necessary but not sufficient; the redaction test that
greps saved bytes for removed content (§5.2) is what will hold this
honest, provided its fixtures include object-stream-compressed
content.

**Object creation and `/Size` suppression.** The Pass 3.1 fuzzer found
a real bug class here: creating a new object by raising `/Size`
**resurrected** xref entries that the base trailer's `/Size` was
suppressing (§7.5.4/§7.5.8: entries beyond `/Size` shall be ignored —
and real chains carry such entries, which then fail to parse when
exposed). Fix: `next_object_number` allocates above the **unfiltered**
chain maximum (never reusing a suppressed number), and creation is
refused by name when `/Size` suppresses entries
(`EditError::ObjectCreationWouldExposeHiddenObjects`, CLI exit 9);
editing existing objects still works on such files. Lesson:
`C:\personal_rag\pdf\lesson_20260731_xref_size_suppresses_trailing_entries_raising_resurrects.md`.

**A fourth source, found `Pass 67.0` phase E (2026-08-12, `d87fb58`) —
the newest cross-reference STREAM's own object number.** The three
sources above (`objects` map maximum, `/Size`-derived maximum, the
`highest_object_number` running counter) all miss it. §7.5.8 makes the
newest xref stream an indirect object like any other, and §5.7's own
promotion rule above already establishes that pdfcer's writer **reuses**
that object's number for the section it re-emits — but the object is
never filed in `objects` by the parser (it IS the section, not a body
object inside it), and nothing in the standard requires it to appear
in its own `/Index` or be covered by its own `/Size`. A file can
legally read `75 0 obj << /Type /XRef /Size 75 /Index [9 1 29 45] >>`
— object 75 exists, and all three prior sources answer 74.
`next_object_number` now also takes the current xref stream's own
object number (`SectionShape::Stream { id, .. } => id.num`, else `0`
for a classic table, which spends no number) as a fourth candidate in
the `max()` chain. Found on a real pdfium fixture
(`testing/resources/annotation_stamp_with_ap.pdf`) by `tools/embed-sweep`'s
pixel-identity oracle — an embedded font program came back as a 44-byte
cross-reference stream, because the session's newly created object was
allocated number 75, written there, and then silently overwritten by
the writer's own re-emitted xref stream at the same number. **Not
specific to font embedding** — every object-creating command
(add-text, add-image, annotations, form fields) was exposed on any
file shaped this way; it survived because no producer any existing
pdfcer fixture came from emits a newest xref stream outside its own
`/Size`. Fixture: `fixtures/synthetic/embed/embed-xrefstream-outside-size.pdf`.
Regression test: `a_created_object_never_collides_with_the_cross_reference_stream`.
Standing rule: **R189** (`ROADMAP.md`, *Standing rules*) — full record
at `ROADMAP.md`'s `Pass 67.0` phase E Shipped entry.

### 5.8 Flatten burns in by overlay-APPEND, not content-stream surgery

*(Added 2026-08-01, Pass 7.1 — the first operation that makes an
authored appearance part of a page's rendered content. Records the
design and why it is MORE minimal-diff than the in-place rewrite the
Pass scope anticipated.)*

**The problem.** Flattening a form field removes the interactive widget
and bakes its current appearance into the page so it renders identically
in a non-form-aware viewer. The obvious implementation — splice the
widget's appearance operators into the existing page content stream —
would rewrite that stream, which under §5.6 (never normalize) and the R46
identity discipline is exactly the destructive re-emission pdfcer avoids on
every object it did not logically change.

**The design pdfcer adopted.** Flatten does NOT touch the existing page
content stream. It:

1. builds a one-line overlay content stream that sets the widget's
   placement matrix (the §12.5.5 `fit_matrix_for` `/Rect`→`/BBox`
   transform) and `Do`-invokes the widget's existing `/AP` `/N` form
   XObject by name (`ContentBuilder::invoke_xobject` — `/Name Do`);
2. APPENDS that new stream to the page's `/Contents` array (promoting a
   single-stream `/Contents` to an array as needed);
3. registers the `/AP` `/N` XObject under the page's `/Resources`
   `/XObject` (`add_page_xobjects`, merging into the page's effective
   resources); and
4. removes the widget from `/Annots` and the field from `/AcroForm`
   `/Fields` (`remove_from_annots` / `remove_fields_from_form`), clearing
   `/NeedAppearances` if it was set.

The pre-existing page content bytes are never re-serialized. The only new
bytes are the appended overlay stream and the dict edits.

**Consequence — R46 keeps ZERO flattened-page exceptions.** Because the
existing content stream passes through byte-verbatim (§5.6 span
re-emission), the R46 re-emit-everything identity gate finds no new
divergence on a flattened page: GATE PASS over `fixtures/synthetic` +
`fixtures/external`, all divergences the known value-preserving `-0`→`0`
number re-spellings, zero corruptions. In-place surgery would have put
every flattened page's content stream through the canonical serializer,
surfacing (harmlessly, but noisily) the number-respelling class §5.6/R46
document — and, worse, would have been a genuine rewrite of content the
operator did not ask to reformat.

**R48 (flatten discloses its destructiveness) is still honored.** Flatten
is destructive in the sense R48 means — the interactive field is gone. But
under incremental save the field dict survives in the PRIOR revision
(recoverable), which flatten discloses; a `--full-rewrite` save produces a
file with no `/FT`/`/Tx` that still renders the burned value. Flatten uses
the STRICT certification gate (refused on any enforced `/DocMDP`, including
`/P 2` certified — proven by test), NOT the fill path's `/P >= 2` permit,
because flatten is a STRUCTURAL change to the page/annotation/field
structure, not a value fill.

**General pattern (recorded for future Passes).** Overlay-APPEND beats
content-stream-surgery whenever the goal is ADDITIVE burn-in (make
something already-authored part of the rendered page). Reserve true
in-place content-stream surgery for the one operation whose goal is
REMOVAL, not addition: **Redaction (Pass 8)** — the R46 named exception,
where covered operators must actually be deleted from the content stream
(and containers decomposed, §5.7), because visual masking is not removal.
The two operations are mirror images: flatten adds without rewriting;
redaction removes and must rewrite. This finding is escalated as a
`personal_rag/pdf` lesson
(`lesson_20260801_flatten_overlay_append_beats_content_stream_surgery.md`).

### 5.9 Every removal/scrub operation forces a full rewrite (R58 — generalizes §5.2's R35)

*(Added 2026-08-01, Pass 8.0 — Redaction landed, and the
`pdfcer-ui-specialist` review generalized R35 into a standing rule that
binds every future scrub operation, not just redaction-apply.)*

§5.2 established **R35** for redaction specifically: because incremental
save structurally preserves superseded content (§7.5.6 requires the
original contents be left intact and changes appended), a removal saved
incrementally leaves the removed content trivially recoverable in the
prior revision. The remedy — force a full rewrite, refuse incremental
save, drop `/Prev` so prior revisions are gone — is not unique to
redaction. It is the correct posture for **any** operation whose contract
is *removal or scrubbing of content*.

**Binding rule (R58):** every removal/scrub operation rides the same
forced full rewrite as redaction-apply. This includes, prospectively, any
**Sanitize / Remove-Hidden-Information / metadata-scrub** Pass pdfcer may
add. Three obligations travel with the rule:

1. **Force full rewrite, refuse incremental save.** The R35 mechanism,
   enforced in the writer, not left to each scrub Pass to remember.
2. **Decompose every object-stream container holding a scrubbed object**
   (§5.7). Refusing incremental save (R35) is necessary but NOT sufficient:
   object streams carry through verbatim in BOTH save modes, so a scrubbed
   object's old value survives inside its untouched container unless the
   container is rewritten/decomposed. Pass 8.0 proved this concretely — a
   redacted `/Info` compressed in an `/ObjStm` survives without §7.5.7
   Strategy B decomposition (`containers_decomposed >= 1`).
3. **Owe an absence test.** The scrub Pass greps the whole saved output —
   raw bytes AND every decoded content stream — for the removed content
   and asserts zero occurrences. This is R46 inverted: R46 proves presence
   (untouched content re-emitted byte-identical); the absence test proves
   deletion (removed content gone from the entire file). Pass 8.0's
   headline embodied it: `redact-apply` on `demo-secret.pdf` →
   `grep "SECRET" redacted.pdf` = 0 (control `marked.pdf` = 3).

The general framing (§5.8): flatten and redaction are mirror images —
flatten ADDS without rewriting (overlay-append), redaction/scrub REMOVES
and must rewrite (content-stream surgery + container decomposition). R58
is the standing-rule form of "removal is never additive, and never
incremental."

**Staleness flagged, text NOT changed (decision 022 §5.4, filed
`pdfcer-librarian` continuation 80, 2026-08-04 — full text:
`ROADMAP.md` Standing rules R58 and Open operator question (v)).**
R58's binding text above ("every removal/scrub operation forces a full
rewrite") is already contradicted by two shipped operations that
correctly stay under the project's default incremental save:
`EditSession::delete_object` (Pass 9c-min, `76485b5`, content-stream
surgery removing visible page geometry) and `delete_redaction_mark`
(Pass 8). Neither operation's contract is confidentiality — see §5.11,
below, which already established that distinction for in-place text
editing (a change, not a removal) and whose reasoning applies here by
extension (a removal whose contract is "no longer in the current
revision," not "provably unrecoverable"). Decision 022's own proposed
`EditSession::delete_annotation` (Pass 22.0, unbuilt) would be a THIRD
such exception if shipped without a wording fix. **The correction this
rule needs — narrowing "every removal/scrub operation" to "every
operation whose contract is CONFIDENTIALITY" (redaction, scrub, a
recovered-base save) — is deliberately not made in this entry.**
Decision 022 explicitly declines to narrow a standing rule's scope
solo, asking for operator confirmation first (`ROADMAP.md` Open
operator question (v)); this section records the discrepancy rather
than resolving it unilaterally. See §5.12, below, for the settled
(non-wording) part of this same finding: whether annotation deletion
joins the forced-full-rewrite family at all.

**★★ SECOND DEFECT, MEASURED 2026-08-13 (hundred-and-thirty-ninth
filing) — OBLIGATION 1 ABOVE ASSERTS A MECHANISM THAT DOES NOT EXIST,
AND OBLIGATION 2 IS REDACTION-PRIVATE. This is NOT the staleness note
above, and it is NOT open question (v).** The staleness note is about
R58's **SCOPE**: which operations the rule covers. This is about R58's
**MECHANISM**: whether *any* operation is bound by the rule at all.
They are independent, and answering (v) leaves every sentence below
true.

Obligation 1 reads, verbatim, *"Force full rewrite, refuse incremental
save. The R35 mechanism, **enforced in the writer, not left to each
scrub Pass to remember**."* Measured against the working tree at
`6c5124c`:

| claim in the text | command | result |
|---|---|---|
| a writer-level forcing mechanism exists | `grep -rn "force_full" crates/` · `grep -rn "requires_full" crates/` · `grep -rn "RequiresFullRewrite" crates/` | **0 hits each — 0 of 3.** No such identifier exists in any of the four crates |
| it is "not left to each scrub Pass to remember" | read `crates/pdfcer-core/src/redact.rs:1219–1224` | `apply_redactions` **remembers**: it calls `save_full(doc, &dirty, options)?` itself, under a comment naming R35. It is structural only in that the function returns finished bytes, so its own callers cannot override it — that binds `apply_redactions`, nothing else |
| the writer refuses incremental save for scrubs | read `crates/pdfcer-core/src/writer/save.rs:305–312` | the **only** refusal is `doc.loaded_via_recovery()` → `WriteError::RecoveredBaseForbidsIncremental`. Its own comment calls it *"Sibling of R35 / R58"* — a sibling, i.e. **R67 (§5.10)**, not an implementation of R58 |
| obligation 2's decomposition is general | `grep -rn "decompose_containers" crates/` | **2 hits, both in `redact.rs`** (`:1215` call, `:1670` private definition). Redaction-local |
| a verb's save mode is decided in core | `grep -rn "save_full\|save_incremental" crates/pdfcer-cli/src/` | `main.rs:10733/10735/10743` — the **shell** picks via `RoundTripMode`. No `EditSession` verb constrains it |

**Why this is worse than the scope staleness, and why it is a Pass.**
The scope problem produces named exceptions that a reader can see. The
mechanism problem produces **silence**: a future
Sanitize / Remove-Hidden-Information / metadata-scrub Pass — which
obligation 1 names *prospectively, by name* — would compile, review
clean, and save incrementally, and would then satisfy obligation 3 with
an absence test **that cannot fail**, because an absence assertion over
incrementally-saved bytes is vacuous (the superseded object is still in
the file; `C:\personal_rag\pdf\lesson_20260813_absence_assertion_vacuous_under_incremental_save.md`).
**Two safeguards, both reading correct, both inert.** And per decision
058, the only layer that could catch it today is a shell choosing
`RoundTripMode::Full` — the layer that may be replaced wholesale.

**One thing the remedy genuinely cannot do, recorded here because
nothing in `redact.rs` says it.** `crates/pdfcer-core/src/writer/save.rs:584`
returns `WriteError::HybridFullRewrite` for a `SectionShape::Classic
{ xref_stm: Some(_) }` document — a **hybrid-reference file cannot be
full-rewritten at all** (§7.5.8.4, R33 — see §5.6). Since
`apply_redactions` calls `save_full` and `RedactError::Write(#[from]
WriteError)` propagates, **redaction on a hybrid file ERRORS rather than
under-scrubbing.** That failure direction is correct and is not a
defect. It does mean R58's remedy is *unavailable* on that file class,
and any future scrub Pass told to "ride the forced full rewrite" will
meet it as a surprise.

**★ CORRECTED 2026-09-09, `Pass 281.0` (`1177221`) — the paragraph
above was an accurate measurement of the working tree at `6c5124c` and
is left standing as the historical record of that audit; it no longer
describes current behaviour.** `WriteError::HybridFullRewrite` and the
`{ xref_stm: Some(_) }` refusal it names are gone from
`save.rs`'s general case — a hybrid-reference file now **fully
rewrites and redacts** (see §5.6.1's own dated correction, above,
which is the authoritative current text). The narrower successor
refusal — a hybrid file whose `/XRefStm` does not itself parse — still
errors rather than under-scrubs, so the sentence "redaction on a
hybrid file ERRORS rather than under-scrubbing" is now true of a
strict subset of hybrid files, not all of them.

**Obligation 1's wording is deliberately NOT rewritten here.** The fix
is `ROADMAP.md`'s **`Pass 73.0`** (*Next up*), whose criterion 6
requires this text to be corrected in the same filing that gives it a
real mechanism to describe, and whose criterion 2 requires that
mechanism to carry a **greppable identifier** — so the next audit of
R58 is answerable by `grep`, which this one was not.

**Addendum, 2026-09-09 (`Pass 284.0`, `ea4acb3`, decision 146).** R58's
obligation 3 ("owe an absence test") is sharpened by this Pass, not
revised: the absence test proves removed content is gone from objects a
carrier *examined*, and this Pass establishes that the set of objects
worth examining is **every object the cross-reference table lists**, not
every object the document graph *reaches* from the trailer/catalog. New
fourteenth carrier `redact::residual_sweep` sweeps the difference between
those two sets directly, scoped by the xref table's own enumeration
rather than by a computed reachability walk — reachability computations
on this format fail *silently*, not loudly (object streams reached by a
type-2 xref entry, cross-reference streams by byte offset, the
linearization dictionary unreferenced by a `shall`), which is the
argument decision 146 makes in full. No change to R58's core mechanism
(force full rewrite, decompose containers) — this closes a gap in *what
gets swept*, not in *how a rewrite is forced*. Full record: §12's
2026-09-09 entry, decision 146; standing rule `R249`; `ROADMAP.md`
*Shipped*, `Pass 284.0`.

### 5.10 A cross-reference-recovered document forces a full rewrite (R67 — third sibling of §5.2/R35 and §5.9/R58)

*(Added 2026-07-31, decision 013. **FLIPPED TO SHIPPED/ACTIVE 2026-08-01**
— Pass 13b (rebuild-by-scan recovery) shipped this session; the contract
below is now enforced code, not a forward-looking design note. R67 is now
IN FORCE. See `ROADMAP.md` Shipped, Pass 13b, for the acceptance numbers:
566 previously-failing real-world files now open (1,109-file corpus), zero
regression on the 2,907-file veraPDF corpus, `*-fail-*` reconciliation
complete.)*

§5.2 (R35, redaction) and §5.9 (R58, all removal/scrub) force a full rewrite
because incremental save structurally *preserves* superseded content.
Cross-reference recovery forces a full rewrite for a **different but equally
structural** reason: a document loaded via rebuild-by-scan had an **invalid
base cross-reference table**. An incremental append onto it would write a new
section whose `/Prev` points at a cross-reference section that does not
correctly exist — the appended file would be self-inconsistent and would fail
to reload. **Incremental-append onto a broken base is structurally
impossible, not merely undesirable.**

**Binding rule (R67):** a recovered document's save is a **mandatory full
rewrite** (`save_full`) emitting a fresh valid classic xref/trailer/
`startxref`. `save_incremental` on a recovered document is **refused by
name** (`WriteError::RecoveredBaseForbidsIncremental`). The recovered/rebuilt
status is flagged on the `Document` (a `recovery: Option<RecoveryReport>`
field), disclosed in the CLI + GUI, and counted (R20) — recovery is a
reviewable fact, never a silent repair (fuzzy-never-sneaky).

**Interaction with §5.6 "never normalize" (stated explicitly so a future
reader does not think recovery breaks R33):** §5.6 governs *clean
passthrough* objects — it forbids reformatting a file the operator loaded
intact. It does **not** bind a recovered file: the base was invalid, so
emitting a fresh normalized classic xref (`SectionShape::Classic { xref_stm:
None }` — the most compatible form) is the correct, honest output, not a
normalization violation.

**Why this never perturbs a clean file:** recovery triggers **exclusively on
the strict-load error path** (`document.rs::from_bytes` only invokes it when
`load_xref_chain` / `probe_header` returned `Err`). A file that loads cleanly
never enters recovery code, so the round-trip/minimal-diff invariant for
clean files (§5.1) is preserved **by construction**, not by policy. Full
record: `docs/decisions/013-xref-recovery.md`; standing rule R67.

**★ AMENDED 2026-08-07 — R67 IS UNCHANGED AND WAS NOT VIOLATED. The
failure was UPSTREAM of it, and the distinction is the useful part.**
`49dfe81` fixed a case where a recovered document's save produced a file
naming a `/Pages` object that was **not in it** — which looks at first
glance like a §5.10 breach and is not. R67 did exactly what it promises:
the save was a full rewrite emitting a **fresh, valid** classic
xref/trailer/`startxref`. **The xref was valid over an INVENTORY THAT WAS
SHORT.** `parse_object_at` requires `endobj` (§7.3.10), so an object whose
only damage was a missing four-byte keyword was never registered by
`confirm_candidates`, and R67 then faithfully emitted a correct table of
everything recovery had — including a catalog pointing at something it did
not.

**Stated as the reusable sentence, because it generalises past this
defect:** ***a valid cross-reference table over an incomplete inventory is
still a broken document.*** R67 guarantees the table is well-formed and
self-consistent; **it guarantees nothing about completeness**, and cannot,
because completeness is decided one level up in `recover.rs` /
`parser.rs`. Any future recovery work should read R67 as a **write-side**
contract only. Full record: §12's fifteenth 2026-08-07 entry;
`ROADMAP.md` *Shipped*, the `first defect the veraPDF gate found` entry.

### 5.11 In-place text editing is surgery-under-incremental-save, NOT a fourth forced-full-rewrite sibling (decision 014 — SHIPPED 2026-08-01, Pass 14.0–14.3 all COMPLETE)

*(Added 2026-08-01 as a forward-looking design note ahead of Pass 14.1;
FLIPPED to shipped/active 2026-08-01 on decision 015's filing — all four
Pass 14.x slices are now shipped (see `ROADMAP.md` Shipped). This section
records the actual module layout, mirroring how §5.10 was rewritten on
Pass 13b's ship.)*

§§5.2/5.9/5.10 (R35/R58/R67) form a **forced-full-rewrite family**: every
member exists because incremental save structurally *preserves* superseded
content, which is disqualifying for redaction, scrub, and a recovered-base
save alike. **In-place text editing is confirmed NOT a fourth member of
that family.** Editing is a content *change*, not a removal or a
recovery — it uses the project's **default** incremental save (R36/R70),
and prior text surviving in history is a disclosed, accepted consequence,
not a defect. Truly removing text remains Redaction's job (§5.2/R35);
conflating the two would either weaken redaction's absence guarantee or
force every keystroke through a full rewrite that drops revision history
for no security reason.

**Shipped module layout.** `crates/pdfcer-core/src/text_edit/`:

- **`model.rs`** — the derived Run→Line→Block hierarchy over Pass 4's
  extraction (`Block`/`Line` with union `bbox`, `line_indices`, `column`;
  `BlockRecognitionOptions` — `column_overlap_ratio`,
  `paragraph_leading_ratio`, `indent_ratio`, `line_baseline_ratio`;
  `BlockDiagnostics` counting every inference, R72). `line_at`/
  `word_range_at`/`line_range_at`/`word_bounds` accessors (added Pass 14.3
  for caret/selection navigation).
- **`edit.rs`** — the advance-preserving REMOVE→REPLACE content-stream
  surgery (extends Pass 8.0's `redact.rs` interpreter, R69/R47), the
  inverse-encoding builder (Unicode→code, inverting Pass 4's §9.10.2 decode
  ladder), `FollowerDisposition` (same-line relayout past the original
  margin, disclosed), `EditReport.disclosures` (verbatim-surfaced
  refusals/warnings), and the R-INV-1..8 font-on-edit gate (R71) keyed on
  `GlyphSource` + glyph presence (decision 012). Also owns `EditSession` —
  the undo/redo command log — split as `plan_edit(...) -> EditPlan` /
  `plan_format(...) -> FormatPlan` (shared by both the free-function path
  and the session path) + `write_incremental`; `CommandKind::{EditText,
  FormatText}` (Pass 14.3 addition; `ReflowBlock` is Pass 15.1's addition,
  see §12's decision-015 entry) apply as ONE undo-able command each over
  the session's in-memory object graph, proven byte-identical to the free
  function for a single edit.
- **`format.rs`** — formatting-on-selection (Pass 14.2): size (`Tf`), fill
  colour (`rg`/`g`/`k`, storing the operator's actual chosen colour space —
  RGB/CMYK/gray — unlike Acrobat, which always stores `DeviceRGB`
  regardless of the picker mode shown), gated font-family/style change
  (re-encode into an available covering face, else refuse-and-disclose).
- **`vartext.rs`** — reused verbatim for reflow line-breaking (Pass 15.x);
  not itself part of the 14.x edit path but the shared line-breaking
  substrate.

**GUI (`pdfce-gui`, Pass 14.3):** `CanvasTool::TextEdit` — click→caret,
Shift-click→extend, double-click→word, drag→select; `TextEditState`/
`PendingEdit` in `main.rs`; live preview (mask + draft text + a dashed
"PREVIEW — not yet applied" tag), Accept/Reject buttons, the verbatim
disclosure/refusal strips, a read-only block-boundary review overlay, and
the property bar (size / colour-model / font, trust-labelled per R63).
`ui_text.rs` carries the ~30 new user-facing strings. Deferred, named
non-goals: triple-click/arrow-Home-End caret nav (accessor plumbing already
shipped, wiring deferred), split/merge/reorder of recognized blocks,
commit-on-focus-loss for the property bar (an explicit Apply button is used
instead).

The edit mechanism **is** content-stream surgery — the second sanctioned
page-content-rewriting operation after Pass 8.0's redaction interpreter
(R47's surgery-vs-overlay line), extended from REMOVE to REPLACE. It reuses
Pass 8.0's §9.4.4 advance-preservation machinery so un-edited same-line
text does not slide. The crux design call is **font-on-edit**: a keystroke
is applied only when the run's font can already supply the glyph (an
embedded program's existing glyphs, or a non-embedded font's full
bundled/supplied coverage per decision 012); a glyph an embedded *subset*
lacks is refused-and-disclosed by name, never faked or silently substituted
(R71). Block recognition is derived, counted, reviewable structure over
Pass 4's extraction output — never authoritative, never a silent re-layout
(R72; reflow itself is Pass 15.x, see below). An edit inside a
marked-content sequence preserves its BDC/EMC + MCID wrapper and discloses
staleness rather than corrupting the structure tree the way Acrobat's own
in-place edit is known to (R73) — minimal-diff turned into an
accessibility guarantee.

**Not a fourth forced-full-rewrite sibling, confirmed by the shipped
gates.** Every Pass 14.x ship re-verified `cargo tree -p pdfcer-core` /
`-p pdfcer-render` zero egui/eframe/winit/wgpu/glow (GUI-core separation
intact); the round-trip/R46 gate stays green for untouched objects; only
the edited content stream(s) (+ changed resource/font dict) are re-emitted.

**Forward pointer — reflow (FF-A) is a separate Pass family, not an
extension of 14.x's module boundary.** Decision 015 (2026-08-01) scopes
within-block offline reflow as `ROADMAP.md`'s ★ Pass 15.x, building a
`ReflowEngine`/`ReflowPreview` on top of this same `text_edit::model`/
`edit` substrate (15.0 read-only engine, 15.1 surgery +
`CommandKind::ReflowBlock`, 15.2 canvas UI). Reflow remains an *opt-in*
beside — never a replacement for — the default single-line relayout
described above (R75). Full design: `docs/decisions/015-ffa-within-block-offline-reflow.md`;
decision-log entry below.

**Forward pointer — FF-H (direct text-state formatting) re-scoped and
sliced by decision 019 (2026-08-03), a shared prerequisite for FF-C and
FF-B, not a peer extension of this section's surgery model.** FF-H's
own emission mechanism reuses this section's `set_ops`/`restore_ops`
pattern (Pass 14.2's `format.rs`) directly — `Tc`/`Tz`/`Ts` slot into
the existing `pre | set_ops | mid | restore_ops | post` splice with no
structural change. Two new architectural facts this decision
establishes, both binding on any future text-state-emitting code in
`pdfcer-core`: (1) **`q`/`Q` are illegal inside `BT…ET`** (ISO 32000-1
§8.2 Table 51/Figure 9) — ambient text-state restoration after a
formatted run is therefore always **restore-by-value**, resolved by a
ladder in the same family as `TextColor::restore_bytes` (fill colour)
but with one more rung than that ladder needed (R88, corrected by
Amendment A below — see the decision-log entry for why a third
"available" case exists between "observed raw bytes" and "refuse"); (2)
**`Tc`/`Ts` are unscaled text-space quantities (§9.3) and are not
rescaled by `Tfs`** — pdfcer's model stores them as a discriminated
`Absolute | Relative` quantity so a font-size change cannot silently
mis-scale a stored rise or tracking value (R89 — `Tf`/`Tfs` themselves
are explicitly OUT of this unification, per Amendment A item 3, to
avoid perturbing already-published glyph positions). A third finding
was a code-hygiene one rather than a spec fact: ambient
`Tc`/`Tw`/`Tz`/`Ts` state was independently tracked three times in
three different modules (`text_extract::page::TextState`,
`text_edit::edit::Walk`/`reflow_apply::BlockTextState`,
`vector::decompose::GState`) with zero shared publication.

**Narrowed by decision 019 Amendment C (Pass 19.2, `ebe35d8`):** the
"one shared consolidation" claim above is specifically about **the six
§9.3 text-state parameters** (`Tc`/`Tw`/`Tz`/`TL`/`Ts`/`Tr`) that R88's
ladder covers. Synthetic bold (§3.6/R90) introduced two more tracked
quantities — stroke line width and stroking colour — that are
**ordinary graphics state shared with path painting, not text state**,
and are tracked and restored separately from the `TextStateParams`
model rather than folded into it; a synthetic-bold run's stroke
settings would otherwise leak into later stroked *paths* on the page,
not just later text. Pass 19.2 also added `Tm`/`Tlm` tracking to
`text_edit::edit::Walk` (`BT` reset, `Td`/`TD`/`T*` derivation,
§9.4.4 advance accumulation, a `matrix_known` honesty flag, and a new
`Rec::EndText` variant) — needed for the absolute-`Tm`-required-for-
followers refusal gate (see below), and not anticipated by the
original decision text or by Amendment A's `Tf`/`Tfs` exclusion. So:
exactly one definition of the six text-state parameters in
`pdfcer-core`, plus two separately-tracked shared-graphics-state
parameters, plus a separately-tracked text matrix — three distinct
things, not one, and the distinction is deliberate rather than an
oversight.

**Pass 19.0 SHIPPED (2026-08-03, `38fffad`) — this consolidation is now
built, not merely planned.** New `pdfcer-core/src/text_state.rs` in two
layers: `TextStateParam`/`TextStateParams` (parameter identity +
resolved values, for arithmetic-only consumers) and
`AmbientValue`/`AmbientOrigin`/`AmbientTextState`/`AmbientRestoreError`
(values plus restore provenance, four-rung — see Amendment A below).
One `apply_operator` update rule is now shared by all three walks.
`GlyphProvenance` gains `text_state` (the resolved ambient parameters at
the glyph's show point) and `composite` (whether this glyph came from a
composite/synthesized run) fields, published for the first time —
previously dropped at provenance-construction time. `Tw` is tracked and
preserved but still **not** promoted to a direct authoring control by
this decision — its inter-word-distribution job stays with 15.1's
`TJ`-based reflow design, and any future promotion is gated behind a
corpus census (R91), never built speculatively. Synthetic bold/italic
(Tr 2 + `Tm` shear, R90) is new authoring surface, not a data-model
change, and does not warrant its own subsection here. `cargo test
--workspace` 1613 → 1643; zero new Cargo dependencies;
`fixtures/synthetic` roundtrip byte-identical (verified from a genuine
pre-change worktree build, not a `git stash` on an already-clean tree —
see `D:\dev\rag\rust\git_stash_on_clean_tree_makes_before_after_comparison_vacuous.md`
for why the first comparison attempt was vacuous). Full design:
`docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md` +
Amendment A; decision-log entries below; Pass slicing (19.0
consolidation SHIPPED → 19.1 `Tc`/`Tz`/super-subscript IN PROGRESS →
19.2 `Ts`/synthesis → 19.3 GUI → 19.4 `Tw` conditional) in
`ROADMAP.md`'s ★ Pass 19.x entry.

**Pass 19.1 SHIPPED (2026-08-03, `603b051`) — `Tc`/`Tz`/superscript/
subscript authoring now built, not merely planned.** Rides the existing
`pre | set_ops | mid | restore_ops | post` splice with no structural
change; new `MetricSpec`/`ScriptPosition`/`ScriptMetrics` types,
`push_state_param` (the R88 ladder's application point). CLI:
`format-text --char-spacing`/`--h-scale`/`--superscript`/`--subscript`/
`--no-script`. Superscript/subscript ratios (0.60× size, +0.34×/−0.18×
rise, both of the BASE size per decision 019 Amendment B item B.3) are
pdfcer's own choice, not an Acrobat parity claim (Acrobat's own values
are an unsourced gap in the parity catalog). **Decision 019 Amendment
B, filed same day, corrects three things found while building this
slice** — see the decision-log entry immediately below for the full
account: (1) the `Tz`×justify disclosure named the wrong mechanism (the
real cause is the formatted run's width delta, not a `TJ`-adjustment
rescale — the rescaled-`TJ` premise is true in general but the specific
`TJ` numbers carrying justify slack sit outside the edit's set/restore
wrap); (2) the `Ts`-rise spec-citation flag was verified NOT to be an
error in this document (only in `text_state.rs`, already fixed); (3)
R89's "`Tfs`" is now stated explicitly as the BASE size. Also fixed in
this slice: a live defect where `EditSession::format_text`'s own
hand-listed no-op predicate had drifted out of sync with the `FormatRequest`
fields Pass 19.1 added, making a spacing-only request a phantom no-op on
the GUI-facing `EditSession` path specifically (the CLI's `set_format`
path was unaffected) — replaced with `req.is_empty()` so the predicate
cannot drift again. Second occurrence of the same bug shape as Amendment
A.4's missing `q`/`Q` arms (a hand-maintained check mirroring a
structure's shape, rather than derived from it) — see `ROADMAP.md`'s new
standing rule R92.

**Pass 19.2 SHIPPED (2026-08-03, `ebe35d8`) — free-form `Ts` and
synthetic bold/italic now built.** New
`crates/pdfcer-core/src/text_edit/synth.rs`: `StyleSynthesis` (the shared
policy type used by both `format.rs` in-place edit and `addtext.rs`
Add-Text), `SynthesisPath` (the *only* asymmetry between the two paths
is remedy *order*, per decision 019 §3.6), `SynthesisOffer`,
`OBLIQUE_TAN`/`BOLD_STROKE_RATIO` constants, `shear_into` (a true
matrix premultiplication, not a naive single-component overwrite —
tested against a pre-rotated matrix, where overwriting just the shear
component loses the lean entirely), `matrix_scale` (determinant-based,
so a shear does not perturb the derived bold stroke width), and
`detect` (reload-time re-detection of synthetic styles by byte
inspection, pdfcer's own and other producers'). CLI: `--rise`,
`--bold-synthetic`, `--italic-synthetic`. The render-honours-`Tr
2`-and-sheared-`Tm` prerequisite named in the decision was confirmed
**empirically, by mutation testing** — a new
`crates/pdfcer-render/tests/synthetic_style_render.rs` rasterizes built
fixtures and interrogates pixels, then deliberately breaks the renderer
three separate ways (drop mode-2 stroking, zero the `Tm` shear
component, zero the rise) and re-runs to confirm each mutation fails
exactly the tests it should — the standard the original by-inspection
prerequisite check should have met (see the decision-log entry for the
general methodology finding). **Decision 019 Amendment C filed**
(six corrections found while building this slice — the wrong restore
set named for stroking colour/line width, a narrower-than-written
absolute-`Tm`-required-for-followers refusal, a disclosed two-of-three-
factor bold-width formula, unanticipated `Tm`/`Tlm` tracking needed in
the authoring walk, two named unhandled conflicts refused by name
(rise-vs-toggle, synthetic-italic-vs-`--pin`), and Add-Text synthesis
flagged as not wired despite the shared type existing) — see the
decision-log entry immediately below for the full account, and
`docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md` Amendment
C for the complete record. **No GUI code and no GUI verification this
Pass** (slice 19.3, the property surface, is a separate
`pdfcer-ui-specialist` dispatch) — verified via the CLI oracle and a new
R85 case, exercising the same `EditSession` path the GUI will use.

**Pass 19.3 SHIPPED (2026-08-03, `74052d3`) — the GUI property surface
is now built, AND a defect that had silently disabled every property-
bar Apply since Pass 14.3 is fixed.** GUI slice: Option-B wrapper
(`StyleOutcome`/`StyleResolution`/`probe_synthesis`/
`preview_style_resolution` in `pdfcer-core`, read-only and side-effect-
free — `preview_style_resolution` calls `gate_synthesis` up to three
times rather than re-deriving, proven byte-equal to a non-previewed
commit) plus the `pdfce-gui` property tree (`MetricUnit`/
`BaselineChoice`/`AmbientSnapshot`, 11 new `TextEditState` fields, five
`FormatOp` variants). **The headline finding is a data-contract defect
predating this decision entirely, exposed only because this slice
stopped discarding failed anchor lookups with `.ok()`.**
`GlyphProvenance::operator_span` (§9.4, published by the extraction
walk) names the span of the operator token ALONE; `text_edit::edit`'s
`OpRec` (the authoring walk's own record) names the OPERAND-INCLUSIVE
extent of the same operation. `find_anchor`'s pinned-request path
(`pin_names_operator`) compared the two spans for EXACT EQUALITY —
since the GUI always pins from published provenance, and the authoring
walk always records the wider span, **the two never matched, and every
GUI-issued `format-text`/`edit-text` Apply since Pass 14.3 refused
with `NoMatch` before reaching the surgery**, invisible in the running
application until this slice made the failure visible instead of
swallowing it. **Fix:** `pin_names_operator` now accepts either
convention — `pin.end() == r.end && pin.start >= r.start` — since two
operations in one content stream cannot share an end offset; a
regression test proves the relaxed match still DISCRIMINATES a
near-miss span (does not degrade into false-positive editing of the
wrong run). **Verified by mutation:** reverting to exact-equality
matching makes a new regression test fail; restoring the fix makes it
pass. Both doc comments that had independently asserted the two
conventions already agreed (`EditRequest::pinned_span`'s "matches the
same span," `text_edit/page.rs`'s "the surgery locates the operator by
exactly this span") are corrected in place — this is the architectural
fact this section previously stated incorrectly, now fixed at the
source. `cargo test --workspace` 1708 → 1722, 0 failed; `cargo tree`
re-verified clean; zero new Cargo dependencies. Full record: the Pass
19.3 Shipped entry, `ROADMAP.md` (top of Shipped), and the new standing
rule R93 (methodology: a code comment asserting a cross-module contract
is a claim, not evidence, even when two independent comments on both
ends of the contract agree — third occurrence of this failure shape in
this project, after decision 018's `refresh_pages` comment and the
`.gitattributes` ordering incident).

**The `Tw` census (decision 019 §3.3, gating slice 19.4) has been RUN
(2026-08-03) — Amendment E.** New out-of-workspace crate
`tools/tw-census` measured reachability, keyed by show operator
(`GlyphProvenance`'s `(ContentStreamRef, ByteSpan)`), over the Pass-11
render-fidelity corpus (4,012 files; 1,224 text-bearing after excluding
627 unloadable + 2,172 zero-show-operator files): **91.6% of show
operators / 97.4% of shown glyphs are on a simple (non-composite)
font** — the BUILD band (≥60%), not marginal. Slice 19.4 is cleared to
build but has **not started**; the engineer prioritized a real
document-loading defect this same census sweep found (see below).
**§3.2 reason 2 — that Type0/Identity-H composite embedding is "a
large and growing share" of documents, which would make a `Tw` control
inert on most files — is FALSIFIED on this corpus**: 81.2% of
text-bearing documents contain no composite run at all. The "growing"
half of that claim is untestable on this corpus (its files are older
PDF-tooling test suites, not a sample of recently-produced documents).
Full numeric record, sub-corpus breakdown, and both caveats (corpus
vintage; corpus composition — PDF-tooling test suites, not organic
documents): `docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md`
Amendment E; `ROADMAP.md`'s continuation-67 In-progress entry.

**Same sweep found a pdfcer document-loading defect, engineer-verified —
FIXED 2026-08-03, committed `409a6b5`:** 341 corpus files (8.5%)
refused to open at all with "page /Contents is neither a stream nor an
array of streams." Hand-verified NOT a correct rejection —
`fixtures/external/qpdf/qpdf/qtest/qpdf/add-contents.pdf` is a legal
file per ISO 32000-1 (`/Contents [ 4 0 R 5 0 R 6 0 R ]`, all eight
objects present, three intact text-bearing content streams) that pdfcer
refused outright. **The originally-filed diagnosis was wrong in
mechanism**, not just incomplete: Pass 13b's rebuild-by-scan recovery
does not undercount objects — the scan correctly proposes all 8
headers, but object 5 was dropped at the strict-confirmation step with
"endstream not found where /Length points." The real cause:
`add-contents.pdf` is an **LF file converted to CRLF**, so every
`/Length` (measured on the LF form) is now short by one byte per
internal line, and the declared extent lands mid-content — the same
CRLF shift that broke `startxref`/`xref` in the first place (why
recovery engaged at all) also silently ate the content stream
recovery existed to save. One damage event, two symptoms.

**Two fixes, kept deliberately separate (both new, both opt-in/scoped
rather than changing default strict parsing):**
1. **`StreamLengthPolicy`** (`Strict` default, unchanged;
   `RecoverFromEndstream` re-derives a stream's extent from the
   `endstream` keyword — reachable only from existing recovery paths).
   This is not a heuristic: §7.3.8.2 *defines* `/Length` as the byte
   count "to the last byte just before the keyword `endstream`," so
   deriving the extent from the keyword reads the same normative
   sentence from its other end.
2. **Per-element `/Contents` degradation.** A `/Contents` array
   reference resolving to null contributes nothing and is dropped
   (§7.3.10's dangling-reference-is-null-object rule + Table 30's
   `/Contents`-is-optional rule — degrade the one element, not the
   document); a genuine *type* error (a non-reference array element,
   or a reference resolving to the wrong object type) is still
   `BadContents`, unchanged. A direct `null` (not an unresolved
   reference) is treated as absent per §7.3.9 and deliberately excluded
   from the `contents_unresolved` disclosure count, which is reserved
   for content that should have been present and was not. Counted and
   surfaced, never silent: `RecoveryReport.stream_lengths_recovered`
   (CLI + GUI recovery banner) and `Page.contents_unresolved` →
   `render::Diagnostics.contents_streams_unresolved` /
   `TextDiagnostics.contents_unresolved` (CLI stable line, GUI
   "unsupported items" detail list).

**★ AMENDED 2026-08-07 — A THIRD OPT-IN RECOVERY POLICY NOW EXISTS. The
numbered pair above is UNCHANGED and still describes the `/Contents`
defect's two fixes; this marker exists so a reader looking for *"which
parser policies can the recovery path turn on?"* does not stop at two.**

3. **`TerminatorPolicy`** (`Strict` default, unchanged;
   `RecoverAtNextHeader` accepts a definition whose body parsed cleanly
   but whose `endobj` is missing) — added `49dfe81`, reachable **only**
   from the rebuild-by-scan recovery path, exactly like
   `StreamLengthPolicy::RecoverFromEndstream`. **It is that policy's
   sibling by construction, not by analogy:** both are cases where the
   file contradicts §7.3, **which of two readings to believe is a POLICY
   choice rather than a spec choice**, and pdfcer makes it an explicit
   parameter instead of a hidden default. The leniency accepts **only
   when the terminator is an integer**, so it cannot swallow trailing
   garbage, and the object's provenance is **`RecoveredFile`** — **R94's
   second instance**, and for the identical reason the `/Length` repair
   above needed the variant: the source bytes no longer agree with the
   value, so verbatim re-emission would carry the malformation into the
   saved file. Counted and surfaced, never silent:
   `RecoveryReport.missing_endobj_recovered` → CLI
   `missing-endobj-recovered=N` plus a prose NOTE citing §7.3.10.

**Why a third one was needed at all, in one sentence:** the `/Contents`
work fixed the case where a recovered object's **extent** was wrong;
`49dfe81` fixed the case where a recovered object's **terminator** was
missing and the object was therefore **never registered** — different
failure, same requirement that the repair be explicit, bounded, counted,
and provenance-invalidating. Full record: §12's fifteenth 2026-08-07
entry.

**The round-trip gate caught a bug in the fix itself.** The first
repair attempt corrected the recovered object's byte span but left its
stale `/Length` untouched; because the writer copies `Provenance::File`
objects verbatim, `save_full` produced a file pdfcer itself could not
reload — a self-inflicted §5.10 round-trip violation, caught by the
gate that contract exists to enforce. Resolved by adding a third
`Provenance::RecoveredFile` variant to the already-`#[non_exhaustive]`
`Provenance` enum, meaning "bytes exist but no longer agree with the
value" — objects in this state are always re-serialized (recomputing
`/Length`) rather than copied verbatim. §5.10 is not weakened: the
mutation is deliberate, disclosed via the existing `RecoveryReport`
channel, and both existing verbatim-passthrough call sites already
excluded non-`File` provenance via `let-else`, so both were correct by
construction against the new variant. **Generalized as standing rule
R94** (`ROADMAP.md`, Standing rules): a repair that mutates a value
must invalidate any "these-bytes-are-verbatim" provenance attached to
it, or a downstream verbatim-copy path re-emits stale bytes beside a
corrected value. **R95** states the per-element `/Contents`-degrade
rule as binding (extends the R67 forced-full-rewrite-on-recovery
family with a read-side sibling: dangling optional/array-valued
content degrades in place, it never condemns the whole document).

**Result:** 289 of the 341 files now open with real content (verified
independently by re-running `tools/tw-census`: text-bearing documents
1,224 → 1,513, page-tree load failures 497 → 163, `BadContents` 341 →
1, zero regressions). Full numbers, sub-corpus breakdown, and gates:
`ROADMAP.md`'s `/Contents`-defect-fix Shipped entry (top of Shipped).

**Pass 19.4 SHIPPED (2026-08-03, `a1638f4`) — `Tw` direct-authoring
control now built; decision 019 / FF-H is COMPLETE end-to-end (all five
slices 19.0–19.4 shipped).** Rides the existing `push_state_param`
four-rung ladder and `pre | set_ops | mid | restore_ops | post` splice —
no new authoring path. `FormatRequest::set_word_spacing` shares the same
`MetricSpec::{Absolute, Relative}` model `Tc` uses (Pass 19.1), resolved
against the BASE font size per Amendment B item B.3; `FormatError::
WordSpacingComposite`; `FormatReport::word_spacing_change` +
`word_spacing_affected_codes`. `Tw` enters the §9.4.4 advance via
`eff_tw` and joins the existing justify-invalidation trigger set
(`disclosure_justify_invalidated`, Pass 19.1's mechanism, not a second
path). CLI `--word-spacing V[pt|em]`, generalizing `parse_char_spacing`
into `parse_text_metric` so `Tc`/`Tw`/`Ts` share one grammar and one
error voice. GUI row live for simple-font runs; the composite strip
stays the existing read-only R83 presentation.

**Amendment F filed** (three findings this slice's build surfaced,
none anticipated by the original decision or Amendments A–E — full
account in `docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md`
Amendment F): (1) **the composite refusal (R91) was UNREACHABLE as
originally implemented** — `match_run` filters every composite run to
`NoMatch` (its decoded text is always empty) before the font-aware gate
ever runs, so R91 would have shipped as referenced, documented, never-
executed dead code; fixed by hoisting font resolution above `match_run`,
verified by a test proving the gate now fires AND a second test proving
the OTHER three controls stay live on the same composite run (a
specific capability gate, not a blanket composite refusal). Generalized
as `D:\dev\rag\rust\dead_guard_clause_behind_a_filter_the_guarded_case_cannot_pass.md`.
(2) **A named limit:** the fixed refusal is reachable via the pinned-span
path but not via CLI `--find` (composite-run text search finds nothing,
so the CLI reports "not found in an editable run," a less specific
message than the decision describes) — closing this needs composite
decoding in the authoring walk, FF-E's scope, not this slice's. (3)
**`Tw` is multiplied by `Th`** (§9.4.4, same basis as `Tc`) — the
decision names this only as a reason `Tw` is an awkward control, never
as something needing disclosure; the word-spacing disclosure now quotes
the effective delivered value whenever `Th ≠ 1`. Filed to
`C:\personal_rag\pdf\lesson_20260803_word_spacing_multiplied_by_horizontal_scaling.md`.
Also recorded, not a correction: `Some(0)` affected-spaces is emitted
and disclosed as a real answer (a `Tw` set on a code-32-free run is
genuine, legitimate state), and Amendment A.1's fourth restore rung
needed no change to correctly handle `"` setting `Tw`/`Tc` as a
side-effect of showing text — its first concrete, load-bearing test.
`cargo test --workspace` 1738 → 1756, 0 failed; zero new Cargo
dependencies; round-trip proven non-vacuous by two binaries differing in
both MD5 and size. Full record: `ROADMAP.md`'s Pass 19.4 Shipped entry
(top of Shipped).

**MILESTONE — decision 019 / FF-H is COMPLETE end-to-end.** This closes
item #3 ("finish off all the text handling stuff") of the operator's
four-item priority sequence as far as FF-H's own scope goes (FF-C and
FF-B remain unscheduled, per this decision's own Q3 build order).

Full design, the four-case font-on-edit matrix, the fast-follow ladder
(FF-A offline reflow ladder through FF-H spacing/synthetic-styles — FF-A/
FF-B boundary amended by decision 015, FF-H re-scoped by decision 019,
see below), and the six standing rules (R69–R74) are in
`docs/decisions/014-acrobat-text-editing.md`; Pass slicing (14.0
read-only model → 14.1 edit+relayout+font-gate → 14.2 formatting → 14.3
canvas UI) and its Shipped records are in `ROADMAP.md`.

### 5.12 Annotation deletion is surgery-under-incremental-save, NOT a fifth forced-full-rewrite sibling (decision 022 — DECIDED, Pass 22.0 unbuilt)

*(Added 2026-08-04, `pdfcer-librarian` continuation 80, as a
forward-looking design note ahead of Pass 22.0's build — same
disposition §5.11 had ahead of Pass 14.1, per §5.11's own header note
above. This section records the SETTLED half of decision 022 §5.4's
finding — family membership — and separates it from the UNSETTLED half
— R58's exact wording — which stays flagged, not fixed, at §5.9,
above, pending Open operator question (v).)*

§§5.2/5.9/5.10 (R35/R58/R67) form a **forced-full-rewrite family**:
every member exists because incremental save structurally *preserves*
superseded content, which is disqualifying wherever the operation's own
contract is confidentiality. **Deleting a pdfcer-authored annotation
(`EditSession::delete_annotation`/`delete_dimension`, decision 022 §6.1,
Pass 22.0) is confirmed NOT a fifth member of that family**, by the same
reasoning §5.11 already applied to in-place text editing: deleting an
annotation is a removal, but its contract is "this is no longer in the
current revision," never "this must be provably unrecoverable." The
prior revision remaining reachable through undo/version history is that
mechanism working as intended, not a defect the way a redaction that
leaves the redacted text recoverable would be. Truly making content
unrecoverable remains Redaction's job (§5.2/R35) and, where it applies,
Sanitize/scrub's (§5.9/R58); conflating annotation deletion with either
would force a routine "remove this ce dimension" action through a full
rewrite that drops revision history for no security reason the
operation ever promised.

**Exactly which objects change on an annotation delete, per decision
022 §5.1 — at most four, and the fourth only for a pdfcer-authored ce
dimension:**

1. The `/Annots` container — indirect-array XOR inline, never both
   (`EditSession::remove_from_annots`, already shipped and reused
   verbatim, no new logic).
2. The annotation dictionary — a `Removal`.
3. The `/AP` `/N` stream object — a `Removal`, resolved BEFORE any
   mutation (the `delete_redaction_mark` pattern).
4. The catalog `/PieceInfo` sidecar (ce dimensions only) — via the
   existing `catalog_dimension_write`, in the SAME command (R113).

**Zero page content streams change** — this is not content surgery at
all, which is a cheap, machine-checkable distinguishing claim
(`tools/content-identity` reporting 0 for `annot-delete`/
`dimension-delete`, decision 022's acceptance criterion A4). This
places annotation deletion architecturally closer to the R107 family
(precisely-named object allocation/removal, proven by
object-id-disjointness, not a runtime guard) than to R35/R58/R67's
content-stream-rewrite family, despite both being "delete" operations
in the colloquial sense.

**What this section does NOT settle:** whether R58's own binding TEXT
should be corrected to name this exception explicitly (`ARCHITECTURE.md`
§5.9, above; `ROADMAP.md` Standing rules R58; Open operator question
(v)) — that is a standing-rule-wording call decision 022 itself declines
to make solo, and this librarian is not making it here either. This
section settles only the underlying architectural question (family
membership), which is not in genuine dispute — three independent
instances (`delete_object`, `delete_redaction_mark`, and now
`delete_annotation`) already agree.

### 5.13 Signing is the CANONICAL incremental-save case, and the private key enters `pdfcer-core` from exactly one source — a PKCS#12 file — behind a hash-in / signature-out `Signer` trait (decision 136 — DECIDED, `Pass 10.7`–`10.9` unbuilt AT WRITING; shipped since — footer below)

*★ **Footer 2026-09-06 (452nd filing).** The heading's *"`Pass 10.7`–`10.9`
unbuilt"* was true on 2026-09-05 and has been false since `7734261` (the
438th filing, the same day): `10.7`–`10.9` shipped there, `10.14` (hardening,
composed appearance, P-384) at `187fa09` (450th) and `10.12` (certifying
signatures — `/DocMDP` + catalog `/Perms`, one per document, first signature
only) at `02bb1ba` (452nd). The SHAPE this section records is the shape that
was built: one `Signer` trait, `Pkcs12Signer` the only in-core implementation,
signing the canonical incremental-save case. Found by the 452nd filing's
rule-11 sweep — a body section is the living truth, not the audit trail, so
the heading is annotated rather than left as a dated record. Still unbuilt:
`10.10` (store/token signers, shell-side by this section's own design),
`10.11` (B-T timestamp), `10.13` (sign into a pre-placed field).*

*★ **Footer 2026-09-06 (453rd filing).** The line above is now stale by
one item: `10.13` (sign into a pre-placed field, `/Lock` → `/FieldMDP`,
`/SV` enforced in full) shipped at `ab40127` the same day, one filing later.
Still unbuilt from this section's arc: `10.10` and `10.11` only. The 452nd's
footer is kept as written — it was true at 04:52 and false by 05:13.*

*(Added 2026-09-05, 436th filing, as a forward-looking design note ahead
of the signing arc's build — the same disposition §5.11 had ahead of
Pass 14.1 and §5.12 ahead of Pass 22.0. Records the SETTLED shape; the
crate stack that implements the key operation is a separate decision,
claimed as `137`, owed before the working tree's `Cargo.toml` change is
committed — rule 13.)*

*★ **The crate stack IS now decided — decision 137 (437th filing, same
day).** `rsa 0.10.0-rc.18` (blinded paths only, RUSTSEC-2023-0071 accepted
for signing with the reasoning in the record), `p256`/`p384 0.14`,
`signature`, `rand_core`, and `sha1`/`hmac`/`pbkdf2`/`des`/`rc2` for the
PKCS#12 import, all behind a default-ON `signing` feature; CMS/DER writing
(`sign/der_out.rs`) and PKCS#12 parsing (`sign/pkcs12.rs`) are in-house on
`asn1.rs`; `cms`, `pkcs12`, `pkcs5`/`pkcs8[encryption]`, `x509-cert`,
`p12-keystore`, `p12`, `ring` are not taken. The paragraph above is left as
written because it was true when written; §12 decision 137 and §9's sixth
dependency paragraph are the current statement.*

**Signing and R36.** Table 252's `Changes` row (*"each signature results
in an incremental save"*) and §7.5.6 make signing the operation for which
incremental save is not the default but the ONLY legal mode: a full
rewrite re-serialises prior objects, their bytes move, and every earlier
signature's `/ByteRange` digest fails. `pdfcer sign` and the `EditSession`
signing verb therefore **refuse** a full-rewrite save by name and
**surface** (not duplicate) R67's refusal on an xref-recovered document.
`/SigFlags 3` is written so any downstream editor reads the same fact
(`AppendOnly`). The write itself is the two-pass hole of
`iso32000__ref__signature_creation.md` `SC-2`: reserve a zero-filled
`/Contents`, fix `/ByteRange` to reach EOF, digest the two spans, build
the CMS, back-patch inside the hole and change **no byte outside it** —
then self-verify with the verifier `Pass 10.1` already ships.

**The key boundary.** Acrobat's four digital-ID source classes
(`Acrobat_Features\signatures__digital_id_sources.md`) divide on one
fact: only a **PKCS#12 file** puts the raw private key in the signing
application's memory; a non-exportable Windows-store key, a PKCS#11
token/HSM and a cloud (CSC) identity each take a hash and return a
signature. So the core primitive is a **`Signer` trait — `sign(digest) →
signature`, `certificate_chain()`, and the algorithm the bytes are** — and
`pdfcer-core` ships exactly one implementation, **`Pkcs12Signer`**
(`Pass 10.7`). Every custodial source is a **shell-side** implementation
(`Pass 10.10`): the key never leaves its custodian, and the engine gains
no OS key store, device driver or network client — the same line §1.1 and
decision 135 draw. One pipeline serves every source; a second pipeline
per source is the drift this shape exists to prevent.

**Level and format.** PAdES **B-B** is the only level `pdfcer-core` can
produce unaided (`pades__ref__creation_by_level.md` `PC-1`); B-T needs a
supplied RFC 3161 token (network — a shell fetches, core embeds,
`Pass 10.11`); B-LT/B-LTA need the revocation material `Pass 10.6`
provides. The default `/SubFilter` is **`ETSI.CAdES.detached`** (CAdES),
with `adbe.pkcs7.detached` an option — a deliberate divergence from
Acrobat's out-of-box legacy-PKCS#7 default, recorded as such. The CMS
`signing-time` attribute is never written; the claimed time is the PDF
`/M`, caller-supplied, and if the CLI derives it from the system clock it
prints that it did (rule 4/11). The level ACTUALLY produced is always
disclosed (`PC-12`).

**What this section does NOT settle:** the constant-time signing crates
(decision 137, owed); the signature appearance composer; `/FieldMDP` lock
dictionaries; cloud/CSC signing (covered by the trait, unscheduled).

## 6. Packaging: single-folder portable

- **Platform scope (decided 2026-07-30, decision 003 §4.1 — no longer
  a default):** v1 ships **Windows 10/11 x86_64 only**, as a
  deliberate scope decision. The codebase stays platform-clean at all
  times (no `#[cfg(target_os)]` in `pdfcer-core`/`pdfcer-render`, rule
  R10), verified continuously by cross-target `cargo check` CI for
  macOS-arm64 and wasm32 — a compile signal, never a support claim
  (rule R9). See `docs/decisions/003-distribution-posture.md` for the
  full reasoning, the macOS/Linux gating triggers, and the
  CLI-first-via-musl rule if Linux ever ships.
  **★ NARROWED 2026-08-17 (decision 067).** The cross-target check's
  guarantee covers Rust source, not every build script a dependency
  runs under it — `cargo check` still executes build scripts, and one
  can compile platform-sensitive C. `pdfcer-fetch` (`ureq` → `rustls` →
  `ring`) is **excluded** from this job because `ring`'s build script
  fails cross-compiling to `aarch64-apple-darwin` from the Linux
  runner; the exclusion is safe only while `pdfcer-fetch` has **zero
  workspace dependents** (verify with `cargo tree -i pdfcer-fetch`
  across all four member crates) — the moment that changes, the
  exclusion silently widens and needs re-scoping. `pdfcer-core` +
  `pdfcer-render`'s own wasm32 check is unaffected.
- No installer. Build produces `pdfcer.exe` (Windows first target) plus
  whatever DLLs/assets are needed, all in one output folder.
  **Release artefact name, as of `v0.28.0` (2026-09-03, `Pass 247.2`,
  401st filing):** `tools/package-portable.py` emits
  `D:\builds\pdfcer-<yyyymmdd>-<hhmm>-<short-hash>\` and the release
  zip is **`pdfcer-v<version>-windows-x64.zip`** with a `.sha256`
  sidecar (measured for `v0.28.0`: 18,269,564 B; folder 32,411,461 B;
  `pdfcer.exe` 19,938,816 B). `v0.27.0` and earlier were
  `pdfce-v<version>-windows-x64.zip` carrying `pdfce-cli.exe`; the two
  release lines are one version sequence (decision 128).
- **Payload/user-state partition (decision 003 R15, binding from the
  first Pass that persists anything) — BUILT `Pass 51.0`, 2026-08-08
  (`2a1b0df`):** the distribution folder is split into replaceable
  payload (binaries, assets, `THIRD_PARTY_LICENSES.md`, README) and
  user state (settings today; recents, later OCR data, ribbon/keymap
  layouts to come) in a clearly named location — because the documented
  update procedure is "replace the folder," and replacing a folder
  destroys whatever the user kept in it. **The location is
  `<exe dir>/userdata/`** — the concrete name for what this paragraph
  and decision 003 §6.3's README copy both used to hold as a literal
  `<user-state>` placeholder (decision 003 §6.3 itself still carries
  the unresolved placeholder text on disk as of this entry — a
  follow-up owed to whoever next touches that record). When
  `userdata/` cannot be created or written (a read-only share,
  `Program Files` without elevation), pdfcer falls back to the platform
  configuration directory and **discloses which one it used** — the
  two locations behave differently on update, so an operator who does
  not know which is live cannot follow the update instructions
  correctly (fuzzy-never-sneaky, rule 4, applied to a location pdfcer
  inferred on the operator's behalf). Settings persist as a flat,
  hand-editable `key = value` text file (`settings.txt`) with **per-key**
  fail-soft recovery (one bad line loses one setting and names its own
  line number; a missing file is silently every default) —
  deliberately not `serde`+`toml`, because `ARCHITECTURE.md` §7's
  fail-soft contract is per-key while derived deserialization fails
  per-document. `pdfcer_core::settings` (`crates/pdfcer-core/src/
  settings/mod.rs`) has zero GUI/windowing dependencies (rule 2 holds);
  both `pdfcer` and `pdfce-gui` load it once at startup. User state
  never sits loose among the binaries; the update instructions name
  exactly which files to keep. ~~The packaging smoke test verifies the
  partition.~~ **CORRECTED 2026-08-10 (`pdfcer-librarian`, from
  `tools/package-portable.py`, `9146b41`) — this sentence had no test
  behind it when written and still does not name a real one.** No
  automated "packaging smoke test" existed anywhere in the repo until
  this correction was written, and none exists now either — what
  exists is `tools/package-portable.py`, which assembles the payload
  into a dated `D:\builds\pdfcer-<stamp>-<hash>\` folder and stops; it
  performs no verification of its own (no launch, no partition check,
  nothing). The one confirmation on record that `userdata/` truly stays
  absent until first run, and that the partition behaves as designed,
  was a MANUAL run of the built binaries from inside the output folder
  (`--version`, `list-fields` against a fixture) — done once, by hand,
  not wired into any script or gate that would repeat it on the next
  packaging pass. The **intended** procedure is still the one three
  paragraphs below (§6's own "Verify every packaging pass with a real
  smoke test": zip, unzip to an unrelated path, launch, confirm it
  renders a fixture) — that procedure is design intent, not yet built
  either, and this sentence's error was stating a downstream consequence
  of an unbuilt test as though the test existed. R175's shape (a
  document's claim about the state of the world, uncorroborated by
  anything that checks it) applied to this document's own body text
  rather than to an external fact. **Still open, and worth a Pass of its
  own:** an automated smoke test — zip/unzip/launch/render, or at minimum
  a scripted post-build check that `userdata/` is absent — that runs
  every packaging pass rather than depending on someone remembering to
  check by hand. ~~**Still open:** no in-app settings editor exists yet —
  `Settings::save` has no caller anywhere in the workspace; the file is
  hand-edit-only until a future Pass adds a write path.~~ **★ BUILT
  2026-08-08 (`Pass 51.4`, `6d63d81`) — a File-tab settings window now
  exists** (`crates/pdfce-gui/src/settings_panel.rs`), grouped by
  subject, disclosing each setting's evidence tier and BYTES-vs-RENDER/
  EXTRACT blast radius, editing a working copy that only reaches disk
  on Save. `Settings::save` has its first caller. See §12's `Pass 51.4`
  entry and `docs/ROADMAP.md`'s own Shipped entry for the full record.
- No registry writes, no `%APPDATA%` requirement for the app to run
  (per-user settings/recents may still use a conventional config dir,
  but the app must run read-only-folder-clean with no config present).
- Verify every packaging pass with a **real smoke test**: zip the
  output folder, unzip it to an unrelated path (e.g. a fresh temp
  dir), launch from there with no prior install step, confirm it
  opens and renders a fixture PDF. This is the packaging equivalent of
  MatExtractor's "smoke-import MainWindow" rule — don't claim a
  packaging pass done without actually running the copied folder.
- **Release channels (decision 127, 2026-09-03; `R229`, 2026-08-29).** A
  release is finished when the packaged folder has reached **both**
  channels, in this order after the tag and the smoke test: the CLI to
  OneDrive, alternating `pdfcer1`/`pdfcer2` so a previous version survives
  (`tools/deploy-onedrive.py`, `R229`); then a **GitHub release for the
  tag with the portable zip as its asset, marked latest** (`gh release
  create <tag> <zip> --latest`, decision 127); then
  `tools/verify-release.py <tag>` green. The two channels serve different
  readers — OneDrive is the operator's CLI, GitHub is everyone else's full
  folder and the offsite copy of the build — and neither is a `git
  bundle`. Nine versions (`v0.18.0`–`v0.26.0`) reached only OneDrive
  because the recipe did not name the second channel; decision 127 exists
  so that cannot recur by omission, and since `c0c8dee` (2026-09-03)
  `verify-release.py`'s GitHub-release check **fails** on a missing
  release — `skip` only when `gh` is not installed on the machine
  (`shutil.which`) — so it cannot recur by habit either.

## 7. CLI capabilities (`pdfcer`)

pdfcer ships a real command-line interface alongside the GUI, not as a
debug afterthought. Design points:

- **Same crate-separation discipline as the GUI.** `pdfcer` depends
  on `pdfcer-core` + `pdfcer-render` exactly like `pdfce-gui` does, and
  is held to the same zero-GUI-dependency-in-core invariant (§3) — the
  CLI's existence is itself proof that invariant is doing its job:
  two completely different front ends, one shared core, no logic
  duplicated.
- **Subcommand shape** (`clap`-based, final surface scoped alongside
  each feature's own Pass — see `docs/ROADMAP.md`): one subcommand per
  batch operation, e.g. `pdfcer merge a.pdf b.pdf -o out.pdf`,
  `pdfcer extract-pages in.pdf 3-7 -o out.pdf`, `pdfcer
  bates-stamp *.pdf --start 1 --format "DOC-{:06}"`, `pdfcer
  to-pdfa in.pdf --level 2b -o out.pdf`, `pdfcer validate-pdfa
  in.pdf` (prints a conformance report, non-zero exit on failure —
  scriptable in CI/document-pipeline contexts), `pdfcer sign in.pdf
  --cert cert.p12 -o out.pdf`, `pdfcer render-page in.pdf 3 -o
  page3.png --dpi 150`. **Not every subcommand's output is a PDF**:
  `pdfcer export-dxf in.pdf --page 1 -o out.dxf [--units in|mm]
  [--scale S] [--no-fit-arcs] [--no-text]` (`Pass 52.1`, 2026-08-09)
  writes ASCII DXF for CAD import, read-only on the input, with a
  three-way stderr disclosure (`skipped_text`/`unreadable_text`/
  `skipped_images`) before its stdout summary — see §3's `export/dxf.rs`
  note and §12 decision 035.
- **The Reader-parity sweep (`Pass 55.x`, 2026-08-10, §12 decision 036)
  added four subcommands that read rather than mutate the PDF, plus one
  that reports on a Windows subsystem the PDF never touches:**
  `pdfcer find-text <input> <needle> [--ignore-case]` (the search-to-
  quad scan Pass 8's redaction verb already contained, extracted to run
  without marking anything for removal — no encryption/certification
  gate, since reading is not writing); `pdfcer list-outline <input>
  [--flat]` (§12.3.3 bookmark tree, read-only); `pdfcer
  list-attachments <input>` (§7.11.7 embedded files, both standard
  paths, with a stderr `MAY_BE_ENCRYPTED` warning per §7.6.5's
  otherwise-invisible per-file `/EFF` encryption — see §3's
  `attachments.rs` note); `pdfcer list-printers` and `pdfcer
  print-preview <input> [--printer NAME] [--scale-percent N]` (Windows
  spooler enumeration + page-fit report; **does not print anything** —
  see §3's `printing.rs` note for why spooling is deliberately
  unbuilt). **Completed 2026-08-10 (seventy-fifth filing) with a sixth:**
  `pdfcer list-layers <input>` (§8.11 optional-content groups,
  READ-only — see §3's `layers.rs` note; no toggle subcommand exists,
  by design, R83).
- **`pdfcer list-signatures <input>` (`Pass 10.0`, 2026-08-10,
  `2676d4d`+`2ae9991`) — read-only `/ByteRange` coverage, explicitly NOT
  cryptographic verification.** (★ Qualified 2026-09-03: true of THIS
  subcommand still; its sibling `verify-signatures`, next bullet, IS the
  cryptographic check.) Distinct from the Reader-parity sweep
  above (decision 036 named signature validation as a related gap but
  deliberately left it in the existing "Digital signatures" Backlog
  bucket, unstarted) — this is that bucket's first slice, chosen because
  it needs no crypto dependency at all. Every summary line states the
  coverage-only caveat unconditionally, not gated on a flag — see §3's
  `signature.rs` note and §12's decision-037/038 claims for the two
  spec questions this slice surfaced but did not settle. **037 has since
  been settled** — answered by measurement against the installed Acrobat
  on 2026-08-11 (`04f8acd`); **038 remains genuinely open** and needs a
  spec read rather than a fixture, since its two readings diverge only
  for a group named in both `/ON` and `/OFF`.
- **`pdfcer verify-signatures <input>` (`Pass 10.1`, 2026-09-03,
  `22421b6`; §12 decision 129) — the cryptographic sibling of
  `list-signatures`.** One block per `/FT /Sig` field with a `/V`, in
  `list-signatures`' order: `integrity` — *verified* with the digest and
  signature algorithms named / *digest mismatch* (the covered bytes were
  altered) / *signature invalid* (the digest matches; the CMS signature or
  certificate does not) / *unverifiable: <reason>* (a subfilter, algorithm
  or curve pdfcer lacks — `adbe.x509.rsa_sha1`, `ETSI.RFC3161`, P-521,
  Brainpool — never reported as either failure); `coverage` — the
  `list-signatures` arithmetic, folded in; **`trust: not checked`** in
  those words on every block; then the certificate's subject / issuer /
  validity dates, `signingTime` and the dictionary's `/Name` `/M`
  `/Reason` `/Location` as CLAIMS; then the rule-4 `notes` (a SHA-1
  digest, non-zero `/Contents` padding, extra `/ByteRange` gaps with
  their extents, an ETSI signature short of EOF). Exit **0** every
  signature verified; **12 `SIGNATURE_FAILED`** any digest mismatch or
  invalid signature; **13 `SIGNATURE_UNVERIFIABLE`** none failed but at
  least one unverifiable — *"pdfcer cannot say"* is a different exit from
  *"tampered"*, so a script can branch on the distinction. **The word
  *valid* is never printed**, and neither is *"signed by X"* — the
  disclosure contract is stated in `docs/core-api/01-reading-and-model.md`
  §12.5. `list-signatures` is unchanged and still computes no digest; its
  help no longer claims pdfcer performs no verification, **but its summary
  line still does** (`crates/pdfcer-cli/src/main.rs:14821`, and the
  subcommand's doc comment at `:14587`) — owed to the engineer, reported
  at the 398th filing.
- **`pdfcer export-data --format csv` / `import-data` — a third
  form-data interchange format alongside FDF/XFDF (`Pass 62.0`,
  2026-08-11) — carries a security obligation the other two formats
  don't.** A spreadsheet reads a cell beginning `=`/`+`/`-`/`@` as a
  live formula, not text, so an unneutralised form value can trigger a
  process launch or a network fetch (`=WEBSERVICE(...)`) the moment
  the exported file is opened — the same capability §1.1/**R12**
  already refuses inside pdfcer's own tree, reached here by handing the
  spreadsheet the trigger instead. Export neutralises with a leading
  apostrophe, discloses every neutralised field BY NAME, and import
  reverses it exactly, so a round trip through a spreadsheet does not
  accumulate apostrophes. See §3's `formcsv.rs` note and §12's
  2026-08-11 entries for the full design and the tall-vs-wide shape
  decision (the wide, one-row-per-document shape for batch-filling
  many copies of one form is a distinct, unbuilt feature — see
  `docs/ROADMAP.md` *Backlog*).
- **Exit codes matter.** Since this is meant to be genuinely scriptable
  (unlike Acrobat, which has no real CLI), follow normal Unix
  conventions: `0` success, non-zero on any failure, with a specific,
  documented meaning per non-zero code where it's useful for a calling
  script to distinguish failure modes (e.g. "input not found" vs
  "encrypted, no password given" vs "PDF/A validation failed").
- **`--open-password <PW>` / `--open-password-file <PATH|->` — global
  flags** (`Pass 5` increment 1's CLI follow-up, 2026-08-11,
  `0a79da4`), so every subcommand that opens a document honours them
  without threading `Option<&[u8]>` through each one. Stored in a
  process-lifetime `OnceLock<Option<Vec<u8>>>`, written once in `run`
  before dispatch. Named `--open-password`, deliberately not
  `--password` — `add-text-field --password` already exists as the
  Table 228 field-flag, and `clap` does not refuse the collision at
  build time; it panics at run time on the first `add-*` subcommand
  parsed. `--open-password-file` is preferred (avoids process-list and
  shell-history exposure), reads `-` from stdin, and strips exactly one
  trailing newline. An unreadable password file fails immediately,
  naming the file, rather than proceeding password-less and surfacing
  later as an opaque "password-protected" refusal.
- **Same round-trip / redaction / fuzzy-never-sneaky invariants apply.**
  A CLI redact command must truly remove content, same as the GUI
  path (§5); a CLI OCR command's output is still a hint the caller
  chooses to apply, not silently baked into the saved file, unless an
  explicit `--apply` (or similarly unambiguous) flag says otherwise.
- **Packaging: `pdfcer.exe` is the ONLY binary in the portable
  folder, as of 2026-09-03 (`Pass 247.0`, `da3b2f8`, 399th filing;
  decision 128).** Before this Pass the folder shipped two entry-point
  binaries (`pdfcer.exe` beside `pdfce-gui`'s executable); the GUI
  binary is now built and packaged entirely by the separate
  `D:\dev\pdfcer-gui` project. `tools/package-portable.py`'s
  `BINARIES` list is `["pdfcer.exe"]`. The packaging smoke test
  (§6) covers the one binary this repo now ships.
- ~~**`pdfce-gui`'s own command-line surface is exactly three answers,
  and that boundary is deliberate** (2026-08-12, `9ea0c88`; §12
  decision 054). `pdfce-gui` accepts one positional argument — a
  document to open, which is what a double-click and a file
  association supply — and additionally answers `--help`, `-h`,
  `--version` and `-V` on the terminal **before `eframe` initialises
  anything**, exiting 0. An unrecognised leading-dash argument exits 2
  rather than being opened as a filename. Before this, `pdfce-gui
  --version` opened a window and never returned: a script or installer
  probing the binary hung indefinitely with no output. The help text
  points at `pdfcer` for batch work **so this stays a courtesy to a
  terminal and does not grow into a second, competing CLI** — that is
  the boundary this bullet exists to state. Parsed by hand, not by
  `clap`: four string comparisons do not justify an argument-parser
  dependency in the GUI crate (rule 13).~~ **Moot as of `Pass 247.0` —
  the crate this decision governed no longer exists in this repo.**
  Kept struck-through, not deleted, because decision 054 is real
  history; whether `pdfcer-gui` needs an equivalent boundary is that
  project's own call, not inherited automatically.

## 8. Code style & public API design

`pdfcer-core`'s public API (and, downstream of it, `pdfcer`'s
argument/output design) follows the official Rust ecosystem
conventions, not an invented house style:

- **Formatting** — the Rust Style Guide, enforced via `cargo fmt`.
- **API design** — the Rust API Guidelines checklist (naming
  conventions, trait derives, error-type design, documentation,
  predictability, type safety).

Full condensed reference, kept up to date as a cross-project resource
(useful to any future Rust project, not just pdfcer):
`D:\dev\rag\rust\rust-style-guide-and-api-guidelines.md`. This is a
binding engineering discipline, not a style preference — see
`.claude\agents\pdfcer-engineer.md` §"Code style & API design
discipline" for the enforcement mechanics (`cargo fmt --check` and
`cargo clippy -- -D warnings` clean before any Pass ships).

### 8.1 A closed, stability-promised discriminant is not a shortcut for a new question

*(Added 2026-09-07, 468th filing, `Pass 260.0` / `75793b5`. Created by
**decision 140**, §12.)*

**The rule.** When an existing public discriminant enum carries a written
stability contract that its variant SET will not grow (buckets closed,
what falls into them open — the inverse of the usual `#[non_exhaustive]`
default, adopted deliberately per `text_edit::RefusalKind`, `Pass 249.0`),
that enum is not the vehicle for a new, orthogonal classification question,
even when it is the closest-shaped precedent in the crate. **A new sibling
type is minted instead**, with a bridging method
(`ErrorType::decline() -> SiblingEnum`) rather than a widened match arm.

**Why the enum looks reusable when it is not.** The stability posture is a
*written promise*, not a structural fact a reader can infer from the type
alone — `RefusalKind` is an ordinary-looking public enum with no
`#[non_exhaustive]` and four variants; nothing about its shape signals
"do not add a fifth." The promise lives in a doc comment and a design note
in its shipping Pass. **A closed-set discriminant's non-growth contract
must be stated at the type's own definition**, or every future author who
reaches for it as precedent has to re-derive, from a different question,
that widening is off the table.

**The distinguishing test, applied going forward.** Same question, new
instance of an existing bucket → fine, that is what the promise permits.
Different question that merely wants the same closed-set SHAPE → mint a
sibling enum. `ReflowDecline` (`RetryAfterSaveAndReopen` /
`StructureForbids` / `NotFound` / `NotReflowable`) answers *"is this
refusal recoverable, and how"* — a question `RefusalKind`'s four buckets
(what kind of thing went wrong) do not ask and every reflow refusal would
have landed in exactly one of regardless. `ReflowApplyError::decline()`
bridges the two; `is_recoverable()` is derived FROM `decline()`, never
stored independently, so the two questions cannot answer inconsistently
about the same error.

**Forbidden refactor:** adding a variant to `RefusalKind` to express
recoverability. Permitted: a new `ReflowApplyError` variant that maps into
an *existing* `ReflowDecline` bucket via `decline()`'s match arms — that is
exactly the "buckets closed, contents open" shape both enums now share.

## 9. Open-source dependencies & attribution

pdfcer builds on the existing Rust/OSS ecosystem rather than
reinventing every primitive — see `docs/PRIOR_ART.md` for the
survey/decision record and `docs/LEGAL.md` §6 for the binding
licensing discipline (permissive-vs-copyleft classification, the
mandatory per-dependency license check, and why pdfcer's own license
gates which prior art is even usable). Attribution for whatever's
actually adopted is **generated**, not hand-maintained — `cargo-about`
produces `THIRD_PARTY_LICENSES.md` from the real `Cargo.lock`,
regenerated at every packaging pass (§6).

**`docs/DEPENDENCIES.md` (new 2026-08-11, `2b4b2bf`)** is the
purpose-shaped companion to the generated, licence-shaped
`THIRD_PARTY_LICENSES.md` — every direct dependency, by crate, with
what it's *for*, plus what pdfcer implements itself instead (MD5/RC4,
the PDF-specific predictors, the ASCII filters) and why. Written by
hand because a generated file cannot answer "why is this here,"
only "what license does it carry." Re-run its own §5 commands and
update it whenever the dependency set changes, same discipline as
`THIRD_PARTY_LICENSES.md`.

**pdfcer's own license is MIT (decided 2026-08-01, `LEGAL.md` §1; see
§12 decision log).** `LICENSE` (repo root) + `license = "MIT"` in
`Cargo.toml` `[workspace.package]`, inherited by all four member
crates via `license.workspace = true`. Every current dependency is
permissive (verified against `THIRD_PARTY_LICENSES.md`), so this
decision required no dependency rework. **Consequence: GPL/AGPL prior
art (MuPDF, Poppler, Ghostscript) is now categorically, permanently
excluded as a real dependency** — reference-only (architecture/
algorithms studied, never linked or copied), per `LEGAL.md` §6.1.

**First conjunctive-attribution dependency: `jpeg-encoder` 0.7.1
(2026-08-08, `Pass 48.2`, see §12's thirty-fifth-filing entry).**
`(MIT OR Apache-2.0) AND IJG` — permissive at the grant level, with an
IJG attribution NOTICE condition that applies unconditionally alongside
it (an `AND`, not a caller's choice between license terms). Accepted on
direct operator ruling; the attribution sentence is generated into
`about.hbs`, never hand-written. pdfcer's first shipped image ENCODER,
under standing rule R28's own named exception (`ROADMAP.md`).

**First dependency for which R24's own lever does not exist: `aes`
0.9.2 + `cbc` 0.2.1 (2026-08-11, `Pass 5` increment 2, `f7aee60`; see
§12's decision 039).** Both `MIT OR Apache-2.0`; nine transitive crates
(`cipher`, `crypto-common`, `inout`, `block-padding`, `typenum`,
`hybrid-array`, `cpufeatures`, `cpubits`, `cfg-if`), all permissive,
zero copyleft — rule 13's escalation trigger did not fire.
`THIRD_PARTY_LICENSES.md` regenerated via `cargo-about` (Apache-2.0
count 133 → 142, exactly the nine new crates). Every other codec/crypto
dependency in this project is compiler-enforced free of `unsafe` via
`default-features = false` (R24, and the in-crate MD5/RC4 posture at
decision 039's own sibling entry). `aes` cannot be forced into that
shape: its intrinsic hardware backends are selected on a **cfg**
(`aes_backend = "soft"`), and a cfg is settable only from `RUSTFLAGS` or
`.cargo/config.toml` — global to the build, and **not inherited by any
downstream consumer of `pdfcer-core` as a library**. Forcing the soft
backend from pdfcer's own build config would buy a guarantee true for
pdfcer's own binaries and false for every other crate that depends on
`pdfcer-core`. The hardware backend is therefore accepted deliberately,
bounded by a CI job (`decision 039 — assert \`aes\` carries no extra
features`, `.github/workflows/ci.yml`) pinning `hazmat` and `zeroize`
OFF across four targets — the two widenings still under pdfcer's control.
On `wasm32-unknown-unknown` specifically, `aes` pulls no `cpufeatures`
and resolves to the soft backend automatically, so the WASM web-fork
target keeps zero-`unsafe` without any special-casing. Full reasoning:
§12's decision 039.

**Second dependency under decision 039's exception, same shape, no new
decision: `sha2` 0.11.0 (2026-08-11, `Pass 5` increment 3, commits
`f79f044..f79d9a2`; see §12's decision 039 amendment).** SHA-256 is the
entire `/R` 5 (AES-256) key-derivation primitive (Adobe Supplement
ExtensionLevel 3 §3.5, Algorithms 3.2a/3.8–3.13). Three new packages
(`sha2`, `digest`, `block-buffer`), all `MIT OR Apache-2.0`, zero
copyleft — `THIRD_PARTY_LICENSES.md` Apache-2.0 count 142 → 145. Its
intrinsic backend is **cfg**-selected (`sha2_backend` etc.), identical
in shape to `aes`'s hardware-backend dispatch, so it cannot be forced
off via `default-features = false` for the same reason `aes` cannot;
`default-features = false` still removes `sha2`'s own `alloc`/`oid`
defaults, and a CI job (extending decision 039's existing one) asserts
`alloc`/`oid`/`zeroize` stay off across all four targets. Full
reasoning: §12's decision 039 amendment.
**★ Amended 2026-09-05 (440th filing, decision 138): `oid` is now ON**
— `rsa`'s optional `sha2` dependency declares `features = ["oid"]`, and
RSASSA-PKCS1-v1_5 needs `Sha256: AssociatedOid` for the RFC 8017 §9.2
`DigestInfo` prefix. `oid` is `const-oid` trait impls only (no backend
change, no new `unsafe`, no new package). The CI job now asserts
`alloc`/`zeroize` stay off (`1c6f670`); `aes`'s fence is unchanged.

**Fourth dependency, first for the `/BrotliDecode` filter: `brotli` 8.0.4
(2026-08-25, `Pass 123.0`, `4163ad9`; see §12 decision 086).**
`BSD-3-Clause AND MIT`, both permissive — no operator licence call needed
under rule 13, verified live against the crate's own published metadata
at adoption time, not relayed from an earlier reading. Pure Rust
deliberately, unlike `aes`/`sha2`'s cfg-selected hardware backends above:
a binding to the C reference implementation would break both single-
binary packaging and the wasm32 web-fork target, so `cargo check --target
wasm32-unknown-unknown` clean was verified as part of adoption, not
assumed. **Decode only** — the crate's compressor half is not compiled
into the shipped binary; see decision 086 for why the write side is a
separate, deferred deliverable rather than a smaller slice of the same
one. `THIRD_PARTY_LICENSES.md` regenerated via `cargo-about`.

**Fifth dependency, and the first that is a SIBLING PROJECT rather than a
third-party crate: `iccce-profile` + `iccce-cmm`** — declared in
`crates/pdfcer-render/Cargo.toml`, **`pdfcer-core` does not depend on it**, so
the object model stays free of a colour engine. **MIT**, verified against
`iccce`'s own `Cargo.toml` at adoption time rather than relayed from an
earlier reading; permissive, so rule 13's escalation trigger did not fire and
no operator licence call was needed. `THIRD_PARTY_LICENSES.md` regenerated via
`cargo-about`.

★ **NOTED 2026-09-02 (`Pass 242.0`, `48f8fbb`; pdfcer-librarian 385th
filing, no new decision minted) — a THIRD crate from the same sibling
project, `iccce-color`, is now DECLARED DIRECTLY in
`crates/pdfcer-render/Cargo.toml` too.** It carries the PCS value types
(`Xyz`) that `iccce-cmm`'s PCS-side entry points take; it was already in
the dependency tree transitively, as `iccce-cmm`'s own dependency, but
`iccce-cmm` does not re-export the type, so code that needs an
`iccce_color::Xyz` value directly needs the crate declared directly too.
Same repository, same pinned `rev`, same MIT licence as the other two —
dependency **set** unchanged (it was already resolved and vetted
transitively), so this is a manifest-declaration change, not a new
licence question; `THIRD_PARTY_LICENSES.md` unchanged, verified by
regenerating. Not treated as decision-log-worthy on its own: nothing
about which crate is used, what it is used for, the `pdfcer-core`/
`pdfcer-render` boundary, or the pin form (decisions 064/115/123) changes
— it is the same dependency, now named where it is used instead of only
where it is transitively pulled in.

★★ **RE-PINNED 2026-09-02 (`Pass` untracked, `e868d36`; see §12 decision
123, which amends decision 115's pin form without disturbing its reasoning)
from `tag = "v0.3.0"` to `rev = "a4d9003bf87c61299fa1c6f9c2e2ffffa30de0c3"`
— the SAME commit the tag pointed at, at `iccce`'s own request** (its reply
in `D:\Dev\FeatureRequests\iccce_FeatureRequests\open\reply_depend_on_a_pinned_rev_and_the_four_intent_rules_are_accepted.md`,
2026-09-01): a `rev` is reproducible without asking the sibling project to
cut a release it cannot promise on pdfcer's schedule, and — the reason this
is a decision and not a formatting change — **a tag can be MOVED** by the
tag-owning repository, while a `rev` cannot. Dependency **set** unchanged;
both lockfiles moved; `cargo about` regenerated `THIRD_PARTY_LICENSES.md`
with no change.

★ **The form is a GIT dependency PINNED, and pinning (now by commit, not by
tag) is what stays deliberate.** A **path** dependency (`../../../iccce`)
resolves only on the author's machine and would be a broken build for anyone
cloning the public repository; **pinning** keeps colour output reproducible,
because an unpinned git dependency would let a sibling-project commit change
what pdfcer renders between two builds of the same pdfcer commit — silently,
in the one area where a silent change stays invisible until a conformance
figure moves. A **vendored copy** was considered and rejected: a fork's
maintenance burden with none of a fork's purpose. Reversible if the operator
prefers crates.io publication; decision 115 names it as such.

★ **A downstream literal is now stale and is flagged, not fixed, here** —
this section documents the dependency, not the build-provenance banner.
`iccce_provenance()` (`crates/pdfcer-core/build.rs`) derives its printed pin
description (`"tag v0.3.0"` vs `"rev …"`) from the resolved `Cargo.lock`
source string, so it self-corrects to the new form without a code change —
but its own doc comment's worked example (`build.rs:272`,
`crates/pdfcer-core/src/build.rs:32,109`) and `docs/FEATURES.md`'s *Build
provenance stamp* row still quote the pre-re-pin literal
(`"iccce: 0.3.0 (tag v0.3.0, a4d9003b, committed …)"`) as current output.
Owed to the engineer: verify the live `--version` banner text and correct
those three sites — not corrected in this filing because this role has no
shell this session and will not assert an unmeasured output string
(hard rule 8).

★★ **`iccce` has ZERO external dependencies and sets `unsafe_code = "deny"`
in its own `Cargo.toml`**, which is why it is admissible without any of the
machinery `aes`/`sha2` needed above. That one fact clears the **no-network**
gate, the **wasm32** web-fork target, and **R24's zero-`unsafe` posture** at
once — **compiler-enforced inside the dependency**, rather than negotiated
via `default-features = false` and bounded by a CI job the way decision 039's
cfg-selected hardware backends had to be. **No decision-039-style exception
is needed or claimed.**

**Owed at the time of this writing (2026-09-01):** `docs/DEPENDENCIES.md`
was **not** updated when this dependency landed, and this section requires it
— see `ROADMAP.md`'s *Backlog*. It is the one dependency in the tree whose
*"why is this here"* is an architectural decision (064, 115) rather than a
utility choice, so its absence from the purpose-shaped companion is the
worst-placed gap that file can have.

**★ `Pass 10.1` (2026-09-03, `22421b6`; §12 decision 129) — signature
VERIFICATION added NO dependency, and that is a decision, not an
omission.** `Cargo.lock` is absent from the commit's 38-file stat. The
RustCrypto route `docs/PRIOR_ART.md` had pre-selected (`num-bigint`,
`p256` / `p384` / `ecdsa` / `elliptic-curve`, `cms`, `x509-cert`) was
resolved in a scratch crate and measured: **25 crates**, with
`cms 0.3.0-pre.2` and `rsa 0.10.0-rc.18` still pre-release, and `cmov` /
`hybrid-array` carrying cfg-selected `unsafe` in exactly the shape
decision 039 accepted for `aes` / `sha2`. Decision 039's exception
exists because constant-time code protects a SECRET; verification
handles none — the public key, the signature and the digest are all in
the file — so the argument that admitted `aes` does not apply here and
was not stretched to. Instead the in-crate MD5/RC4 judgement (the
ninety-sixth filing's entry, §12) is extended under a NEW discriminant —
*no secret is handled* — to six modules: `crypto/bignum.rs` (548 lines;
u32 limbs, Knuth Algorithm D division, square-and-multiply modpow,
Fermat inversion), `crypto/sha1.rs` (141; FIPS 180-4 vectors),
`crypto/rsa.rs` (285; PKCS#1 v1.5 by strict whole-EM compare, RSASSA-PSS
per RFC 4055), `crypto/ecdsa.rs` (435; P-256 / P-384, RFC 6979 A.2.5 /
A.2.6 vectors), `asn1.rs` (292; DER, definite lengths only, no BER) and
`cms.rs` (418) — **2,119 lines over six modules**, each header stating
that the judgement does **not** extend to signing. `THIRD_PARTY_LICENSES.md`
unchanged. **Owed:** `docs/DEPENDENCIES.md`'s *"what pdfcer implements
itself instead"* list (MD5/RC4, predictors, ASCII filters) is short these
six. `docs/PRIOR_ART.md`'s three *candidate — verification only* rows are
closed *checked, NOT taken* (398th filing); its `cms` / `rsa` /
`x509-cert` rows stay open for the SIGNING half, which handles a private
key and therefore falls under decision 039's condition, not this one.

**★ Sixth dependency set, and decision 039's shape applied a THIRD time
(after `aes` and `sha2`): the SIGNING crates — `rsa 0.10.0-rc.18`,
`p256`/`p384 0.14`, `signature 3.0`, `rand_core 0.10`, `sha1 0.11`,
`hmac 0.13`, `pbkdf2 0.13`, `des 0.9`, `rc2 0.9` (2026-09-05; §12
decision 137; `Pass 10.7`–`10.9`, decision 136).** The paragraph above
said the signing half *"falls under decision 039's condition"*, and this
is that condition met: a private key is handled, constant time is
required, and the in-crate verify-only arithmetic is barred from it
(decision 129). All ten are `optional = true` behind a new **default-ON
`signing` feature** in `pdfcer-core`'s existing strippable-capability
convention — verification is deliberately NOT gated; a
`--no-default-features` build still reads and checks signatures and
cannot make one. Sourced to `docs/signing-crate-survey.md` (2026-09-05):
**48 crates resolved, 32 new to the lock, 18 `unsafe` carriers of which
11 are new** — `cmov` with three `asm!` files, `crypto-bigint`,
`cpufeatures 0.3.1`, `sha1`, `base16ct`, `base64ct`, `der`,
`elliptic-curve`, `const-oid`, `pem-rfc7468`, and the `aes 0.9.2 → 0.9.3`
bump — the cfg-selected constant-time-backend shape accepted here for the
same reason it was accepted for `aes`: the property protects a secret.
Every one `MIT OR Apache-2.0` / `Apache-2.0 OR MIT`; rule 13's copyleft
clause did not fire. `THIRD_PARTY_LICENSES.md` regenerated via
`cargo-about` (+1,108 / −42 lines). **wasm32 posture:** `rsa`'s
`getrandom` feature is OFF (it drags `getrandom 0.4`, which fails on
`wasm32-unknown-unknown`), so RSA signing refuses there
(`SignError::RandomUnavailable`) exactly as encryption authoring does,
while ECDSA (RFC 6979, no RNG) signs on wasm32; `cargo check -p
pdfcer-core -p pdfcer-render --target wasm32-unknown-unknown` clean at
adoption. **`der 0.8` enters transitively and is NOT used directly** — the
CMS/DER writer is in-crate (`sign/der_out.rs`) so no foreign type crosses
a `pub` signature. **One open advisory travels with this set —
RUSTSEC-2023-0071 ("Marvin") against every `rsa` version — accepted for
the signing path on reasoning recorded in decision 137 (the residual
channel is a decryption de-padding oracle signing never runs; the modexp
channel closed on `crypto-bigint 0.7`; pdfcer signs only through the
blinded `Randomized*` paths; the `signing` feature OFF removes it from the
tree). No `cargo audit`/`cargo deny` gate exists in CI; if one is added it
carries `ignore = ["RUSTSEC-2023-0071"]` with that rationale, re-justified
at each `rsa` bump.** `docs/DEPENDENCIES.md` updated in the same filing.
Not taken, with the survey's reasons in decision 137: `cms` (its `builder`
does not compile against today's dependencies), `pkcs12` (no decryption,
no MAC verify, pins `cms =0.3.0-pre.1`), `pkcs5`/`pkcs8[encryption]` (no
PBES1), `x509-cert` (certificates pass through raw), `p12-keystore`/`p12`
(wasm32 fail; stale generation), `ring` (unchanged).

## 10. Adversarial input hardening & fuzzing

`pdfcer-core` parses files from the public internet by design — every
PDF it opens must be treated as **untrusted, potentially adversarial
input**, not just "possibly malformed." This is a real gap identified
2026-07-23: the project justified choosing Rust partly on this basis
(§2) but had never written down what that actually requires structurally.

### 10.1 Resource-limit guards (decompression-bomb defense)

Every filter decoder (`FlateDecode`, `LZWDecode`, `CCITTFaxDecode`,
`JBIG2Decode`, `DCTDecode`, `JPXDecode`, `BrotliDecode` — added
2026-08-25, `Pass 123.0`, §12 decision 086) **must** enforce a maximum
decoded-output-size cap before/while decoding, not just check the
result afterward — a few KB of compressed input can expand to
gigabytes (classic zip-bomb pattern), and PDF's filter chaining
(e.g. `ASCII85Decode` → `FlateDecode` → raw image data) can compound
this. Concretely:

- Every decoder takes an explicit output-size ceiling (a sane default,
  overridable) and returns an error rather than continuing once
  exceeded — never silently truncate, never allocate unbounded.
- Object/dictionary nesting (page tree, `Kids` arrays, `Resources`
  inheritance chains, annotation appearance-stream references) needs
  cycle detection and a depth cap — a maliciously crafted circular
  reference must fail cleanly, not hang or stack-overflow.
- Content-stream interpretation (path construction, clipping, nested
  `Form XObject`/`q`/`Q` graphics-state pairs) needs an operation-count
  or time budget per page — pathological but syntactically valid
  content streams (e.g. millions of degenerate path segments) must not
  be able to hang the renderer indefinitely.
- Object counts / xref table size get a sanity ceiling too (a
  100 MB file claiming 500 million objects is lying).
- **Concrete instance (added 2026-07-30, Pass 1.1 slice): recursive
  Form-XObject execution (`Do`) gets `MAX_XOBJECT_DEPTH` = 64,
  corpus-measured, not guessed.** An initial guard of 16 (intuition)
  overflowed on exactly one of 2,914 veraPDF/PDF-Association corpus
  files — a **conformant** 32-deep chain
  (`veraPDF-corpus/PDF_A-1b/6.1 File structure/6.1.12 Implementation
  limits/veraPDF test suite 6-1-12-t08-pass-c.pdf`, objects 19–50).
  Annex C sets no form-nesting limit and PDF/A §6.1.12 forbids a
  reader from imposing Annex C limits anyway. Raised to 64 (2× the
  deepest conformant structure measured); corpus-wide overflows are
  now 0. This is the SECOND guard in this project caught by the
  veraPDF §6.1.12 implementation-limits suite (the first was
  `MAX_TOKEN_LEN`) — see the `ROADMAP.md` standing rule requiring
  every new resource guard to be run against that suite specifically
  before shipping.
- **★ Concrete instance (added 2026-08-07, `0df6158`) — the first guard
  in this codebase motivated by a SPEC OBLIGATION rather than by a bomb,
  and the first on the WRITE side: `save::MAX_REWRITE_OBJECT_NUMBER` =
  8,388,607.** §7.5.4 obliges a single-section full rewrite to emit one
  cross-reference entry per object number **from 0 to the file's
  maximum**, so `save_full`'s hole-filling loop is **O(largest object
  NUMBER), not O(object count)** — and the largest number is chosen by
  whoever wrote the input. pdfium's **1.2 KB** `bug_455199.pdf` names
  `2147483648 0 obj` (2³¹) and therefore asks for **2,147,483,649**
  entries: measured at **~27 MB/s of steady allocation, CPU pinned** —
  about an hour and 40 GB. **Not an infinite loop, which is what made it
  survive:** it looks like progress the whole way down, so a liveness or
  progress check cannot detect this class and **only a wall-clock budget
  can**. In the GUI it is an unrecoverable freeze with no error, no
  cancel and no save. **Refused by name (R27)** rather than complied with
  — a sparse table would be malformed (§7.5.4) and compact renumbering
  would break §5's per-object byte-identity contract. The value is
  **sourced from Annex C Table C.1's maximum indirect objects (2²³ − 1)**,
  not guessed, and **deliberately not clamped to the object COUNT**
  (a sparse-but-small file with one enormous number is exactly the
  adversarial shape). The same table caps a PDF **integer** at
  2,147,483,647, so that file's object number is **one more than the spec
  permits** — unrepresentable, not merely improbable, which is why the
  guard refuses nothing a conforming producer can write. **Reading is
  unaffected**; `inspect` and `extract-text` both succeed on the file.
  ~~**The §6.1.12 implementation-limits run this bullet's own standing rule
  requires is OWED for this guard**~~ — **★ DISCHARGED 2026-08-07: the run
  was performed.** All **44** files of the four `*6.1.12*` directories
  (`Isartor test files/PDFA-1b`, `PDF_A-1b`, `PDF_A-2b`, `PDF_A-4`) swept at
  `--mode full`: **0 hangs, 0 regressions, 0 REFUSED.** **Shown non-vacuous
  in the same breath, which is the half that matters:** *"0 refused"* reads
  identically to *"the guard cannot fire"*, so the guard was separately
  verified **firing** on `bug_455199.pdf`. **Fires on a real file, silent
  across all 44 — two-sided.** The third validation this suite has produced
  and the first that **passed** rather than exposing a bad bound (the two
  before it, `MAX_TOKEN_LEN` and `MAX_XOBJECT_DEPTH`, were intuition-chosen;
  this one came from Annex C Table C.1). See §12's seventeenth 2026-08-07
  entry for the owed record and the eighteenth for the discharge.
- **★ Concrete instance (added 2026-08-18, `6af5655`, `Pass 75.0`,
  decision 071) — the first guard on a RETAINED structure rather than a
  transient one: `display_list::MAX_DISPLAY_LIST_BYTES` = 256 MiB.**
  Every guard above bounds something **produced and consumed** — a
  decoded stream, a pixmap, a rewrite pass. **A display list is HELD
  ACROSS FRAMES by design; that is the entire feature.** So a recorder
  retains every path, every clip and every image reference the page
  names, and a hostile 100 KB file becomes an **unbounded allocation that
  arrives as a HANG rather than as an error** — the same shape
  `MAX_REWRITE_OBJECT_NUMBER` was minted for (*"it looks like progress
  the whole way down, so a liveness or progress check cannot detect this
  class"*), reached by a different route. `record_page` refuses **by
  name** (`RenderError::PageNotRecordable`) once the running total
  crosses the ceiling, and the caller falls back to
  `render_page_region`, which is bounded by `MAX_PIXMAP_EDGE` as before.
  **The value is calibrated, not guessed** — ~~*"~8.5× the measured
  29.5 MiB of the A3 CAD reference sheet"*~~ **★ CORRECTED 2026-08-18:
  the CAD sheet (148,517 paints, 127,267 recorded ops, ~240 B per op,
  29.5 MiB) is NOT the largest list measured.** Real headroom against
  the largest observed input is **6.1×** — 256 MiB ÷ **41.9 MiB**, the
  list built by `veraPDF … 6.1.12 … t03-fail-c.pdf`, across 3,245 files.
  Against the CAD sheet it is 8.7×. **The ceiling did not move; the
  claim about it did.** The asymmetry is deliberate and is the whole
  justification — **a false refusal costs a fallback that is merely
  slower; a ceiling set too high costs the process.**
  **★★ The §6.1.12 implementation-limits run this bullet's own standing
  rule requires is DISCHARGED 2026-08-18** by
  `crates/pdfcer-render/examples/guard_probe.rs` (`2aa1066`): **3,245
  files walked — 3,145 recorded, 0 TOO-LARGE, 77 refused for a
  CAPABILITY reason (2.4 %: shading / overprint composite / soft mask,
  not the guard), 23 unloadable** — covering the whole
  `fixtures/external/veraPDF-corpus` (both the Isartor and veraPDF
  §6.1.12 suites, 32 files between them), every synthetic fixture, and
  `D:/Dev/temp/pdfce`. **The SILENT half is discharged at 73.8× the
  rule's 44-file bar. The FIRING half is NOT shown against a real file
  and is not claimed to be** — it fires only through
  `display_list.rs`'s injectable `max_bytes`, because no real file
  reaches 256 MiB. **Non-vacuity is instead established by the measured
  MAXIMUM (41.9 MiB, 16.4 % of the ceiling), which proves the
  accumulator counts real magnitudes** — an instrument the 2026-08-07
  `MAX_REWRITE_OBJECT_NUMBER` run could not have used, since its counter
  was an object number rather than a running total. Full ruling and the
  named alternative reading: `ROADMAP.md`'s `Pass 75.0` Shipped entry,
  hundred-and-eighty-fourth filing. Fourth guard to face that suite;
  **first to ship ahead of it, and first discharged by a ratio rather
  than by a refusal.**

#### ★★★ 10.1a — THE SCOPE OF THIS SECTION IS **UNTRUSTED INPUT**, AND AN OPERATOR-SET BOUND IS NOT IN IT (added 2026-08-26, `Pass 132.0`, `76eb04c`, §12 decision **089**)

**Everything above is about sizes that come from a FILE.** A decoded-output
cap, a nesting depth, an xref object count, a display-list ceiling — each
bounds a quantity an adversary chooses. **That is the property that makes the
bound necessary**, and it had been conflated with *"any large allocation"* for
as long as every ceiling in the renderer happened to be a compile-time
constant.

**A number the operator typed is not untrusted input.** So a ceiling the
operator sets is **uncapped**: no guard, no warning, no preflight. The first
one is
`pdfcer_core::settings::Settings::max_cmyk_buffer_bytes` — the subtractive
compositing buffer's size limit — on the operator's own ruling for
`max_zoom_percent`: *"it is up to the user to determine how much of a
performance hit they want to take."* The renderer's built-in
`DEFAULT_MAX_CMYK_BUFFER_BYTES` (256 MiB) remains, as a **default**, because
the *page's* dimensions are still untrusted and still decide whether the
default is reached.

**★★ THE OBLIGATION THAT COMES WITH IT, AND IT IS NOT OPTIONAL.** Removing a
cap on a size the operator names is safe **only where the allocation behind it
is fallible.** `vec![0.0; n]` and `Vec::with_capacity` allocate **infallibly**:
on failure they call the allocation error handler, which **aborts the
process** — no unwind, no error path, no page rendered, no disclosure. That is
tolerable while the only reachable size is a compile-time constant the project
chose. It stops being tolerable the moment an operator can name 64 GiB in a
text file.

`CmykBuffer::try_planes` therefore uses **`try_reserve_exact` + `resize`**, so
a ceiling the machine cannot honour produces **the same disclosed refusal**
(`cmyk_buffer_refused`, the page composited in sRGB and said so) as a ceiling
the page exceeded. **One failure mode, one disclosure, whichever end it came
from.**

⇢ **Binding on any future session that raises or removes an operator-settable
allocation bound: check that the allocation behind it is fallible, in the same
change.** Widening the bound and hardening the allocation are one edit.
Shipping the first without the second converts an operator's typo into a
crash. See §12 decision **089** for the full reasoning and the alternatives
weighed.

### 10.2 Fuzz-testing (required, not optional, before Pass 1 ships)

Set up a `cargo-fuzz` target against the tokenizer/object-parser as
part of Pass 1 (add explicitly to its acceptance criteria in
`docs/ROADMAP.md` if not already there by the time Pass 1 starts).
Minimum scope for the first fuzz target: raw byte-stream → tokenizer
→ COS object parser, asserting only "never panics, never hangs past a
bounded timeout, never allocates past a bounded ceiling" — not
semantic correctness (that's what the fixture-based tests in §5/§9
cover). Expand fuzz targets to each filter decoder as they're
implemented. Treat any fuzz-discovered crash as a release blocker for
the Pass that introduced the vulnerable code path, not a "file it and
move on" backlog item.

### 10.3 Where this lives in the codebase

Guard logic (size ceilings, depth counters, timeouts) belongs in
`pdfcer-core` itself — not bolted on as a wrapper in `pdfce-gui`/
`pdfcer` — so both front ends (and the future WASM fork) inherit
the same hardening automatically. Document each guard's default limit
and rationale in the doc comment of the function it guards, per the
documentation-first rule; a reader should understand *why* the number
is what it is (e.g. "1 GiB default output cap — larger than any
legitimate single decoded PDF stream this project has seen, small
enough that hitting it can't exhaust a typical machine's memory").

**Amendment (2026-07-30, Pass 1.1 slice):** the principle stated above
("guards belong in `pdfcer-core`") is precise for **parse-time**
recursion (page-tree walk, xref/ObjStm cycles) but not for
**render-time** recursion — a Form XObject's recursive `Do` execution
happens inside `pdfcer-render`'s content-stream interpreter (§3's
implementation note), which `pdfcer-core` has no visibility into (it
only ever sees one content stream's tokens at a time, never resolves
`Do` itself). `MAX_XOBJECT_DEPTH` therefore lives in `pdfcer-render`.
Both front ends (and the WASM fork) still inherit it automatically,
because both depend on `pdfcer-render` for any rendering at all — the
"automatically inherited by every front end" property is what actually
matters, not which of the two GUI-agnostic crates holds the constant.
General rule going forward: a guard against adversarial input lives in
whichever of `pdfcer-core`/`pdfcer-render` actually performs the
recursive/expanding operation being guarded.

### 10.4 A `debug_assert` postcondition is a tripwire, not a guard — the two are different mechanisms with different audiences (2026-08-30, `Pass 185.1`/`185.2`, standing rule `R236`)

**Added because §10.2 above is bootstrap-era text and does not describe the
mechanism this project actually leans on most.** §10.2 says *set up a
`cargo-fuzz` target* and *expand to each filter decoder*; twenty-seven targets
later, the interesting question is no longer *which parsers are fuzzed* but
**what tells you a fuzzed run went wrong.** Two distinct mechanisms answer
that, they are easy to confuse, and confusing them is what let a corruption
ship for seventy-four Passes.

| | a **guard** | a **tripwire** |
|---|---|---|
| example | `EditError::FieldObjectIsInPageTree`, `MAX_XOBJECT_DEPTH` | `debug_assert_page_tree_still_walks` (`edit.rs`), `debug_assert_not_in_path` (`writer/content.rs`) |
| present in the shipping build | **yes** | **no** — `#[cfg(debug_assertions)]`, compiled out |
| audience | the **operator** — it refuses, names the collision, and leaves a document they can look at | the **developer, and only via a test or a fuzzer** |
| what its silence means in release | the input was fine | **nothing at all** — it is not there |

**★★ The failure mode, concretely, because it is not obvious.** A
`debug_assert` postcondition over a committed state change reads like
protection. It is not. In the build operators run, a verb that violates it
returns `Ok`, saves `Ok`, and writes a file pdfcer cannot reopen — which is
precisely the shape the 2026-08-20 `/Contents` corruption had, and which
`debug_assert_page_tree_still_walks`'s own panic message names. **The only
thing that makes such an assertion speak is somebody generating the input**,
and a test suite generates the inputs its authors thought of. A `cargo-fuzz`
build has debug assertions on; that is the only reason `Pass 185.1`'s defect
was ever visible.

⇒ **`R236`:** every `debug_assert` postcondition over state derived from
untrusted input **owes a `cargo-fuzz` target over the verbs it guards, or a
written exemption at the site.** Writing such an assertion is an admission that
a corruption class exists and is undetectable in the shipping build; the
assertion is the *detector*, and it needs an *input source*.

★★ **The unit is NOT "a named helper", and that scoping was corrected before
the rule shipped.** `grep -rn "fn debug_assert" crates/*/src/` finds **2**; the
population of real **invocations** across `pdfcer-core` + `pdfcer-render` is
~~**24** (12 + 12), measured 2026-08-30 at `7ac98da`~~ → **22** (12 core + **10**
render), **re-measured 2026-08-31 at `baf0c29` (`Pass 189.0`)** and
**unchanged at `77631a6`** (`Pass 190.0`/`190.1` added +334 lines to `edit.rs`
and no new `debug_assert`) — census and per-site verdicts in `ROADMAP.md`'s
`R236` under *THE DENOMINATOR*. ★ **The line numbers in that table move and the
rows do not**; cite a site by its message and verb, not by a line.

★★★★ **AND THE DENOMINATOR MOVED *BECAUSE THE RULE WAS OBEYED*, which is a
defect in the census COMMAND rather than in anyone's arithmetic.** `R236`'s
remedy for a non-adversarial assertion is *"a written exemption at the site."*
`Pass 189.0` wrote one — 30 lines, correctly — and **it says `debug_assert` five
times, which is the word the census greps for.** On that one file, both sides of
one commit, decomposing exactly at both ends: `grep -c debug_assert` went
**10 → 13** (raw hits) while the graded population went **10 → 8**
(invocations), the difference being **0 → 5 prose lines**. ⇒ **The raw grep rose
30% while the real population fell 20%, in a single commit, driven by the
compliance action the rule demands.** The 350th filing found that a source grep
over a documentation-first codebase counts the codebase's own prose about the
construct and blamed the project's general documentation discipline; **this is
the sharper form — the mechanism is driven by the RULE, so the denominator
degrades monotonically in the direction of compliance and fastest where the rule
is best obeyed.** The remedy is unchanged and must not be relaxed: **file the
invocation count, always with its decomposition**, so the prose term stays
visible instead of folding into a total.

★★ **Establishing "covered" is a claim about the CALL GRAPH — linking is not
reaching.** Added 2026-08-31 from `Pass 189.0`, whose *"0 covered"* over
`cmyk_buffer.rs` was **measured rather than assumed**: every item in the module
is `pub(crate)`, and the **three fuzz targets that link `pdfcer-render` all stop
at a leaf parser** — `mesh_shading` builds a `ParseInput`, calls `mesh::parse`
and **never paints**; its `CmykIntent::default()` is a *field of the parse
input*, the sole reason a grep for `cmyk` lists it, and a **false positive for
reachability**. Same shape as `R236`'s own execution caveat below and as `R209`:
**an artefact's existence is not the artefact's effect.** The unsafe direction
is the comfortable one — *"something probably covers it"* is the assumption that
never gets checked. Recorded here as a step in the check rather than minted as a
rule (`n = 1`; see `Pass 189.0`).

★★ **The class has a SECOND ordering this rule does not produce, and the two
disagree.** `R236` sorts assertions by **provenance** — *is the state
untrusted-derived?* `Pass 189.0` found the operationally decisive question to be
*what does the **shipping build** do when the assertion would have fired?* Nine
of `cmyk_buffer.rs`'s ten answered **panic** (the compositing loops index by
`y * width + x`, so a mismatched operand runs off the end even in release, which
makes a `debug_assert` an adequate tripwire). **One answered *emit a wrong
page*:** `into_knockout` replaces the receiver's planes wholesale with clones of
`initial`'s, so a larger operand leaves every plane **longer** than
`width * height`, every index addresses the wrong pixel, and **nothing ever runs
off the end** — the image is sheared, silently, in release. That one is now a
**runtime refusal** (decision `110`'s remedy, second application). ⇒ **An
assertion can be EXEMPT under this rule and still be the most dangerous one in
its file.** ★★ **That second ordering is now standing rule `R238`** (minted
2026-08-31, 353rd filing, at `n = 2` — this instance plus fuzz finding #3's
mis-sizing, corrected above). `R236` asks *does this owe an input source?*;
`R238` asks *how bad is it when it is wrong?*, and the two orderings disagree.

★★★ **`R236`'S LEDGER IS EMPTY AS OF 2026-08-31 (`Pass 190.0`/`190.1`,
`77631a6`), AND THAT IS NOT THE SAME CLAIM AS "THESE VERBS ARE CLEAN."** Both
of the rule's remaining sites closed in one commit — the group cascade's two
derivations now agree by construction (`edit.rs:18713`), and annotation
deletion has a tracked fuzz target whose reachability was **measured**
(`fuzz/fuzz_targets/annot_delete_sequence.rs`, `edit.rs:23079`). **The target
still fires**, on a second and distinct route — a document with two `3 0 obj`
definitions, signature `BadKid(ObjId 3)` — at a site this section's own ledger
has recorded **COVERED** since the rule was minted. ⇒ **Finding a defect is
what coverage is FOR. `R236`'s ledger measures whether every tripwire has an
input source; it does not measure whether the verbs are correct.** A reader who
collapses those two hands the next session a false all-clear. **The rule also
does not retire** — its trigger is the postcondition set, which grows with the
crate.

★★★ **AND THE SECOND CARRIER IS THE STRUCTURAL LESSON OF `Pass 190.1`.**
`Pass 185.1`/`185.2` fixed *"a structural array entry that is also a structural
object"* for `/AcroForm` `/Fields` and built `refuse_if_in_page_tree` to do it.
`delete_annotation` — a page's `/Annots` — had the identical defect, and **the
helper existed and was never called from it.** ⇒ **A guard written for one
carrier is a claim about a CLASS**, and the class members are greppable at the
moment the first one is understood well enough to fix. That obligation was
already standing as **`R219`** and went unpaid; `R219`'s trigger is widened
from *routes to a behaviour* to **carriers of a hazard** in the same filing.
★ Note the pairing with the bullet below: `Pass 185.2` had **every caller and
the wrong set**; `Pass 190.1` had **the right set and a missing caller.** Both
failures are real, they are opposite, and **doing one does not discharge the
other.**
~~`grep -rn "debug_assert" crates/pdfcer-core/src/` finds **34**~~ — **struck
2026-08-30 (350th filing): that command returns 44, not 34, and always did;
of those, 7 are `cfg(debug_assertions)` attributes and 10 are comment prose
ABOUT `debug_assert`. A source grep over a documentation-first codebase counts
the codebase's own self-description.** The same fuzz
target's **third** finding is a **bare inline `debug_assert_eq!`**
(~~`edit.rs:17486`~~ → **`:18713`**, re-measured at `77631a6`) comparing two
independent derivations of one quantity — a postcondition in substance whatever
its syntax. **Fixed 2026-08-31 by `Pass 190.0`**: the two now agree by
construction, the prediction keyed on `ObjId` and the subtree selected
structurally rather than by name prefix. **The unit is: any assertion
made after a mutation, over committed state or over two independent derivations
of one quantity.** The named-helper grep is the cheap first cut, not the
population. **Discriminator:** *could two parts of this program disagree about
this?* If yes, adversarial input can make them.

★★★★ **Severity is part of the finding, not a footnote — AND THE FIGURE THIS
PARAGRAPH GAVE WAS WRONG, WHICH IS THE SHARPER LESSON. Amended 2026-08-31
(353rd filing, `Pass 190.0`, `77631a6`); standing rule `R238` is minted from
it.** The struck text:

> ~~That third item is a `debug_assert_eq!` over a **disclosure count**, so its
> release-build consequence is a **wrong `nodes_removed` reported to the
> operator — not corruption.** Filing it beside two page-tree destructions
> without saying so would send the next reader to the wrong priority.~~

**Measured.** Four hand-built shapes reproduce the two derivations'
disagreement deterministically, and **three are release-visible**: a `/T`-less
terminal makes `delete_field_group` **return `Ok` and delete nothing**; a
terminal with no `/Parent` makes it **write a dangling `/Kids` into the saved
file**; two shapes give a wrong count. The item was carried at the struck
sizing for a day and de-prioritised on it.

★★ **The moral survives and the figure beside it does not**, and the two halves
must not be collapsed. **An open item inherits the severity of what it was
found next to unless its own severity is stated** — still true, still the
reason to write a sizing at all. But: ⇒ **STATING a severity is not MEASURING
one.** A `debug_assert`'s `#[cfg(debug_assertions)]` gating says **where the
check runs** and **nothing** about what the shipping build does when the
checked property is false; deducing the second from the first answers a
question about the **guard**, not about the **bug**. **`R238`** makes the
release-behaviour question an explicit, written step. ★ The struck sizing
looked exemplary — specific, hedged, warning against over-prioritising — and
was propagated verbatim to three documents on the strength of being all three.

**★ This does NOT move a guard into the release build.** `Pass 111.0`'s
reasoning stands: the page-tree postcondition re-walks the whole tree,
`O(pages)` per command, and a batch job committing thousands of edits should
not pay for it. When a specific collision *is* found, the remedy is a **named
refusal** in the release build — that is what `FieldObjectIsInPageTree` is —
not a promoted assertion. The tripwire finds the class; the guard closes the
member.

**★★ What `R236` buys and what it does not, both measured on its founding
arc.**

- **It buys adversarial input.** The target found the first defect within two
  minutes of existing.
- **It does not buy a correct protected set.** `Pass 185.1` guarded three
  deletion routes; a fourth (`cut_field`) was verified to delegate. Route
  coverage was total — and every route consulted a set built from
  `page_slots`, whose `ancestors` chain stops at the `/Pages` root and **omits
  the catalog that points at it**. `Pass 185.2` is that omission.
  ⇒ **A guard built from "the tree" is not a guard against losing the tree,
  because the thing that reaches the structure is outside a walk of the
  structure.** Generalise this whenever a protected set is enumerated by
  walking what it protects.
- **It does not buy execution.** `.github/workflows/ci.yml`'s `fuzz-smoke` job
  runs `cargo +nightly fuzz build` and never `cargo fuzz run` — deliberately
  (*"minutes-long runs in CI buy little coverage and cost every push"*). Both
  defects in this arc were found by a **person choosing to fuzz**. `R236`
  schedules a target's compilation, not its execution, and a future reader must
  not read *"a target exists"* as *"the class is covered"*.

### 10.5 A structural defect that leaves the object graph AMBIGUOUS, not UNDEFINABLE, is opened under a disclosed, overridable policy — never refused outright (decision 145 — SHIPPED, `Pass 283.0`, standing rule `R248`)

*(Added 2026-09-09.)* Two operator rulings, verbatim, given after a real
drawing Acrobat opens and pdfcer refused:

> "We should be making pdfcer so that it opens pdfs that have errors, and
> have a way that it manages those errors such that they aren't fatal, and
> if the user can intervene in a decision that should always be an option
> along with them not having to intervene."
>
> "We should be doing this for all defects where it is possible to continue
> and open the file."

This is a posture for every future defect class the loader meets, not a fix
for the two defects present in the one file that surfaced it.

**The boundary, stated once so it is not re-litigated per defect.** A defect
is in scope when the object graph is left **AMBIGUOUS** — a choice between
two or more readings the file itself supplies (which value of a duplicated
key, where a stream really ends, where an object really terminates, what an
unreadable reference should resolve to). It is **not** in scope, and stays
fatal, when continuing would require pdfcer to **invent** a reading the file
supplies none of at all: no `/Root` is the pinned example (a test holds this
line so a future widening of the policy cannot cross it by accident), and
encryption with no working password is the second, because asking for the
password **is** the intervention.

**The mechanism (`Pass 283.0`).** `LoadOptions` carries one named policy per
defect class (`DuplicateKeyPolicy`, `StreamLengthPolicy`, `TerminatorPolicy`,
`UnreadableObjectPolicy`), each defaulting to whichever reading keeps the
file open. `Document::load_anomalies() -> &[LoadAnomaly]` reports every
decision actually taken, carrying **both** the value kept and the value
discarded — a count would say only that pdfcer chose; the pair is what lets
a shell show what it chose *between*. The CLI exposes one global override,
`--on-malformed keep-last|keep-first|refuse`. Taking the other reading means
**re-loading** under the other policy, not patching the result: the
discarded value was never built into the document, so there is nothing in
memory to edit.

**The unifying justification is the standard's own, not a tolerance pdfcer
invented.** §7.3.10: an indirect reference to an undefined object "shall not
be considered an error by a conforming reader; it shall be treated as a
reference to the null object." An object whose bytes will not parse is
undefined as far as every consumer is concerned, so omitting it produces a
document ISO 32000 **describes**, not one pdfcer fabricated.

**Relation to `R27` — extends its kernel, does not relax it.** `R27`, read
from *Standing rules*, is "unsupported codec sub-features fail clean and are
counted BY NAME" — a decoder-level rule whose actual target was always
*silence*, not refusal (a prior filing corrected a citation that had
misread it as a no-clamp/no-substitution rule generally; it is narrower —
see `ROADMAP.md`). This decision carries that same kernel — fail by name,
never substitute a guessed value silently — up one layer, from decoder to
loader, and adds the piece `R27` had no occasion to need: an operator-facing
override, because a loader's ambiguity is frequently something the operator
can genuinely arbitrate in a way a codec's missing sub-feature is not. **A
counted, disclosed, overridable decision is not silence.**

⇒ **Standing rule `R248`** (Standing Rules ceiling was `R246`; `R247` is
reserved-but-unclaimed for two unrelated triggers as of the 483rd
`ROADMAP.md` filing and is left untouched — `R248` is minted past it
deliberately rather than entangling with that reconciliation): *a newly
discovered structural defect is triaged against this posture before it is
fixed as a one-off refusal — if the file supplies two or more readings,
pdfcer picks one under a named default, discloses what it picked and what it
discarded, and lets the operator take the other reading; only a defect that
would require inventing a reading the file supplies none of stays fatal.*

**Body-section effects.** §5 (round-trip/minimal-diff) — **unaffected**: a
load-time policy decision changes what the in-memory object graph *is*, not
how a later save diffs against a base revision; no writer path was touched
and no new forced-full-rewrite sibling was created (contrast §5.10, where
recovery *does* force one — this mechanism does not). §3 (GUI-core
separation) — unaffected, no `Cargo.toml` touched. `docs/core-api/` owes the
new `Document`/`LoadOptions`/`LoadAnomaly` surface per the engineer's
always-rule; check that document directly for exact signatures rather than
this section.

**Scope, so it is not over-read.** The parser's own default
(`DuplicateKeyPolicy::default()`) stays `Refuse` — every caller that builds
a `Parser` directly (fuzz targets, the recovery confirmation pass) keeps the
behaviour it was written against. Only the **loader** opts in to leniency;
the parser does not opt in on its callers' behalf. `LoadOptions::default()`
is hand-written rather than derived, specifically so this asymmetry cannot
silently drift back into agreement.

**Addendum, 2026-09-09 (`Pass 283.1`, `d8fcb68`).** The mechanism above
shipped its operator-facing override on the bytes entry point only;
`Document::load_with_options(path, password, options)` is new, and
`pdfcer`'s own file-based `open_document` now goes through it. No decision
or body-section change — the boundary, mechanism and `R248` text above are
all unaffected; only the reach of the existing mechanism changed. Filed as
`R245`'s sixth dated instance (`ROADMAP.md` *Standing rules*) rather than a
new rule: a facility present on one of two parallel entry points and absent
from its twin — the same shape `R245` already names for a guard, now shown
for an affordance.

Full record: §12's 2026-09-09 entry, decision 145 (addended for
`Pass 283.1`); standing rule `R248`; `ROADMAP.md` *Shipped*,
`Pass 283.0`/`283.1`.

### 10.6 A required page-tree attribute absent (or dangling) defaults to the value the STANDARD itself names for that key, when one exists — never to one pdfcer invented (decision 150 — SHIPPED, `Pass 290.0`)

*(Added 2026-09-10.)* Two real files made the case. The operator's own
Acrobat-written signature stamps (`%APPDATA%\Adobe\Acrobat\DC\Stamps\
YTV_yyfVN1TzJ0_6oei-GB.pdf`) have a blank spacer page 1 with no
`/Contents` and no `/Resources`; pages 2 and 3 hold his two signatures and
are perfect. pdfcer refused all three, on every verb that walks the page
tree, because `page_tree::resolve_page` treated an absent `/Resources` as
`MissingRequired` and the `?` sits on a walk that returns ONE `Result` for
the WHOLE page tree — one spacer page cost every other page in the file.
`fixtures/synthetic/minimal.pdf`, pdfcer's own smallest legal fixture, has
the identical shape, so this walk could not read the project's own minimal
file either.

**The distinction from decision 145/`R248`, stated once so it is not
conflated.** Decision 145 covers a defect where the FILE supplies two or
more readings and pdfcer picks one under a named default. Here the file
supplies nothing at all — no `/Resources` key on the page or any ancestor,
or a reference that dangles to the null object (§7.3.10/§7.3.9). The
"other reading" is not lying in the file waiting to be chosen; it is named
by the STANDARD itself, once, for exactly this key: Table 30's own
`/Resources` row states *"If the page requires no resources, the value of
this entry shall be an empty dictionary."* Applying that stated default is
not a liberty pdfcer is taking — it is reading the row that already
answers the question.

**The boundary that keeps this from becoming a general "default anything
missing" licence.** Only a key the standard **names** a default for is
defaulted. `/MediaBox` has no such clause anywhere in the corpus — no
default box exists to fall back to — so its absence is unchanged:
`PageTreeError::MissingRequired("MediaBox")`, still fatal. A page whose
resources are genuinely empty is a fact about the file; inventing a media
box would not be.

**Corrected mid-Pass, by the dispatched spec-librarian, before this
shipped — worth recording because the correction replaced the argument,
not merely its wording.** The first draft argued a page with no
`/Contents` can never NAME a resource, so an empty resource dictionary
could never be observably wrong. That is false: §7.8.3's third bullet lets
a form XObject or a Type 3 font omit its own `/Resources` and inherit the
page's, and the ISO 32000-2 erratum extends that inheritance to an
ANNOTATION APPEARANCE STREAM — precisely the stamp-page shape that
motivated this Pass. The decision to default survives; the reason first
given for it does not. Dispatch the spec librarian **before** reasoning
from a clause, not after.

**Mechanism.** `page_tree::resolve_page` — an absent `/Resources` on the
page and every ancestor, or an indirect reference resolving to null, no
longer raises `MissingRequired`; it resolves to `Dict::new()` and sets
`Page::resources_defaulted = true`. A `/Resources` present and not a
dictionary is unaffected and is still refused, now by its own named
variant, `PageTreeError::BadResources` — the same present-but-wrong /
absent-and-degradable split the tree already makes for `/Contents`.
`/MediaBox` is untouched by this mechanism.

**Disclosure (decision 059/rule 4's discipline, applied at the loader
layer, same posture as decision 145).** `Page::resources_defaulted` on the
model; `pdfcer-render::Diagnostics::page_resources_defaulted` and
`TextDiagnostics::pages_resources_defaulted` on the two consuming crates;
`render-page`'s stable metrics line gains `page_resources_defaulted=<0|1>`
and `extract-text`'s gains `pages_resources_defaulted=<n>`, both
**appended**, never inserted, per the modules' own never-reorder
contracts.

⇒ No new standing rule minted. This is decision 145/`R248`'s fail-clean
kernel — never refuse when a defensible, disclosed reading exists;
disclose whichever one was taken — reaching a case that kernel had not yet
covered: a reading supplied by the STANDARD rather than by the file. It is
filed as its own decision rather than a dated `R248` instance because the
discriminator it establishes — *does the standard's own text for THIS key
name a default, checked key by key, never "is the omission plausible"* —
is a reusable interpretive method future Passes will need against other
required attributes, the same posture decision 149 took toward `R43`.

**Body-section effects.** §3 (GUI-core separation) — unaffected, no
`Cargo.toml` touched. §5 (round-trip/minimal-diff) — unaffected: this is a
read-time resolution: a defaulted resource dictionary is not written back
unless the operator otherwise edits the page. `docs/core-api/
01-reading-and-model.md` already carries the `resources_defaulted`/
`MissingRequired`-narrowed-to-`MediaBox` table update, done in the same
Pass per the engineer's always-rule.

**A test that leaned on the defect it was fixing, found twice in the same
Pass — filed as `R225`'s 18th dated instance, a new sub-shape.** Two
pre-existing tests obtained their "unwalkable page tree" fixture by
depending on `minimal.pdf`'s now-fixed defect, and both went RED when the
defect was fixed — not because either assertion was wrong, but because the
mechanism producing their precondition was the bug under repair. Every
prior `R225` instance is a test that measured LESS than its name or doc
comment claimed; this is the inverse — a test that measured exactly what
it claimed, reached through a defect rather than through the condition it
named. Both were repointed at a fixture built to fail unwalkability a
different way (`fixtures/synthetic/xref-recover/page-tree-cycle.pdf`, a
`/Pages` node listing itself in its own `/Kids`). Full text:
`ROADMAP.md`'s dated-instance note, this filing (494th).

Full record: §12's 2026-09-10 entry, decision 150; `ROADMAP.md` *Shipped*,
`Pass 290.0` (494th filing).

### 10.7 A rasterizer's own ceiling is a measured floor, never a derived constant (decision 151 — SHIPPED, `Pass 296.0`)

A region render at extreme zoom panicked inside tiny-skia's own rasterizer —
a worker-thread crash, not a bounded refusal — and unlike every guard in
§10.1 it is not adversarial-FILE input; it is the operator's own zoom
control, the category §10.1a already carves out as uncapped-but-must-
stay-fallible.

**Measuring the boundary across six page geometries found no ordering
variable.** The scale at which tiny-skia's rasterizer breaks does not
correlate with page width, page area or device extent — the largest sheet
measured failed at the *lowest* scale, and a business-card-sized page shared
a boundary A4 never reached. A single named constant would therefore have
been **invented**, not measured, whichever value was chosen — the failure
mode §10.1a's discipline exists to prevent, one layer further out: there the
risk was an uncapped bound with a fallible allocation behind it; here it is
an exact-sounding number with no measurement behind it.

**Mechanism.** `RenderError::RasterizerLimit` is returned once a region
request crosses `MAX_GUARANTEED_REGION_SCALE`, checked before the call
reaches tiny-skia (`crates/pdfcer-render/src/lib.rs`). **The constant is
published as a floor below the lowest observed failure, never as the
boundary itself** — the guarantee pdfcer makes is the caught, named
refusal, not the number attached to it.

⇒ **Standing rule `R252` minted**: when a measured boundary does not order
with any input dimension, publish it as a floor below the lowest observed
failure and make the guarantee the caught refusal, not the constant.

Full record: §12's 2026-09-11 entry, decision 151; `ROADMAP.md` *Shipped*,
`Pass 296.0` (509th filing).

## 11. Undo/redo architecture

Identified as a real design gap 2026-07-23: the UI standing rule
"every edit is undoable" (see `pdfcer-ui-specialist.md`) was never
reconciled with the round-trip/minimal-diff invariant (§5) — the two
interact in a way that needs an explicit mechanism, not just a UX
promise.

### 11.1 The core design: command log over the in-memory object graph, diffed at save time

- Every edit the user makes is represented as a small command object
  (`PendingEdit` or similar) with `apply()` and an inverse/`revert()`,
  operating on `pdfcer-core`'s **in-memory** `Document` object graph —
  never on file bytes directly, and never on the saved file at all.
  The undo stack holds these commands.
- The **original loaded byte buffer / object graph is retained
  unmodified** as the "base revision" for the life of the open
  document (this is already required by §5 for lazy round-trip
  passthrough — undo reuses the same retained state, doesn't add a
  new one).
- Undo/redo operates **entirely pre-save**: hitting Undo reverts the
  in-memory graph via the command's inverse. It has no relationship to
  what's on disk until the user actually invokes Save.
- **Critical rule**: the "dirty set" (which objects actually differ
  from the base revision, i.e. what an incremental save must include)
  is computed as a **structural diff against the base revision at save
  time** — it is *not* the union of every object any command ever
  touched during the session. If a user edits an object and then
  undoes that specific edit before saving, that object must **not**
  appear in the incremental update, because compared to the base
  revision nothing net changed. Tracking "was this object touched
  by history" instead of "does this object currently differ from
  base" would silently violate the minimal-diff promise the moment
  undo is involved — this is exactly the subtle bug this section
  exists to prevent someone from introducing.
  **★ CROSS-REFERENCE ADDED 2026-08-13 (decision 059) — a USER-FACING
  guarantee now rests on this bullet.** Project rule 4's first clause,
  as amended that day, says **the commit point is SAVE**: nothing in an
  open edit session is document state, **Undo rejects and Save commits**,
  and therefore an inference that lands in the session and draws on the
  canvas needs **no accept/reject gate and no provisional marking** in
  front of it — *the session IS the preview*. **That clause is true only
  because of the save-time-diff rule stated immediately above.** If the
  dirty set were ever changed to "every object any command touched",
  **an undone inference could reach the saved bytes**, and rule 4's
  clause 1 would become false in the same edit that made the change.
  So: **this bullet is not merely an internal correctness rule about
  incremental save — it is the mechanism a user-visible promise is
  built on**, and any future proposal to track a touched-set instead
  must be read against decision 059 as well as against §5.
- Redo stack is invalidated (cleared) the instant a new edit is made
  after an undo — standard editor behavior, stated here for
  documentation-first completeness, not because it's subtle.
- Bound the undo history (a configurable max operation count) rather
  than keeping it unbounded — large documents with long editing
  sessions shouldn't accumulate unbounded command-object memory.
  Acrobat itself bounds undo; matching that expectation is fine.
- **CORRECTION (2026-08-03, Pass 17.1) — at most one `ObjectWrite` per
  object id per command.** The command model above assumes a command's
  writes compose; in practice `EditSession` applies a command's
  `ObjectWrite`s in sequence against the PRE-command state, and nothing
  commits mid-command — so a SECOND whole-dictionary `ObjectWrite` to an
  object id already written earlier in the SAME command **replaces**
  the first rather than merging with it. Found via `flatten_fields`,
  which issued three whole-dictionary writes to the SAME page object in
  one command (`/Contents`, `/Resources /XObject`, `/Annots`), each
  cloned from the identical pre-command page dict; the `/Annots` write
  landed last and silently discarded the `/Contents`/`/Resources`
  changes, so every flattened form lost its burned-in visible values
  while still reporting correct counters (`fields_flattened`/
  `widgets_burned`/`pages_touched`). No existing test caught this
  because none rendered the result — R85 (`ROADMAP.md` Standing rules)
  closed exactly this class of gap. **Binding rule going forward:** a
  command that needs to touch the same object's `/Contents`,
  `/Resources`, AND `/Annots` (or any other combination) in one step
  must accumulate ONE merged dictionary write per id, never issue N
  separate whole-dict writes to the same id within a command. Other
  multi-write commands are owed the same audit — not yet performed
  exhaustively as of this entry. Full record: `ROADMAP.md`'s Pass
  17.1/17.2 Shipped entry.

### 11.2 Redaction is the deliberate exception, and only after save

Redaction's true-content-removal behavior (§5 corollary) is
undo-able **like any other edit, right up until the document is
saved**. Once a redaction has actually been written to disk (the
underlying content genuinely gone from the saved bytes), that save is
not reversible by "Undo" in a later session — there is no data left
in the file to restore. This matches real-world expectation (redact +
save = permanent) and is exactly why the UI standing rule requires an
explicit, honest confirmation dialog for redaction specifically
(`pdfcer-ui-specialist.md`) — the operator needs to understand *before*
saving that this is the one edit type Undo can't rescue them from
after the fact.

**Cross-reference added 2026-07-31 (Pass 3.0, decision 007 W2/R35):**
the *save-side* half of this exception is now specified in **§5.2**.
Redaction must force a **full rewrite** and must **refuse incremental
save**, because incremental save structurally preserves superseded
content — the old bytes of every replaced object stay in the file by
construction (§7.5.6). Without that rule, a redaction saved in pdfcer's
*default* mode would leave the redacted content trivially recoverable,
and the confirmation dialog this section describes would be promising
something the writer did not deliver. §5.5 records the resulting
conflict with signed documents, which is a genuine operator either/or.

**Correction cross-reference added 2026-07-31 (Pass 3.1):** forcing a
full rewrite is NOT by itself enough to make redacted content
"genuinely gone from the saved bytes" when the redacted object lives
in an object stream — containers carry through verbatim in both save
modes, so the Redaction Pass must also rewrite/decompose the
containers holding redacted objects. See §5.7.

### 11.3 Snapshot fallback for bulk structural edits

The command-pattern model is the default for content-level edits (text,
annotations, form fields, single-page operations). For bulk structural
operations where per-item commands would be awkward (e.g. reordering
50 pages in one drag operation), a coarser "before/after page-order
snapshot" command is an acceptable specialization of the same pattern
— still one undo-stack entry, still diffed against the base revision
at save time via the same mechanism. Don't invent a second, parallel
undo system for this case.

### 11.4 Scope for Pass planning

Read-only Passes (Pass 1) need none of this. **The first Pass that
introduces any editing capability must build the command-log/undo-
stack mechanism as part of that Pass, not after** — retrofitting undo
onto edit code that was written assuming direct mutation is
significantly more expensive than designing it in from the first edit
feature. Flag this explicitly when `docs/ROADMAP.md` scopes the first
editing Pass.

### 11.5 Implementation record — the overlay design (Pass 3.1, 2026-07-31)

*(§11.4's obligation bound at Pass 3.1 — the first editing Pass — and
was honored: the mechanism below shipped in that Pass, not after.
This section records the shape actually built, so §11.1's design
prose and the code stay reconcilable.)*

- **`EditSession` command log** (`crates/pdfcer-core/src/edit.rs`,
  1,608 lines): every edit is a command with apply/revert, exactly as
  §11.1 specifies — operating on an **overlay** above the base
  revision, never on the base object graph and never on file bytes.
  The base revision (buffer + parse) stays untouched for the life of
  the open document; the overlay holds only the objects that
  currently differ.
- **The dirty set is derived, not accumulated:** at save time the
  overlay yields a `DirtySet` (replacements + trailer patch +
  `changes_content`) as a diff against the base revision. An
  edit that has been undone leaves no trace in the overlay, so it
  cannot appear in the save — §11.1's "union of every command ever
  run" bug is structurally unexpressible, and executably pinned:
  **edit → undo → save is byte-identical, 2,897/2,897 corpus files
  (100%)**, plus dedicated fixture tests including a 12-command
  history and undo → redo → save.
- **One writer path:** both save modes take `&DirtySet`;
  `DirtySet::empty()` is Pass 3.0's identity writer as a strict
  pinned subset (§5.7).
- **Undo granularity matches operator intent:** the GUI applies edits
  on button press, not per keystroke — one undo step per intent, so
  the stack holds meaningful operations (a deliberate Pass 3.1
  decision, §12 continuation-18 entry).
- Redo invalidation on new-edit-after-undo behaves as §11.1 states.

### 11.6 Implementation record — `coalesce_last`, the second mechanism the undo stack grew (Pass 168.0, 2026-08-29; decision 101)

*(§11.5 records the overlay design as built. This records the one addition
the stack has taken since, so §11.1's "one undo step per intent" prose and
the code stay reconcilable.)*

- **The problem it solves.** `R168` requires a verb offered on an N-target
  selection to act on the whole selection or refuse; `R179`/`R49` require one
  gesture to be one undo entry. Several deletion verbs **route** to
  specialised verbs that commit for themselves (§4.1 (L) —
  `delete_annotation` routes ce dimensions and redaction marks), so a
  multi-target gesture cannot build its own writes without duplicating the
  most intricate deletion logic in the crate.
- **The mechanism.** `EditSession::coalesce_last(count, kind) -> bool`
  (**`pub` since `Pass 212.0`, 2026-09-01 — was private at `Pass 168.0`**;
  `crates/pdfcer-core/src/edit.rs:12926`, verified live 2026-09-01) folds the
  last `count` entries on the undo stack into **one** entry carrying `kind`.
  The verb calls the per-target verb N times, then folds. Routing, refusals and
  spec-governed behaviour stay in one implementation.
- **The fold COLLAPSES repeated objects; it does not concatenate.** `undo()`
  applies each recorded `before` walking **forward**, which is correct only
  while an object appears at most once per command. A repeated object is
  therefore collapsed to a single write taking the **earliest `before` and the
  latest `after`**; removals and the trailer collapse the same way. A naive
  concatenation restores the original and then re-applies the intermediate,
  leaving a document that **looks fine and is wrong by one**. Pinned by
  sabotage: neutering the duplicate lookup fails
  `cutting_two_annotations_from_one_page_is_one_undo_entry_and_undo_restores_both`
  with exactly `left: 1, right: 2`.
- **`count` is measured from the stack's own depth, never from the caller's
  intention.** A verb that calls a helper which may **return early without
  committing** (`set_outline_open` when the state already matches) can
  otherwise pass a count larger than what reached the stack; the fold's
  `undo.len() < count` guard then correctly refuses, and the gesture
  **silently becomes N undo entries** with nothing erroring. See decision 101.
- **`count == 1` is not a no-op — it relabels.** Without it a one-target cut
  carries the destination verb's `CommandKind` and an undo control reads
  *"undo delete"* after the operator pressed **cut**, which is a false promise
  about the clipboard. `CommandKind::CutSelection` and
  `CommandKind::PasteOutlineItem` exist for this.
- **Bounded by `MAX_UNDO_DEPTH` (256):** a larger selection is refused **by
  name before anything is removed** (`EditError::SelectionTooLargeForOneUndo`).
- **Not a replacement for §11.3's snapshot fallback.** Snapshots remain the
  answer for **bulk structural** edits; `coalesce_last` is for N genuine,
  individually correct, individually spec-governed commands the **operator**
  performed as one act.
- **Current users** (2026-08-29): `cut_selection`, `cut_annotations`,
  `cut_field`, `cut_pages`, `cut_outline_item`, `cut_attachment`,
  `paste_outline_item`.
- **★ It is now a PUBLIC primitive as well as an internal one**
  (`Pass 212.0`, 2026-09-01, decision `117`). The crate boundary was drawn one
  notch too tight: **a shell gesture that needs two verbs — place a push
  button, then give it an action — cost two undo entries**, so `Ctrl+Z` took
  the action off and left an inert button on the page. `cut_field` already
  composes exactly this way internally, so the composition was never in
  question — only whether it may be spelled **outside** this crate. **The
  narrower `add_push_button_with_action` was offered and declined**: it fixes
  one instance of a shape that recurs every time a gesture needs two verbs.
- **The PUBLIC contract states three things the internal one never had to.**
  **(1) Check the return.** `false` means every change was **applied** and only
  the **grouping** failed (the stack was shorter than `count`) — disclose that
  the gesture takes more than one undo; do not retry. **(2) `count` counts
  commands YOU just pushed**, most recent first; **overcounting folds a
  neighbour's edit in and nothing guards that** — it is the same
  count-from-the-stack hazard the bullet above records, now reachable by a
  caller this crate cannot see. **(3) Fold immediately**, before anything else
  can push a command. `0` and `1` return `true`; `1` relabels rather than
  no-ops (see the `count == 1` bullet above).

### 11.7 Implementation record — ONE page-tree reader: the overlay (Pass 186.0, 2026-08-31; decision 111)

*(§11.5 records the overlay design as built; §11.6 the one mechanism the undo
stack has grown since. This records the correction of a **split** in how that
overlay was read — the design in §11.5 was right and eight call sites did not
follow it.)*

**The state this section replaces.** `EditSession` carried **two** page-tree
readers. The **authoring** verbs used the overlay-aware
`EditSession::pages()`. **Eight content-editing entry points** used
`page_tree::pages(&self.base)` — the document **as it was on disk**. Both were
individually defensible; together they meant a shell's page index and the
engine's page index could name **different sheets**, with no refusal and no
disclosure.

**The rule now (decision 111).** **Every operator-facing verb resolves a page
index through `EditSession::pages()` — the overlay.** A front end computes an
index against **what the operator is looking at**, which is the overlay by
definition; a verb resolving that index against the base is **addressing a
different sheet**.

**Two consequences that follow directly, and are the reason this is a §11
concern rather than a bug fix:**

- **Content appended this session is part of the page.** `add_image`,
  `add_text`, `paste_objects` and `flatten_fields` append a new content stream
  (and often a new resource) into the staging overlay. A base-derived page's
  `/Contents` does not name it, and `vector/decompose.rs` **emits no object for
  a `Do` it cannot classify** — so the object was on the canvas and absent from
  the model. Resolvers (`DocumentXObjects` / `DocumentFonts`) are now built
  from the **session view**.
- **Structural page edits move the addresses.** `delete_pages`,
  `insert_pages`, `reorder_pages` and the merge verbs commit into the overlay.
  Measured before the fix on `fixtures/synthetic/pageops/four-pages.pdf`: after
  `delete_pages(&[0])`, `page_objects(3)` on a **three**-page document returned
  **the text of page four**.

**★ THE ONE NAMED EXCEPTION, and it is narrow on purpose.** `reflow_block`'s
planner (`plan_reflow_from_doc`) is **base-indexed by necessity** — it needs
extraction provenance the staging buffer does not carry, which is the same
reason its pre-existing already-edited refusal exists. Converting it silently
would splice **one sheet's reflowed bytes into a different sheet's content
object**: **strictly worse** than the defect being fixed, which at least kept
both halves consistent with each other. It therefore **refuses by name** when
the base page at `page_index` is not the overlay page at `page_index`.
Teaching the planner the overlay is a real feature and is **not** `Pass 186.0`.

**The model-agreement query.** `EditSession::page_content_generation(page_index)
-> u64` — a 64-bit FNV-1a digest of the decomposition memo's key (page id,
every `/Contents` entry with its staged span, the effective `/Resources`, and —
since `Pass 188.0` — every form the walk reached). A shell asserts agreement
**continuously** instead of decomposing twice. **Three non-promises, documented
at the verb and in `docs/core-api/`:** it is **not** a content digest, **not**
stable across sessions or saves, and **not** minimal (it may change when the
model did not).

**★ AMENDED — `Pass 197.0` (`6e2b69e` + `28b982c`, decision 113).** The verb
was `&self` and could only report whatever key value the memo already held
cached from a previous mutating call; `Pass 188.0` widened the KEY (above)
but nothing on the read path forced a fresh walk before hashing it. A
session whose only mutation rewrote a form XObject's own content — without
otherwise triggering a redecomposition — could see the digest report
**unchanged** across the edit: `pdfcer-core`'s own internal staleness
handling had become strictly stronger than the signal it published
externally. **BREAKING: the verb is now `&mut self`** and forces a fresh
decomposition walk, forms included, before hashing — so the digest always
reflects the key as of the call, never a stale cached one. `PageObjects`
addresses content by INDEX, so a consumer trusting an unmoved generation
after a real form edit was the silent-corruption shape — reported and
diagnosed by the consuming shell, sabotage-verified against its own
reported numbers with a nothing-changed control.

**★★ THE TESTING PROPERTY THIS SECTION EXISTS TO PIN.** Base and overlay
**agree by construction** on a session whose page set has not been structurally
edited and whose content has not been appended this session. **Every** test in
`pdfcer-core` was of that shape, so all **4,861** passed identically before and
after the fix. **The property needs TWO VERBS IN ONE SESSION to be observable
at all.** `crates/pdfcer-core/tests/session_overlay_skew.rs` (10 tests) is the
suite that has that shape; a future refactor that reintroduces a base-only read
is caught **only** there.

## 12. Decision log

> **Decisions before 2026-09-01, and the retired §4.1 surface log, are in [`history/architecture-decisions-before-2026-09.md`](history/architecture-decisions-before-2026-09.md)** — verbatim, still binding, still read by `check-ledger-numbers.py`.
> Moved there 2026-09-10, when this file had reached 34,340 lines.

**Index of the archived entries** — number or date, and the verdict in a line. Grep the archive for the argument.

- 2026-07-23 — Project bootstrap.
- 2026-07-23 (same-session amendment) — Added `pdfce-cli` as a
- 2026-07-23 (same-session amendment 2) — Added a second reference
- 2026-07-23 (same-session amendment 3) — Added §9, open-source
- 2026-07-23 (same-session amendment 4) — Research synthesized
- 2026-07-23 (Pass 0 — workspace bootstrap) — Pass 0 shipped (see
- 2026-07-30 — `oxidize-pdf` gate CLOSED: decision (c)
- 2026-07-30 — i18n/l10n architecture decided (decision 002; second
- 2026-07-30 — Distribution posture decided (decision 003; third use
- 2026-07-30 — Pass 1 text-rendering font strategy decided (decision
- 2026-07-30 (continuation 8) — Pass 1.1 item 1 shipped
- 2026-07-30 (continuation 9) — Pass 1.1 slice shipped: Form and
- 2026-07-30 — Image-codec strategy decided (decision 005; fifth use
- 2026-07-30 (continuation 12) — Pass 2.1 shipped (DCT + LZW +
- 2026-07-31 — CMYK/YCCK JPEG inversion rule decided (decision 006;
- 2026-07-31 (continuation 15) — Pass 2.3 shipped (JPXDecode via
- 2026-07-31 (continuation 16) — Next subsystem decided (decision
- 2026-07-31 (continuation 17) — Pass 3.0 shipped (identity writer
- 2026-07-31 (continuation 18) — Pass 3.1 shipped (mutation writer
- 2026-07-31 (continuation 19) — Pass 3.2 shipped (structural page
- 2026-08-01 (continuation 20) — Pass 4 shipped (text extraction /
- 2026-08-01 (continuation 21) — Decision 008: next subsystem after
- 2026-08-01 (continuation 22) — §7.6 encryption spec-corpus session
- 2026-08-01 (continuation 23) — Pass 6.0 shipped (annotation &
- 2026-08-01 (continuation 24) — Pass 6.1 shipped (authored streams +
- 2026-08-01 (continuation 25) — Pass 6.2 shipped (text-bearing
- 2026-08-01 (continuation 26) — Pass 7.0 shipped (AcroForm field model
- 2026-08-01 (continuation 27) — Pass 7.1 shipped (form flatten +
- 2026-08-01 (continuation 28) — Pass 8.0 shipped (Redaction — mark +
- 2026-08-01 — Post-redaction priority decided (decision 010; the
- 2026-08-01 — Pass 11 SHIPPED (render-fidelity verification harness) +
- 2026-08-01 (GUI-polish interlude + launcher) — An operator-requested
- 2026-07-31 — Root-cause font fix (NUL-misroute) + operator-supplied
- 2026-07-31 — Cross-reference recovery decided (decision 013); Pass 13a
- 2026-07-31 — Acrobat-style in-place text editing decided (decision
- 2026-08-01 — Pass 13b (rebuild-by-scan xref recovery) SHIPPED;
- 2026-08-01 — FF-A within-block offline reflow decided (decision 015);
- 2026-08-01 — Next text-parity step prioritized + FF-D scoped
- 2026-08-01 — License = MIT (operator decision).
- 2026-08-02 — Decision 018: the canvas renders the edited document
- 2026-08-02 — Decision 017: two-compartment vertical panel list for
- 2026-08-02 (same-day continuation 56) — Decision 018 implementation
- 2026-08-02 (same-day continuation 57) — Decision 017 AMENDMENT A:
- 2026-08-03 (same-day continuation 58) — Decision 018 follow-up:
- 2026-08-03 (same-day continuation 58) — Decision 017 Amendment A,
- 2026-08-03 (same-day continuation 60) — Decision 017 Amendment A
- 2026-08-03 (same-day continuation 61) — Decision 017 Amendment A /
- 2026-08-03 (same-day continuation 60) — Documentation-process
- 2026-08-03 — Decision 019: FF-H re-scoped to direct text-state
- 2026-08-03 (same-day, Amendment A to decision 019) — Pass 19.0
- 2026-08-03 (same-day, Amendment B to decision 019) — Pass 19.1
- 2026-08-03 (same-day, Amendment C to decision 019) — Pass 19.2
- 2026-08-03 (same-day, Amendment D to decision 019) — Pass 19.3
- 2026-08-03 (same-day, Amendment E to decision 019) — the §3.3 `Tw`
- 2026-08-03 (same-day, decision-013 addendum, no new decision
- 2026-08-03 (same-day, Amendment F to decision 019) — Pass 19.4
- 2026-08-03 (same-day continuation 71) — Decision 020 filed: form
- 2026-08-03 (same-day) — Decision 021 filed: FF-C, font subsetting
- 2026-08-03 (same-day) — Decision 021 AMENDED after
- 2026-08-04 (continuation 76) — Decision 021 implementation update:
- 2026-08-04 (continuation 77) — §3/§4 body-section sync for Pass
- 2026-08-04 (continuation 80) — Decision 022 filed: annotations in
- 2026-08-04 (continuation 80) — Decision 023 filed: the Obj tool is
- 2026-08-04 (continuation 81) — Decision 024 filed: a ribbon command
- 2026-08-04 (continuation 82) — Decision 025 filed: the subpath rung
- 2026-08-04 (continuation 82) — Decision 026 filed: linear
- 2026-08-04 (continuation 82) — Forward pointer on decision 023's
- 2026-08-05 (continuation 83) — Decision 027: REFUSE what has no good
- 2026-08-05 (continuation 85) — Decision 028 filed: the node rung made
- 2026-08-05 (continuation 85) — Two correctness fixes to shipped GUI
- 2026-08-05 (continuation 87) — decision 029: development stays ONE
- 2026-08-05 (continuation 88) — §4 SYNCED against the crate for the
- 2026-08-05 — Decision 030: preserving the option of a future plugin
- 2026-08-05 (continuation 89) — Decision 031: where implicit commit
- 2026-08-05 (same-day continuation 92) — Decision 031, BUILD
- 2026-08-05 (continuation 94) — Pass 34.1 slices 2–3 SHIPPED
- 2026-08-06 (continuation 105) — The left dock's shape is decided
- 2026-08-06 (continuation 105, second entry) — The canvas claims
- 2026-08-06 (continuation 105, third entry) — Retained
- 2026-08-06 (continuation 107, first entry) — When widening a
- 2026-08-06 (continuation 107, second entry) — Selection and
- 2026-08-06 (continuation 107, third entry) — A confirmation
- 2026-08-06 (continuation 107, fourth entry) — UI density comes
- 2026-08-06 (continuation 108, first entry) — "ENABLED" and
- 2026-08-06 (continuation 108, second entry) — A rule stated as a
- 2026-08-06 (continuation 108, third entry) — A tree renders the
- 2026-08-07 (first entry) — A certified document's signature
- 2026-08-07 (second entry) — A form field is THREE writes and they
- 2026-08-07 (third entry) — When a decision record REJECTS a write
- 2026-08-07 (fourth entry) — the write-side field-path resolver
- 2026-08-07 (fifth entry) — two defects found building the resolver
- 2026-08-07 (sixth entry) — R105 (`/TU` mandatory-or-declined) and the
- 2026-08-07 (seventh entry) — the CLI verb shape for field creation is
- 2026-08-07 (eighth entry) — the radio verb is RULED
- 2026-08-07 (ninth entry this day) — Pass 20.2 COMPLETE: radio groups
- 2026-08-07 (tenth entry this day) — Pass 20.5 PARTIAL: the GUI can
- 2026-08-07 (eleventh entry this day) — veraPDF is ELECTED UNDER
- 2026-08-07 (twelfth entry this day) — `R163` IS MINTED: prefer making
- 2026-08-07 (thirteenth entry this day) — A GATE THAT DIVERGES FROM
- 2026-08-07 (fourteenth entry this day) — VALIDATION AGAINST A
- 2026-08-07 (fifteenth entry this day) — A MISSING `endobj` COSTS THE
- 2026-08-07 (sixteenth entry this day) — `R164` IS MINTED: A VERDICT
- 2026-08-07 (seventeenth entry this day) — THE FULL-REWRITE WRITER
- 2026-08-07 (eighteenth entry this day) — A FULL REWRITE DROPS BYTES
- 2026-08-07 (nineteenth entry this day) — `R165` IS MINTED: WHERE A
- 2026-08-07 (twentieth entry this day) — THE RENDERER'S COST CENTRE IS
- 2026-08-07 (twenty-first entry this day) — THE CLIP BECOMES SHARED
- 2026-08-07 (twenty-second entry this day) — THREE FIGURES WRONG BY TWO
- 2026-08-07 (twenty-third entry this day) — `R166` IS MINTED: A NUMBER
- 2026-08-07 (twenty-fourth entry this day) — THE RENDERER HAS A
- 2026-08-07 (twenty-fifth entry this day) — THE 86% IS BROKEN DOWN AND
- 2026-08-07 (twenty-sixth entry this day) — A RENDER CAN BE STOPPED, AND
- 2026-08-07 (twenty-seventh entry this day) — THE RENDER MOVES TO A
- 2026-08-07 (twenty-eighth entry this day) — ONE CLIP PATH IS 97.3% OF
- 2026-08-07 (twenty-sixth filing, `ce57ed5` + `c3d8853`) — THE CLIP-MASK
- 2026-08-07 (twenty-seventh filing, `9681112`) — THE RENDER WORKER STARTS
- 2026-08-07 (twenty-eighth filing, `3d345aa`) — THREE DECISIONS ON FIELD
- 2026-08-07 (twenty-ninth filing) — geometry manipulation is its own capability, not a forms one: `Pass 46.0` + `Pass 46.1` filed on the operator's request; re-flag sta…
- 2026-08-07 (thirtieth filing) — decision record `032` is OPENED, NOT DECIDED: the vector-scale mechanism (wrap in `cm` vs rewrite operands) becomes a recorded question…
- 2026-08-07 (thirty-first filing, `247b8fa` + `fd6eadd`) — F6 CLOSES, `Pass 20.6` STOPS BEING PARTIAL, AND `Pass 46.0` DELIVERS ITS FIRST SLICE — plus TWO CORRECTIONS T…
- 2026-08-08 (thirty-second filing, `baeb624`) — `Pass 20.3` COMPLETES and F3 CLOSES: three engineer rulings on push buttons, and a correction to a plan that said a key…
- 2026-08-08 (thirty-fifth filing) — pdfce's FIRST image encoder ships (`jpeg-encoder` 0.7.1, R28's first exception); write-side CMYK/YCCK polarity RULED to warrant its…
- 2026-08-08 — Decision 006 §3.7's deferred colorimetry gap is CLOSED
- 2026-08-08 (`Pass 51.0`, `2a1b0df`) — R15's user-state partition is
- 2026-08-08 (`Pass 51.3`, `6d63d81`) — `pdfce_render::font::
- 2026-08-08 (`Pass 51.4`, `6d63d81`) — the operator settings surface
- 2026-08-09 (`Pass 38.5`, `0a727bb`, first entry) — annotation
- 2026-08-09 (`Pass 38.5`, `0a727bb`, second entry) — a preview query
- 2026-08-09 (`Pass 38.5`, `0a727bb`, third entry) — the general
- 2026-08-09 (`Pass 38.5`, `a4c1a8e`, fourth entry) — the `/P`-aware
- 2026-08-09 (`Pass 38.5`, `b8e23c8`) — `AnnotationDeletion::appearance_streams_removed`
- 2026-08-09 (`pdfce-gui`, `fcb6544`) — the Comments panel's per-row
- 2026-08-09 (`Pass 23.3` residual, `e1430d8`) — anchors passed to a
- 2026-08-09 (`Pass 23.3` residual, `e1430d8`) — a correctness argument
- 2026-08-09 (`Pass 23.3` GUI half, `6fb7ffb`) — a multi-step
- 2026-08-09 (`Pass 32.0` core+CLI, `462fe0e`→`947ea5d`→`5bfb8fc`→
- 2026-08-09 (`Pass 32.0` CLI, `5bfb8fc`) — `EditSession::
- 2026-08-09 (`Pass 32.0` GUI half, `03c4c0f`) — the Part rung is
- 2026-08-09 (`Pass 32.0` GUI half, `03c4c0f`) — a channel-routing
- 2026-08-09 (process note, no code) — a relayed gate figure is
- 2026-08-09 (`b5b9f23`) — `set_edit_note` is now the single traced
- 2026-08-09 (`Pass 32.1`, `e85824a`) — the object-rung delete
- 2026-08-09 (`d3ea5de`, `1edf4e3`, `9abf5b5`, `e167867`, `01b90c4` —
- 2026-08-09 (`8672cbc`, `62dda19`, `365856f`, `817d518`, `9a2bc15` —
- 2026-08-09 — DXF export (`Pass 52.0`/`52.1`/`52.3`, `3c4aca4`→
- 2026-08-09 — `Pass 52.2` core+CLI substrate (`d2d03a5`): a fourth
- 2026-08-09 (fifty-fifth filing) — CORRECTION to the fourth-fork
- 2026-08-09 (fifty-sixth filing) — `Pass 52.1`'s CLI slice extended
- 2026-08-09 (`Pass 53.0`, `a3ba0f8`) — a display-string/edit-string
- 2026-08-09 (`Pass 53.0`, `a3ba0f8`) — a map keyed by a derived
- 2026-08-10 (`269361d` correction + operator ruling) — the
- 2026-08-10 (`ef88973`, Pass 24.0's Enter-commit half) — Enter-to-commit
- 2026-08-10 (`45a88f2`) — a diagnostic-harness trace that would
- 2026-08-10 (`Pass 53.1`, `0c102e4` + `6611812`) — shared state
- 2026-08-10 (`2b41b77`) — a hover-scoped disclosure needs a named
- 2026-08-10 (`Pass 37.3` scoping, no code yet — `1e3422e` +
- 2026-08-10 (`252ffde`+`62ba5ac`, `Pass 37.3` slices 1–2) — the
- 2026-08-10 (`b1d7858`) — a cascade-deletion type must name what it
- 2026-08-10, decision 036 — the Reader-parity sweep: pdfce audited
- 2026-08-10 (seventy-sixth filing) — `Pass 10.0` (signature `/ByteRange` coverage, `annot::oc_refs`/`layers::group_refs` consolidation); TWO decisions CLAIMED as OWED (…
- 2026-08-10 (seventy-seventh filing, `ec8abfe`) — a reachability audit must EXCLUDE the observation/diagnostic harness's own driver function before searching, not merel…
- 2026-08-10 (seventy-eighth filing, `71592d3`) — §8.11.3.1 recorded as a load-bearing invariant: hidden optional content is not drawn, not not run; suppression is blit-…
- 2026-08-10 (seventy-eighth filing, `df874ca`) — a query string typed by the operator is matched literally by default; pattern-language reinterpretation (wildcards, reg…
- 2026-08-10 (seventy-ninth filing, `6ab72ec`) — `pdfce_render::LayerVisibility` REPLACES the document's default OCG configuration rather than merging with it; the opera…
- 2026-08-10 (eightieth filing, `2387a58`) — a theme module and its ONE hard boundary: chrome is themed, document colour never is; `Settings::theme` is a plain `String`,…
- 2026-08-10 (eightieth filing, `255cf86`→`3a699cf`→`fc137e2`) — `main.rs` split into three modules; two pre-existing defects surfaced by the move itself, neither a crat…
- 2026-08-10 (eighty-first filing, `e5c6870`) — an unevaluable `/VE` visibility expression falls back to `/OCGs`+`/P`; this is the behaviour §8.11.2.2 NOTE 2 designed, n…
- 2026-08-10 (eighty-second filing, `6171313`) — the §8.11.4.5 viewer/printer `shall not` is enforced by an `Option`'s default, not by caller discipline
- 2026-08-10 (eighty-second filing, `6171313`) — the §8.11.4.4 absent-usage-category rule is a DEFENDED DEFAULT (setting candidate `DA-A13`, deliberately not made a sett…
- 2026-08-10 (eighty-third filing, `21910fa`) — an auto-managed OCG's Layers-panel state is REPORTED AS `/D`-INITIAL AND SAID SO, not corrected to the viewer's current u…
- 2026-08-10 — Decision 009 CORRECTION (hollow-shall retraction) + `/CO` clause fix + Pass 7.2 (posture-B native recompute) ships
- 2026-08-10 (eighty-fifth filing) — the `pdfce-print` crate boundary: no dependency on `pdfce-render`, a platform-free `DeviceGeometry` input, shared by both shells so…
- 2026-08-10 (eighty-fifth filing) — content-stream colour spaces + PDF functions ship; spot colour renders the document's own tint transform, not a neutral stand-in
- 2026-08-11 — reset-form (§12.7.5.3): `/V` REMOVAL vs `/DV` assignment is chosen from the resolved, inherited default; three skip categories stay separate; a reset neve…
- 2026-08-11 (eighty-seventh filing, `ed6db1c`) — addendum to the 2026-08-10 `Pass 7.2` entry: a date/time GRAMMAR can be fully sourced while its PARSE is sourced nowher…
- 2026-08-11 (eighty-eighth filing) — Adobe's actual DRM product is server-mediated and pdfce's is not: a scope boundary this project already assumed, now stated once, o…
- 2026-08-11 (eighty-ninth filing, `5039ecf`) — an unencrypted §7.6.7 wrapper is detected on a marker the spec's OWN erratum record rules out using differently, and disc…
- 2026-08-11 (eighty-ninth filing, `f83be5a` design + `30c0940` wiring) — a field's four new authoring properties are represented as the spec's OWN refusal boundary, not…
- 2026-08-11 (ninetieth filing, `a64b5fd` design + `23eee9b` GUI wiring) — a CSV cell is not inert: exporting form data to a spreadsheet format is a second route to the…
- 2026-08-11 (ninetieth filing, `a64b5fd`) — form-data CSV is two columns (`name,value`), not one column per field; the wide, batch-across-documents shape is a distinct,…
- 2026-08-11 (ninety-first filing, `04f8acd`) — decision 037 ANSWERED BY MEASUREMENT: the literal "every OCG-shaped object" reading of `/BaseState /OFF` is falsified aga…
- 2026-08-11 (ninety-second filing, `ecf2302`) — decision 038 RECONCILED: Table 101 read whole is §8.11.4.5 b) with a redundant no-op prepended, not a second competing r…
- 2026-08-11 (ninety-second filing, `b3ba63b`) — encrypted-fixture provenance: `/R` 2–4 are real evidence against a from-spec implementation, `/R` 6 is a refusal-only fi…
- 2026-08-11 (ninety-sixth filing, `14a7400`) — `Pass 5` (Encryption) ACTIVATES: in-crate MD5/RC4 defers, not answers, the AES dependency question; `pdfce-core` gains it…
- 2026-08-11 (hundredth filing, `f7aee60`) — decision 039: `aes`/`cbc` accepted as `pdfce-core`'s first dependency where R24's own lever does not exist, the hardware bac…
- 2026-08-11 (hundred-and-second filing, `5d2b19b` + `483cb4d`) — decision 040: `print_render_options` is the single shared builder for print render policy; a second ind…
- 2026-08-11 (hundred-and-third filing) — decision 041: `DeviceGeometry::from_caps`/`for_orientation` REPLACE `From<&PrinterCaps>`; the un-rotated device view becomes un…
- 2026-08-11 (hundred-and-fourth filing, `4ddd6c4`) — decision 042: at most one confirmation dialog is ever pending, and that invariant — not the match order in `pending…
- 2026-08-11 (hundred-and-seventh filing, `f2ac2af`) — decision 043: a dependency-graph check proves absence of a class of dependency, never that the crate type-checks f…
- 2026-08-11 (hundred-and-tenth filing, `Pass 5` increment 3, commits `f79f044..f79d9a2`) — decision 044: pdfce REPORTS `/Perms` mismatch, never refuses on it and never…
- 2026-08-11 (hundred-and-tenth filing, `Pass 5` increment 3, commits `f79f044..f79d9a2`) — decision 045: a non-ASCII `/R` 5 password is ATTEMPTED, never refused, becaus…
- 2026-08-11 (hundred-and-thirteenth filing) — NOT a new decision: decision 043's dependency-graph-vs-buildability distinction gets its first CLEAN result against a targ…
- 2026-08-12 (hundred-and-seventeenth filing, `Pass 67.0` phase B, commits `f3acd24`+`2473602`+`d3baae5`+`f78c9d7`) — decision 046: the §9.6.4 subset tag is STRIPPED fro…
- 2026-08-12 (hundred-and-seventeenth filing, same commits) — decision 047: `/CIDSet` and `/CharSet` are removed together with the embedded program, because both describ…
- 2026-08-12 (hundred-and-twentieth filing, `Pass 67.0` phase E, commits `b358657..d8a8948`) — decision 048: a font dictionary's `/Subtype` may be RE-DECLARED from `/Typ…
- 2026-08-12 (hundred-and-twentieth filing, same commits) — decision 049: `/Encoding` is PINNED as a full `/Differences` array whenever Synthesise authors a dictionary t…
- 2026-08-12 (hundred-and-twentieth filing, same commits) — decision 050: embedding blocks on ANY shared `/FontDescriptor` — an asymmetry with `font_unembed.rs`'s own sh…
- 2026-08-12 (hundred-and-twentieth filing, same commits) — decision 051: the symbolic-font guard is against the MAPPING (§9.6.6.4 Branch B), not against symbolic fonts…
- 2026-08-12 (hundred-and-twentieth filing, same commits) — decision 052: composite/CID (`Identity-H`) fonts and Type 3 fonts are refused by name, for two different and…
- 2026-08-12 (hundred-and-twentieth filing, same commits) — decision 053: §9.9's embedding-permission paragraph (`fsType`) is enforced against every candidate donor, eve…
- 2026-08-12 (hundred-and-twenty-third filing, `9ea0c88`) — decision 054: `pdfce-gui` answers `--help`/`--version` itself, hand-parsed rather than via `clap`, and that s…
- 2026-08-12 (hundred-and-twenty-fifth filing, `74582ca` + `95c3416`) — no decision NUMBER minted; three rulings recorded against §4.1 (Q) instead, because each is a pro…
- 2026-08-12 (hundred-and-twenty-sixth filing, `fbcb946`) — decision 055: an optional capability is stripped by a Cargo feature whose OFF switch lives at the WORKSPACE R…
- 2026-08-13 (hundred-and-thirty-fourth filing, `d5431a4`) — decision 056: the ce-dimension STYLE cascade is THREE tiers with ONE `Option` per property, the exotic inher…
- 2026-08-13 (hundred-and-thirty-fifth filing, `c057682`) — decision 057: ce-dimension TOLERANCE is the TENTH and ELEVENTH properties of decision 056's cascade, seven of…
- 2026-08-13 (hundred-and-thirty-seventh filing) — decision 058: a shell crate THIS REPO OWNS may be replaced by an EXTERNALLY-DEVELOPED one, and §3's crate-boundary inv…
- 2026-08-13 (hundred-and-fortieth filing) — decision 059: THE COMMIT POINT IS SAVE, so an inference that lands in the open session has not "become document state" — inf…
- 2026-08-13 (hundred-and-forty-second filing, `2fe6216`) — decision 060: REGION RASTERISATION'S COST MODEL, and the three architectural positions it fixes — the guard b…
- 2026-08-13 (hundred-and-forty-fourth filing, CI change `197f0a5`) — decision 061: THE NO-NETWORK ABSOLUTE BECOMES A TWO-SCOPE RULE (engine ENFORCED, shells ALLOWED) —…
- 2026-08-13 (hundred-and-forty-fourth filing) — ADDENDUM to decision 061, NOT a separate decision.
- 2026-08-14 (hundred-and-forty-seventh filing) — decision 062: MARKUP AUTHORING HAS EXACTLY ONE ENTRY POINT.
- 2026-08-17 (hundred-and-forty-ninth filing) — decision 063: RENDER-SIDE SHADING IS SPLIT OUT OF DECISION 007'S EDIT-SIDE FOLD-IN.
- 2026-08-17 (hundred-and-forty-ninth filing) — decision 064: THE `iccce` BOUNDARY — pdfce owns COMPOSITING (overprint, blend modes, transparency groups, and what a PDF'…
- 2026-08-17 (hundred-and-fiftieth filing) — decision 065: AMB-3 RESOLVED — pdfce paints a radial shading's `s`-circles ON their circumference (`|P−c(s)|=r(s)`, ISO 3200…
- 2026-08-17 (hundred-and-fifty-seventh filing) — decision 066: PDFCE DOES NOT ROUTE A SPEC-GOVERNED COMPUTATION TO A DEPENDENCY WHOSE OUTPUT IT HAS NOT VERIFIED AGAINST…
- 2026-08-17 (hundred-and-fifty-ninth filing) — decision 067: THE CROSS-TARGET COMPILE-CHECK GATE'S OWN GUARANTEE IS NARROWED TO WHAT IT ACTUALLY TYPE-CHECKS — `pdfce-fe…
- 2026-08-17 (hundred-and-sixtieth filing) — decision 068: TRANSPARENCY GROUPS COMPOSITE INTO A PAGE-SIZED OFFSCREEN BUFFER, NOT A BBOX-SIZED ONE, AND THE CONTENTS' GRAP…
- 2026-08-18 (hundred-and-sixty-sixth filing) — decision 069: OVERPRINT IS SIMULATED PER-PAINT, BY RECONSTRUCTING CMYK FROM THE RGB BUFFER THROUGH AN EXACT-INVERSE ROUND…
- 2026-08-18 (hundred-and-sixty-seventh filing) — decision 070: A SOFT MASK IS MULTIPLIED INTO THE CLIP, NOT THREADED AS A SECOND MASK THROUGH EVERY PAINT SITE — AND THE…
- 2026-08-18 (hundred-and-eighty-second filing) — decision 071: A DISPLAY LIST IS KEYED ON `(page, epoch, SCALE)` AND REFUSES A MISMATCH BY NAME; A PAGE IT CANNOT RECORD…
- 2026-08-19 — Decision 072.
- 2026-08-19 — Decision 073.
- 2026-08-19/20 — Decision 074.
- 2026-08-20 — no decision NUMBER minted; two rulings recorded instead,
- 2026-08-20 — Decision 075.
- 2026-08-20 — Decision 076.
- 2026-08-21 — Decision 077.
- 2026-08-21 — Decision 078.
- 2026-08-21 — Decision 079.
- 2026-08-21 — Decision 080.
- 2026-08-22 — Decision 081.
- 2026-08-22 — Decision 082.
- 2026-08-23 — Decision 083.
- 2026-08-23 — Decision 084.
- 2026-08-24 — Decision 085.
- 2026-08-25 — Decision 086.
- 2026-08-25 — Decision 087.
- 2026-08-26 — Decision 088.
- 2026-08-26 — Decision 089.
- 2026-08-27 — Decision 090.
- 2026-08-27 — Decision 091.
- 2026-08-27 — Decision 092.
- 2026-08-27 — Decision 093.
- 2026-08-27 — Decision 094.
- 2026-08-28 — Decision 095.
- 2026-08-28 — Decision 096.
- 2026-08-29 — Decision 097.
- 2026-08-29 — Decision 098.
- 2026-08-29 — Decision 099.
- 2026-08-29 — Decision 100.
- 2026-08-29 — Decision 101.
- 2026-08-29 — Decision 102.
- 2026-08-29 — Decision 103.
- 2026-08-29 — decision `104`: `OverprintZeroTintScope::GreyAsKOnly` IS
- 2026-08-30 — Decision `105`.
- 2026-08-30 — Decision `106`: BOLD IS RESOLVED BY AN AUTOMATIC FALLBACK
- 2026-08-30 — Decision `107`: pdfce NOW AUTHORS DECLARED `/A` ACTIONS ON
- 2026-08-30 — Decision `108`: WHEN A SPEC AMBIGUITY'S TWO READINGS DIFFER
- 2026-08-30 — Decision `109`: pdfce REPAIRS A REFERENCE WHEN IT KNOWS THE
- 2026-08-30 (three-hundred-and-forty-ninth filing) — decision 110: WHEN A MALFORMED DOCUMENT MAKES A DESTRUCTIVE VERB AMBIGUOUS, pdfce REFUSES BY NAME — AND THAT IS *NO…
- 2026-08-31 (three-hundred-and-fifty-first filing) — decision 111: A PAGE INDEX MEANS THE PAGE AS THE *SESSION* HAS IT.
- 2026-08-31 (three-hundred-and-fifty-first filing) — decision 112: EDITING INSIDE A SHARED FORM XObject IS EDIT-IN-PLACE, DISCLOSED — FOR *GEOMETRY* AS WELL AS TEXT.

### 2026-09-01 (358th filing, `6e2b69e` + `28b982c`) — decision 113: **A MODEL-AGREEMENT DIGEST MUST FORCE THE WALK IT REPORTS ON, NOT READ WHATEVER THE MEMO HAPPENED TO HOLD — `page_content_generation` IS NOW `&mut self`, BREAKING**

**★ Sourcing.** No shell available to this role this filing (hard rule 8);
relayed from the engineer's dispatch, cross-checked against this
document's own `Pass 186.0`/`Pass 188.0` entries and §11.7 (which the
mechanism below is consistent with). Commit messages not independently
read via `git log`.

**Context.** `Pass 186.0` (decision 111) shipped `page_content_generation`
as a **read-only** digest of the decomposition memo's key so a consuming
shell could assert model agreement without decomposing twice. `Pass 188.0`
widened the KEY the memo tracks to include every form a page's walk
reaches (§11.7, `R237`) — but the accessor stayed `&self`, so it could
only hash whatever key value the memo already held from the last call
that happened to force a walk. A session that mutated a form XObject's
content **without otherwise triggering a redecomposition** left the
memo — and therefore the published digest — pointed at the pre-edit key.
**`pdfce-core`'s own internal staleness handling (the memo itself,
corrected by `Pass 188.0`) had become strictly stronger than the signal
it handed to a consumer.**

**Reported and diagnosed by the consuming shell**, not found internally —
it keys its own decomposition cache on this digest, so a stale digest
served it a stale `PageObjects` model. `PageObjects` addresses content by
**index**, so this is the silent-corruption shape: no error, no panic,
just an index resolved against the wrong generation of content.

**The fix.** `EditSession::page_content_generation` is now `&mut self`
and forces a fresh decomposition walk — computing the current key,
including any form the walk now reaches — before hashing it. **Breaking
signature change**, sabotage-verified against the consuming shell's own
reported before/after numbers, with a nothing-changed control run
alongside (an unrelated mutation on an unrelated page leaves the digest
unchanged, confirming the fix does not over-fire).

**Two documentation misses in the first commit, corrected by `28b982c`:**
the doc-comment PROSE was updated to the new signature, but
`docs/core-api/02-editing-and-saving.md`'s own VERB TABLE ROW — the index
a consumer reads first — still stated the old `&self` form; and a rustdoc
sentence reading *"it is literally the cache key"* was itself the defect
restated as a reassurance — accurate about the accessor, inaccurate about
the cache's freshness, and read as an argument that no further check was
needed. Both are `R93`'s shape (a cross-module doc claim, unverified
against the module it describes) — recorded as a further `R93` instance,
not a new rule (this project does not keep a formal instance count on
`R93`, per the ninety-fourth filing's ruling).

**★ Pass-ID collision, disclosed rather than silently fixed.** `6e2b69e`'s
own commit subject and doc comments name this **`Pass 196.0`** — a
collision with the ALREADY-FILED `Pass 196.0`/`Pass 196.1` (`4299174`,
357th filing, *Shipped*, this same day). Renumbered to **`Pass 197.0`**
in code, tests and docs by `28b982c`; **the pushed commit message cannot
be corrected** (project rule 8 forbids rewriting published history), so
`6e2b69e` is cited here **by hash**, never by the Pass number its own
subject line claims. `tools/check-passes-filed.py`'s collision detector
(keyed on the claimed ID text, not the hash) will report `Pass 196.0`
claimed by two commits as a `note` — informational by the tool's own
design, not a failure — and this entry is the reason a future reader
should not be surprised by that note.

### 2026-09-01 (359th filing, `a821393` + `9f1887e`) — decision 114: **RENDERING INTENT IS FOUR SEPARATE DEFAULTS (D1–D4), NOT ONE — AND THE CONTENT-STREAM `ri` OPERATOR GOVERNS PAINTING ONLY, NEVER THE PAGE-GROUP→DEVICE HOP (D4)**

**Status: DECIDED, PARTIALLY SHIPPED.** D1/D2 (page-start default,
unrecognised-name fallback) and D3 (image default) implemented and tested
(`Pass 199.0`/`Pass 199.1`, `ROADMAP.md`, *Shipped*). D4 (page-group→device
conversion) is a stated boundary, not yet consumed by any conversion code —
see `Pass 199.2`, *Backlog*.

**★ Sourcing.** No shell available to this role this filing (hard rule 8).
Relayed from the engineer's dispatch; cross-checked against live source
(`crates/pdfce-core/src/color/intent.rs`,
`crates/pdfce-render/src/interpret.rs:2800-2820`) via `Read`/`Grep`, not
`git log`.

**The finding.** pdfce parsed the `ri` operator (§8.6.5.8) and discarded
it — a recognised no-op. That is a conformance defect, not a quality gap:
Table 70's four intents "shall be recognized"; an unrecognised name "shall
use `RelativeColorimetric`" (§8.6.5.8); the intent used at paint time
"shall be the current rendering intent in effect in the graphics state"
(§11.7.5.3). The printed NOTE that reads as an escape hatch — "a
particular device does not have to support all PDF rendering intents" —
is **struck** by ISO-approved erratum `pdf-issues` #63 (closed
2021-04-16): NOTEs are informative only, the normative requirement to
support all four intents remains.

**Four distinct defaults, and merging them is the failure mode this
decision exists to prevent:**

| | question | answer | clause |
|---|---|---|---|
| D1 | intent at page start | `RelativeColorimetric` | Table 52 initial value, §8.4.1 `shall` |
| D2 | unrecognised name | `RelativeColorimetric` | §8.6.5.8 `shall` |
| D3 | image with no `/Intent` | **the graphics state's current intent**, not a constant | Table 89 default value |
| D4 | page-group→device conversion | `RelativeColorimetric` (ISO 32000-2 §11.4.7 `shall`) | — |

**D4 is the one that would have been got wrong.** A content-stream `ri
/Saturation` does not govern the page-group's own conversion to the
device — that is a separate step with its own answer (§11.4.7), and
applying a source-side painting intent to a destination-side conversion is
a category error, not a refinement. This is the load-bearing half of the
decision: it forecloses the obvious-looking wrong fix before any
conversion code is written, rather than after.

**A fifth rule, not a default: `gs` does not reset the intent.** §8.4.5:
`ExtGState` results "shall be cumulative" and persist until explicitly
overridden. An `/ExtGState` dict with no `/RI` key must leave the
graphics-state intent alone. ISO 32000-2's Table 57 printed "The default
value is: Default" for this entry — the only entry in that table to claim
one — and ISO-approved erratum `pdf-issues` #360 **deletes** that line for
exactly that reason; re-raised as #746 in 2026 and closed as a duplicate.
A live implementer trap, not a historical curiosity.

**A measured ink-error ranking is not evidence that an intent is
correct.** `Saturation` and `Perceptual` carry no output metric in either
standard — ISO 32000-1 §10.2 puts gamut mapping in the reader's own
implementation; ISO 32000-2 §10.3.1 defers to ICC.1:2010, whose clause 0.4
states perceptual/saturation rendering "is vendor specific." So the fix
carries the file's own declared intent faithfully to whatever converts
colour; it never hard-codes an intent because a fixture happens to score
well under it.

**Extends decision 064's `iccce` boundary, does not revise it.** pdfce
owns what a colour component *means* and now owns carrying the document's
declared rendering intent through the graphics state (D1–D3, this
decision); `iccce` will own consuming that intent inside a real conversion
(D4 and the terminal CMYK path) once `Pass 199.2` lands. Forward pointer
added to decision 064, above.

**No standing rule minted** — this is a spec-interpretation/invariant
decision, not a finding about pdfce's own tooling or gates.

**★ No dedicated `ARCHITECTURE.md` body section exists for colour
management** (unlike decisions 111–113 above, which pair with §11.7
because undo/redo's overlay is genuinely that section's topic — rendering
intent is not). This role's tertiary duty ("both the decision log and the
body section change together") is discharged here as a **flagged gap**
rather than by inventing a mismatched section: the body of record for this
invariant is `pdfce_core::color::intent`'s own module doc plus this entry
and `Pass 199.0`/`199.1`'s `ROADMAP.md` entries. Whether a dedicated
colour-management section belongs in `ARCHITECTURE.md` is the engineer's
call, not decided speculatively here.

**GUI-core separation:** not independently re-verified by this role — no
shell available this filing (hard rule 8); relayed as the engineer's
measurement.

**Body-section counterpart:** §11.7, "The model-agreement query"
paragraph, amended in place in this filing.

**Decision ceiling moves `112` → `113`; next free `114`.** No standing
rule minted; `R93` gains a further cited instance (text in `ROADMAP.md`'s
*Shipped*, `Pass 197.0` entry).

### 2026-09-01 (360th filing, `3194f1b`) — decision 115: **`iccce` ENTERS AS A GIT DEPENDENCY PINNED TO TAG `v0.3.0` — NOT A PATH DEPENDENCY, NOT A VENDORED COPY. pdfce NOW HAS AN ICC COLOUR-MANAGEMENT ENGINE FOR THE FIRST TIME, AND IT IS `ICCBased`-ONLY BY REFUSAL**

**Status: DECIDED and SHIPPED** (`Pass 199.2`, `ROADMAP.md` *Shipped*).
**Extends decision `064`** (which set the boundary — pdfce owns compositing
and meaning, `iccce` owns conversion — and recorded both consumers as NOT
STARTED). **This is the integration half of `064`. `064` is not superseded;
its boundary is unchanged, and it is what made this a dependency question
rather than a design question.**

**★ Sourcing.** A shell WAS available to this role this filing (hard rule 8).
The commit message was read with `git log -1 --format=%B 3194f1b`; the
dependency declaration was read live from
`crates/pdfce-render/Cargo.toml:57-80`; `iccce_provenance()`'s behaviour was
read live from `crates/pdfce-core/build.rs:225-251`.

#### The decision

`crates/pdfce-render/Cargo.toml` gains **two** crates from the sibling
project — `iccce-profile` and `iccce-cmm`, both declared as
`{ git = "https://github.com/KenM76/iccce.git", tag = "v0.3.0" }`.

**In `pdfce-render` only** — `pdfce-core` does not depend on `iccce`, which
keeps the object model free of a colour engine, and which is also the reason
the version banner did not self-update (see below).

★ **AMENDED-IN-PLACE 2026-09-02 (`Pass 242.0`, `48f8fbb`) — this count is
now stale but was correct when written.** A third crate, `iccce-color`
(the PCS value types `iccce-cmm`'s entry points take), is now also
declared directly, for the same reasons and the same terms as the two
above — see §9's own dated note, added the same edit. Not a re-opening
of this decision: the boundary (`pdfce-render` only), the pin form
(now `rev`, per decision 123) and the licence terms are all unchanged.

#### Why a git dependency, and why pinned — the engineer's reasoning, recorded in full

The 359th filing recorded three candidate forms and said the choice was the
operator's: **publish `iccce` to crates.io**, **vendor its source into
pdfce's tree**, or **a git dependency**. It also verified that the third was
*"probably better"*. The engineer took the third, on two grounds:

1. **A PATH dependency would not resolve for anyone cloning the public pdfce
   repo.** `iccce` lives beside pdfce on the author's machine at
   `D:\dev\iccce`, so `path = "../../../iccce"` builds **only there**. The
   repository is public (`LEGAL.md` §1.1), so a dependency form that works on
   one machine is a broken build for every other reader — the same class of
   error as a document asserting an unmeasured fact about the environment.
2. **PINNING TO A TAG KEEPS COLOUR OUTPUT REPRODUCIBLE.** An unpinned git
   dependency would let a sibling-project commit change what pdfce renders,
   silently, between two builds of the same pdfce commit. **Colour output is
   exactly the kind of result where a silent change stays invisible until a
   conformance figure moves and nobody can say why.** A tag makes the colour
   engine's identity part of pdfce's own revision.

**REVERSIBLE, and named as such.** If the operator prefers publication to
crates.io — for third-party consumers of `pdfce-render`, or for `Cargo.lock`
semantics a git dependency does not provide — this decision is the one to
amend. **A vendored copy remains rejected** for the maintenance reason the
359th filing already gave: a fork's burden with none of a fork's purpose.

#### Why it is admissible at all — the three gates it had to clear

| gate | result |
|---|---|
| **rule 13 / `LEGAL.md` §6** — licence classification before adoption | **MIT**, verified against `iccce`'s own `Cargo.toml` at adoption time (2026-09-01), not relayed from an earlier reading. Permissive; no operator licence call needed. `THIRD_PARTY_LICENSES.md` **regenerated via `cargo-about`**, never hand-edited. |
| **rule 2 / §3** — GUI-core separation | `cargo tree -p pdfce-core` and `-p pdfce-render` clean: **no GUI dependency.** |
| **§1.1 / wasm32 web-fork target** | **`iccce` has ZERO external dependencies and sets `unsafe_code = "deny"`.** That single fact clears **three** obligations at once: the **no-network** gate (nothing to fetch at runtime), the **wasm32** target (nothing that cannot cross), and **`R24`'s zero-`unsafe` posture** (compiler-enforced inside the dependency rather than negotiated via `default-features = false`). ★ Contrast `aes`/`sha2` above, whose cfg-selected hardware backends could **not** be forced off and required decision `039`'s named exception. **`iccce` needs no exception**, and that is the strongest single argument for it over any third-party ICC crate. |

#### What is managed, and what is REFUSED

**Managed:** an **`ICCBased`** paint on a page that composites in ink. The
document's own embedded source profile plus the catalog's `/OutputIntents` →
`/DestOutputProfile` destination, converted at the intent the graphics state
asked for (decision `114`'s D1–D3 supply that intent; **D4 remains a stated
boundary**).

**REFUSED, not omitted — `DeviceRGB` is NOT managed.** `iccce` exposes a
built-in sRGB as a **DESTINATION** only, and **pdfce is not entitled to
invent a source characterisation the document never made.** ⇒ Only the case
where **the file did the work of saying what its numbers mean** is
colour-managed. This is the same posture as decision `114`'s refusal to
hard-code an intent because a fixture scores well under it, and it should be
read as the standing shape of pdfce's colour policy rather than as this
Pass's local scope.

**★ CORRECTED 2026-09-02 (`Pass 240.0`, `f978291`, 384th filing, this role's
own hard-rule-11 sweep — not a new architectural decision).** This paragraph
is about `DeviceRGB` (a device colour space with no source characterisation
the document supplies) and remains true of it. But `ICCBased /N 3` is a
DIFFERENT case this paragraph had been read as covering too — an
`ICCBased` RGB space DOES carry a source characterisation (its own embedded
profile), so "pdfce is not entitled to invent a source characterisation the
document never made" never applied to it. `Pass 240.0` extended management
to `ICCBased /N 3` on every route (fills, strokes, text, images, both page
kinds); only `DeviceRGB` and `ICCBased /N 1` (Gray) remain refused. See
`ROADMAP.md` `Pass 240.0` (*Shipped*) for the mechanism and the retraction
of a separate measured-negative claim this same boundary had produced.

**Also deliberately unchanged: `overprint::rgb_to_cmyk` stays on the
round-trip path.** It is an **invertible** max-GCR formula that exists so
`snapshot_srgb_backdrop` and `composite_srgb` return where they started.
Substituting an accurate, non-invertible transform there would make the
return leg drift.

★★ **The defect this Pass fixed was that same function used as a TERMINAL
conversion — a correct function in the wrong job, producing numbers that look
exactly like an arithmetic defect** (three blend-arithmetic hypotheses were
raised and ablated away before the real cause was found). **The generalizable
half of decision 115: an INVERTIBLE transform and an ACCURATE transform are
different objects with opposite requirements, and a codebase that composites
in one space and converts to another needs BOTH, named as such.** Naming them
is the cheap defence; the expensive one is three ablations against innocent
arithmetic.

#### Disclosure obligation this decision creates (rule 4)

Colour management is **invisible by construction** — nothing on the page is
drawn differently, and a managed paint and an approximated one are
indistinguishable to look at. The decision therefore ships with **a PAIR of
counters**, `icc_managed_paints` and `icc_unmanaged_paints`, on
`render-page`'s stable metrics line and JSON output. **The pair is part of
the decision, not an implementation detail:** `managed = 0` alone cannot
distinguish *"the engine ran and agreed with the fallback"* from *"the branch
was never reached"*, and a counter whose zero has two readings is not a
disclosure.

#### Body-section counterpart

**§9 (Open-source dependencies & attribution)** gains its **fifth "Nth
dependency" paragraph** in this same filing, so the decision log and the
living body section move together (this role's tertiary duty).

★ **A body-section gap is flagged rather than invented, exactly as decision
`114` flagged it:** there is still **no dedicated colour-management section**
in this document. `114` recorded that gap for rendering intent; `115` makes
it larger, because there is now an **engine**, a **boundary**, a **refusal**
and a **disclosure pair** whose body of record is spread across
`pdfce_core::color::intent`'s module doc, `crates/pdfce-render/src/icc.rs`'s
module doc, and several `ROADMAP.md` entries. **Whether to create a §13
"Colour management" is the engineer's call**; this role does not invent a
section, and records that this is the **second consecutive filing** to want
one.

#### One consequence NOT swept, and it is a false claim in shipped output

**`pdfce-cli --version` still reports `iccce: not-linked-yet (integration
pending -- Pass 97.x; see ARCHITECTURE.md decision 064)`.** Verified live:
`crates/pdfce-core/build.rs:250` returns that literal, because it detects the
dependency via `DEP_ICCCE_PROVENANCE` — an env var Cargo sets only for a
dependency declaring a `links` key, which `iccce` does not — **and because
the build script lives in `pdfce-core`, which does not depend on `iccce` at
all.** The function's own doc comment promised *"this begins reporting the
moment that becomes true."* **The moment came and it did not begin
reporting.** Filed as owed work in `ROADMAP.md`'s *Backlog*; recorded here
because it is a direct consequence of **where** this decision put the
dependency, which is the kind of consequence a decision log exists to carry.

**Decision ceiling moves `114` → `115`; next free `116`.** **No standing rule
minted** — `R93` gains a fifth cited instance from a sibling commit in the
same filing (`ROADMAP.md` *Standing rules*), and two rule candidates were
assessed and declined (*Standing-rule disposition, 360th filing*). **Standing
rules ceiling `R239` — UNCHANGED.**

---

### 2026-09-01 (362nd filing, `6ed5b9b`) — decision 116: **A COLORANT'S IDENTITY IS ITS DECODED *BYTES*, NOT A LOSSY-DECODED `String` — AND THE PROJECT KEEPS `from_utf8_lossy` FOR *DISPLAY* DELIBERATELY, IN THE SAME MODULE**

**The decision.** `Colorant::Named` carries **`Box<[u8]>`**, not `String`.
`Colorant::parse` stores the lexer's already-`#xx`-decoded bytes **verbatim**.
Comparison, hashing and any future name-keyed map operate on those bytes.

**Why it is a decision and not a fix.** `String::from_utf8_lossy` maps **every
distinct invalid byte sequence onto the same `U+FFFD`**, so two documents
naming **two different colorants** produced the **same `Colorant`**, comparing
**equal**. The standard forecloses the alternative in three places:
**§8.6.6.4** makes the device test consult **only the name** (the alternate
space and tint transform are the *fallback when that test fails*, so they are
not identity); **§7.3.5 NOTE 4** makes names differing **in bytes** distinct
names *even if they render identically*, and specifies **no case folding and
no Unicode normalisation** anywhere; and UTF-8 is a **should** for *display*,
not a rule for *equality*.

**★★ THE SPLIT THIS DECISION EXISTS TO PROTECT, because it looks like an
inconsistency and is not.** **Lossy decoding is correct for showing a name to
an operator and never correct for deciding whether two names are the same.**
`crates/pdfce-render/src/color.rs` therefore **still calls `from_utf8_lossy`
on its diagnostic paths, on purpose** (verified live 2026-09-01 at
`color.rs:1420` and `color.rs:1543`, with the rationale recorded at
`color.rs:184` and `color.rs:201`). **A later sweep that "unifies" those two
call sites with the identity path re-introduces the defect.** Recorded here so
the rationale outlives the doc comment.

**★ ASCII case-insensitivity in `process_channel` is a pdfce CHOICE, now
labelled as one.** ISO 32000 defines **no** case folding for colorant names
(corpus note `SEP-A1`). It is kept, and being ASCII-only it **cannot fold two
distinct non-ASCII names together** — so the choice does not re-create the
collision class this decision removes.

**Timing, and it is the reusable half.** Fixed **while still harmless**:
nothing currently keys on a colorant name, so today a collision changes no
pixel. It stops being harmless **the moment the per-spot-colorant plane
lands**, because a plane is a **map from name to plate** and two colliding
names would silently composite as one colour — **a wrong picture with no
error, no counter and no visible symptom.** ⇒ **A latent correctness bug with
no consumer cannot be detected by any test until the consumer exists, at which
point it is a REGRESSION rather than a known debt.** Fix it *before* the
consumer, not with it.

**Left open, deliberately, for the plane to decide:** `Box<[u8]>` vs
`Arc<[u8]>`. `ROADMAP.md`'s *Backlog* entry recommended `Arc` on the reasoning
*"the key is cloned per paint and never mutated"*; **`Box` does not share on
clone**, so that per-paint clone is a copy. There is **no per-paint clone
today**, so `Box` is correct now and the question belongs with the roster that
will actually perform the allocation. **Cheap to change while the variant has
one constructor.**

**★ AMENDED 2026-09-01 (decision 118, `Pass 225.0`) — no roster was built.**
Planes are allocated lazily at first use, not from a pre-pass roster; the
"the roster that will actually perform the allocation" clause above is kept
for its history but no longer names a component that exists. The `Box` vs
`Arc` question now belongs with `CmykBuffer::spot_index`
(`cmyk_buffer.rs:1086`), which performs the allocation instead.

**Consequence recorded because a decision log exists to carry it:** the
signature change **acted as a duplicate detector** — a **verbatim inline copy**
of `process_channel` in `authored_tints` (four arms, the same four names)
**had already drifted**, one taking `&str` and the other bytes, and was found
only because the original's signature moved. **A refactor that touches a
signature is the cheapest duplicate-code sweep this project has**, and it runs
only when someone changes a signature.

**Decision ceiling moves `115` → `116`.** **Standing rules ceiling `R239` —
UNCHANGED.**

---

### 2026-09-01 (362nd filing, `77f95b5`) — decision 117: **`coalesce_last` IS PUBLIC, AND THE GENERAL PRIMITIVE WAS CHOSEN OVER THE NARROW CONVENIENCE VERB THAT WAS OFFERED — PLUS: A FOUR-STATE ACTION READER, BECAUSE A THREE-STATE ONE CANNOT STAY HONEST AS COVERAGE GROWS**

**Supersedes the visibility half of decision `101`** (`coalesce_last`,
`Pass 168.0`, 2026-08-29), which recorded the primitive as **private**. That
entry stays as filed; **body counterpart is §11.6**, updated in this filing.
`ROADMAP.md`'s `Pass 168.0` entry carries a dated forward pointer for the same
reason.

**Decision A — `EditSession::coalesce_last` becomes `pub`
(`crates/pdfce-core/src/edit.rs:12926`).** The crate boundary was drawn one
notch too tight. **Placing a push button *with* an action is one operator
gesture calling two verbs**, so it left **two** entries on the undo stack:
`Ctrl+Z` removed the action and left an **inert button** on the page.
`cut_field` already composes exactly this way internally (`copy_field` +
`delete_field` + `coalesce_last`) and its own doc block makes the shell's
argument verbatim — *"two commands is two undos for one gesture"*. **So the
composition was never in question; only whether it may be spelled outside this
crate.**

**★★ Decision A′ — the narrower `add_push_button_with_action` was OFFERED and
DECLINED, on the requesting shell's own reasoning: it fixes ONE INSTANCE OF A
SHAPE THAT RECURS.** Every future gesture needing two verbs would need its own
convenience verb, each with its own refusal surface, its own disclosure and its
own tests — and each one is a second implementation of a composition the crate
already performs correctly. **Exporting the primitive costs one visibility
keyword and a contract; exporting N convenience verbs costs N maintained
surfaces that can disagree with each other.** ⇒ **Prefer the primitive when
the narrow verb is an instance of it.**

**The public contract adds three obligations the internal caller never had**
(§11.6 carries them in full): **check the `bool` return** — `false` means
applied-but-not-grouped, which is a **disclosure**, not a retry; **`count`
counts the caller's own commands**, and overcounting **folds a neighbour's edit
in with nothing guarding it**; **fold immediately**, before anything else can
push.

**Decision B — `EditSession::button_action` answers with FOUR states, not the
three that were requested.** `ButtonActionState` (`edit.rs:14715`,
`#[non_exhaustive]`): `None` / `Known(ButtonAction)` / **`Unmodelled(String)`**
/ `Foreign(String)`.

**★★★ The proposed three-state shape cannot stay honest as `Known` coverage
grows, and that is the whole argument.** `Foreign`'s contract is *"an action
pdfce recognises and **will not author**"*. A `/SubmitForm` this reader does
not yet decode is **not that** — **pdfce authors `/SubmitForm` happily** — so
returning `Foreign("SubmitForm")` would tell a shell *"pdfce will not touch
this"* about an action pdfce writes on request, and the shell would correctly
grey a control that should have been offered. ⇒ **`Unmodelled` and `Foreign`
differ in exactly one thing: whether REPLACING is offered**, which is the
decision the operator is being asked to make, so it is the distinction the enum
must carry.

**The general form, recorded because it will recur:** *a three-way enum that
folds "we cannot read it **yet**" together with "we will not write it
**ever**" is a statement about the READER wearing a statement about the
WRITER's clothes — and it decays every time the reader improves.* The reader's
coverage is a moving fact; the writer's refusal list is a policy. **They do not
belong in one variant.**

**Decision B′ — the reader answers for the field's FIRST widget, and SAYS SO
rather than reconciling.** §12.7.3.1 lets one field own widgets on several
pages, and **nothing requires their `/A` entries to agree**. pdfce therefore
**picks**, and discloses that it picked. **A chosen answer presented as *the*
answer is precisely the failure this verb was created to remove** on the
display side, so re-committing it inside the verb would be self-defeating.
**A per-widget reader (`Widget::action`) is deliberately NOT built** pending a
real document whose one field carries widgets with differing `/A` — filed under
*Backlog* with that trigger. **The mirror of `R151`: not a capability with no
caller, but a capability with no INPUT** — and the worse failure of the two,
because an unused API can be deleted while a wrongly-shaped one has consumers.

**Refusal symmetry, recorded as a contract rather than an implementation
detail:** reading is refused on **the same footing as writing** — a
non-push-button is `ButtonActionWrongFieldType` — so **a shell cannot learn
through the reader about a field it would be refused permission to change.**

**Test-design consequence worth carrying:** the `Foreign` fixture is
**hand-authored**, because *a fixture built with the writer could never contain
an action pdfce refuses to write.* **A test for a refusal cannot be built by
the thing that refuses.** And the sabotage is precise about its blind spot:
collapsing `Foreign` into `Unmodelled` fails the JavaScript test **and nothing
else**, while the round-trip test **correctly stays green** — a green
round-trip test beside a broken enum is exactly the reassurance that lets a
collapse ship.

**Decision ceiling moves `116` → `117`; next free `118`.** **Standing rules
ceiling `R239` — UNCHANGED**, next free `R240`; no rule minted this filing.

---

### 2026-09-01 (366th filing, `16eaaa2`) — decision 118: **A SPOT-COLORANT PLANE IS ALLOCATED LAZILY, AT FIRST USE — NOT FROM A PRE-PASS ROSTER. THIS IS A CORRECTNESS ARGUMENT, NOT A PERFORMANCE ONE, AND IT DELETES A PLANNED SUBSYSTEM BEFORE IT WAS BUILT**

**Status: DECIDED.** `Pass 225.0` (`16eaaa2`, step 2 of ~4 of the
spot-colorant plane: storage, blending and collapse, landed still PROVED
inert — conformance sweep byte-identical, 7 FAIL / 37 pass / 7 UNRESOLVED of
51 before and after) ships `CmykBuffer::spot_index`
(`crates/pdfce-render/src/cmyk_buffer.rs:1086`) as the sole allocator of a
`SpotPlane`. It finds-or-creates a plane on first use, bounded by
`compositor::MAX_SPOTS` (4) and the buffer's own byte ceiling, refusing —
counted via `spots_flattened`, never silent — when neither can be
satisfied.

**Supersedes a scoped plan, not merely extends one.** `Pass 217.0`'s own
scoping study (361st filing, `ROADMAP.md` *Backlog*) and the handoff that
carried it forward (`docs/NEXT_SESSION.md` §0, as it stood before this
Pass) both described this step as *"roster + ONE plane"*: a resource
pre-pass over a page's content stream (and its forms, patterns, annotation
appearance streams and Type 3 glyph procedures) that would enumerate every
spot colorant a page names, before any paint ran, so a plane could be
provisioned up front. **No roster and no pre-pass exist in the shipped
code.** This is not an implementation detail arrived at while building the
scoped design — it is a different design, decided instead of the scoped
one, and recorded here because it reverses a plan a prior filing already
committed to paper.

**The argument for lazy allocation is correctness, not speed — stated
because the obvious reading of "skip the pre-pass" is a performance
trade, and that reading is wrong.** A plane created part-way through a
page's content stream is all zeros for every pixel painted before that
point, and **zero is the exactly correct value**: "no ink of this
colorant" is true, by construction, of every mark laid down before the
document first named the colorant a plane now exists for. There is
nothing to back-fill, and therefore nothing a pre-pass would have bought
except earlier knowledge of a fact lazy allocation never needs.

**What the pre-pass would have cost, and it is a real hazard, not a
hypothetical one.** To be complete, a resource pre-pass has to recurse into
every place a colour space can be named: form XObjects (nested arbitrarily
deep), tiling and shading patterns, annotation appearance streams, and
Type 3 glyph procedures. **Any colorant such a walk MISSED would be
flattened silently** — the plane would simply never exist for it, with no
counter, no refusal, no visible symptom — because a roster is only
checkable against the render it was built to serve; there is no
independent oracle for "did the pre-pass see everything." Lazy allocation
has no such blind spot **by construction**: a colorant gets a plane exactly
when a paint operator asks for one, so there is no enumeration step that
could be incomplete, and therefore no population the allocator could have
missed.

**Consequence for decision 116.** Decision 116 (colorant identity as
`Box<[u8]>`) deferred its `Box` vs `Arc` question to *"the roster that will
actually perform the allocation."* That roster does not exist; the
question now belongs to `spot_index` itself, which performs the allocation
instead. Amended in place at decision 116, above, with a forward pointer
to this entry — decision 116's own analysis is otherwise unaffected, since
`spot_index` still performs at most one allocation per colorant per page
regardless of how it discovers that colorant.

**Two spec findings landed the same commit, recorded here because they
bear on `fold_spots_srgb`'s design, not because they are architectural
decisions of their own.** `CmykBuffer::fold_spots_srgb`
(`cmyk_buffer.rs:2443`) implements ISO 32000-2 §10.8.3 "Separation
simulation" step (c) — a corpus finding, not an invention: ISO 32000-1
never describes overprint/separation preview (0 hits, negative result,
still true of the 2008 text), while ISO 32000-2:2020 defines a four-step
algorithm, names the capability `SeparationSimulation` (Table 275) and
NOTE 5's it as "Overprint Preview." Two deviations from the clause are
disclosed in the code rather than buried — the multiply runs in sRGB
rather than the clause's undefined "flat XYZ (no gamma)," and the
per-separation ink→colour map is the tint transform rather than
colorimetry the clause declines to specify — and neither is a conformance
failure, because **§10.8 contains no `shall` at all**: the clause binds the
result the algorithm must produce, not the method used to produce it.

**Two sabotage findings, filed as further dated instances of `R225`
(`ROADMAP.md` *Standing rules*: "a sabotage is only as discriminating as
its fixture"), not as a new rule** — checked against the existing family
before filing, per this project's own two-occurrence-then-consolidate
discipline. First: the §11.7.4.2 non-separable-blend-mode guard in
`compositor::blend_spots` is redundant with `blend_separable`'s own final
arm, which already answers `cs` for any non-separable mode; kept for
legibility, both sites now say so. Second, the one worth generalising: the
zero-tint early-out in `fold_spots_srgb` **was** load-bearing, and the
original test could not see it, because its LUT happened to return white
at tint 0 — the same coincident-oracle shape `R225` already names, newly
instanced against a document-supplied tint-transform function rather than
against a fixture's baked geometry. Fixed with a deliberately malformed
LUT returning solid red at zero.

**Decision ceiling moves `117` → `118`; next free `119`.** **Standing rules
ceiling `R239` — UNCHANGED**, next free `R240`; no rule minted this filing
(`R225` gains two further dated instances, recorded in `ROADMAP.md`'s
`Pass 225.0` entry rather than here).

### 2026-09-02 (372nd filing) — decision 119: **TABLE 148/149 (1.7) / TABLE 146 (2.0) ARE DETERMINATE FOR "ANY PROCESS COLOUR SPACE × SPOT COLORANT": `OP true` MEANS PRESERVE, IN BOTH OVERPRINT-MODE COLUMNS, IN BOTH EDITIONS — pdfce'S RENDER IS THE CONFORMING ONE, AND A DIVERGENCE FROM AN ACROBAT REFERENCE RENDER ON THIS CELL IS NOT EVIDENCE OF A pdfce DEFECT**

> **★★ SUPERSEDED IN PART, SAME DAY — READ DECISION 120 BELOW FIRST.**
> `pdfce-spec-librarian` re-adjudicated with new evidence (a re-dispatch that
> tested this decision's own §E hypotheses) and found a **fourth mechanism**,
> `iso32000__s__8.6.7.md` UPDATE 2026-09-02 (SECOND FILING) §H–§P. Every
> quotation and table cell below is still correct, and the **outcome does
> not change** (pdfce does not change its render; `Pass 97.x` is not
> retracted). What changes: this decision's claim that pdfce's render is
> **"the conforming one"** — implying Acrobat's is not — is **over-read**.
> **Both renders conform**, on two different device-colorant-set models;
> see the struck sentences below and decision 120 for the corrected
> framing, the new `OP-A7` setting, and the harness consequence.

**Context.** A print-conformance patch cell: a spot-colorant backdrop
(`/Separation` over `/DeviceCMYK`), a white `DeviceGray` object (`1 g`, i.e.
CMYK `0 0 0 0`) painted over it with `/OP true`. **Acrobat's reference render
shows pure white (spot knocked out); pdfce renders the spot green,
`(142,198,63)` (spot preserved).** This was dispatched to
`pdfce-spec-librarian` on the working premise that pdfce's render was the
defect. **It came back against that premise.**

**The verdict (`iso32000__s__8.6.7.md` UPDATE 2026-09-02, §A).** ~~There is no
reading of either edition under which `OP true` erases an unnamed spot
colorant on a process-space source.~~ **★ CORRECTED by the second filing
(§H–§K): this is true only of a device that HAS the spot colorant. A device
that does not is routed to an alternate colour space by §8.6.6.4's own
`shall`, at colour-space-set time, before this cell is ever consulted — see
decision 120.** Six independent supports converge on the same cell (§B),
**given that premise**: §8.6.7's own prose (*"anything previously painted in other
colorants is left undisturbed"*), §11.7.3 (*"every object paints every
existing colour component, both process and spot… unspecified components take
an additive value of 1.0"* — the `DeviceCMYK`/`ICCBased`-source case is the
clause's own worked example), Table 148 (1.7 only — 2.0 deletes the table,
not the rule), Table 149 (1.7) / Table 146 (2.0) — verified **verbatim**
against both source PDFs, character-identical on this row — §11.7.4.5 NOTE 1
(*"there is no difference in the treatment of spot colour components"*
between the opaque and transparent imaging models), and the fact that `OPM 0`
and `OPM 1` are **identical** on this row (the zero-tint carve-out that
distinguishes them exists only in row 1's `DeviceCMYK`-direct cell). **No
edition delta.** ⇒ ~~**pdfce's green is the conforming render.**~~ **★
CORRECTED: pdfce's green is A conforming render** — the one this decision's
title calls out, reached via ISO 32000-2 §10.8.3's optional
separation-simulation branch. **Acrobat's white is also a conforming
render**, reached via §8.6.6.4's mandatory alternate-space-substitution
branch, on a device without the named spot colorant. See decision 120.

**The internal paraphrase pdfce carries — *"a colorant not named in the
source colour space is left to the backdrop under `OP true`"* — reaches the
correct cell by a WRONG ROUTE (§C).** That phrasing belongs to the
`Separation`/`DeviceN` rows only, where "named in source space" separates the
one spot the space names from every other spot. On a **process**-source row
the test is unconditional — §11.7.3 says a `DeviceGray`/`DeviceCMYK`/etc.
source paints **every** spot colorant, with subtractive tint 0.0, and
overprint decides only whether that 0.0 is written (`OP false`) or replaced
by the backdrop (`OP true`). There is no "cannot address" category the
paraphrase implies. **Action: fix the comment in `crates/pdfce-render/src/`
that carries this paraphrase (at least one doc comment and one test message
use it); the code itself is correct and untouched by this decision.**

**Why Acrobat shows white anyway — two spec-sanctioned routes, neither of
which touches the overprint rule (§E).** The most likely: **Acrobat is not
simulating overprint for this file at all.** §8.6.7 (both editions):
*"If overprinting is not supported, the value of the overprint parameter
shall be ignored"* — ignoring `/OP` puts the cell in the **`OP false`**
column (*paint 0.0* / `c_s`), which erases the spot and yields exactly the
measured white. **Acrobat's `Use Overprint Preview` preference (Page
Display) defaults to `Only for PDF/X files`**; a patch not recognised as
PDF/X renders with overprint off by default. Falsifiable and not yet
falsified: set the preference to `Always` and re-render the same cell — if it
turns green, the route is confirmed and no pdfce code needs to change. A
second, independently-sourced route (§E `R2`) — the white object being a
`/Group`, or under a non-`Normal` blend mode, or a combined fill+stroke —
would also reach white by Table 149's bottom row (*"a group (not an
elementary object)"* × *"all colour components"* = `c_s` in **all three**
columns, `OP true` included) and needs checking independently of `R1`.

**Explicit, and binding on any future session that revisits this cell: do
NOT "fix" pdfce to match the white.** Doing so would contradict five sourced
1.7 provisions (four in 2.0) and would break the `Separation`/`DeviceN` rows
through the same code path — the same shape of scope error already corrected
once in `OP-A5` (2026-08-31). ~~If `R1` is confirmed, the correct downstream
action is regenerating the Acrobat reference renders with Overprint Preview
forced on, not editing `overprint.rs`.~~ **★ CORRECTED — see decision 120:
`R1`'s own experiment came back INCONCLUSIVE (a positive control that did
not move, and a region-count metric structurally blind to hole-filling),
and it turned out not to matter — `R4` (decision 120) explains Acrobat's
white regardless of whether overprint preview is on or off. This sentence's
conclusion (do not edit `overprint.rs`) still holds; its stated reason does
not.**

**Consequence for the conformance ledger.** ~~The two remaining traps on
`PCS 3.0`/`PCS 4.0` (`ROADMAP.md`, 371st filing) were being treated as
confirmed pdfce defects. At least the spot-preservation half of that reading
is now suspect — an operator action (re-generate the Acrobat oracle with
`Use Overprint Preview` = `Always`) is owed before either trap is diagnosed
further as a pdfce-side bug.~~ **★ CORRECTED — see decision 120: the
spot-preservation half of the `PCS 3.0`/`PCS 4.0` reading is not merely
suspect, it is settled — neither render is a defect, the divergence is a
device-colorant-set model, and re-generating the Acrobat oracle at a
different preference setting is no longer a diagnostic prerequisite (though
it may still be useful for deciding what the harness's own oracle should
assume — see decision 120's harness-rule consequence). `ROADMAP.md` open
operator question **(cb)** closed accordingly.**

**Sourcing.** `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__8.6.7.md`,
UPDATE 2026-09-02, §A–§G (verdict, six supports, the crux-question rejection,
four candidate readings answered, the two white-routes table, scope-of-
evidence notes, and the sourcing/extraction method — including that
`pdftotext -layout` silently drops 2.0 Table 146's cell values, recovered by
a positional `pdfminer` pass); `iso32000__ref__spot_colour_overprint.md`
(`OP-N3`, promoted from a settled-row note to the sourced answer to this
dispatch); `iso32000__ref__ambiguity_settings_register.md` (register entry).
The empirical half (Acrobat's own preference default and its rendering
consequence) is filed separately, per the spec RAG's own §F instruction not
to hold real-world viewer behaviour in the standards corpus:
`C:\personal_rag\pdf\lesson_20260902_acrobats_overprint_preview_defaults_to_pdfx_only_so_op_true_renders_as_op_false.md`.

**Decision ceiling moves `118` → `119`; next free `120` (superseded in part
same day — see decision 120 immediately below).** **Standing rules ceiling
`R239` — unchanged**, next free `R240`; no rule minted this filing.

### 2026-09-02 (374th filing) — decision 120: **THE MISSING FOURTH MECHANISM — §8.6.6.4 CONVERTS AN UNAVAILABLE SPOT COLORANT TO THE ALTERNATE SPACE *BEFORE* §8.6.7 IS EVER CONSULTED (a `shall`, both editions) — BOTH pdfce'S GREEN AND ACROBAT'S WHITE CONFORM, ON TWO DIFFERENT DEVICE-COLORANT-SET MODELS; `OP-A7` / `spot_colorant_device_model` BECOMES A SHIPPED SETTING**

**Context.** Decision 119 (same day, immediately above) adjudicated the
identical cell on the premise that both renders were reached by *reading*
§8.6.7 differently. `pdfce-spec-librarian` was re-dispatched to test decision
119's own §E hypotheses (`R1` Acrobat not simulating overprint, `R2` the
white object is a transparency group) empirically. Both were run down, and
the result forced a search for a mechanism neither this project nor the
first filing had located.

**What the re-dispatch established (`iso32000__s__8.6.7.md` UPDATE 2026-09-02
SECOND FILING, §H–§L).**

- **`R2` (transparency group) — REFUTED on the file bytes**: 0 `/Group`, 0
  `/S /Transparency`, 0 `/SMask` in either patch. This is worth more than the
  route it closed: it confirms the document is **purely opaque-model**, which
  removes §11.7 (the transparent-imaging-model clause family) from the case
  entirely.
- **`R3` (an `/op` override in the same dict) — REFUTED.** Accepted without
  reservation.
- **`R1` (Acrobat not simulating overprint) — NOT REFUTED, INCONCLUSIVE.**
  The engineer's own experiment (toggle `Use Overprint Preview` to `Always`,
  re-capture, compare connected-green-region counts: `9 → 9`, control patch
  `10 → 10`) has two independent defects: (1) **the positive control did not
  move either** — the control was chosen precisely because toggling the
  preference is supposed to change its appearance, so a run in which the
  control is also inert indicates the preference never reached the renderer
  in the captured process, not that the feature is a no-op; (2) **a connected
  -region COUNT cannot detect the transformation being tested** — filling a
  white hole inside a green region with green does not change the region
  count (an annulus and a filled disc are both one component), so `9 → 9` is
  consistent with either outcome. **A control that does not move is a control
  that did not control**, and a count invariant under the tested
  transformation is not a measurement of it.

**`R4` — the fourth mechanism, found in two files already in the corpus.**
ISO 32000-1 §8.6.6.4 (unchanged in substance in ISO 32000-2, `conforming
reader` → `PDF processor`, `colorant` → `colourant`):

> "At the moment the colour space is set to a `Separation` space, the
> conforming reader **shall determine whether the device has an available
> colorant** corresponding to the name of the requested space. **If so**...
> subsequent painting operations... **shall apply the designated colorant
> directly**... **If the colorant name... does not correspond to a colorant
> available on the device, the conforming reader shall arrange for
> subsequent painting operations to be performed in an alternate colour
> space.**"

This substitution happens **at colour-space-set time — before any painting
operation runs, and therefore before §8.6.7 or Table 148/149/146 is ever
consulted.** On a device without the named spot colorant, the backdrop is
**never a spot colorant on the page**; the tint transform has already turned
it into process CMYK. The white `1 g` object then lands on Table 148's *"any
process colour space × process colorant"* row — `Paint source` in **all
three** overprint columns, `OP true` included. Source `0 0 0 0` renders pure
white, **with overprint fully simulated and fully honoured**. The
*spot-colorant* row decision 119 rested on is never selected on this device.

**Two conforming device models, not one right reading and one wrong one:**

| model | reached via | Table-148 row for the backdrop | result | conforms under |
|---|---|---|---|---|
| **A — `alternate_space_substitution`** | §8.6.6.4's `shall` (device lacks the colorant) | process colorant | white — spot knocked out | **both editions** |
| **B — `simulate_separations`** (pdfce, `Pass 97.x`) | ISO 32000-2 §10.8.3's `may` (a simulated device that supports spot colours) | spot colorant | green — spot preserved | **2.0 only** |

**★ The edition asymmetry, stated so it cannot be mis-read as "pdfce is
wrong":** model B has **no ISO 32000-1 basis at all** for a non-separating
device — it is 2.0-only and 2.0 makes it optional. Model A conforms under
both editions. But §8.6.6.4 NOTE 7 (both editions) and §10.8.3's own worked
example (cyan-then-yellow overprinted gives green under separation
simulation, yellow on an overprint-ignoring device — *"dramatically
different colours,"* the standard's own words) rank model B as producing
**better results under overprint**. **pdfce is on the recommended branch of
an optional 2.0 feature; Acrobat's default composite view is on the
universally available branch. Neither is a defect.**

**Correction to decision 119's reasoning (outcome unchanged).** Struck
in-place in decision 119, above: the claim that "there is no reading of
either edition under which `OP true` erases an unnamed spot colorant" and
"pdfce's green is the conforming render" both over-read a narrower true
claim — *given a device that has the spot colorant*, the standard draws no
distinction between "not named in the source space" and "cannot be
addressed by the source space," and pdfce is right about that narrower
claim. What does not survive: the leap to "therefore Acrobat's white is
non-normative." **Do not change pdfce's render. Do not retract `Pass 97.x`
or any Pass built on it.**

**`OP-A7` / `spot_colorant_device_model` — new setting, shipped as of this
decision.** Neither edition specifies *how* a processor answers §8.6.6.4's
device-colorant-availability test, nor whether it may declare a simulated
device with more colorants than a physical one is entitled to. Both
branches are legitimately available, so this is a **setting**, not a
hard-coded choice, per the project's own standing "spec ambiguity is a
setting" directive:

- **Key:** `spot_colorant_device_model`
- **Options:** `simulate_separations` (**default — pdfce's current,
  unchanged behaviour**) / `alternate_space_substitution`
- **Blast radius: RENDER only.** Never changes emitted bytes.
- ~~**Implementation status: IN PROGRESS as of this filing** — the engineer
  reported starting it in the same dispatch that produced this decision. A
  follow-on librarian filing (Pass shipped) is owed once the commit lands;
  no Pass ID has been assigned by this filing, and none is minted here.~~
  **★ SHIPPED 2026-09-02 (375th filing) — `Pass 233.0`, `047a6d8`.** Enum
  `pdfce_core::settings::SpotColorantDeviceModel` (`SimulateSeparations`
  default / `AlternateSpaceSubstitution`), persisted in `settings.txt` with
  the fork's rationale written in-line, `render-page
  --spot-colorant-device-model` (per-render override, never written back,
  an unknown token refused by name), and a settings-window control — all
  three shells reach the key. One behavioural branch, in
  `authored_spot_inks`: under the composite model it returns no spot inks,
  so no plane is allocated and the paint falls through to the already-
  flattened tint transform, which **is** the alternate space; overprint
  still runs in full on both call sites (`authored_spot_inks` feeds the
  ordinary paint AND `composite_overprint`'s own deposit), it simply has
  nothing spot-coloured left to act on under the composite model. **Measured
  on the suite's four spot patches, same binary, one flag apart:
  `simulate_separations` 9 trap marks fire, `alternate_space_substitution`
  16** — the corpus's traps are built to catch a composite renderer, so
  this is evidence for the shipped default on this corpus, and equally
  evidence that an engine in composite mode is not the oracle the suite
  intends (the harness consequence this decision already records, above,
  is unaffected). See `ROADMAP.md`'s `Pass 233.0` Shipped entry for the
  full cross-checked sourcing.

**★★★ Harness consequence — the most actionable finding, and it is a design
correction, not a bugfix.** **Acrobat cannot serve as an oracle for any
spot-colorant overprint cell unless it is first confirmed to be in
separation-simulation mode** — in composite mode it is measuring a
*different device*, and the standard says a different device legitimately
gets a different answer on this cell. The correct expected value for such a
fixture is **not a single colour; it is a colour per device model.** A
harness or reference file that records one absolute expected RGB for a
spot-overprint cell is encoding an unstated device assumption and will fail
correct code whenever the oracle's own capture mode drifts — the `9 → 9`
non-result in this very adjudication is the first symptom of exactly that.
**Flagged to the engineer: check whether `docs/suite-patch-reference.md`
(not one of this role's five storage tiers, so not edited here) states
absolute expected RGBs for `PCS 3.0`/`PCS 4.0`-class spot-overprint cells;
if so, it needs the per-device-model qualifier.**

**Consequence for `ROADMAP.md` open operator question `(cb)`.** The question
as filed asked whether the operator would re-generate Acrobat reference
renders with `Use Overprint Preview` forced on, to diagnose two conformance
traps. That diagnostic premise is now moot: neither render is a defect, so
there is nothing left to diagnose by re-capturing. **Closed** — see the
resolution note added in place at `(cb)`'s own entry, below.

**Sourcing.** `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__8.6.7.md`,
UPDATE 2026-09-02 (SECOND FILING), §H–§P — the re-dispatch's grading of `R1`/
`R2`/`R3`, the `R1` inconclusiveness argument, the `R4` derivation (`OPSP-7`,
`OPSP-8`), the availability-qualifier survey (five instances, four clauses),
the `OP-A7` table, the correction to §C's over-read crux answer (§K), and the
harness-design consequence (§M.4). Empirical half (Acrobat's composite-view
behaviour and the failed positive-control run) belongs in
`C:\personal_rag\pdf\` per the spec RAG's own §F instruction, not here — see
the dated amendment to
`lesson_20260902_acrobats_overprint_preview_defaults_to_pdfx_only_so_op_true_renders_as_op_false.md`
and the new
`lesson_20260902_a_region_count_metric_is_topologically_blind_to_hole_filling.md`.

**Decision ceiling moves `119` → `120`; next free `121`.** **Standing rules
ceiling `R239` — unchanged**, next free `R240`; no rule minted this filing
(the region-count-instrument finding is filed as a personal_rag/pdf lesson,
not proposed as a new standing rule — see this role's report).

- **2026-09-02 — Decision 121. "ALWAYS GO AHEAD AND PUSH THE LATEST ONE"
  GRANTS STANDING AUTHORITY FOR THE RELEASE ACT ITSELF — CUTTING A TAG,
  PACKAGING, AND DEPLOYING — NOT FOR ANY PUSH DECISION `090` ALREADY DECLINED
  TO GRANT.** (librarian filing, 377th, no shell.)

  **The ruling, verbatim.** Ken, 2026-09-02, given directly after the
  engineer reported that `main` was pushed but fifteen commits sat
  unreleased and the OneDrive-deployed CLI still carried `v0.20.0`:
  **"always go ahead and push the latest one."**

  **What it grants.** `CLAUDE.md` rule 8's remaining gated clause — cutting a
  tag or a release needs "an explicit, current go-ahead each time" — is
  satisfied **standingly** from this ruling forward, for the release act:
  version bump, tag, package, deploy. No future session needs to ask before
  cutting the next release once its constituent Passes are shipped and
  gated green.

  **★ What it does NOT grant, narrowed on the same warrant decision `090`
  used for the push half of this same rule, because the sentence answers one
  question and the rule still bundles more than one act under "push":**

  1. **`git push --force`, or any push that rewrites published history.**
     Unaffected — decision `090`'s reasoning holds without restatement; a
     release ruling does not touch it.
  2. **Pushing any branch other than `main`, or creating remote branches
     beyond a release tag.** The ruling was given about releasing pending
     work sitting on `main`; it does not reach an act it was never asked
     about.
  3. **Skipping the release gates themselves.** "Always go ahead and push
     the latest one" authorises *not asking first*; it does not authorise
     cutting a tag without `run-gates.sh` green, or deploying without the
     fresh-folder smoke test — those obligations are unchanged by who
     approved the act.

  **The precedent, restated because it is doing the same work a second
  time.** Decision `090` narrowed "always push" to the push half of rule 8
  and explicitly declined to extend it to the release half, on the
  reasoning that a release is "a claim that a particular state is fit to
  use — a different act from making commits visible." That reasoning is
  unchanged by this ruling; what changed is that the operator has now
  answered the *second* question decision `090` left open, in the same
  narrow, one-sentence form. **Two separate operator rulings, six days
  apart, each read for exactly what it says rather than for the broadest
  thing it could be read to say** — the same discipline, applied twice, not
  a discipline that wore out the first time.

  **Why this is a decision and not merely a rule-8 edit.** Rule 8's own
  2026-08-05 correction already recorded push and release as "each its own
  decision" — plural, deliberately. Decision `090` answered one. This
  answers the other. Collapsing both into a single rule-8 edit without a
  decision-log entry for each would be the same over-reading rule 4's
  history already warns against.

  **Body-section update, filed in this same edit:** none required, for the
  same reason decision `090` recorded none — §12 is the decision log itself,
  and no other `ARCHITECTURE.md` body section states rule 8's push/release
  split. **The editable mirror of this fact is `CLAUDE.md` rule 8, which
  this role cannot edit** (outside the librarian's five storage tiers) —
  flagged for the engineer to amend, striking the superseded "releasing
  still needs a go-ahead" clause in place rather than deleting it, matching
  how decision `090`'s own push clause was struck rather than removed.

  **No new standing rule number.** Rule 8 is amended, not replaced; `R239`
  is unchanged, next free `R240`. **Ceiling moves `120` → `121`; next free
  `122`.**

  > ★ **Cross-reference 2026-09-03 (395th filing, decision 127).** The
  > release act this decision authorises — *"version bump, tag, package,
  > deploy"* — has one more step than this text names: **a GitHub release
  > with the portable zip as its asset**, on the operator's 2026-09-03
  > instruction. The grant above is unchanged; the recipe is stated in
  > full under decision 127. Between this decision and that one, nine
  > versions (`v0.18.0`–`v0.26.0`) were released without a GitHub page,
  > because this recipe did not name it and `verify-release.py` reported
  > its absence as a `skip`.

### 2026-09-02 (378th filing) — decision 122: **A RENDER-PRESET AXIS THE STANDARD DOES NOT REACH STILL DEFAULTS TO `LeaveAlone` — EXCEPT WHERE THE VALUES RENDER VISIBLY DIFFERENTLY AND THE STANDARD'S OWN LABEL CREATES AN EXPECTATION AN UNPINNED CONTROL WOULD SILENTLY DEFEAT; `PDF/X` NOW PINS `SpotColorantDeviceModel` AT TIER `Implied`**

**(librarian filing, 378th, no shell — relayed by the engineer, `Pass 237.0`,
`c7a774c`.)**

**The gap found.** `Settings::spot_colorant_device_model`
(`SpotColorantDeviceModel`, decision 120, `Pass 233.0`) shipped two Passes
after `RenderPreset`'s per-standard axis grid (`Pass 128.1`) and was never
added to it — `pdfceGUI`'s own coverage contract caught the *setting*
(every setting needs a control) but nothing on the preset side caught the
*axis* (every setting a standard could plausibly constrain needs a grid
row). Found by `pdfceGUI` asking the question directly, not by an internal
audit.

**What the standards actually say — checked, not assumed.** No PDF/X
clause (ISO 15930-1/-3/-4/-7/-9, plus a CGATS/NPES application-notes
document) uses the relevant vocabulary (`OPM`, `simulat`, `proof`); no
clause is even *about* this axis. Every PDF/A part's Scope clause
excludes "operational details of rendering" as an **affirmative
disclaimer**, not a silent gap (0 of 129 veraPDF PDF/A-1 rules touch it).
Read narrowly, `PresetAction::LeaveAlone` (the existing, load-bearing
design choice for an axis no clause reaches — see `ROADMAP.md`'s
`only_sourced_cells_may_claim_to_be_sourced` discussion) is the textbook
answer for every part of this axis, PDF/X included.

**Why PDF/X is pinned anyway — the general principle this decision
records.** The axis's two values (`simulate_separations` /
`alternate_space_substitution`) render a spot colour under overprint
**visibly differently** — preserved on one device model, knocked out on
the other. A control labelled against a named standard ("ISO 15930-7")
carries an implicit promise — *"show me what the press will get"* — that
`LeaveAlone` does not decline to answer: it silently ships whatever global
override the operator last set into a view they read as authoritative.
That is a rule-4 problem (silent inference), not a conformance one, and it
is the reason this decision exists rather than being folded into decision
120's own entry: **`LeaveAlone`'s default is correct in general and the
exception is narrow enough to name.**

**The inference chain pinning `SimulateSeparations` at tier `Implied`
(argued, not sourced)**: ISO 15930-1 §6.3.1 exchanges print elements as
"CMYK data, gray scale data, or separation colour data" for a single
characterized printing condition ⇒ the PDF/X target device **carries the
separations** ⇒ on that device, ISO 32000-1 §8.6.6.4's colorant-
availability test succeeds and the spot colorant survives overprint by a
`shall` (see decision 120's §8.6.6.4 derivation) ⇒ simulating that device
is what the preset's label promises. **No ISO 15930 clause requires
this** and the shipped entry's own `why` field says so in the operator-
facing text, exactly as `LeaveAlone`'s own entries state their absence of
a clause.

**Body-section update.** None in `ARCHITECTURE.md` — this project's preset
design (axis grid, tier vocabulary, `LeaveAlone`, the
`only_sourced_cells_may_claim_to_be_sourced` allow-list) is documented in
`docs/core-api/01-reading-and-model.md` §8.5, not in this file; that
document gained §8.5a "Axis 7" in the same commit. No other
`ARCHITECTURE.md` body section states the preset design, so — as with
decision 121 — none is edited here.

**The reusable rule, for the next axis this happens to.** Before adding
any `Settings` key that a render-fidelity standard *could* plausibly
constrain: (1) check every reachable clause of every subset standard for
the vocabulary, the way this Pass's sourcing did — absence is a real,
citable finding, not a shortcut past one; (2) if no clause reaches it,
default `LeaveAlone`, per the existing design; (3) pin only where the
values are **visibly** different under some real document AND the axis's
own name or the preset's own label would lead an operator to expect the
standard to have an opinion. Two conditions, both required — a control
whose values render identically, or whose name creates no such
expectation, stays `LeaveAlone` even if a diagram happens to exist.

**Sourcing.** Relayed by the engineer (no shell this filing):
`crates/pdfce-core/src/settings/presets.rs` (axis 7 block, full argument in
the source comment), `docs/core-api/01-reading-and-model.md` §8.5a;
spec corpus `D:\Dev\Rag-Specialized\PDF_Spec\pdfx\pdfx__ref__conformance_and_rendering_axes.md`
(axis 7, `PXC-15`..`PXC-21`) and
`pdfa\pdfa__ref__conformance_and_rendering_axes.md` (`PAR-16`..`PAR-18`), not
independently read by this role.

**Decision ceiling moves `121` → `122`; next free `123`.** **Standing rules
ceiling `R239` — unchanged**, next free `R240`; no rule minted this filing.

### 2026-09-02 (381st filing, `e868d36`) — decision 123: **`iccce` RE-PINNED FROM `tag = "v0.3.0"` TO `rev = "a4d9003bf87c61299fa1c6f9c2e2ffffa30de0c3"` — THE SAME COMMIT, AT `iccce`'s OWN REQUEST; EXTENDS DECISION 115'S PIN FORM WITHOUT REVISING ITS REASONING**

**(librarian filing, 381st, no shell — relayed by the engineer, `e868d36`,
part of the `v0.22.0` version bump.)**

**What changed and what did not.** `crates/pdfce-render/Cargo.toml`'s
`iccce-profile`/`iccce-cmm` declarations move from `{ git = "…", tag =
"v0.3.0" }` to `{ git = "…", rev =
"a4d9003bf87c61299fa1c6f9c2e2ffffa30de0c3" }` — the identical commit the
tag resolved to at adoption time (decision 115). **Nothing about what is
managed, what is refused, or the GUI-core boundary changes** — this is a
pin-FORM decision, exactly as decision 115 itself framed the git-vs-path-
vs-vendored choice as separable from the boundary question decision 064
settled. Both lockfiles regenerated; dependency **set** unchanged;
`cargo about` regenerated `THIRD_PARTY_LICENSES.md` with **no** diff;
`cargo tree -p pdfce-render` / `-p pdfce-core` re-verified GUI-free.

**Why — at `iccce`'s own request, and the request is the operative fact.**
`iccce`'s reply (2026-09-01,
`D:\Dev\FeatureRequests\iccce_FeatureRequests\open\reply_depend_on_a_pinned_rev_and_the_four_intent_rules_are_accepted.md`)
accepted depending on a pinned `rev` rather than a `tag`. Two reasons, both
the sibling project's: a `rev` is reproducible **without** committing
`iccce` to cutting a release on pdfce's schedule — a promise it cannot make
— and **a tag can be moved** by the repository that owns it, while a
resolved commit hash cannot. Decision 115 picked pinning **at all** over an
unpinned dependency for exactly this reproducibility reason; this decision
picks the pin form that removes the one channel (tag re-pointing) through
which that reproducibility could still be defeated without pdfce's own
`Cargo.lock` changing.

**Closes a `docs/NEXT_SESSION.md` §E item this role cannot itself verify
against.** That handoff flagged the rev-pin question as *"may already be
satisfied — not verified"*. It was **not** satisfied before this commit
(the manifest said `tag`, not `rev`) and **is now**, per the engineer's
relayed diff.

**★ Downstream literal now stale, flagged not fixed here — see §9's own
note on this same block.** `iccce_provenance()`'s printed banner
self-corrects from the `Cargo.lock` source string, but its own doc-comment
example and `docs/FEATURES.md`'s *Build provenance stamp* row both quote
the pre-re-pin `"(tag v0.3.0, …)"` literal as current output. Not corrected
here — no shell this filing, and this role will not assert an unmeasured
output string (hard rule 8). Owed to the engineer.

**Body-section update.** `ARCHITECTURE.md` §9's `iccce` paragraph, updated
in the same edit as this entry.

**Decision ceiling moves `122` → `123`; next free `124`.** **Standing
rules ceiling `R239` — unchanged**, next free `R240`; no rule minted this
filing — the finding below is recorded as a dated instance under the
existing 2026-08-25 name-ban ruling (open question `(bt)`), not a new
number.

### 2026-09-02 (381st filing, `1611119`) — a documentation-agent DISPATCH is a THIRD leak vector for the licensed suite's name, after a commit message and an untracked file

`tools/check-suite-name-absent.py`, re-run on the release tree before any
push, caught a leaked path component naming the licensed print-conformance
suite inside `ROADMAP.md` (this file's own *Shipped* entry for `Pass
239.0`) and `SESSION_LOG.md` (the 380th filing's entry) — both describing
the operator's OneDrive output folder by its actual path, which contains
the suite's own name as its last component.

**The leak was this role's own**, not the engineer's: the 380th filing's
dispatch prompt named the path verbatim, and it was filed faithfully. Both
lines now read *"the operator's OneDrive `pdfTests` output folder"*
(`ROADMAP.md` line 221; `SESSION_LOG.md` line 86730 as of the correction).

**Recorded, not newly numbered, under the 2026-08-25 operator ruling (open
question `(bt)`) that already governs `tools/check-suite-name-absent.py`.**
That ruling's own record already names two leak vectors the gate cannot
see structurally: **commit messages** (83 occurrences found 2026-08-25, 82
already published) and an **untracked file** the gate does check but a
careless writer could still create outside its scan root. **A dispatch to
a documentation agent is a third**, and the mechanism is different from
both: the gate scans the *work tree*, and a dispatch prompt is neither a
tracked file, an untracked file, nor a commit — it is text that becomes a
tracked file only once the agent it addresses files it, which is exactly
what happened here.

**Binding correction for this role going forward:** a dispatch prompt is
not a trusted source merely because it is faithfully transcribed. This
role must read any dispatch text for the gate's own forbidden terms before
filing it into an editable document, not only run the gate afterward —
running it afterward is what caught this instance, but only after the
false text had already been committed twice.

**No standing rule minted** — this is a dated instance of an existing
ruling's enforcement surface, the same disposition the 368th and 369th
filings gave the "sweep-spelling" and CI-mechanism families before either
crossed this project's two-occurrence bar for a new number.

### 2026-09-03 (388th filing, `e7db280`) — decision 124: `OverprintZeroTintScope`'S DEFAULT FLIPS `GreyAsKOnly` → `DeviceCmykOnly` — DECISION 104'S OWN NAMED CONDITION FOR THE FLIP (THE PER-SPOT-COLORANT PLANE) WAS MET BY `Pass 238.0`/`239.0`, AND RE-MEASURING WITH IT PRESENT SHOWS THE LITERAL ISO 32000-1 READING NOW AGREES WITH THE REFERENCE ON EVERY PRINT-CONFORMANCE PATCH THE HARNESS CAN JUDGE

**(librarian filing, 388th. Commit `e7db280` and its measured figures relayed
by the engineer in the dispatch prompt; independently cross-checked against
live source rather than taken on the dispatch text alone (hard rule 8) —
`crates/pdfce-core/src/settings/mod.rs:765-791` confirmed `#[default]` now
sits on `DeviceCmykOnly`, not `GreyAsKOnly`, and the type's own doc comment
(lines 690-869) already carries the identical reasoning and figures this
entry records, independently authored by the engineer in the same commit.)**

**Why this is a decision and not just a settings-default bookkeeping change.**
`overprint_zero_tint_scope` governs what *every* render of a `DeviceGray`
(and, under `AllProcessSpaces`, `DeviceRGB`/`CalRGB`) fill under `/OP true
/OPM 1` produces by default, on every shell. Decision **104** (2026-08-29,
above) considered flipping this exact default and **declined**, naming a
specific, checkable condition for revisiting that refusal: *"The honest fix
is the literal row assignment **together with** the per-spot-colorant
plane… it will change when the n-colorant buffer lands."* That is a
pre-announced trigger, not a closed question — this decision is the record
that the trigger fired and was acted on, not a fresh argument from
first principles.

**The condition, and when it was met.** Decision 104's own diagnosis: at the
time, flipping the default alone was **trap-neutral** on the conformance
corpus (17 traps before and after) because it corrected one cell and broke
another that only passed through a **compensating error** — pdfce flattened
a spot colorant into C/M/Y for want of a spot plane, and the wrong (`c_b`
under `OP true`) row assignment happened to preserve exactly those
flattened channels. The per-spot-colorant plane `Pass 238.0` (images) and
`Pass 239.0` (shadings, shading patterns, transparency/knockout groups)
shipped closed that compensating error for every route the sweep exercises.
**Nobody re-measured the zero-tint-scope default against the now-present
plane until the operator pointed at `PCS 3.0` and `PCS 4.0.1` still
carrying a trap** — the gap between the plane landing and the re-measurement
is itself worth naming: a divergence kept *because* it compensated for a
different, now-fixed error needs re-measuring the moment that error closes,
not left standing on the reasoning that justified it originally.

**Measured (`tools/suite-check.py`, 51 patches; totals beside their
per-item form, hard rule 10).** Sweep **0 FAIL / 43 pass / 8 unresolved of
51** under the new default (`DeviceCmykOnly`), against **2 FAIL / 41 pass /
8 unresolved of 51** under the old (`GreyAsKOnly`) — same 51-patch
denominator both times. Three patches change a pixel on the flip: `PCS 3.0`
(changed-pixel error 107.7 → 40.6), `PCS 4.0.1` (287.3 → 34.2), and `PCS
9.0` (font-support page grey text rows move toward the reference, not a
pass/fail change). **Nothing else moves.**

**The reference-engine-presumed combination was tried and is WORSE.**
Pairing the literal zero-tint reading with `alternate_space_substitution`
(`OP-A7`/`SpotColorantDeviceModel`, decision 120's other axis) — the
combination that would be presumed correct if Acrobat's reference render
were assumed to use the mandatory §8.6.6.4 substitution branch throughout —
regresses `PCS 3.0` back to **3 traps**. `spot_colorant_device_model`'s own
default (`simulate_separations`) is **unchanged by this decision**; the two
axes are independent and this decision touches only
`overprint_zero_tint_scope`.

**`OP-N3` still holds, unweakened.** Tables 148/149 place *"any process
colour space" × spot colorant × `OP true`* at `c_b` — *do not paint* — in
**both** overprint-mode columns, so a grey fill over a **spot** backdrop
still preserves it under `DeviceCmykOnly` exactly as it did under
`GreyAsKOnly` (`OP-N3`, decision 104). **Only the grey-over-PROCESS-
component case moved** — the discriminating geometry `OP-N3` itself
identified as the one a spot-backdrop fixture cannot exercise.

**Consequence for `(cb)`.** `PCS 3.0`/`PCS 4.0.1`'s remaining traps had been
filed under open operator question `(cb)` (372nd/374th filings) as
*"device-model adjudication"* — that label was imprecise: the two cells were
the **zero-tint-scope** question this decision resolves, not the **device-
colorant-set-model** question decision 120 answered. `(cb)`'s own record
(*Open operator questions*, below) carries a dated amendment saying so;
`(cb)` itself stays CLOSED (374th filing) and now concerns only the device
model in the abstract, with **no patch in the corpus depending on it**.

**What this closes.** The print-conformance sweep now has **nothing left
that pdfce can fix** — every patch the harness can judge (43 of 51; the
remaining 8 are `unresolved`, not `FAIL`) passes. This is a stopping point
for this axis of work, not a claim that the corpus is exhausted — an
`unresolved` patch is a harness limitation, not a graded pass.

**Disclosure.** `pdfceGUI` note filed:
`open/note_the_overprint_zero_tint_default_moved_to_device_cmyk_only.md`
— rule 4 applies to a default flip the same way it applies to any other
inference pdfce makes on the operator's behalf without being asked per
render.

**★ Retracts `docs/NEXT_SESSION.md` §D item 4** ("LEAVE THE DEFAULT ALONE"),
which carried counts 2/2/4 — measured before `Pass 239.0`'s group-merge fix
closed the compensating error this decision's condition depended on. The
engineer will rewrite `NEXT_SESSION.md`; not edited here (outside this
role's five storage tiers).

**Body-section counterpart: none required**, the same shape as decision
104's own note. No crate boundary is redrawn, no invariant defined, no
public API changed — `OverprintZeroTintScope`'s three variants are
byte-for-byte what `Pass 143.0` shipped; only the `#[default]` attribute's
target variant moved. The living account is the type's own doc comment
(`crates/pdfce-core/src/settings/mod.rs:690-869`, already rewritten in the
same commit), `docs/FEATURES.md`'s `overprint_zero_tint_scope` row
(corrected in this same filing), and the spec register's `OP-A5`/`OP-N3`
entries (unchanged — this decision does not revise their reasoning, only
acts on the trigger decision 104 attached to them).

**GUI-core separation:** unaffected and **not re-verified** — no crate
manifest was touched and no dependency added, so `cargo tree` was neither
run nor claimed.

**Decision ceiling moves `123` → `124`; next free `125`.** **Standing rules
ceiling `R240` — unchanged**, next free `R241`; no rule minted this filing.

### 2026-09-03 (390th filing, `98d4377`) — decision 125: REDACTION DESTROYS IMAGE SAMPLES INSTEAD OF REFUSING THE APPLY — CELL-EXACT CLEARING IN THE DECODED SAMPLES WITH LOSSLESS RE-ENCODE, WHOLE-PLACEMENT REMOVAL WITH IN-PLACE TOMBSTONING, COPY-ON-WRITE FOR SHARED IMAGES, AND PER-MARK (NOT PER-DOCUMENT) RETENTION WHEN A PLACEMENT CANNOT BE DECODED; SUPERSEDES THE MECHANISM OF `Pass 8.0` CLAUSE (e), KEEPS ITS POSTURE

**(librarian filing, 390th. Shell available; commit `98d4377` confirmed by
`git log`; `RedactError::ImageUndestroyable` confirmed present and
`ImageRegion` confirmed absent from every `.rs` under `crates/` by `grep`;
`crates/pdfce-core/src/redact_image.rs` confirmed at 1,484 lines by
`wc -l`. Measured figures relayed by the engineer, not re-run.)**

**What was decided, and why each part is a decision rather than
bookkeeping.**

1. **Destroy, don't refuse — and the gate is the SAMPLES, not the
   rectangle.** `Pass 8.0` (2026-08-06, clause (e) above) refused any
   region whose rectangle touched an image's rectangle, by name, with the
   written rationale that a partial raster clear was "more error-prone
   than an honest refusal." Twenty-eight days later the operator's own
   drawings (`OneDrive\pdfTests\Redact`, 11 files, 2 with images) showed
   the refusal is unusable on the document class this project exists for:
   a CAD title block sits inches from a logo, so *touching a rectangle*
   is the common case, not the edge case, and the refusal presented as
   *"the feature does not work"* (operator, verbatim, in pdfceGUI's
   request). The replacement: map each region through the inverse
   placement matrix into image space, snap OUTWARD (floor near, ceil far
   — the glyph surgery's own over-cover bias), overwrite exactly those
   cells in the decoded samples at every bit depth, re-encode as
   `FlateDecode`. A region that touches the rectangle but covers no cell
   destroys nothing and is not an image redaction.
2. **Lossless re-encode; never re-run a lossy codec.** The requirement is
   that the in-region samples are gone, not that the survivors match the
   producer's compression. Re-running DCT or JPX would change every
   surviving sample as a side effect of removing some, which is a
   minimal-diff violation on content the operator did not mark (§5) —
   Flate changes bytes, not samples.
3. **A soft mask's alpha is a SHAPE, so it is cleared over the same
   cells.** A signature on a transparent background is recognisable from
   its `/SMask` alone; a stencil `/Mask` stream likewise. A colour-key
   `/Mask` *array* names values, not positions, and carries no shape —
   left as is.
4. **A wholly covered placement is REMOVED, and its object is TOMBSTONED
   in place (a 1×1 zero-sample image under the same object number) rather
   than deleted.
   > ★ **Amended 2026-09-03 (391st filing, `194b3a1`, decision 126):
   > "zero-sample" is no longer literally true. The tombstone's single
   > sample, like every destroyed cell, now takes the colour space's
   > **no-ink ("paper") value** — `0xFF` for Gray/RGB, `0x00` for CMYK —
   > per `redact_image.rs:1224`. The property this clause argued for
   > (resolves everywhere, carries none of the original samples, costs a
   > few dozen bytes) is unchanged; only the value changed, and why is in
   > decision 126 §5. Kept as written above because it was true on its
   > date.** Chosen over deletion because a `/Resources` dictionary
   shared by other pages or forms would otherwise carry a dangling
   reference — and a dangling `/XObject` entry is the kind of thing a
   different viewer reports as a broken file. The `Do` (or the whole
   inline `BI…EI`) is deleted from the content; the report names the
   removal with page, position, size and resource name.
5. **Copy-on-write for shared images, with every census miss biased
   toward "shared".** A document-wide use census (page content, forms
   recursively to depth 32, annotation appearance streams, tiling
   patterns walked as if painted) decides whether an image is still
   painted somewhere unmarked. A partially covered placement ALWAYS gets
   its own clone under a fresh page-local name (`/pdfceRd<obj>_<n>`); the
   original is tombstoned only when every use was marked. The bias is
   the safe direction: a false "shared" costs one redundant clone; a
   false "unique" would destroy samples under an unmarked placement.
6. **Refusal is per MARK, not per document.** A placement pdfce cannot
   decode (a codec feature it lacks, a corrupt codestream, a bit depth
   Flate cannot carry) RETAINS the marks touching it — left as unapplied
   `/Redact` annotations, nothing removed under them, no overlay drawn —
   while every other mark applies. `RedactionReport::marks_retained`
   counts them, a note names each with its reason, and a new `images`
   carrier reads `DisclosedNotScrubbed` so the acknowledgement gate
   trips. Only when NO mark can be applied does apply refuse:
   `RedactError::ImageUndestroyable { page, reason }`. This is rule 4's
   shape exactly: render the retained mark as it is, disclose off-canvas
   what was not done and why, no gate in front of the marks that CAN be
   applied.
7. **`RedactError::ImageRegion` is REMOVED, not deprecated** — a breaking
   change carried by `v0.26.0` (MINOR, `7d94fe3`). A variant that can no
   longer be produced is a lie in the type; pdfceGUI matches `other =>`
   and is unaffected.

**Also decided, as a side effect of verifying on the operator's file:**
a painted vector path crossing a region — never removed AND never
disclosed since `Pass 8.0`, a rule-4 silence — is now counted
(`vector_paths_intersecting`; the nine painting operators count, `n`
clip-only does not) and disclosed through a `vector_paths` carrier reading
`DisclosedNotScrubbed`. Cutting is `Pass 246.0` (*Next up*). The decision
here is only that the silence closes before the cutting ships, not after.

**What this supersedes.** `Pass 8.0` clause **(e)**'s mechanism (whole-
document refusal by rectangle) — amended in place above with a dated
footer. Its POSTURE — never falsely claim a raster region redacted —
survives unchanged and is now satisfied by destruction plus per-mark
retention. `Pass 8.0`'s deviation 1 (*"partial raster clear was rejected
as more error-prone than an honest refusal"*) was a reasonable call at
n = 0 files; the operator's files are the measurement it lacked.

**Body-section counterpart.** §5's redaction corollary (*"redaction is the
one deliberate exception"*) is unchanged in wording and now more true —
image samples join glyphs as content the exception actually removes. §4's
API surface: `RedactionReport` gains `images_cleared`, `images_removed`,
`images_cloned_shared`, `images_overcovered`, `marks_retained`,
`vector_paths_intersecting`; `RedactError` gains `ImageUndestroyable` and
loses `ImageRegion`. The living account is `docs/core-api/03-capabilities.md`
§4.2/§4.5 (moved in the same commit; `check-core-api-verbs` green) and
`crates/pdfce-core/src/redact_image.rs`'s module doc, which cites
§12.5.6.23, §8.9.3/§8.9.4, Table 93 and the spec RAG's
`iso32000__ref__redaction_removal.md` §4.

**GUI-core separation:** `cargo tree` core/render **re-run by the engineer
and reported clean** (no GUI deps) — relayed, not re-run here; no manifest
was touched (the Pass's `--stat` shows no `Cargo.toml`).

**Decision ceiling moves `124` → `125`; next free `126`.** **Standing rules
ceiling `R240` — unchanged**, next free `R241`; no rule minted this filing
(the decline is argued in `ROADMAP.md`'s 390th-filing *Shipped* block).

### 2026-09-03 (391st filing, `194b3a1`) — decision 126: REDACTION CUTS VECTOR GEOMETRY OUT AND NEVER CLIPS IT; THE REGION IS SUBTRACTED FROM A FILL AS CONVEX STRIPS (SUTHERLAND–HODGMAN), NOT BY A POLYGON BOOLEAN, SO NO BOOLEAN-OPERATIONS DEPENDENCY ENTERS THE CRATE; A CLIP-MARKED PATH OBJECT KEEPS ITS ORIGINAL CONSTRUCTION AS A CLIP AND IS DISCLOSED; ONLY GEOMETRY THAT MEETS THE REGION IS REWRITTEN; DESTROYED IMAGE CELLS ARE PAPER, NOT BLACK; `vector_paths_intersecting` BECOMES THE RESIDUAL

**(librarian filing, 391st. Shell available; commit `194b3a1` confirmed by
`git log`; `crates/pdfce-core/src/redact_vector.rs` confirmed at 1,420
lines by the commit's `--stat`; the rationale below is lifted from that
module's own doc, sections "Why strips rather than a polygon boolean",
"Over-cover, stated" and "The clip-marked path object", and from
`redact_image.rs:85–97` "What a destroyed cell becomes: paper". Measured
figures relayed by the engineer, not re-run.)**

**What was decided, and why each part is a decision rather than
bookkeeping.**

1. **Cut, never clip — §12.5.6.23's clipping ban is read as binding on
   vector content.** The clause forbids hiding *image* data behind a
   clip; the module reads the same prohibition onto paths, because a path
   under a clip is a path whose bytes survive in the file. This is the
   same posture as `Pass 8.0`'s glyph surgery and decision 125's sample
   destruction: the content is gone from the stream, not masked. It is
   also what makes pdfce **exceed** the Acrobat reference, which
   rasterises when it cannot clip (`Acrobat_Features/redaction__content_removal_scope.md`).
2. **The region is subtracted from a fill as up to four convex STRIPS
   (left / right / below / above, bounded by the path's own box), each
   clipped with Sutherland–Hodgman, rather than by a polygon boolean.**
   This is a *library-choice* decision as much as an algorithm: a general
   `polygon − rect` needs a robust boolean-operations library
   (self-intersections, winding rules, coincident edges), and **a wrong
   boolean is a silent wrong picture** — the failure class redaction can
   least afford. `polygon ∩ convex-rect` needs only Sutherland–Hodgman,
   which preserves the winding number of every point inside the clip for
   *any* subject polygon (concave, self-intersecting, multi-subpath), so
   nonzero and even-odd fills both stay **exact**. The cost is a few extra
   path objects for the paths that cross a region; nothing crosses on
   most pages, and a wholly-inside path is simply deleted. **No new
   dependency** — the Pass's `--stat` touches no `Cargo.toml`, and
   `cargo tree` core/render stays GUI-free (§3).
3. **A `W`/`W*`-marked path object keeps its ORIGINAL construction as a
   clip, after the cut paint.** §8.5.4: the clip takes effect after
   painting, from the path as constructed. Rewriting the construction
   would rewrite the clip and shrink the window every later, *unmarked*
   object draws through — content nobody marked would vanish, which is
   the mirror of the failure rule 4 guards against. So the object is
   emitted as the cut paint followed by the original construction with
   `W n`. The original geometry therefore **survives in the stream as a
   clip**; it paints nothing, but a clip shaped like the secret is a
   residual the operator should hear about — counted in
   `vector_clips_kept` and named in the notes. Disclosed, not silent
   (rule 4); not counted as *uncut*, because it is not painted content.
4. **Only geometry that actually MEETS the region is rewritten.** The
   trigger is the path's edges intersecting the region, or a winding test
   at the region's corners and centre landing inside a fill — never the
   bounding box. A stroke passing beside the mark, or an even-odd ring
   whose hole holds it, is left **byte-identical**. This is §5's
   minimal-diff invariant applied inside the one operation that is
   allowed to destroy content: destroy what the mark covers, touch nothing
   else. (The bounding-box test was exactly the coarseness decision 125
   §1 removed from the image half; the vector half was built without it.)
5. **Destroyed image cells are PAPER, not black — and the tombstones
   follow.** Decision 125's cleared cells were zero samples (black in
   Gray/RGB). Table 192 leaves a mark with no `/IC` *transparent*, so the
   destroyed part of an image must look like the page behind it, not like
   a black block the operator did not ask for; with an `/IC` the burnt box
   covers it anyway. The no-ink sample is colour-space-dependent
   (all-ones for Gray/RGB/Cal*/ICC 1,3; all-zeros for
   CMYK/Separation/DeviceN/ICC 4; all-ones for an `/ImageMask`; flipped by
   an inverted `/Decode`; `/Indexed` → entry 0, there being no palette
   answer for "paper"); a soft mask goes transparent over the same cells,
   a stencil `/Mask` masked-out, JPX alpha transparent. **How this was
   found is the part worth keeping:** the pixel proof
   (`crates/pdfce-render/tests/redaction_leaves_no_ink.rs`) asserted zero
   inked pixels inside a region marked with no `/IC`, and its first run
   found **6,241 black pixels — all of them the cleared image.** No unit
   test of decision 125 could have asked that question; they asserted the
   samples were *gone*, not what they had *become*. A design choice with
   a written rationale was wrong at the level the operator sees, and only
   an oracle at that level could say so.
6. **`vector_paths_intersecting` changes meaning: it is now the
   RESIDUAL.** Decision 125's side-effect paragraph made it *every painted
   path crossing a region*; it now counts only the path objects that
   **cannot be replaced as a unit** — a foreign operator between
   construction and paint, which §8.2 forbids — and reads zero on every
   well-formed page. Three counters are added beside it
   (`vector_paths_cut`, `vector_paths_dropped`, `vector_clips_kept`), and
   the `vector_paths` carrier reads `Scrubbed` when something was cut and
   nothing left, `DisclosedNotScrubbed` only for the residual. Rule 4's
   discipline is preserved by construction: the count reaches zero by
   cutting, and a counter with a narrower meaning got a new name for each
   thing it stopped counting. A singular CTM (zero-area placement)
   touching a region drops the object whole — it cannot be inverted back
   into its own coordinates, and a zero-area path is at most a hairline.

**Over-cover, stated as policy rather than left in the numerics:** the
stroke cutting region is the mark expanded by **one full stroke width**
(not inset by half, as the *Next up* entry had sketched — the more
conservative direction), under the CTM's larger scale factor, with zero
width counting as one unit; a cubic piece smaller than the 0.05 pt
flattening tolerance that still straddles the boundary is **dropped**; a
coordinate within `1e-6` of a region edge counts as on the redacted side.
Every bias runs toward removing more, never less — the glyph surgery's
and the image surgery's own bias, now stated for the third kind.

**What this supersedes.** Nothing struck. Decision 125's side-effect
paragraph (*"cutting is `Pass 246.0`"*) is fulfilled, and its clause 4's
"zero-sample" tombstone is amended above with a dated footer. `Pass 8.0`'s
posture — never claim content redacted when it is not — is untouched and
now satisfied for glyphs, image samples and vector geometry alike.

**Body-section counterpart.** §5's redaction corollary is unchanged in
wording and covers a third content kind. §4's API surface:
`RedactionReport` gains `vector_paths_cut`, `vector_paths_dropped`,
`vector_clips_kept`; `vector_paths_intersecting` is re-documented as the
residual; no error variant added or removed (`v0.27.0` is MINOR for the
additions). The living account is `docs/core-api/03-capabilities.md`
§4.2/§4.5 (moved in the same commit; 2,769 lines · 65 clauses) and
`crates/pdfce-core/src/redact_vector.rs`'s module doc, which cites
§12.5.6.23, §8.5, §8.5.4 and the spec RAG's
`iso32000__ref__redaction_removal.md` §3.

**GUI-core separation:** `cargo tree` core/render **re-run by the engineer
and reported clean** (no GUI deps) — relayed, not re-run here; no
manifest was touched. The pixel proof lives in `pdfce-render`'s tests
because core cannot rasterise — the boundary held rather than bent.

**Decision ceiling moves `125` → `126`; next free `127`.** **Standing rules
ceiling `R240` — unchanged**, next free `R241`; no rule minted this filing
(the decline is argued in `ROADMAP.md`'s 391st-filing *Shipped* block).

### 2026-09-03 (395th filing) — decision 127: **"PUSH THE CURRENT RELEASE'S SOURCE AND RELEASE TO GITHUB" — A GITHUB RELEASE WITH THE PORTABLE ZIP AS ITS ASSET IS A STEP OF THE RELEASE ACT, EVERY RELEASE, FROM `v0.27.0` ON. DECISION 121'S RECIPE GAINS THAT ONE STEP; ITS GRANT OF AUTHORITY IS UNTOUCHED. THE OBLIGATION'S MECHANISM EXISTS AND IS DISARMED — `verify-release.py` PRINTS `skip`, NOT `FAIL`, FOR A MISSING RELEASE — AND ARMING IT IS THE OWED HALF**

**(librarian filing, 395th. Shell available: `gh release list` → `v0.27.0`
`Latest`, published `2026-09-03T12:15:02Z`, preceded by `v0.17.0` of
`2026-08-30`; `gh release view v0.27.0 --json assets` → one asset,
`pdfce-v0.27.0-windows-x64.zip`, 25,281,588 bytes, byte-identical to
`D:\builds\pdfce-v0.27.0-windows-x64.zip` by `ls -l`; `python
tools/verify-release.py v0.27.0` → `ok GitHub release has at least one
asset`. The instruction and the act are the engineer's report; the
reading of the instruction as standing is his and is adopted here.)**

**The ruling, verbatim.** Ken, 2026-09-03, mid-session, while the engineer
was working toward the next release:

> *"while you are working on the new relase please push the current
> releases source and release to github!"*

**What was done on it, immediately.** The *source* half was already true:
`main` was on `origin` at `68163a2` and tags `v0.26.0`/`v0.27.0` were on
`origin`. The *release* half was not — no GitHub release had been created
for any version after `v0.17.0`, nine versions (`v0.18.0`–`v0.26.0`) tagged
and deployed to OneDrive with no release page — so `gh release create
v0.27.0 <zip> --latest` was run, the zip a `Compress-Archive` of the
packaged folder `D:\builds\pdfce-20260903-0717-dfce8a9` (49,338,217 bytes;
the zip is 51.2 % of it), with notes covering `v0.18.0`–`v0.27.0` and
naming the nine as OneDrive-only.

**What it decides.** **Every release gets a GitHub release with the
portable zip as its asset.** The release act decision 121 authorised —
*"version bump, tag, package, deploy"* — is, in full and in order:

1. version bump and tag (decision 121);
2. package (`tools/package-portable.py`) and the fresh-folder smoke test
   (§6);
3. deploy the CLI to OneDrive, alternating slots (`R229`);
4. **GitHub release for the tag, the portable zip attached, marked
   latest** (this decision);
5. `tools/verify-release.py <tag>` green on every check.

A release missing step 4 is unfinished in the same sense a release missing
step 3 has been unfinished since `R229`.

**Why this is a decision and not an amendment to 121.** Decision 121 is a
grant of *authority*: the engineer need not ask before releasing. This is
an *obligation*: the engineer must do one more thing when releasing. The
two are different kinds of content and would blur inside one record — a
reader of 121's *"version bump, tag, package, deploy"* would still not see
the step, and **that omission is how the gap opened**: 121's recipe never
named the GitHub release, `verify-release.py` reported its absence as
`skip GitHub release -- gh unavailable or not authenticated`, the
engineer's dispatches read that line as tool trouble (the 391st filing's
premise, verbatim: *"the script's `gh` subprocess reports unavailable, as
it has since v0.18.0"*), and the release records before the 391st did not
mention the check at all — by grep of both ledgers — while nine versions
shipped without a page. `R229`'s own warning applied exactly: *an
instruction that survives on narrative stops the moment no session repeats
it.* The 391st filing caught it by reading the script instead of the
message; the operator's sentence, three filings later, settled it.

**Why not a standing-rule number.** `R229` carried the OneDrive obligation
as a rule because it shipped with a mechanism that checks it. Here the
mechanism **exists and is disarmed**: `tools/verify-release.py:202–203`
turns a non-zero `gh release view` into a `skip` — the same branch for
"`gh` not installed" and "no release for this tag". **A mandated step whose
absence prints `skip` is enforced by nobody.** That is `R241`'s argument
(394th filing) one act further along the same recipe, and `R241`'s
disposition applies: once the check fails closed, the gate is the rule and
a number beside it would be a second copy. So: **owed — `verify-release.py`
must `FAIL` on a not-found release and say `gh` is unavailable only when it
is** (the 391st filing's owed message fix, now load-bearing rather than
cosmetic). Until then the obligation rests on this record and the
engineer's memory, which is the state this project has three instances of
not surviving.

**Read narrowly, on the 090/121 warrant, and the narrowing is part of the
ruling.** The sentence covers the GitHub half of the release act for the
current and future releases. It does **not**:

1. authorise backfilling release pages for `v0.18.0`–`v0.26.0` — not
   asked; the engineer folded their notes into `v0.27.0`'s body, which is
   recorded here as the disposition of the gap;
2. touch decision `090`'s or `121`'s exclusions — `--force`, non-`main`
   branches, remote refs other than the release tag;
3. relax any gate — a release page is a public claim that a state is fit
   to use (decision 090's own reasoning), so it comes *after* green gates
   and the smoke test, never instead of them; `check-shipped-assets.py`'s
   licence enforcement applies to what the zip carries (`LEGAL.md` §1.1:
   the repository is public, so an asset publishes).

**Closes `ROADMAP.md` open question (391st filing):** *"decide whether
`v0.18.0`–`v0.26.0` should get GitHub release pages or whether OneDrive is
the release channel now."* **Both are channels, for different readers.**
OneDrive carries the CLI for the operator, two slots, previous version
preserved (`R229`). GitHub carries the whole portable folder for anyone
else and is the offsite copy of the build (the `v0.9.0` release record's
*"a GitHub release is an offsite copy of one build and one PDF asset — it
is not a `git bundle`"*, `ROADMAP.md`, 2026-08-25). Neither substitutes
for the `git bundle` backup.

**Body-section counterpart, filed in this same edit:** §6 (*Packaging*)
gains a *release channels* bullet naming both channels and the order.
**`CLAUDE.md` rule 8**'s recipe sentence — *"So: cut the tag, package,
smoke-test, deploy to OneDrive, no per-release go-ahead"* — is one step
short and is outside this role's tiers; flagged for the engineer, as
decision 121 flagged the same sentence.

**Decision ceiling moves `126` → `127`; next free `128`.** **Standing rules
ceiling `R241` — unchanged**, next free `R242`; no rule minted (argued
above).

> ★ **ARMED 2026-09-03 (396th filing, `c0c8dee`).** The owed half is
> done: `tools/verify-release.py` distinguishes *`gh` not installed*
> (`shutil.which("gh") is None` → `skip`, a machine fact) from *`gh
> release view <tag>` failed* (→ **`FAIL GitHub release exists`**, the
> message naming this decision, `gh release create` and `gh auth status`).
> Probed both ways by the engineer and again by the librarian: `v0.27.0`
> → `ok`; `v0.26.0` → `FAIL … release not found`. The argument above for
> not minting a rule number now holds in full — the gate fails closed, so
> the gate is the rule. The heading's *"EXISTS AND IS DISARMED"* is kept
> as history of the state this decision was written in. The same commit
> resolves `R241` clause 2's variance (the sweep goes red on an inactive
> hook), recorded under that rule.


### 2026-09-03 (397th filing) — decision 128: **THE PRODUCT IS NAMED `pdfcer` — "pdf-see-er": create, edit, read — AND `pdfce` IS ITS PRE-RELEASE CODE NAME. THE FORK IS A `git clone` INTO `D:\Dev\pdfcer`, NEVER A FRESH REPOSITORY, BECAUSE THE RECORD CITES 2,040 COMMITS BY HASH; THE IN-REPO GUI CRATE IS REMOVED FROM THE PRODUCT; DATED RECORDS KEEP THE CODE NAME; THE CLI BINARY IS `pdfcer`, NO DASH**

**(librarian filing, 397th. Operator ruling, two messages the same
session, verbatim in `ROADMAP.md`'s `Pass 247.x` entry: the plan-only
message ending *"well I guess our code name for this pre-release might be
pdfce!"*, then **"Let's do it."** given on the librarian's measured sizing.
Figures below by shell: `wc -l`, `grep -ro`, `cargo tree` reading of the
manifests, `crates.io` API, `gh repo view`, `gh search repos`.)**

**What is decided, and why each part is architecture rather than
housekeeping.**

1. **The name.** Product, repository, folder, workspace crates and CLI
   binary become `pdfcer` (`pdfcer-core`, `pdfcer-render`, `pdfcer-cli`,
   `pdfcer-print`, `pdfcer-fetch`; binary **`pdfcer`**). `pdfce` is the
   code name under which everything to date was built and is **kept in
   every dated record** — `ROADMAP.md` *Shipped*, `SESSION_LOG.md`, this
   log's entries, `docs/decisions/`. Those are append-only and were true
   when written; the operator's own Windows analogy is the framing. A
   present-tense `pdfce` after `Pass 247.1` is a defect; a dated one is
   history. Name checked free on crates.io and GitHub (read-only, this
   filing).
2. **The fork is a clone.** `git clone D:\Dev\pdfce D:\Dev\pdfcer`, then
   changes inside the clone. The alternative — a new repository seeded
   from a snapshot — would orphan the **2,040** distinct commit hashes
   the two ledgers cite and that `check-cited-commits-exist.py` verifies
   on `main` in CI, and would blind `check-commits-filed.py`'s
   whole-history walk. A clone keeps every hash, every tag
   (`v0.5.1`–`v0.27.0`) and the release notes, so the version line
   continues (`v0.28.0` is the first `pdfcer` release, not a `v0.1.0`).
   `D:\Dev\pdfce` is never written to again except for one README
   pointer commit when `KenM76/pdfce` is archived — it is the backup the
   operator asked for.
   **★ NOTE 2026-09-03 (401st filing, `Pass 247.2`) — "one README pointer
   commit" became TWO, and the second is what makes the first
   filable.** `fbc53ee` is the README pointer; `c0c67d3` is a final
   `SESSION_LOG.md` entry filing it. `tools/check-commits-filed.py`
   counts a README change as a CODE commit and is red on any code
   commit no filing names — so a one-commit archive would have frozen
   `KenM76/pdfce` with its own filing gate permanently red at the tip.
   Archiving disables Actions, so neither commit ever ran CI there (the
   last run is `cce414e`, green); the gate is green by construction of
   the record, not by a run. Both pushed, then `gh repo archive
   KenM76/pdfce -y` → `isArchived: true` (relayed from the engineer; the
   librarian ran no `git` in `D:\Dev\pdfce`). The backup folder has now
   received its last writes. Nothing here was false when written — it
   described a plan whose premise (a README commit is not a code
   commit) the gate refuted; corrected in place per this log's
   footer convention, as item 3 was.
3. **The in-repo GUI crate is removed, not paused.** `crates/pdfce-gui`
   (62,902 lines, 16.8 % of 373,548) has been paused since 2026-08-13
   and superseded by `D:\dev\pdfceGUI` (decision 073's ownership
   statement; the operator's README of `c1e4c17` calls it obsolete). It
   leaves the workspace with its dependencies and the four gates that
   only ever read it. **§3's GUI-core separation invariant survives the
   crate**: the *zero GUI deps* CI job stays, trivially green — the
   invariant is a property of `pdfce-core`/`pdfce-render` that a
   downstream shell relies on, not a property of having a shell in the
   tree. `docs/FEATURES.md`'s `gui` column is unchanged: it has tracked
   `D:\dev\pdfceGUI` since decision 073.
   **★ CORRECTED 2026-09-03 (399th filing, `da3b2f8`) — "the four gates
   that only ever read it" was the PLAN's premise, not what shipped.**
   Read against `Pass 247.0`'s own diff: **three** gates were deleted
   with their CI steps (`check-ui-strings.sh`, `check-theme-colors.sh`,
   `check-disclosure-channel.sh`); a **fourth**, `check-string-gaps.sh`,
   was assumed GUI-only by this decision and by `ROADMAP.md`'s own
   `Pass 247.0` step 3 — it is not: it scans `for root in crates tools`
   and is the gate that caught a `pdfce-cli` refusal with fourteen baked
   spaces on 2026-08-27. It **stays**, losing only its dead
   `"pdfce-gui: "` prefix branch. Net: three deleted, five de-branched
   (unchanged from this entry's plan), one stays. **This decision's own
   text is corrected in place per the append-only-decision-log footer
   convention** — nothing struck, because nothing here was false at the
   time it was written; it described a plan, and the plan's premise is
   what needed correcting once the diff existed to check it against.
4. **`D:\dev\pdfceGUI` is a downstream consumer with a path dependency
   into this tree**, and is re-pointed through its own channel, not
   silently. Whether it renames is open question **(cd)**; default no.
   *Answered the same session, operator verbatim: "yes pdfcerGUI is
   getting the rename too and is being taken care of in a different
   session by its engineer agent."* and then *"FYI decided on the other
   repo being named pdfcer-gui and in d:\dev\pdfcer-gui"* — so it
   becomes **`pdfcer-gui`** at **`D:\dev\pdfcer-gui`** by its own hand; the two renames must agree on crate names and folder
   before either switches its path dependency.
5. **Authority.** Pushing `main` and cutting a release are standing
   (decisions 090, 121, 127). **Creating `KenM76/pdfcer` and archiving
   `KenM76/pdfce` are not** — they are acts outside the working tree
   that the operator's *"Let's do it"* authorises for `Pass 247.2` only,
   because the plan he approved named them. Not read as standing.
   **★ NOTE 2026-09-03 (401st filing): both acts are DONE and the
   one-Pass authority is CONSUMED** — `KenM76/pdfcer` created (public,
   `main` + 31 tags, first CI run on `562ca7e` green, relayed) and
   `KenM76/pdfce` archived. `git remote -v` in `D:\Dev\pdfcer` →
   `https://github.com/KenM76/pdfcer.git`, `0 0` against `origin/main`
   (measured by the filing). Pushing and releasing continue under
   decisions 090/121/127; creating or archiving any further repository
   would need its own go-ahead.

**Sequencing is part of the decision:** strip (`247.0`), then rename
(`247.1`), then publish/archive/release (`247.2`), each green before the
next. The rename is the widest change (~14,500 occurrences in code and
tools) and the strip the deepest (a crate and nine gates); the narrow-deep
step runs first so the wide-shallow one runs over a smaller tree, and a
failure in one cannot be mistaken for a failure in the other.

**Body-section counterparts, deferred to the shipping filings, stated
now so they are not forgotten:** §3 (workspace layout) loses
`crates/pdfce-gui` at `247.0` and renames all crates at `247.1`; §6
(packaging) loses `pdfce-gui.exe` and gains the `pdfcer` binary name;
§7's CLI name; §2's stack table (egui/eframe leave the product's
dependency set — they remain pdfceGUI's). The project `CLAUDE.md` and
`.claude/agents/*.md` rename at `247.1`. **The global
`C:\Users\Ken\.claude\CLAUDE.md`** names `D:\Dev\pdfce\` in its Cross-project
RAG list and agent roster — flagged for the operator, never edited by an
agent.

**Decision ceiling moves `127` → `128`; next free `129`.** **Standing rules
ceiling `R241` — unchanged**, next free `R242`. **Open operator questions:
`(cd)` minted and answered the same session** (see item 4), next free
`(ce)`.


### 2026-09-03 (398th filing, `22421b6`) — decision 129: **SIGNATURE VERIFICATION'S ARITHMETIC AND PARSING ARE IN-CRATE — NO NEW DEPENDENCY — BECAUSE CONSTANT-TIME CODE PROTECTS A SECRET AND VERIFICATION HOLDS NONE. THE MD5/RC4 IN-CRATE JUDGEMENT IS EXTENDED NARROWLY, UNDER THAT NEW DISCRIMINANT, AND EVERY MODULE HEADER SAYS IT DOES NOT EXTEND TO SIGNING**

**(librarian filing, 398th; the previous dispatch for this filing was
killed by an API error after `ROADMAP.md` and `FEATURES.md` were written
and before this entry existed — finished here, nothing already written
re-touched. Figures by shell: `git show 22421b6 --stat`, `wc -l`, `sed -n`
on the cited source lines; the scratch-crate resolution and the gate
results are the commit message's and are marked relayed where used.)**

**Context.** `Pass 10.1` answers `pdfceGUI`'s 2026-09-03 request for
signature validation with integrity and coverage split from trust. Doing
the integrity half needs RSA (PKCS#1 v1.5 and PSS) and ECDSA (P-256,
P-384) verification, SHA-1 beside the existing SHA-2, arbitrary-precision
modular arithmetic, a DER reader and a CMS/X.509 walker. `docs/PRIOR_ART.md`
had pre-selected the RustCrypto stack for all of it, and the 396th filing
licence-checked `num-bigint` 0.5.1, `p256` / `p384` 0.14.0 and `sha1`
0.11.0 as *candidates — verification only*. The engineer's commit records
that the stack was **resolved in a scratch crate before deciding**.

**What the resolution showed (relayed from `22421b6`'s message).**
`num-bigint` + `p256` / `p384` / `ecdsa` / `elliptic-curve` + `cms` +
`x509-cert` → **25 crates**. `cms` is `0.3.0-pre.2` and `rsa` is
`0.10.0-rc.18` — pre-release, matching the 396th filing's `cargo info`
reading. `cmov` and `hybrid-array` carry **cfg-selected `unsafe`** — the
same shape decision 039 accepted for `aes` and later `sha2`: a backend
chosen on a `cfg`, not a Cargo feature, so `default-features = false`
cannot switch it off and no downstream consumer of `pdfce-core` inherits
whatever pdfce's own build sets.

**The decision, and the argument that carries it.** Decision 039 accepted
`unsafe` intrinsics for AES because the alternative — a hand-rolled block
cipher — carries a real constant-time hazard, and **constant-time code
exists to protect a SECRET** (a key, a password-derived key). Signature
verification **handles no secret**: the public key is in the certificate,
the signature is in `/Contents`, the digest is computed over bytes the
verifier already holds. A timing leak in verification leaks nothing an
attacker does not already have. So the argument that admitted `aes` does
not apply, and stretching it would have accepted a 25-crate, partly
pre-release, partly-`unsafe` dependency set for a hazard that is not
present. The in-crate judgement of the ninety-sixth filing (MD5/RC4:
frozen, read-only/compat-only, auditable in one sitting) is **extended
under a fourth discriminant — no secret is handled** — to:

| Module | Lines (`wc -l`) | Reference it is checked against |
|---|---|---|
| `crypto/bignum.rs` | 548 | 400 random cases vs a bit-serial reference; modpow and inversion vs Python's `pow` |
| `crypto/sha1.rs` | 141 | FIPS 180-4 vectors |
| `crypto/rsa.rs` | 285 | PKCS#1 v1.5 strict whole-EM compare (Bleichenbacher 2006); RSASSA-PSS, RFC 4055, MGF1, trailer `0xBC` |
| `crypto/ecdsa.rs` | 435 | RFC 6979 A.2.5 / A.2.6 published vectors, both curves; `n·G` is the identity; off-curve point refused (SEC 1 §4.1.4) |
| `asn1.rs` | 292 | DER only — definite lengths, single-byte tags, no BER |
| `cms.rs` | 418 | RFC 5652 SignedData / SignerInfo; X.509 v3 with SubjectKeyIdentifier |

**2,119 lines over six modules** (`signature_verify.rs`, 739, is the
verifier that uses them, not a primitive). Fuzz target `signature_verify`,
67,204 runs / 241 s = **279 runs/s**, 0 crashes, cov 3,767 (relayed).

**What it does NOT decide — stated by name, because the ninety-sixth
entry's warning is the reason this entry exists.** *Signing* handles a
private key and therefore falls under decision 039's condition, not this
one; every one of the six module headers says the in-crate judgement does
not extend to signing (`crypto/bignum.rs:20`, read here: *"It does NOT
extend to signing."*). The `cms` / `rsa` / `x509-cert` rows in
`docs/PRIOR_ART.md` stay open for that half. Trust — chain building,
revocation, a store, a clock — is not built and `Trust` has one variant,
`NotChecked`, so nothing here pre-decides how a trust stage sources ITS
crypto either. And this is not a widening of decision 039: no cfg-selected
`unsafe` was admitted; the set of dependencies under 039's exception is
still `aes`, `cbc`, `sha2`.

**Body sections updated in this filing:** §3's `pdfce-core` cell
(`signature.rs` gains the verifier's re-export; the six modules named);
§7 (`verify-signatures` bullet with the exit-12/13 contract; the
`list-signatures` bullet's *"NOT cryptographic verification"* qualified);
§9 (the no-new-dependency paragraph, and the owed `DEPENDENCIES.md`
line). The ninety-sixth entry carries a forward footer to here.

**Related facts carried so they are not re-derived.** (1) The verifier
found a pre-existing defect in `tools/gen-signature-fixtures.py` — the
`Pass 10.0` coverage fixtures excluded the hex digits but not the `<` `>`
delimiters from `/ByteRange`, against §12.8.3.3 (`SI-W3`) and against
the generator's own comment; byte-counting coverage could not see it,
the first consumer that read the delimiters did. Fixed in the same
commit; footered to `D:\dev\rag\rust\a_claim_in_a_comment_is_not_a_check.md`.
(2) The spec side records two corrections owed to its own §7.6.1 /
encryption-impl files: a signature's `/Contents` is never encrypted, and
encryption is applied BEFORE a signature is incorporated — so the verifier
hashes ciphertext as-is and never decrypts on the verification path, and
`Pass 5.4`'s `set_encryption` on a signed document must refuse by name
or disclose signature destruction (noted in that entry, `ROADMAP.md`).
(3) The verdict names (`Verified` / `DigestMismatch` / `SignatureInvalid`
/ `Unverifiable`) are pdfce's own until ETSI EN 319 102-1 is ingested;
`TOTAL-PASSED` / `TOTAL-FAILED` / `INDETERMINATE` are that standard's and
are not borrowed before it is read.

**Decision ceiling moves `128` → `129`; next free `130`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted**; `(cd)` answered at the 397th; next free `(ce)`.


### 2026-09-03 (399th filing, `da3b2f8`) — decision 130: **A CLAIM ABOUT CODE THIS REPOSITORY NO LONGER CONTAINS IS FILED AS A PINNED-COMMIT CITATION OR A NAMED, PRINTED TABLE — NEVER A BARE ALLOWLIST — SO IT STAYS CHECKABLE AFTER THE THING IT DESCRIBES HAS LEFT THE TREE**

**(librarian filing, 399th. No shell this session — every figure below
is relayed from the engineer's `Pass 247.0` dispatch, not independently
re-verified; nothing here is asserted as shell-checked. `Pass 247.0`
ships decision 128's `247.0` step, and this decision covers the two
pieces of that step decision 128 flagged as possibly needing their own
entry — the settings-gate exemption and the core-api citation
convention — judged here to be one architectural pattern, not two.)**

**The problem, stated once because it now has two instances.** Deleting
`crates/pdfce-gui` from this workspace does not delete the FACTS that
were true about it — that `docs/core-api/` cited 26 specific lines in
it, and that `Settings::theme` is read by code that lives there. Two
mechanisms in this repo now have to keep making a claim about
something the repo can no longer show a reader:

1. **`docs/core-api/`'s 26 line-anchored citations into
   `crates/pdfce-gui/src/...`.** Re-pointed, in the same commit, to
   `pdfce@cce414e:crates/pdfce-gui/...` — a pinned commit in the
   now-backup `D:\Dev\pdfce` repository, the fork point, with a head
   note in each of the three affected `docs/core-api/` files giving the
   exact command a reader runs to see the cited line
   (`git -C D:\Dev\pdfce show cce414e:<path>`). `check-core-api-verbs.py`
   verifies the citation format, not that the pinned commit still
   exists on disk — that half is the same trust `D:\Dev\pdfce` already
   carries as "the untouched backup" (decision 128 item 2).
2. **`tools/check-settings-consumed.py`'s `theme` key.** With the GUI
   branch removed from the gate (its only in-tree reader gone with the
   crate), the gate went RED — correctly, because nothing in THIS tree
   reads `theme` any more. Its one remaining reader is
   `D:\dev\pdfcer-gui` (`crates/pdfcer-gui/src/app/frame.rs`
   `self.settings.theme`; `settings_window.rs`
   `Preset::from_key(&self.settings.theme)` — grepped in that tree by
   the engineer, relayed here). Fix: a new named table,
   `CONSUMED_BY_OUT_OF_TREE_GUI`, listing `theme` against that citation;
   the gate **prints** the table's entries as part of its PASS output
   rather than silently treating them as exempt.

**Why these are one decision, not two.** Both are the same move under
two different failure shapes. A citation into a repository this project
does not build (`D:\Dev\pdfce` after the fork, `D:\dev\pdfcer-gui`
always) cannot be verified by anything running IN this repository's CI
— `check-cited-commits-exist.py` walks `main`'s own history, not a
sibling folder's, and no test here can import `pdfcer-gui`'s source.
**The failure mode this decision exists to prevent is `R203`'s, in code
rather than in prose**: `D:/dev/rag/rust/a_blocker_naming_another_repository_cannot_fail_a_test_so_it_decays_silently.md`
found that a bare claim about another repository has no falsifier here
and rots silently regardless of diligence — two `FEATURES.md` rows
stayed wrong for weeks under exactly that shape before `R203` adopted
the practice of citing a **surface and a date**, not asserting a bare
verdict. The settings gate and the core-api citation are `R203`'s
practice **applied to machine-checked artifacts instead of a
hand-maintained doc row**: neither can be verified, so both are filed as
an explicit, named, PRINTED claim — a citation with a command that
reproduces it, or a table entry the gate echoes on every green run —
rather than as a silent pass that looks identical to "nothing to check
here."

**What this is NOT.** Not a relaxation of `check-settings-consumed.py`'s
enforcement — an in-tree setting with no in-tree AND no cited
out-of-tree reader still fails the gate exactly as before; only a
**named, cited** out-of-tree reader is accepted, and the citation is
itself a claim someone could go verify, not a blanket exemption clause.
Not a new dependency or coupling between the two repositories — no code
here imports from `pdfcer-gui` or vice versa; the table is documentation
the gate happens to print, not a build-time check across the boundary.

**Consequence for any future cross-repository claim this project files
(`pdfcer-gui`, `pdfce` after the archive, any later fork).** Use one of
these two shapes, not a bare assertion: (a) a **pinned-commit citation**
(`repo@hash:path:line`) when the claim is about a specific piece of code
at a specific point in time, or (b) a **named table the checking script
itself prints**, when the claim is "X is still true of the current state
of another repository" and needs to be re-asserted, not merely dated.

**Body sections updated in this filing:** §3's `pdfce-gui\` workspace-
layout node (now a removal notice plus the pinned-commit citation,
history preserved beneath it, unchanged); §7's packaging bullet
(`pdfce-cli.exe` is the only binary; decision 054's bullet struck as
moot); §2's `Cargo.lock` bullet (now produces `pdfce-cli` only). `§12`
decision 128 item 3 gained a dated correction footer (the "four gates"
premise; see that entry) — filed under decision 128 rather than here,
because it corrects THAT entry's own claim, not a new decision.

**Decision ceiling moves `129` → `130`; next free `131`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted**; next free `(ce)`.

### 2026-09-03 (400th filing, `4db298d`) — decision 131: **A RENAME MUST NOT COST THE OPERATOR HIS MEASUREMENTS. WHEN AN IDENTIFIER THAT A SAVED DOCUMENT CARRIES CHANGES, THE READER ACCEPTS EVERY NAME THE PRODUCT EVER WROTE, THE WRITER EMITS ONLY THE CURRENT ONE, AND A LEGACY NAME IS RETIRED ON THE FIRST WRITE — NEVER CARRIED FOREVER, NEVER DROPPED SILENTLY. FIRST INSTANCE: THE CE-DIMENSION SIDECAR KEY `/PieceInfo /pdfce` → `/pdfcer`**

> ★★ **SUPERSEDED BY OPERATOR RULING — 2026-09-03, the same day, one
> filing later (402nd filing, `Pass 247.3`, `4d52fb3`).** The operator,
> verbatim:
>
> > *"I see you made the engine backwards compataible for the
> > measurements. This is unecessary as no one has actually used the
> > software yet in production including myself. This compatibility
> > layer can be removed."*
>
> **What the ruling changes.** This decision's three clauses argued
> from the builds — *25 of 30 tags wrote `/pdfce`* — to an obligation
> toward the documents those builds saved. The ruling supplies the fact
> the argument did not have: **no such document exists**, the
> operator's own included. A compatibility obligation is owed to
> documents, not to builds, so with the set empty the obligation is
> void and the mechanism is dead weight. **The measurement that
> corroborates him** (relayed from the engineer): with the fallback
> removed and the fixtures not yet re-keyed, **six `dimension_rotate`
> tests failed** — the fallback's entire caseload was this repository's
> own test corpus.
>
> **What is removed at `4d52fb3`** (read in source, 402nd filing):
> `EditSession::sidecar_entry`, `SIDECAR_KEY_LEGACY`, the
> `remove(LEGACY)` step in `write_dimension_model`, and
> `crates/pdfcer-core/tests/sidecar_legacy_key.rs` (3 tests; 4,733 →
> 4,730). Both readers take `piece.get(SIDECAR_KEY)` directly
> (`edit.rs:37598`, `:37616`); the writer inserts `/pdfcer` and removes
> nothing (`:37669`). Nine fixtures re-keyed `/pdfce <<` → `/pdfcer<<`
> in place at equal byte length; three regenerated. `v0.28.0` is the
> one release that carries the fallback; the next will not; no
> re-release, since nothing operator-visible differs.
>
> **What survives, and in what standing.** (1) The **`pdfceF{n}` KEEP
> addendum below stands on its own reasoning** — a `/Resources` key is
> allocated, never looked up — and is unaffected. (2) The **scope
> statement** (*a guard is owed only where a reader BRANCHES on the
> identifier*) survives as the correct **description** of when a
> format change is a format change; what no longer follows from it is
> an obligation to build a guard for a pre-1.0 shape pdfcer itself
> wrote. (3) The three clauses are **the right shape for a rename that
> reaches documents that exist** — a future rename made after the
> product is in production would reach for them; that is a decision
> for that day, taken with the operator, not a standing obligation
> carried from this one. (4) The equal-byte-length fixture discipline
> was reused in reverse for the re-keying.
>
> **The instruction in force from here, without a numbered rule** (the
> standing-rule candidate was named and declined at n = 1 in the
> `Pass 247.3` `ROADMAP.md` entry, §7): *do not build a
> backward-compatibility layer for a pre-release format pdfcer itself
> wrote without an operator ask; re-key fixtures instead of teaching
> the reader two spellings; compatibility with other producers' files
> is a different thing and unaffected.* It lives in
> `docs/NEXT_SESSION.md` and the engineer's agent memory.
>
> **Why this is filed as SUPERSEDED rather than reverted or deleted.**
> The decision was correct on its own premises and wrong on a premise
> it did not check — a reader arriving here should see both the
> reasoning and the fact that defeated it, because the reasoning is
> what a post-1.0 rename will need. The 401st filing's addendum below,
> written while the removal sat uncommitted in the tree, said this
> filing owed 131 *"a dated disposition"*; this is it. Text of the
> decision below is untouched.

**(librarian filing, 400th. Shell available, read-only: the shape below
is read in source — `crates/pdfcer-core/src/edit.rs:33190` `const
SIDECAR_KEY: &[u8] = b"pdfcer";`, `:33193` `const SIDECAR_KEY_LEGACY:
&[u8] = b"pdfce";`, `:37634-37635` the `get(SIDECAR_KEY).or_else(…
LEGACY)` read, `:37691-37692` the `remove(LEGACY)` then
`insert(SIDECAR_KEY)` write — and the test file
`crates/pdfcer-core/tests/sidecar_legacy_key.rs` holds 3 `#[test]` by
`grep -c`. The test count 4,733 and the gate runs are relayed from the
commit message. `Pass 247.1` is decision 128's second step; this
decision covers the one place where that step, planned as a
no-behaviour-change mechanical rename, turned out to change what an
existing document means.)**

**The problem.** `Pass 247.1` renames every present-tense `pdfce` to
`pdfcer`. Almost all of those names are internal — crate names, module
paths, env variables, a binary — and renaming them changes nothing any
document carries. **One is not.** The ce-dimension model (decision 026)
persists its groups, scale, number format, style cascade and label
overrides in a catalog sidecar at `/PieceInfo << /<name> << /Private …
>> >>`, and ISO 32000-1 §14.5 keys that dictionary **by the application's
name** — the standard's own instruction that the key be the product
name is why a product rename reaches a saved file. The writer, after the
mechanical pass, emits `/pdfcer`. **A reader that then looked only for
`/pdfcer` would open every document saved with ce dimensions by
`v0.5.1`–`v0.27.0` — **25 of the 30 tags** by `git tag | sort -V`,
every one after `v0.5.0` — and show none of their
measurement model**, while the `/Line` annotations kept rendering off
their baked `/AP`. Nothing would look wrong on the page. The groups,
the scale, the overrides, the ability to re-measure or re-scale — all
gone on the next open, with no error, because "no sidecar" is a valid
state for a document that never had one. This is the worst kind of
loss: silent, plausible, and discovered only when the operator reaches
for a verb that needs the model.

**The rule, in three clauses, and each clause is load-bearing:**

1. **The reader accepts every key the product ever wrote.**
   `EditSession::sidecar_entry` reads `/pdfcer` first and falls back to
   `/pdfce`. Order matters: a document that somehow carried both would
   read the current key, never the legacy one.
2. **The writer emits only the current key.** `write_dimension_model`
   writes `/pdfcer`. A new document, or a legacy document's next save,
   is keyed the one way the current product spells its name.
3. **A legacy key is retired on the first write, in the same write.**
   `write_dimension_model` **removes** `/pdfce` beside the `/pdfcer` it
   inserts. The alternative — leave the old key in place — would let one
   document carry two sidecars that a later edit could make disagree
   (and a reader following clause 1 would silently prefer one of them).
   The other alternative — remove it on OPEN — would be a mutation the
   operator did not ask for, against §5's minimal-diff invariant and
   §11.1's commit-point-is-save rule. So the retirement rides the
   operator's own save, where the sidecar is being rewritten anyway.

**What this is NOT.** Not a migration tool, not a version bump —
`SIDECAR_VERSION` does not move, because the sidecar's *contents* are
unchanged; only the dictionary key naming its owner moved. Not a
promise to read `/pdfce` forever as a matter of policy — it is a promise
to read it for as long as a document written by a pre-rename build can
exist, which in practice is forever, and the fallback is two lines. Not
an exception to §5: an incremental save of a legacy-keyed document
re-emits the catalog object it was going to re-emit anyway (the sidecar
lives on the catalog), so the retirement costs no additional object.

**Why it is filed as a decision rather than a Pass detail.** The rename
was scoped, planned and dispatched as *mechanical* — `247.1`'s own step 1
says "one mechanical, case-preserving pass" — and the ledger's whole
argument for a rename being safe rested on it changing no behaviour.
That premise held everywhere except here, and it failed here for a
reason that generalises: **any identifier the product writes INTO a
document is part of that document's format, and renaming it is a format
change with a compatibility obligation, however small the diff.** The
candidates in this codebase, by the same reading, are the sidecar key
(this decision), `/Producer` (informational — a reader does not branch
on it, so no guard is owed), the OCProperties configuration name
*"pdfcer dimensions"* (display text, likewise), and the emitted resource
name prefix `pdfceF{n}` / `pdfceFm{n}` (**not renamed by `247.1` and not
listed among its deliberate keeps** — reported in `ROADMAP.md`'s `247.1`
Shipped entry as an owed disposition; if it is renamed later, this
decision's clause 1 does not apply because a resource name is opaque to
every reader, and nothing looks a document up by it).
**★ ADDENDUM 2026-09-03 (401st filing, `Pass 247.2`) — the `pdfceF{n}` /
`pdfceFm{n}` prefix is a KEEP, ruled by the engineer, and this decision
is the record of why no compatibility work is owed.** (1) They are
`/Resources` dictionary keys — ISO 32000-1 §7.8.3 leaves the name
arbitrary; its only property is uniqueness within its own dictionary,
and no reader, viewer or operator ever sees it. (2) The allocation
logic is *"first unused `/pdfceF{n}`"* and is prefix-agnostic: a
`/pdfceF3` written by a `v0.27.0` build and any name a later build
writes coexist in one dictionary with no rule between them, so there is
nothing for clause 1 to protect and nothing for clause 3 to retire.
(3) A rename would change every add-text and format output byte and
move ten files of tests, fixtures and generators (`grep -rl pdfceF
crates/ tools/`) for no operator-visible effect. **Scope statement:
this decision covers identifiers a saved document carries; it obliges
a compatibility guard only where a reader BRANCHES on the identifier.**
The sidecar key is looked up by name (guard owed, built); `/Producer`
and the OCProperties configuration name are display text (no guard);
a resource name is allocated, never looked up (no guard, and no rename
either — the cost is real and the benefit is nil). `pdfceF` in
`crates/` and `tools/` is therefore a correct survivor for every later
hard-rule-11 sweep.
*Measured at the moment of this addendum (hard rule 8): the working
tree of `D:\Dev\pdfcer` held an UNCOMMITTED engineer change removing
`SIDECAR_KEY_LEGACY`, `EditSession::sidecar_entry` and
`tests/sidecar_legacy_key.rs`, its doc comment citing an operator
ruling that nothing in production ever wrote `/pdfce`. That change is
not filed here and not this addendum's subject; if it ships, the
filing that ships it owes this decision a dated disposition — the
general rule (three clauses) can stand while its first instance is
withdrawn on the ground that the compatibility obligation had no
documents to protect.*

**Test discipline that made the guard checkable.** The legacy-keyed
fixture is manufactured at **equal byte length** to the current-keyed
one, so every xref offset in the file stays valid without regenerating
the table — the same trick the project's fixture generators use, and
the reason the fallback test exercises the real parser rather than a
hand-built object tree. Three tests: a fresh save carries only
`/pdfcer`; a legacy document opens with its ce dimension; saving it
retires the legacy key and the result re-opens with both ce dimensions
(the one it had, the one the test added).

**Body sections updated in this filing:** §3's ce-dimension subsection
(H) gains a paragraph naming the key, the fallback and the retirement,
beside its existing `SIDECAR_VERSION` note; §3's workspace-layout node
for the CLI crate corrected `pdfcer\` → `pdfcer-cli\` (a rename-script
artefact, not a decision — the directory is `crates/pdfcer-cli`, the
binary is `pdfcer`); §3's removed-GUI node's *"renamed from"* clause
restored to `pdfceGUI` (same artefact class). `FEATURES.md`'s
ce-dimension *author* row carries the one-sentence operator-facing
consequence. `ROADMAP.md`: `Pass 247.1` *Shipped* entry, item 3.

**Decision ceiling moves `130` → `131`; next free `132`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted**; next free `(ce)`.

### 2026-09-03 (403rd filing, `c549219`) — decision 132: **A RECORDER THAT REFUSES FOR CORRECTNESS (`R211`) GAINS A SIBLING MODE THAT NEVER REFUSES, FOR EXPORT — ONE INTERPRETER, TWO POSTURES, NOT A SECOND INTERPRETER. FIRST INSTANCE: `pdfcer_render::display_list`'S PLANNED "EXPORT" RECORDING MODE (`Pass 248.1`, SVG, IN PROGRESS)**

**(librarian filing, 403rd. No shell available — this decision is read
from `docs/export-and-copy-out-plan.md` §1 (committed `c549219`) and the
engineer's dispatch text, not from code: `Pass 248.1` has not shipped.
Minted anyway, on the same basis decision 073 recorded the GUI pause
before any replacement code existed — the design is already committed
and dated, even though the implementation is in progress.)**

**Why this is a decision and not only a Pass-local implementation
choice**, since the engineer who scoped `Pass 248.1` framed it as the
latter and invited disagreement: `record_page`'s refusal posture
(`PoisonReason`, §4.1 above, decision **084**, `R211`) is itself a
**standing invariant** — "a cache that replays a plausible wrong picture
is worse than none" — not a one-off implementation detail, and it
already has its own decision entry precisely because of that status.
What `Pass 248.1` is building is the **structural opposite** of that
invariant, living in the **same module**: a mode in which the recorder
must never fail to produce *something*, and instead **discloses** what
it approximated (rule 4 — fuzzy, never sneaky) rather than **refusing**.
A second posture in the interpreter that the first posture's own
decision record governs is exactly the kind of fact a future session
reading decision 084/`R211` needs to find from there, not rediscover by
reading `Pass 248.1`'s diff. Recording it now, ahead of the code, means
the *ARCHITECTURE.md* body text (§4.1, edited alongside this entry)
already carries the forward pointer the day the plan was committed,
rather than three filings after the fact — the exact drift `ARCHITECTURE.md`
§4 has been caught running behind shipped surface more than once.

**The decision, precisely.** `pdfcer_render::display_list::record_page`
gains a `RecordMode::{Cache, Export}` (or equivalent) parameter.
**`Cache` mode is `R211` verbatim, unchanged**: `sh`, shading patterns,
overprint composites, soft masks and any operator with no recordable
formulation cause a **refusal** (`PageNotRecordable`), because a wrongly
plausible cached picture the shell then pans and zooms is worse than a
fallback to `render_page_region`. **`Export` mode never refuses**: at
every one of those same poison sites, it rasterises that ONE operator,
at the recording scale, into a transparent scratch, and records the
result as an image fill — clipped to that operator's own device bounds,
never promoted to a whole-page fallback (a page-level poison must not
become a page-level rasterisation, or a page with one gradient becomes
a bitmap wearing a vector costume). **Every rasterisation export mode
performs is COUNTED**, so the SVG's own disclosure can state, off-canvas,
exactly what was approximated and how much of it there was — the same
obligation rule 4 places on every other pdfcer inference.

**What this decision does NOT cover, deliberately**: which specific
poison sites get a rasterised fallback vs. a future native SVG primitive
(axial/radial shadings as `<linearGradient>`/`<radialGradient>` is
named in the plan as "the obvious later upgrade, a refinement not a
prerequisite"); the SVG element vocabulary itself; the oracle-test
design (`resvg` comparison). Those are `Pass 248.1`'s own implementation
decisions, correctly the engineer's to make without a decision record
each.

**Scope beyond SVG.** `record_page` is `pdfcer-render`'s only
page-to-display-list interpreter; any future consumer that wants a
full-fidelity, non-refusing traversal of a page (a thumbnail generator,
a future PDF/A raster fallback, a print-preview cache warmed ahead of
first paint) inherits this same two-mode shape rather than growing its
own second interpreter or its own refusal policy — which is the second,
independent reason this belongs at the module's own decision level
rather than filed only under `Pass 248.1`.

**Body sections updated in this filing:** §4.1's "What this surface
REFUSES" subsection (near the `record_page` write-up) gains a dated
forward-pointer paragraph naming the coming `Export` mode and stating
explicitly that `Cache` mode and `R211` are unchanged by it.

**Decision ceiling moves `131` → `132`; next free `133`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted**; next free `(ce)`.

**★ AMENDED 2026-09-03 (404th filing, `80f1c3e`) — SHIPPED, and the real
names differ slightly from the placeholder this entry used.** `Pass 248.1`
shipped `RecordMode`'s two postures as `ExportState`/`ExportTally` +
`Recorder::new_for_export`, not the generic `RecordMode::{Cache, Export}`
sketched above — the design this decision records is otherwise unchanged.
**One thing the design did NOT anticipate and the implementation found**:
the `Cache`-mode recorder had a **pre-existing defect** this decision's own
subject matter made visible — an elementary object's `gs /SMask` was never
a poison site in `Cache` mode, because the mask had been folded into the
enclosing *clip*, which a recording never carries, so a cached replay of
such an object painted it unmasked. `Cache` mode now calls
`refuse(PoisonReason::SoftMask)` there too, closing the gap `R211`/decision
084 always intended to cover. See `ROADMAP.md`'s `Pass 248.1` *Shipped*
entry (404th filing) for the full build record, and the amendment to
§4.1's forward-pointer paragraph below.

### 2026-09-04 (414th filing) — decision 133: **SOURCING SIGNATURE TRUST ANCHORS FROM AN INSTALLED ACROBAT/READER (opt-in) — pdfce DIRECT-PARSES THE USER'S OWN `addressbook.acrodata` WITH ITS OWN COS + X.509 CODE, RATHER THAN MAINTAINING AN AATL BUNDLE OR DRIVING ACROBAT. OFF BY DEFAULT. SCOPES `Pass 10.2` (import) + `Pass 10.3` (evaluate)**

**(librarian filing, 414th. Docs-only scoping filing — no code shipped.
Recorded ahead of `Pass 10.2`/`10.3` on the same basis decisions 073 and
132 were recorded ahead of their code: the design is committed and dated,
the implementation is Backlog. Grounded in three RAG files created this
session — the PPKLITE format measured from the real specimen
(`D:\Dev\Rag-Specialized\PDF_Spec\security\security__ppklite_addressbook.md`),
the AATL/EUTL superset argument
(`D:\Dev\Rag-Specialized\Acrobat_Features\signatures__trust_anchor_sources_aatl_eutl.md`),
and the trust-flags + verdict pipeline
(`D:\Dev\Rag-Specialized\Acrobat_Features\signatures__trust_flags_and_verdict_pipeline.md`).
Citing them here closes the cross-RAG handoff — a deliverable is not
handed off until a pdfce doc names it.)**

**Why this is a decision and not only a Pass-local choice.** `Pass 10.1`
shipped signature verification with `trust = NotChecked` **by name** —
integrity and coverage as real facts, trust deliberately empty because
pdfce has no trust store. Closing that gap forces three choices that
outlive any one Pass: *where the anchors come from*, *how pdfce reads
them*, and *whether it is on by default*. Each is an invariant a future
session must find from here, not re-derive.

**1. WHERE — an installed Acrobat/Reader is the only 1:1 anchor source,
because AATL ⊇ Windows-roots ∪ EUTL by construction.** Adobe's AATL is
built by Adobe's own independent CA audit, with no reference to the
Windows Trusted Root program or to the EU Trusted List; it is a superset
of both **by how it is constructed**, not by coincidence, which is why
the operator measured Windows+EUTL at ~55.6% of AATL. Crucially there is
**no public, machine-consumable AATL bundle** to fetch and ship — so an
already-refreshed Acrobat/Reader install (which pulls the current AATL +
EUTL into its address book) is the only source that reproduces the anchor
set an Acrobat verdict would actually use. pdfce maintaining its own
bundle would mean maintaining a worse, staler copy of a list it cannot
authoritatively obtain.

**2. HOW — direct-parse, not automate.** pdfce reads
`%APPDATA%\Adobe\Acrobat\DC\Security\addressbook.acrodata` with its
**own COS parser and its own X.509 decoder**, rather than driving
Acrobat to emit a verdict. Two reasons. (a) The file is trivially within
pdfce's existing reach: it is a **classic-COS PPKLITE file** — header
`%PPKLITE-2.1`, classic 20-byte xref, a `/Type/Catalog` root, **no
streams, no object streams, no `/Encrypt`** — so the existing tokenizer
and classic-xref parser read it once the `%PDF-` header gate is relaxed
to also accept `%PPKLITE-`/`%FDF-` (one branch), and each entry's `/Cert`
is a raw DER X.509 the `Pass 10.1` decoder already handles (no new ASN.1).
(b) Acrobat's **automatable verdict surface is unproven and fragile**;
reading a static file the user already has on disk is deterministic and
depends on nothing pdfce cannot see. The schema and both of its
reference-resolution schemes (object refs in `/Entries`; `/ID` refs from
`/ABEType 2` groupings) are recorded in the PPKLITE RAG file; the
`/Trust` numeric bitfield constants are **unpublished by Adobe**, so
pdfce's bit→category mapping is **derived and must be disclosed as
provisional** (rule 4) — pinning it is named as a spec-research
dependency for `Pass 10.3`.

**3. WHETHER-BY-DEFAULT — off by default, explicit opt-in, disclosed.**
Legal posture (engineer's educated read, not counsel): reading the
**user's own already-downloaded file on their own machine** is clean on
the copyright/redistribution axis — the certs are public, the container
is an open-standard COS file, and pdfce redistributes nothing. The
residual fuzz is **contractual** — the Adobe Reader EULA on using its
data outside Reader — which the engineer judges **fuzzy, not blocking**.
Per the operator's own standing rule (*"leave it to the user to enable
if the legal question is fuzzy"*), the feature ships **opt-in /
off-by-default**, and an **operator EULA review is an OPEN ITEM that
gates any future enable-by-default** — it does not gate shipping the
opt-in capability.

**★★ THE EULA-REVIEW OPEN ITEM IN §3 IS RESOLVED 2026-09-04 (423rd
filing) — see decision 134 below.** The operator ruled that the EULA
gate is replaced by an **explicit at-own-risk opt-in setting**, off by
default: pdfcer does not undertake an EULA review at all, because Adobe
could change the EULA after any review, so the durable posture is user
consent per-setting rather than a pdfcer legal determination. The
"enable-by-default" that §3 said this item gated is simply **not a
destination** — the feature stays operator-enabled, at the operator's own
risk. This paragraph is kept as written for its history; decision 134 is
the current text.

**Crate boundary.** The `.acrodata` **reader** is `pdfcer-core` (COS +
X.509, no GUI, local file read only, no network — the GUI-core-separation
and no-network invariants both hold). The **locator** that finds the
Acrobat/Reader Security dir across track variants (Acrobat vs Reader; DC
vs 2020/2017) is **shell-side** (CLI + a setting), because it is
platform- and install-specific policy, not object-model logic.

**Scope produced:** `Pass 10.2` (import — a `.acrodata` reader → a
`TrustAnchorSet`, `/Source` filtering, a fuzz target, a shell-side
locator, a read-only list CLI, disclosure of source-counts + store
freshness + the provisional `/Trust` mapping) and `Pass 10.3` (evaluate
— chain a signer to a `TrustAnchorSet` anchor, turning `NotChecked` into
a real verdict, "signer unknown" kept DISTINCT from "valid but
untrusted", revocation/clock named as separate later increments). Both
Backlog, under the Digital-signatures leg. Revocation and timestamp are
**not** at address-book level — they live in each cert's DER extensions
(RFC 5280 CDP/AIA) and are recovered by decoding `/Cert`.

**Decision ceiling moves `132` → `133`; next free `134`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted here** — the Adobe-Reader-EULA review is filed as
an OPEN ITEM on `Pass 10.2`, not as a lettered operator question; next
free `(ce)`.

### 2026-09-04 (423rd filing) — decision 134: **THE ADOBE-READER-EULA REVIEW (decision 133 §3's open item) IS REPLACED BY AN EXPLICIT AT-OWN-RISK OPT-IN, OFF BY DEFAULT, DISCLOSED — NOT A pdfcer LEGAL DETERMINATION. RESOLVES decision 133 §3.**

**(librarian filing, 423rd. Recorded with `Pass 10.3`, which shipped the
CLI opt-in that this decision governs — `verify-signatures
--trust-from-acrobat`, code `55062c5`.)**

**The open item.** Decision 133 §3 shipped the Acrobat trust-store import
**opt-in / off-by-default** and named an **operator Adobe-Reader-EULA
review** as an OPEN ITEM that would gate any future *enable-by-default*.
That item is now resolved.

**The operator's ruling, verbatim (2026-09-04):** *"they could change the
eula after this. we just need the user to set a setting that allows at
their own risk."*

**What it decides.** The EULA gate is **removed and replaced by user
consent per-setting**, for a reason that outlives any one review: **an
EULA review is not durable** — Adobe can change the terms after pdfcer
reviews them, so a one-time legal reading would be stale the moment it is
filed. The durable posture is therefore **not** a pdfcer legal
determination about the EULA; it is an **explicit operator opt-in, off by
default, at the operator's own risk, disclosed**. The user consents each
time (per invocation) or once (per persistent setting); pdfcer states the
risk and does not decide it.

**Consequences.**

- **"Enable-by-default" is no longer a destination.** Decision 133 §3
  framed the EULA review as the gate in front of flipping the default;
  with this decision there is no default-on to gate — the feature stays
  operator-enabled, always. The gate is dissolved, not merely satisfied.
- **The CLI opt-in is the per-invocation consent:** `pdfcer
  verify-signatures --trust-from-acrobat` (shipped `Pass 10.3`,
  `55062c5`), which prints the **at-your-own-risk** disclosure.
- **The persistent consent is a SETTING**, scoped as `Pass 10.4`
  (*Backlog*): an `AcrobatTrustStore { Off, AtOwnRisk }` setting in
  `pdfcer_core::settings`, off by default, read by the CLI as its default
  and by the GUI's security tab. A setting, not an inference gate — so no
  accept/reject flow (rule 4); the at-own-risk text is the disclosure.
- **This is the operator's standing "leave a fuzzy legal question to the
  user to enable" rule applied once more** (the same rule decision 133 §3
  cited), now made concrete: the vehicle of that consent is a named
  setting, and the EULA-review branch of §3 is closed.

**Decision ceiling moves `133` → `134`; next free `135`.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted; the resolved item was an OPEN ITEM on `Pass 10.2`,
never a lettered question — next free `(ce)`.**

### 2026-09-04 (427th filing) — decision 135: **SIGNATURE REVOCATION (CRL/OCSP) IS ARCHITECTURALLY EXCLUDED FROM `pdfcer-core` BY THE NO-NETWORK INVARIANT. `pdfcer-core` VALIDATES ONLY OFFLINE REVOCATION EVIDENCE — EMBEDDED DSS/LTV DATA AND SHELL-SUPPLIED OCSP/CRL RESPONSES — AND DECODES CDP/AIA URLs FOR A SHELL TO FETCH; THE ACTIVE FETCH IS A SHELL'S JOB. SCOPES `Pass 10.6`.**

**(librarian filing, 427th. Recorded with `Pass 10.5` (`e1cdd3b`), which
shipped the deterministic offline trust checks and left
`PathChecks.revocation_checked = false` unconditionally; the decision it
records is why that field is a constant this build, and what a real
revocation Pass must look like.)**

**The architecture point.** `Pass 10.5` added the deterministic, no-network
half of RFC 5280 path validation to `trust_chain::evaluate` — certificate
validity dates against the signing-time clock (§4.1.2.5), CA/`keyUsage`
constraints on intermediates (§4.2.1.9 / §4.2.1.3), and RSA-PSS certificate
signatures (RFC 4055). It did **NOT** add revocation, and the reason is
structural, not scheduling: **CRL and OCSP are network fetches, and
`pdfcer-core` is forbidden the network** (the no-network invariant — see the
`no-network` fail-closed CI job, a `cargo tree` denylist against
`reqwest`/`hyper`, §1.1 above). A crate that cannot open a socket cannot
fetch a CRL or query an OCSP responder.

**What it decides.** Revocation does not become "unsupported"; it becomes a
**layered** capability whose active-fetch step lives outside core:

- **`pdfcer-core` validates OFFLINE revocation evidence** — the CRLs/OCSP
  responses a PAdES B-LT/B-LTA document already carries in its `/DSS`
  dictionary (ETSI EN 319 142), and CRL/OCSP responses a **shell** hands it
  after fetching them. Validation is arithmetic over bytes already in hand;
  it is offline; it belongs in core.
- **`pdfcer-core` decodes, never fetches, CDP/AIA** — it reads each cert's
  DER `CRLDistributionPoints` / `AuthorityInfoAccess` extensions and
  surfaces the URLs for a shell to fetch. Decoding a URL is offline;
  dereferencing it is the shell's act.
- **The active fetch is a SHELL's job** — a GUI or CLI that IS permitted the
  network (the operator's 2026-08-08 ruling narrowed the no-network rule to
  core + render specifically; a shell may fetch) retrieves the CRL/OCSP and
  passes the response back into core to validate.

**Why this does not alter the invariant body.** This decision is an
**application** of the existing no-network invariant, not a change to it —
§1.1 already states `pdfcer-core`/`pdfcer-render` make no network calls, and
that text is unchanged. The decision records the *consequence* for signature
trust: revocation is drawn on the core/shell boundary, the same boundary
GUI-core separation draws for windowing.

**Disclosure stays honest (rule 4).** `Pass 10.5`'s verdict note and CLI
output state exactly which checks ran and that revocation is not among them;
`PathChecks.revocation_checked` is `false` this build and is not hidden.
Every uncertainty still resolves to `Untrusted`, never a false `Trusted` —
the `Pass 10.3`/`10.5` safety direction is preserved.

**Scopes `Pass 10.6`** (*Backlog*): the three offline-respecting routes
above, plus pinning the provisional `/Trust` bitfield decoding (provisional
since `Pass 10.2`).

**Decision ceiling moves `134` → `135`; next free `136`.** **Standing rules
ceiling `R241` — unchanged**, next free `R242`. **Open operator questions:
none minted — next free `(ce)`.**

### 2026-09-05 (436th filing) — decision 136: **THE SIGNING ARC'S SHAPE — PAdES B-B FIRST, CAdES (`/SubFilter /ETSI.CAdES.detached`) AS THE DEFAULT FORMAT, A PKCS#12 FILE AS THE FIRST KEY SOURCE; EVERY KEY SOURCE COLLAPSES TO HASH-IN / SIGNATURE-OUT BEHIND A `Signer` TRAIT IN `pdfcer-core` WHOSE ONLY IN-CORE IMPLEMENTATION IS `Pkcs12Signer` — WINDOWS-STORE, PKCS#11 AND CLOUD SIGNERS ARE SHELL-SIDE. THE CRATE STACK IS NOT DECIDED HERE. SCOPES `Pass 10.7`–`10.11`.**

**(librarian filing, 436th. Records the operator's approval of the arc's
shape and the engineer's architecture for it; nothing built yet. The
decision the crate survey produces is a SEPARATE record — `137`, claimed
here by name, not yet authored — owed before the `Cargo.toml` change now sitting uncommitted in the
working tree is committed, per rule 13.)**

**Provenance — stated exactly, so a later reader does not over-read it.**
The operator's words on record are the batch ruling, verbatim: *"build all
before the next portable release unless I say otherwise."* The signing
SHAPE below was the engineer's proposal, put to the operator on 2026-09-05
and **approved** — recorded by the engineer in `docs/NEXT_SESSION.md` at
`26f0257` (*"operator approved the shape"*, §2) and carried into the
2026-09-05 second-session handoff (`94f67ee`). No further operator
quotation exists and none is invented here. The approval covers building
`Pass 10.7`–`10.9` in the current batch, before the `0.40.0` release.

**What it decides — five points.**

1. **Order:** PAdES **B-B** first (the only level `pdfcer-core` can
   produce with no network — `pades__ref__creation_by_level.md` `PC-1`);
   then Windows-certificate-store and PKCS#11 key sources; then **B-T**
   (a supplied RFC 3161 token); then **B-LT/B-LTA**, which wait on
   `Pass 10.6` (revocation material). Each later stage is a further
   incremental update on the same signature (`PC-11`), never a rewrite.
2. **Format default: CAdES.** `/SubFilter /ETSI.CAdES.detached` with the
   ESS `signing-certificate-v2` attribute and NO CMS `signing-time`
   (`PC-2`, `PC-3`); `adbe.pkcs7.detached` remains an option. **This
   diverges from Acrobat's out-of-the-box default (legacy PKCS#7 via
   `aSignFormat`; "CAdES-Equivalent" is opt-in) on purpose** — the Acrobat
   RAG's own `DISCHARGED [2026-09-05]` note, and the standing memory rule
   that parity is a floor: pdfcer has no installed base to stay compatible
   with, and a fresh Acrobat's output *"is not conformant with the PAdES
   baseline profile"*. SHA-256 default; SHA-1/MD5 never authored; RSA
   PKCS#1 v1.5 default with RSASSA-PSS and ECDSA options (Acrobat: PSS is
   opt-in).
3. **First key source: a `.pfx`/PKCS#12 file.** The only one of Acrobat's
   four ID source classes where the raw private key is *legitimately* in
   application memory (`signatures__digital_id_sources.md` (a)), so B-B
   from a file is fully self-contained — no network, no OS key store, no
   device.
4. **★ The load-bearing architecture: one `Signer` trait, hash in /
   signature out.** `sign(digest) → signature` and `certificate_chain()`
   in `pdfcer-core`, plus — the librarian's note from `CB-5`, for the
   engineer to shape — the algorithm identity the bytes carry. Three of
   the four source classes are *"send a hash, receive a signature"*
   (CNG `NCryptSignHash`, PKCS#11 `C_Sign`, CSC), so the trait takes the
   digest, not the message, and the PKCS#1 v1.5 `DigestInfo` wrap lives
   inside the signer. **`Pkcs12Signer` is the ONLY in-core
   implementation.** Windows-store, PKCS#11 and any cloud signer are
   **shell-side** implementations of the same trait — the key never leaves
   its custodian, and `pdfcer-core` gains no OS key store, device driver or
   network client. One pipeline for every source; a second pipeline per
   source is the drift the trait exists to prevent.
5. **Signing is the canonical incremental-save case** (R36, `SC-7`): the
   verb refuses a full rewrite by name, writes `/SigFlags 3`, uses the
   two-pass zero-filled `/Contents` hole with `/ByteRange` reaching EOF
   (`SC-2`, `SC-3`), never shrinks the hole (`SC-6`), never encrypts
   `/Contents` (`SC-8`), and **self-verifies with the `Pass 10.1` verifier
   before returning** — the verifier already knows the `0x31` retag and is
   the natural oracle. Refusals by name, sourced from Acrobat: an
   encrypted document is PERMISSION-BIT-gated, not blanket; `/DocMDP /P 1`
   forbids any further signature; a SECOND certification is refused
   regardless of `/P`.

**Why this is an application of existing invariants, not a change to
them.** §1.1's engine row (network-free, gate-enforced) and §3's GUI-core
separation already forbid a network client and a windowing dependency in
`pdfcer-core`; decision 135 applied the same boundary to revocation
(shell fetches, core validates). This decision applies it to the private
key (shell custodian signs, core assembles) and to the TSA round trip
(shell fetches, core embeds). No invariant text changes; §1.1 gains a
pointer and §5 gains §5.13 so the consequence is findable from the body,
not only from this log.

**Disclosure (rule 4/11).** `/M` is caller-supplied; when the CLI derives
it from the system clock it PRINTS `m_source=system-clock`. The PAdES level
ACTUALLY produced is printed (`level=B-B`; `B-T` only with a verified
token) — `PC-12`, and the exceed-Acrobat point both Acrobat RAG files
name: Acrobat's no-TSA, clock-derived time is visually indistinguishable
from a TSA time.

**What it does NOT decide, by name.** (a) **The crate stack** —
`docs/signing-crate-survey.md` (untracked at filing) and the uncommitted
`Cargo.toml` adopting `rsa 0.10.0-rc.18`, `p256`/`p384 0.14`,
`signature`, `rand_core`, `sha1`/`hmac`/`pbkdf2`/`des`/`rc2` behind a
`signing` feature are the engineer's working state, **not a decision until
record `137` says so** with the Marvin advisory, the pre-1.0 pin, the
wasm32 posture and the `cms`-builds-vs-in-house-writer question
addressed, and `PRIOR_ART.md`'s rows amended in the same commit (rule 13).
(b) The signature appearance composer (Name/Date/Reason/DN, graphic,
watermark). (c) `/FieldMDP` lock dictionaries. (d) Cloud/CSC signing —
covered by the trait, unscheduled. (e) B-LT/B-LTA — filed when `Pass 10.6`
ships.

**Scopes** `Pass 10.7` (PKCS#12 import + the trait), `Pass 10.8` (CMS
`SignedData` build), `Pass 10.9` (the PDF-level write + `pdfcer sign`) —
*Next up*; `Pass 10.10` (shell-side key sources), `Pass 10.11` (B-T) —
*Backlog*. Body: §5.13 (new), §1.1 (pointer).

**Decision ceiling moves `135` → `136` (minted). `137` is CLAIMED by name
above for the crate-stack record and NOT YET AUTHORED — `check-ledger-numbers`
reads a §12 mention as spoken for and therefore reports ceiling `137`, next
free `138` (precedent: `037`/`038`, claimed before authored). Author `137`;
do not skip to `138`.** **Standing rules ceiling `R241` — unchanged**, next
free `R242`. **Open operator questions: none minted — next free `(ce)`.**

### 2026-09-05 (437th filing) — decision 137: **THE SIGNING CRATE STACK — the survey's LEAN stack is ADOPTED behind a default-ON `signing` feature: `rsa 0.10.0-rc.18` (blinded `Randomized*` paths ONLY, `getrandom` OFF), `p256`/`p384 0.14` (RFC 6979), `signature`, `rand_core`, and the PKCS#12 import plumbing `sha1`/`hmac`/`pbkdf2`/`des`/`rc2`; CMS/DER building and PKCS#12 parsing are IN-HOUSE on `asn1.rs`; `cms`, `pkcs12`, `pkcs5`/`pkcs8[encryption]`, `x509-cert`, `p12-keystore`, `p12` and `ring` are NOT taken. RUSTSEC-2023-0071 ("Marvin") is OPEN against every `rsa` version and is ACCEPTED FOR SIGNING, with the reasoning recorded here. This is decision 129's stated other half. Rule 13: every crate permissive; `THIRD_PARTY_LICENSES.md` regenerated.**

**(librarian filing, 437th. Authors the record decision 136 CLAIMED by
name; the engineer's decision of 2026-09-05, sourced to
`docs/signing-crate-survey.md` — 513 lines, dated 2026-09-05, written by a
research agent this session, cited below by section — and to the working
tree this role READ: `git diff crates/pdfcer-core/Cargo.toml`, the
`crates/pdfcer-core/src/sign/` module. Rule 13's order is honoured
because the tree was UNCOMMITTED when the 436th filing observed it and is
still uncommitted at this filing — `git status --short` shows ` M
Cargo.lock`, ` M THIRD_PARTY_LICENSES.md`, ` M crates/pdfcer-core/Cargo.toml`,
` M crates/pdfcer-core/src/lib.rs`, ` M crates/pdfcer-core/src/signature_verify.rs`,
`?? crates/pdfcer-core/src/sign/`, `?? crates/pdfcer-core/tests/pkcs12_import.rs`,
`?? docs/signing-crate-survey.md` — and, by the time the gates ran minutes
later, the engineer's CONCURRENT work had added ` M crates/pdfcer-cli/Cargo.toml`,
` M crates/pdfcer-cli/src/main.rs`, ` M crates/pdfcer-core/src/edit.rs`,
` M docs/core-api/02-editing-and-saving.md`, `?? crates/pdfcer-core/tests/sign_document.rs`.
The tree is moving while this is filed. The engineer commits all of that
AFTER this record lands; this filing stages five docs files by name and
nothing under `crates/`, `Cargo.toml`, `Cargo.lock`, `THIRD_PARTY_LICENSES.md`,
`fixtures/`, `docs/core-api/` or the survey.)**

**What decision 129 left open, stated so the two records read as one.**
Decision 129 kept signature VERIFICATION in-crate — `crypto/bignum.rs`,
`crypto/rsa.rs`, `crypto/ecdsa.rs`, `asn1.rs`, `cms.rs` — on the argument
that constant-time code protects a SECRET and verification handles none;
each of those headers says the judgement *"does NOT extend to signing"*, and
`PRIOR_ART.md`'s `num-bigint` and `p256`/`p384` rows closed with *"Still
the crate to reach for if SIGNING ever ships"* and *"Signing, if it ever
ships, gets the constant-time dependency — the `aes` argument, not the
`md5` one."* **Signing now ships (`Pass 10.7`–`10.9`, decision 136), a
private key IS handled, and this record is that clause coming due.** It is
the decision-039 shape — an `unsafe`-carrying, cfg-selected constant-time
backend accepted because the property it buys protects a secret — applied a
third time (`aes` and `sha2` were the first two, §9).

**What is decided — the stack, exactly as the `Cargo.toml` diff carries
it.** A new Cargo feature `signing` on `pdfcer-core`, in the existing
`[features]` block's strippable-capability convention (default ON; the
`--no-default-features` build CI already runs still compiles; the gated
path refuses by name). `default = ["jpx", "ocrs", "signing"]`;
`signing = [dep:rsa, dep:p256, dep:p384, dep:signature, dep:rand_core,
dep:sha1, dep:hmac, dep:pbkdf2, dep:des, dep:rc2]`, every one
`optional = true`, `default-features = false`:

| Crate | Pin | Features | Role | Survey |
|---|---|---|---|---|
| `rsa` | `0.10.0-rc.18` | `sha2`, `encoding` — **NOT `getrandom`** | RSA PKCS#1 v1.5 and RSASSA-PSS private-key operation on `crypto-bigint 0.7` constant-time Montgomery modexp | §2 |
| `p256`, `p384` | `0.14` | `ecdsa`, `pkcs8` | ECDSA P-256 / P-384, RFC 6979 deterministic `k`; `pkcs8` = `PrivateKeyInfo` import from the `.pfx` key bag | §3 |
| `signature` | `3.0` | — | the signer traits `rsa`/`ecdsa` implement | §1 |
| `rand_core` | `0.10` | — | the `TryRng`/`TryCryptoRng` traits pdfcer's own RNG adapter implements | §2 |
| `sha1` | `0.11` | — | a `digest::Digest` impl for the PKCS#12 KDF and legacy HMAC-SHA1 MAC (the in-crate `crypto/sha1.rs` has no `Digest` impl) | §4 |
| `hmac` | `0.13` | — | PKCS#12 MAC verification; PBKDF2 PRF | §4 |
| `pbkdf2` | `0.13` | `hmac` | PBES2 key derivation (modern `.pfx`) | §4 |
| `des` | `0.9` | — | `TdesEde3` for `pbeWithSHAAnd3-KeyTripleDES-CBC` (legacy key bag) | §4 |
| `rc2` | `0.9` | — | `Rc2::new_with_eff_key_len(k, 40)` for `pbeWithSHAAnd40BitRC2-CBC` (legacy cert bags) | §4 |
| `cbc` (already present) | `0.2.1` | **gains** `alloc`, `block-padding` | PKCS#7 unpadding of the decrypted bags | §8 |

**Measured (survey §7/§8, scratch member `m_lean`; the survey's method is
stated in its own header):** the LEAN stack resolves **48 unique crates, 32
new to pdfcer's `Cargo.lock`, 18 `unsafe`-carrying crates of which 11 are
genuinely new** — `cmov` (25 lines, **3 `asm!` files**), `crypto-bigint`
(14), `cpufeatures 0.3.1` (11, 1 asm — `0.3.0` already present), `sha1` (7,
1 asm), `base16ct` (4), `base64ct` (4), `der` (4), `elliptic-curve` (4),
`const-oid` (1), `pem-rfc7468` (1), plus `aes 0.9.2 → 0.9.3` as a
semver-compatible bump. `rsa`, `p256`, `ecdsa`, `pkcs8`, `spki`,
`signature`, `pkcs1`, `sec1`, `rfc6979`, `des`, `rc2`, `hmac`, `pbkdf2`,
`rand_core`, `digest` are all 0-`unsafe`. **One pre-release pin** (`rsa`,
which drags `pkcs1 0.8.0-rc.4`); every ECDSA crate is the Jul-2026 stable
wave (§0 finding 8). **No second `sha2`/`digest`**: the whole stack is
`sha2 0.11.0` / `digest 0.11.3` / `signature 3.0.0` / `rand_core 0.10.1`,
`cargo tree -d` on the full member printed *"nothing to print"* (§6). In
pdfcer's REAL tree at this filing (this role's `cargo tree -p pdfcer-core
-e normal`): `rsa v0.10.0-rc.18`, `p256`/`p384 v0.14.0`, `signature
v3.0.0`, `rand_core v0.10.1`, `sha1 v0.11.0`, `hmac v0.13.0`, `pbkdf2
v0.13.0`, `des v0.9.0`, `rc2 v0.9.0`, `der v0.8.2`, `crypto-bigint v0.7.5`,
`cmov v0.5.4`; **one** `sha2 v0.11.0`, **one** `digest v0.11.3`;
`getrandom` at **`v0.2.17` only** — no `0.4`. `Cargo.lock` **+406 lines**,
`THIRD_PARTY_LICENSES.md` **+1,108 / −42** (`git diff --numstat`, this
role), regenerated by `cargo-about`.

**★ RUSTSEC-2023-0071 ("Marvin Attack") — OPEN against every `rsa`
version, "no patched versions", last modified 2026-04-25; ACCEPTED FOR THE
SIGNING PATH. The reasoning, from survey §2 in substance:**

1. **The advisory's residual channel is a DECRYPTION oracle, and signing
   never executes it.** Issue `rsa#19` (*modpow not constant-time*) **closed
   2026-01-07** on the `crypto-bigint 0.7` migration (`BoxedMontyParams`
   constant-time `pow_mod_params`). The same day `rsa#626` (*padding
   implementation is not constant-time*) opened and is still open; its fix,
   PR `#680` (*implicit rejection for PKCS#1 v1.5 decryption*), is unmerged.
   On 2026-06-01 a contributor demonstrated an end-to-end Bleichenbacher
   attack against rc.18 **`decrypt`** that fails against `#680`. That
   channel is the PKCS#1 v1.5 **de-padding** step of decryption — the
   attacker submits chosen ciphertexts and times the unpad. **Signing has
   no de-padding step**: EMSA-PKCS1-v1_5 (RFC 8017 §9.2) and EMSA-PSS encode
   a PUBLIC digest deterministically; the secret enters only in the modexp,
   which (1) above made constant-time.
2. **pdfcer signs ONLY through the blinded paths.** Survey §0 finding 5:
   `rsa`'s plain `Signer`/`DigestSigner`/`PrehashSigner` impls pass
   `rng = None` and **skip blinding** (`pkcs1v15/signing_key.rs:96–125,
   176–195`); only `RandomizedSigner`/`RandomizedDigestSigner`/
   `RandomizedPrehashSigner` blind. The working tree honours this:
   `sign/mod.rs:332` imports `rsa::signature::RandomizedDigestSigner` and
   feeds it `PdfcerRng` (`mod.rs:402`/`:422`, `rand_core::TryRng` +
   `TryCryptoRng`) over `crate::crypto::rng::fill`. The module header
   (`mod.rs:44`) states the rule.
3. **The advisory's own workaround describes the deployment.** *"Avoid using
   the crate in environments where attackers can observe timing data
   (local, non-compromised systems are considered safe)."* An operator
   clicking *Sign* in a desktop application, or running `pdfcer sign` on a
   file, is that environment; the Kocher-style attack that would remain
   needs an attacker able to trigger many signatures with chosen inputs and
   observe timing.
4. **The exposure is DISCLOSED, not zero, and re-checked on every `rsa`
   bump.** The `Cargo.toml` dependency block carries the reasoning inline
   (*"Read the survey §2 before touching this"*); `PRIOR_ART.md`'s `rsa` row
   is amended in this filing; `docs/signing-crate-survey.md` §9 risk 1 is
   the standing watch item. The `rsa 0.9.x` stable line is **not** an
   escape — it is the `num-bigint-dig`, NOT-constant-time implementation
   (§1), strictly worse for a private key.
5. **`signing` OFF is the build that keeps the advisory out of the tree
   entirely.** A consumer who wants no `rsa` in their graph has the
   feature switch; verification is deliberately NOT gated and still runs.
6. **No `cargo audit` / `cargo deny` gate exists in CI today** — measured
   by this role: `grep -n "cargo audit\|cargo-audit\|cargo deny\|cargo-deny\|
   rustsec" .github/workflows/ci.yml` returns nothing. **If one is ever
   added, it carries `ignore = ["RUSTSEC-2023-0071"]` with points 1–5 as
   the rationale**, and the ignore is re-justified at each `rsa` bump, not
   carried forward silently.

**wasm32 posture.** `rsa`'s `getrandom` feature is deliberately OFF: it
drags `getrandom 0.4.3`, which **fails on `wasm32-unknown-unknown` without
`wasm_js`** (survey §2, measured), and would be a second `getrandom` beside
pdfcer's target-gated `0.2`. RSA signing on wasm32 therefore refuses —
`SignError::RandomUnavailable` (`sign/mod.rs:179`), raised BEFORE any key
arithmetic (`mod.rs:388`–`398`) — exactly as encryption authoring already
refuses there (`crypto/rng.rs`). **ECDSA signing WORKS on wasm32**: RFC 6979
deterministic `k` needs no RNG (§3). `cargo check -p pdfcer-core -p
pdfcer-render --target wasm32-unknown-unknown` — the engineer reports
*Finished* after the edit, **and this role re-ran it on the working tree
at filing: `Finished` in 13.51 s, exit 0** (8 pre-existing `pdfcer-core`
lib warnings, not this filing's). GUI-core and no-network invariants
**measured to hold** by this role: `cargo tree -p pdfcer-core -p pdfcer-render -e normal`
contains no `egui`, `eframe`, `winit`, `wgpu`, `reqwest` or `hyper`.

**CMS/DER building is IN-HOUSE — the `Pass 10.8` question answered.** The
RustCrypto `cms` crate's `builder` feature **does not compile** against
today's resolution — neither `0.3.0-pre.1` nor `-pre.2`, native or wasm32,
**11 errors** (`unresolved import cipher::crypto_common`,
`Array::generate_from_rng` missing, `PublicKey::to_encoded_point` missing);
the Jan-2026 pre-releases predate the Jun/Jul-2026 finals of their own
dependencies (§0 finding 1, §5). Its types-only half compiles but is
~150 lines of `#[derive(Sequence)]` structs behind a pre-release pin
whose exact value `pkcs12` would dictate (`=0.3.0-pre.1`). So the encoder
is `crates/pdfcer-core/src/sign/der_out.rs` — **262 lines by `wc -l`**
(the dispatch's *"~170"* and the file's own *"two hundred lines"* are both
low; filed as measured) — a DER writer with definite lengths, minimal
INTEGER, and **X.690 §11.6 `SET OF` ordering** (`der_out.rs:25`, `:65`),
tested against the existing `asn1.rs` reader so the verifier and the
writer agree byte-for-byte. **`der 0.8` enters the tree transitively
anyway** (via `rsa/encoding` and `p256/pkcs8`) **and is NOT used
directly** — `grep -rn "der::" crates/pdfcer-core/src/sign/` returns
nothing — so **no foreign type crosses a `pub` signature** of
`pdfcer-core`; `der_out.rs`'s header gives the reason (*"a foreign type
system between pdfcer's ASN.1 reader and its writer, and the two must
agree byte-for-byte"*). Survey §5's fallback is what shipped; its stated
risk (strict verifiers reject a mis-sorted `SET OF` while OpenSSL and
pdfcer's own reader accept it) is why `Pass 10.8` criterion 8 demands the
`openssl cms -verify` oracle and the insertion-order sabotage test.

**PKCS#12 parsing is IN-HOUSE on `asn1.rs`** —
`crates/pdfcer-core/src/sign/pkcs12.rs` (**1,046 lines** by `wc -l`,
including tests), with the RFC 7292 Appendix B.2 KDF written from the RFC
(`pkcs12.rs:690`) and **verified against the `pkcs12` crate's published
test vector** (`pkcs12.rs:1016`–`1022`, `kdf_matches_the_reference_vector`:
password `"ge@äheim"` as BMPString, salt `0102030405060708`, 100 rounds,
SHA-256, ids 1/2/3 — the crate's own vector, itself cross-checked against
OpenSSL; RFC 7292 publishes none). Not taken, each for the survey's reason
(§4 coverage matrix, §8 item 5): **`pkcs12 0.2.0-pre.0`** — parses and
derives keys but **does not decrypt** (`// todo: add decryption support`,
`// todo: add RC2 support`), has no MAC verify, and **hard-pins `cms
=0.3.0-pre.1`** (finding 2); **`pkcs5 0.8.1` / `pkcs8[encryption]`** —
**no PBES1 decryption at all** (`Error::NoPbes1CryptSupport`
unconditionally; the PKCS#12 PBE OIDs are absent from its enum), and its
`pbes2` feature drags 8 AEAD/scrypt crates no `.pfx` uses; **`x509-cert
0.3.0`** — certificates pass through as raw DER from the `.pfx` into the
CMS `certificates` set, and the in-crate `cms.rs` X.509 parser already
exists for the leaf's issuer/serial; **`p12-keystore 0.3.1`** — 78 crates,
duplicates `asn1.rs` with `x509-parser`/`asn1-rs`/`nom`/`time`, and
**fails wasm32** (`rand 0.10` → `getrandom 0.4.3`); its `src/pbes1.rs`
(68 lines, MIT/Apache-2.0) was a READ reference for the legacy path —
**★ the dispatch said it is credited in `pkcs12.rs`'s header; it is NOT**
(`grep -rn "p12-keystore\|pbes1.rs" crates/pdfcer-core/src/sign/` → no
hits, this role). The survey's instruction was *"credit it in the doc
comment IF structure is borrowed"*; whether structure was borrowed is the
engineer's to state, and if it was, the credit is owed in the same commit
as the code; **`p12 0.6.3`** — last release 2022-02-18, the previous
RustCrypto generation (`digest 0.10`), unconditional `getrandom 0.2`,
would reintroduce a second `sha1`/`hmac`/`cipher`/`des`/`cbc` line;
**`ring`** — unchanged from `PRIOR_ART.md` (C/asm, hurts WASM).

**Rule 13.** Every crate above is `MIT OR Apache-2.0` or `Apache-2.0 OR
MIT` (survey §1, `cargo info` 2026-09-05) — permissive, no copyleft, the
operator-escalation clause is not engaged. `THIRD_PARTY_LICENSES.md`
regenerated via `cargo-about` (+1,108 / −42). **`docs/DEPENDENCIES.md`** —
§9's purpose-shaped companion — gains the ten rows and closes its
*"that decision is deliberately not made here"* sentence in this filing.

**What this decision does NOT decide, by name.** (a) Whether `signing`
stays default-ON in the shipped portable folder if an audit gate is ever
adopted — today it does; the question is filed as the re-check on each
`rsa` bump, not as an open operator question. (b) The `rsa 0.10.0` FINAL
migration when it lands (rc.11 → rc.18 took four months and one `pkcs8`
major; expect API churn). (c) SHA-384/512 as signing options (`Pass 10.8`
criterion 6 leaves them to the engineer). (d) Anything about
`Pass 10.10`'s shell-side signers — they implement the trait and take no
crate from this list.

**Observation for the engineer, not a decision (crates/ is outside this
role's remit):** `lib.rs` gates the ENTIRE `sign` module
(`#[cfg(feature = "signing")] pub mod sign;`), including the `Signer`
trait and `SignatureAlgorithm`. A shell-side `Signer` implementation
(`Pass 10.10`) therefore requires the `signing` feature ON in
`pdfcer-core` even though it takes no private-key crate — the trait and
the private-key impls share one gate. Fine for now (default ON); worth a
sentence in `Pass 10.10`'s scoping if a lite build is ever expected to
sign through a custodian.

**Working tree at filing, measured, for the record — NOT a Pass status
claim:** `crates/pdfcer-core/src/sign/` holds `mod.rs` 510, `pkcs12.rs`
1,046, `der_out.rs` 262, `cms_build.rs` 204, `apply.rs` 465 = **2,487
lines over five files**, plus `tests/pkcs12_import.rs` 118 (`wc -l`). The
presence of `cms_build.rs` and `apply.rs` means the engineer's tree is
already into `Pass 10.8`/`10.9` territory; those Passes ship when the
engineer says so and the librarian files them — this record decides the
crate stack only.

**Body sections:** §5.13 (pointer added: the stack is decided), §9 (new
paragraph — the sixth dependency set, decision-039 shape, third
application), `docs/PRIOR_ART.md` (Cryptography rows amended and added;
decision-log entry), `docs/DEPENDENCIES.md` (§2 `pdfcer-core` rows, §4
verification paragraph closed).

**Decision ceiling: `137` AUTHORED (claimed by 136, now written) — the
ledger gate's reported ceiling `137` / next free `138` is unchanged in
number and changed in meaning: `137` is no longer a claim.** **Standing
rules ceiling `R241` — unchanged**, next free `R242`. **Open operator
questions: none minted — next free `(ce)`.**

### 2026-09-05 (440th filing) — decision 138: **DECISION 039'S `sha2` ACCEPTANCE IS EXTENDED TO THE `oid` FEATURE — `rsa`'s optional `sha2` dependency is declared with `features = ["oid"]`, so `pdfcer-core` now carries `sha2 feature "oid"` on every target; `oid` = `digest/oid` = `const-oid` `AssociatedOid` impls for the SHA-2 types and NOTHING ELSE (no backend change, no new `unsafe`, `const-oid 0.10.2` already in the tree), and the edge is LOAD-BEARING for RSASSA-PKCS1-v1_5 (`SigningKey::<D>::new` requires `D: Digest + AssociatedOid` to build the RFC 8017 §9.2 `DigestInfo` prefix). `alloc` and `zeroize` stay FORBIDDEN. The CI guard regex moved from `(alloc|oid|zeroize)` to `(alloc|zeroize)` in `1c6f670`, AFTER this record was minted. Caught by CI on `8a18e53` — the local sweep cannot see this step — and the packaged, LOCALLY-tagged `v0.40.0` was UN-tagged and HELD for CI green on the amended tree.**

**(librarian filing, 440th. Authors the record the engineer minted on
2026-09-05 and the `ci:` commit `1c6f670` cites by number. Every fact below
was MEASURED by this role at filing unless attributed: the CI run via
`gh run view 33994927156 --json jobs,conclusion,headSha`; the crate
manifests in `~/.cargo/registry/src/*/`; the feature graph via `cargo tree
-p pdfcer-core -e features`; the tag state via `git tag -l 'v0.40*'`.)**

**What happened, in order, with times from `git log --format=%ci`.**
`8a18e53` (the 7-commit batch push of the 439th filing) started CI run
`33994927156`. `03f6004` *"chore(release): bump workspace version to
0.40.0"* landed at **18:06:50** (`Cargo.toml` + `Cargo.lock` only, 2 files,
+6/−6); the release was built and packaged at 18:19
(`D:\builds\pdfcer-20260905-1819-03f6004`, zip
`D:\builds\pdfcer-v0.40.0-windows-x64.zip`, sha256 `e6472135b4ca…`) and
**tagged locally**. Then CI reported: **`conclusion: failure` on `headSha
8a18e53…`, 10 jobs, 9 `success`, 1 `failure`** — the job *"verify
pdfcer-core / pdfcer-render have zero GUI deps"*, and inside it exactly one
step: *"decision 039 (extended) — assert `sha2` carries no extra features"*.
The nine green: *cargo fmt --check*; *verify the ENGINE needs no network
(core + render)*; *repository audits (19 checks)*; *cargo test
(ubuntu-latest)*; *cargo test (windows-latest)*; *cross-target compile
check (macOS / wasm32)*; *fuzz targets build (nightly)*; *cargo clippy -D
warnings*; *third-party license audit*. The step's grep matched `sha2
feature "oid"` on all four targets it loops over (native,
`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `wasm32-unknown-unknown`).
The engineer **deleted the local tag** (never pushed — `git tag -l 'v0.40*'`
is EMPTY at this filing; `git ls-remote --tags` not consulted because the
tag never left the machine), wrote the memory note `796ff8c` (18:24:24),
minted this decision, and amended the guard in `1c6f670` (18:24:48). The
release is **HELD** until CI is green on the amended tree.

**Cause — the mechanism, stated precisely because the dispatch's wording
was close but not exact.** `rsa 0.10.0-rc.18`'s `Cargo.toml` has **no
`sha2` key in its `[features]` table** (lines 52–77 declare `default`,
`encoding`, `getrandom`, `hazmat`, `pkcs5`, `serde`, `std` — nothing else).
The `sha2` feature pdfcer enables is Cargo's **IMPLICIT optional-dependency
feature**, and the edge lives on the dependency declaration itself:

```toml
# rsa-0.10.0-rc.18/Cargo.toml:174–178
[dependencies.sha2]
version = "0.11"
features = ["oid"]
optional = true
default-features = false
```

Enabling `rsa/sha2` activates that dependency **with the features its
declaration names**, so `sha2/oid` turns on. Cargo's feature unification
then applies it to pdfcer-core's own `sha2 = { version = "0.11.0",
default-features = false }` line — the `default-features = false` there is
intact and irrelevant, because unification is a UNION across every path to
the package. `sha2 0.11.0`'s `oid = ["digest/oid"]` (its `Cargo.toml:51`);
`digest 0.11.3`'s `oid = ["dep:const-oid"]` (`Cargo.toml:52`). Net: the
`const_oid::AssociatedOid` impls for `Sha224`/`Sha256`/`Sha384`/`Sha512`
and their truncated variants, nothing else. **`const-oid 0.10.2` was ALREADY
in the tree** — `cargo tree -p pdfcer-core -i const-oid --depth 1` at this
filing: `der v0.8.2`, `digest v0.11.3`, `rsa v0.10.0-rc.18` (the dispatch
said "via `der`/`spki`/`pkcs8`"; `spki` and `pkcs8` reach it THROUGH `der`,
and `rsa` depends on it directly — same conclusion, the inverse tree is the
measured form). Decision 137's survey counted `const-oid` at **1 `unsafe`
line**; that count is unchanged by a trait impl being turned on.

**Why the edge is load-bearing, not incidental.**
`rsa::pkcs1v15::SigningKey::<D>::new` is bounded `D: Digest + AssociatedOid`
— the PKCS#1 v1.5 encoding (RFC 8017 §9.2, EMSA-PKCS1-v1_5) prefixes the
hash with the DER `DigestInfo` whose `AlgorithmIdentifier` carries the
digest's OID, and `rsa` obtains that OID from `<D as AssociatedOid>::OID`
rather than from a hand-written table. Without `sha2/oid` the
`RsaPkcs1Sha256` arm of `SignatureAlgorithm` (`Pass 10.7`) does not
compile. RSA-PSS (`RsaPssSha256`) and ECDSA do not need it; the RSA v1.5
default (`Signer::default_algorithm()` for an RSA key) does.

**The ruling.** Decision 039's `sha2` acceptance — the cfg-selected
intrinsic backend, first extended to `sha2` on 2026-08-11 — **now also
accepts the `oid` feature**, on the grounds above: it changes no backend,
adds no `unsafe`, adds no package, and is required by a shipped Pass.
**`alloc` stays forbidden** (pdfcer's own hashing calls only
`Sha256::new`/`update`/`finalize`; nothing needs an owned digest buffer).
**`zeroize` stays forbidden, for 039's own reason, restated so it is not
re-litigated:** `FileKey` holds the file-encryption key in a plain
`Vec<u8>`, so zeroizing the hash STATE while the key it derived sits un-zeroed
would be theatre — a guarantee the code cannot honour. The guard now reads
`grep -E 'sha2 feature "(alloc|zeroize)"'` and its comment block and error
message name decision 138 (`.github/workflows/ci.yml`, `1c6f670`, +16/−10).
**`aes` is untouched**: `cargo tree -p pdfcer-core -e features | grep -E
'aes feature|sha2 feature'` at this filing prints exactly one line,
`sha2 feature "oid"`.

**Order of operations, stated honestly.** The local gate sweep was GREEN
on the pushed tree and stayed green after the widening, because
`check-ci-parity.py` classes this step as *genuinely CI-only* (it needs
three cross-target `cargo tree` invocations the local sweep does not run).
So the widening was invisible locally and **was caught only by CI, after
the push — which is exactly what the guard exists for**: a feature edge
into an audited crate got a record BEFORE the guard moved, not after. What
went wrong is not the guard but the SEQUENCE around it — a release was
built, packaged and locally tagged **before CI had reported on the tree it
would ship**. Two rules fall out, both now in the engineer's memory
(`796ff8c`, `a-dependency-feature-can-widen-a-neighbour`): (1) **a release
tag waits for CI green on the PUSHED tree**, never on a local sweep; (2)
**any `Cargo.toml` change gets `cargo tree -p pdfcer-core -e features |
grep -E 'aes feature|sha2 feature'` run locally before the push** — the
one line that would have shown `sha2 feature "oid"` before `8a18e53`.

**Relationship to 039 and 137.** 039 accepted the `aes`/`sha2` `unsafe`
intrinsic backends and fenced their Cargo features; this is the first time
one of those fences MOVED, and it moved by one named feature with the
reason recorded. 137 adopted the signing stack; its table lists `rsa` with
`sha2`, `encoding` and its §6 line 364 DOES quote rsa's
`[dependencies.sha2] version = "0.11" features=["oid"]` — so **the survey
recorded the edge; the gap was PROPAGATION**: nobody carried that
parenthetical from the survey's generation check to the CI guard's fence
(or to 137's feature table, which is where a reader would look). Filed as a
propagation gap, not a survey fault, and not a fault of the guard, which
did its job. Cross-project half (the general shape, greppable):
`D:/dev/rag/rust/a_dependencys_feature_can_enable_a_feature_on_an_already_audited_neighbour_audit_cargo_tree_e_features.md`.

**Survivors of the old claim ("`oid` is off"), swept by this role (hard
rule 11, searching for the CLAIM):** §9's `sha2` paragraph — dated pointer
added this filing; §12 decision 039's 2026-08-11 amendment — dated
forward-pointer footer added (the entry itself stays, append-only);
`D:/dev/rag/rust/sha2_0_11_default_features_and_cfg_selected_backend_same_shape_as_aes.md`
— dated footer added (it also mis-states `oid` as
`["digest/oid", "dep:const-oid"]`; the manifest says `["digest/oid"]`).
**One survivor OUTSIDE this role's remit, owed to the engineer:**
`crates/pdfcer-core/Cargo.toml:345–361` (the `sha2` dependency's comment
block) still says *"`oid` would pull const-oid in to name an ASN.1
identifier no PDF path ever reads"* and *"`.github/workflows/ci.yml`
asserts alloc/oid/zeroize stay off"* — both false since `1c6f670`.
`docs/DEPENDENCIES.md`, `docs/PRIOR_ART.md`: no `oid` claim found
(`grep -n -i 'oid'`).

**Body sections:** §9 (the `sha2` paragraph — pointer to this decision).
No other body section describes the feature fence.

**Decision ceiling: `137` → `138`**, next free `139`. **Standing rules
ceiling `R241` — unchanged**, next free `R242`. **Open operator questions:
none minted — next free `(ce)`.**

### 2026-09-06 (460th filing) — decision 139: **THE REVIEW MODEL'S CRATE BOUNDARY — pdfcer AUTHORS THE `/IRT` CHAIN §12.5.6.3 REQUIRES AND REPORTS WHICH NODE IT ATTACHED TO (`attached_to`, `chain_depth`), AND STATES NO OPINION ABOUT WHICH STATUS IS *CURRENT*, BECAUSE THE STANDARD DEFINES NO ORDERING (`current state` / `most recent` occur nowhere in either edition in an annotation context) AND `/M` IS OPTIONAL AND EMPIRICALLY TIES. THE READ SIDE IS THE OPEN SET (`Option<String>`, VERBATIM), THE WRITE SIDE THE CLOSED ONE (`ReviewState`, SEVEN VARIANTS, NO `Other`). ★ THE REQUEST'S OWN API AND THE `Pass 253.1` *Backlog* ENTRY BOTH PROPOSED `add_review_state(target, …)` AS A PURE FUNCTION OF ITS ARGUMENTS; §12.5.6.3's CLOSING `shall` MAKES THAT IMPOSSIBLE, AND A SPEC-LIBRARIAN INGESTION DISPATCHED *BEFORE ANY CODE WAS WRITTEN* (PROJECT RULE 1) IS WHAT FOUND IT — THE WRONG SHAPE RENDERS IDENTICALLY TO THE RIGHT ONE, SO NOTHING DOWNSTREAM COULD EVER HAVE REPORTED IT.**

**(librarian filing, 460th. Authors the record for a boundary the engineer
drew in `fe746ec` and flagged in the dispatch as *"decision-shaped… I lean
yes but it is your call"*. Every fact below is either MEASURED here — the
commit, the source, the ledger gate — or explicitly RELAYED from the commit
message and the engineer's test run.)**

**The decision, in one line.** *`pdfcer-core` owns the graph; the shell owns
the verdict.*

**What was decided, and each clause's warrant.**

1. **`add_review_state` walks `/IRT` and filters by `/T`.** §12.5.6.3, a
   `shall`: *"Additional state changes shall be made by adding text
   annotations **in reply to the previous reply** for a given user."* So a
   second status by the same author attaches to **that author's previous
   status**, and the per-author chain is what carries their history. A
   different author starts their own chain from the target. This is not a
   preference — the alternative is non-conforming.
2. **`attached_to` and `chain_depth` are reported.** ★ **This is the clause
   that makes clause 1 checkable, and it exists because the failure is
   invisible by construction.** A star of state annotations all pointing at
   the target renders identically to a correct chain in every viewer. No
   screenshot, no render diff, no `content-identity` run and no operator
   report would ever surface it; the history a reviewer's chain encodes would
   simply not be there, and the document would look fine. Rule 4 applies in
   its purest form — the inference the operator *cannot see by definition* is
   the one that owes an off-canvas disclosure.
3. **No resolver for which status is *current*.** MEASURED, not preferred:
   the standard defines no ordering over the statuses on a target (`current
   state` / `most recent` occur nowhere in either edition in an annotation
   context), and `/M` is **optional** (Table 164) and empirically ties — a
   batch set in one session shares a timestamp to the second. A resolver would
   be pdfcer **inventing a rule and presenting it as a reading of the file**,
   which is `R27`'s failure mode one layer up. The requester asked for exactly
   this split: *"Give us the annotations and the keys; we will pick."*
4. **`/State` and `/StateModel` are text strings, not names.** `/State
   /Accepted` is a different COS object type that no conforming reader would
   match. Sabotage: writing them as names fails 3 tests.
5. **`/StateModel` is derived, not taken.** Table 171 makes it *"required if
   `State` is present, otherwise optional"* — and the converse does **not**
   hold. So the one non-conforming pairing is made **unrepresentable** rather
   than validated: `ReviewState` derives its own model, and the write-side
   argument the *Backlog* entry asked for is deleted instead of checked.
6. **Open read, closed write.** `Annotation::state` / `::state_model` are
   `Option<String>` — neither key carries a *"shall be one of"* in either
   edition, so a producer's own vocabulary is **modelled, not repaired**
   (`R27`). `edit::ReviewState` is closed at seven variants — `Accepted`,
   `Rejected`, `Cancelled`, `Completed`, `None` (the Review model) and
   `Marked`, `Unmarked` (the Marked model) — with **no `Other`**, because
   pdfcer authoring a vocabulary the standard does not define is the mirror
   image of repairing one it does not recognise.

**★★★ Why this record exists at all, and it is a stronger reason than "an API
shape was chosen".** The *Backlog* entry `Pass 253.1` — written by this role
on 2026-09-05 (`5f6bf65`) from the requester's own file — asserted, in
writing, that *"history is preserved (three reviewers, three surviving
statuses, "current" = most recent)"*. **That sentence is wrong twice**: it
asserts an ordering the standard does not define, and it names a currency rule
nothing supports. It sat in *the contract* for 34 hours and would have been
implemented as written had the engineer worked from it. **What stopped it was
project rule 1** — §12.5.6.3 was not in the spec corpus, so
`pdfcer-spec-librarian` was dispatched **before any of the code was written**,
and the ingestion (new corpus file `iso32000__s__12.5.6.3.md`) changed the
design three separate times. This is the first case in the project's record
where rule 1 can be shown to have prevented a defect that **no amount of
testing the built thing could have caught**, and that is why it is minted as a
decision rather than left in a Pass entry: *the value of sourcing before
writing is highest exactly where the unsourced version would still have
passed.*

**What this decision does NOT decide.** Whether a shell should surface a
"current" status at all (it is `pdfcer-gui`'s call, and Acrobat's own answer
was not sourced — no `pdfcer-acrobat-librarian` dispatch was made for the
status vocabulary, which the `Pass 253.1` entry had listed as an acceptance
step); whether `list-annotations` gains a `state=` column (named as a gap in
the 460th filing, not built); whether `/RT /Group` authoring is ever added
(`Pass 253.0` scoped it out and nothing here reopens it).

**Body sections updated in this filing:** **§4.2.2** — the review-state chain
guarantee, in §4.2's fixed entry format (property · measurement · pinning test
· forbidden refactor), including the refactor list this decision forbids. §4
itself is retired in favour of `docs/core-api/` (decision 102), which the
commits updated in-place (verb count **205 → 208**).

**Rule-11 sweep for this record (searching for the CLAIM):** the only
documents asserting *"review status is not modelled"* were `FEATURES.md`'s
*Planned* row and `ROADMAP.md`'s `Pass 253.1` *Backlog* entry — the first
replaced, the second struck with a dated correction note naming both wrong
clauses rather than silently edited (that section is append-only in the same
sense as *Shipped*). `grep -in "state_model\|StateModel\|review status"` over
`docs/` finds no other claim. ZERO survivors.

**Decision ceiling: `138` → `139`**, next free `140`. **Standing rules ceiling
`R241` → `R242`** (minted the same filing, for a different subject — a scoped
request being invisible to an audit that reads only `open/`), next free
`R243`. **Pass ceiling `258.3` — UNCHANGED.** **Open operator questions: none
minted — next free `(ce)`.**

### 2026-09-07 (468th filing, `75793b5`) — decision 140: **AN ENUM WITH A WRITTEN NON-GROWTH PROMISE GETS A NEW SIBLING TYPE FOR AN ORTHOGONAL QUESTION, NOT A WIDENED VARIANT SET, EVEN WHEN IT IS THE CLOSEST-SHAPED PRECEDENT IN THE CRATE. FIRST INSTANCE: `text_edit::ReflowDecline` MODELS RECOVERABILITY SEPARATELY FROM `text_edit::RefusalKind` (`Pass 249.0`), WHICH IS NOT WIDENED**

**(librarian filing, 468th. The engineer flagged this as decision-shaped in
the dispatch — *"I lean yes... but decisions are yours to mint"* — and it is
minted on that judgement. Every fact below is either verified directly by
this role via `Grep`/`Read` on live source — no shell tool this dispatch —
or explicitly relayed from the engineer's own test/gate run.)**

**The decision, in one line.** *A closed-set discriminant's non-growth
promise binds against ANY new concern, not only new instances of its
existing one — the remedy for a new concern that wants the same shape is a
sibling type, not a widened bucket set.*

**What was decided, and each clause's warrant.**

1. **`ReflowApplyError::Unsupported(String)` had carried ten distinct
   refusal causes in one opaque string**, one of them — text added to the
   page this session, `Pass 251.0`'s guard — recoverable by a save and
   reopen, and the commonest cause reported. A shell with no discriminant
   can only print the weakest sentence true of all ten, so the operator was
   denied a remedy pdfcer already knew about. VERIFIED: `PageEditedThisSession`
   is now its own `ReflowApplyError` variant, carved out with its sentence
   byte-identical (`crates/pdfcer-core/src/text_edit/reflow_apply.rs:258`).
2. **`RefusalKind`/`RefusalClass` (`Pass 249.0`) was the right SHAPE and the
   requester's own named precedent, and is the WRONG VOCABULARY** —
   `UnsupportedFont` / `StructureFrozen` / `NotFound` / `Other` answer *what
   kind of thing went wrong*, and every one of the ten reflow refusals maps
   to exactly one of those buckets regardless of recoverability. Widening
   `RefusalKind` to express recoverability would have looked like an
   answer, compiled, and told the caller nothing new.
3. **`RefusalKind` could not grow a fifth bucket for this**, because its own
   shipping Pass states a written stability contract — buckets are closed,
   deliberately not `#[non_exhaustive]`, so a consumer's exhaustive `match`
   is compiler-proved complete, and growing the set is a breaking change
   the design explicitly rules out (a fifth-bucket proposal was already
   declined once, `docs/ROADMAP.md`, 443rd filing). A recoverable reflow
   case is not an exception to that promise; it is exactly the kind of
   "we learned something new" pressure the promise exists to refuse.
4. **A sibling type was minted instead**: `ReflowDecline`
   (`RetryAfterSaveAndReopen` | `StructureForbids` | `NotFound` |
   `NotReflowable`), bridged from `ReflowApplyError` by `.decline()`
   (`reflow_apply.rs:352`), with `.is_recoverable()` (`:377`) **derived
   FROM** `.decline()` rather than stored independently — verified directly:
   `is_recoverable()`'s body is `matches!(self.decline(), ReflowDecline::RetryAfterSaveAndReopen)`,
   so the two questions cannot answer inconsistently about the same error
   by construction.
5. **A silent-inheritance bug was caught and fixed on the way past.**
   `pdfcer-cli`'s `cmd_reflow` exit-code match ended in a bare `_` arm
   before this Pass; a new variant would have inherited `RUNTIME_ERROR`
   silently, and the most recoverable refusal in the set would have exited
   as if something had crashed. VERIFIED directly at
   `crates/pdfcer-cli/src/main.rs:24370-24383`: `PageEditedThisSession` is
   now listed explicitly (`exit::EDIT_REFUSED`), with a doc comment naming
   the hazard by name — *"a new variant silently inheriting the catch-all
   is how a correct engine change becomes a wrong exit code."* The CLI also
   prints a distinct remedy sentence when `is_recoverable()` is true
   (`main.rs:24355-24360`), sourced from the engine's own answer rather than
   the shell's reading of the error's `Display` text.

**Proof.** `crates/pdfcer-core/tests/reflow_decline.rs` — **5** tests
(counted directly via `Grep "#\[test\]"`, not relayed). One existing test
(`content_edit_no_duplication::reflow_refuses_after_text_was_added_rather_than_deleting_it`)
changed its EXPECTATION, not its claim: it asserted `Unsupported(String)`
containing `"added"` (the only handle available when `Pass 251.0` shipped)
and now additionally asserts `is_recoverable()` — the original string
assertion is untouched. Sabotage reverted the guard to
`Unsupported(String)`; the construction-site test failed and named the
defect, the other four in the new file stayed green.

**Tests run this Pass (relayed, not independently re-run by this role — no
`cargo` execution available):** `pdfcer-core --lib` 2,039; `reflow_decline`
5; `content_edit_no_duplication` 3; `add_text` 18; `session_overlay_skew`
10; `text_edit` 5; `pdfcer-cli --bin` 20; `reflow` 5;
`inspect_reflow_preview` 11; `font_licence_notice` 3; `edit_text` 6 — **all
0 failed.** This is **not** a full `cargo test --workspace` run; recorded as
what was run, per hard rule 8.

**Gates (relayed):** `fmt`, `clippy -p pdfcer-core -p pdfcer-cli
--all-targets -D warnings`, `check-core-api-verbs` (`docs/core-api/index.md`
bumped to a stated 4,802 lines / 152 clauses — **not independently
re-measured this filing**, and flagged as such given `Pass 259.0`'s
same-session finding that this document tree's own line citations are
unreliable), `check-public-fns-documented`, `check-outcome-disclosed`,
`check-clap-help`, `check-cli-help-leads`, `check-control-bytes`,
`check-cited-verbs-exist`, `check-ledger-numbers`,
`check-suite-name-absent` — all reported **PASS**.

**What this decision does NOT decide.** Whether `pdfcer-gui` consumes
`is_recoverable()`/`decline()` — not yet built, `FEATURES.md` row amended
`core [x]` · `cli [x]` · `gui [ ]`; whether `docs/core-api` needs its own
`ReflowDecline` section (owed, not confirmed done — `reflow_apply.rs` has
no `core-api` cross-reference in its doc comments as of this filing,
verified by grep); whether the same audit ("does an existing discriminant's
stability contract get checked before reuse") should become a standing
rule — considered and **declined at n=1**, same warrant this role has used
before: one founding instance, however clean, does not by itself outrun a
documented pattern already written into `RefusalKind`'s own shipping
record and now restated here and in the cross-project RAG.

**Origin.** `open/request_reflow_unsupported_is_ten_causes_in_one_string.md`
(`pdfcer-gui`, 2026-09-07, against `v0.45.0` at `527b1523`). `R242` checked
before scoping — grep of `ROADMAP.md` for the request filename returned
zero hits; not previously scoped, `Pass 260.0` minted fresh. Reply written:
`open/reply_2026-09-07-reflow-decline-SHIPPED.md`. **Not released** —
`v0.45.0` stands; rides the next cut.

**Body sections updated in this filing:** **§8.1** (new) — the API-design
pattern this decision establishes, in the same fixed-shape convention
§4.2's subsections use (rule stated, warrant, distinguishing test, forbidden
refactor).

**Cross-project record:**
`D:/dev/rag/rust/an_enum_with_a_non_growth_promise_gets_a_sibling_type_not_a_widened_variant_set.md`
— the generalised form for any Rust project reaching for an existing
discriminant enum as a shortcut for a new classification question.

**Decision ceiling: `139` → `140`**, next free `141`. **Standing rules
ceiling `R244` — UNCHANGED**, next free `R245` (considered and declined at
n=1, see above). **Pass ceiling `259.0` → `260.0`** (this Pass) **→
`261.6`** (seven Backlog sub-IDs minted the same filing for an unrelated
Acrobat-comment-type catalogue — see `ROADMAP.md` *Backlog*), next free
family `262.x`.

### 2026-09-08 (474th filing, `caf4c1d`) — decision 141: **AUTHOR-TIME OPTIONS TRAVEL IN A SIBLING TYPE BESIDE A SPEC, NEVER INSIDE IT. A "SPEC" IS WHAT REBUILD VERBS REGENERATE FROM, SO ANYTHING A REBUILD MUST NOT BE ABLE TO CHANGE IS DISQUALIFIED FROM LIVING IN IT — AND ANY CARRIER OF A SPEC MUST CARRY THE SIBLING TOO. FIRST INSTANCE: `annot_author::MarkupCarry` BESIDE `MarkupSpec`, AND `ClipAnnotation::Markup` WIDENED TO CARRY BOTH (`Pass 270.0`)**

**(librarian filing, 474th. The engineer left the judgement explicitly —
*"a decision record IF you judge the 'carry beside the spec, never in it'
choice to be one … Your call."* Minted on three grounds, stated below. This
dispatch **had a shell**, contrary to its own opening premise, so every fact
here is measured with the command named, not relayed.)**

**The decision, in one line.** *A spec is the input a REBUILD regenerates
from; therefore the test for spec membership is not "does this belong to the
object?" but **"may a reshape or a restyle change this?"** — and everything
that fails that test travels in a sibling type that every carrier of the spec
must carry alongside it.*

**Why this is decision-shaped and not merely a tidy refactor — three
grounds, and the first is the one that made it mandatory.**

1. **It SUPERSEDES A PLAN ALREADY WRITTEN INTO THE BACKLOG.** `ROADMAP.md`'s
   *"lossless markup/annotation clipboard-copy fidelity"* entry recorded the
   engineer's own next step, deliberately filed *"here rather than left in a
   commit message"*: **"carry BOTH representations and choose by transform"** —
   the raw dictionary for a pure translation, the spec when it must rotate or
   scale. **That is not what shipped**, and a superseded plan sitting
   unmarked in the Backlog is precisely what a future session picks up and
   builds. Recorded so it cannot be.
2. **It is a public-API shape change with a breaking edge.**
   `ClipAnnotation::Markup` went from `Box<MarkupSpec>` to
   `(Box<MarkupSpec>, Box<MarkupCarry>)`. The enum carries `#[non_exhaustive]`
   at **enum** level, which stops downstream exhaustive matching and **does
   not** protect a variant's arity — verified by reading
   `crates/pdfcer-core/src/vector/clip.rs:304`. Any consumer writing
   `ClipAnnotation::Markup(spec)` breaks. (The version number is correct:
   under Cargo's 0.x semver `0.48.0` → `0.49.0` **is** the breaking slot.)
3. **The sibling will GROW, and the rule decides where the next three go.**
   `Pass 264.1` (`/BM`), `264.3` (`/BE`) and `264.4` (`/OC`) are all
   markup properties lost or unwritable at the time of this decision, and every
   one of them presents the same "spec or sibling?" question. **This decision
   answers all three in advance**, which is the difference between a rule and a
   one-off. ★ **Already exercised once, within the hour:** `70e8f53`
   (2026-09-08 09:55 −0400) fixes `/BM` — *"an annotation's blend mode is the
   file's, and restyle was deleting it"* — and is the **deferred tip**, filed
   by the 475th filing, not this one. Whether it placed `/BM` in the spec or
   beside it is **not assessed here**; the next filing owes that reading
   against this decision.

**What was decided, and each clause's warrant.**

1. **The four properties are not in the spec, and keeping them out is the
   right call rather than the legacy one.** `/BS` `/S`+`/D` (dash), `/CA`
   (opacity), `/Contents` (note) and `/T` (author) are **author-time
   options** living in `MarkupOptions`; `MarkupSpec` describes the **shape**.
   `reshape_annotation` and `set_markup_style` both **rebuild from the
   spec** — so had the four been moved in, a reshape would have acquired the
   power to change an author's name and a restyle the power to rewrite a
   comment. ⇒ **The disqualifying test is the rebuild, not the ownership.**
   The properties belong to the annotation in every ordinary sense; that is
   not the question the spec answers.
2. **The cost of that correctness was paid silently by every carrier of a
   spec, and the clipboard was one.** `ClipAnnotation::Markup` held a spec
   and nothing else, so it was **structurally incapable** of carrying the
   four and the paste was structurally incapable of noticing. Four
   operator-visible properties dropped on every copy-paste: a dashed
   revision cloud came back solid, a 50 %-opacity highlight opaque, a comment
   blank and unsigned. ⇒ **A rebuild-safety decision taken in one type
   created a silent-loss defect in a different one, and nothing connected
   them.** That is the general hazard this decision names.
3. **The remedy is a SIBLING, not a widened spec and not a second
   representation.** `annot_author::MarkupCarry` — `#[non_exhaustive]`, four
   `Option` fields (`dash`, `opacity`, `contents`, `author`) — with
   `encode_carry`/`decode_carry` beside the existing `encode_spec`/
   `decode_spec`. VERIFIED at `crates/pdfcer-core/src/annot_author.rs`
   (`MarkupCarry` at `:3325`, `encode_carry` at `:1209`, `decode_carry` at
   `:1240` — all read directly); `pub mod annot_author` at `crates/pdfcer-core/src/lib.rs:75`, so
   all three are public API.
4. **★ The paste applies the carry through the SAME `MarkupOptions`
   authoring uses** — `add_markup_with`, not `add_markup`
   (`crates/pdfcer-core/src/edit.rs:12225`). **One code path**, so a
   pasted mark and a freshly-authored one **cannot disagree about how a dash
   is written.** This is the clause that makes the design self-enforcing
   rather than merely tidy: a future change to how a dash is emitted cannot
   fix authoring and miss pasting, because there is no second emitter to
   miss. Same family as decision `140`'s `is_recoverable()` being **derived
   from** `decline()` rather than stored beside it.
5. **`None` MEANS ABSENT, NOT DEFAULT — and this is a correctness clause,
   not a style one.** A plain square must come back plain. An implementation
   that filled every absent field with a default would **pass a round-trip
   test** while quietly adding `/CA`, `/Contents` and `/T` to every unadorned
   mark anybody copied — **changing the bytes of files where nothing was
   asked for**, which is a round-trip/minimal-diff violation (project rule 3,
   `ARCHITECTURE.md` §5). There is a dedicated test for exactly this, because
   the round-trip test alone cannot see it.
6. **`decode_carry` DELIBERATELY CANNOT FAIL**, returning
   `MarkupCarry::default()` for anything unreadable. Refusing a whole paste
   over a garbled optional property would lose the **geometry** too, and
   trading a lost dash for a lost annotation is worse than the defect being
   fixed. **Accepted cost, stated rather than discovered later:** absent and
   garbled are indistinguishable afterwards.

**The distinguishing test, for the next property that asks.**

> **Ask: may `reshape_annotation` or `set_markup_style` change this value as
> a side effect of doing its own job?**
> **Yes** → it is geometry or style; it belongs in `MarkupSpec`.
> **No** → it is an author-time option; it belongs in `MarkupCarry`, **and
> every carrier of the spec must be widened to carry it in the same
> commit.**

**The forbidden refactor, named because it will look like a simplification.**
Folding `MarkupCarry`'s fields into `MarkupSpec` "so there is one type to
pass around". It compiles, it deletes a parameter, and it hands every rebuild
verb the power to rewrite an author, a comment and an opacity that nobody
asked it to touch. **The two types are separate because two different verbs
must have different powers over them**, which is not visible at the call
site.

**Proof.** `crates/pdfcer-core/tests/markup_clip_carry.rs` — **3** `#[test]`
(counted directly by grep, not relayed), asserted on the **PASTED annotation
read back out of the session**, not on the clip: *a clip that carries a value
and a paste that drops it would satisfy any clipboard-only assertion.*
**Sabotaged four times, once per carried property individually** — dropping
the dash, the opacity, the note or the author each turns the round-trip test
red; verified per instance, not per class. Green alongside (relayed from the
commit message): `annotation_clip_serialisation` 13, `markup_border_style` 15,
`form_field_clipboard` 24, `field_properties` 9.

**★★ WHAT THIS DECISION DID **NOT** COVER, AND THE GAP COST A DEFECT IN THE
SAME COMMIT.** Clause 4's *"every carrier of the spec must carry the
sibling"* was honoured for the **in-memory** carrier (`ClipAnnotation`) and
for the **byte** carrier's writer and reader — but **not for the byte
carrier's VERSION.** `ObjectClip::to_bytes` now writes a second positional
COS object per markup and `from_bytes` reads it unconditionally, while
`CLIP_VERSION` (`vector/clip.rs:88`) and `ObjectClip::needed_version`
(`:799`) were left untouched. **This violates decision `105` by name** —
whose own text says *"a second key added later cannot be wired into the
writer while missing the decider"* — and the reason the safeguard failed is
that **decision `105` reasons about droppable dictionary KEYS while this
added a non-droppable POSITIONAL FIELD.** Filed as **`Pass 270.1`**
(`ROADMAP.md` *Backlog*), owed **before** `v0.49.0` is tagged. ⇒ **A future
application of this decision must ask not only "which carriers?" but "does
any carrier declare a VERSION, and does the sibling change it?"**

**Body sections updated in this filing:** **§5** is untouched (the
minimal-diff invariant is applied here, not redefined). **§12** carries this
entry. No crate boundary moved; `cargo tree` invariant unaffected — no
dependency changed.

**Origin.** No external request: found by the engineer while auditing the
markup family, and filed against the Backlog entry the 317th filing opened
from `pdfceGUI`'s clipboard-fidelity question. **Not released** — `v0.49.0`
is bumped (`14ee766`) and **not tagged**; `origin/main` sits at `14ee766`
with **CI red** (run `34230418986`), and `c56f63e`/`caf4c1d` are unpushed
(`git log origin/main..HEAD`).

**Cross-project record owed, not yet written:**
`D:/dev/rag/rust/a_spec_is_what_a_rebuild_regenerates_from_so_author_time_options_get_a_sibling_type.md`
— the generalised form for any Rust project whose "spec"/"builder input"
type is also the input to a regeneration verb. Flagged to the engineer rather
than written this filing; it is the direct sibling of decision `140`'s
`an_enum_with_a_non_growth_promise_gets_a_sibling_type_not_a_widened_variant_set.md`.

**Decision ceiling: `140` → `141`**, next free `142`. **Standing rules
ceiling `R245` — UNCHANGED**, next free `R246` (`R209` grew clause (f) and
`R245` gained a dated instance; both mints declined with an argument — see
`ROADMAP.md` *Standing rules*). **Pass ceiling `269.0` → `270.0`** (this
Pass) **→ `270.1`** (one Backlog sub-ID minted the same filing for the
missing `CLIP_VERSION` bump), next free family `271.x`.
### 2026-09-08 (475th filing, `44a2485`) — decision 142: **A POSITIONAL FIELD IN A VERSIONED BINARY FORMAT IS NOT A DROPPABLE KEY. IT NEEDS A GATE ON *BOTH* SIDES, AND THE VERSION DECIDER MUST BE A FOLD OVER INDEPENDENT REQUIREMENTS RATHER THAN A LADDER. EXTENDS DECISION `105`, WHOSE STATED SCOPE — DROPPABLE DICTIONARY KEYS — IS EXACTLY WHY ITS SAFEGUARD DID NOT FIRE**

**Origin.** `Pass 270.1` (`44a2485`), discharging a defect the 474th filing's
own hard-rule-11 sweep found in the commit it was filing (`caf4c1d`,
`Pass 270.0`). **Found and fixed between the commit and the tag** — no release
carried it. Not an external request.

**★ THIS IS AN EXTENSION OF DECISION `105`, NOT AN UNRELATED NUMBER, AND IT IS
FILED AS A NEW ENTRY BECAUSE THIS LOG IS APPEND-ONLY.** Decision `105`'s own
entry gains a dated forward pointer to this one. Read them together: `105`
answers *when to bump*; `142` answers *what a bump has to gate, and where the
decider must live, when the format is positional rather than keyed*.

---

#### 1. What happened

`Pass 270.0` made `ObjectClip::to_bytes` write a **second positional COS
object** per markup annotation (a `MarkupCarry` beside the `MarkupSpec`,
decision `141`) and made `from_bytes` read it **unconditionally** — and left
`CLIP_VERSION` at `3`.

A reader from that build, handed a payload an older build wrote, **took an
object that was not there.** What it actually consumed was the *next*
annotation's tag byte and spec, **misaligning the parse for every annotation
after it.** Silently: `decode_carry` cannot fail by design, because refusing a
whole paste over a garbled optional property would lose the geometry with it.
**Both of those decisions are individually correct; their interaction is the
defect.** ⇒ **A guard that cannot report going wrong must not be reachable by
accident.**

#### 2. Why decision `105`'s safeguard did not fire — the transferable half

Decision `105` says, in its own words, that the clip's version *"is made by a
function, `ObjectClip::needed_version(&[ClipAnnotation]) -> u32`, rather than
by a branch at the write site — so there is **one** place that answers 'what
version is this content?', and **a second key added later cannot be wired into
the writer while missing the decider**."*

**A second field was wired into the writer and the decider was missed. The
rule was right; its stated SCOPE did not reach the change that needed it.**

| | droppable dictionary key | positional field |
|---|---|---|
| older reader meeting an unknown one | **skips it by name** and carries on | **cannot skip it** — no name, no length |
| older reader missing an expected one | reads a shorter dict; correct | **takes the next record's bytes** |
| failure mode | a **lost preference** | a **desynchronised parse** — silent, and unbounded past the first record |
| does `105`'s discriminator apply? | yes — *"bump when the loss changes what the document asserts"* | **NO — a desynchronisation is not a loss at all**, so neither branch of the discriminator ranks it |

That last row is the point. **`105`'s discriminator is a function of *what is
lost*, and a positional misread loses nothing — it misreads everything after.**
The discriminator is not merely hard to apply here; **it has no input.**

⇒ **Decision 142's rule.** For a positional binary format the version is **not
optional and the discriminator does not run**: any change to the **number or
order of positional records** is a **mandatory** version event, gated on
**both** the writer and the reader, and the reader's gate is the half that
makes an older payload readable at all.

#### 3. The three mechanism rulings

**(a) The version decider is a FOLD, not a LADDER.** `needed_version` returns
the **maximum** version any single element requires:

```
4  if any markup carries an author-time property (MarkupCarry)
3  if any ce dimension carries a text override
2  otherwise
```

**Written as a max, deliberately, not as an `else if` chain**, because the
features are **independent** — one clip can hold a dashed square **and** a ce
dimension with an overridden label — and **a ladder makes one of them
unreachable the moment a third is added.** ★ The general statement: *a version
requirement is a property of a SET of independent features, so its decider is a
fold over that set. The ladder shape silently converts it into a priority list,
and the conversion is invisible at the call site.*

**(b) A comparison against a version constant must name the EPOCH, not the
CEILING.** Two pre-existing gates read `if self.version >= CLIP_VERSION` and
`if version >= CLIP_VERSION`. **They were the version-3 label-override gates.**
Bumping the constant to `4` would have **silently re-pointed both at 4** and
broken the ce-dimension text override — **a defect introduced by a change that
never touched those lines.** Both now name `CLIP_VERSION_PRE_MARKUP_CARRY`,
which is what they always meant. ⇒ **A comparison written against a mutable
constant is a comparison against whatever that constant becomes.** Decision
`105` created `CLIP_VERSION_PRE_LABEL_OVERRIDE` for exactly this reason and
then **used it at one of the two sites** — so this is not a new insight, it is
`105`'s own mechanism applied completely. **Every `>=` against a version
constant names a named epoch, or it is a latent defect awaiting the next bump.**

**(c) The gate is on BOTH sides, and the reader's half is the load-bearing
one.** A writer-only gate makes new clips well-formed and leaves the new reader
still mis-parsing every old clip — which is precisely the shipped defect. A
reader-only gate leaves new clips undeclared and breaks the *other* build. Both
halves, tested in **both directions**.

#### 4. Decision `105`'s content-dependent-version ruling is HONOURED, on its second independent encounter

`CLIP_VERSION` rises 3 → 4 and `needed_version` emits `4` **only for a clip
that actually carries the field**; a plain clip still declares **2**. The
operator runs two builds out of two folders and copies in one to paste in the
other; **a blanket bump breaks every paste between them from the day it ships,
to protect a field most clips do not have.** That argument was written onto
`CLIP_VERSION` for the version-3 bump and applies here unchanged.

**★ MINT OF A STANDING RULE FOR `105`'s PRINCIPLE: STILL DECLINED, AND THE
DECLINE IS ARGUED RATHER THAN INHERITED.** Decision `105` declined a rule,
reserved **`R235`** as the number it would take, and named its own trigger:
*"the next FORMAT in this project that gains an optional key whose loss changes
what a document asserts — a third, independent encounter."* `Pass 270.1` is a
**second independent encounter with the SAME format**, not a third format.
**The trigger says *format*, and this role does not get to loosen a decline
written by the same role in order to reach a mint.**

**★★ AND THE RESERVED NUMBER NO LONGER MEANS WHAT `105` MEANT BY IT.** `R235`
was spent on unrelated work long before this filing (the standing-rule ceiling
was `R245` on the day `142` was minted, and is `R246` after it). **A reader
arriving at decision `105` and looking up `R235` will find a rule about
something else.** Recorded here so that search ends at this paragraph rather
than at a false match — and it is a small instance of this project's recurring
shape: **an obligation stayed correct while a fact inside its own statement
went stale**, which is what librarian hard rule 8's 2026-08-07 amendment
records about itself.

#### 5. What this costs, stated so it is not discovered as a surprise

Decision `105` already recorded that a content-dependent version makes the
writer *"look at the content before it can state its version"*, and that
**omitting the obligation produces a merely over-versioned file, which no test
fails on.** **142 adds the sharper cost:** for a **positional** format,
omitting the obligation produces a file that is **mis-parsed** — and **no
same-version round-trip test can see it.** The round trip is the test everyone
writes; it is green in both the correct and the defective implementation,
because both halves move together.

⇒ **The only test that can see this defect constructs a payload at the OTHER
version.** `clip_version_gating.rs` does it by driving **this build's own
writer at version 2** — reproducing the old format rather than reasoning about
it — and uses **two** annotations, because with one an over-consuming reader
merely runs out of bytes and could plausibly error, while with two it eats the
second annotation's tag and spec, **which is the silent wrong answer rather
than the loud failure.** ★ *Choose the fixture that produces the QUIET symptom;
the loud one would have been caught anyway.*

#### 6. Public-surface consequence

`CLIP_VERSION_PRE_LABEL_OVERRIDE` and `CLIP_VERSION_PRE_MARKUP_CARRY` are now
**exported from `pdfcer_core::vector`**. A shell reasoning about cross-build
compatibility **could not name the versions it needed to reason about** — the
epochs existed as private constants, so the only way to ask *"will the other
folder's build read this?"* was to hard-code a number. **An epoch a consumer
must reason about is public, or the consumer re-derives it wrongly.**

#### 7. Scope, so this is not over-read

This governs **pdfcer's own private binary formats** — currently the
`ObjectClip` clipboard file, and the `/PieceInfo` ce-dimension sidecar if it
ever becomes positional (today it is keyed, and therefore under decision `105`
unchanged). **It says nothing about PDF itself**, whose object model is keyed
and whose forward-compatibility story is §7.3.10's ignore-unknown-keys rule.
**And it does not reopen decision `141`** — a sibling type beside a spec is
still the right shape; this is about how that sibling **travels in a byte
stream**, a different question `141` correctly did not answer.

**Body sections updated in this filing:** **§5** untouched — the round-trip
invariant is *applied* here, not redefined: a plain clip still serialises to the
bytes it always did, which is exactly the property a blanket bump would have
broken. **§12** carries this entry and the dated forward pointer on decision
`105`. No crate boundary moved and no dependency changed, so the `cargo tree`
GUI-core-separation invariant is unaffected.

**Cross-project record:**
`D:/dev/rag/rust/a_positional_field_in_a_versioned_binary_format_is_not_a_droppable_key.md`
— **written this filing, not owed.** ★ **And decision `141`'s own owed RAG
file, flagged by the 474th filing and still unwritten at the start of this
one, is written this filing too:**
`D:/dev/rag/rust/a_spec_is_what_a_rebuild_regenerates_from_so_author_time_options_get_a_sibling_type.md`.

**Decision ceiling: `141` → `142`**, next free `143`. **Standing rules ceiling
`R245` → `R246`** — `R246` minted this filing for an unrelated finding (*a
correction must reach every corpus this project READS*; see `ROADMAP.md`
*Standing rules*) — next free `R247`. **Pass ceiling `270.1` → `270.2`**, next
free family `271.x`.

### 2026-09-08 (476th filing, `56fee79`) — decision 143: **WHERE THE STANDARD'S OWN SELECTOR IS UNRELIABLE IN PRACTICE, pdfcer TAKES THE SPEC-MANDATED BRANCH *FIRST* RATHER THAN *ONLY* — A LADDER, NOT A SWITCH. ISO 32000-1 §9.6.6.4 SAYS THE `/Encoding` ENTRY IS *"IGNORED"* ON A SYMBOLIC FONT; pdfcer STILL FALLS BACK TO IT, MATCHING ACROBAT**

**Origin.** `Pass 271.0` (`56fee79`), from an operator report on a real 2013
SolidWorks drawing: *"Text on this sheet is scrambled in pdfcer, but appears
fine in acrobat reader."* Not a scoping decision — a ruling forced by the fix.

---

#### 1. The clause, and what pdfcer actually does

§9.6.6.4 splits simple-TrueType glyph selection on the font descriptor's
`Symbolic` flag (Table 123 bit 3, value `4`):

| branch | condition | chain |
|---|---|---|
| **A** | nonsymbolic, `/Encoding` present | code → glyph **name** → Unicode (AGL) → `(3,1)`; else name → **Mac OS Roman code** → `(1,0)`; else `post` |
| **B** | `Symbolic` set — *"the `Encoding` entry is ignored"* | the **raw code**, into the program's own cmap |

pdfcer's ladder, after this Pass:

```
1. symbolic && embedded  ->  program's built-in encoding, raw code   (Branch B)
2. name -> Unicode -> (3,1)                                          (Branch A)
3. name -> Mac OS Roman code -> (1,0)                                (Branch A)
4. name -> post                                                      (Branch A)
5. raw code -> built-in encoding                                     (Branch B)
6. None -> .notdef, counted
```

**Rung 1 is new. Rungs 2–4 remaining reachable BELOW it is the decision.** A
literal reading of *"ignored"* would make them unreachable for a symbolic font.

#### 2. The warrant — the standard's own selector is not trustworthy

§9.8.2 Table 123 states that `Symbolic` (bit 3) and `Nonsymbolic` (bit 6)
*"shall not both be set or both be clear"*. **Real producers break that
constantly**, which makes `Symbolic` a signal that is usually right and
sometimes meaningless. Two consequences, and they point opposite ways:

- **Honouring it is mandatory** — the measured defect below shows what
  ignoring it costs.
- **Honouring it *exclusively* is not safe** — a font mislabelled `Symbolic`
  whose real encoding is its `/Differences` would render as `.notdef`
  throughout, and pdfcer would have no route left.

⇒ **Take the branch the flag names, and keep the other branch as a fallback.**
The asymmetry that makes this free rather than a compromise: **Branch B
returning `None` costs nothing** — there is no wrong answer to discard, only a
miss — so trying it first can never *lose* information, and falling through
after it can only *add* a candidate where the alternative is a guaranteed
`.notdef`.

**This is the same posture Acrobat takes**, and the spec corpus records the
divergence as a known interop fault line rather than as pdfcer's invention:
`D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__9.6.6.md:252-255` —
*"`Differences` on a symbolic embedded TrueType is 'should not' but ubiquitous
… Readers differ here; Acrobat is more permissive than the text."*

#### 3. Why this is a decision and not just a bug fix

**Because the fix could have been narrower and was deliberately not.** The
defect — Branch A running *before* Branch B on a symbolic font — is repaired
completely by rung 1 alone. Making rungs 2–4 unreachable for symbolic fonts
would be **more faithful to the clause's text** and is what a spec-literal
implementation does. pdfcer **declines that**, and the declining is the part a
future reader needs, because a later maintainer reading §9.6.6.4 will see the
word *ignored* and read the fallback as a bug.

**The generalised form, which is what earns the number:** *where a
standard-mandated selector is itself unreliable in real files, implement the
mandated branch as the FIRST rung of a ladder rather than as the ONLY arm of a
switch — provided the mandated branch's failure mode is a **miss** rather than
a **wrong answer**.* That proviso is load-bearing and is what stops this
generalising into "be permissive everywhere": a fallback after a branch that
can return a **confidently wrong** value would compound the error instead of
recovering from it. **That is precisely the failure this Pass fixed, in the
other direction** — Branch A's chain 2 was reached first and returned a valid,
wrong glyph, so falling through past it was never an option.

#### 4. The gate is `symbolic && embedded`, and the `embedded` half is UNPROVEN — labelled, not deleted

A **substituted** face's "built-in encoding" is the *substitute's*, with no
relationship to the document's codes, so taking it first would break every
non-embedded symbolic font. That is the same distinction `encoding_table`
already draws one function above.

**Ablating `embedded` leaves the suite GREEN**, and the reason is structural
rather than a missing fixture: a failed Branch B falls straight through to the
name chains at no cost, and for the guard to bite, a substitute face would have
to carry a `(3,0)`/`(1,0)` subtable, which a normal text face does not.
**Shipped labelled as unproven in place** (`crates/pdfcer-render/src/text.rs`,
the `builtin_first` comment block) rather than deleted, because it is the
correct statement of the rule and a future face that *did* carry one would
otherwise silently start resolving raw codes against it — *a guard no test can
fail is indistinguishable from a guard that does nothing, and the next reader
should not have to re-run the ablation to learn which this is.* Same
disposition as the 463rd filing's three flagged-in-place `crates/` survivors.

#### 5. Relation to decision 051

Decision **051** (*"the symbolic-font guard is against the §9.6.6.4 Branch B
MAPPING, not against symbolic fonts as a class"*) is about **font embedding** —
whether a missing symbolic font may be *substituted*. **143 is about
rendering** — which glyph a code selects once a program is present. They cite
the same clause and do not overlap; neither amends the other. Read together
they say the project has twice declined to treat `Symbolic` as a class
boundary, and twice for the same underlying reason: **the flag describes a
mapping, not a kind of font.**

#### 6. Measured consequence

On the reporting file (`WSQMXO+TT19Et00`, symbolic, `WinAnsiEncoding` +
`/Differences`, `(1,0)` + `(3,0)`, no `(3,1)`): **`.notdef` glyphs on page 1
went 961 → 0**, and Branch A chain 2 had been returning valid unrelated glyphs
for the rest — code 3 `/three` → GID 56 `U`, code 8 `/one` → GID 48 `M`, code
10 `/two` → GID 52 `Q`. **Both spec branches would have been correct**; pdfcer
had been taking a third path that was neither. Full table in `ROADMAP.md`'s
`Pass 271.0` entry.

#### 7. Body-section effect and invariants

**§4 (core API surface) unchanged** — `resolve_gids` is private to
`pdfcer-render`; its signature gained `flags: u32, embedded: bool` with no
public item added or altered. **No crate boundary moved and no dependency
changed**, so the §3 GUI-core-separation invariant is unaffected and no
`cargo tree` check is owed. **§5 round-trip is untouched** — this is a render
path; no writer, no save mode.

**Cross-project record:**
`C:\personal_rag\pdf\lesson_20260908_symbolic_truetype_subset_private_1_0_cmap_renders_wrong_glyphs_while_extraction_is_clean.md`
— **written this filing, not owed**, and it closes a pointer the spec corpus
has carried unresolved (§9.6.6.4's *"known interop fault line →
`C:\personal_rag\pdf\`"* named a destination that did not exist).

**Decision ceiling: `142` → `143`**, next free `144`. **Standing rules ceiling
`R246` — UNCHANGED**; `R247` considered and **declined** (the fixture finding
is `R225`'s tenth instance and a new medium, not a new cause — see
`ROADMAP.md` *Standing rules*), next free `R247`. **Pass ceiling `270.2` →
`271.0`**, next free family `272.x`.

---

### 2026-09-08 (477th filing, `757386d`) — decision 144: **A LOCATOR WITH TWO INDEPENDENT AXES MUST BE ABLE TO EXPRESS BOTH. WHERE AN API FORCES A CALLER TO CHOOSE BETWEEN *WHAT* AND *WHICH ONE*, SOME TARGETS BECOME ***UNREACHABLE*** RATHER THAN MERELY AWKWARD — AND THE NEW POWER ARRIVES AS A SEPARATE CONSTRUCTOR, NEVER AS A SILENT WIDENING OF THE EXISTING ONE**

**Origin.** `Pass 272.0` (`757386d`), from `pdfcer-gui`'s
`request_a_spanning_find_cannot_be_anchored_at_a_pinned_operator.md`. Not a
scoping decision — a ruling forced by the shape of the gap, and by a second
measurement taken while closing it.

---

#### 1. The two axes, and why "awkward" was the wrong word for it

`EditSession::edit_text` locates its target from an `EditRequest`. Two
independent pieces of information can identify a run, and until this Pass a
caller could supply either but never both:

| axis | field | answers | what it cannot do |
|---|---|---|---|
| **WHAT** | `find: String` | *which text* | pick between identical texts — pdfcer chooses |
| **WHICH ONE** | `pinned_span: Option<ByteSpan>` | *which show operator* | describe a run that spans more than one operator, because the pin confines the match **inside** the pinned operator |

A producer that emits **one glyph per show operator** — routine in CAD output
— puts every multi-character run in the intersection of those two failures:
the pin cannot hold the run, and the `find` cannot say which one.

**★ The word "awkward" is what this decision rejects.** A missing convenience
leaves the target reachable by a longer route. **This left it reachable by no
route at all**, and the proof is measurement §3 below: the alternative
spelling (`find` alone, pin dropped) does not merely risk the wrong
occurrence — on a page carrying a single-operator twin it can never reach the
spanning one, at any distance, in any direction.

⇒ **The ruling.** *Where a locator carries two orthogonal axes, the API owes a
spelling that supplies both. Absence of that spelling is a **reachability**
defect, not an ergonomics one, and must be triaged as such.*

#### 2. The new spelling, and why it is a separate constructor

```rust
pub fn spanning_from(page_index: usize, span: ByteSpan, find: &str, replace: &str) -> Self
pub span_from_pin: bool                     // explicit, defaults false
```

Three lines of implementation (`find_replace(...).pinned(span)` with the flag
set) and **no new search logic** — the span search is the existing one with
every guard intact: the same `spannable` test, the same `same_line` `Td`/`Tm`
tolerance, the same trim-to-the-operators-the-match-touches rule, the same
requirement that the match **begin** inside the anchor. **Only the starting
operator differs.**

**The cheaper design was available and was declined.** Widening `pinned` to
mean *"start here"* needs no new symbol at all. It was rejected on the
requesting shell's own argument, adopted here as the general form:

> **A silent widening of an existing constructor changes what every existing
> caller's REFUSAL means.** Callers who never asked for the new behaviour are
> the ones who pay: their `NoMatch` stops meaning *"the text is not in that
> operator"* and starts meaning *"the text is not in that operator or any
> operator after it"*, and no compiler, test or type signature marks the
> change. **A widened meaning is a breaking change with no diff.**

⇒ **Corollary, and it generalises past this API:** *a capability that
relaxes a constraint gets a new name; only a capability that TIGHTENS one may
be added in place, because tightening turns silent wrong answers into
refusals, and relaxing turns refusals into silent answers.* `Pass 272.0` did
both — it added the relaxation under a new name **and** tightened the existing
path (§4) — which is why the pair is instructive.

#### 3. The measurement that changes the advice, not just the diagnosis

**`find_replace` does NOT edit "the first occurrence."** `find_anchor` tries a
**single-operator** match across the **whole page** *before* the spanning
search runs at all.

> **A single-operator occurrence anywhere on the page beats a spanning one
> above it.**

⇒ **a spanning run is unreachable by `find` alone whenever a single-operator
twin exists anywhere on that page.**

**This contradicted the request's framing and the engineer's own first draft
of the test**, and it surfaced only because the fixture carries both shapes:
the test failed on an assertion written from the wrong model. It is recorded
in §12 rather than only in the Pass entry because it is a **standing property
of the locator's contract** that every shell must design against, and because
it is the fact that upgrades §1 from *inconvenient* to *unreachable*.

**Documented for consumers** in `docs/core-api/02-editing-and-saving.md`,
§*`EditRequest::spanning_from` — when the text REPEATS on the page*.

#### 4. `unwrap_or(0)` on a locator is not a default — it is a wrong answer wearing a right answer's shape

The pinned path computed its anchor with
`let pos = s.text.find(find).unwrap_or(0);`. When the search missed, the
fallback **claimed the match began at byte 0 of the pinned operator** — an
anchor pointing at bytes nobody asked about, which then failed downstream with
a message blaming the **text**.

**The operation failed either way; that is not the cost.** The cost is that
**the failure was reported against the wrong subject**, and a diagnosis
performed on that report lands one layer away from the cause. The requesting
shell located the fault at a guard **one arm too late** — an
`Err(e) if req.pinned_span.is_some()` arm that is **unreachable for a pin that
resolves**, because `find_anchor` returns `Ok(i)` for a pinned request without
consulting `find` at all. Their reasoning was correct; their evidence was
manufactured by this line.

Measured three ways before anything changed:

| request | result | what it proves |
|---|---|---|
| pin + `find` inside that operator | succeeds | the pin resolves |
| bogus pin | `PinnedSpanNotFound` | that arm fires only here |
| pin + spanning `find` | `NoMatch` | the failure is elsewhere |

⇒ **Now a named refusal at the point of detection** (`EditError::NoMatch`),
with `PinnedSpanNotFound` kept distinct: **the first means the pin is wrong,
the second means the pin is fine and the text does not begin there.** A shell
switching on refusal kind needs exactly that fork, and conflating them is what
sent the report to the wrong guard.

**The generalised rule:** *a locator may not synthesise a position. Where it
cannot locate, it refuses — and it refuses at the site of the failed lookup,
because a refusal raised later carries the wrong subject.* This is the
`.unwrap_or(0)` shape specifically: an `Option` returned by a **search** is
not the same kind of `Option` as one returned by a **lookup with a natural
identity element**, and `unwrap_or` cannot tell them apart.

#### 5. Body-section effect and invariants

**§4 (core API surface) — CHANGED.** `EditRequest` gains one public associated
function (`spanning_from`) and one public struct field (`span_from_pin: bool`),
both documented. Recorded here explicitly because §4 drifting behind the
shipped core surface is a failure this project has already had, and a new
public item is the exact event that causes it.

**§3 GUI-core separation — unaffected**, and **no `cargo tree` check is owed**:
no `Cargo.toml` was touched. **§5 round-trip / minimal-diff — unaffected**:
this changes *which* bytes an edit targets, never *how many* are rewritten.
The spanning contract's existing behaviour — the replacement lands in the
operator holding the match end, earlier matched operators are emptied to
`() Tj` — is unchanged and is **not** new here; it is noted because it is the
trap that makes a naive byte assertion fail against a correct edit.

**Rule 4 (fuzzy, never sneaky) — nothing owed, and the reason is worth
stating.** `spanning_from` performs **no inference**. The caller supplies both
axes; pdfcer chooses nothing. The verb that *does* infer — `find_replace`
picking an occurrence — is the one this decision makes avoidable, so the net
effect is **less** unreported inference, not more.

#### 6. Relation to earlier decisions

- **Decision 094** (the `operator_span`-slice invariant, published guarantee,
  0 exceptions in 29,246 groups over 4,289 files) is what makes a pin a
  **trustworthy** locator in the first place. 144 spends that guarantee: it is
  only safe to let a caller start a span search at a pin because the pin is
  known to name a real operator boundary.
- **`Pass 152.0`'s finding** — that `EditRequest::whole_operator` existed as
  behaviour for three Passes with *"no symbol to grep"*, and the consuming
  shell filed a defect saying the verb could not be reached — is the same
  class of failure at the **documentation** layer that this decision addresses
  at the **API** layer. In both, the capability's absence was a *naming*
  absence. `R220`(f) covers the documentation half; this covers the case where
  the capability genuinely was not there.
- **No decision is amended or superseded.** 144 stands beside 094 and does not
  touch it.

#### 7. Scope, so it is not over-read

This is **not** a licence to add a constructor per argument combination.
The obligation fires when **(a)** two axes are genuinely orthogonal — neither
derivable from the other — and **(b)** their combination identifies targets
that **no** existing spelling reaches. An argument that merely *shortens* an
existing route is ergonomics and does not qualify. The `nth: usize`
occurrence-index the request explicitly did **not** ask for is the worked
counter-example: it would be a third spelling of the *same* axis (WHICH ONE),
computed by the caller over extracted text to address operator text, and the
requesting shell declined it on exactly that ground.

**Decision ceiling: `143` → `144`**, next free `145`. **Standing rules ceiling
`R246` — UNCHANGED**; `R247` considered and **declined** (the assertion-site
vacuity is `R225`'s eleventh instance plus a dated widening clause, not a new
cause — see `ROADMAP.md` *Standing rules*), next free `R247`. **Pass ceiling
`271.0` → `272.0`**, next free family `273.x`.

### 2026-09-09 (484th filing, `dce2223`) — decision 145: **A STRUCTURAL DEFECT THAT LEAVES THE OBJECT GRAPH AMBIGUOUS, NOT UNDEFINABLE, IS OPENED — pdfcer PICKS A READING UNDER A NAMED DEFAULT, DISCLOSES WHAT IT PICKED AND DISCARDED, AND LETS THE OPERATOR TAKE THE OTHER ONE. EXTENDS `R27`'S FAIL-CLEAN KERNEL FROM THE DECODER LAYER TO THE LOADER LAYER; ONLY A DEFECT REQUIRING pdfcer TO INVENT A READING THE FILE SUPPLIES NONE OF STAYS FATAL**

**Origin.** `Pass 283.0` (`dce2223`), from the operator's own report — a real
drawing, `A-726 BASKET ATTACHMENT_REV 5.pdf`, that Acrobat opens and pdfcer
refused on a duplicate `/PageMode` key — followed by two rulings that widen
the fix into a posture, both quoted in full in §10.5 above and not repeated
here.

---

#### 1. Why this is a decision and not just a bug fix

Fixing the file's own duplicate key would have refused one object later, on
its `/Metadata` stream's missing `/Length` — **the file had two independent
defects, and clearing one exposed the other.** That fact is what forces the
answer to be a *policy for the loader* rather than a *patch for a defect*:
any fix scoped to one defect class leaves the next class refusing exactly as
before, and the operator's second ruling ("for all defects where it is
possible to continue") says explicitly that this is not acceptable practice
going forward.

#### 2. What the spec actually says about the motivating defect, and where my own first draft got it wrong

Dispatched to `pdfcer-spec-librarian` rather than reasoned from memory,
because a duplicate-key ruling is exactly the kind of thing training data
gets confidently wrong.

- **It is a `shall not`, not merely a `should`**, identical in body text
  across both ISO 32000 editions: *"Multiple entries in the same dictionary
  shall not have the same key."* The "Adobe over-enforced a should"
  hypothesis is refuted — the PDF Association records the 1.7 `Note:`/should
  and the ISO `shall not` as having "the same technical meaning."
- **It binds the FILE, not the reader.** §2.1/2.3 make conformance a
  property of files and writers; §1 excludes validation methods from scope
  entirely. ISO 32000 obliges pdfcer neither to render such a file nor to
  refuse it.
- **Reader behaviour is acknowledged out of scope, not merely silent** —
  pdf-issues #199 (open since 2022): "as soon as a PDF violates a mandated
  'shall' requirement... then how that PDF is to be interpreted is beyond
  the scope of ISO 32000."

**★ My own first justification for keeping the LAST value was wrong, and is
corrected in the shipped code, not left standing.** The first draft argued
from §7.5.6 — "every other override-by-repetition in PDF is last-wins" —
which is an **analogy dressed as a citation**: §7.5.6 orders objects across
*incremental updates*, an ordering the standard makes meaningful by
construction, while §7.3.7's own preceding sentence says a *dictionary's
entry order* "shall be ignored." Borrowing authority from an ordering rule
to justify a decision about a structure the standard explicitly says has no
order is exactly backwards. The real support, substituted before shipping,
is **observed behaviour**: qpdf, pdf.js and pdfium all keep the last
occurrence and none refuses — qpdf even warns in the terms pdfcer now uses.
Worth its own line: ISO's one *resolved* duplicate-key erratum (#3, inline
images) picks its winner by **content** and rejects first/last positional
logic **by name** — so the original citation would have borrowed authority
from a clause that says the opposite about this exact structure. Filed as a
`D:\dev\rag\rust\` methodology finding (see Ledger below) because the
failure mode — a real citation, wrong clause, opposite meaning — generalises
past PDF entirely.

**The empirical, cross-implementation half of this — that real readers
converge on keep-last where the standard leaves the question open — is a
PDF-domain finding, not a spec-text one, and is filed to
`C:\personal_rag\pdf\` rather than duplicated into the spec RAG** (see
Ledger).

#### 3. The mechanism, and the two shell surfaces

```
Document::load_anomalies() -> &[LoadAnomaly]
Document::from_bytes_with_options(bytes, password, LoadOptions)
LoadOptions::new() | ::strict() | ::with_duplicate_keys(..)
CLI: --on-malformed keep-last|keep-first|refuse   (global)
```

`LoadAnomaly::DuplicateDictKey` carries **both** values, not a count — a
count says pdfcer chose; only the pair lets a shell show what it chose
*between*, and without that the intervention promised by the operator's
first ruling is theoretical rather than real. The other variants carry the
object and a **reason**, because "object 4 could not be read" is a fact and
"...because X" is something an operator can act on. Taking the alternative
is done by **re-loading** under the other policy, never by patching the
built document — the discarded reading was never constructed, so there is
nothing in memory to edit toward it.

**The flag's own name was corrected before shipping, for the same reason as
§2's citation.** The first cut called it `--duplicate-keys`, while its
`strict` value also silently disabled two unrelated recoveries (`/Length`,
`endobj`) — a flag named for one member of the class it actually governs.
Renamed to `--on-malformed` before shipping, and the CLI's own remedy
sentence (which used to hard-code "re-run with keep-first" regardless of
which policy was already active) now names the policy **not** currently in
force, so the advice cannot be wrong the moment the operator has already
taken it.

#### 4. What stays fatal, and why that is not strictness reasserting itself

A file with no `/Root`: there is no document to show, and continuing would
mean pdfcer **inventing** a catalog — the one thing nothing in this Pass
does. A test pins this line so a future session widening the policy further
has a recorded boundary to check against rather than a feeling. Encryption
without a working password stays fatal too, because asking for the password
**is** the intervention this decision otherwise automates away.

#### 5. The `R27` relation, and the standing-rule question

Argued in full in §10.5 above: `R27`'s actual kernel — fail by name, never
substitute a guessed value silently — is extended from the decoder layer
(where it was minted) to the loader layer (where it had never been stated),
and the operator-facing override is new because a loader's ambiguity is
often genuinely arbitrable by the operator in a way a codec's missing
sub-feature is not. **This is an extension, not a relaxation**: `R27` was
never "refuse on any defect," so a counted, disclosed, overridable decision
does not weaken it.

**On minting a standing rule for this posture — argued, not deferred.** For:
the operator's second ruling is explicitly general ("for all defects..."),
which is exactly the shape that produced `R35`/`R58`/`R67` (this project's
own precedent for minting a rule directly from a single decisive ruling
rather than waiting for a second occurrence) — a future engineer meeting a
seventh defect class needs a rule to triage against, not a re-read of this
one Pass's narrative. Against: the two-occurrence bar this project applies
to *emergent, discovered* patterns does not literally apply to an
*operator-issued* posture, so there is a genuine question of whether this
belongs as a decision-only ruling (as `R27` originally was, before this
extension). **Minted anyway** — `R248` — because the shape matches the
project's forced-full-rewrite-sibling precedent (a ruling with a stated
general scope, not a one-off fix) more closely than it matches the
emergent-pattern family `R221`/`R224`/`R225` govern. Numbered past the
reserved-but-unclaimed `R247` deliberately, so this claim does not entangle
with that unrelated, still-unreconciled reservation (see `ROADMAP.md`
*Standing rules*).

#### 6. Verification (relayed from the shipping commit's own message)

`malformed_opens` 12 (new), `pdf15_streams` 18, `document` 46, full
workspace suite green; `cargo fmt` clean; `cargo clippy --all-targets
--all-features -- -D warnings` clean; `tools/run-gates.sh` PASS on all 29.
R34: the corpus round-trip harness re-run — 237/241 loadable, no
shortfalls, identical to before this Pass. Four sabotages, all red. Four
existing tests that asserted the old refusals were **amended, not
deleted** — each keeps its original assertion as its second half under
`LoadOptions::strict()`.

**Decision ceiling: `144` → `145`**, next free `146`. **Standing rules
ceiling `R246` → `R248`** (`R247` UNCHANGED, still reserved-but-unclaimed —
see `ROADMAP.md` *Standing rules*), next free `R249`. **Pass ceiling
`282.0` → `283.0`**, next free family `284.x`.

---

**Addendum, 2026-09-09 (`Pass 283.1`, `d8fcb68`).** Completes the wiring
decision 145 specified: the override reached only
`Document::from_bytes_with_options`; `Document::load_with_options(path,
password, options)` is new and `pdfcer`'s `open_document` now uses it
instead of duplicating `Document::load`'s own `std::fs::read`. **No new
decision** — the boundary and mechanism above are unchanged, only their
reach. Filed as `R245`'s sixth dated instance (`ROADMAP.md` *Standing
rules*: a facility present on one of two parallel entry points and absent
from its twin — `R245`'s guard shape, shown here for an affordance instead),
not a new standing rule. **Decision ceiling unchanged at `145`, next free
`146`. Standing rules ceiling unchanged at `R248`, next free `R249`. Pass
ceiling `283.0` → `283.1`, next free family unchanged, `284.x`.**

### 2026-09-09 (486th filing, `ea4acb3`) — decision 146: **A DESTRUCTIVE SWEEP OBLIGED BY AN OUTCOME-SHAPED REQUIREMENT ("REMOVE ALL TRACES OF X") IS SCOPED BY THE EVIDENCE THE REQUIREMENT ITSELF NAMES, NEVER BY A COMPUTED REACHABILITY OR LIVENESS WALK. THE PROOF IS EMPIRICAL, NOT ONLY ARGUED: THE CENSUS PROBE BUILT TO MEASURE THIS FIX REPRODUCED THE SAME SILENT-FAILURE SHAPE TWICE WHILE MEASURING IT**

**Origin.** `Pass 284.0` (`ea4acb3`), the queue's own owed item 16
("orphaned `/Info`-shaped object survives a redaction," recorded at the
482nd filing) — dispatched as one instance and returned as the whole
class: `redact`'s carriers find their target by navigating the document
graph, `writer::save_full` emits objects by enumerating the cross-
reference table, and every object in the difference between those two
sets was re-emitted verbatim into a redacted file, offered to no carrier.

---

#### 1. Why the fourteenth carrier is a decision and not a bug fix

Patching `carrier_info` a second time (it was already patched once, at
`Pass 282.0`) would have closed exactly the one object shape the
motivating file happened to carry — an `/Info`-shaped dictionary. It
would not have closed a thread's own information dictionary, an XMP
packet on a component or on a marked-content property list, or any
future carrier-shaped object nobody has written a check for yet. The
generalisation the fourteenth carrier makes is not "check one more
shape" but "stop finding shapes by enumeration and start finding them by
sweeping the file's own listing of what it contains" — a change to
*what "all content" means for a saved artifact*, not an addition to a
list of known shapes.

#### 2. The line that makes this a posture change rather than a patch

`RedactionReport` never lied. `prior_revisions action=dropped_by_rewrite`
was true of the byte ranges a forced full rewrite actually drops.
`info action=scrubbed` was true of the `/Info` dictionary the trailer
points at. **Every report line was accurate, and the redacted words were
still in the file.** A carrier-by-carrier model of correctness — "did
each named carrier do its job" — cannot see this class of gap by
construction, because the gap lives entirely in objects no carrier was
ever pointed at. The fix has to change what scopes the sweep, not what
any one carrier does once scoped.

#### 3. The mechanism, and why it is scoped by evidence rather than reachability

```
redact::residual_sweep            (crates/pdfcer-core/src/redact.rs)
  scope: doc.xref().iter()        — the cross-reference table's own listing
  NOT:   a graph walk from trailer/catalog outward
```

Two independent reasons, both load-bearing:

1. **The clause obliging this is written against the artifact's content,
   not against the graph.** §12.5.6.23: *"they shall remove all traces of
   the specified content"*, scoped by *"all content that can exist in a
   PDF document."* It never mentions reachability, the catalog, or the
   trailer. Scoping the sweep to a graph walk substitutes a set the
   clause never named for the one it did.
2. **Reachability computations on this format fail silently, not
   loudly**, and this project has now measured that twice in one Pass:
   object streams are reached by a **type-2 xref entry** (§7.5.7), cross-
   reference streams by **byte offset**, the linearization dictionary is
   unreferenced by a **`shall`** (Annex F.3.3), and §7.3.10 makes a
   reference to a missing object *"not … an error."* An over-broad or
   under-broad graph walk does not error out — it produces a **valid**
   file missing an outline, a structure tree, or a whole object stream,
   and nothing downstream notices.

**★★★ The second reason is not merely argued in this decision — it is
demonstrated, in the same Pass, by the tool built to verify the fix.**
`crates/pdfcer-core/examples/unreachable_census.rs`, written by the
engineer in the same hour as reading §12.5.6.23, first counted every
object stream in the corpus as an "orphan," then — after noticing that
was wrong — counted every cross-reference stream as one too, before
arriving at the correct figure. **The reported measurement moved from
21% of files down to the true 12% across two intermediate, both-wrong
counts, each one exactly the shape of silent reachability failure named
in reason 2 above, produced by someone who had just finished writing
that argument.** This is the strongest form of evidence available for
"do not trust a reachability computation, however careful, as a proxy for
the evidence a correctness obligation actually names" — it did not need
a second, unrelated incident to prove the point; the same incident proved
it twice, days apart within the hour, against itself.

**The action table**, by shape, is in `docs/core-api/03-capabilities.md`
— read there for the exact per-shape behaviour (dictionary string entry:
scrubbed; `/Type /Metadata` stream: blanked whole; any other evidence-
carrying stream: `DisclosedNotScrubbed`, named rather than risked, since
blanking bytes inside a font programme or image on a coincidental match
would corrupt content pdfcer never meant to touch).

#### 4. What this closed without being asked to

Because the sweep is scoped by evidence rather than by an enumerated list
of known carrier shapes, it closed **three carriers nobody had written a
check for**: a thread's own information dictionary (Table 160: its
contents *"shall conform to the syntax for the document information
dictionary,"* live and reachable, never examined by `carrier_info`
because nothing routed it there), and two further XMP attachment routes
(§14.3.2 B and C — `carrier_xmp` only reads route A, the catalog's own
`/Metadata`). This is the direct, positive argument for evidence-scoping
over enumeration: a carrier that has to be told a shape exists can only
ever cover the shapes someone thought of; a sweep scoped to what the file
itself lists does not need to be told.

#### 5. What stays owed, by design, not by oversight

A **non-metadata content stream** carrying redacted text — one pdfcer's
own earlier surgery abandoned, e.g. an emptied form XObject — is **named
by `residual_sweep`, not blanked**. This is deliberate: pdfcer already
knows, from its own edit history, which content streams its own surgery
rewrote, so closing this needs no reachability walk and no new design
argument — but it is a destructive act on a class of object
(page-content-shaped bytes, not metadata) this decision did not reason
about, and is filed as its own Pass rather than folded in under this
decision's authority. `ROADMAP.md` owed item 18.

#### 6. Relation to `R58`/§5.9, and the standing-rule question

§5.9 (`R58`) already establishes that every removal/scrub operation
forces a full rewrite and must decompose every object-stream container
holding a scrubbed object — the mechanism this decision's sweep runs
inside of. This decision does not revise that mechanism; it revises
**what counts as a target for it**, and is filed as a short addendum to
§5.9 rather than a new body section, since no crate boundary, library
choice, or writer-mode invariant changed.

**On minting a standing rule — accepted, from the engineer's own
argument, offered unnumbered.** The engineer stated the generalisation
explicitly and by design left the number unclaimed — `R247` reserved-but-
unclaimed, `R248` the ceiling — for this filing to judge rather than
pre-empting it. **Minted as `R249`,** on the same precedent `R248` itself
used one filing prior: a third, unrelated candidate claims the next free
number rather than entangling with an already-contested reservation. The
two-occurrence bar this project applies to *emergent* patterns
(`R221`/`R224`/`R225`) does not transfer cleanly here either — but unlike
`R248`, which rested on an operator's single decisive ruling, `R249`
rests on a design argument **empirically corroborated twice within the
Pass that produced it**, which this filing judges a stronger warrant for
minting from `n=1` than a ruling alone, not merely an equal one. Full
rule text: `ROADMAP.md` *Standing rules*.

#### 7. Verification (relayed from the shipping commit's own message)

New `crates/pdfcer-core/tests/redaction_residual_sweep.rs`, 7 tests
including two controls (trailer's own `/Info` still scrubbed; a clean
file reports `CheckedClean`); three sabotages, all red. `tools/
run-gates.sh` PASS on all 29; `cargo test --workspace` green; `cargo fmt`
and `cargo clippy --all-targets --all-features -- -D warnings` clean.

**Decision ceiling: `145` → `146`**, next free `147`. **Standing rules
ceiling `R248` → `R249`** (`R247` UNCHANGED, still reserved-but-
unclaimed), next free `R250`. **Pass ceiling `283.1` → `284.0`**, next
free family `285.x`.

---

### 2026-09-09 (490th filing, `1bbb7c1`) — decision 147: **WHERE THE SPEC DEFINES NO KEY FOR A SUBTYPE'S DERIVED PARAMETER, PDFCER BORROWS THE KEY THE STANDARD ALREADY DEFINES FOR THE IDENTICAL PROBLEM ON A SIBLING SUBTYPE, NEVER A PRIVATE SIDECAR — `/DA` ON `/Stamp`, SOURCED FROM §12.7.3.3'S `/FreeText` ENTRY, NOT `/PieceInfo`. A FIT POLICY IS SETTABLE BUT NEVER RECOVERED: NO STORED KEY RECORDS AN AUTHOR'S INTENT, AND A GEOMETRIC GUESS WOULD INVENT ONE NOBODY MADE**

**Sourcing (hard rule 8) — NO SHELL THIS FILING.** The commit hash and
technical detail below are relayed from the dispatching engineer's own
message; independently corroborated against `docs/core-api/
03-capabilities.md:1198-1258`, which already carries the `Pass 287.0`
citation, the `StampStyle`/`StampFit`/`DEFAULT_STAMP_FONT_SIZE` names,
and this decision's `/DA`-over-`/PieceInfo` reasoning, matching the
dispatch's own description rather than invented for this entry.

**Origin.** `Pass 287.0` (`1bbb7c1`), the operator's report that a
stamp drawn too small for its text has no way to be fixed afterward —
stretching the box stretches the text with it.

**The choice, and why it is a decision rather than an implementation
detail.** §12.5.6.12's `/Stamp` table defines exactly one subtype key,
`/Name` — no font, no size. Two storage options existed: invent an
application-private `/PieceInfo` (§14.5) entry, or reuse `/DA` — the
key §12.7.3.3 already defines for the **identical** problem (a
variable-text appearance that must survive a later resize) on
`/FreeText`. Dispatched to `pdfcer-acrobat-librarian` before choosing
(`Acrobat_Features/markup__stamp_text_size_and_resize_behavior.md`,
`markup__custom_stamp_file_format.md`): the standard has no `/Stamp`
answer, and Acrobat has no documented answer either — having no
regeneration-on-resize hook, it very likely stretches its own stamp
text on resize exactly as pdfcer did before this Pass. **This is not a
parity gap; it is a place the standard left empty.** `/DA` wins: a font
size is not private data, and burying a legible answer in an
application-keyed sidecar makes every other conforming tool unable to
read what pdfcer could simply write in the open. This generalises
decision 141's framing (author-time options travel beside a spec,
never invented as private) one step further: where the *spec itself*
already has a slot for the same problem on a related object, filling
that slot beats inventing a new one, private or not.

**The second, independent ruling: `fit` is a caller-time policy, never
a recovered one.** `StampStyle`'s `points`/`legacy_derived`/`with_fit`/
`with_font_size` recover a stamp's authored **size** and **label** from
the baked appearance (`EditSession::recover_stamp_parameters`) for
every stamp that predates this Pass — necessary, because a stored
property with no recovery path would silently change the rendered size
of every stamp already in every document the first time it was
touched. But `StampFit` (`GrowToText`/`ShrinkToBox`/`ClipToBox`) is
**not** recovered, by design: nothing in a `/Stamp` dictionary records
which policy its author intended, and inferring one from the box's
current proportions relative to the text would invent an intent the
author never stated — the same "don't guess a decision nobody made"
posture as the ce-dimension resize refusal (decision 096), applied to
a property rather than to a whole verb.

**Consequence for `resize_annotation`'s authorship check.** Recovering
the label closed a standing defect rather than only enabling the new
feature: `/Contents` on a `/Stamp` is a *comment about* the stamp, not
its words, so the authorship rebuild used to reproduce only the stamp
name's **default** label, compare it against the operator's actual
custom text, and refuse the operator's own stamp as foreign — the
long-open `request_resize_annotation_refuses_a_pdfcer_authored_stamp_
as_foreign.md`. **The test was correct; the spec it tested against was
lossy.** `resize_annotation` now carries a **third** authorship arm
(markup, `/FreeText`, and now `/Stamp`) — `R245`'s shape on a family of
three routes, closed rather than merely enumerated again.

**Body section.** `docs/ARCHITECTURE.md` §4 is retired (decision 102)
in favour of `docs/core-api/`; the current surface is documented at
`docs/core-api/03-capabilities.md:1198-1258`, already carrying
`StampStyle`/`StampFit`/`DEFAULT_STAMP_FONT_SIZE` and this decision's
reasoning as of `Pass 287.0`'s own commit — no separate body-section
edit needed here.

**Decision ceiling: `146` → `147`**, next free `148`. Standing rules,
Pass and other ceilings unchanged by this entry — see `ROADMAP.md`'s
own Ledger for this filing.

---

### 2026-09-10 (491st filing, `554897e`) — decision 148: **CUSTOM STAMP COLLECTIONS ARE PLAIN PDFS — CATEGORY IN `/Info` `/Title`, EACH STAMP NAMED IN THE CATALOG'S `/Names`→`/Pages` NAME TREE (LEXICOGRAPHIC PER §7.9.6, NOT PAGE ORDER), `#` MARKS A DYNAMIC STAMP. `/PieceInfo` REJECTED AS A RED HERRING. THERE IS NO SEPARATE INTERCHANGE FORMAT.**

**Sourcing (hard rule 8) — NO SHELL THIS FILING.** The commit hash and
technical detail below are relayed from the dispatching engineer's own
message; not independently re-verified against a live Acrobat install
by this role (the engineer reports reading the files directly).

**Origin.** `Pass 288.0` (`554897e`), the operator's request that
pdfcer's custom-stamp authoring be "compatible with Adobe's" and offer
"the same import/export."

**The choice, and why it is a decision rather than an implementation
detail.** Two facts had to be established before any writer could be
built: where a stamp collection's category name lives, and whether the
community-reported `#`-prefix convention for dynamic stamps was real.
`pdfcer-acrobat-librarian`'s Feature RAG had already reached the
correct overall shape (one PDF per category, one page per stamp) from
convergent community sources, but labelled the finding `(c)
convergent-secondary` and flagged both specifics as unconfirmed rather
than guessing. Both were resolved by reading Adobe's own shipped
stamp files on this machine (`…\Acrobat DC\Acrobat\plug_ins\
Annotations\Stamps\ENU\StandardBusiness.pdf`, `Dynamic.pdf`) directly:
`/Info /Title (Standard Business)` names the category; each stamp's
`internal=display` pair (e.g. `SBApproved=Approved`) lives in the
catalog's `/Names` → `/Pages` name tree; a dynamic stamp's name carries
a literal `#` prefix (`#DApproved=Approved`) and is paired with an
`/AcroForm` `/CO`+`/Fields` pair that drives its text via calculation
JavaScript rather than static content. `/PieceInfo` (§14.5) — the
plausible alternative the secondary research had already flagged and
which this Pass considered — was **rejected**: it appears in these
files only where they separately carry `/Illustrator` authoring data,
never as part of the stamp-naming mechanism itself. pdfcer's writer
reproduces the confirmed shape exactly; it does not invent a private
key where Adobe's own files show none is used.

**The second, independent finding: name-tree order is spec-mandated,
not producer's choice, and the primary read is what caught it.** §7.9.6
requires a name tree's `/Names` array in lexicographic order. Adobe's
own `StandardBusiness.pdf` proves a writer cannot substitute page
order for it: `SBApproved` names page 0 and `SBCompleted` names page 4,
which is alphabetical but not sequential by page. A writer that emitted
entries in page order would produce a tree a conforming reader could
binary-search incorrectly. `Pass 288.0`'s test suite deliberately gives
page 0 the alphabetically-**last** stamp name specifically so a
page-order-emitting implementation fails it — a fixture that could not
tell the two orderings apart would not have caught this, in the same
family of caution `R225` names elsewhere in this log.

**Consequence: "same import/export" has an honest, and slightly
deflating, answer.** There is no Adobe-defined interchange format
separate from the collection PDF itself — "export" is handing someone
the file. pdfcer's `stamp-pack`/`stamp-list` produce and read exactly
that PDF, so pdfcer's stamp collections and Acrobat's are
interchangeable in both directions by construction, not by a bespoke
sidecar format neither product actually has.

**Body section.** `docs/ARCHITECTURE.md` §4 is retired (decision 102)
in favour of `docs/core-api/`; this decision's reasoning belongs beside
`Pass 288.0`'s own commit and `docs/core-api/`'s stamp-collection
entry rather than a separate body-section edit here — no crate
boundary, library choice or writer-mode invariant changed, only a
compatibility encoding.

**On the sourcing-methodology finding, and why it is recorded as a
standing rule rather than only here.** That a `(c)`-labelled Feature-RAG
entry, correctly hedged, was closed by reading a primary artifact
already present on the machine — rather than by further secondary
research — is a generalisable engineering-discipline finding, not a
pdfcer-specific one. Filed as standing rule `R250`; full text in
`ROADMAP.md`'s *Standing rules* section, this filing's Shipped entry.

**Decision ceiling: `147` → `148`**, next free `149`. **Standing rules
ceiling `R249` → `R250`**, next free `R251`. **Pass ceiling `287.0` →
`288.0`**, next free family `289.x`.

### 2026-09-10 (493rd filing, `9b7bc6c`) — decision 149: **THE GRAMMATICAL SUBJECT OF A SPEC `shall` CLAUSE — "CONFORMING READERS SHALL…" VERSUS "THE ANNOTATION SHALL…" — IS THE DISCRIMINATOR FOR WHETHER A NO-`/AP` LOOK IS SYNTHESIS (FORBIDDEN BY `R43`) OR AN OBLIGATION (`R43` DOES NOT REACH IT). NARROWS `R43`, DOES NOT REPEAL IT.**

**Sourcing (hard rule 8) — NO SHELL THIS FILING.** `Read`/`Grep`/`Glob`
only. The commit hash, the byte-level `startxref` offset, the exact test
count and the sabotage-fixture mechanism are relayed from the dispatching
engineer's message. Independently verified here against the live tree:
`crates/pdfcer-render/src/annot.rs::paint_named_icon` exists, is called
from the `Appearance::None` arm of the same per-annotation loop
`paint_appearance` is called from, and its doc comment quotes §12.5.6.4
Table 172 and §12.5.6.12 Table 181 verbatim; `annots_icon_painted` is
printed on `render-page`'s metrics line in `crates/pdfcer-cli/src/main.rs`.

**Origin.** The operator's `Annotations_output.pdf` (PDFsharp 1.3)
rendered blank in pdfcer-gui despite showing content in Acrobat Reader.
Measured: three annotations, `/Text /Note`, `/Text /Help`, `/Stamp
/TopSecret`, none carrying an `/AP`. `R43` ("render from `/AP` or not at
all") was working exactly as written — the question was whether it was
being applied to a class of annotation the standard never assigned it to.

**The choice, and why it is a decision rather than a bug fix.** `R43`
(decision 008, 2026-08-01) was written as one blanket rule covering every
annotation subtype with no `/AP`. The standard does not treat every
subtype the same way. §12.5.6.4 Table 172 and §12.5.6.12 Table 181 place
an explicit duty on **conforming readers** — not on the annotation, not on
the document — to provide predefined icon appearances for named standard
icons on `/Text` and `/Stamp` respectively. §12.5.6.8's square/circle
clause, by contrast, places its `shall` on **the annotation itself**
("Square and circle annotations shall display…"), which is silent about
who acts when the annotation has no `/AP` to display from — there the
original reading of `R43` (decline, disclose, stay blank) remains the only
defensible one, because drawing *something* would require inventing
geometry the file never supplied. `/Line`, `/Ink` and `/Caret` carry no
`shall` of this kind at all and are likewise unaffected. **The
generalisable rule extracted from this comparison: before treating a
no-`/AP` annotation as ungoverned, read the clause's grammatical subject.
"The reader shall" is an obligation on pdfcer; "the annotation shall" is
a property of a well-formed file, silent about recovery, and stays inside
`R43`'s original refusal.**

**Why this is a decision and not merely an `R43` reading note.** It
changes what pdfcer **draws**, for the first time since `R43` was
written five weeks earlier — a rendering-policy change with no byte-level
consequence (round-trip rule 3 and `R44` are not in play; nothing is
written to the file) but a real pixel-level one, verified by the operator's
own report. That combination — a durable, reusable interpretive method
(read the grammatical subject) applied to reverse a five-week-old,
previously-uncontested rendering default — is judged to clear the bar for
a decision record rather than staying only a dated `R43` sub-note, even
though the sub-note is also written (see `ROADMAP.md`'s `R43` entry,
amended in the same filing).

**A corpus-propagation finding, recorded here because it is the second
instance.** §12.5.2's *"individual annotation handlers may ignore this
entry and provide their own appearances"* has been present in
`D:\Dev\Rag-Specialized\PDF_Spec\iso32000__s__12.5.2.md` line 75 since
2026-07-31 — filed in the **same session** that wrote `R43`'s own
justification. It never reached the decision it bore on, for over five
weeks. This is the same shape as the XFA-deprecation finding recorded in
`CLAUDE.md`'s "Outstanding open items" §XFA-scope bullet (answered
2026-08-11): a fact was already sourced in one document while another
document still treated the question as open, or in this case, treated the
opposite reading as settled. **n=2** for this specific failure mode — a
corpus fact that exists and does not reach the decision it governs. Not
yet worth a standing rule of its own (two instances, five weeks apart,
different corpora); worth a `pdfcer-spec-librarian` corpus-propagation
sweep if a third instance surfaces.

**Body section.** No `ARCHITECTURE.md` body-section edit — this is a
rendering-policy narrowing inside `pdfcer-render`'s existing annotation-
paint loop, not a crate-boundary, library-choice, or writer-mode-invariant
change; §4 is retired in favour of `docs/core-api/` (decision 102) and
nothing in that tree currently states `R43`'s scope canonically enough to
need a parallel edit. The canonical, amendable text lives in `ROADMAP.md`'s
`R43` entry itself, amended in the same filing (Standing rules discipline,
not `ARCHITECTURE.md` body-section discipline).

**A known gap, opened as its own item rather than folded in here.** The
same `Annotations_output.pdf` also exposed a `startxref` pointing 134
bytes short of its own `xref` keyword (a PDFsharp writer defect); pdfcer's
recovery path drops the resulting unrecoverable object (a corrupted
content stream) with no anomaly recorded and describes it as "not in the
file" when it is present and simply declined. That is a gap in decision
145's disclosure obligation, in the recovery path rather than the loader
path decision 145 already covers — filed as `ROADMAP.md` owed item 18, not
resolved by this decision.

**Decision ceiling: `148` → `149`**, next free `150`. **Standing rules
ceiling unchanged at `R250`**, next free `R251` — `R43` gains a narrowing
note under its own existing number, no new rule minted. **Pass ceiling
`288.1` → `289.0`**, next free family `290.x`.

### 2026-09-10 (494th filing, `556878e`) — decision 150: **A REQUIRED PAGE-TREE ATTRIBUTE ABSENT (OR DANGLING) DEFAULTS TO THE VALUE THE STANDARD ITSELF NAMES FOR THAT KEY (TABLE 30'S `/Resources` ROW: THE EMPTY DICTIONARY), NEVER TO ONE PDFCER INVENTED. `/MediaBox` HAS NO SUCH CLAUSE AND STAYS FATAL. EXTENDS DECISION 145/`R248`'S KERNEL TO A READING SUPPLIED BY THE STANDARD RATHER THAN THE FILE.**

**Sourcing (hard rule 8) — NO SHELL THIS FILING.** `Read`/`Grep`/`Glob`
only; this role has no shell in this invocation. The commit hash, the two
motivating files, the mid-Pass correction and the exact test/gate counts
are relayed from the dispatching engineer's message. **Independently
verified here, by `Read`/`Grep` against the live tree**, not taken on the
dispatch's word alone: `crates/pdfcer-core/src/page_tree.rs` carries
`resources_defaulted: bool` on `Page` (line 193), `PageTreeError::
BadResources` (line 318), the `MissingRequired`/`BadResources` split at the
resolution site (lines 802–815), and a test named
`a_resourceless_page_does_not_cost_its_siblings` (line 1203) among five new
tests in that file; `docs/core-api/01-reading-and-model.md` already states
the corrected `MissingRequired` scope (`MediaBox` only, line 881) and the
`resources_defaulted` table row (line 818); `crates/pdfcer-core/src/
stamp_file.rs` carries `page_tree_error: Option<String>` and
`crates/pdfcer-cli/src/main.rs` prints `page=UNKNOWN` distinctly from
`page=MISSING` (lines 41857–41880); `fixtures/synthetic/xref-recover/
page-tree-cycle.pdf` exists. **Not independently re-verified this filing:**
the exact test count (5 in `page_tree.rs`, 2 in `stamp_collection.rs`), the
`cargo test --workspace` 5,285-pass figure, and `run-gates.sh`'s 28/29-then-
29/29 sequence — these are relayed.

**Origin.** Two files, neither exotic: the operator's own Acrobat-written
signature stamp collection (`%APPDATA%\Adobe\Acrobat\DC\Stamps\
YTV_yyfVN1TzJ0_6oei-GB.pdf`) has a blank spacer page 1 with no `/Contents`
and no `/Resources`; pdfcer refused all three pages of the file, on every
verb that walks the page tree. pdfcer's own `fixtures/synthetic/
minimal.pdf` has the identical shape, so the project's own smallest legal
fixture could not be read by this walk either.

**The choice, and why it is a decision rather than a bug fix.**
`page_tree::resolve_page` treated `/Resources` as required in the same
sense `/MediaBox` is required — absent means `MissingRequired`, fatal for
the whole tree because the walk returns one `Result` for every page. The
two keys are not the same kind of "required": Table 30 states an explicit
default for `/Resources` ("If the page requires no resources, the value of
this entry shall be an empty dictionary"); no clause anywhere states a
default `/MediaBox`. The fix generalises past this one key: **before
refusing a page tree over an absent required attribute, check whether the
standard itself names a default for that specific key. If it does, resolve
to the default and disclose that a default was used. If it does not, the
refusal stands.** `/MediaBox` is the pinned negative case — a test asserts
it stays `MissingRequired`, so a future widening of this decision cannot
cross that line by accident, the same discipline decision 145 used for its
own `/Root` boundary.

**Relation to decision 145/`R248` — extends the kernel, does not restate
it.** Decision 145's mechanism picks between readings the FILE supplies.
Here the file supplies none; the reading comes from the STANDARD, once,
for a named key. The underlying kernel — never refuse when a defensible,
disclosed reading exists; disclose what was chosen — is the same, and no
new standing rule is minted for that reason: this is `R248`'s posture
applied one layer further, not a new mechanism. What is new, and why this
gets its own decision number rather than a dated `R248` instance, is the
interpretive method: **does the standard's own text for THIS key name a
default, checked key by key, never "is the file's omission plausible."**
That method is reusable against future required-attribute questions the
same way decision 149's grammatical-subject test is reusable against
future `shall`-clause questions.

**Corrected mid-Pass, by the dispatching spec-librarian, before code
shipped.** The engineer's first justification argued a page with no
`/Contents` cannot name a resource at all. False: §7.8.3's third bullet
lets a form XObject or Type 3 font omit its own `/Resources` and inherit
the page's, and the ISO 32000-2 erratum extends that inheritance to an
annotation appearance stream — precisely the stamp-page shape that
motivated the Pass. The decision survives; the reasoning was replaced
before shipping, not after.

**A test that leaned on the defect it was fixing, found twice in the same
Pass — filed as `R225`'s 18th dated instance, a new sub-shape within the
family.** Two existing tests (`fontinfo`'s
`an_unwalkable_page_tree_is_reported_not_rendered_as_no_fonts` and the
CLI's `an_unwalkable_page_tree_is_flagged_rather_than_reported_as_empty`)
obtained their "unwalkable page tree" fixture by relying on `minimal.pdf`'s
now-fixed defect, and both went RED when the defect was fixed — not
because either assertion was ever wrong, but because the mechanism
producing their precondition was the bug under repair. Every prior `R225`
instance is a test that measured LESS than its name/doc-comment claimed;
this is the inverse shape — a test that measured exactly what it claimed,
reached through a defect rather than through the condition it named. Both
were repointed at a new fixture built to fail unwalkability a different
way (`fixtures/synthetic/xref-recover/page-tree-cycle.pdf`, a `/Pages`
node listing itself in its own `/Kids`). Full text: see `ROADMAP.md`'s new
dated-instance note, this filing.

**Also filed the same push, no decision of its own: `Pass 290.1`
(`bce4703`).** `stamp_file::read` built its page list via `page_tree::pages
(doc).map(..).unwrap_or_default()`, so a page-tree walk failure produced an
EMPTY page list — indistinguishable from "every stamp's name points at a
page the document does not have," which `page_index: None` already means.
On the operator's own Acrobat-written stamp file `pdfcer stamp-list`
printed `page=MISSING` beside both of his real signatures. Fixed by adding
`StampCollection::page_tree_error: Option<String>` rather than making
`read` fallible (the consuming project's own preferred shape); the CLI
prints `page=UNKNOWN` and names the cause on stderr. **Flagged, not
minted, as a candidate finding at n=2**: a value computed and discarded
via `.unwrap_or_default()`, whose ABSENCE is then read as a content fact,
is the same shape `Pass 285.0`'s whole-buffer blank was. Not yet a standing
rule — two instances, different subsystems — worth a mint if a third
surfaces.

**Body section.** New `ARCHITECTURE.md` §10.6, sibling to §10.5 (decision
145). No `Cargo.toml` change (§3 unaffected); no writer-path change (§5
unaffected — read-time resolution only).

**Decision ceiling: `149` → `150`**, next free `151`. **Standing rules
ceiling unchanged at `R250`**, next free `R251` — `R225` gains an 18th
dated instance under its own existing number, no new rule minted. **Pass
ceiling `289.0` → `290.1`**, next free family `291.x`.

### 2026-09-11 (509th filing, `69d4d67`) — decision 151: **A REGION RENDER'S RASTERIZER CEILING IS PUBLISHED AS A MEASURED FLOOR, NEVER AS A DERIVED CONSTANT — THE BOUNDARY DOES NOT ORDER WITH ANY PAGE-GEOMETRY DIMENSION, SO `RenderError::RasterizerLimit` IS THE GUARANTEE, NOT `MAX_GUARANTEED_REGION_SCALE`'S VALUE**

**Sourcing (hard rule 8) — NO SHELL THIS FILING.** `Read`/`Grep` only. The
five commit hashes, the six-geometry measurement and the exact per-Pass
reasoning are relayed from the dispatching engineer's own summary, which
states plainly that it is an index, not the record — the commit messages
are authoritative and this role could not read them. **Independently
verified against the live tree by `Grep`, not taken on the summary's word
alone**: `RenderError::RasterizerLimit` and `MAX_GUARANTEED_REGION_SCALE`
exist in `crates/pdfcer-render/src/lib.rs`, with a dedicated regression test
(`tests/deep_zoom_refuses_instead_of_panicking.rs`) and measurement example
(`examples/region_panic_ceiling.rs`). **Not independently re-verified**: the
exact six measured geometries and their three boundary values, the workspace
test count, and `run-gates.sh`'s result — these are relayed.

**Origin.** Two duplicate inbound reports from `pdfcer-gui` (`G002`) named
the same symptom: a region render at an extreme scale panicked inside
tiny-skia's own rasterizer, taking its worker thread down rather than
returning an error.

**The choice, and why it is a decision rather than a bug fix.** The obvious
fix — catch the panic, name a maximum scale, done — has a trap this project
has been burned by before under a different name (`R213`, `Pass 74.x`'s
region-render precision work): a quantity true of one measured case gets
published as though it were true of all of them. Measuring the scale at
which tiny-skia's rasterizer actually breaks, across six page geometries,
found **three distinct boundary values that order with none of page width,
page area or device extent** — the largest sheet measured is the *most*
fragile, and a business-card-sized page shares a boundary A4 never reaches.
Publishing any single value as an exact ceiling would therefore have been
**invented**, not measured. The decision: publish `MAX_GUARANTEED_REGION_SCALE`
only as a **floor below the lowest observed failure**, and make the actual
guarantee the caught, named refusal (`RenderError::RasterizerLimit`) rather
than the constant — the refusal cannot be wrong because it is the failure
itself, caught and named, not a prediction of where the failure will occur.

**Relation to §10.1a (decision 089) — same discipline, one layer further
out.** §10.1a's obligation is: an operator-settable bound is safe only where
the allocation behind it is fallible, because an infallible allocator aborts
the process on a bound the machine cannot honour. This decision is the
converse failure mode on the same class of quantity — a **derived** bound
(not operator-set, but computed from measurement) that does not generalise
across inputs. Both are instances of the same underlying rule: **a number
attached to hardware/library behaviour that was not itself measured against
the full input space is not a guarantee, it is a hope with a value attached.**
No new decision was needed to state that rule generally; `R252` (below)
states it as a standing rule for this specific shape (a measured boundary
with no ordering variable) rather than widening decision 089 to cover it,
because the mechanisms differ (allocator fallibility vs. published-constant
honesty).

**Standing rule `R252` minted**: when a measured boundary does not order
with any input dimension (size, area, extent), publishing it as an exact
constant is an invented number wearing a measurement's clothes — publish it
as a floor below the lowest observed failure, and make the guarantee a
caught, named refusal rather than the number.

**Body-section effects.** New `ARCHITECTURE.md` §10.7, sibling to §10.6
(decision 150). No `Cargo.toml` change (§3 unaffected); no writer-path
change (§5 unaffected — this is render-time only, nothing is persisted).

**Decision ceiling: `150` → `151`**, next free `152`. **Standing rules
ceiling: `R250` → `R252`**, next free `R253` — `R251` also gains a dated
observation this filing (`ROADMAP.md` *Standing rules*) that this role
declined to file as a mechanical instance count; see that entry for the
reasoning. `R245` gains its 8th dated instance under its own existing
number, no new rule minted for that. **Pass ceiling `295.1` → `296.4`**,
next free family `297.x`.
