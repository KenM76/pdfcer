# gif — provenance and attribution

GIF fixtures for `pdfcer-core`'s `image_import::gif` and
`EditSession::add_image`. Used by `crates/pdfcer-core/tests/image_gif.rs` and
`crates/pdfcer-cli/tests/add_image_gif.rs`.

## Source material and license (LEGAL.md §5, project rule 7)

`LEGAL.md` §5 category (a): **wholly synthetic**. Every pixel comes from a
formula in `gen-gif-fixtures.py`, and the tests re-derive those formulas as
their oracle. Nothing is derived from a third-party file; no attribution is
owed or claimed.

The files are written by **Pillow** (HPND licence), a development tool only,
never a pdfcer dependency. Pillow's GIF encoder is pure Python and independent
of `weezl`, the crate pdfcer decodes with; the unit tests in `gif.rs` encode
with weezl, so a bit-order or sub-block mistake shared by both halves of one
library is caught here.

## Files

| File | What it pins |
|---|---|
| `two-colour-transparent.gif` | GIF89a, 8x6 checkerboard, index 1 transparent via a Graphic Control Extension → `/SMask` clear on `(x+y)%2==1`. |
| `interlaced.gif` | GIF87a, 32x20, interlaced; pixel `(y + x/8) % 4` over a four-entry table, so a wrong pass order moves a row. |
| `animated-3-frames.gif` | GIF89a, 16x16, three solid frames (indices 0, 1, 2), the later two with local tables → frame one placed, 2 frames reported. |
| `truncated.gif` | The first half of `interlaced.gif` → refused as corrupt. |

## Regenerate

```
python fixtures/synthetic/gif/gen-gif-fixtures.py
```
