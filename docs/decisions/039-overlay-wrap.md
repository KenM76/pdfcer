# Decision 039 — Overlays appended to a page are isolated from the state its content leaves behind

- **Date:** 2026-09-28
- **Status:** DECIDED and shipped.
- **Authored by:** `autonomous-builder` / KenAgent (design "A+ hybrid"),
  with one engineer amendment (§3.3).
- **Trigger:** GitHub issue #1 — OCR text and added text landed offset and
  scaled on a page whose first content stream is `1.1 0 0 1.1 0 0 cm`,
  never restored.
- **Clauses:** ISO 32000-2 §7.8.2 (a `/Contents` array is one stream;
  stream boundaries are token boundaries), §8.4.2 (`q`/`Q` need only
  balance), Table 30 (`/Contents` shapes).
- **Code radius:** `pdfcer_model::page_tree::{plan_overlay_append,
  OverlayAppend, remove_overlays, is_state_neutral, WRAP_SAVE,
  WRAP_RESTORE}`; every append route in `pdfcer-core` (add text, OCR
  layer, add image, paste, flatten fields, flatten annotations, Bates);
  the OCR-layer and Bates removal paths.

## 1. The problem

A page may leave any graphics or text state in effect at the end of its
content: a `cm`, a clip, `gs` (`/CA`, `/BM`), colour, line style, `Tc`/`Tw`/
`Ts`/`TL`/`Tr`/`Tz`. That is conforming. An appended stream inherits all of
it, because the array is one concatenated stream and state is initialised
once per page. Setting every parameter explicitly inside the overlay (the
previous approach for text) cannot undo a CTM or a clip.

## 2. Options

- **A — always wrap:** `[q-stream, …original…, Q-stream, overlay]`.
  Correct, but every append adds two objects and a nesting level.
- **B — rewrite the original** to append `Q`s. Violates round-trip
  (ARCHITECTURE §5): an untouched stream would change.
- **C — emit the inverse CTM** at the overlay's head. Undoes only the CTM;
  the clip cannot be undone, and a singular CTM has no inverse.
- **A+ hybrid (chosen):** wrap only when the content could leak, and
  recognise pdfcer's own wrapper so repeated appends share one.

## 3. The rule

1. **Recognise the wrapper.** `items[0]` decodes to exactly
   `WRAP_SAVE` and some later item (the last such) decodes to exactly
   `WRAP_RESTORE`. The streams carry a `%pdfcer overlay wrap` comment, so a
   foreign stream holding only `q` is never mistaken for one. Recognition
   uses *decoded* bytes (an optimiser may Flate them) and skips any stream
   whose raw length exceeds 256 bytes without decoding it.
2. **Wrapped page:** classify only the tail after the restore. A
   state-neutral tail (every pdfcer overlay is one) → append, no new pair.
   A dirty tail (foreign content added later) → wrap again (a second
   pair, one more nesting level).
3. **Unwrapped page (engineer amendment):** if the whole content is at most
   64 KiB raw and state-neutral, append without a wrap; otherwise wrap.
   Large content is wrapped unread — the cost of a wrap is two tiny
   objects, the cost of a scan is unbounded.
4. **State-neutral** = at `q`-depth 0 only `q`, marked-content operators
   (`BMC`/`BDC`/`EMC`/`MP`/`DP`), `BX`/`EX` and comments; depth never
   negative, ends at 0; the bytes parse. Anything uncertain is not
   neutral: it costs a wrap, never a leak.
5. **One pair per command**, allocated lazily, shared by every page the
   command wraps.
6. **Originals stay byte-identical.** Only the page's `/Contents` element
   list changes.
7. **Removal** (OCR layer strip, Bates removal) drops its streams, then
   peels wrapper pairs left with nothing after them, so stamp-then-remove
   returns the original list.
8. **Bates labels** are self-contained `q … Q` streams; the former shared
   leading `q` stream and `Q q` label head are gone (unreleased format, no
   back-compat — pre-release formats are not supported).
9. There is no public append path that bypasses the plan; the former
   `page_tree::append_content_stream` is removed.

## 4. Known limitation

An original with unbalanced `q`/`Q` (non-conforming) is not repaired: an
extra `Q` in it can pop the wrapper's `q`. The wrap still protects every
conforming page.

## 5. Tests

`crates/pdfcer-core/tests/leaked_page_state.rs`: the issue #1 repro for
OCR and add-text; six leak classes × five routes (add text, OCR, image,
Bates, paste) checked structurally with `is_state_neutral`; one pair across
many appends; clean page and absent `/Contents` unwrapped; clean vs dirty
tail after an existing wrapper; a Flate-compressed wrapper recognised; a
foreign `q` stream not mistaken for one; OCR remove / replace and Bates
remove restore the original list; the wrapper survives a full rewrite.
Flatten-annotations coverage is in `flatten_annotations.rs`. Each test was
sabotage-checked (never wrap, always wrap, no peel, raw-byte recognition,
old Bates head).
