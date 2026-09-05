# pdfcer

# pdfcer

## NOTE THAT THE GUI HAS BEEN MOVED INTO IT'S OWN PDFGUI PROJECT.
I have split the GUI off into its own project with a proper plan.
Do not use the one included in this project as it is obselete.
New GUI is here:
https://github.com/KenM76/pdfcer-gui

If you need a fast RUST native pdf GUI editor and are planning it as an
LLM project the core from this should save you weeks of time.
If you need a CLI tool to use with your LLM to interact with pdf
documents, this might be your tool of choice to do that too.

An open-source, non-monetized PDF editor for Windows, written in Rust.
The long-term goal is feature parity with Adobe Acrobat Pro; **it is not
there yet**, and the sections below say plainly what does and does not
work today.

It is a native desktop application — no web server, no browser runtime,
no local network listener. It runs from a single folder, dependencies
included, no installer. Alongside the GUI it ships **`pdfcer`**, a
first-class scriptable command line with 108 subcommands, which is
deliberately not a debug tool: Acrobat Pro has no real equivalent.

## Status: pre-1.0, under active development

**Working today**, among other things: opening and rendering PDFs;
page operations (merge, split, extract, insert, delete, reorder,
rotate); text extraction and text editing with reflow; AcroForm field
creation, editing, filling, flattening and FDF/XFDF import/export;
markup annotations; redaction (mark, review and apply — either
finalizing or undo-preserving); Bates numbering; PDF/A validation and
conversion; **digital-signature verification** (integrity and
byte-range coverage) and, opt-in, **trust evaluation** against an
imported Acrobat/Reader trust store (certificate-chain linkage,
validity dates at signing time, and RFC 5280 CA/key-usage constraints);
vector object and node editing; measurement and dimension authoring;
image placement from PNG, JPEG, BMP and TIFF; printing, with page
placement, orientation, duplex, copies and n-up/booklet/poster
imposition; **opening password-protected documents** — RC4 40–128 bit,
AES-128 and AES-256 (both `/R 5` and `/R 6`), including the
empty-user-password case that opens with no prompt at all; and
**authoring encryption** (AES-256, `/R 6`), setting the eight permission
bits, and removing encryption from an owner-authenticated document.

**Not built yet**, among other things: OCR, JavaScript, XFA, *creating*
digital signatures (pdfcer verifies them but does not yet sign),
signature revocation checking (CRL/OCSP — deliberately excluded from
the engine by its no-network rule, so it belongs to a shell or to
embedded revocation data), and a long tail of Acrobat Pro's surface.
Some capabilities exist in `pdfcer-core` and `pdfcer` but have no GUI
yet.

**`docs/FEATURES.md` is the honest, current answer** — a capability
list with per-surface (core / CLI / GUI) checkboxes, updated whenever a
feature lands rather than at release time. Read that before assuming
anything here is complete.

