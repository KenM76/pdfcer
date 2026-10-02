# Decision 185 — Image stamps and push-button icons

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 443.0`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** pdfcer-gui requests G095 (an image as a stamp comment) and G097
  (a push-button icon).
- **Clauses:** ISO 32000-1 §12.5.6.12 (rubber stamp), Table 189 (`/MK /I`,
  `/TP`, `/IF`), Table 247 (icon fit dictionary), §11.6.5.3 (`/SMask`).

## 1. Image stamp

- One `/Stamp` whose `/AP /N` is a form drawing the image contain-fitted and
  centred in `/Rect`. The image goes through `add_image`'s writer, so alpha
  becomes an `/SMask`; no second image path.
- No `/Name`: Table 181's names describe standard faces, and this face is the
  image. A reader that ignores `/AP` has nothing correct to draw from a name.
- `resize_annotation` re-fits the image rather than stretching the old form.

## 2. Push-button icon

- The icon is a form XObject at **one point per pixel**, so its natural size
  is the image's pixel size; `/IF` decides the scale into the button.
- `/IF << /SW /A /S /P /A [0.5 0.5] >>` is written when absent (scale always,
  proportionally, centred). An existing `/IF` is the producer's choice and is
  kept.
- **Default caption position** when an icon is set with no position: the stored
  `/TP` if it already shows an icon; else `IconOnly` when there is no caption;
  else `CaptionBelow`. A caption-only button that gains an icon must show it,
  and below is the conventional toolbar layout.
- **Clear** removes `/I` and `/TP`; `/IF`, `/RI` and `/IX` stay. They are
  harmless without `/I`, and a later icon set reuses the fit.
- A stored `/TP` that shows an icon with no `/I` draws the caption alone,
  rather than refusing or drawing an empty icon box.
- An edit that sets no icon patches `/MK` in place, so another producer's `/I`
  survives a colour or position edit.
- A foreign push-button `/AP` is replaced only under the existing
  `replace_foreign_appearance` opt-in, and the replacement draws the stored
  `/MK /I`; without the opt-in the edit is recorded, not painted.
- Icon and position edits on any field that is not a push button refuse with
  `EditError::NotAPushButton` before anything is staged.

## 3. RAG gap

Table 247 (icon fit dictionary) is not in the spec RAG; Table 189 only points
to it. The `/IF` keys (`SW` A/B/S/N, `S` A/P, `A`, `FB`) are therefore
unverified against the RAG. `pdfcer-spec-librarian` should file Table 247 and
the reader in `annot_author/button_icon.rs` be checked against it.
