---
name: security-defaults-lean-safe-plus-stricter-mode
description: On security/privacy features Ken takes the safer default AND asks for an optional stricter mode on top — propose the strict option, don't assume he wants maximum permissiveness
metadata:
  type: feedback
---

On any **security- or privacy-touching** capability, Ken's pattern is
consistent: he accepts the safer default when it is offered, and then **adds a
stricter optional mode himself**. Offer the strict option proactively; do not
read an earlier permissive ruling as a preference for permissiveness.

**Why:** observed three times in one plan-only conversation (2026-08-26, form
submission / network scoping):

1. He ruled submission destinations wide open — *"we'll allow a submit to send
   filled data wherever the document's author said."*
2. Offered a disclose-the-destination default, he took it — *"your default is
   good."*
3. Then, unprompted, he **added two restrictions I had not proposed**:
   *"we'll have support for allowing only submission to whitelists, and … we
   can show what is being sent as an option too."*

The permissive ruling in step 1 read, at the time, like a preference for
minimal friction. It was not — it was a ruling on the **default**, and he
expected the **stricter modes to exist alongside it**. Reading step 1 as "he
wants this unrestricted" would have produced a design missing both of the
things he actually wanted.

**How to apply:** when scoping anything that sends data, executes code, reaches
the network, or weakens an existing refusal — present it as a **posture ladder**
(permissive default → disclosure → restriction), not as a single yes/no. He
picks the rung for the default and expects the other rungs built. This is the
security-flavoured instance of [[spec-ambiguity-defaults-are-mine]]: that one
assigns me the default, this one says the *non*-default rungs still get built.

Consistent with [[keep-the-error-catching]] — he rejects trades that give up
detection — and with
[[exceed-the-parity-reference-when-you-can]]: a local operator-owned whitelist
is **stronger than Adobe's** cross-domain model, where the destination host
serves the file that grants itself access.
