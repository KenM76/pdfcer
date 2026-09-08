---
name: flag-values-arithmetic-and-cross-corpus-corrections
description: Never copy a bit-flag VALUE from a table cell — derive it as 2^(N-1) from the bit number; and a correction is not done until every corpus pdfcer READS is fixed, not just the tree that was wrong
metadata:
  type: feedback
---

Two rules, one incident (2026-09-08).

**Rule A — derive flag values, never copy them.** Any `/F`, `/Ff` or
permissions bit value written into this corpus is computed as **2^(N-1)**
from its bit number (PDF tables number bits from 1). Do not lift the value
out of a table cell, not even from `PDF_Spec`.

**Rule B — a correction propagates to every corpus the project READS.**
Fixing the tree that was wrong is half the job. `pdfcer` reads
`Acrobat_Features` and `PDF_Spec`; a value corrected only inside
`D:\Dev\pdfcer\docs\` walks straight back in from either of them.

**Why:** `markup__vertex_editing_and_reshape.md` carried
`LockedContents (bit 10, value 1024, PDF 1.7)`. Bit 10 is 2^9 = **512**;
1024 is bit 11, which ISO 32000-1 §12.5.3 Table 165 does not assign. The
phrase was copied verbatim into a pdfcer engineering dispatch and from
there into two committed doc-comment lines in
`crates/pdfcer-core/tests/locked_contents.rs`. It never appeared in a
commit message, so no reviewer of that change saw it. pdfcer had already
corrected the same value across its own `docs/` three days earlier — that
sweep stopped at the repository boundary. pdfcer minted standing rule
**R246** from the incident. Root cause is upstream at
`PDF_Spec/iso32000/iso32000__s__12.5.3.md:54`, a file that states the
2^(N-1) rule two lines above a table that then breaks it — which is
precisely why Rule A says derive rather than copy.

**How to apply:** when writing or reviewing any flag value here, do the
arithmetic in-line and say the bit number alongside the value (`bit 10,
value 512`) so the next reader can re-check it in one glance — writing
"bit 1024" (bit number conflated with value) is what let this one hide.
When told of an error in this corpus, ask whether the same value exists in
`PDF_Spec` and in `D:\Dev\pdfcer\`, and grep the WHOLE tree for sibling
values of the same table rather than editing only the reported line.

**Bonus fact worth keeping, since it is repeatedly mis-stated:** Table 165
bit 10 `LockedContents` covers the annotation's **contents — its comment
text (`/Contents`/`/RC`) — only**, and explicitly "does not restrict
deletion or other property changes." Colour, border style and opacity are
property changes and stay PERMITTED. `Locked` (bit 8, value 128) is the
near-complement: forbids deletion/position/size/properties, permits the
comment edit. A description of LockedContents as covering "contents
(colour, text, style)" is over-scoped and wrong.

Related: [[helpx-fetch-reliability]],
[[cross-reference-existing-spec-primaries-over-web-search]],
[[secondhand-claim-verification-dispatch]].
