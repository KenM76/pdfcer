# Decision 175 — A refused text edit may be applied by an opt-in, disclosed workaround

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 436.0` (core + CLI).
- **Authored by:** `pdfcer-engineer`, under the operator's direction that
  text be editable in every case through workarounds the operator opts into.
- **Builds on:** `Pass 427.0` (`UnsupportedCause`, `edit_capability`),
  `Pass 428.0` (cross-text-object span), `Pass 431.0` (fallback face,
  decision 174's sibling route).
- **Clauses:** ISO 32000-2 §9.3.1 (`Tc`, `Tw`, `Tf`), §9.4.2 (`T*`, line
  matrix), §9.4.3 (`Tj`, `TJ`, `'`, `"`; a `TJ` number moves the text
  position by `-n/1000 * Tfs * Th`), §9.4.4 (advance), §9.6.2.2
  (standard 14 fonts), §9.7.4.3 (`/WMode`).

## 1. Decision

`EditOptions::workarounds: WorkaroundPolicy`, default `Refuse`.

- **`Refuse`** changes no behaviour. Every refusal that has a workaround
  says so: `Display` gains `-- a workaround is on offer when workarounds are
  enabled: <what it does>` and `EditError::workaround()` returns it. The
  CLI adds `re-run with --workaround to apply it`.
- **`Apply`** runs the exact edit first. Only when it refuses with a cause
  that offers a workaround is the workaround planned. The result is one
  plan, so one undo entry, and preview and commit share it.

Opt-in because a retype changes text the operator did not ask to change
(the operators around the match are reset) and may substitute a face; a
silent default would break rule 4's spirit even with a disclosure.

## 2. The offer table

| Refusal | Workaround | Kind |
|---|---|---|
| `NoMatch { SpansTextObjects }` | `JoinTextObjects` | exact |
| `Unsupported(QuoteOperator)` | `RewriteQuoteOperator` | exact |
| `NoMatch { SplitRun }` (new: one text object, one line, the match crosses operators the matcher does not join) | `Retype` | approximate |
| `Unsupported(CrossElementTj / FontUnresolvable / EncodingNotInvertible / CompositeWithoutToUnicode / FontMapNotInvertible / VerticalWriting)` | `Retype` | approximate |
| `Refused` with trigger `Composite`, `SymbolicNoEncoding` or `ToUnicodeOnly` and no `character` (the whole run's font is unusable) | `Retype` | approximate |
| Everything else, explicitly `ReferenceXObject`, `OpiProxy`, `ObjectNumbersExhausted`, `CommitFailed`, `StateNotRestorable`, `InsideFormXObject`, a one-character coverage refusal | none | — |

A one-character coverage refusal is left to `with_fallback`, which sets that
character alone and keeps the rest of the run intact; a retype would throw
that precision away. A proxy form's printed content is not what the page
shows, so no rewrite reaches it.

## 3. Routes

- **Join:** the request is re-planned as a pinned span from the first
  object to the last (the `Pass 428.0` machinery). If that refuses (a font
  or size seam between the objects), the route falls back to **retype**.
- **Quote rewrite:** the stream is re-parsed with the named `'` replaced by
  `T* … Tj` and `"` by `aw Tw ac Tc T* … Tj` (§9.4.3 defines them so), and the
  exact edit is planned on that. If the inner edit refuses with a cause
  offering `Retype`, the route retypes.
- **Retype:** every show operator the match touches is removed — its string
  bytes deleted, as redaction deletes them — and one `Tj` is set where the
  first stood, with the text state, colour, CTM and text matrix in force
  there. The new text is the touched operators' unmatched text around the
  replacement, so nothing outside the match changes character. The face is
  the run's own font when it can encode all of it (subset limits respected),
  else the `with_fallback` face, else non-embedded `Helvetica` (§9.6.2.2). A
  vertical run is set horizontally from its origin and says so. A following
  `Tj`/`TJ` that continued from the run is held in place by a compensating
  `TJ` number (the advance difference, §9.4.3), so " world" in
  `(Hel) Tj /F2 12 Tf (lo) Tj /F1 12 Tf ( world) Tj` stays put.

A route that cannot apply returns `EditError::WorkaroundRefused { refused,
workaround, why }`, naming the original refusal and the route's reason.
`RefusalFormat` and `refusal_kind` classify it as its `refused`.

## 4. Disclosure (rule 4)

Off-canvas only. `EditReport.workaround: Option<WorkaroundUse>` and
`disclosures[0]`: `workaround (<label>, exact|approximate): the exact edit
was refused (<reason>), so pdfcer applied: <what>`. A retype adds what it
removed and set, the compensating `TJ` when used, the fallback face (and
`EditReport.fallback`), the vertical note, and a save note: an incremental
save keeps the prior revision, which still holds the removed bytes; redact
or full-rewrite to remove them from the file. Nothing is drawn on the page.

## 5. A defect fixed on the way

`narrow_span` narrowed a cross-object span to its matched operators and
re-spaced the line, but the next text object's `Td` is relative to its own
`BT` and was not moved, so "Right" overlapped "-Hand Door". It now declines
to narrow when an `ET` lies between the narrowed part's last operator and
the match's end; the whole-span plan is used. A narrowed part that ends in
the match's last text object (G082's appended character) still narrows. The
defect was reachable without a workaround, by any `EditRequest::spanning_from`
across text objects.

## 6. Rejected

- **Workarounds on by default.** See §1.
- **A typed `workaround` field on `TextEditPreview`.** The preview's
  disclosures already carry it and equal the commit's; a field can follow
  when a shell asks.
- **Retyping one-character coverage refusals.** See §2.
- **Keeping kerning on retype.** The removed operators' `TJ` numbers are not
  carried; the new run is set at plain advances, and the disclosure says the
  text was retyped.
