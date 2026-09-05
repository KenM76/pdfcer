# What pdfcer is built from

Every third-party package pdfcer depends on, what each one is *for*, and
which parts pdfcer implements itself instead.

This answers a different question from `THIRD_PARTY_LICENSES.md`. That file
is **generated** by `cargo-about` and is licence-shaped — it lists every
crate in the graph with its licence text, because that is a legal
obligation. It cannot tell you why anything is there. This file is the
purpose-shaped view, written by hand.

Measured at `644b564`, 2026-08-11. Figures come from `cargo metadata` and
`cargo tree`, not from memory — if you change the dependency set, re-run
those and update this file, and regenerate `THIRD_PARTY_LICENSES.md`.

---

## 1. The crates pdfcer is

Five crates, ~281,000 lines. The split is not organisational tidiness — the
boundary between the first three and the last two is the project's
load-bearing invariant.

| Crate | Lines | What it is |
|---|---|---|
| **`pdfcer-core`** | ~143,000 | The PDF engine. Object model, tokenizer, cross-reference parsing, incremental writer, filters, fonts, colour, encryption, forms, redaction, editing. Knows nothing about screens. |
| **`pdfcer-render`** | ~20,000 | The headless rasterizer. Turns a page into pixels. Runs with no window and no GPU. |
| **`pdfcer-print`** | ~5,100 | Printer discovery, device capabilities, page placement on a sheet, imposition (n-up, booklet, poster), and spooling. The only crate with platform-specific code. |
| **`pdfcer-cli`** | ~18,000 | The command-line tool (binary `pdfcer`). Every batch operation, scriptable. |

### Why the split matters

**`pdfcer-core`, `pdfcer-render` and `pdfcer-print` must never depend on a
GUI or windowing crate.** Not "should not" — CI greps `cargo tree` on every
push and fails the build if `egui`, `eframe`, `winit` or `wgpu` appears in
their dependency trees.

Two things fall out of that. The engine is testable without a screen, which
is why the whole suite — ~3,350 tests — runs in about a minute on a
desktop, with no display and no printer needed for the vast majority of
it. And a future web version is a
*shell swap* — a WASM front end over the same crates — rather than a
rewrite. (The desktop GUI is exactly such a shell: the separate
`pdfcer-gui` project, which depends on this workspace and is not part of
it. The original in-repo `pdfce-gui` crate was removed in Pass 247.0.)

`pdfcer` is held to the same rule, which is why the CLI is a real
first-class tool rather than a debug harness bolted onto the GUI.

---

## 2. Third-party packages, by crate

~~**10 direct third-party dependencies in the engine, 3 in the GUI.**~~
★ **Corrected 2026-09-05 (437th librarian filing, decision 137): `pdfcer-core`
declares 25 direct third-party dependencies — 24 in `[dependencies]` plus
`getrandom` under a `cfg(not(target_arch = "wasm32"))` target table —
counted from `crates/pdfcer-core/Cargo.toml` in the working tree that adopts
the signing stack (15 before it; the table below had been short five rows —
`brotli`, `ocrs`, `rten`, `sha2`, `getrandom` — since well before signing,
and gains them now with the ten signing crates).** The "3 in the GUI" half
of the struck sentence is not re-measured here — the GUI is a separate
project since `Pass 247.0`. The
GUI's manifest lists six, but three of those are pdfcer's own crates. The
full resolved
graph is 435 packages, but most of that is the GUI stack's own
transitive tree — `pdfcer-core` pulls a deliberately small set.

### `pdfcer-core` — the engine

