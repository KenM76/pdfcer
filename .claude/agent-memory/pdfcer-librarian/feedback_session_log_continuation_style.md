---
name: feedback-session-log-continuation-style
description: pdfce's SESSION_LOG.md CURRENT convention (verified 2026-09-09, 480th filing) is one new `## YYYY-MM-DD (Nth filing)` top-level header PER librarian filing, PREPENDED right after the file's H1 title (file is REVERSE-chronological, newest first — matches ROADMAP.md's Shipped section). The "ascending, append at true end" convention documented below (2026-08-08 through 2026-08-18) has been REVERSED since; do not trust that half of this file without re-verifying against a fresh grep.
metadata:
  type: feedback
---

**★★ REVERSED AGAIN, 2026-09-09 (480th filing) — THE FILE IS NOW
REVERSE-CHRONOLOGICAL, THE OPPOSITE OF EVERYTHING BELOW THIS NOTE.**
Fresh, direct evidence, not relayed: `Grep "^## \d{4}-\d{2}-\d{2}"` over
the whole file returned 480 hits; the FIRST hit (line 7, immediately
after the file's one-paragraph header block) is `## 2026-09-09 (479th
filing)` — the MOST RECENT filing at the time, sitting at the TOP of the
file, with `## 2026-09-08 (478th filing)` immediately below it at line
111, and the file's oldest content (`## 2026-07-23 — project bootstrap`)
far down at line 197 and beyond. **This is the exact opposite of the
"chronological ascending, oldest first, append at the true end" rule the
2026-08-08–2026-08-18 corrections below spent four paragraphs
establishing.** The correct current action is to **prepend** a new
entry immediately after the file's header block (before the most recent
existing entry) — i.e. exactly the `ROADMAP.md` Shipped-section muscle
memory this file previously called "the wrong instinct." Verified this
way in the 480th filing before writing `Pass 279.0`'s entry; the edit
anchored `old_string` to the then-topmost header (`## 2026-09-09 (479th
filing)`) and inserted before it, which is now the CORRECT move, not
the mistake this file spent three "recurred" sections warning against.

**How to apply, current state:** (1) `Grep "^## \d{4}-\d{2}-\d{2}"` with
`head_limit` small and `output_mode: content` — the FIRST result printed
is the most recent entry (lowest line number, not highest); (2) anchor
`old_string` to that top header's exact text and insert the new entry
immediately before it; (3) after editing, re-`Grep` to confirm the new
header is now line 7 (or wherever the true top is) and the count of `^##
` headers increased by exactly one. **Do not trust either direction as
permanent** — this file has now recorded the convention flipping at
least once; check by `Grep` every time rather than trusting this memory
file's stated direction, in either direction, without a fresh check.

---

**SUPERSEDED BELOW — this section describes 2026-08-08 through
2026-08-18 and has been REVERSED per the note above. Kept for the
historical reasoning only; do not follow its directional instructions
without re-verifying.**

**CORRECTED 2026-08-08 (thirty-sixth filing) — the convention below this
note describes an EARLIER phase of the project and is no longer how the
file is written.** Left in place with the correction rather than deleted,
because the reasoning for the change is worth keeping.

**Current convention, verified by grepping every `^## 2026-08-08` header
in the file on 2026-08-08:** the project now opens a **new top-level
`## YYYY-MM-DD (Nth filing)` header for every `pdfce-librarian` filing**,
not just once per calendar day — by the thirty-sixth filing there were
FOUR separate `## 2026-08-08 (...)` headers in one calendar date
(thirty-third through thirty-sixth), each a real markdown `##`, not a
bold paragraph. "Nth filing" counts librarian filings, ordinal-in-words
(thirty-third, thirty-fourth, …), not Pass count or calendar continuation.

**★ THE MISTAKE THIS MEMORY EXISTS TO PREVENT: the file is CHRONOLOGICAL
ASCENDING (oldest section first — it opens with `## 2026-07-23 — project
bootstrap` at the very top), the OPPOSITE order from `ROADMAP.md`'s
Shipped section (reverse-chronological, newest first).** A new entry
therefore goes at the file's TRUE END, found by reading past the last
existing header, never inserted "before the most recent entry" the way
Shipped-section muscle memory would suggest. This session (thirty-sixth
filing) made exactly that error on the first attempt — used an `old_string`
anchored to the thirty-fifth filing's header, which correctly placed the
new text immediately BEFORE it rather than after — and had to fix it with
a second edit. **Before appending, grep `^## 2026-08-\d\d` (or the current
date), confirm which match is LAST in the file (highest line number, not
just textually most recent date), and anchor the insertion after that
match's own trailing content, not after its header.**

