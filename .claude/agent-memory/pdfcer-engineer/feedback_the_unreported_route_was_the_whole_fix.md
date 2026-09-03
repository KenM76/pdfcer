---
name: the-unreported-route-was-the-whole-fix
description: R219 proof by measurement — the route added only because "enumerate every route" said so accounted for 100% of the fix on the reported patch; the reported route contributed nothing
metadata:
  type: feedback
---

Enumerating every route in the same Pass is not thoroughness insurance. On
2026-08-27 it was **the entire fix**.

**Why:** the bug report described a direct `Separation`/`DeviceN` image
bridging through sRGB. Walking `R219`'s checklist turned up a second,
unreported route in the same subsystem — an `/Indexed` image over such a base,
a duotone. I implemented both.

Ablated afterwards on a debug build:

| configuration | bridged | native |
|---|---|---|
| the reported route only (`/Indexed` route reverted) | 25,870 | 0 |
| both routes | 0 | 25,870 |

**The reported patch's images were `/Indexed` all along.** Had I implemented
only the route the report named, **the reported defect would not have been
fixed at all** — and every counter I would have quoted (`cargo test`, the gate
sweep) would have been green.

**How to apply:** when a report names a route, treat "which route does the
reporting artefact actually take?" as a question to **measure**, not to assume
from the report's wording. Cheapest form: ablate each route separately against
the reported file and see which one moves the number. The report describes the
symptom accurately and the mechanism only by guess.

★ And the corollary for reporting: I nearly wrote up the direct route as "the
fix" and the `/Indexed` route as "also enumerated, for completeness". That
would have been exactly backwards in the permanent record.

Related: [[feedback_fixing_one_route_makes_the_others_look_broken]],
[[feedback_verify_each_instance_not_the_class]],
[[feedback_priority_is_a_measurement]].