| Package | Licence | What it does for pdfcer |
|---|---|---|
| `thiserror` | MIT OR Apache-2.0 | Derives the error types. Every failure pdfcer reports is a typed error rather than a string, and this removes the boilerplate for that. |
| `flate2` | MIT OR Apache-2.0 | **Deflate/zlib** — the `/FlateDecode` filter. The single most common compression in any PDF; nearly every modern file's page content and fonts are Flate-compressed. Built with `rust_backend`, so it is pure Rust with no C zlib linked in. |
| `zune-jpeg` | MIT OR Apache-2.0 OR Zlib | **Decodes JPEG** images (`/DCTDecode`) — every photo and most scans. |
| `jpeg-encoder` | (MIT OR Apache-2.0) AND IJG | **Encodes JPEG**, for importing an image into a PDF. Note the `AND IJG` — unlike every other dependency here, that is a *conjunctive* licence with its own attribution requirement. |
| `weezl` | MIT OR Apache-2.0 | **LZW** — the `/LZWDecode` filter (older PDFs and TIFF images). |
| `hayro-ccitt` | Apache-2.0 OR MIT | **CCITT Group 3/4 fax decoding** — the compression almost every black-and-white scanned document uses. |
| `hayro-jbig2` | Apache-2.0 OR MIT | **JBIG2 decoding** — a later bilevel scan format, common in aggressively compressed scans. |
| `hayro-jpeg2000` | Apache-2.0 OR MIT | **JPEG 2000 decoding** (`/JPXDecode`). |
| `aes` | MIT OR Apache-2.0 | **AES block cipher**, for opening AES-128 encrypted PDFs. |
| `cbc` | MIT OR Apache-2.0 | **CBC block-cipher mode**, which is how PDF applies AES. *(2026-09-05: gains `alloc` + `block-padding` for PKCS#7 unpadding of PKCS#12 bags.)* |
| `sha2` | MIT OR Apache-2.0 | **SHA-256**, the key-derivation primitive for AES-256 (`/R` 5) encrypted PDFs, and the digest every signature pdfcer authors or verifies uses. *(Row added 2026-09-05; present since 2026-08-11.)* |
| `getrandom` | MIT OR Apache-2.0 | **OS randomness** for encryption authoring and RSA blinding. Target-gated OFF on wasm32, where those operations refuse by name instead. *(Row added 2026-09-05.)* |
| `brotli` | BSD-3-Clause AND MIT | **Brotli decoding** — the `/BrotliDecode` filter (PDF 2.0). Decode only. *(Row added 2026-09-05; present since `Pass 123.0`.)* |
| `ocrs` + `rten` | MIT OR Apache-2.0 | **Pure-Rust OCR** (`ocrs` feature, default ON): text recognition over page images, written as an invisible text layer. `rten` is its inference runtime. *(Rows added 2026-09-05; present since `Pass 71.0`.)* |
| `rsa` | MIT OR Apache-2.0 | **★ SIGNING (feature `signing`, default ON; decision 137, 2026-09-05).** RSA private-key operation — PKCS#1 v1.5 and RSASSA-PSS — on a constant-time big-number backend. Used ONLY through its blinded (`Randomized*`) paths. `0.10.0-rc.18`, a release candidate; carries the open RUSTSEC-2023-0071 advisory, accepted for signing because the advisory's channel is a decryption oracle signing never runs — reasoning in `ARCHITECTURE.md` §12 decision 137, re-checked at every bump. Verification of RSA signatures does NOT use it (in-crate, decision 129). |
| `p256`, `p384` | Apache-2.0 OR MIT | **SIGNING.** ECDSA over NIST P-256 / P-384 with a deterministic nonce (RFC 6979), so it needs no randomness and works on wasm32. Also reads the EC private key out of a `.pfx`. Verification of ECDSA signatures does NOT use them (in-crate). |
| `signature`, `rand_core` | Apache-2.0 OR MIT / MIT OR Apache-2.0 | **SIGNING.** Trait crates only — the signer interface `rsa` and the ECDSA crates implement, and the RNG interface pdfcer's own randomness adapter implements so `rsa` never pulls its own. |
| `sha1`, `hmac`, `pbkdf2` | MIT OR Apache-2.0 | **SIGNING — PKCS#12 import only.** Opening a `.pfx`/`.p12` digital ID: `hmac` checks the file's integrity MAC (the password check), `pbkdf2` derives keys for modern (OpenSSL 3) containers, `sha1` is the digest every LEGACY container's MAC and key derivation are built on. pdfcer never writes a `.pfx` and never authors a SHA-1 signature. |
| `des`, `rc2` | MIT OR Apache-2.0 | **SIGNING — PKCS#12 import only, legacy READ.** Triple-DES and 40-bit RC2 are the ciphers inside every Windows-exported and OpenSSL ≤ 1.1 `.pfx`. Same posture as `rc4`: obsolete ciphers, read because operators have the files, never written. |

### `pdfcer-render` — the rasterizer

| Package | Licence | What it does for pdfcer |
|---|---|---|
| `tiny-skia` | BSD-3-Clause | The **CPU rasterizer**. Fills and strokes paths, blends, clips — a software port of Skia (the engine behind Chrome's graphics). CPU-only is what lets rendering run headless, in tests and in the CLI. |
| `skrifa` | MIT OR Apache-2.0 | **Reads font files** and extracts glyph outlines from TrueType/OpenType/CFF, so text can be drawn as shapes. |
| `subsetter` | MIT OR Apache-2.0 | **Font subsetting** — cuts an embedded font down to only the glyphs a document actually uses, so saved files do not carry a whole typeface for six characters. |
| `iccce-profile` | MIT | **Parses ICC colour profiles.** Reads the profile embedded in a PDF's `/ICCBased` colour space or its `/OutputIntent`, so pdfcer can know what a document's colours actually mean rather than guessing from the `/Alternate` space. |
| `iccce-cmm` | MIT | **The colour-management engine** — converts between profiles at a chosen rendering intent. This is what makes a CMYK image in a document with a press profile come out the colour the press would print, rather than the colour a naive formula produces. |
| `thiserror` | MIT OR Apache-2.0 | As above. |

#### ★ `iccce` is the one dependency whose "why" is an architectural decision

Every other package in this file was chosen because writing it would be a
waste of time. `iccce` is different, and this file is the place that has to
say so, because the generated `THIRD_PARTY_LICENSES.md` can only answer
*what licence* and never *why is this here*.

`iccce` (`github.com/KenM76/iccce`, MIT) is a **sibling project of pdfcer's,
written for pdfcer** — its README names pdfcer as its first consumer. By
**decision 064** it owns *all* colour conversion: pdfcer never implements a
colour-management module, never picks up a competing CMM crate, and hands
colour questions across that boundary instead. Decision 115 records the
consumption terms.

Two consequences worth stating plainly:

- **It is pinned to a git tag, not a crates.io version** —
  `tag = "v0.3.0"` in `crates/pdfcer-render/Cargo.toml`. `iccce` is not
  published to crates.io, and its author has declined to promise a release
  cadence, so a tag is the strongest pin available. A tag is immutable in
  practice and `Cargo.lock` records the resolved revision regardless, which
  is what `pdfcer --version` reports.
- **It clears the wasm32 gate**, which is why it is admissible at all.
  Anything that could not cross into the future web fork would make colour
  management the first capability pdfcer could not carry there — see
  `ARCHITECTURE.md` §3's GUI-core separation invariant, of which the wasm32
  CI job is the enforcement.

Note where it sits: `pdfcer-render`, **not** `pdfcer-core`. Colour management
is a *rendering* concern; the object model has no opinion about what a
colour looks like. That placement is also why `pdfcer-core`'s build script
could not see the dependency for six days after it landed — see
`crates/pdfcer-core/build.rs`.

### `pdfcer-print` — printing

| Package | Licence | What it does for pdfcer |
|---|---|---|
| `windows` | MIT OR Apache-2.0 | Microsoft's official Win32 bindings. Used for printer enumeration, device capabilities (resolution, printable area, duplex), the `DEVMODE` job settings, and spooling. **Windows-only** — gated behind `cfg(windows)`, so the crate still builds and its geometry logic still tests on Linux and macOS. |
| `thiserror` | MIT OR Apache-2.0 | As above. |

### `pdfcer-cli` — the command line (binary `pdfcer`)

| Package | Licence | What it does for pdfcer |
|---|---|---|
| `clap` | MIT OR Apache-2.0 | Argument parsing, subcommands, `--help` text, shell completions. |

### The desktop app — not here

The GUI is the separate `pdfcer-gui` project and carries its own dependency
record. Nothing in this workspace depends on `egui`, `eframe`, `winit` or
`wgpu` — CI asserts it on every push, and `THIRD_PARTY_LICENSES.md` no
longer lists them. (Before Pass 247.0 an in-repo `pdfce-gui` crate pulled
`eframe`, `egui_tiles` and `rfd`; that crate is gone.)

---

## 3. Licensing posture

pdfcer is **MIT**. Every direct dependency above is permissive —
MIT, Apache-2.0, BSD-3-Clause, Zlib, or a choice among them.

- **No copyleft obligation anywhere.** Stated carefully, because the loose
  version of this sentence is false and it is worth knowing why. Two crates
  in the resolved graph *mention* a copyleft licence — `self_cell`
  (`Apache-2.0 OR GPL-2.0-only`) and `r-efi`
  (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`). Both are **disjunctive**: the
  `OR` is a choice the user makes, and pdfcer takes the permissive branch.
  `cargo-about` resolves `self_cell` to Apache-2.0, and
  `THIRD_PARTY_LICENSES.md` accordingly contains no GPL section at all.
  `r-efi` is a UEFI-target crate and is not in the Windows build in the
  first place — it appears only under `cargo tree --target all`.

  So: **nothing pdfcer ships is under a copyleft licence, and nothing
  imposes a copyleft obligation.** What is *not* true is the tempting
  shorter claim that the strings "GPL" or "LGPL" appear nowhere in the
  graph — they do, twice, harmlessly, and a future audit that greps for
  them will find them. Better to know that now than to rediscover it as a
  scare.
- A **conjunctive** licence is the one that would actually bind, and there
  is exactly one: `jpeg-encoder`'s `AND IJG` (below). `AND` means both sets
  of terms apply; `OR` means you pick. That distinction is the whole of
  dependency-licence review.
- Linking GPL or AGPL code is categorically impossible for an MIT project,
  so it is not a judgement call.
- **`iccce-profile` and `iccce-cmm` are plain MIT**, verified against
  `iccce`'s own `Cargo.toml` on 2026-09-01. They are a *git* dependency
  rather than a registry one, so `cargo-about` resolves them from the
  fetched source; they appear in `THIRD_PARTY_LICENSES.md` like any other
  package. Being a sibling project of the operator's own does not exempt
  them from classification — rule 13 applies to every dependency
  regardless of who wrote it.
- `jpeg-encoder` is the one **conjunctive** licence — `(MIT OR Apache-2.0)
  AND IJG` — so its IJG terms apply on top of the choice, not instead of it.
- MuPDF, Poppler, Ghostscript and Inkscape are **behavioural references
  only**, never dependencies and never a source of code. They are all
  GPL/AGPL.
- Adding any dependency means classifying its licence first, and a copyleft
  one is escalated to the operator rather than decided by an engineer.
- `THIRD_PARTY_LICENSES.md` is regenerated by `cargo-about` whenever the
  set changes, and ships with every release. It is never hand-edited.

---

## 4. What pdfcer implements itself, and why

Some of this is unavoidable — nobody publishes a crate for "PDF
cross-reference stream parsing". The interesting entries are the ones where
a dependency *did* exist and was declined.

### The engine, essentially all of it

The COS object model, the tokenizer, cross-reference tables **and** streams,
object streams, the incremental-update writer, page-tree walking, content
stream interpretation, form fields, annotations, redaction, text extraction
and editing, colour spaces and functions, font embedding, digital-signature
inspection — all pdfcer's own code. This is the ~143,000 lines of
`pdfcer-core`, and it is the project.

### Filters written in-crate

| Filter | Why not a dependency |
|---|---|
| `ASCIIHexDecode`, `ASCII85Decode` | Tens of lines each. A dependency would be larger than the code. |
| `RunLengthDecode` | Same. |
| **PNG/TIFF predictors** | The de-filtering step applied *after* Flate or LZW. Specified inside the PDF standard rather than by the compression formats, so no compression crate implements it. |

`FlateDecode` and `LZWDecode` use `flate2` and `weezl` — the compression
itself is standard and well-served; only the PDF-specific parts around it
are ours.

### Signature verification — read-only arithmetic, in-crate

`Pass 10.1` (decision 129) added six modules with no new dependency:
`asn1` (DER), `cms` (RFC 5652 `SignedData` + RFC 5280 X.509 subset),
`crypto::bignum`, `crypto::rsa` (PKCS#1 v1.5 and PSS verification),
`crypto::ecdsa` (P-256/P-384) and `crypto::sha1`, consumed by
`signature_verify`. The argument is the MD5 one, not the AES one: a
*verifier* holds no secret, so constant-time discipline and the audit
surface that justify a dependency for a cipher or a signer do not apply.
A **signing** implementation would hold a private key and takes the
audited dependency — ~~that decision is deliberately not made here.~~
★ **made 2026-09-05, decision 137:** signing DOES take the audited,
constant-time dependencies (`rsa`, `p256`/`p384` — rows in §2), behind a
default-ON `signing` feature, while the verify-only modules above stay
exactly as they are. What stayed in-crate on the signing side is, again,
the part that is not a cryptographic hazard: the DER encoder
(`sign/der_out.rs`), the CMS `SignedData` assembly (`sign/cms_build.rs`)
and the PKCS#12 container walk (`sign/pkcs12.rs`) — parsing and byte
layout pdfcer already owns the reader for, with no secret arithmetic in
them. The RustCrypto `cms` crate was not taken because its builder does
not compile against current dependencies; `pkcs12` because it does not
decrypt. Both facts are dated in `docs/PRIOR_ART.md`.

### Cryptography — the deliberate split

This is where the reasoning is most explicit, and it goes both ways.

**Written in-crate: MD5 and RC4.** Both are needed only to *read* documents
other tools already produced. Both are frozen — RFC 1321 has not changed
since 1992 — so there is no upstream to track and no CVE stream to follow.
Both are under 200 lines. Taking a dependency for them would have added
supply-chain surface for no benefit, and `pdfcer-core` had no cryptographic
dependency at all at that point.

**Taken as a dependency: AES.** The module that hand-rolled MD5 recorded, in
the same breath and *before there was a case to argue*, that the reasoning
"does not extend to AES" — AES has real implementation hazards (timing, key
schedules, mode handling), a live ecosystem, and well-audited permissive
crates. When AES-128 arrived, that limit was honoured rather than
relitigated.

What stayed in-crate for AES is the part that is *not* a cryptographic
hazard: which bytes are the initialisation vector, and what to do about
padding that does not verify. That last one is a product decision — a
damaged file is more recoverable if the bytes are kept — and it belongs to
pdfcer, not to a cipher library.

**Neither is a security recommendation.** RC4 is broken, MD5 is broken, and
PDF encryption below AES-256 has no integrity protection whatsoever.
pdfcer reads these files because operators have them.

### Small things, deliberately not dependencies

- **The temp-directory helper in the test suite** — about 30 lines, versus
  adding a package to the graph that every release then has to attribute.
- **Colour conversion tables** for CMYK, generated and checked in rather
  than computed at runtime.

---

## 5. Checking this yourself

```
cargo tree -p pdfcer-core            # the engine's real dependency graph
cargo tree -p pdfcer-core -e features   # ...including which features are on
cargo tree --duplicates             # two versions of one crate = a problem
cargo about generate about.hbs      # regenerate THIRD_PARTY_LICENSES.md
```

The GUI-separation invariant is checkable in one command, and it is worth
knowing how, because it is the claim everything else in §1 rests on:

```
cargo tree -p pdfcer-core   | grep -Ei "egui|eframe|winit|wgpu"   # must be empty
cargo tree -p pdfcer-render | grep -Ei "egui|eframe|winit|wgpu"   # must be empty
```
