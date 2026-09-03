# XFA implementation survey — is there anything pdfcer could build on?

**Written 2026-08-21**, in answer to an operator question: *"is there a
dynamic XFA form rust library you can use to add support? gemini mentioned
one on git that is also mit license."*

**Answer: no. There is no MIT-licensed Rust dynamic-XFA library.** There is
no Rust dynamic-XFA library under any licence, and no complete FormCalc
implementation in Rust in any licence.

This file exists because the question will be asked again — by a future
session, or by another model — and because *why* the answer is no turns out
to be a better argument than the answer.

★ **Scope note:** this survey is about **availability**, not about whether
pdfcer wants the feature. That was decided separately and earlier —
`docs/decisions/020-form-field-authoring.md` §3.2.1 rules dynamic XFA
`out_of_scope` on four independent grounds, and `ROADMAP.md` open question
**(p)** is the operator's to answer. Nothing here overturns that; it only
removes "but a library exists" from the discussion.

---

## 1. The searches, and what they returned

- **crates.io** — `xfa`, `xdp`, `formcalc`, `xfa-forms`, `pdf-xfa`: nothing
  that implements XFA.
- **GitHub, Rust** — the only MIT Rust codebase containing real XFA code
  **disclaims dynamic XFA in its own manifest**, and it is worth being
  precise about what it *does* have, because "nothing exists" is the wrong
  summary. Its XFA module's own header reads:

  > *"Bounded XFA packet discovery, static extraction, minimal dynamic
  > layout, and active-content sandbox policy. This module deliberately
  > implements a useful subset rather than an Adobe LiveCycle/AEM
  > compatibility claim. XML is parsed without DTDs or external entities;
  > scripts are disabled by default; JavaScript is always inventory only;
  > and every recursive or generated structure is capped."*

  Its capability struct then declares `dynamic_xfa: false` and
  `dynamic_reflowing_subforms: false`. ★ **So the honest finding is not
  "no Rust project touches XFA" — it is that the one which does says NO to
  the dynamic half itself, in a field, in its own source.** That is a
  well-behaved project stating its limits, and it is the opposite of the
  description-versus-source mismatch in §2.

  ⚠ **The crate is not on crates.io** (a `crates.io` API fetch for the name
  its error type implies returns nothing), so the licence claim above is
  the researching agent's reading of a `LICENSE` file and has **not** been
  re-verified here. Do not act on the licence without checking it.
- **GitHub, "FormCalc", all languages** — 61 repositories, **none** of them
  an XFA FormCalc implementation. Every hit is either the Feynman-diagram
  physics package of the same name or a generic form calculator.
- **The Rust PDF ecosystem** — `lopdf`, `pdf-rs`, `oxidize-pdf`, `hayro`,
  `printpdf`, `pdf-writer`, `krilla`: none claims XFA.
  `pdfium-render` binds PDFium, which *does* have XFA — see §3.

## 2. ★ THE PATTERN THAT PROBABLY PRODUCED THE CLAIM, and it is systematic

Projects in this corner advertise XFA support in their **repository
description** and disclaim it in their **source**:

| project | description says | source says |
|---|---|---|
| `quorbe/xfa.js` | *"script & fill"* | its scripting module `throw`s *"not implemented yet (Phase 3)"*. 12 commits over 8 days, then dormant |
| `benedoc-inc/pdfer` | *"comprehensive XFA support"* | its own `xfa-scope.md` lists **"Layout engine"** and **"Script execution"** as explicit **non-goals** |

A model summarising repository metadata will report the first column. **The
first column is marketing.** Any future XFA claim should be checked against
the second column before it is repeated — and that is a general rule about
this ecosystem, not a complaint about one model.

## 3. What actually implements dynamic XFA, and what it costs

### PDFium — the only complete FormCalc implementation in open source, anywhere

`xfa/fxfa/formcalc/` holds a lexer, a parser and an expression tree, and
every expression node declares:

```cpp
virtual bool ToJavaScript(WideTextBuffer*, ReturnType) const = 0;   // 88 impls
```

**PDFium compiles FormCalc to JavaScript and executes it on V8.** That is
the design, and it is why XFA is chained to V8 throughout the build:

```
pdf_enable_xfa = $ENABLE_V8
```

⇒ **"Adding XFA support" means shipping a JavaScript engine.** Not as an
optional extra — as the execution substrate for the form's own scripting
language. For pdfcer that is a V8-sized dependency in a project whose
engine crates must also compile to `wasm32`.

Licence is BSD-3-Clause, so it is not a *licence* blocker. It is a C++
blocker and a V8 blocker.

### Chromium — compiles it in, then turns it off

`build_overrides/pdfium.gni` enables XFA on ChromeOS, Linux, macOS and
Windows; `pdf/pdf_features.cc` then declares
`BASE_FEATURE(kPdfXfaSupport, base::FEATURE_DISABLED_BY_DEFAULT)`, reachable
at `chrome://flags/#pdf-xfa-forms`. The enterprise policy
`PdfXfaFormsEnabled` is registered only under `IS_CHROMEOS`.

