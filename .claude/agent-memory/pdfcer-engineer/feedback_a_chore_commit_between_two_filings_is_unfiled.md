---
name: a-chore-commit-between-two-filings-is-unfiled
description: A version-bump / chore commit made AFTER one librarian dispatch and BEFORE the next is filed by neither; run check-commits-filed.py before every push, not just after a Pass
metadata:
  type: feedback
---

Before every push, run `python tools/check-commits-filed.py` — and name
EVERY code commit since the last filing in the librarian dispatch,
including `chore:` version bumps.

**Why:** on 2026-09-02 the v0.23.0 bump (`6876cd3`) was committed after the
384th filing's dispatch went out and before the 385th's was written. The
385th dispatch described Pass 242.0 only, so the bump was filed by neither.
CI on the pushed tip went red on `check-commits-filed`, and it took a
386th filing to clear it. The gate had been green locally at the 385th
filing's commit because I ran it BEFORE pushing the bump — the sweep
certified a tree that was not the one pushed.

**How to apply:** the dispatch is the drift point, not the librarian.
Between "commit code" and "push", `git log --oneline <last-filing>..HEAD`
and put every hash in the dispatch. A release is at least THREE code
commits' worth of filing (the Pass, the bump, the release-resolved
follow-on); the bump is the one that slips because it feels like
housekeeping.

**Recurred 2026-09-03 in a NEW shape: the gate RAN and was RED, and the
push happened anyway.** The chain was `python gate >/dev/null; echo rc=$?;
other && other && echo ok && git push` — the push depended on the two
later gates, not on the one whose rc I printed and then did not read.
`4319f06` (a docstring fix committed after the 393rd dispatch went out)
reached origin unfiled; a 394th filing was needed. **The fix is
structural, not attentional: `python tools/check-commits-filed.py &&
python tools/check-suite-name-absent.py && git push origin main` — every
pre-push gate in ONE `&&` chain that ends in the push, never a `;` and
never a separate echo.** Also: do not commit ANY code after a dispatch
goes out until that filing is committed; the docstring fix should have
waited or gone into the dispatch.
