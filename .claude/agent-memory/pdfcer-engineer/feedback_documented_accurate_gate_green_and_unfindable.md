---
name: documented-accurate-gate-green-and-unfindable
description: A verb can be present, correct and gate-green while still being unfindable by the question a reader actually has — pdfceGUI filed a request for a verb that had shipped five days earlier.
metadata:
  type: feedback
---

# A document can be complete, accurate, gate-green — and unfindable

Recorded 2026-08-27.

## What happened

`EditSession::format_text` shipped 2026-08-20. It changes **size, fill colour
and font family on existing page text**, plus five §9.3 text-state
parameters. It is fully and correctly documented in
`docs/core-api/02-editing-and-saving.md`.

On 2026-08-25 `pdfceGUI` filed `request_restyle_an_existing_text_run.md`
whose table read, for existing text, **"none available"** across face, bold,
italic, size and colour. They had looked. They found
`writer::content::set_font`, correctly concluded it was not the answer, and
had **nothing else to find**.

## Why no gate caught it, and none can

`tools/check-core-api-verbs.py` exists precisely to stop this — it was built
after `pdfceGUI` shipped a wrong operator disclosure about `insert_pages`,
whose only description anywhere was a chat reply.

**It fires on a verb being ABSENT.** This verb was present, accurate, and the
gate was green. It was filed under *"text editing mechanics"*, and the
reader's question was *"how do I make this selection bold"*.

> **Findability is not a property any script can evaluate.** A gate can check
> that every verb appears somewhere. It cannot check that a reader with a
> question will arrive at it.

## The reason it happened, which is structural

`format_text` reached the channel **only as prose inside a `Pass 119.2` note
about form XObjects** — framed as "the asymmetry `119.0` shipped, paid off
before the session ended". Nothing anywhere said *there is now a verb that
restyles existing text*. It was documented as a **mechanism**, in the
mechanism document, and never as a **capability**.

## How to apply

1. **A verb ships in two documents, not one.** `02-editing-and-saving.md`
   answers *"what does this call do"*; `03-capabilities.md` answers *"how do
   I do X"*. A verb that only appears in the first is reachable only by
   somebody who already knows its name.
2. **When a Pass pays off an asymmetry as a bonus at the end of a session,
   that bonus gets the weaker write-up** — it is described relative to the
   thing it fixed, not as a thing in itself. Watch for it.
3. **A filed request for something that already exists is a documentation
   defect report**, not a mistake by the requester. Answer it as one, and say
   so — `pdfceGUI` did the right thing twice this week and should keep doing
   it.
4. The measured limit belongs in the capability entry too. `set_font`
   **selects** a font; it does not **create** one, so bold/italic fail on any
   page that does not already carry such a resource. That is worse than a
   run-level limit for a UI: **the predicate is a property of the PAGE, not
   the selection**, so the same button on identical-looking text behaves
   differently in two files.

Related: [[feedback_a_gate_that_underreports_looks_green]] — sibling class,
one level up. [[project_gui_request_channel]].
