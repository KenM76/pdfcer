# Decision 192: Image optimisation settings model (Pass 616.4)

Consultant: `autonomous-builder` (KenAgent protocol). Builds on decision 191.

## Decision

Images are a `StreamAction::Image(ImagePatch)` variant. Every patch field is
optional. The actions matching a stream fold in precedence order: the
default `streams`, then rules earliest to latest, then the per-object
override. `leave` and `recompress-N` replace the whole state; an
`image(...)` patch merges into it field by field. A patch landing on
`Recompress{level}` has base encode `Flate{level}`; landing on `Leave`, base
encode `Keep`. Acrobat's colour / grey / mono panels (plus Indexed, as
PDF-XChange has) ship as preset rules selecting by `image-class`.

## Reasoning

- Ken asked for control over each object. With a field-wise merge,
  `--object "42 0=image(method=off)"` keeps a preset's JPEG quality and turns
  off only downsampling; a whole-action override would restate every preset
  value and drift when the preset changes.
- One mechanism means class settings compose with every existing selector
  (pages, size, filter, colour space, ppi, ids) — survey gap 12.
- The plan records which source set each resolved field, so every lossy
  change names its cause (rule 4).

## Resolution basis (a correction to the audit)

Effective ppi = pixels x 72 / drawn points. Downsampling scales every
placement by the same factor, so the placement that limits the reduction is
the largest drawn one: the **lowest** effective ppi. Each image axis scales
independently, `s_a = target / min over placements of ppi_a`, and an axis is
resampled only when every use is above the trigger. The report names the
binding placement (page, drawn size, ppi before and after).
`ImageSpace::max_effective_ppi` measured the wrong end for this and is
replaced by a minimum-based `binding_ppi`; `Selector` min/max-ppi test that.

## Defaults

| Setting | Default |
|---|---|
| Method when enabled | bicubic (also average, subsample, lanczos) |
| Trigger (`above`) | 1.5 x target |
| Unplaced image | not resampled, reason disclosed (option: `max-pixels=N`, long side) |
| JPEG named levels | minimum 20, low 40, medium 60, high 80, maximum 95 (IJG scale) |
| Chroma | auto: 4:4:4 at q >= 90, else 4:2:0 |
| Existing JPEG, pixels unchanged | untouched |
| Re-encoding a JPEG | quality capped at the source's estimated quality |
| Bilevel | Otsu, threshold disclosed (options: fixed N, dither) |
| Indexed images | resampled by nearest only (palette indices never blend), disclosed |
| Mono downsampling in presets | off except the screen tier |

## Pipeline and validation

decode -> apply /Decode -> convert to grey -> resample -> threshold -> encode
-> only-if-smaller / `min-saving` guard. Refused by name: ppi above the
trigger, ppi 0, JPEG with bilevel or a 1-bit source, quality outside 1..=100,
an unknown key inside `image(...)`. Display prints no whitespace, because
`--rule` splits on whitespace. `PlannedChange::lossy` comes from what the
pipeline actually did, not from the action.

## Colour conversion boundary (engineer's note, decision 064)

Grey conversion of `DeviceRGB` / `DeviceCMYK` uses ISO 32000-2 §10.4.2.2 /
§10.4.2.3's device formulas, which are PDF semantics and pdfcer's. Anything
ICC-managed belongs to `iccce`: an `ICCBased` image converted by formula is
disclosed as "device formula, not colour-managed"; `Lab`, `Separation` and
`DeviceN` are skipped with a reason in 616.4.

## Alternatives rejected

- **Per-class struct plus separate override map:** a per-image choice in two
  places; class settings cannot be scoped by page or size.
- **Whole-action replacement:** overrides restate the preset.
- **Uniform scaling on the minimum axis:** per-axis meets the same guarantee
  and saves more on stretched placements.

## Risks

SMask with /Matte must keep its parent's dimensions (ISO 32000-2 §11.6.5.3):
resample in lockstep or skip both. SMask and /Mask images take placements from
the parent. A colour-key /Mask with a blending resampler makes fringe colours:
subsample or skip, disclosed. Inverted Adobe CMYK JPEGs need the matching
/Decode. 16 bpc to JPEG is lossy. Dedupe runs before resampling (or placements
are the union over duplicates). Mono results are mostly not-smaller until 616.5
adds CCITT / JBIG2 encoders — disclosed, not hidden.

## References

Decision 191; rules 1, 4, 13; `docs/research/pdf-size-reduction-survey.md`
gaps 6, 10, 12.
