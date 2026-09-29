---
name: feature-request-channels
description: at session start check THREE inbound channels — `gh issue list` on KenM76/pdfcer, plus ls pdfce_FeatureRequests\open\ and iccce_FeatureRequests\open\ — they are outside the repo, so no gate can ever contradict a stale claim about them
metadata:
  type: project
---

Two cross-project request channels bear on pdfce, and **both must be listed
at session start**:

| channel | direction |
|---|---|
| `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\` | inbound from `pdfceGUI` |
| `D:\Dev\FeatureRequests\iccce_FeatureRequests\open\` | the `iccce` colour project |

**Why:** on 2026-08-18 the session handoff stated *"`pdfce_FeatureRequests/open/`
is EMPTY — nothing owed to `pdfceGUI`."* It was not empty. It held a detailed
correctness report about `insert_pages`, filed the same day that code shipped.
I repeated the claim to the librarian before checking it.

The same handoff said the `iccce` channel held **two** owed requests; it holds
**five** files. That count had been carried forward, unverified, for three
consecutive filings.

**★ The reason this recurs is structural, not careless.** Those directories
are **outside the git repository**, so:

- no gate walks them — `check-commits-filed`, `check-passes-filed` and
  `check-ledger-numbers` all read files under `D:\Dev\pdfce\`;
- a sentence in `NEXT_SESSION.md` asserting the channel is empty is therefore
  **unfalsifiable by any tooling this project owns**, and it propagates by
  being copied into the next handoff;
- the failure is silent in the worst direction: an unread request looks
  exactly like no request.

Every omission-detector pdfce owns points *inward*. This gap is between two
project trees, which is precisely where nothing is looking — the same shape as
the cross-RAG hand-off failure the engineer role file already records.

**How to apply:** `ls` both `open/` directories at session start, before
reading `NEXT_SESSION.md`'s claims about them, and treat any count in a
handoff as hearsay until re-listed. If a request is found, parse it into Pass
entries and dispatch the librarian the same way as an operator request — an
inbound request from a consuming project *is* an operator request, just
arriving through a different door.

## A third channel: GitHub issues (added 2026-09-28)

`gh issue list --state open` on the public repo. Issue #1 (external
reporter, OCR text layer misplaced on a page that leaves a `cm` in effect)
sat unanswered for six days, 2026-09-22 → 2026-09-28, while every session
checked only the two folders above. Ken had to ask. A public repo's issue
tracker is the channel a stranger uses; it is the one most costly to miss.
Posting a reply there is a public side effect — ask Ken first.
