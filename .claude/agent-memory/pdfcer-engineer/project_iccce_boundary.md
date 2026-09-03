---
name: iccce-boundary
description: iccce is the operator's sibling MIT colour-management project; decision 064 gives it ALL colour conversion — never propose a third-party CMM crate
metadata:
  type: project
---

**`D:\Dev\iccce\` is the operator's own from-scratch MIT ICC colour-management
project, created 2026-08-17, and its README names pdfce as its first
consumer.** `ARCHITECTURE.md` **decision 064** fixes the boundary:

> pdfce owns **COMPOSITING** — overprint, blend modes, transparency groups,
> and what a PDF's `/Separation`, `/DeviceN` or `/ICCBased` entry *means* in
> context. `iccce` owns **CONVERSION** — profile parsing, transform
> construction, rendering intents, ΔE.

**Why this memory exists:** on 2026-08-18 I commissioned research on the
CMYK→sRGB collapse, recommended **`moxcms`** as "the candidate of record",
and had the librarian file a `PRIOR_ART.md` row for it — **against a decision
made the day before.** There is no candidate slot for a third-party CMM. See
[[feedback-read-architecture-every-session]].

**How to apply:**
- **Any ICC / profile / colour-conversion need routes to `iccce`.** Do not
  evaluate CMM crates. `lcms2` stays rejected on its own separate merits (C
  binding, fails the wasm32 gate) — that rejection is unaffected and still
  correct.
- **`iccce` is not in any `Cargo.toml` yet, deliberately** — the compositor
  (`Pass 97.0`/`97.1`) must exist before there is anything to convert, and a
  dependency with no caller is the `R151` shape.
- **Cross-project channel: `D:\Dev\FeatureRequests\iccce_FeatureRequests\`**
  (`open/`, `archive/`, `INDEX.md`). It is a live, two-way conversation and it
  is **outside the repo**, so nothing in pdfce's own gates will remind you it
  exists. Check `open/` alongside `pdfce_FeatureRequests` — see
  [[project-gui-request-channel]].
- **Verify iccce's capability claims against its source, not its replies.** On
  2026-08-18 iccce retracted its own §3 capability sentence with *"ASK 1 —
  DONE. It has been done for some time, and our own reply is what misinformed
  you."* pdfce had two `ROADMAP.md` rows gated on that retracted blocker.

**What iccce ships that pdfce needs (verified in
`crates/iccce-cmm/src/transform.rs`, 2026-08-18):**
`Chain::with_destination(&src, Destination::None, intent)` — a built-in sRGB
destination **constructed from published constants** (BT.709-6, W3C transfer
constants, Bradford to D50 per ICC.1:2022 Annex E.3). **No shipped `.icc`**,
so the profile-redistribution objection in `LEGAL.md` §6.1 does not arise.

**Two traps to carry into any adopting Pass:**
1. **`Destination::None` is NOT `Option::None`** — it is a caller *assertion*
   that no destination exists. A destination that WAS declared and failed to
   parse must become a **propagated refusal**, never a fallback. A PDF/X page
   silently rendered to substituted sRGB looks completely normal. Surface
   `destination_provenance()`.
2. **~1.4 Mpix/s ≈ 6 s/page vs pdfce's ~0.6 s render — about 10×.** A `u8`
   buffer surface would fix memory (4× in, 8× out) and **nothing** for time.
   So the collapse cannot be an unconditional per-frame step: export-only,
   cached, or scoped to pages that actually use overprint.

**Owed by pdfce, unread as of 2026-08-18:**
`request_profile_population_census.md` and
`request_header_tag_channel_disagreement.md`.