The **[latest release](https://github.com/KenM76/pdfcer/releases/latest)**
is a single-folder portable build for Windows x64. No installer, no
registry writes: unzip it and run `pdfcer.exe`. The desktop GUI is the
separate [pdfcer-gui](https://github.com/KenM76/pdfcer-gui) project, which
builds on this engine and ships its own releases.

<!--
  ★ THE VERSION NUMBER IS DELIBERATELY NOT WRITTEN HERE.

  It used to be, and it said "v0.3.0" through v0.4.0, v0.5.x, v0.6.0 and
  v0.7.0 — FOUR releases, on a public repository, as the first concrete
  claim a stranger reads.

  It survived every review for one reason worth keeping: the LINK was
  always `/releases/latest`, so clicking it went somewhere correct and
  only the LABEL lied. A wrong claim next to a right link reads as a
  right claim.

  So the label is gone rather than corrected. A number that must be
  updated by hand at a moment nobody is editing this file is a number
  that will be stale again, and "remember to update the README" is a
  reminder rather than a remedy. The link already carries the answer and
  cannot go stale by construction.
-->

## Privacy, platform and signing

> **pdfcer does not use the network.** It contains no HTTP client and no
> TLS stack — you can confirm this yourself in
> `THIRD_PARTY_LICENSES.md`, which lists every library linked into the
> binary. There is no telemetry, no analytics, no crash reporting, no
> licence check, and no update check. Every document you open is
> processed entirely on your machine.
>
> If you click a link inside pdfcer, pdfcer hands the address to your
> operating system's default browser. The request is made by your
> browser, not by pdfcer.
>
> **Updates** are manual: download the new zip and replace the program
> files (keep your `userdata` folder). pdfcer will never update itself.
>
> **Supported platform:** Windows 10/11, 64-bit. pdfcer's code is kept
> portable and is compiled for Linux, macOS, and WebAssembly on every
> change, but those builds are not tested or supported, and no artifact
> is published for them.
>
> **The download is not code-signed.** Windows will show a SmartScreen
> warning ("Windows protected your PC") the first time you run it;
> choose *More info* → *Run anyway*. This warning will appear again for
> each new version, because an unsigned program's reputation does not
> carry across releases. Verify your download against the published
> SHA-256 checksum if you want certainty about what you received.

## Building

```sh
cargo build --release
cargo run --release -p pdfcer-cli -- --help
```

Rust toolchain version is pinned in `rust-toolchain.toml`. There are no
system dependencies to install.

## Design

The engine crates, and the split is load-bearing rather than cosmetic:

| Crate | Role |
|---|---|
| `pdfcer-core` | The PDF engine — parsing, the object model, editing, writing. **Zero GUI or windowing dependencies.** |
| `pdfcer-render` | Headless rasterizer. Also zero GUI dependencies. |
| `pdfcer-cli` | The scriptable shell; its binary is `pdfcer`. |

The desktop application is **not** in this repository. The original in-repo
GUI crate was removed in Pass 247.0 (2026-09-03); the shipping GUI is the
separate `pdfcer-gui` project, which depends on `pdfcer-core` and
`pdfcer-render` exactly as `pdfcer` does — two independent front ends
over one engine.

Two invariants shape most decisions:

- **GUI–core separation.** `pdfcer-core` and `pdfcer-render` never gain a
  GUI dependency, which is what keeps an eventual WebAssembly build a
  shell-crate swap instead of a rewrite — and what makes the GUI and
  CLI two independent front ends over one engine.
- **Round-trip / minimal-diff editing.** Objects pdfcer did not
  logically touch are re-emitted byte-identically, or simply omitted
  under the default incremental save. Redaction is the one deliberate
  exception: it must genuinely remove covered content, not mask it.

Where the PDF standard is genuinely ambiguous — and it often is —
pdfcer does not pick silently. The choice becomes an operator setting
with a documented default, reachable from *File → Settings* or from a
plain-text file beside the program. Each one states what the standard
leaves open, how well-founded the default is, and whether changing it
affects the file or only the view.

## Documentation

| Doc | What's in it |
|---|---|
| `docs/FEATURES.md` | **What pdfcer can do today**, per surface. Start here. |
| `docs/ARCHITECTURE.md` | Crate layout, data model, invariants, packaging, dated decision log. |
| `docs/ROADMAP.md` | Pass-by-pass plan and history, with the full reasoning. Large. |
| `docs/decisions/` | Numbered decision records for the choices that needed one. |
| `docs/DEPENDENCIES.md` | What each third-party package is *for*, by crate, and what pdfcer implements itself instead. The purpose-shaped view `THIRD_PARTY_LICENSES.md` (generated, licence-shaped) can't give you. |
| `docs/LEGAL.md` | Licensing posture, PDF-spec sourcing rules, test-corpus rules, dependency attribution. |
| `docs/PRIOR_ART.md` | What existing crates and tools were adopted, what was reference-only, and why. |
| `docs/SESSION_LOG.md` | Append-only development record. |

The documentation is unusually detailed on purpose: the project's
standing rule is that the docs are the logic and the code is the syntax
that enacts it. `ROADMAP.md` in particular is a working engineering
record, not a brochure.

## Testing

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Test fixtures are synthetic and generated by committed scripts under
`tools/`. A larger corpus of third-party PDFs is used for differential
testing against pdfium, but it is **not** redistributed in this
repository.

## Contributing

This is a personal project developed in the open rather than a project
seeking contributors, and it moves fast. Issues and observations are
welcome; please do not be surprised if a large unsolicited pull request
does not fit the direction. If you are considering one, open an issue
first.

## License

**MIT** — see `LICENSE`.

Third-party dependency licences are generated into
`THIRD_PARTY_LICENSES.md` by `cargo-about` and are all permissive; no
copyleft code is linked. pdfcer deliberately does not use GPL/AGPL PDF
engines (MuPDF, Poppler, Ghostscript), which is why several things were
implemented from the specification rather than adapted from existing
code.
