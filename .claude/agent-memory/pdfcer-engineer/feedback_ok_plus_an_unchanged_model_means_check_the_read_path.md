---
name: ok-plus-unchanged-model-means-check-the-read-path
description: A verb returning Ok while the model shows no change is almost always a stale MEMO, not a failed write — and a memo whose key omits a dependency makes every verb over it silently address the wrong object
metadata:
  type: feedback
---

When a verb returns `Ok` and the model afterwards looks unchanged, **suspect
the read path before the write path.** Twice in one session (2026-08-31) the
write was perfect and the answer came from a memo whose key could not see it.

**Why:** `EditSession::page_objects` memoises a page's decomposition. Its key
was `(first content object id, that object's staged span)` — which is a
*guess* at the dependency set, not the dependency set.

- `Pass 186.0`: `add_image` appends a **new** content stream and adds an
  `/XObject` resource. It touches neither key input, so the memo served the
  pre-insert model back. The image was on the canvas and absent from the model.
- `Pass 188.0`: a form-stream rewrite touches the page's `/Contents`,
  `/Resources` and dictionary not at all. All six new form verbs returned `Ok`
  while the model insisted nothing had happened.

**★ The reason this matters more than an ordinary cache bug:** the model is
addressed by INDEX. A stale model is not a stale *answer*, it is **an edit
applied to the wrong object** — and the index is in range on both sides, so
nothing can refuse.

**How to apply:**

1. **First diagnostic, before reading any write code:** call the read twice
   around the edit and print both. If the numbers are identical, it is the
   memo. This is one probe and it resolved both cases in a minute.
2. **A memo's key is the whole dependency set of what it caches.** For the
   decomposition that is: the page id, *every* `/Contents` entry with its
   staged span, the effective `/Resources`, **and every form the walk
   descended into**.
3. **★ Where an input cannot be named before the walk, take it from the walk's
   OUTPUT.** Which forms a page paints is a *result* of decomposing it, so it
   cannot go in a key computed beforehand. Record the ids the fill actually
   reached and re-read their spans on lookup. That is exact, and it is the
   only shape that works for a recursive dependency.
4. **A test that primes the cache is load-bearing.** `read → edit → read`
   catches this; `edit → read` does not, because a cold cache is always right.
   Say so at the test, or somebody will delete the priming call as redundant.

Sabotage confirms it cheaply: neuter the key comparison and count the reds.

Related: [[a-test-can-become-vacuous-later]] — a cold-cache test proves nothing
about a warm one. [[fixing-one-route-makes-the-others-look-broken]] — both
instances were found while fixing something else in the same cache.