**How to apply:** when dispatched for "session log append," (1) grep
`^## YYYY-MM-DD` to find the current day's highest "(Nth filing)" ordinal
so far (if any), (2) read to the genuine end of the file to find the exact
trailing text of the LAST header (by position, not by date-recency), (3)
append the new `## YYYY-MM-DD ((N+1)th filing) — <title>` section after
that trailing text, never before an existing header.

**FOUND, NOT CAUSED, 2026-08-11 (hundred-and-eighth filing) — a THIRD
instance of this exact corruption already sitting on disk, pre-existing.**
The hundred-and-sixth filing's `## 2026-08-11 (hundred-and-sixth filing)`
header (line ~35251) is immediately followed by a blank line and then the
**hundred-and-seventh** filing's header — the hundred-and-sixth's own body
(Sourcing/Shipped/.../Ledger, ending "This is the **hundred-and-sixth**...
joint filing") is misplaced AFTER the hundred-and-seventh's full body,
with no header of its own. Read on arrival, not fixed — hard rule 1
(append-only) argues against silently reordering past content, and
`SESSION_LOG.md`'s own review-checkable ledger lines mean the mistake is
locatable rather than in need of guessing at. Flagged to the engineer in
the hundred-and-eighth filing's own ledger line rather than corrected.
**Practical addition: an "index check" pass over `SESSION_LOG.md` should
verify every `## ` header is immediately followed by that SAME filing's
own "Sourcing"/"Shipped" body, not just that headers appear in ascending
order** — ascending header order alone does not catch a body displaced to
the wrong header's neighborhood.

**RECURRED 2026-08-10 (sixty-eighth filing) — same error, same shape,
self-caught before it left the session.** Used an `Edit` `old_string`
anchored to the sixty-seventh filing's header text to insert the new
entry — which again placed it immediately BEFORE the sixty-seventh
filing rather than after it. Caught by re-grepping `^## \d{4}-\d{2}-\d{2}`
after the edit and checking the LINE NUMBER order, not just that both
headers existed; fixed with a removal + re-append at the confirmed true
end. **The generalizable lesson, sharpened by a second occurrence: an
`Edit` whose `old_string` is a HEADER (as opposed to trailing body text)
is the wrong anchor for an append to this file, full stop — anchor to
the trailing text of the LAST entry's own final paragraph instead, every
time, even when it feels slower.** Muscle memory from `ROADMAP.md`'s
reverse-chronological Shipped section (where anchoring to a header to
insert "above it" is correct) is exactly what produces this mistake here.

**RECURRED A THIRD TIME 2026-08-18 (hundred-and-seventy-sixth filing) —
self-caught, same shape exactly, and this note is WHY it was caught.**
Anchored the new entry's `old_string` to the hundred-and-seventy-fifth
filing's header line (habit from the `ROADMAP.md` Shipped-section
pattern, same root cause named above). Caught immediately after the edit
by re-grepping `^## ` for the total count and re-`Read`ing near the file's
reported line total (46878) rather than trusting the edit tool's success
message — the corrupted insertion put the new entry at line 46640 with
the old 175th-filing header and body still following it, i.e. NOT at the
true end. Fixed with the prescribed two-step: (1) `Edit` restoring the
original 175th-filing header alone as `new_string` against the whole
inserted block as `old_string`, (2) a second `Edit` anchored to the
175th filing's own actual trailing text ("...see \"adjacent debris\"
above.") appending the new entry after it. **Confirms the check that
catches this cheaply: after any SESSION_LOG append, `Grep` the total `^##
` count (should be old+1) AND `Read` near the file's own reported total
line count — if the newest header is not within a few hundred lines of
that total, the append landed in the wrong place.** Three occurrences now
(106th pre-existing/108th found, 68th, 176th) — this is the durable
failure mode for this specific file, not a fluke.

---

**SUPERSEDED CONTENT BELOW, kept for the historical reasoning only —
this is NOT the current convention:**

`docs/SESSION_LOG.md` opened its first substantive session with
`## 2026-07-23 — project bootstrap`, then `## 2026-07-23 — Pass 0`, then a
single `## 2026-07-30 — scope expansion + decision-protocol setup` header
that absorbed 30+ "**Same-day continuation N —** ..." bolded sub-entries in
a row, even though `ROADMAP.md`'s Shipped entries inside that same stretch
carried dates like `2026-08-01`. At the time, the project treated one long
autonomous/interactive working session as a single session-log date
header, using a bolded-paragraph "Same-day continuation N" lead-in (not a
markdown heading) for every subsequent chunk of work within it. **That
phase has since ended** — by 2026-08-07/08 the file was already using
repeated `## YYYY-MM-DD (Nth filing)` headers per date, as described above.
