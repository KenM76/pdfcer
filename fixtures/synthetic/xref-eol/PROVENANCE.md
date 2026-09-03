# fixtures/synthetic/xref-eol — provenance

Synthetic, self-authored by `tools/gen-xref-eol-fixtures.py`. No third-party
content (LEGAL §5). CC0. Regenerate with
`python tools/gen-xref-eol-fixtures.py`.

Positive-control fixtures for decision 013 Pass A (classic-`xref` EOL
correctness, ISO 32000-1 §7.5.4 + §7.5.1). Each is a complete, well-formed
single-revision one-page document (catalog → page tree → one page → content
stream) whose only variable is the cross-reference table's end-of-line
handling. All must load: a 5-entry table (objects 0..=4, object 0 the
free-list head) and a resolvable `/Root` catalog with 4 in-use objects.
Exercised by `crates/pdfcer-core/tests/xref_eol.rs`.

- `entry-spcr.pdf` — every 20-byte entry ends `SP CR` (20 0D); structural
  lines end LF.
- `entry-splf.pdf` — every entry ends `SP LF` (20 0A), the common form.
- `entry-crlf.pdf` — every entry ends `CR LF` (0D 0A).
- `struct-cr.pdf` — entries `SP LF`; the `xref`/header/`trailer`/`startxref`
  structural lines end in a bare CR (§7.5.1 fallback).
- `struct-crlf.pdf` — entries `CR LF`; structural lines end CR LF.
- `multi-subsection.pdf` — three subsections `0 1` + `3 2` + `1 2`, emitted
  out of natural order, proving subsection-relative object addressing.
- `trailing-space.pdf` — an incidental trailing SPACE before each structural
  line's EOL (`skip_one_eol` / lexer tolerance).
- `bare-cr-oldmac.pdf` — old-Mac file: every EOL is a bare CR (entries use
  the legal `SP CR` form).
- `mixed-eol.pdf` — entries rotate all three legal EOLs and structural lines
  rotate CR/LF/CRLF, proving the parser makes no uniform-EOL assumption.
