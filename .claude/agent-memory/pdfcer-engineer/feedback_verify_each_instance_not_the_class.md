---
name: verify-each-instance-not-the-class
description: Running one instance of a change and generalising the result to its siblings is how a defect ships beside a verified twin — verify each, or use a gate that does
metadata:
  type: feedback
---

When a change produces **several instances of the same kind of thing**, an
oracle run against one of them does not cover the others. Run each, or find
an instrument that covers the class.

**Why:** on 2026-08-18 I added two `RenderError` variants twelve lines apart
in one commit. I verified `PageNotRecordable`'s message by *running the
binary and reading it* — the right method, and the one my own notes prescribe
for string literals. Then I treated the method as discharged for the commit
and never ran `DisplayListStale`. It shipped with ten literal spaces baked
into the middle of its sentence, from a `\` line-continuation the patch
tooling ate.

The librarian found it in a sweep. The shape is what makes it worth a memory
rather than a shrug: **the verified twin is what creates the confidence.**
Having watched one message render correctly, I was not careless about the
second — I believed it was already covered, which is a different and more
durable error.

This is the same failure the `check-ledger-numbers.py` star anchor has now
made twice: the first fix accepted `★ ` because `★ ` was the spelling that
had been seen, and `★★ ` stayed invisible for another eight months. Repairing
the instance in front of you is not repairing the class.

**★ FIVE INSTANCES IN ONE SESSION (2026-08-18), same author, same day.** The
count is the point — this is not an occasional slip, it is the default
failure mode of fixing things:

1. `PageNotRecordable` verified by running it; `DisplayListStale`, twelve
   lines away in the same commit, shipped broken.
2. The 8.5× headroom claim corrected in `display_list.rs`; missed in
   `guard_probe.rs`, written in the same commit, in the same hour — **by the
   very harness whose own run had disproved it.**
3. The ledger gate's star anchor, fixed once to accept `★`, still blind to
   `★★` — hiding a real duplicate Pass number for months.
4. **A gate I wrote to catch a stale count read exactly one file** — the one
   I happened to be editing — and went green while the front door of the same
   directory carried the stale count ten lines away.
5. The widened gate's first run then found a **fourth** stale count *inside
   the file I had already fixed an hour earlier*. Four existed; I had
   corrected one.

**Four of the five were found by somebody else's sweep or by a widened
instrument. None was found by me re-reading my own work.** That is the
argument for gates over care, and it is now a data point rather than an
opinion.

Instance 4 is the one to remember: **the instrument inherited the bug it was
built to catch**, because its scope was set by whatever was in front of me
while I wrote it.

**★★ RECURRED 2026-08-27, AND THE INSTANCE I SKIPPED WAS THE MORE REACHABLE
ONE.** A consuming project reported that `preview_font_resources(page, "",
Some(pin))` surveyed zero characters and reported every font as accepted. I
fixed the **pinned** case — and *reasoned* that the **unpinned** case already
errored, because `match_run` refuses an empty `find` on the commit path.

It does not. `find_anchor` with no pin runs `s.text.contains(find)`, and
**every string contains the empty string**, so an unpinned empty `find`
silently matched the first show operator on the page and surveyed against
nothing. Same defect, no pin required — i.e. reachable *by accident* rather
than by following documentation, which makes it the **more** likely one to be
hit.

The reporter had actually offered that half as their alternative remedy
("refuse an empty `find` by name") and I read it as an either/or. **Both were
needed and I had one.**

A test I wrote expecting it to pass is what caught it. That is the pattern
worth keeping: when you catch yourself writing *"unchanged, and it must stay
unchanged"* in a test comment, you are asserting a belief you have not
measured. Write the test anyway — it costs nothing and it is the only thing
that distinguishes a true assumption from a false one.

**How to apply:**
- Ask *"how many of these did I just create?"* before declaring an oracle
  discharged. Two is enough for this to bite.
- **When writing a gate, set its scope from the CLASS, never from the
  instance that prompted it.** If the defect was in one file, the gate reads
  the directory. If it was in one spelling, the gate accepts all of them. The
  prompting instance is the least interesting member of the set.
- Prefer a **gate over a habit** when the class is syntactic. Reading each
  message aloud does not scale; `tools/check-string-gaps.sh` does, and it
  found 44 more the same afternoon.
- **Run the new gate before fixing the thing that prompted it**, so its first
  output is the full census. Both gates written this day found more than the
  one defect that motivated them.

Related: [[windows-paths-need-literal-edits]] (the mechanism that eats the
backslash), [[gates-i-owe-myself]] (the gates I skip), and
[[two-modes-one-pattern-is-one-measurement]] — same family: agreement
between things produced the same way is not verification.
