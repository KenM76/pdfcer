---
name: an-extrapolation-reported-as-a-measurement
description: Weighting a deduplicated record by its occurrence count assumes the thing being tested; it reported 91 where the measured answer was 2. And fixing it in the aggregate left it alive in the detail export.
metadata:
  type: feedback
---

# A per-group sample weighted by group size is an EXTRAPOLATION, and it wears the same clothes as a measurement

Recorded 2026-08-27, from `tools/icc-census`, before any number left the
repository.

## What happened

The census deduplicated ICC profiles by content fingerprint, keeping one
record per distinct profile plus an embedding count. To report "how often
does a PDF's `/N` disagree with the profile's own channel count?" I
multiplied **each distinct profile's first-seen `/N`** by its embedding
count.

It reported **91 disagreements**. The measured figure, tallied per
embedding, is **2**. Wrong by 45×.

## Why it is a trap and not just a bug

**The weighting silently assumes the very thing the axis exists to test.**
`/N` lives on the *stream*; the profile bytes are *shared*. Two embeddings of
one profile can declare different `/N` — that is exactly the phenomenon being
measured — so weighting one sample by the group size presumes they do not.
The number is circular, and it is circular in a way that produces a
plausible, quotable figure rather than an obvious error.

**Nothing catches this.** Not a type, not a test, not a gate. It is
arithmetic over correct data with a wrong premise.

## ★★ The half that is genuinely worth carrying: fixing the aggregate did NOT fix the export

After adding a per-embedding counter, the aggregate was right — and the TSV's
`pdf_n` column, one row per *distinct* profile, was **still a first-seen
sample**. I then wrote a sentence from that column ("the single culprit is
one `Coated FOGRA27` stream declaring `/N 3`") which the column could not
support. Named properly, it is **two profiles and two directions** — the
second being a PDF declaring *four* channels for three-channel Adobe RGB,
which is the more surprising finding and was invisible.

> **Correcting an extrapolation in an aggregate does nothing about the same
> extrapolation in the detail export, and nothing connects the two.**

The repair that actually works is **naming the column so the mistake is
unavailable**: `pdf_n` → `pdf_n_FIRST_SEEN`. A comment saying "do not read
this as a population value" would have been read by nobody, including me,
twice in one session.

## How to apply

Whenever a report deduplicates and then weights:

1. **Ask what varies WITHIN a group.** If the field being reported can differ
   between members of the group, one member's value cannot stand for all of
   them — tally it per member.
2. **Check the detail export separately.** It is a second implementation of
   the same summary and it does not inherit the fix.
3. **Name any first-seen / representative field so.** `foo_FIRST_SEEN`, not
   `foo`.
4. **Print the individuals, not just the count.** "There are two" cannot be
   investigated. Naming them is what surfaced that both directions occur.

## The third one, same session, same tool

A bucket labelled `"agree, or no LUT tag to compare"` reported **100 % across
2,494 embeddings** — which reads as a strong negative result and is *equally
consistent with the check never having run*. Split into three states, the
honest answer was 136 checkable / 136 agreeing / 2,358 not checkable.

All three were caught by **reading the output**, never by a test. See
[[feedback_a_gate_that_underreports_looks_green]] — same class: a number that
cannot distinguish "clean" from "not measured".

Related: [[feedback_priority_is_a_measurement]],
[[feedback_a_claim_about_callers_is_a_measurement]],
[[feedback_two_modes_one_pattern_is_one_measurement]].
