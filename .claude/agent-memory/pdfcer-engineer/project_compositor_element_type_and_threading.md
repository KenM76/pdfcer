---
name: compositor-element-type-and-threading
description: 2026-08-21 — Ken asked directly about f64 and multithreading mid-build; both answered with measurements, both decided, and the threading answer carries a wasm gate hole a compile check cannot see
metadata:
  type: project
---

**Ken paused a build in progress to ask two cross-cutting architecture
questions**: *"can we do 64 bit, and is this where we want to include
multi threading?"* Both were answered with numbers rather than preference,
and both are now settled.

## `f32`, not `f64` — decided, and the switch is one line

`crates/pdfce-render/src/cmyk_buffer.rs`, `pub(crate) type Chan = f32`,
deliberately a single alias.

**Why**, so it is not re-litigated: floats are needed at all because
§11.4.4's backdrop removal contains a `1/α_gn`, where an 8-bit half-level
error becomes **25 levels** at `α_gn = 0.02`. The same amplified error in
`f32` is ~`1.5e-6`, about **1/2600th of one 8-bit level**, and the final
quantisation to 8 bits dominates by three orders of magnitude. `f64` doubles
memory (24 → 48 B/px; a letter page at 300 DPI is 193 → 385 MB) and halves
SIMD lanes to shrink an already-invisible error. Every surveyed production
engine composites in 8- or 16-bit **integer**, so `f32` is already the most
precise implementation in the field.

The one real argument for `f64` — `iccce`'s evaluation surface is `f64`-only
— does not survive: widening `f32`→`f64` is **exact**, and it happens once
per pixel at the collapse, not inside the blend loop. `iccce` was told, in
their channel, per their explicit request.

## Threading — NOT in the engine, and there is a gate hole

pdfce had **no** threading anywhere as of this date — not `pdfce-core`, not
`pdfce-render`, not `pdfce-cli`.

**★ THE FINDING THAT MAKES THIS AN ARCHITECTURE DECISION RATHER THAN A
PREFERENCE, verified empirically with a throwaway crate:
`std::thread::spawn` AND `rayon` BOTH COMPILE CLEANLY FOR
`wasm32-unknown-unknown`.** So CI's `cargo check --target
wasm32-unknown-unknown` job — the gate that protects the web-fork invariant
— **cannot catch a threading regression**. It stays green while the web
build fails at runtime. That is `R209`'s shape (an unobserved gate reading as
a passing one) in a new place.

**Where the speed actually is:** page-level parallelism in `pdfce-cli`,
which is outside the wasm boundary entirely, embarrassingly parallel, and
what would shorten the 4,000-file corpus sweeps this project runs
constantly. Inside the compositor the work is memory-bandwidth-bound and
wants banding first.

**How to apply:** if threading is ever proposed for `pdfce-core` or
`pdfce-render`, it needs a written rule (a dependency denylist, like the
`no-network` job) — **not** a reliance on the wasm compile gate, which will
not object. Propose it in the shells.

See [[compositor-state]].