★ **And this is live work, not an abandoned experiment** — the policy
landed 2025-11-11 and *"Always build all XFA codec fuzzers when XFA"* landed
2026-08-10. Worth recording because it is the one fact in this survey that
cuts *against* "XFA is dead", and a survey that only collected agreeing
evidence would not be worth keeping.

### pdf.js — further from working than "partial" suggests

- The FormCalc parser is imported **only by a unit test** and has **no code
  generation at all** — only `dump()` to JSON.
- **XFA JavaScript is not executed either.** `template.js`'s `Script` class
  is a pure data holder; `scripting_api/doc.js` hardcodes
  `get dynamicXFAForm() { return false; }`; `exportXFAData()` and
  `importXFAData()` are both `/* Not implemented */`.
- `src/core/xfa/som.js` states the gap in a comment: *"XFA - SOM expression
  contains a FormCalc subexpression which is not supported for now."*
- The **layout** half is real (`handleOverflow`, `overflowLeader`/`Target`/
  `Trailer`, `$isSplittable`, `breakBefore`/`breakAfter`) — but
  `instanceManager` **appears nowhere in the repository**, so `<occur>` is
  parsed and never acted on. There is no add-row. That is issue #15256.
- It renders XFA through an **HTML layer**, not the canvas engine, so there
  is no rasterisation path for XFA content at all.
- **Frozen.** The last 15 commits touching `src/core/xfa` (through
  2026-08-18) are codebase-wide mechanical refactors — optional chaining,
  ESLint rollouts. The only maintainer characterisation on record is from
  2021: *"XFA support is being developed and experimental."* Never
  superseded. The README does not mention XFA.

## 4. The commercial reality, which is the most persuasive column

| vendor | position |
|---|---|
| **Foxit** | the **only independently-built** commercial dynamic XFA renderer (`XFADoc`, `e_Dynamic`/`e_Static`, engine-computed `GetPageCount()`, `StartRenderXFAPage`). Separate module licence. |
| **Datalogics** | *"built on the same rendering engine as Adobe Acrobat"* — Adobe's engine relicensed, **not a second implementation** |
| **iText** (`pdfXFA`) | **flatten-only**: a real layout engine producing static output |
| **Apryse / PDFTron** | does **not** support it, and its documented workaround **shells out to Adobe Reader from the command line**: *"Adobe Reader is the only reliable software to handle the deprecated XFA format."* |
| **Syncfusion, Nutrient, Qoppa** | refuse outright. Qoppa: *"There are very few PDF viewers that support XFA Dynamic Forms, one can count them on the fingers of one hand."* |
| **PDFBox** | logs `"Flatten for a dynamix XFA form is not supported"` [sic] |

⇒ **A commercial PDF SDK's documented answer to dynamic XFA is to invoke
Adobe Reader as a subprocess.** That is the size of the problem, stated by
somebody with a revenue incentive to have solved it.

## 5. What would have to be built, if the answer were ever yes

Not a renderer. An interpreter for a second document format that happens to
live inside a PDF:

1. **Three DOMs** — template, data, form — plus the binding rules between
   them.
2. **A layout engine** that reflows and paginates *at render time*. "Dynamic"
   means the page count is not known until the data is bound; `<occur>` and
   `instanceManager` mean rows appear and disappear.
3. **Two scripting languages** — FormCalc **and** JavaScript — in XFA's own
   object model, with SOM expression resolution across them.
4. Rich text, images, barcodes and XFA-specific digital signatures.
5. A rasterisation path, since XFA content is not PDF content streams.

## 6. Evidence quality, stated rather than assumed

- **Verified by fetching source or vendor documentation:** everything in
  §2, §3 and §4 above.
- **Independently reproduced** by a second agent: the
  `pdf_enable_xfa = $ENABLE_V8` line, and that pdf.js's `formcalc_parser` is
  imported only by a unit test.
- **⚠ NOT verified:** LibreOffice and macOS Preview. No vendor statement
  could be fetched for either (TDF wiki denied access; Apple's PDFKit docs
  returned only page chrome). "No XFA support" for those two is **inference,
  not sourcing.**
- **⚠ Treat Adobe's own claim with care:** *"XFA forms are not supported by
  anything other than the desktop versions of Adobe Acrobat Reader and
  Adobe Acrobat"* is Adobe-authored and **demonstrably overstated** — it
  ignores Foxit, PDFium and pdf.js.

## 7. Bearing on ROADMAP open question (p)

Question (p) asks whether to retire or re-scope the standing
"verify XFA deprecation status" item. **This survey does not answer it** —
it answers a different question (availability) and is filed so that the
next person to consider (p) does not have to re-run these searches.

What it adds to the record: the *availability* argument, which decision 020
did not have, and which points the same way as the four grounds it did
have. And one counter-fact — Chromium's XFA work is active as of
2026-08-10 — recorded because a survey that suppressed it would be worth
less than one that did not.
