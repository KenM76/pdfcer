# pdfcer-core Consumer API Map — Part 1: Reading and the Object Model

> **Covers.** Everything you can learn from a PDF without mutating it:
> loading (`document`, `xref`, `parser`, `lexer`, `objstm`, `recover`,
> `linearization`, `crypto` read paths), the COS object model (`object`,
> `span`), the graph/view abstraction (`graph`, `view`), page structure
> (`page_tree`), content streams (`content`), text extraction
> (`text_extract`, `textstring`, `text_state`), fonts (`fontinfo`,
> `fontdata`, `text_extract::font`, `text_extract::cmap`), vector geometry
> and picking (`vector`, read/query half), stream + image decoding
> (`filters`, `image_codec`, `color`, `function`), and the read half of
> navigation/metadata (`outline`, `attachments`, `layers`, `annot`,
> `wrapper`, `signature` census), plus `settings`.
>
> **Does NOT cover.** `edit` / `EditSession` mutation verbs, the writer and
> save path, ce dimensions, form authoring/filling, annotation authoring,
> redaction, OCR, printing, `pageops` mutation, `export`. Those are
> **`02-editing-and-saving.md`** and **`03-capabilities.md`**.
>
> **Date.** 2026-08-13.
> **Verified against commit.** `6c5124c`. (Enumeration was performed at
> `5c37c7c`; `git diff --stat 5c37c7c 6c5124c -- crates/` is empty — no
> source file changed between the two, only `docs/`. Every `file:line`
> below is therefore valid at `6c5124c`.)
>
> **★ `crates/pdfce-gui/...` citations.** That crate was removed from this
> workspace in `Pass 247.0`. Every `pdfce@cce414e:crates/pdfce-gui/...`
> reference below is a *reference implementation* frozen at the last commit
> that carried it — read it with
> `git -C D:\Dev\pdfce show cce414e:crates/pdfce-gui/src/<file>` (the
> untouched backup repository) or on GitHub at `KenM76/pdfce` (archived). The shipping
> GUI is the separate `pdfcer-gui` project.
>
> **Audience.** An engineer or agent building a new GUI shell at
> `D:\dev\pdfcer-gui` against this crate, in a different session, with no
> ability to ask questions here. Rustdoc already exists and is good; this
> document is the thing rustdoc cannot be — *"I want to do X, what do I
> call, in what order, and what will bite me?"*

---

## 0. How to read this

- **`file:line` is a promise.** Every symbol named below was read from
  source at the stated line. If a name here does not compile, trust the
  compiler and treat this document as stale — do not assume you typoed.
- **`UNVERIFIED — …`** marks a genuine gap. It is never filler; it names
  exactly what you would have to check.
- Paths are relative to `D:\Dev\pdfcer\crates\pdfcer-core\`.
- Rust snippets are **compiling-shaped**, not copy-paste-complete: they
  assume `use` statements shown, elide error plumbing behind `?`, and
  assume a `doc`/`view`/`page` binding where obvious.

### The one architectural fact you must not break

`pdfcer-core` has **zero GUI/windowing dependencies** and must keep them
(`CLAUDE.md` rule 2, `ARCHITECTURE.md` §3; enforced in CI by grepping
`cargo tree -p pdfcer-core`). Its entire dependency set is `thiserror`,
`flate2`, `zune-jpeg`, `weezl`, `hayro-ccitt`, `hayro-jbig2`,
`hayro-jpeg2000` (optional, `jpx` feature), `jpeg-encoder`, `aes`, `cbc`,
`sha2` (`crates/pdfcer-core/Cargo.toml`). Your GUI depends on core; core
never learns your GUI exists. This is what keeps a future WASM fork a
shell swap rather than a rewrite.

`pdfcer-core` is also **panic-free by policy** on untrusted input
(`lib.rs:66-72`): `clippy::unwrap_used`, `expect_used`, `panic`, and
`indexing_slicing` are `deny` crate-wide, and `unsafe_code` is
`forbid`ed. Fallible paths return `Result`; unresolvable lookups return
`Option` or `Object::Null`. **Do not "fix" a function that returns
`Option` by unwrapping it in your shell** — the `Option` is usually
carrying a spec-mandated degradation, not an oversight.

### Feature flags

`default = ["jpx"]`. One strippable capability today: `jpx` gates JPEG 2000
decoding via `hayro-jpeg2000`. A gated-out path **refuses by name**
(`ImageCodecError::FeatureUnsupported`), never silently renders blank. CI
builds `--no-default-features`, so both configurations compile.

---

## 1. Capability index

*"I want to…"* → *"call this"*. Grep this table first.

| I want to… | Call this | Section |
|---|---|---|
| Check a file is a PDF without parsing it | `pdfcer_core::probe_file(&Path)` / `probe_header(&[u8])` — `lib.rs:280`, `lib.rs:252` | §3.1 |
| Load a PDF from disk | `Document::load(&Path)` — `document.rs:360` | §3.2 |
| Load a PDF from memory | `Document::from_bytes(Vec<u8>)` — `document.rs:392` | §3.2 |
| Open a password-protected PDF | `Document::load_with_password(&Path, Option<&[u8]>)` — `document.rs:382` | §3.5 |
| Know whether the open document was encrypted, and how | `Document::encryption() -> Option<&DocumentEncryption>` — `document.rs:755` | §3.5 |
| Read the author's declared permission bits | `enc.config.permissions()` — `crypto/standard.rs:795`, then `Permissions::granted(bit)` — `standard.rs:317` | §3.5 |
| Detect that the file was structurally damaged and rebuilt | `Document::loaded_via_recovery() -> bool` — `document.rs:1065`; detail via `Document::recovery()` — `document.rs:1057` | §3.6 |
| Warn that saving will destroy Fast Web View | `Document::linearization()` — `document.rs:1076`, then `Linearization::save_invalidates_fast_web_view()` — `linearization.rs:110` | §3.7 |
| Detect an ISO 32000-2 §7.6.7 encrypted-payload wrapper | `wrapper::detect(&graph) -> WrapperInfo` — `wrapper.rs:90`; message via `WrapperInfo::message()` — `wrapper.rs:141` | §3.8 |
| Get the effective PDF version (header + catalog `/Version`) | `Document::version()` — `document.rs:932` | §3.2 |
| Fetch one indirect object | `Document::get(ObjId) -> Option<&IndirectObject>` — `document.rs:951` | §4 |
| Follow a reference to a real value | `ObjectGraph::resolve(&Object)` / `::resolved(ObjId)` — `graph.rs:139`, `graph.rs:167` | §5 |
| Get the document catalog | `ObjectGraph::catalog_dict() -> Option<&Dict>` — `graph.rs:187` (or `Document::catalog() -> Result` — `document.rs:982`) | §5 |
| Iterate every object in the file | `Document::objects()` — `document.rs:997`; count via `object_count()` — `document.rs:992` | §4 |
| Read a dictionary key (null-collapsing, per spec) | `Dict::get(&[u8]) -> Option<&Object>` — `object.rs:157` | §4.2 |
| Get a page list with inheritance resolved | `page_tree::pages(&Document) -> Result<Vec<Page>, _>` — `page_tree.rs:228` | §6 |
| Do the same over an edit session or any graph | `page_tree::pages_in::<G: ObjectGraph>(&G)` — `page_tree.rs:245` | §6 |
| Get a page's MediaBox / CropBox / rotation | `Page::media_box`, `::crop_box`, `::rotate` — `page_tree.rs:111,115,118` | §6 |
| Build a read view to pass to render/vector/content | `Document::view() -> DocumentView<'_>` — `document.rs:910` | §5.2 |
| Decode + tokenize a page's content streams | `ContentStream::from_page(&DocumentView, &Page)` — `content.rs:208` | §7 |
| Walk content-stream operators semantically | `ContentStream::operations()` — `content.rs:296`; name via `Operation::operator_name(buf)` — `content.rs:137` | §7 |
| Extract all text from one page | `text_extract::extract_page(&Document, &Page, idx, &ExtractOptions)` — `text_extract/mod.rs:1093` | §8 |
| Extract all text from the whole document | `text_extract::extract_document(&Document, &ExtractOptions)` — `mod.rs:1191` | §8 |
| Extract text reflecting unsaved edits | `text_extract::extract_page_view` / `extract_document_view` — `mod.rs:1148`, `mod.rs:1210` | §8 |
| Get text as one string | `ExtractedText::plain_text()` — `mod.rs:1005`; file-sourced only: `sourced_text()` — `mod.rs:1027` | §8.3 |
| Get per-glyph positions for a selection highlight | `PageText::runs[].glyphs[]` → `ExtractedGlyph{x,y,advance,size}` — `mod.rs:407-452` | §8.4 |
| Get the byte-exact origin of a glyph (for editing) | `ExtractOptions::default().with_provenance(true)` then `ExtractedGlyph::provenance` — `mod.rs:948`, `mod.rs:325` | §8.4 |
| Search for text across the document | `EditSession::find_text_with(&needle, &TextSearchOptions)` — `edit.rs:11853` **(read-only in effect, but needs a session)** | §8.5 |
| Search for text **and learn what was unreadable** | `EditSession::search_text(&needle, &TextSearchOptions)` — `edit.rs:16450` → `TextSearch { matches, diagnostics }` | §8.5 |
| Render-setting preset for a subset standard (PDF/X, PDF/A, PDF/UA) | `pdfcer_core::settings::presets::RenderPreset::for_standard(RenderStandard)` | §8.5a |
| Decode a PDF text string (`/Title`, `/Author`, bookmark labels) | `textstring::decode_text_string(&[u8]) -> DecodedText` — `textstring.rs:363` | §8.6 |
| Inventory every font the document uses | `fontinfo::inventory(&DocumentView) -> FontInventory` — `fontinfo.rs:1601` | §9.1 |
| Know if a font is embedded / subsetted / removable | `FontRecord::program`, `::removability` — `fontinfo.rs:1209-1259`; `split_subset_tag` — `fontinfo.rs:1320` | §9.1 |
| Read a font's embedding permission (`OS/2 fsType`) | `fontinfo::read_fs_type(&[u8])` — `fontinfo.rs:744` | §9.1 |
| Resolve one font resource for text decoding | `ExtractFont::resolve(&DocumentView, &Dict)` — `text_extract/font.rs:381` | §9.2 |
| Map a character code to Unicode via `/ToUnicode` | `ToUnicodeCMap::parse(&[u8])` → `::lookup(u32)` — `cmap.rs:272`, `cmap.rs:552` | §9.3 |
| Get Base-14 metrics without any font file | `fontdata::std14_width`, `std14_descriptor` — `fontdata/mod.rs:382`, `:489` | §9.4 |
| Turn a page into selectable vector/text/image objects | `vector::decompose_page(&DocumentView, &Page, Matrix)` — `vector/decompose.rs:1626` | §10.1 |
| **Find what the user clicked** | **`vector::hit_test_point_deep(&PageObjects, Point, tolerance)` — `vector/hit.rs:255`** | §10.3 |
| Find what the user clicked, **page stream only** | `vector::hit_test_point(&PageObjects, Point, tolerance)` — `vector/hit.rs:126` | §10.3 |
| Cycle through overlapping objects under the cursor | `vector::hit_test_point_all` — `vector/hit.rs:174` | §10.3 |
| Reach objects drawn **inside** a form XObject | `PageObjects::leaves` → `vector::decompose::FormLeaf` — `vector/decompose.rs:1011` | §10.3 |
| **Marquee-select a region** | **`vector::hit_test_rect_deep(&PageObjects, Bounds, MarqueeMode, FormMarquee)`** — `vector/hit.rs`. `hit_test_rect` still exists and is **shallow**: it cannot see inside a form, so a rubber band and a click disagree about what is selectable | §10.3 |
| Drill into which text run / subpath was clicked | `vector::hit_test_text_runs` — `hit.rs:277`; `vector::hit_test_subpaths` — `hit.rs:340` | §10.3 |
| Snap a point to geometry | `vector::snap_candidates(Point, &SnapConfig, &PageObjects)` — `vector/snap.rs:449` | §10.4 |
| Pick a straight edge (CAD-style measuring) | `vector::linepick::pick_line_in_page` — `vector/linepick.rs:344` | §10.5 |
| Classify two picked edges as parallel/angled | `vector::linepick::classify_two_lines` — `vector/linepick.rs:392` | §10.5 |
| Decode a stream through its `/Filter` chain | `filters::decode_stream(&Dict, &[u8])` — `filters/mod.rs:186` | §11.1 |
| Know which image codec a stream ends in, without decoding | `image_codec::terminal_codec(&Dict)` — `image_codec/mod.rs:467` | §11.2 |
| Decode an image XObject to samples | `image_codec::decode_image(&Document, &Dict, &[u8], inline)` — `image_codec/mod.rs:503` | §11.2 |
| Convert a device colour to sRGB | `color::{gray_to_srgb, rgb_to_srgb, cmyk_to_srgb}` — `color/mod.rs:197, 215, 254` | §11.3 |
| Resolve a full `/ColorSpace` object (Separation, ICCBased, Indexed…) | **Not in `pdfcer-core`** — `pdfcer_render::ColorSpace`, `pdfcer-render/src/color.rs:215` | §11.3 |
| Evaluate a PDF function (type 0/2/3/4) | `function::PdfFunction::load(&DocumentView, &Object)` then `::eval` / `::eval_into` — `function.rs:751, 979, 1025` | §11.4 |
| Enumerate bookmarks as a tree, pages already resolved | `outline::read_outline(&graph)` — `outline.rs:1066`; flat list `Outline::flatten()` — `outline.rs:919` | §12.1 |
| List embedded attachments | `attachments::list_attachments_with_notes(&graph)` — `attachments.rs:850` | §12.2 |
| Extract an attachment's bytes | `attachments::extract_attachment(&DocumentView, &Attachment)` — `attachments.rs:1496` | §12.2 |
| Enumerate optional-content layers + default visibility | `layers::read_layers(&graph)` — `layers.rs:944` | §12.3 |
| Compute hidden layers, correctly for print/export | `annot::optional_content_default_off(&graph)` — `annot.rs:701` | §12.3 |
| Refine layer visibility for on-screen view only | `annot::apply_view_usage(&graph, …)` — `annot.rs:1268` **(never on a print path — T-12.8)** | §12.3 |
| List annotations on a page with their rects | `annot::page_annotations(&graph, page.id)` — `annot.rs:531` | §12.4 |
| Make hyperlinks clickable | `annot::page_link_destinations(&graph, page.id, &reader)` — `annot.rs:852`, with `outline::DestinationReader::new(&graph)` — `outline.rs:1649` built ONCE per document. Returns rect + fully resolved `Destination` per `/Link`. **`Pass 222.0` — this row previously said "no direct API"; that is obsolete.** | §12.4, §12.6.4 |
| Resolve where ONE annotation goes (incl. a `/Widget` pushbutton) | `Annotation::destination(&graph, &reader)` — `annot.rs:622`. Needs `Annotation::id`; use `page_link_destinations` when completeness matters. | §12.5.6.5 |
| Report dangling cross-references (document health) | `pageops::references::census_dangling` — `pageops/references.rs:336` ⚠️ **Counts REFERENCES only.** `/ResetForm`, `/SubmitForm` and `/Hide` name their targets by fully-qualified **name string**, and a name is not a reference — so deleting such a field leaves this report at zero while the buttons stop working. `is_empty() == true` is therefore **not** a clean bill of health on its own; pair it with `delete_field`'s `action_targets_orphaned` and `rename_field`'s `action_targets_retargeted` (`Pass 184.0`). | §12.4 |
| Census digital signatures, their byte coverage, and (`Pass 10.1`) their integrity | `signature::census(&graph)` — `signature.rs:370`; `signature::byte_range_coverage` — `signature.rs:900`; `signature::verify_all(&graph, bytes)` / `verify(&graph, bytes, index)` — `signature_verify.rs` | §12.5 |
| Read `/Info` title / author / subject / keywords | `EditSession::info_text(InfoField)` — `edit.rs:3807` **(needs a session; only those 4 fields)** | §12.6 |
| Read `/Producer`, `/CreationDate`, XMP, or page labels | **No public reader** — read the raw `/Info` dict via `ObjectGraph` | §12.6 |
| Load / persist user settings | `settings::resolve_store()` — `settings/mod.rs:1677`; `Settings` — `settings/mod.rs:840` | §13 |

---

## 2. Conventions: coordinate spaces, units, errors

### 2.1 Coordinate spaces — read this before writing any geometry code

This is the single most common integration defect, so it is stated once,
here, and then restated per function.

| Space | Y direction | Origin | Units | Where it appears |
|---|---|---|---|---|
| **PDF default user space** (a.k.a. "page space" in `vector`) | **y-UP** | bottom-left of the page | points (1/72 inch), `f64` | `Rect`, `Page::media_box`, `Bounds`, `Point`, all `vector` read APIs, `TextRun::bbox`, `Quad` |
| **Text space** | y-up | text-object relative | unscaled `Tf`/`Tc`/`Tw`/`Tz` operand units, `f64` | `TextStateParams`, `AmbientValue`, `TextFont::size`, `GlyphProvenance::tf_size` |
| **Glyph space** | y-up | glyph origin | 1/1000 em, `i16`/`u16` | `fontdata::std14_width`, `Std14Descriptor` |
| **Content-buffer byte offsets** | — | byte 0 of the *decoded* content stream | bytes, `usize` | `ContentToken::span`, `GlyphProvenance::operator_span` |
| **File byte offsets** | — | byte 0 of the retained file buffer | bytes, `usize` | `ByteSpan` in `Provenance`, `Stream::data_span` |
| **Screen / canvas space** | **y-DOWN** | top-left of your widget | pixels, your choice of type | **Does not exist anywhere in `pdfcer-core`.** |

**★ `pdfcer-core` never takes or returns screen space.** Not a point, not a
rectangle, and — critically — **not a tolerance**. Converting screen
pixels to page units, including the hit-test/snap catch radius, is
entirely your shell's job and nothing in core will check it for you. See
Trap T-1.

Two coordinate subtleties inside user space itself:

- `Rect` (`page_tree.rs:63`) is **normalised**: `llx ≤ urx`, `lly ≤ ury`
  always, because ISO 32000-1 §7.9.5 permits the two corners in either
  order. Build one with `Rect::from_corners` (`page_tree.rs:78`), never by
  assigning the raw array positionally.
- `Quad` (`annot_author.rs:129`) has `ul`/`ur`/`ll`/`lr`. Because y is UP,
  **`ul.1` is the LARGER y** — see `Quad::from_rect` at
  `annot_author.rs:143`, which sets `ul: (rect.llx, rect.ury)`. A quad is a
  general quadrilateral (`/QuadPoints`, §12.5.6.10), so take bounds over
  all four corners, never from `ll`/`ur` alone.

### 2.2 Page rotation is NOT applied to geometry

`Page::rotate` (`page_tree.rs:117`) is a display instruction — 0/90/180/270
clockwise. Every geometry value core hands you (`media_box`, `TextRun::bbox`,
`Bounds`, `Point`, snap candidates, hit-test input) is in **unrotated** page
space. Your canvas applies the rotation. Passing a rotation-adjusted point
back into `hit_test_point` will miss.

### 2.3 Page indices

Core is **0-based** everywhere (`PageText::page_index`, `TextMatch::page_index`,
the index into `page_tree::pages()`'s `Vec`). Humans are 1-based, and
`pdfcer` converts at the print boundary (`crates/pdfcer-cli/src/main.rs:9360-9361`:
*"1-based page, matching every other page-addressing surface in this CLI.
The extraction is 0-based and the operator is not."*). Do the same, once, at
your presentation layer.

### 2.4 Error style

Every error type is `thiserror`-derived and **`#[non_exhaustive]`**. Your
`match` arms **must** carry a wildcard, and new variants will arrive. This is
deliberate (`lib.rs:180-183`) — treat a wildcard arm as "an unexpected
structural failure I will report verbatim", not as dead code.

`Option` vs `Result` is meaningful, not stylistic:
- `Result` = a real failure the caller must handle.
- `Option::None` = a spec-sanctioned absence or degradation (a dangling
  reference, an absent optional key, an inference that could not be made).
- `Object::Null` = the resolution of anything unresolvable, per §7.3.10,
  which explicitly *"shall not be considered an error"*.

---

## 3. Loading a document

**Module set:** `document`, `xref`, `parser`, `lexer`, `objstm`, `recover`,
`linearization`, `crypto`, `wrapper`.

**★ The headline: you touch almost none of it.** `lexer`, `parser`, `xref`,
`objstm`, `recover`, and all of `crypto::{standard,r5,aes,rc4,md5,apply}`
are `pub` for crate-internal reuse and for `pdfcer`/tests. They are
driven exclusively from inside `Document::from_bytes_with_password`
(`document.rs:404`). A GUI calls `Document::load*`, matches on `DocError`,
and then reads four accessors: `encryption()`, `recovery()`,
`linearization()`, `version()`.

### 3.1 Cheap probe (no parse)

```rust
use pdfcer_core::{probe_file, probe_header, PdfVersion, HEADER_SCAN_WINDOW};

let v: PdfVersion = probe_file(std::path::Path::new("in.pdf"))?;  // lib.rs:280
println!("declares {v}");                                          // Display -> "1.7"
```

Reads **at most `HEADER_SCAN_WINDOW` = 1024 bytes** (`lib.rs:145`), so it is
safe on a hostile multi-gigabyte file. The 1024-byte tolerance for a
leading BOM/whitespace is **empirical practice, not spec** — `lib.rs:42-50`
says so explicitly and records that an earlier revision miscited it.

Use this for a file-picker filter or a drag-and-drop hover check. It says
only *"looks like a PDF, declares M.N"* — nothing about whether it opens.

### 3.2 The real load

```rust
use pdfcer_core::document::{Document, DocError};

let doc = Document::load(std::path::Path::new("in.pdf"))?;   // document.rs:360
let version = doc.version();                                  // document.rs:932
let n_objects = doc.object_count();                           // document.rs:992
```

`Document::load` = `from_bytes(std::fs::read(path)?)` (`document.rs:361`).
`Document` **owns the complete source bytes for its lifetime**
(`document.rs:259`) — this is the provenance substrate that makes
minimal-diff saving possible, and it means a loaded `Document`'s memory
cost is at least the file size. Budget for it; do not hold twenty open.

`version()` reconciles the header against the catalog's `/Version` and
returns the **max** of the two (§7.5.5) — an incremental update can raise
the version without touching byte 0.

### 3.3 The load pipeline (what actually happens, in order)

Useful for mapping an error back to a stage and for a progress UI.

1. **Header probe** — `probe_header` — called at `document.rs:409`.
2. **Strict xref chain load** — `xref::load_xref_chain` — `document.rs:411`,
   defined `xref.rs:480`. Walks `/Prev`, cycle-guarded.
3. **`Document::assemble`** — `document.rs:412`, defined `document.rs:468`:
   1. **Phase 1** — eagerly parse every in-use file-level object
      (`document.rs:503-528`).
   2. **Phase 1.5** — decrypt in place (`document.rs:541`, defined
      `document.rs:606`).
   3. **Phase 2** — inflate object streams and parse compressed objects
      (`document.rs:548`, defined `document.rs:810`).
   4. **Linearization detect** — `linearization::detect` — `document.rs:555`.
4. **On xref failure of a recoverable kind** → rebuild-by-scan recovery
   (`recover::recover`, `document.rs:432`) then the same assemble.
5. **On header-probe failure** → recovery is still attempted
   (`document.rs:450`), succeeding only if the scan finds objects **and** a
   `/Catalog`; otherwise the original "not a PDF" error stands.

The load is **eager and strict**: every in-use object is parsed before
`load` returns. There is no lazy-object mode. For a large file this is
your dominant open cost — put it on a worker thread (see §5.3, the
`Send + Sync` note).

### 3.4 `DocError` — the complete table

`document.rs:107`, `#[non_exhaustive]`.

| Variant | `file:line` | Meaning | Your action |
|---|---|---|---|
| `Io(io::Error)` | `:110` | file unreadable | report; not recoverable |
| `Encryption(EncryptionUnsupported)` | `:119` | encrypted with a config pdfcer refuses (e.g. `/R` 6) | **no password will help** — show the specific reason |
| `PasswordRequired` | `:133` | decryptable in principle; empty password already tried silently and failed | **prompt and retry** |
| `PasswordRequiresNormalisation` | `:160` | `/R` 5 + non-ASCII password; pdfcer does not implement SASLprep so a *correct* password can be rejected | **do NOT say "wrong password"** — say pdfcer cannot verify this one |
| `Header(PdfError)` | `:163` | not a PDF, and recovery also found nothing | report "not a PDF" |
| `Xref(XrefError)` | `:168` | unrecoverable xref failure, or the encrypted-and-damaged case | not recoverable |
| `BadObject{id,offset,source}` | `:171` | an xref-declared object failed to parse | not recoverable |
| `ObjectIdMismatch{expected,found,offset}` | `:183` | table and body disagree | not recoverable |
| `ObjectStreamMissing{container,num}` | `:198` | type-2 entry names an absent container | not recoverable |
| `ObjectStream{container,source}` | `:206` | container present but undecodable | not recoverable |
| `ObjectStreamIdMismatch{…}` | `:219` | pair table contradicts the xref | not recoverable |
| `NoCatalog` | `:236` | trailer `/Root` missing or not a dict | not recoverable |
| `Recovery(RecoverError)` | `:247` | recovery was attempted and failed cleanly | not recoverable — never a partial document |

Nested enums you may want to surface verbatim: `xref::XrefErrorKind`
(`xref.rs:306`, 12 variants), `parser::ParseErrorKind` (`parser.rs:90`, 11
variants), `lexer::LexErrorKind` (`lexer.rs:227`, 9), `objstm::ObjStmError`
(`objstm.rs:121`, 12), `recover::RecoverError` (`recover.rs:269`, 4),
`crypto::EncryptionUnsupported` (`crypto/standard.rs:118`, 7). All are
`#[non_exhaustive]`. Their `Display` strings are written to be shown to an
operator — prefer printing them over re-wording them.

### 3.5 Encryption: driving a password prompt

```rust
use pdfcer_core::document::{Document, DocError};
use pdfcer_core::crypto::AuthKind;

// ★ `None` is NOT the empty password. It means "no password known".
//   §7.6.3.1 requires trying the empty user password first and silently
//   in either case, so `None` still opens a permissions-only document
//   with no prompt (document.rs:364-371).
let doc = match Document::load_with_password(path, None) {          // document.rs:382
    Ok(doc) => doc,
    Err(DocError::PasswordRequired) => {
        // Genuinely has a non-empty user password. Prompt, then retry.
        let pw = prompt_for_password();
        match Document::load_with_password(path, Some(pw.as_bytes())) {
            Ok(doc) => doc,
            Err(DocError::PasswordRequired) => return Err("wrong password".into()),
            Err(DocError::PasswordRequiresNormalisation) => {
                // ★ NOT "wrong password". pdfcer cannot verify this one.
                return Err("password contains non-ASCII characters pdfcer cannot normalise".into());
            }
            Err(e) => return Err(e.into()),
        }
    }
    // No password will ever help for this one.
    Err(DocError::Encryption(e)) => return Err(e.into()),
    Err(e) => return Err(e.into()),
};

// Disclose what happened.
if let Some(enc) = doc.encryption() {                    // document.rs:755
    match enc.auth {                                      // document.rs:320
        AuthKind::EmptyUser => { /* no prompt was needed */ }
        AuthKind::User      => { /* user password: /P-limited access */ }
        AuthKind::Owner     => { /* owner password: full access, /P advisory */ }
    }
    let perms = enc.config.permissions();                 // crypto/standard.rs:795
    let can_print = perms.granted(pdfcer_core::crypto::PermissionBit::Print); // standard.rs:317
    // `granted` returns Option<bool>: None == "not applicable at this /R".
    let _ = enc.perms; // PermsCheck — Algorithm 3.13 verdict; see below.
}
```

Three things a shell gets wrong here:

- **Permissions are a disclosure, not a gate.** §7.6.3.1 states plainly
  that *"there is nothing inherent in PDF encryption that enforces the
  document permissions"* — quoted at `document.rs:315-318`. `permissions()`
  returns the dictionary's declared `/P`, and pdfcer never substitutes the
  decrypted copy. If you choose to grey out a button because of a
  permission bit, project rule 4 requires you to **say that you did**.
- **`PermsCheck::NotApplicable` is the ordinary answer for every `/R` ≤ 4
  document**, not a failed check. `document.rs:326-329` says so and warns
  a front end must not render it as one.
- **`AuthKind::EmptyUser` vs `Some(b"")`.** `None` means no password known;
  `Some(b"")` means the operator explicitly submitted an empty box. Same
  key, different `AuthKind`, and shells use it to know whether a prompt was
  ever shown (`crypto/standard.rs:1039-1044`).

### 3.6 ★ Recovery detection — check this before offering "Save"

```rust
if doc.loaded_via_recovery() {                       // document.rs:1065
    let r = doc.recovery().expect("just checked");    // document.rs:1057 -> &RecoveryReport
    // recover.rs:202 — reason, file_level_objects, objstm_objects,
    // last_wins_collisions, stream_lengths_recovered,
    // missing_endobj_recovered, trailer_source, offset_start
    show_banner(r);
    disable_incremental_save();
}
```

Recovery is **automatic and not opt-in**: when the strict xref path fails
with a recoverable kind, or the header probe fails, core rebuilds the table
by scanning for `N G obj` headers and tells you afterwards. A recovered
document **cannot be saved incrementally** (`ARCHITECTURE.md` §5.10 / R67;
the writer refuses, `document.rs:1061-1064`). Both accessors are `const fn`
— free to call, so gate your save UI on them rather than on catching the
refusal.

`RecoveryReport` is a *counted* disclosure by design (project rule 4,
"fuzzy, never sneaky"): show the counts, do not summarise them to "the file
was repaired".

### 3.7 Linearization

```rust
use pdfcer_core::linearization::Linearization;
match doc.linearization() {                                    // document.rs:1076
    Linearization::None => {}
    l => if l.save_invalidates_fast_web_view() {               // linearization.rs:110
        warn("saving will remove Fast Web View");
    }
}
```

`Linearization` (`linearization.rs:75`) is `None | Live{declared_length} |
Stale{declared_length, actual_length}`. pdfcer **never repairs it and never
strips a stale `/Linearized` dictionary** (`document.rs:1072-1074`).
`linearization::detect(&[u8])` (`linearization.rs:140`) is infallible.

### 3.8 Encrypted-payload wrapper (§7.6.7)

```rust
use pdfcer_core::wrapper;
let info = wrapper::detect(&doc);                    // wrapper.rs:90, takes any &G: ObjectGraph
if let Some(msg) = info.message() {                   // wrapper.rs:141
    show_banner(&msg);   // "the visible page is a cover sheet"
}
```

Cheap enough to run on **every** open (`wrapper.rs:85-88`: *"a detector an
operator has to remember to run is a detector that does not fire on the day
it matters"*). `WrapperInfo{is_wrapper, payload_name, payload_count}` —
`wrapper.rs:66`.

### 3.9 Resource limits (guards you must not remove)

All are pdfcer policy per `ARCHITECTURE.md` §10.1 unless noted.

| Constant | Value | `file:line` | Guards |
|---|---|---|---|
| `HEADER_SCAN_WINDOW` | 1024 B | `lib.rs:145` | header scan |
| `document::MAX_RESOLVE_DEPTH` | 32 | `document.rs:102` | reference cycles → `Object::Null`, not an error |
| `lexer::MAX_TOKEN_LEN` | 1 MiB | `lexer.rs:117` | unbounded token |
| `lexer::MAX_STRING_LEN` | 16 MiB | `lexer.rs:126` | unbounded string |
| `parser::MAX_NESTING_DEPTH` | 256 | `parser.rs:72` | `[[[[…` stack bomb |
| `xref::STARTXREF_SCAN_WINDOW` | 4096 B | `xref.rs:157` | trailing scan |
| `xref::MAX_XREF_SECTIONS` | 1024 | `xref.rs:165` | `/Prev` cycle |
| `xref::MAX_XREF_ENTRIES` | 10,000,000 | `xref.rs:172` | table size |
| `xref::MAX_W_FIELD_WIDTH` | 8 | `xref.rs:181` | xref-stream `/W` |
| `xref::MAX_XREF_STREAM_ROW` | 32 | `xref.rs:189` | xref-stream row |
| `objstm::MAX_OBJSTM_OBJECTS` | 1,000,000 | `objstm.rs:112` | `/N` before allocation |
| `filters::MAX_DECODED_LEN` | 256 MiB | `filters/mod.rs:94` | decompression bomb, enforced **incrementally** |
| `page_tree::MAX_TREE_DEPTH` | 64 | `page_tree.rs:52` | page-tree nesting |
| `page_tree::MAX_PAGES` | 1,000,000 | `page_tree.rs:57` | page count |
| `crypto::r5::MAX_PASSWORD_LEN` | 127 B | `crypto/r5.rs:109` | `/R` 5 truncation (spec) |
| `linearization::LINEARIZATION_SCAN_WINDOW` | 1024 B | `linearization.rs:65` | **spec-mandated** (Annex F.3.3), not policy |

### 3.10 Traps — loading

- **T-3.1 `None` ≠ empty password.** `document.rs:364-371`. Passing
  `Some(b"")` when you meant "user hasn't typed anything" changes the
  reported `AuthKind` and therefore your UI's story.
- **T-3.2 `PasswordRequiresNormalisation` must not be shown as "wrong
  password".** `document.rs:145-151`: doing so *"would send the operator to
  re-check a password that was correct."*
- **T-3.3 A damaged **and** encrypted file reports as
  `DocError::Xref(XrefErrorKind::EncryptionUnsupported)`, not as a recovery
  error** (`document.rs:434-439`). Do not route it into your "file damaged"
  branch.
- **T-3.4 Object-level failures never trigger recovery.** `recover.rs:341-347`:
  recovery is scoped strictly to the xref-parse stage, so `BadObject`,
  `ObjectIdMismatch`, `ObjectStream*` after a clean xref load are terminal.
  Documented limitation, not an oversight — do not build a "try harder"
  retry on top.
- **T-3.5 `Document` retains the whole file buffer.** `document.rs:259`.
  Memory scales with file size, not object count.
- **T-3.6 Permissions are advisory.** §3.5 above. Enforcing one silently
  violates project rule 4.

### 3.11 Stability

Settled. `lexer.rs`, `objstm.rs`, `linearization.rs`, `span.rs` have only
their initial implementation commit (`d8b3903`, 2026-08-01). `parser.rs` and
`recover.rs` took two robustness fixes (`409a6b5`, `49dfe81`). `xref.rs`
took encryption and writer-fidelity work. **`crypto/*` is the youngest and
most active part** — `/R` 5 / AES-256 landed most recently (`bb6d678`,
`3618072`, 2026-08-12), and `/R` 6 is explicitly and currently unsupported
(`EncryptionUnsupported::UnsourcedRevision`, `crypto/standard.rs:166-179`).
Expect new `DocError`/`EncryptionUnsupported` variants; the
`#[non_exhaustive]` attributes already protect your match arms.

---

## 4. The object model

**Module set:** `object`, `span`.

### 4.1 `Object` — the COS value

`object.rs:260`, `#[non_exhaustive]`:

```
Null | Boolean(bool) | Integer(i64) | Real(f64) | String(Vec<u8>)
| Name(Name) | Array(Vec<Object>) | Dict(Dict) | Stream(Stream)
| Reference(ObjId)
```

Accessors, all `Option`-returning (`object.rs:292-347`): `as_int`,
`as_number` (widens `Integer` to `f64` per §7.3.3 NOTE 2 — **use this, not
`as_int`, for anything a producer may write either way**), `as_name`,
`as_dict`, `as_array`, `as_reference`.

`Integer` and `Real` are deliberately distinct. `String(Vec<u8>)` is **raw
bytes**, escapes already applied — interpreting it as *text* is
`textstring::decode_text_string`'s job (§8.6), never `String::from_utf8`.

### 4.2 `Dict` — and its one spec-driven surprise

`object.rs:143`: `pub struct Dict(pub Vec<(Name, Object)>)` — an ordered
`Vec`, not a hash map, so parsed entry order is preserved for minimal-diff
re-emission.

**★ `Dict::get` collapses null.** `object.rs:157`: an entry whose value is
`Object::Null` returns `None`, because §7.3.7/§7.3.9 make a null-valued
entry identical to an absent one. This is implemented once so no call site
needs a second null check. Consequently `Dict::len()` (`object.rs:176`) is
the **physical** count including explicit nulls and may exceed the number of
keys `get` will answer for. Use `len` only for serialisation; use `get` /
`contains_key` for semantics.

`Name` (`object.rs:93`) stores the **decoded** bytes with `#`-escapes
expanded, so `/Type` and `/Ty#70e` hash and compare equal. Names are raw
bytes, not guaranteed UTF-8. Look keys up with byte literals:
`dict.get(b"MediaBox")`.

### 4.3 `IndirectObject`, `ObjId`, `Provenance`

- `ObjId{num: u32, generation: u16}` — `object.rs:62`. `Display` renders
  `"num gen"`.
- `IndirectObject{id, value, provenance}` — `object.rs:572`.
- `Provenance` — `object.rs:474`, `#[non_exhaustive]`:
  `File(ByteSpan)` | `RecoveredFile(ByteSpan)` | `ObjectStream{container, index}`.

**★ `Provenance::file_span()` and `is_verbatim_safe()` answer different
questions.** `object.rs:526` returns `Some` for both `File` and
`RecoveredFile` — the bytes genuinely exist and a UI showing "where is this
object defined" is right to ask. `object.rs:552` (`is_verbatim_safe`) is
`true` only for `File`. The doc comment states the reason: testing
`file_span().is_some()` *"would silently start copying self-contradictory
bytes the day the third variant appeared."* Read-only shells only need
`file_span`; do not repurpose it as a save-safety test.

### 4.4 `ByteSpan`

`span.rs:71`: `{start: usize, len: usize}`, `Copy`, ordered, hashable.
Methods: `new` `:83`, `from_range` `:95`, `end` `:104`, `range` `:110`,
`slice(&[u8]) -> Option<&[u8]>` `:122`.

`slice` returning `None` *"always indicates a logic error (a span applied to
a buffer it wasn't produced from)"* — surfaced as `Option` rather than a
panic per the crate policy. If you see `None`, you mixed up buffers (very
likely base vs. session — see §5.2). `from_range` on an inverted range
degrades to a zero-length span rather than panicking (`span.rs:88-93`).

### 4.5 Worked sequence — read an arbitrary catalog key

```rust
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;

let catalog = doc.catalog_dict().ok_or("no catalog")?;      // graph.rs:187
// /PageLayout is a name; /OpenAction may be an array or a dict.
let layout = catalog.get(b"PageLayout")                      // object.rs:157
    .map(|o| doc.resolve(o))                                 // graph.rs:139
    .and_then(Object::as_name)
    .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned());
```

Note the shape: `get` → `resolve` → `as_*`. **Always `resolve` before
`as_*`**, because any value position may hold an indirect reference
(§7.3.10 substitutability). Forgetting it produces a silent `None` on files
that happen to write the value indirectly — a bug that passes on your
fixtures and fails on a customer's file.

### 4.6 Stability

`object.rs` and `span.rs` are effectively frozen — `span.rs` has only
`d8b3903` (2026-08-01), `object.rs` took one fix (`409a6b5`, 2026-08-03,
adding `Provenance::RecoveredFile`). Treat as settled.

---

## 5. The graph abstraction and views

**Module set:** `graph`, `view`.

### 5.1 `ObjectGraph` — why most read APIs are generic

`graph.rs:113`:

```rust
pub trait ObjectGraph: Send + Sync {
    fn value(&self, id: ObjId) -> Option<&Object>;              // :124  REQUIRED
    fn trailer_entry(&self, key: &[u8]) -> Option<&Object>;     // :132  REQUIRED
    fn resolve<'a>(&'a self, obj: &'a Object) -> &'a Object;    // :139  provided
    fn resolved(&self, id: ObjId) -> &Object;                   // :167  provided
    fn catalog_dict(&self) -> Option<&Dict>;                    // :187  provided
    fn catalog_id(&self) -> Option<ObjId>;                      // :199  provided
}
```

Implementors: `Document` (`graph.rs:210`), `DocumentView<'_>`
(`view.rs:387`), and `EditSession`'s overlay views. The trait exists so
there is exactly **one** page-tree walk, one outline walk, one copier —
correct for both the file-as-loaded and the file-as-edited
(`graph.rs:17-33`). The §7.3.10 resolution rules (dangling → null, cycle
depth-guarded) live in the provided methods so no view can get them subtly
different (`graph.rs:35-44`).

**Which do you pass?** If a function takes `&G: ObjectGraph`, pass
`&doc` for the base file or `&session` for edited state. If it takes
`&DocumentView`, see §5.2 — that choice is load-bearing.

`Document` also has inherent `resolve` (`document.rs:959`) and `catalog`
(`document.rs:982`, `Result`-returning) with identical semantics; inherent
methods win method resolution, so both spellings work.

### 5.2 ★ `DocumentView` — and the base-vs-session trap

`view.rs:282`. Built by `Document::view()` (`document.rs:910`) or
`EditSession::view()` (`edit.rs:3469`). It bundles three things: a
`&dyn ObjectGraph`, a **byte source**, and the version.

The byte source is the point. `StreamSource` (`view.rs:144`) is
`Contiguous(&[u8])` | `Split{base, staged}`. A `Document` has one buffer;
an `EditSession` has two, because content rewritten this session lives in a
staging buffer. So:

```rust
let v = doc.view();          // base revision — the file as it is on disk
let v = session.view();      // edited state — what the operator is looking at
```

`content.rs:186-203` states the consequence bluntly: *"Getting this wrong is
not a crash, it is the Pass 17.0 defect: the content parses fine and shows
the wrong document."* Every function in §7–§10 that takes a
`&DocumentView` inherits this choice.

Accessors: `graph()` `:337`, `source()` `:345`, `slice(ByteSpan)` `:355`,
`bytes() -> Option<&[u8]>` `:365`, `version()` `:375`.

**★ Use `view.slice(span)`, never `span.slice(doc.bytes())`.**
`DocumentView::bytes()` returns `Option` and is `None` for a split
(session) view (`view.rs:357-364`) — deliberately, because *"any answer
other than 'there isn't one' would be the X5 mis-slice hazard wearing a
plausible face."* If you find yourself unwrapping `bytes()`, you are about
to read a session's authored appearance streams off the end of the base
buffer.

`Document::view()` is cheap — *"two borrows plus a version probe. Building
one per call is the intended usage; there is nothing to cache"*
(`document.rs:889-891`).

### 5.3 Threading

`ObjectGraph: Send + Sync` (added 2026-08-07, `e4256f2`) exists specifically
so **a page can be rasterized off the UI thread** (`graph.rs:80-97`). The
doc comment quantifies why: inline rasterization of a real CAD sheet is
*"~10 s at 1× and ~58 s at 2× — not a slow redraw but a dead application."*
`DocumentView<'a>` is what crosses the thread boundary.

Build your shell assuming render and text extraction run on a worker from
day one. `pdfcer-render` also gained a `RenderCancel` / `RenderOptions.cancel`
mechanism in the same commit — see part 3.

### 5.4 Stability

`graph.rs` changed once since inception (the `Send + Sync` supertrait,
`e4256f2`, 2026-08-07). `view.rs` has one commit (`3a56b55`, 2026-08-02,
decision 018 — the change that created it). Both settled; note that
`view.rs`'s current signature *is* the post-migration shape, so the
base-vs-session distinction is a decided design, not a transitional state.

---

## 6. Pages

**Module:** `page_tree`.

### 6.1 Entry points

```rust
use pdfcer_core::page_tree::{self, Page, Rect, PageTreeError};

let pages: Vec<Page> = page_tree::pages(&doc)?;        // page_tree.rs:228
// Generic over any graph — use for an EditSession:
let pages = page_tree::pages_in(&session)?;            // page_tree.rs:245
```

**★ `pages(&doc)` is the base revision, not the edited state.**
`page_tree.rs:218-221` flags this with a warning marker: anything that must
see unsaved structural edits calls `EditSession::pages()` (`edit.rs:4016`),
which walks the overlay through the same code. After a page delete, a base
walk still returns the deleted page.

Also available: `page_slots(&G) -> Vec<PageSlot>` (`page_tree.rs:362`) with
`PageSlot` (`:283`) and `InheritedRaw` (`:306`) — the unresolved view, for
writers that need to know where an attribute physically lives. A read-only
GUI wants `pages`/`pages_in`.

### 6.2 `Page` — `page_tree.rs:103`

| Field | Type | Line | Notes |
|---|---|---|---|
| `id` | `ObjId` | `:106` | always known (pages are reached via indirect `Kids`) |
| `resources` | `Dict` | `:109` | resolved: own, inherited, or explicit empty |
| `media_box` | `Rect` | `:111` | normalised, user space, points |
| `crop_box` | `Rect` | `:114` | defaults to `media_box`; **this is what you clip display to** (Table 30) |
| `rotate` | `u16` | `:117` | 0/90/180/270 clockwise, display only — see §2.2 |
| `contents` | `Vec<ObjId>` | `:122` | in order; concatenate. Empty = empty page, **not** an error |
| `contents_unresolved` | `usize` | `:147` | **see below** |

**★ `contents_unresolved` is a count you must surface.** `page_tree.rs:123-146`:
a `/Contents` element naming an object not in the file degrades to nothing
(§7.3.10 makes a dangling reference the null object; Table 30 makes absent
`/Contents` an empty page). Non-zero means *"content the page asked for
could not be drawn or extracted."* The doc comment states the obligation
directly: *"a silently-empty page is indistinguishable from a genuinely
blank one, and the operator would have no way to tell that text they
expected is missing."* Under project rule 4 this is a disclosure, not a
diagnostic to swallow.

Note the boundary: an element of the wrong *type* (a number, a dict, a
non-reference) is still a hard `PageTreeError::BadContents`. Only
resolves-to-null degrades.

### 6.3 `Rect` — `page_tree.rs:63`

`{llx, lly, urx, ury}: f64`, **always normalised** (min,min)→(max,max)
because §7.9.5 allows the corners in either order. Construct via
`Rect::from_corners(x1,y1,x2,y2)` (`:78`). `width()` `:88` and `height()`
`:98` are non-negative by construction.

### 6.4 `PageTreeError` — `page_tree.rs:153`, `#[non_exhaustive]`

`NoPageTreeRoot` `:156` · `BadKid(ObjId)` `:159` · `Cycle(ObjId)` `:162` ·
`TooDeep` `:165` · `TooManyPages` `:168` · `MissingRequired(&'static str)`
`:173` · `BadRectangle(&'static str)` `:176` · `BadRotate(i64)` `:179` ·
`BadContents` `:196`.

A well-formed empty tree (`/Count 0`, empty `Kids`) returns an **empty
vec, not an error** (`page_tree.rs:226-227`).

### 6.5 Worked sequence — page list for a thumbnail rail

```rust
use pdfcer_core::page_tree;

let pages = page_tree::pages(&doc)?;
for (i, p) in pages.iter().enumerate() {
    let w = p.crop_box.width();          // points, user space
    let h = p.crop_box.height();
    // Swap for display if the page is rotated 90/270.
    let (dw, dh) = if p.rotate % 180 == 90 { (h, w) } else { (w, h) };
    push_thumbnail(i, dw, dh, p.contents_unresolved > 0 /* show a damage pip */);
}
```

### 6.6 Traps — pages

- **T-6.1 `pages()` is the base document.** Use `EditSession::pages()` for
  edited state (`page_tree.rs:218-221`).
- **T-6.2 Clip to `crop_box`, size from `crop_box`, not `media_box`.**
  Table 30; `page_tree.rs:112-113`.
- **T-6.3 `rotate` is not applied to any geometry core returns.** §2.2.
- **T-6.4 An empty `contents` vec is legal.** Do not treat it as failure.
- **T-6.5 `contents_unresolved > 0` is silent data loss unless you show it.**

### 6.7 Stability

Settled — `page_tree.rs` has two commits: initial (`d8b3903`) and the
dangling-`/Contents` degradation (`409a6b5`, 2026-08-03) that added
`contents_unresolved`.

---

## 7. Content streams

**Module:** `content`.

This is the lossless token layer beneath text extraction and the vector
model. You need it directly only if you are inspecting or annotating raw
operators; for selection and text, prefer §8 and §10.

### 7.1 Entry points

```rust
use pdfcer_core::content::{ContentStream, ContentError, ContentTokenKind};

// Decode + concatenate + tokenize a page's /Contents.
let cs = ContentStream::from_page(&doc.view(), &page)?;   // content.rs:208
// Or tokenize bytes you already have:
let cs = ContentStream::parse(decoded_bytes)?;            // content.rs:242

for op in cs.operations() {                                // content.rs:296
    match op.operator_name(&cs.buf) {                      // content.rs:137
        Some(b"Tj") | Some(b"TJ") => { /* operands in op.operands */ }
        Some(name) => { /* other operator */ }
        None => { /* ★ an inline image (BI…EI) — NOT an operator */ }
    }
}
```

`ContentStream{buf: Vec<u8>, tokens: Vec<ContentToken>}` — `content.rs:110`.
`buf` is the **decoded, concatenated** content; every `ContentToken::span`
indexes into it (not into the file). `ContentTokenKind` (`content.rs:85`) is
`Operand(Object)` | `Operator` | `InlineImage{params, data}`.

Multiple `/Contents` streams are joined with a single LF (`content.rs:180-184`),
because §7.7.3.3 guarantees the split falls on a token boundary but not that
the boundary carries whitespace.

`ContentError` — `content.rs:148`, `#[non_exhaustive]`: `Lex`, `BadOperand`,
`TooDeep`, `BadInlineParams`, `UnterminatedInlineImage`, `Decode`,
`NotAStream`.

### 7.2 Traps — content

- **T-7.1 `operator_name` returns `None` for an inline image**, not
  `b"BI"` (`content.rs:126-142`). `.unwrap()` here panics on any page with
  an inline image.
- **T-7.2 `operations()` silently drops trailing operands with no
  operator** (`content.rs:288-296`) — the tolerance every real viewer
  applies. The tokens remain in `self.tokens` for lossless re-emission, so
  a token-count and an operation-count will legitimately disagree.
- **T-7.3 Spans index the decoded buffer, not the file.** Mixing a
  `ContentToken::span` with `doc.bytes()` gives garbage, not an error.
- **T-7.4 `from_page` takes a `DocumentView`** — base vs session, §5.2.

### 7.3 Stability

Two commits; the `from_page(view, page)` signature is the *result* of
decision 018 (`3a56b55`, 2026-08-02), so treat it as post-migration settled.

---

## 8. Text extraction

**Module set:** `text_extract` (+ `textstring`, `text_state`).

**★ Structural fact:** `text_extract/mod.rs:133-134` declares
`mod layout;` and `mod page;` — **private**. Everything in `page.rs` and
`layout.rs` is internal. `pub mod cmap;` (`:131`) and `pub mod font;`
(`:132`) are public. Do not attempt to build against
`text_extract::page::*` or `::layout::*`; they do not exist outside the
crate.

### 8.1 The six entry points

```rust
// text_extract/mod.rs
pub fn extract_page(doc: &Document, page: &Page, page_index: usize,
                    options: &ExtractOptions) -> Result<PageText, ExtractError>;      // :1093
pub fn extract_page_view(doc: &DocumentView<'_>, page: &Page, page_index: usize,
                    options: &ExtractOptions) -> Result<PageText, ExtractError>;      // :1148
pub fn extract_document(doc: &Document,
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; // :1191
pub fn extract_document_view(doc: &DocumentView<'_>,
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; // :1210
pub fn extract_pages(doc: &Document, indices: &[usize],
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; // :1255
pub fn extract_pages_view(doc: &DocumentView<'_>, indices: &[usize],
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; // :1273
```

The `_view` variants exist for the base-vs-session choice (§5.2). Use them
for anything reflecting unsaved edits.

`ExtractError` — `mod.rs:961`: `PageTree(PageTreeError)`,
`NoSuchPage{index, count}`, `Content(ContentError)`.

**★ The plural and singular forms have different failure semantics.**
`extract_document*` / `extract_pages*` **swallow** a per-page content
failure: the page becomes an empty-`runs` `PageText` and the failure is
counted in `TextDiagnostics::pages_unreadable` (`mod.rs:1220-1240`).
`extract_page` / `extract_page_view` **propagate** `ExtractError::Content`
for the one page requested (`mod.rs:1076-1078`). A whole-document extract
that "worked" may therefore have lost pages — check the diagnostic.

### 8.2 `ExtractOptions` — `mod.rs:725`, `#[non_exhaustive]`, builder-style

| Field | Default | Line | Note |
|---|---|---|---|
| `include_artifacts` | `false` | `:733` | policy, not conformance — §14.8.2.2 requires nothing |
| `word_gap_ratio` | `0.20` | `:737` | derived word space |
| `line_gap_ratio` | `0.30` | `:740` | derived line break |
| `backward_jump_ratio` | `0.50` | `:750` | two-column detection |
| `max_form_depth` | `64` | `:756` | corpus-corrected; a conformant PDF/A file has a 32-deep chain |
| `capture_provenance` | `false` | `:769` | **must opt in** for per-glyph provenance |
| `unmappable_code` | `ReplacementChar` | `:783` | the sentinel for an unmappable code |
| `actual_text` | `Always` | `:792` | whether `/ActualText` replaces glyph-derived characters |

**★ The three gap ratios have zero spec basis** (`mod.rs:714-722`, negative
results S3/S4). If you expose them as settings, label them as heuristics,
not conformance knobs. The last two are `settings::UnmappableCode` /
`settings::ActualTextPrecedence` — spec ambiguities deliberately made
settings per the operator's standing directive (R169), not hard-coded.

### 8.3 Getting a string out

```rust
use pdfcer_core::text_extract::{self, ExtractOptions};

let opts = ExtractOptions::default();
let all = text_extract::extract_document(&doc, &opts)?;   // mod.rs:1191
let s = all.plain_text();      // mod.rs:1005 — sourced chars + derived whitespace
let s2 = all.sourced_text();   // mod.rs:1027 — ONLY characters the file actually contains
```

**★ Pages are joined by U+000C (form feed), never `\n`** (`mod.rs:991-998`).
Splitting on `\n` will not separate pages and may merge one page's last line
with the next page's first.

**★ `sourced_text()` is not readable prose.** Line breaks are *always*
derived, even in Tagged PDF (`mod.rs:33-36`, negative result S5), so
`sourced_text` for a two-line file is `"HelloworldSecond line"` with no
separator. Use `plain_text()` for anything a human reads;
`sourced_text()` only when you must prove a character came from the file.

### 8.4 Positioned runs and glyphs — what a highlight needs

```rust
use pdfcer_core::text_extract::{self, ExtractOptions, TextOrigin};

let opts = ExtractOptions::default().with_provenance(true);   // mod.rs:948
let all = text_extract::extract_document(&doc, &opts)?;

for page in &all.pages {                       // Vec<PageText>            mod.rs:524
    for run in &page.runs {                    // Vec<TextRun>, content order  mod.rs:459
        if run.artifact.is_some() { continue; }         // ★ see T-8.3
        let bbox = run.bbox;                            // Option<Rect>, USER SPACE, f64
        if run.origin == TextOrigin::Glyphs {
            for g in &run.glyphs {                       // ExtractedGlyph     mod.rs:407
                let (x, y) = (g.x, g.y);                 // user space, f32, points
                let adv    = g.advance;                  // user space, f32
                let size   = g.size;                     // EFFECTIVE size, user space, f32
                // ★ NOT one char per glyph:
                let text = &run.text[g.text_start as usize
                                    ..(g.text_start + g.text_len) as usize];
            }
        }
    }
}
```

`TextRun`: `text`, `origin`, `glyphs`, `artifact`, `mcid`,
`bbox: Option<Rect>`; method `direction() -> (f32, f32)`.
★ `text` is **not** one `char` per glyph and the run is **not** one show
operator — see §8.4.0 before building anything that locates an edit from a
run.
`TextOrigin`: `Glyphs` | `ActualText` | `DerivedWordSpace` |
`DerivedLineBreak`. Only `Glyphs` runs have glyphs.
`ExtractedGlyph`: `code`, `rung`, `text_start`, `text_len`,
`x`, `y`, `advance`, `size`, **`direction: (f32, f32)`**, `invisible`,
`provenance`; methods `up()`, `advance_end()`, `cell()`.
`GlyphProvenance`: `operator_span`, `text_matrix`, `ctm`,
`tf_size`, `composite`, … — `None` for every glyph unless
`capture_provenance` was set.

#### ★★★ 8.4.0 A run's `text` is not one character per glyph, and a run is not one show operator (`Pass 145.0`)

**Two facts a caller building an edit locator out of a run needs, both of
which were true and neither of which was written down anywhere they would
look.** A consuming project got each of them wrong in turn, on the same
afternoon, and the operator-facing symptom was *"eleven pieces of text went
bold and the twelfth refused"* on a page where nothing is unusual.

##### `text.chars().count()` is not `glyphs.len()`

`/ToUnicode` maps a character **code** to a **string**, not to a character
(ISO 32000-1 §9.10.3). So **one glyph can carry several characters**: an
`ffl` ligature is one glyph and three `char`s; a code mapping above the BMP
is one glyph and two `char`s. `ExtractedGlyph::text_start` / `text_len`
already say this — they are a **range**, not an index — but a caller reading
"what text is in this run" lands on `TextRun::text` and sees a `String`.

Measured over pdfcer's fixture corpus: **1 of 191 synthetic runs** has
`len(text) != len(glyphs)` (`text/identity-h-tounicode.pdf` — 8 characters
over 6 glyphs). That ratio is the trap, not a reassurance: **it is near-zero
on synthetic test text and routine on real typeset copy**, so a locator built
on a 1:1 assumption passes every fixture a shell writes for itself and fails
on the customer's document.

⇒ **Do not rebuild a `find` string from a run and expect it to match the
content stream.** Use `FormatRequest::whole_operator(page, span)` to
restyle it, or `EditRequest::whole_operator(page, span, replacement)` to
replace its text — see `02-editing-and-saving.md`.

Both halves are named here deliberately. This paragraph previously named only
the restyle verb, and it is the paragraph a consuming project was acting on
when it filed a defect saying the edit verb could only be addressed by `find`
(2026-08-28, `Pass 152.0`). A locator-facing sentence that names one of two
sibling verbs reads as a statement that the other has no such route.

##### A `TextRun` is NOT a show operator

`text_extract::layout` closes a run on **geometry** (a gap, a direction
change, a style change). A producer closes a `Tj`/`TJ` wherever its writer
felt like. The two agree often enough to look like a rule and **do not**:

| measured over `fixtures/` — 4,289 files, 1,623 with text | |
|---|---:|
| runs | 18,559 |
| glyphs | 669,436 |
| distinct `operator_span` groups | 29,246 |
| **runs carrying glyphs from MORE THAN ONE show operator** | **2,420 (13 %)** |

`crates/pdfcer-core/tests/operator_span_invariant.rs` is that measurement, and
it re-runs on every `cargo test`.

##### ★ The invariant you may rely on, now that it is measured

> **The glyphs sharing one `GlyphProvenance::operator_span` slice a
> contiguous, matchable range out of their run's `text`.**

**0 exceptions in 29,246 operator groups.** Both halves are asserted by that
test file: contiguity (no other operator's glyphs interleave inside the
range) and clean indexing (the slice lands on `char` boundaries inside
`run.text`). This was previously undocumented load-bearing behaviour that a
downstream project had already shipped against without being able to check
it; it is now a guarantee, and a `layout` refactor that breaks it turns that
test red rather than a customer's document.

**What is NOT guaranteed:** that a run has only one such group — see the 13 %
above.

##### Getting a span from outside the library

`pdfcer extract-text --json --spans` emits `op_start`, `op_len` and
`stream` per glyph. Without `--spans` those three fields are **absent**, not
zero, because provenance capture is off by default and "not captured" must
not read as "offset 0". `stream` is `"page"` or `"form:N"`, and it is not
optional to read: a page's `/Contents` are concatenated into one decoded
buffer and every form XObject is a separate one, so a form's span pinned
against the page names a different operator or none.

#### ★★★ 8.4.1 Text is not always horizontal — `direction` (`Pass 139.0`)

**`advance` and `size` are MAGNITUDES.** They are the lengths of the two
transformed basis vectors of §9.4.4's text rendering matrix. Until
`Pass 139.0` the directions were discarded, and every consumer downstream
had no choice but to assume the text ran along `+x`.

That assumption holds for virtually every word-processor page and **fails on
every CAD title block**, which stamps its source path with
`Tm = [0 1 -1 0 e f]` — ordinary horizontal-mode text placed by a rotated
matrix, **not** §9.7.4.3 vertical writing mode.

| you want | use | not |
|---|---|---|
| the next glyph's origin | `g.advance_end() -> (f32, f32)` | `g.x + g.advance` |
| a glyph's page-space box | `g.cell() -> Rect` | `min(x, x+advance)`, `y-0.25*size` … |
| which way a run reads | `run.direction() -> (f32, f32)` | assuming `(1, 0)` |
| "up" from the baseline, for a caret or `/QuadPoints` | `g.up()` | `(0, 1)` |
| a caret's page-space point | `model.caret_point(pos) -> Option<(f32,f32)>` | `model.caret_x(pos)` |

`(1.0, 0.0)` for ordinary horizontal text and for a degenerate matrix, so a
consumer that ignores `direction` entirely behaves exactly as it did before
the field existed.

**Every glyph in one run shares that run's direction**, guaranteed:
`text_extract::layout` closes a run on a direction change, and
`EditableTextModel`'s Stage 1 splits a line on one. So `TextRun::direction()`
answers from the first glyph without a scan, and `Line::direction` is a claim
about the whole line rather than about its head.

**What `direction` is not.** It is not `/WMode 1`. It is not a reading order
(§14.8.2.3.1's *"may or may not coincide"* still stands — content order is
unchanged). And `advance` is still **unsigned**: a glyph whose §9.4.4
displacement came out negative (a negative `Tc` larger than the glyph, a
negative `Tz`) steps *backward* along `direction` and that sign is not
published. No such glyph exists in the corpus; the alternative would have
flipped `direction` by 180° mid-run, which is worse for every consumer that
wants to orient a caret.

**The measurement**, so the size of this is not in doubt: on a SOLIDWORKS
drawing set whose title block carries the source path stamped vertically,
extraction returned that one line as **82 glyphs in 72 runs separated by 71
derived line breaks**. It pasted into a text editor as one character per
line. Acrobat returns one line.

Coordinate summary for this section:

| Value | Space | Units | Type |
|---|---|---|---|
| `ExtractedGlyph::{x,y,advance,size}` | default user space (y-UP) | points | `f32` |
| `ExtractedGlyph::direction` | default user space, **unit vector** | — | `(f32, f32)` |
| `TextRun::bbox` | default user space | points | `Option<Rect>` (`f64`) |
| `GlyphProvenance::tf_size` | **text space** — the raw `Tf` operand | unscaled | `f32` |
| `GlyphProvenance::{text_matrix, ctm}` | §8.3.3 row-vector `[a b c d e f]` | — | `[f32; 6]` |
| `GlyphProvenance::operator_span` | decoded-content byte offsets | bytes | `ByteSpan` |

Note the pairing: `ExtractedGlyph::size` is the **effective** size (the
y-scale of the text rendering matrix); `GlyphProvenance::tf_size` is the
**raw operand**. `mod.rs:338-341` contrasts them explicitly. Use `size` to
draw; use `tf_size` only to reason about the source operator.

### 8.5 ★ Search — it lives on `EditSession`

There is no read-only search entry point. Text search is:

```rust
use pdfcer_core::edit::{EditSession, TextSearchOptions};

let mut session = EditSession::new(doc);                    // edit.rs:3368 (takes ownership)
let opts = TextSearchOptions::default()                     // edit.rs:6486
    .with_case_insensitive(true);
let hits = session.find_text_with("total", &opts);          // edit.rs:11853 -> Vec<TextMatch>
for h in &hits {
    let _page = h.page_index;      // 0-based, SESSION page space
    let _quad = h.quad;            // annot_author::Quad, unrotated page space, y-UP
    let _text = &h.text;           // what was actually matched, not the needle
}
```

`TextMatch` — `edit.rs:6080`: `page_index`, `quad`, `text`. It needs
`&mut self` (an internal cache), so hold the session, not a `&Document`.

**★★ A ZERO MATCH COUNT IS NOT EVIDENCE THE NEEDLE IS ABSENT, and
`find_text_with` structurally cannot tell you so.** Two completely different
situations produce the identical empty `Vec<TextMatch>`:

1. the needle genuinely is not in the document; or
2. the document's text was **never recoverable as Unicode**, so no needle
   could ever have matched it.

Case 2 is not exotic, and its populations render *perfectly* — which is
exactly what makes it invisible. A **Type 3** font (ISO 32000-1 §9.6.5) draws
each glyph with a content stream named by an arbitrary `/CharProcs` key, so
`/g13` carries no Unicode meaning and §9.10.2 method 2's precondition is
false by construction: without a `/ToUnicode` CMap there is **no sourced route
to Unicode at all**. `Identity-H` with no `/ToUnicode` is the composite twin.
Acrobat is gated on the identical entry — this is parity, not a pdfcer
shortfall — and Acrobat's answer is to give up silently, which pdfcer's rule 4
forbids.

```rust
let found = session.search_text("total", &opts);            // edit.rs:16450
for h in &found.matches { /* ... same TextMatch as before ... */ }

let d = &found.diagnostics;                                 // TextDiagnostics
d.type3_fonts_without_to_unicode;   // Type 3 fonts with no /ToUnicode
d.identity_fonts_without_to_unicode;// Identity-H fonts with no /ToUnicode
d.ladder_failures;                  // per-CODE total, every cause
d.codes_total;                      // denominator for the above
```

**For a new GUI:** when a search returns nothing and any of those three is
non-zero, say so beside the result — *“no matches; N font(s) in this
document carry text that cannot be searched”* — rather than a bare
“0 results”. `pdfcer`'s `find-text` does exactly this: the counters ride
its machine-readable summary line (`unreadable_codes=`,
`type3_no_tounicode=`, `identity_no_tounicode=`) and the prose goes to
stderr. The disclosure belongs **off-canvas** (rule 4 as narrowed by
decision 059) — a status line or results panel, never a mark drawn into the
page view.

**★★ `find_text` and `find_text_with` have different default matching
semantics, and this has already caused a real defect.**
`EditSession::find_text(needle, case_insensitive)` (`edit.rs:11792`) passes
`with_wildcards(true)`: **`#` matches any ASCII digit and `?` matches any
single character.** `TextSearchOptions::default()` has `wildcards: false`.

The doc comment records what happened (`edit.rs:6498-6521`): pdfcer's own
Find bar ran through `find_text`, so *"typing `?` into it matched every
character on the page and nothing said why."* It was fixed in the **front
end**, not the function — `find_text`'s pattern behaviour is its documented
contract. The sibling verb `mark_redactions_by_search` matches **literally**,
so a Find bar on `find_text` highlights hits that a "redact every hit"
control then declines to mark.

**For a new GUI: use `find_text_with` with an explicit `TextSearchOptions`,
and expose wildcards as a visible toggle.** Never wire a search box to
`find_text`.

`TextSearchOptions` (`edit.rs:6486`) also carries `whole_word` and
`word_boundary` — the latter because ISO 32000-1 §14.8.2.5 NOTE 1 declines
to define "word" at all, so pdfcer exposes NOTE 4's own menu of strategies
as a setting rather than picking one (R169).

Case-insensitive matching is **ASCII-only and byte-offset preserving** by
design (`edit.rs:6487-6496`): lower-casing would shift byte offsets for
non-ASCII text and the offsets are what map a match back to its glyphs.

### 8.5a Render presets for the subset standards (PDF/X, PDF/A, PDF/UA)

`pdfcer_core::settings::presets`, shipped `Pass 128.1` (`1f79cc1`).

A preset is a **named bundle of values for settings that already exist**,
applied in one act and individually editable afterwards. It adds no rendering
mode, decides no conformance verdict, and validates nothing.

```rust
use pdfcer_core::settings::presets::{RenderPreset, RenderStandard};

let preset = RenderPreset::for_standard(RenderStandard::PdfX4);
let changed: Vec<_> = preset.apply(&mut settings);   // the keys it MOVED
for line in preset.disclosures() { /* show off-canvas */ }
```

**★★ EVERY ENTRY CARRIES ITS OWN EVIDENCE TIER, and that is the whole point.**
`Evidence::{Sourced, Implied, BestEffort, NotApplicable}`. A control labelled
`ISO 15930-7` carries that standard's authority whether or not you intended it
to, so the interesting column is not the value — it is how much weight the
value can bear. For PDF/X-4, **two of seven** entries are a claim about the
standard at all, and both are `Implied` rather than `Sourced`.

**★ Axis 7 — `PresetKey::SpotColorantDeviceModel` (`Pass 237.0`, asked by
pdfcer-gui 2026-09-02).** Every PDF/X level pins
`PresetAction::SpotModel(SimulateSeparations)` at tier `Implied`; every PDF/A
level and PDF/UA leave it alone (`Sourced` — ISO 19005's Scope excludes
rendering). This is the one axis pinned **without a clause that reaches it**:
no ISO 15930 part says a word about a device colorant model. It is pinned
anyway because the two values render visibly differently (a spot under an
overprinting white is preserved under one and knocked out under the other)
and a control labelled `ISO 15930-7` carries the expectation "show me what
the press will get" — leaving it alone would silently ship whatever global
override the operator last set into a view read as authoritative. The
inference is ISO 15930-1 §6.3.1 (print elements exchanged as *separation*
colour data for one printing condition ⇒ the target device carries the
separations ⇒ ISO 32000-1 §8.6.6.4 keeps the spot on that device). **Show
the entry's `why` beside the control** — it names the device, not the
setting, and ends *"No ISO 15930 clause requires this"*. Because the pinned
value is pdfcer's shipped default, `apply()` reports the key as changed only
when it corrected a stale global override. Spec corpus:
`pdfx__ref__conformance_and_rendering_axes.md` Axis 7.

**★ `PresetAction::LeaveAlone` is a real state and your UI needs it.** Roughly
a third of the grid is axes a standard does not reach — the complete clause
lists of ISO 15930-7 and -9 contain no shading clause at all, so no PDF/X part
reaches mesh padding. Render those rows differently from rows with values
(greyed, or "this standard does not specify"), **never blank**: a blank cell
reads as missing data. `RenderPreset::left_alone()` gives you the keys and each
entry carries a `why`.

**Three things to surface, all from `disclosures()`:**

1. Applying a preset **does not make a file conformant and does not check
   whether it is.**
2. PDF/X itself concedes more than one conforming rendering may exist, and its
   stated remedy is embedded **job ticket** data pdfcer does not read.
3. Every PDF/X and PDF/A level guarantees a **colorimetric** device-colour
   definition that pdfcer does not apply — `CmykIntent` picks among fixed
   built-in tables and is not an ICC path. That is a capability gap, not a
   mis-set value, and it is invisible by construction: a colour transform that
   did not happen leaves nothing on screen.

**`RenderStandard::PdfUa1` sets nothing, and that is the sourced answer** —
measured at zero hits for nine rendering terms across all 197 veraPDF PDF/UA
rules. Surface it rather than hiding it; an absent entry reads as unfinished.

`PresetAction::value_string()` formats a value for display. Use it rather than
matching — the type is `#[non_exhaustive]`, so your `match` needs a wildcard,
and that wildcard silently prints a future variant as the fallback.

### 8.6 Text strings (`/Title`, `/Author`, bookmark labels)

```rust
use pdfcer_core::textstring::{decode_text_string, DecodedText, TextStringForm};

let d: DecodedText = decode_text_string(bytes);   // textstring.rs:363, infallible
// d.text: String, d.form: TextStringForm (PdfDocEncoding | Utf16Be), plus flags
```

Also: `decode_utf16be_bytes` `:437`, `encode_text_string` `:566`,
`pdf_doc_char(u8) -> Option<char>` `:268`.

**Never `String::from_utf8` a PDF string.** §7.9.2 strings are
PDFDocEncoding by default and UTF-16BE when they carry a BOM. This function
is the only correct decoder.

**Naming trap:** there is a *second* `decode_text_string` at `edit.rs:5470`
returning an `InfoText` — a different type for the `/Info`-dictionary path.
Import explicitly and check which you have.

### 8.7 `text_state` — ambient text-state tracking

`text_state.rs`. `TextStateParam` `:143`, `TextStateParams` `:300`,
`AmbientTextState` `:634`, `AmbientValue` `:463`, `AmbientOrigin` `:391`,
`AmbientRestoreError` `:440`.

You need this only if you are building text *editing* on top of extraction
(part 2's territory). The read-side relevance is one trap:

**★ `AmbientValue::value` for `HorizScale` is the raw `Tz` percentage
(e.g. `90.0`); `TextStateParams::h_scale` for the same parameter is the
ratio (`0.9`).** `text_state.rs:306` vs `:465`. Mixing them scales advances
by 100×.

`AmbientOrigin::Unobservable` means the value is known but a byte-faithful
restore must be **refused, never guessed** — that refusal is
`AmbientRestoreError`, not a silent default (`text_state.rs:64-70`).

### 8.8 Traps — text extraction

- **T-8.1 `ExtractedGlyph::text_len` is not 1.** `mod.rs:417-420`: *"**Not
  one.** One code may produce many code points — §9.10.3's own example
  decomposes `ffl` from a single code."* Slice with
  `[text_start .. text_start+text_len]`.
- **T-8.2 `ActualText` runs have NO glyphs, by design.** `mod.rs:168-175`:
  §14.9.4 N4 records no length relationship between replacement and replaced
  content, so character-level mapping back to glyph positions is
  *"**impossible**, not merely unimplemented."* Highlight such a run at
  `bbox` granularity or not at all.
- **T-8.3 Artifact runs are ALWAYS in `PageText::runs`.** `mod.rs:470-476`:
  `include_artifacts` filters only the `plain_text()`/`sourced_text()`
  *accessors*. Iterate `runs` directly and you will leak watermarks and
  running heads into your UI. Check `run.artifact`.
- **T-8.4 `origin.is_sourced() == true` ≠ every character is trustworthy.**
  `mod.rs:161-163`: a `Glyphs` run may still contain U+FFFD from
  `LadderRung::Failed`. Per-character confidence is `ExtractedGlyph::rung`.
- **T-8.5 Page separator is U+000C.** `mod.rs:991-998`.
- **T-8.6 `capture_provenance` defaults to `false`.** `mod.rs:769`.
  `provenance.unwrap()` panics without it.
- **T-8.7 `include_artifacts` is captured at extraction time** and is
  private on both `PageText` and `ExtractedText` (`mod.rs:511-518`,
  `:530-532`). Changing the policy means re-extracting.
- **T-8.8 Plural extract swallows per-page failures; singular does not.**
  §8.1. Check `TextDiagnostics::pages_unreadable`.
- **T-8.9 `unmappable_code` changes the sentinel, never the count.**
  `mod.rs:779-782`: `TextDiagnostics::ladder_failures` counts every failure
  regardless. Do not infer "no failures" from the absence of U+FFFD.
- **T-8.10 `Tw` (word spacing) is spec-void on composite 2-byte runs**
  (§9.3.3). `GlyphProvenance::composite` tells you per-run
  (`mod.rs:387-390`).
- **T-8.11 `TextDiagnostics::via_cid_collection` is always zero this Pass**
  (`mod.rs:551-553`). Do not build a feature that depends on it firing.

`TextDiagnostics` (`mod.rs:544`) carries ~30 honesty counters plus
`notes: Vec<String>`. It is the read-side embodiment of project rule 4 —
if you show extracted text, show the diagnostics too, or at least a pip
when they are non-zero.

### 8.9 Stability

`textstring.rs` is frozen (initial commit only). `text_extract/layout.rs`
likewise. `mod.rs`, `font.rs`, `page.rs` and `fontinfo.rs` are the
**highest-churn** files in the crate's read side (most recent: `6d63d81`,
2026-08-08). Expect *additive* change — `mod.rs:817-822` explains the
`#[non_exhaustive]`-plus-builder pattern exists precisely so new fields do
not break callers. `text_state.rs` is young (introduced Pass 19.0, two
commits).

---

## 9. Fonts

**Module set:** `fontinfo`, `fontdata`, `text_extract::font`,
`text_extract::cmap`.

Two different jobs live here. `fontinfo` answers *"what fonts does this
document use, and what may I do with them?"* — a document-level inventory
for a Fonts panel. `text_extract::font` + `cmap` answer *"how do I turn
this font's character codes into text?"* — per-resource decoding.

### 9.1 Document font inventory

```rust
use pdfcer_core::fontinfo::{self, Removability};

let inv = fontinfo::inventory(&doc.view());     // fontinfo.rs:1601 — INFALLIBLE, no Result
for f in &inv.fonts {                            // Vec<FontRecord>, first-discovery order
    // FontRecord: fontinfo.rs:1209
    let embedded = matches!(f.program, fontinfo::Program::Embedded(_));
    let pages = fontinfo::format_page_ranges(&f.pages);    // fontinfo.rs:1416 -> "1-3, 7"
}
println!("{} embedded, {} bytes", inv.embedded_count(), inv.embedded_bytes()); // :1514, :1528
println!("not walked: {:?}", inv.coverage.not_walked());                        // :1183
```

`FontInventory{fonts, coverage, diagnostics}` — `fontinfo.rs:1501`.
`Program` — `:494`: `NotEmbedded` | `Unreadable{key, why}` | `Embedded(EmbeddedProgram)`.
`Removability` — `:866`, `RemovabilityUnknown` — `:902`.
`SurfaceCoverage` — `:1104` with `includes` `:1139`, `walked` `:1154`,
`not_walked` `:1183`.

Embedding permission from an embedded program's `OS/2` table:

```rust
let bits = fontinfo::read_fs_type(program_bytes)?;   // fontinfo.rs:744 -> FsTypeBits
```

`FsType` `:658`, `FsTypeBits` `:623`, `EmbeddingPermission` `:577`,
`FsTypeError` `:544`.

Subset tags: `split_subset_tag` `:1320`; standard-14 test `is_standard_14`
`:1370`.

Guards: `MAX_RESOURCE_NODES` `:177`, `MAX_FONTS` `:184`,
`MAX_RESOURCE_NAMES_PER_FONT` `:192`, `MAX_SFNT_TABLES` `:200`.

### 9.2 Per-resource decoding font

```rust
use pdfcer_core::text_extract::ExtractFont;

let font = ExtractFont::resolve(&doc.view(), &font_dict);  // font.rs:381 — INFALLIBLE
let composite = !font.is_simple();                          // font.rs:800
if let Some(cmap) = font.to_unicode_cmap() { /* :369 */ }
```

Only `base_font: String` and `notes: Vec<FontNote>` are public fields
(`font.rs:224`). `LadderRung` (`font.rs:97`) is the §9.10.2 decoding
ladder: `ToUnicode` | `EncodingAgl` | `CidCollection` | `GlyphNameExtension`
| `Failed`. `Rung3Gap` `:152`, `FontNote` `:178`. All four re-exported at
`text_extract::` (`mod.rs:147`).

### 9.3 `/ToUnicode` CMaps

```rust
use pdfcer_core::text_extract::cmap::ToUnicodeCMap;

let cmap = ToUnicodeCMap::parse(bytes);              // cmap.rs:272 — INFALLIBLE
let s: Option<String> = cmap.lookup(code);            // cmap.rs:552
let stats = cmap.stats();                             // cmap.rs:698 -> CMapStats (:202)
```

Guards: `MAX_BF_ENTRIES` 500_000 `:103`, `MAX_BF_RANGES` 100_000 `:110`,
`MAX_DST_BYTES` 512 (**spec-stated**) `:119`, `MAX_CMAP_TOKENS` 10_000_000
`:128`.

### 9.4 Base-14 metrics without a font file

`fontdata` is compiled-in metrics only — `pdfcer-core` contains **no font
program parser** (rule R21; that lives in `pdfcer-render`).

`Std14` `:179` with `Std14::ALL` `:230` · `std14_by_base_font` `:271` ·
`std14_base_font_name` `:317` · `std14_width` `:382` ·
`Std14Descriptor` `:451` · `std14_descriptor` `:489` ·
`BaseEncoding` `:502` · `encoding_glyph_name` `:546` ·
`glyph_name_to_unicode` `:585` · `glyph_name_to_unicode_string` `:739` ·
`is_standard_latin_or_symbol_name` `:676` · `std14_builtin_encoding` `:830`.

`fontdata::tables` is **private**; its contents are `pub(crate)`.

**Units:** `std14_width` returns **glyph space, 1/1000 em** (`u16`), and
`Std14Descriptor`'s `font_bbox`/`ascender`/`descender` are the same
(`fontdata/mod.rs:452-469`). Multiply by `font_size / 1000.0` to get text
space.

### 9.5 Traps — fonts

- **T-9.1 `FsType::permission()` returning `None` is NOT "permissive".**
  `fontinfo.rs:558-567`, `:674-684`: an absent `OS/2` table, a `ttcf`
  collection, or a decode failure all give `None`, and the spec defines
  **no default** for the absent case. Treating `None` as unrestricted is
  exactly the bug this API is shaped to prevent.
- **T-9.2 `EmbeddingPermission` is a value, not a bitmask.**
  `fontinfo.rs:569-574`: `0` is the *most* permissive (Installable). Never
  test `fsType != 0` for "restricted".
- **T-9.3 A subset tag is EXACTLY six uppercase letters.**
  `fontinfo.rs:1296-1319`: `"ABCDE+Arial"` (five) and `"AbCdEf+Arial"`
  (mixed case) are not tagged — the whole string is the family name.
- **T-9.4 `FontRecord::pages` empty ≠ unused.** `fontinfo.rs:1249-1251`: a
  font reached only through the AcroForm `/DR` has no page list but is a
  live form-default font.
- **T-9.5 `glyph_name_to_unicode` (char) silently drops ligatures.**
  `fontdata/mod.rs:596-603`, `:685-717`: it returns `None` for `f_i` and
  multi-group `uni` names. **For extraction use
  `glyph_name_to_unicode_string`**; the `char` form is the rendering-side
  convenience and will lose text if misused.
- **T-9.6 `ToUnicodeCMap::lookup` returning `None` means "this CMap does
  not cover this code", not "no character".** `cmap.rs:540-546`: the
  fallthrough to rung 2 happens one level up in `ExtractFont`. Using
  `ToUnicodeCMap` directly means implementing the ladder yourself.
- **T-9.7 `ToUnicodeCMap::injective_inverse()` is O(entries) and can
  refuse.** `cmap.rs:615-676`: it materialises up to `MAX_BF_ENTRIES` and
  returns `Err(NotInjective::TooLarge)` past that. Never call it per glyph.
- **T-9.8 `fontinfo::inventory` and `ExtractFont::resolve` and
  `ToUnicodeCMap::parse` are all infallible.** They report problems in
  `notes`/`diagnostics`, not `Result`. An empty error path does not mean a
  clean document — read the notes.

### 9.6 Stability

`fontdata/{mod,tables}.rs` frozen (initial commit). `fontinfo.rs` is high
churn (`d2f1ed3`, 2026-08-11). `font.rs` active — `is_simple()` was made
`pub` recently (Pass 19.0). `cmap.rs` three commits, feature-driven
(`injective_inverse` added Pass 21.1).

---

## 10. Vector geometry, hit-testing, snapping

**Module set:** `vector` (read/query half: `decompose`, `geometry`, `hit`,
`snap`, `linepick`, `centerline`). `vector::edit` is part 2.

This is the subsystem a canvas needs for selection, highlighting and
CAD-style measurement.

### 10.1 Decomposing a page into selectable objects

```rust
use pdfcer_core::page_tree;
use pdfcer_core::vector::{decompose_page, Matrix, PageObjects, VectorObject};

let page = &page_tree::pages(&doc)?[0];
// ★ Matrix::IDENTITY gives geometry in genuine PDF default user space.
let model: PageObjects = decompose_page(&doc.view(), page, Matrix::IDENTITY)?; // decompose.rs:1293
for obj in &model.objects {                    // paint order, back to front
    let bbox = obj.page_bbox();                 // decompose.rs:864 — page space
}
let _ = model.diagnostics;                      // DecomposeDiagnostics, decompose.rs:899
```

Lower-level forms if you already have a `ContentStream`:
`decompose(&cs, initial, &dyn XObjectResolver)` — `decompose.rs:1329`
(geometry only, `NoFonts`) and `decompose_with_fonts(&cs, initial,
&dyn XObjectResolver, &dyn FontResolver)` — `decompose.rs:1371` (the true
entry point). Resolvers: `NoXObjects` `:996` / `DocumentXObjects` `:1019`;
`NoFonts` `:1169` / `DocumentFonts::new` `:1211`.

`VectorObject` — `decompose.rs:851`: `Path(PathObject)` | `Text(TextObject)`
| `Image(ImageObject)`.

**`obj.oc() -> Option<ObjId>`** and the `oc` field on all three object types
(and `FormLeaf::oc()`) give the **optional-content group (layer)** the object
was painted under — a `BDC /OC /Pn` section (§8.11.3.2) or an XObject's own
`/OC` (§8.11.3.3), `Pass 250.0` (`pdfcer-gui` request 2026-09-04). This is what
connects a canvas selection to a Layers-panel row. Three contract points:

- **Membership, not visibility.** It never resolves whether the layer is on/off
  (that needs `/OCProperties`, which this walk does not hold); a shell keeps the
  visibility side itself. An OCMD is reported as its own `ObjId`, never expanded.
- **`None` means "on no layer", NOT "could not tell".** A `BDC /OC` whose `/Pn`
  did not resolve is counted in `DecomposeDiagnostics::oc_unresolved` and its
  object still reports `oc == None` — read the counter to tell the two apart.
- **No default is substituted** — an object under no `/OC` section is `None`,
  never the document's first OCG. `FormLeaf::oc()` delegates to the wrapped
  object (a page-level `BDC /OC` around the form's `Do` is not folded in — a
  documented partial for that nested case).

### 10.2 The object types

**`PathObject`** — `decompose.rs:343`.
`subpaths: Vec<Subpath>` `:346` is **user space**; `page_subpaths()` `:375`
maps them through `ctm` `:350` to **page space**. `style: PaintStyle` `:352`,
`line_width: f64` `:355` (**user space**), `fill_color`/`stroke_color: Rgb`
`:357`/`:359`, `tokens: TokenRange` `:361`, `bytes: ByteSpan` `:363`,
`page_bbox: Bounds` `:367` (page space, control-point hull).

`Subpath` — `:225`: `{start, segments, closed, tokens, starts_implicitly}`;
`anchors()` `:280` yields on-curve points only.
`Segment` — `:179`: `Line{to}` | `Cubic{c1, c2, to}` — control points
**already resolved** (see T-10.2).

**`TextObject`** — `decompose.rs:649`. `page_bbox` `:652` (approximate),
`runs: Vec<TextRun>` `:690` (per-show-op boxes), `approximate: bool` `:698`
(**always `true`**), `bounds_basis: TextBoundsBasis` `:700`, `preview` `:702`,
`font: Option<TextFont>` `:704`.

`TextBoundsBasis` — `:557`: `FontMetrics` | `MetricAdvancesNominalHeight` |
`EstimatedAdvances` | `EmBox`. Four bases, not two, deliberately — a Type 3
or descriptor-less CIDFont has real advances but a guessed height, and
collapsing that into `FontMetrics` would misrepresent confidence
(`ARCHITECTURE.md` §4, Pass 18.6). **Show the basis if you show the box.**

**`ImageObject`** — `decompose.rs:411`: `{ctm, page_bbox, source, pixel_size,
tokens, bytes}`. `ImageSource` `:395`: `Inline` | `XObject` | `Form`.

**`Bounds`** — `geometry.rs:259`: `{min, max: Point}`, with `EMPTY` `:271`,
`union_point` `:293`, `union` `:305`, `inflate` `:313`, `contains` `:325`,
`contained_by` `:338`, `intersects` `:350`.
**`Point`** — `geometry.rs:52`: `{x, y: f64}`.
**`Matrix`** — `geometry.rs:98`: PDF row-vector affine `{a,b,c,d,e,f}`, with
`IDENTITY` `:117`, `map_point` `:149`, `map_vector` `:202`, `post_concat`
`:167`, `inverse -> Option<Matrix>` `:226`, `determinant` `:182`.

### 10.3 Hit-testing

```rust
use pdfcer_core::vector::{hit_test_point, hit_test_point_all, hit_test_rect,
                         hit_test_subpaths, hit_test_text_runs,
                         subpath_bounds, MarqueeMode, Point, Bounds};

// ★ tolerance is PAGE space. Convert your screen pixels first.
let tol = screen_px_tolerance / zoom;
let at = Point::new(page_x, page_y);

let top: Option<usize>  = hit_test_point(&model, at, tol);        // hit.rs:126
let all: Vec<usize>     = hit_test_point_all(&model, at, tol);    // hit.rs:174  (topmost first)
let marquee: Vec<usize> = hit_test_rect(&model, rect, MarqueeMode::Enclosed); // hit.rs:181

// Drill down inside one object:
let runs: Vec<usize>     = hit_test_text_runs(&model, obj_idx, at, tol);  // hit.rs:277
let subpaths: Vec<usize> = hit_test_subpaths(&model, obj_idx, at, tol);   // hit.rs:340
let b: Option<Bounds>    = subpath_bounds(&model, obj_idx, subpath_idx);  // hit.rs:392
```

`hit_test_point` is defined as the head of `hit_test_point_all` — one
private iterator underneath both, so they cannot disagree (`hit.rs:39-50`,
`ARCHITECTURE.md` §4 continuation-60). Use `hit_test_point_all` for alt-click
cycling; never reimplement either.

#### ★★★ For a click, use `hit_test_point_deep`. The others cannot see inside a form.

```rust
use pdfcer_core::vector::{hit_test_point_deep, HitTarget};

match hit_test_point_deep(&model, at, tol).first() {              // hit.rs:255
    Some(HitTarget::Object(i)) => { /* model.objects[*i] -- editable */ }
    Some(HitTarget::Leaf(i))   => { /* model.leaves[*i]  -- read-only  */ }
    None => { /* nothing drawn here */ }
}
```

**`hit_test_point` treats a form XObject as its bounding box, so on a page
whose body is wrapped in a form it answers with the wrapper no matter where
you click.** That is what the operator hit: *"when I click on one of the
objects all I get is the page selected."* He was selecting a real object.

The bbox rule is right for a **raster image**, whose quad genuinely is its ink.
It is wrong for a **form**, whose `/BBox` is a §8.10.1 clipping-**extent**
declaration that says nothing about coverage — a form declaring the whole
MediaBox and drawing one small line is legal and common. So
`hit_test_point_deep` **excludes forms outright** and answers with what is
drawn inside them. A click on empty space inside a form's bbox returns nothing.

The form is still reachable: `FormLeaf::containment` names every enclosing
form, so "select the container" is available as a **deliberate second act**,
which is a different thing from winning by default.


#### ★★ For a MARQUEE, use `hit_test_rect_deep`. `hit_test_rect` is shallow.

```rust
use pdfcer_core::vector::{hit_test_rect_deep, FormMarquee, HitTarget, MarqueeMode};

// Paint order, front-most LAST -- see the ordering note below.
let picked: Vec<HitTarget> =
    hit_test_rect_deep(&model, rect, MarqueeMode::Enclosed, FormMarquee::Exclude);
```

`hit_test_rect` filters `PageObjects::objects` only, so a rubber band across
an object drawn inside a form selects **nothing** while a click on the
identical object selects it. Two gestures that both mean *"select this"*,
disagreeing about what is selectable, is an inconsistency an operator meets in
the first minute — so if you have adopted `hit_test_point_deep`, adopt this in
the same change.

**★ THE ORDER IS THE OPPOSITE OF THE POINT QUERY'S, AND THAT IS DELIBERATE.**
`hit_test_point_deep` returns **topmost first**, because it answers *"which
one?"* and the winner belongs at the head. `hit_test_rect_deep` returns
**paint order, front-most last**, because it answers *"which ones?"* and a
caller iterating them to draw handles, group them or re-emit them wants paint
order. Reversing at your call site is one line; guessing which order a `Vec`
is in is a bug.

##### `FormMarquee` — both readings ship, and the default is `Exclude`

| variant | a form XObject is… |
|---|---|
| `Exclude` *(default)* | never selected; only what is drawn inside it |
| `Include` | selected on its own terms, **alongside** its leaves |

For a *point*, excluding forms needs no argument: a `/BBox` is a clipping
extent, so a point inside it is not evidence the operator aimed at the form.
For a *rect* the case is genuinely weaker — fully enclosing a rectangle **is**
a deliberate statement about that rectangle, and a form is a legitimate
operand.

**The tie-breaker is not which reading is better supported.** It is that a
click can *never* yield a form. If a marquee can, the operator acquires — by
one gesture and not the other — a selection that every edit verb then refuses.
**A capability reachable only by accident is a trap, not a feature.**

**★ `Include` is NOT a route back to `hit_test_rect`.** It returns the form
**and** its leaves; the shallow query returns the container alone. A caller
migrating between them is changing two things, and the leaf half is the one
that will surprise it.

#### The line picker also reaches inside forms, and its result says which list

```rust
use pdfcer_core::vector::HitTarget;
use pdfcer_core::vector::linepick::pick_line_in_page;

if let Some(line) = pick_line_in_page(&model, at, tol) {
    match line.target {
        HitTarget::Object(i) => { /* model.objects[i] */ }
        HitTarget::Leaf(i)   => { /* model.leaves[i]  */ }
    }
}
```

Both lists are searched and the nearest straight segment wins, regardless of
which list it came from. A form is never a candidate — only a `PathObject`
reaches the picker at all, so unlike the point and rect queries there is no
`FormMarquee` analogue to choose: there is no defensible reading under which a
`/BBox` edge is a line the operator drew.

**Nothing here is gated on `FormLeaf::is_editable()`, and that is correct.** A
ce dimension placed against a line inside a form is a **new annotation on the
page**, not a change to the form. You still need `target` — to report which
list the line came from, and to re-resolve it after an edit.

Two lower-level entry points exist if you already hold the path:
`hit_test_subpaths_of(&PathObject, Point, f64)` and
`pick_line_of(&PathObject, HitTarget, Point, f64)`. Both take the object
rather than an index, because **the geometry never needed the index; only the
lookup did** — and an index-based API is structurally incapable of naming a
leaf.

##### Headless equivalents

`pdfcer object-list` mirrors all three, so a script can reproduce what a
click, a marquee or a measure pick would resolve to without a window:

```
object-list <pdf> --page N --hit X,Y [--all-hits] [--hit-scope deep|page]
object-list <pdf> --page N --line-pick X,Y [--tolerance T]
```

`--hit` is **deep by default** since `Pass 138.0`; `--hit-scope page` restores
the old shallow query. A form leaf is reported as `leaf=N containment=…
paint_order=… in_form_index=… placement=… editable=0|1` with `kind=leaf:…`,
**never** as `index=N` — `--object` writes to the *page's* stream, so a leaf
ordinal under that key would be in range and would corrupt the page.

★ `editable=` was a hard-coded `false` until `Pass 188.0` and is now the leaf's
real answer. `subpath-move` and `node-move` take **`--leaf N`** as the
alternative to `--object N`; the two are mutually exclusive and passing both or
neither is refused by name.

#### `PageObjects::leaves` — and why it is a second list

`decompose_page` descends into every reachable form and returns the objects
inside on `PageObjects::leaves`. Each `FormLeaf` carries:

| field / method | meaning |
|---|---|
| `object` | the object, geometry already in **page space** — one hit test serves both lists |
| `containment` | enclosing forms, **outermost first**; never empty |
| `paint_order` | index in `objects` of the **outermost** enclosing form |
| `stream()` | `ContentStreamRef::Form { object }` — **which buffer** its token range indexes |
| `placement` | the CTM at the enclosing form's `Do`, composed with its `/Matrix` and every outer form's placement (`Pass 188.0`) |
| `form_object_index` | this object's index in its **own form's** decomposition — what a form-scoped verb addresses (`Pass 188.0`) |
| `is_editable()` | `true` for a **path**. It was a hard `false` until `Pass 188.0`; it now answers about the **object**, not about whether the feature exists |

**★★ It is a separate list for a safety reason, not a stylistic one.** Eleven
call sites in `edit.rs` resolve a paint-order index and apply content-stream
surgery **to the page's stream**. A leaf's token range indexes the **form's**
stream — a different buffer, and an *in-range* one. A leaf in `objects` would
be handed to those verbs and corrupt the page silently. Keeping the lists apart
makes them correct by construction, and means **your stored paint-order indices
do not move**.

⇒ For **selection**, use the deep test. **★ For editing, use the deep test too,
since `Pass 188.0`** — this line used to say *"use `hit_test_point` and you get
back something you can actually edit"*, which was true while nothing inside a
form was editable and is now advice that throws away the reach.

A leaf is edited through the **form-scoped** verbs (`move_node_in_form`,
`move_nodes_in_form`, `move_handle_in_form`, `move_subpath_in_form`,
`move_objects_in_form`, `delete_objects_in_form`), addressed by its index in
`leaves` and taking **page-space** coordinates exactly as the page verbs do.
`02-editing-and-saving.md` §1.10.1 has the contract, including the one thing a
shell must show the operator: a form has one set of bytes, so the edit reaches
every place that form is drawn, and `FormSurgeryOutcome` says how many.

The safety property above is **unchanged** — leaves are still absent from
`objects`, and the form verbs write to the form's stream, never the page's.
What changed is only that the second list now has verbs of its own.

**★ Ordering is an interleave, not a concatenation.** Leaves and page objects
are two lists but **one paint order**: a form's contents are painted where its
`Do` sits, so something drawn after a form is on top of everything inside it.
`hit_test_point_deep` interleaves on `paint_order`. If you build your own
ordering, do the same — "leaves first" and "leaves last" are both wrong on any
page that draws anything outside its forms.

**Vocabulary note.** `FormLeaf::stream()` and `is_editable()` are deliberately
the **same** `ContentStreamRef` / `is_editable` pair `text_extract` uses for a
`TextRun` inside a form. A form-interior path and a form-interior text run
describe themselves identically, so one selection model covers both.

**Guards, and their disclosure.** Nesting is bounded by
`content::MAX_FORM_DEPTH` (64 — corpus-corrected: veraPDF ships a *conformant*
32-deep chain), and cycles are caught by a guard keyed on the form's **object
number**, because the same stream is reachable under different resource names
and a name-keyed guard misses the cycle. Both are counted on
`DecomposeDiagnostics::{form_depth_overflows, form_cycles}` — **non-zero means
the leaf list is incomplete**, and presenting it as "everything on the page"
would be wrong.

`MarqueeMode` — `hit.rs:82`: `Enclosed` | `Touched`.
`FLATTEN_STEPS` = 16 — `hit.rs:78` (Bézier flattening for hit-testing).

All of the above are re-exported flat at `pdfcer_core::vector::*`
(`vector/mod.rs:81-84`) — verified directly.

### 10.4 Snapping

```rust
use pdfcer_core::vector::{snap_candidates, SnapConfig, SnapKind, SnapCandidate,
                         AxisConstraint, constrained_second_point, measured_length};

let cfg = SnapConfig::new(tol_in_page_units)      // snap.rs:291
    .with_intersections(true)                      // snap.rs:303 — default FALSE, costs perf
    .with_grid(grid)                               // snap.rs:310
    .with_axes(true);                              // snap.rs:317
let cands: Vec<SnapCandidate> = snap_candidates(query_point, &cfg, &model); // snap.rs:449
// SnapCandidate: snap.rs:248 — {point (page space), kind, source_object}
// SnapKind: snap.rs:154, 8 variants; priority() :216 (0 = highest);
//           is_derived() :234 — TRUE only for DerivedCenterline.

// Axis constraint for a second pick (Shift-drag):
let p2 = constrained_second_point(first, raw_second, AxisConstraint::Horizontal); // snap.rs:385
let len = measured_length(first, p2, AxisConstraint::Horizontal);                  // snap.rs:405
```

Guards: `SNAP_FLATTEN_STEPS` 16 `:120`, `MAX_NEIGHBOURHOOD_SEGMENTS` 256
`:130`, `MAX_CANDIDATES` 4096 `:136`.

`SnapKind::is_derived()` is your rule-4 hook: a `DerivedCenterline`
candidate is something pdfcer **inferred** — there is no such line in the
file, pdfcer worked it out from two edges — so the operator has to be able to
tell it apart from a real edge. The API hands you the flag; the disclosure is
your shell's job.

**Disclose it in the snap INDICATOR, not in the placed geometry** (decision
059). A snap indicator is a *pre-commit affordance* — it is the
cursor, describing what is about to happen — so distinguishing a derived
candidate there is exactly right. Once the point is placed, the resulting
geometry renders like any other: **no residual marking on applied content**.
See `03-capabilities.md`'s rule-4 block for why that line is drawn where it is.

Related: `centerline::page_candidates(&model)` (`centerline.rs:69`) and
`derive_from_path(index, &path)` (`:91`), with
`CENTERLINE_ASPECT_THRESHOLD` = 8.0 (`:34`) and `CenterlineCandidate`
(`:43`).

### 10.5 Line picking (CAD measurement)

```rust
use pdfcer_core::vector::linepick::{pick_line_in_page, pick_line, classify_two_lines,
                                   measured_angle_degrees, ParallelPolicy,
                                   PickedLine, TwoLineRelation};

let a: Option<PickedLine> = pick_line_in_page(&model, at, tol);   // linepick.rs:344
let b = pick_line_in_page(&model, at2, tol);
match classify_two_lines(&a?, &b?, ParallelPolicy::default()) {    // linepick.rs:392
    Some(TwoLineRelation::Parallel { distance }) => {}
    Some(TwoLineRelation::Collinear) => {}
    Some(TwoLineRelation::Angled { degrees, apex, apex_is_real }) => {}
    None => {}
}
```

**★ `linepick` is NOT re-exported at `pdfcer_core::vector::*`.** Verified
against `vector/mod.rs:65-88`: there is no `pub use linepick::{…}` block,
unlike `centerline`, `decompose`, `edit`, `geometry`, `hit` and `snap`.
Reach it as `pdfcer_core::vector::linepick::…`. (`pub mod linepick;` is at
`vector/mod.rs:60`.) This is consistent with it being the newest module
(2026-08-12) and is the kind of thing that may change — do not assume the
flat path will keep failing, and do not assume it works.

`PickedLine` — `linepick.rs:48`: `{target, subpath, segment, start, end,
pick}`; `page_object_index()`, `direction()`, `length()`.

**★★★ BREAKING, `Pass 138.0` (2026-08-27): the first field was
`object_index: usize` and is now `target: HitTarget`.** Code written against
v0.14.0 will not compile, and that is deliberate rather than incidental.

A `usize` can only name an entry in `PageObjects::objects`. It cannot name a
`FormLeaf`, so **the old signature made an answer about form contents
unrepresentable** — which is why `pick_line_in_page` returned `None` on every
page whose drawing lives inside a form XObject, i.e. most CAD exports.
Measured on one: **129,758 page objects, one form, 10,256 leaves**, every one
of them a candidate line and every one invisible. The tool was not degraded
there; it was inert.

Migration: `page_object_index() -> Option<usize>` gives you the old value
where one exists. **It is an `Option`, not a sentinel**, on purpose — a leaf
ordinal handed to something expecting a page index is a number that is *in
range and wrong*, which is the worst failure available. If you `unwrap()` it,
you are stating in one visible place that you do not handle form contents.
`ParallelPolicy` — `:111`: `{epsilon_degrees, force_parallel}`, with
`default` `:153`, `from_setting` `:169`, `forcing_parallel` `:178`.
`measured_angle_degrees` `:196` returns the raw angle folded to `[0, 90]`.

### 10.6 ★ Coordinate space table — `vector` read side

| Function / field | Input space & units | Output space & units | Evidence |
|---|---|---|---|
| `decompose*`'s `initial: Matrix` | caller's starting CTM (`IDENTITY` ⇒ page space) | — | `decompose.rs:1296-1298` |
| `PathObject::subpaths` | — | **user space**, `f64` | `decompose.rs:344-345` |
| `PathObject::page_subpaths()` | user space via `ctm` | **page space**, `f64` | `decompose.rs:370-374` |
| `*::page_bbox`, `TextRun::bounds` | — | **page space** `Bounds`, `f64` | `decompose.rs:364-367, 415, 650-652, 796-797` |
| `PathObject::line_width` | **user space** points | — (scaled by `√\|det(ctm)\|` at hit time) | `decompose.rs:353-355`; `hit.rs:474-498` |
| `TextFont::size` | **text space** — raw `Tf` operand, unscaled | — | `decompose.rs:471-481` |
| `ImageObject::pixel_size` | — | **sample count**, not a page size | `decompose.rs:418-435` |
| `hit_test_point/_all/_rect` point/rect | **page space**, `f64` | index(es) | `hit.rs:115-120, 178-181` |
| every `tolerance` argument | **page-space distance** | — | `hit.rs:118-120` |
| `hit_test_text_runs`/`_subpaths` | page space / page distance | `Vec<usize>` nearest-first | `hit.rs:273-275, 334-336` |
| `subpath_bounds` | — | **page space** | `hit.rs:383-392` |
| `snap_candidates` query & `SnapCandidate::point` | **page space** | **page space** | `snap.rs:3-6, 249-250` |
| `SnapConfig::tolerance` | **page-space** catch radius | — | `snap.rs:87-95, 270-273` |
| `constrained_second_point`, `measured_length` | page space | page space / page-space length | `snap.rs:328-343` |
| `CenterlineCandidate::{start,end}` | — | **page space** | `centerline.rs:38-41` |
| `PickedLine::{start,end,pick}` | — | **page space** | `linepick.rs:42-44`; built from `page_subpaths()` at `linepick.rs:266` |
| `TwoLineRelation::Angled{apex}` | — | **page space** | `linepick.rs:228-238` |

Everything is `f64` except `Rgb` (`f32`, `geometry.rs:372`).
`geometry.rs:47-50`: *"Values are `f64` … narrowing to `f32` only at the
render/GUI boundary."*

### 10.7 Traps — vector

- **★ T-10.1 (THE tolerance trap) Every `tolerance` / `SnapConfig::tolerance`
  is PAGE space, and nothing in core checks it.** `hit.rs:118-120`:
  *"`tolerance` is a page-space slack (the GUI converts a few screen pixels
  into page units and passes it here)."* Pass raw screen pixels and your
  hit-testing silently gets more forgiving as the user zooms out and
  unusably tight as they zoom in. The existing shell converts at the call
  site (`pdfce@cce414e:crates/pdfce-gui/src/canvas.rs`'s `screen_tolerance_to_page`); a new shell
  must implement the same conversion itself.
- **T-10.2 `v` and `y` operators have implicit control points.**
  `geometry.rs:429-440`: `cubic_from_v`'s *"first control point is the
  current point — the classic 'v/y trap' that silently mis-renders if
  forgotten"*; `cubic_from_y`'s *"second control point is the endpoint."*
  You avoid this entirely by reading `Segment::Cubic{c1,c2,to}`, which is
  already resolved. Only re-deriving from raw operands re-opens it.
- **T-10.3 Use `Matrix::map_vector` for deltas, `map_point` for
  positions.** `geometry.rs:186-200`: `map_point` on a delta folds in the
  CTM's translation and *"would shove the object across the page."*
- **T-10.4 Hit-test text per RUN, not per object bbox.** `hit.rs:200-240`,
  commit `627c807`: a CAD sheet can have one text object holding 237
  dimension labels, whose bbox *"at one point over a real line beat 57
  genuine objects underneath it."* Use `hit_test_text_runs` / `TextObject::runs`.
- **T-10.5 The drill-down queries return EMPTY on a bad index; they do not
  fall back.** `hit.rs:263-272`, `:331-338`. The top-level point query
  *does* fall back to `page_bbox` when `runs` is empty. Do not assume
  matching behaviour.
- **T-10.6 `pick_line*` skips curves entirely — it never chords them.**
  `linepick.rs:241-247`: *"A Bézier is deliberately NOT approximated by its
  chord: dimensioning 'the line' of a curve would measure something the
  drawing does not contain."* A click near a curve returns `None`.
- **T-10.7 `PickedLine::pick` is load-bearing, not a diagnostic.**
  `linepick.rs:21-36`: two crossing lines bound four angles, and
  `classify_two_lines` picks which one is meant from where the operator
  clicked. Store `pick`; discarding it makes the angle unreconstructible.
- **T-10.8 `ParallelPolicy::force_parallel` is checked BEFORE
  `epsilon_degrees`, unconditionally.** `linepick.rs:405-409`.
- **T-10.9 `TextObject::approximate` is always `true`** (`decompose.rs:698`)
  and `TextFont::size` is the raw `Tf` operand — `/F1 1 Tf` then
  `12 0 0 12 x y Tm` renders 12 pt and reports `1` (`decompose.rs:471-481`).
- **T-10.10 `ImageObject::pixel_size` is a sample count.**
  `decompose.rs:418-435` quotes §8.9.5: printed size comes from the CTM and
  *"has no fixed relationship to these numbers."* Use `page_bbox`.
- **T-10.11 `SnapKind::Midpoint` never appears on curved segments** —
  cubics contribute only a `SegmentCenterline` projection
  (`snap.rs:175-179`, `:587-592`).
- **T-10.12 `SnapKind::Node` vs `Endpoint` depends on `Subpath::closed`.**
  `snap.rs:545-556`: **every** anchor of a closed subpath is `Node`;
  `Endpoint` requires an open subpath's free terminus.
- **T-10.13 `SnapConfig::intersections` defaults `false`** and is
  neighbourhood-bounded (`snap.rs:57-71`). Enabling it on a dense page is a
  documented perf trade, not free.
- **T-10.14 `ARCHITECTURE.md`'s line numbers for `hit.rs` are already
  drifted** (it cites `hit_test_subpaths` at `hit.rs:277`; it is at `:340`).
  Verify against source, not against the architecture doc.

### 10.8 Stability

| Module | Signal |
|---|---|
| `geometry.rs` | **Stable.** 3 commits ever; safe foundation. |
| `centerline.rs` | **Stable.** Single commit (`e13f3e6`, Pass 9a). |
| `snap.rs` | Single commit (`801a748`). Whole design landed at once — stable but *young*, not iterated. |
| `hit.rs` | **Evolving** in lockstep with the text sub-model (`d26d269`, `7fc943a`, `627c807`). Base point/rect queries older and settled; run/subpath drill-down recent. |
| `decompose.rs` | **Highest churn in the crate's read side.** `TextRun`/`RunPositioning`/`TextBoundsBasis` are Pass 30/32 additions. Expect the text sub-model to keep moving. |
| `linepick.rs` | **Newest — 2026-08-12, two commits.** Least baked; `ParallelPolicy` and `TwoLineRelation` shapes may still move. Not yet flat-re-exported. |
| `mod.rs` | Re-export list lags new submodules. **Check the submodule, not `mod.rs`, to decide whether something is public.** |

---

## 11. Filters, images, colour, functions

**Module set:** `filters`, `image_codec`, `color`, `function`.

### 11.1 Stream decoding

```rust
use pdfcer_core::filters::{decode_stream, decode_stream_with_notes, FilterError, FilterNotes};

let bytes: Vec<u8> = decode_stream(&stream.dict, raw)?;                 // filters/mod.rs:186
let (bytes, notes) = decode_stream_with_notes(&stream.dict, raw)?;      // filters/mod.rs:200
```

Runs the **full `/Filter` chain** with `/DecodeParms`, including PNG/TIFF
predictors. `FilterError` — `filters/mod.rs:99`, `#[non_exhaustive]`.
`FilterNotes` — `:166`, `#[non_exhaustive]`, currently
`lzw_framing_anomalies: usize`. Use the `_with_notes` form anywhere the
notes have somewhere to go; every other caller (xref/object/content
streams) uses the plain form.

Individual filters are also public if you need one directly:
`ascii::decode_hex` `:95` / `decode_85` `:211`, `flate::decode` `:41`,
`lzw::decode` `:124`, `runlength::decode` `:84`, `predictor::Params`
`:45` / `::from_dict` `:72` / `unpredict` `:135`.

**★ `decode_stream` deliberately refuses image codecs.** `filters/mod.rs:138-152`:
hitting `DCTDecode`/`CCITTFaxDecode`/`JBIG2Decode`/`JPXDecode` returns
`FilterError::ImageCodec` — a **distinct** variant from `UnsupportedFilter`,
meaning *"you called the wrong entry point"*. Route images through
`image_codec::decode_image*` (§11.2).

### 11.2 Image decoding

```rust
use pdfcer_core::image_codec::{decode_image, decode_image_view, terminal_codec,
                              CodedImage, CodecColorModel, Codec};

let which: Option<Codec> = terminal_codec(&image_dict)?;   // mod.rs:467 — no decode
let img: CodedImage = decode_image(&doc, &image_dict, raw, /*inline=*/false)?; // mod.rs:503
// session-aware form:
let img = decode_image_view(&doc.view(), &image_dict, raw, false)?;            // mod.rs:524
// explicit CMYK-JPEG polarity (R169 setting):
// decode_image_view_with(view, dict, raw, inline, CmykJpegPolarity::…)        // mod.rs:569
```

`Codec` — `mod.rs:169`: `Dct | Ccitt | Jbig2 | Jpx`; `Codec::name` `:183`,
`Codec::allowed_inline` `:201` (§8.9.7 — `Jbig2`/`Jpx` are `false`).
`CodedImage` — `mod.rs:338`, `#[non_exhaustive]`.
`CodecColorModel` — `mod.rs:230`: `Gray | Rgb | Untransformed3 | Cmyk |
Bilevel | Unspecified | Unknown{components}`.
`CodecNotes` — `mod.rs:280`: `geometry_mismatch`, `cmyk_image`,
`cmyk_polarity_unverifiable`, `jpx_smask_in_data_preblended`,
`lzw_framing_anomalies`.
`ImageCodecError` — `mod.rs:396`: `Filter | Unsupported | FeatureUnsupported
| Corrupt | TooLarge | NotAllowedInline | CodecNotTerminal`.

The per-codec modules (`image_codec::{dct, ccitt, jbig2, jpx}`,
`mod.rs:101-105`) have an **empty public surface** — every `decode` is
`pub(super)`. `decode_image*` is the only door.

#### ★ Image output format — exact

Evidence: `mod.rs:338-391`, `bilevel.rs:26-59`.

| Property | Value |
|---|---|
| Row order | **top-down**, row 0 at the top |
| Layout | row-major, interleaved, packed to `bits_per_component`, **each row padded to a byte boundary** (§8.9.3) |
| Bit depth | `CodedImage::bits_per_component` — **codestream-declared**. DCT always 8; CCITT/JBIG2 always 1; JPX codestream-authoritative (`/BitsPerComponent` is ignored, Table 89) |
| Channel count | `CodedImage::components` — codestream-declared; `0` = not declared by any codec |
| Channel order | **RGB, not BGR** (`dct.rs:355,421,932` via `zune_core::colorspace::ColorSpace::RGB`); no BGR path exists |
| CMYK order | C,M,Y,K, **raw** — no `/Decode`, no inversion applied here |
| Alpha | **not premultiplied**, and normally absent. `CodedImage::embedded_alpha` is populated only by JPX with `/SMaskInData == 1`. The opacity channel is **always stripped out of `samples`** — leaving it interleaved *"would shift every colour one position to the right"* (`mod.rs:385-387`) |
| Bilevel polarity | normalised by both adapters to **`0 = black`** regardless of codec-native polarity (`bilevel.rs:47-59`) |
| `width`/`height` units | **pixels/samples**, as the codestream declares — may disagree with `/Width`/`/Height`; see `CodecNotes::geometry_mismatch` |
| `samples` / `embedded_alpha` units | **bytes** (`Vec<u8>`), per the packed+padded layout — not a sample count |

**★ `/Decode` arrays and any polarity flip are `pdfcer-render`'s job, never
this crate's** (rule R26, `mod.rs:65-79`). If your shell rasterizes itself
rather than calling `pdfcer-render`, you must apply `/Decode` and the
colour-space mapping yourself; `decode_image` hands you the codec's raw
samples plus an honest statement of what they are.

### 11.3 Colour

**★ There is no `/ColorSpace` object parser in `pdfcer-core`.** `color/mod.rs`
has exactly three device converters plus an intent variant:

```rust
use pdfcer_core::color::{gray_to_srgb, rgb_to_srgb, cmyk_to_srgb, cmyk_to_srgb_with};
let rgb = cmyk_to_srgb(0.0, 0.0, 0.0, 1.0);   // color/mod.rs:254
```

`gray_to_srgb` `:197`, `rgb_to_srgb` `:215`, `cmyk_to_srgb` `:254`,
`cmyk_to_srgb_with(CmykIntent, …)` `:354`. All take/return `f32` components
in **0.0–1.0**, returning `[f32; 3]` sRGB.

Full `/ColorSpace` resolution (`Separation`, `DeviceN`, `ICCBased`,
`Indexed`, …) lives in **`pdfcer-render`** (`pdfcer-render/src/color.rs:215`,
`pub enum ColorSpace`). This split is deliberate per rule R26 — *"the codec
layer never decides colour"* — not a gap. A `Separation`/`DeviceN` colour is
a two-step composition: `PdfFunction::eval` (tint → alternate-space
components), then the matching `*_to_srgb`.

**★ `DeviceGray` 0.0 = black; `DeviceCMYK` 0.0 = white.** `color/mod.rs:186-188`
exists to keep that polarity trap visible: *"The two device spaces run
opposite ways."*

**★ `cmyk_to_srgb` is a calibrated house choice, never "colorimetrically
correct".** `color/mod.rs:28-31`, `:225-228`: *"There is no 'correct' answer
to be spec-compliant about … it should never be described as
'colorimetrically correct'."* It uses a calibrated 6⁴ node grid — so
`cmyk_to_srgb(0,0,0,1)` is a rich near-black, **not** `[0,0,0]`. Do not
describe it to users as exact, and do not swap in a naive
`255*(1-c)*(1-k)` formula expecting a match.

### 11.4 PDF functions

```rust
use pdfcer_core::function::{PdfFunction, FunctionType, FunctionError};

let f = PdfFunction::load(&doc.view(), &function_obj)?;   // function.rs:751 — validates structure
let outs: Vec<f64> = f.eval(&inputs)?;                     // function.rs:979
// Per-pixel path — reuse the buffer:
let mut buf = Vec::new();
f.eval_into(&inputs, &mut buf)?;                           // function.rs:1025
```

`FunctionType` — `:616`: `Sampled | Exponential | Stitching | PostScript`
(0/2/3/4). Accessors: `function_type` `:852`, `inputs` `:863`, `outputs`
`:869`, `domain` `:879`, `range` `:890`, `cubic_downgraded` `:918`.
`FunctionError` — `:268`, `#[non_exhaustive]`, ~28 variants.

### 11.5 Resource limits

| Guard | Constant / value | `file:line` | Error |
|---|---|---|---|
| Decoded byte-stream ceiling (incremental) | `filters::MAX_DECODED_LEN` = 256 MiB | `filters/mod.rs:94` | `FilterError::OutputTooLarge` |
| Image pixel count | `MAX_IMAGE_PIXELS` = 32 Mpx | `image_codec/mod.rs:144` | `ImageCodecError::TooLarge` |
| Image dimension | `MAX_IMAGE_DIMENSION` = 65,535 | `image_codec/mod.rs:157` | `TooLarge` |
| Decoded sample bytes | `MAX_IMAGE_SAMPLE_BYTES` = 128 MiB | `image_codec/mod.rs:164` | `TooLarge` |
| DCT progressive scans | `dct::MAX_PROGRESSIVE_SCANS` = 100 *(private)* | `image_codec/dct.rs:178` | `Corrupt` |
| JPX working memory | `jpx::MAX_WORKING_BYTES` *(private)* | `image_codec/jpx.rs:282` | `TooLarge` |
| JPX tile count | `jpx::MAX_TILES` = 4096 *(private)* | `image_codec/jpx.rs:304` | `TooLarge` |
| JPX component bit depth | `jpx::MAX_COMPONENT_BIT_DEPTH` = 31 *(private)* | `image_codec/jpx.rs:314` | `FeatureUnsupported` |
| CCITT/JBIG2 sink budget | `BilevelSink::budget` (latched, since vendor sinks are infallible) | `image_codec/bilevel.rs:119-123` | `TooLarge` |
| Type-4 PS stack | `PS_STACK_LIMIT` = 100 — **spec floor+ceiling**, not policy | `function.rs:177` | `StackOverflow{limit}` |
| Type-4 PS steps | `MAX_PS_STEPS` = 1,000,000 | `function.rs:202` | `StepLimit{limit}` |
| Type-4 brace nesting | `MAX_PS_NESTING` = 32 | `function.rs:215` | `PostScriptNestingTooDeep{limit}` |
| Type-0 input dimensions | `MAX_SAMPLED_INPUTS` = 8 | `function.rs:229` | `TooManyInputs{got,limit}` |
| Type-3 recursion (also catches `/Functions` cycles) | `MAX_FUNCTION_DEPTH` = 8 | `function.rs:241` | `NestingTooDeep{limit}` |

**There is no runtime API to raise any of these.** The private ones you
cannot even observe. Do not attempt to bypass them; the only deliberate
configurability in this area is `CmykJpegPolarity` (a polarity choice under
R169, not a limit override).

### 11.6 Traps — decoding and colour

- **T-11.1 `decode_stream` on an image filter is an error by design**
  (`filters/mod.rs:138-152`) — `FilterError::ImageCodec`, not
  `UnsupportedFilter`.
- **★ T-11.2 CCITT `BlackIs1` polarity — *"the single most likely
  correctness bug"*.** `image_codec/ccitt.rs:54-78`: the mapping is the
  **direct** assignment `invert_black = BlackIs1`, not the negation.
  *"Getting this backwards renders every fax image as its own negative,
  which looks deliberate rather than broken."*
- **T-11.3 JBIG2 polarity is unconditional; there is no `/BlackIs1`
  equivalent.** `image_codec/jbig2.rs:57-77`.
- **T-11.4 For JPEG, an APP14 marker outranks `/DecodeParms`
  unconditionally**, and the fallback default is component-count dependent
  — *"a 4-component JPEG with neither defaults to `0`, i.e. no transform,
  not to `1`"* (`image_codec/dct.rs:52-56`).
- **★ T-11.5 pdfcer NEVER applies an "Adobe CMYK inversion" (rule R29).**
  `image_codec/dct.rs:113-129`: *"not on APP14 presence, not on transform-byte
  value, not on component count."* `CmykJpegPolarity::NeverInvert` is the
  default; only the explicit R169 setting changes it. If your shell
  "corrects" CMYK JPEGs by inverting them, you are reintroducing the bug
  four reference engines agree is not there. The residual ambiguity is
  **reported, never repaired**, via `CodecNotes::cmyk_polarity_unverifiable`
  (`mod.rs:298-308`).
- **T-11.6 The YCCK→CMYK step *is* performed** — because zune-jpeg has no
  YCCK arm — and is *"not a polarity guess"* (`dct.rs:246-249`). Do not
  conflate it with T-11.5.
- **T-11.7 For JPX, a present `/ColorSpace` WINS over the codestream.**
  `image_codec/jpx.rs:38`: *"the trap is to read 'the codestream is
  authoritative for JPX' as unconditional"* — it wins only when the
  dictionary is silent.
- **T-11.8 `/SMaskInData == 2` is recognise-and-defer.** `mod.rs:309-324`:
  `embedded_alpha` stays `None` and a note is set, because the colour
  samples are already composited over an unknown backdrop and
  un-premultiplying needs a `Matte` this crate does not have.
- **T-11.9 RunLengthDecode literal-run off-by-one.** `filters/runlength.rs:24`:
  writing `L <= 128` for the literal branch *"consumes the EOD marker as
  data."*
- **T-11.10 PNG Average predictor is not modulo-256.** `filters/predictor.rs:33`:
  `left + prior` reaches 510 and must be computed wide before the
  floor-divide. And **Paeth's tie-break order (a, then b, then c) is
  normative** (`:35`) — a different order is wrong on only *some* inputs.
- **T-11.11 LZW: `BitOrder::Msb` is mandatory** (GIF's LSB packing is a
  different codec) and `/EarlyChange` changes the code-width switch points
  (`filters/lzw.rs:32-41`).
- **T-11.12 Function `/C0`/`/C1` default to the SCALARS `[0.0]`/`[1.0]`,
  not "n zeros".** `function.rs:1689-1697`: *"A type 2 with neither entry
  present is therefore a 1-output function, and a 4-output tint transform
  must carry explicit four-element arrays."*
- **T-11.13 `PdfFunction::range()` returning `None` means NO clipping and
  must not be defaulted.** `function.rs:886-889` quoting Table 38: *"If this
  entry is absent, no clipping shall be done."*
- **T-11.14 NaN inputs are refused, never clamped** (`FunctionError::NonFiniteInput`)
  — *"a NaN tint clamped to /Domain would silently become the domain's lower
  bound — a fabricated value wearing the shape of a real one."*
- **T-11.15 There is no `/FunctionType` 1.** `function.rs:39-42`:
  `UnknownFunctionType` reports `1` exactly as it reports `7`.
- **T-11.16 `/Order 3` may be silently downgraded to linear.** pdfcer always
  evaluates multilinearly and exposes whether a downgrade happened via
  `cubic_downgraded()` (`function.rs:918-947`) — surface it if you show
  gradients.

### 11.7 Stability

`filters/` is effectively **frozen** since the initial import.
`function.rs` landed as **one unit** (`9e70247`) and has not been iterated —
young but untouched. `color/` is **recent**: the calibrated CMYK table is a
rewrite (`edf7c02`), not legacy. `image_codec/` is the **most active** of
the four (`fbcb946`, `6d63d81`, `f51675d`).

**UNVERIFIED — decision record `034` (write-side CMYK/YCCK polarity) does
not exist as a file at HEAD**, only as an `ARCHITECTURE.md` log entry
claiming it. Irrelevant to the read side (which is fully covered by
decision 006 / R29 / R30), but do not go looking for the document.

---

## 12. Navigation, annotations, metadata

**Module set:** `outline`, `attachments`, `layers`, `annot`,
`pageops::references` (read half).

### 12.1 Outline (bookmarks)

```rust
use pdfcer_core::outline::{read_outline, parse_outline, Destination, DestView};

let outline = read_outline(&doc);          // outline.rs:1066 — generic over ObjectGraph
for item in outline.flatten() {             // outline.rs:919 — document order, flat
    let label = &item.title;                // already text-decoded
    let (bold, italic) = (item.is_bold(), item.is_italic());   // :352, :343
    match &item.destination {
        Some(Destination::Page { page_index, view }) => {
            // page_index is ALREADY 0-based into pages_in(&doc) — resolution done for you
            match view {
                DestView::Xyz { left, top, zoom } => { /* target page USER space */ }
                DestView::Fit | DestView::FitB => { /* whole page */ }
                _ => {}
            }
        }
        // ★ The disclosure variants — render as "cannot navigate", never drop:
        Some(Destination::UnmappedPage { .. }) => {}
        Some(Destination::Named(_))            => {}
        Some(Destination::Remote { .. })       => {}
        Some(Destination::NonNavigation)       => {}
        None => {}
    }
}
```

`Outline` — `outline.rs:901`: `{items: Vec<OutlineItem>, diagnostics}`,
`#[non_exhaustive]`. It is a **real tree** (`OutlineItem` `:247` has
children); `flatten()` gives you the document-order flat list a rail wants.
`visible_item_count()` `:958` implements Table 152's root `/Count`.
`parse_outline` `:1162` is `read_outline(g).items` with diagnostics
discarded — prefer `read_outline`.

`Destination` — `:386`, `#[non_exhaustive]`, 6 variants.
`DestView` — `:566`, `#[non_exhaustive]`: `Xyz | Fit | FitH | FitV | FitR |
FitB | FitBH | FitBV | Unknown | Absent`, with `rect()` `:663` and
`zoom_is_retain()` `:691`.
`RemoteTarget` — `:496`, `page_index()` `:542`.
`OutlineDiagnostics` — `:717`, ~25 counters.
`MAX_OUTLINE_DEPTH` = 32 — `:218`.

**Coordinates:** `outline.rs:550-556` — *"Coordinates are in the target
page's **user space**, unmodified. pdfcer does not apply `/CropBox`,
`/Rotate` or any viewer-side clamping here."* Your scroll-to code applies
those.

### 12.2 Attachments (read half)

```rust
use pdfcer_core::attachments::{list_attachments, list_attachments_with_notes,
                              attachment_bytes, extract_attachment};

let view = doc.view();                                  // required for extraction
let (found, notes) = list_attachments_with_notes(&doc);  // attachments.rs:850
for att in &found {
    let display = &att.name;
    let file    = att.safe_name();                       // attachments.rs:648 — sanitized
    match &att.kind { /* DocumentLevel{..} | PageAnnotation{..} */ }  // :274
    if notes.may_be_encrypted { warn_user(); }           // ★ see T-12.4
    let data = attachment_bytes(&view, att);             // :1435 -> Option<Vec<u8>>
    // or extract_attachment(&view, att) -> Result<ExtractedAttachment, _>  :1496
}
```

`Attachment` — `:469`, `#[non_exhaustive]`: `name`, `name_bytes`, `kind`,
`declared_size`, `mime`, `stream_id`, `filespec_id`.
`ExtractedAttachment` — `:769`: `{data, declared_size, size_check}`.
`DeclaredSizeCheck` — `:410`. `AttachmentNotes` — `:664`.
`AttachmentError` — `:733`. `NameHazard` `:1542`, `SafeName` `:1621`,
`sanitize_attachment_name` `:1745`.
Caps: `MAX_ATTACHMENTS` `:247`, `MAX_SAFE_NAME_CHARS` `:256`,
`FALLBACK_SAFE_NAME` `:261`.

**★ Extracted bytes are UNTRUSTED** (`attachments.rs:50`). Never
auto-open, never execute. The declared `mime` and the name extension are
producer claims — `attachments.rs:559,604` says the caller *"must not treat
it as a safety signal"*. The checksum is **reported, never verified**.

### 12.3 Optional-content layers

```rust
use pdfcer_core::layers::{read_layers, read_layers_with, list_layers, LayerScan};

let layers = read_layers(&doc);                 // layers.rs:944 (full CatalogAndPages scan)
for l in &layers.layers {                        // Layer — layers.rs:509
    let name = &l.name;
    let on   = l.visible_by_default;             // :550 — INITIAL /D state only
    let locked = l.locked;                        // :576 — a UI hint, NOT enforced
    let _ = (l.radio_group, l.in_default_config, l.in_order);  // :586, :592, :600
}
let _ = (&layers.order, &layers.radio_groups, &layers.config_name, &layers.diagnostics);
```

`Layers` — `:675`, `#[non_exhaustive]`. `OrderNode` — `:645` (the `/D /Order`
tree, for a nested layers panel). `LayerDiagnostics` — `:706` with
`is_faithful()` `:903`. `LayerScan` `:457`, `LayerSource` `:479`.
`list_layers` `:934` is the convenience form. Caps: `MAX_LAYERS` `:419`,
`MAX_ORDER_DEPTH` `:430`, `MAX_ORDER_NODES` `:437`, `MAX_RESOURCE_NODES` `:446`.

The visibility algebra lives in `annot`:
`optional_content_default_off(&graph)` — `annot.rs:701` — is the
**print/export-correct** OFF set. `oc_is_hidden(&graph, ocg, &off_set)` —
`annot.rs:994`. `apply_view_usage(&graph, …)` — `annot.rs:1268` — refines
that for **on-screen View only**. `MAX_VE_DEPTH` — `annot.rs:809`.

### 12.4 Annotations

```rust
use pdfcer_core::annot::{page_annotations, page_annotations_with, Annotation, Appearance};
use pdfcer_core::page_tree::pages_in;

for page in &pages_in(&doc)? {
    for a in page_annotations(&doc, page.id) {       // annot.rs:531
        if let Some(rect) = a.rect {                  // PDF USER SPACE, y-UP, points
            draw_marker(rect);
        }
        let _ = (a.subtype_label(), a.is_widget(), a.contents.as_deref(), a.title.as_deref());
    }
}
```

`Annotation` — `annot.rs:290`: `id`, `subtype`, `rect: Option<Rect>`,
`flags: AnnotFlags`, `appearance: Appearance`, `is_popup`, `contents`,
`title` (conventionally the author, Table 170), `mod_date` (**raw and
unparsed** — §12.5.2 requires accepting any format), `oc`, `popup`,
`in_reply_to`, `reply_type`, and — **`Pass 255.0`** — the point geometry a
shell draws reshape anchors from: `vertices: Option<Vec<(f64, f64)>>`
(`/Vertices`, Polygon/PolyLine; for a cloud these are the PRE-bulge
vertices), `line: Option<[(f64, f64); 2]>` (`/L`, Line — populated for a ce
dimension too), `ink_list: Option<Vec<Vec<(f64, f64)>>>` (`/InkList`, one
inner vec per stroke; read-only geometry — per-point ink editing is refused
by name). Each is read whenever its key is present regardless of subtype;
absent → `None`, never an empty list. `AnnotFlags::locked_contents()` (bit 10,
value **512**) joined `locked()` (bit 8, 128) — two gates, see part 2 §1.15.
Methods: `is_widget()` `:450`, `is_group_subordinate()` `:468`,
`effective_reply_type()` `:485`, `subtype_label()` `:495`.
`AnnotFlags(pub u32)` `:132`, `Appearance` `:253`, `ReplyType` `:432`.
`page_annotations_with` `:567` takes a `MissingAppearanceState` policy.
`need_appearances(&graph)` `:1553` checks `/AcroForm /NeedAppearances`.
`MAX_ANNOTS_PER_PAGE` = 1,000,000 — `:117`.

**Coordinates:** `annot.rs:299` — *"The `/Rect` in default user space,
normalised per §7.9.5."* y-UP, points, **not flipped**, **not** adjusted for
`crop_box` or `rotate`.

**★★ `Annotation` still has NO `/Dest` and NO `/A` field**, and that is
deliberate — but as of `Pass 222.0` **it no longer means links are
unreachable.** The previous wording of this section said *"clickable
hyperlinks are therefore not available"* and told you to read the raw
dict, map `ObjId`s through `page_slots`, and implement fit-style parsing
yourself. **That is obsolete. Do not do it.** The resolution is now
public and is the same code the bookmarks panel uses.

```rust
use pdfcer_core::annot::page_link_destinations;
use pdfcer_core::outline::{Destination, DestinationReader, DestView};

// Built ONCE per document. It flattens both named-destination
// namespaces and the page map — O(document) — so building one per page
// walks the page tree once per page.
let reader = DestinationReader::new(&doc);          // outline.rs:1649

for page in &pages_in(&doc)? {
    let found = page_link_destinations(&doc, page.id, &reader);   // annot.rs:852
    for link in &found.links {
        // Hit-test on `link.rect`; navigate on `link.destination`.
        if let Destination::Page { page_index, view } = &link.destination {
            go_to(*page_index, view);        // page_index is ALREADY 0-based
        }
    }
    // Links carrying NEITHER /Dest nor /A — clickable, and able to do
    // nothing. Counted, never dropped, so that "no links" and "all the
    // links are broken" stay distinguishable.
    let _ = found.links_without_destination;
}
```

`DestinationReader` — `outline.rs:1620`, with `new` `:1649`,
`page_tree_error()` `:1679`, `named_destination_count()` `:1691`,
`destination(&graph, carrier_dict)` `:1732`, and
`destination_with_diagnostics` `:1758` when you want the
`OutlineDiagnostics` the read produced.

`page_link_destinations(&graph, page_id, &reader)` — `annot.rs:852` →
`PageLinks` `:776` (`links: Vec<LinkDestination>`,
`links_without_destination: usize`). `LinkDestination` `:737` carries
`annots_index` (the `/Annots` position — **not** its position in
`links`), `id`, `rect`, `destination`.

`Annotation::destination(&graph, &reader)` — `annot.rs:622` — resolves a
**single** annotation, including a `/Widget` pushbutton's `/A`. It needs
`Annotation::id`, so a dictionary written directly into `/Annots` (legal,
rare) returns `None` indistinguishably from "carries no destination".
`page_link_destinations` has no such blind spot; prefer it whenever
completeness matters.

**★ The five variants are the disclosure, and four of them are not
`None`.** Only `Destination::Page` is navigable. `UnmappedPage` (a target
that is not a page in this tree — the residue of a page delete), `Named`
(a name neither namespace defines), `Remote` (`/GoToR`, another file —
**never** resolved against this document's names, by design) and
`NonNavigation` (`/URI`, `/Launch`, `/JavaScript`, … — *recognised and
disclosed, never executed*) each say something a viewer should tell the
operator. Collapsing them into "no link here" reports a document full of
working links as empty; collapsing them into a page jump lies about where
it goes.

**`Annotation::action_type` is unchanged** and is still the `/S` name
only. That is the right disclosure for an inventory and costs nothing to
read; this is the separate, expensive question of where the action
*points*.

`pageops::references::DestinationResolver` still exists and still answers
a **different** question — "which page object does this reference, for
the delete/extract dangling census" — discarding the view parameters on
the way. Use `DestinationReader` for anything that navigates.
`DestinationResolver` — `pageops/references.rs:72`, with `new` `:84`,
`named_count` `:121`, `names_targeting` `:129`, `resolve_destination` `:149`,
`resolve_target` `:187`. Also `census_dangling` `:336` → `DanglingReport`
`:287`, useful for a document-health panel.

### 12.5 Signatures — census, coverage, and (`Pass 10.1`) integrity verification

```rust
let census = pdfcer_core::signature::census(&doc);              // signature.rs:370
let cov = pdfcer_core::signature::byte_range_coverage(&doc, /* … */);  // signature.rs:900
// Pass 10.1 — verification. `bytes` is the FILE the graph was loaded from
// (`Document::bytes()`); the digest is over those bytes, not over objects.
let verdicts = pdfcer_core::signature::verify_all(&doc.view(), doc.bytes());
let one = pdfcer_core::signature::verify(&doc.view(), doc.bytes(), 0);   // Option<SignatureVerdict>
```

`SignatureCensus` `:265`, `SignatureImpact` `:174`, `ImpactBasis` `:228`,
`SaveMode` `:249`, `ByteRangeCoverage` `:843`. This tells you what signing
state a document is in and what a save would do to it — enough for a
warning banner.

**`verify_all` / `verify` (`signature_verify.rs`, re-exported from
`signature`).** One `SignatureVerdict` per `/FT /Sig` field with a `/V`, in
`byte_range_coverage`'s order, carrying **three independent facts** that a
shell must keep apart:

| field | type | what it answers |
|---|---|---|
| `integrity` | `Integrity` — `Verified { digest_algorithm, signature_algorithm }` / `DigestMismatch` / `SignatureInvalid` / `Unverifiable { reason }` | are the signed bytes unaltered (digest over `/ByteRange` vs the signed `messageDigest`), and is the signature over the signed attributes genuine against the signer's OWN embedded certificate? `DigestMismatch` = the document was altered; `SignatureInvalid` = the digest matches but the signature/certificate does not; `Unverifiable` names why pdfcer cannot say (a subfilter, algorithm or curve it lacks, a malformed CMS, a missing certificate) and is never either of the others |
| `coverage` | `ByteRangeCoverage` | was anything appended after signing (`covers_to_eof()`) |
| `trust` | `Trust` — `NotChecked` unless anchors are supplied (`Pass 10.3`), then `Trusted`/`Untrusted`/`SignerUnknown` | `verify_all_with_trust` + a trust-anchor pool; chain/signature only, NOT revocation or clock |

Plus claims — `signer_subject`, `signer_issuer`, `cert_not_before`,
`cert_not_after`, `signing_time`, and the dictionary's `name`/`date`/
`reason`/`location` — and `notes` (a SHA-1 digest, non-zero padding, extra
`/ByteRange` gaps, an ETSI signature that does not reach EOF, extra signers).

Implemented: `adbe.pkcs7.detached`, `ETSI.CAdES.detached`, `adbe.pkcs7.sha1`
(the double hash — the inner SHA-1 is pinned by the subfilter); RSA PKCS#1
v1.5 and RSASSA-PSS, ECDSA P-256/P-384; SHA-1/256/384/512.
`adbe.x509.rsa_sha1`, `ETSI.RFC3161`, P-521, Brainpool → `Unverifiable` by
name. All in-crate (`asn1.rs`, `cms.rs`, `crypto::{bignum,rsa,ecdsa,sha1}`),
no new dependency; verified against pyHanko-signed fixtures whose expected
verdicts were recorded from pyHanko's own validator first
(`fixtures/synthetic/signature-verify/PROVENANCE.md`).

★ **Disclosure contract.** `Integrity::Verified` must never be rendered as
"valid" or "signed by X". The sentence pdfcer-gui and the CLI share: *"the
bytes under this signature have not been altered and nothing was appended
after it; pdfcer does not check who signed it or whether to trust them."*
The CLI is `verify-signatures`: exit 0 all verified, **12** any failed,
**13** none failed but some unverifiable.

### 12.5a Trust anchors from an installed Acrobat (`Pass 10.2`, `trust_store`)

Until `Pass 10.3`, the `trust` axis was always `NotChecked` for lack of a trust anchor set.
`pdfcer_core::trust_store` supplies one by reading the AATL + EU-Trusted-List
certificates an installed **Acrobat/Reader** has already downloaded into
`addressbook.acrodata` — a `%PPKLITE-` COS file the existing tokenizer opens
(via `Document::from_cos_bytes`) and whose embedded certs the Pass-10.1 X.509
decoder reads. **This is the anchor POOL only; it does not itself produce a
verdict** (chain-building + revocation + clock are `Pass 10.3`).

```rust
use pdfcer_core::trust_store::{self, SourceFilter};

let set = trust_store::load_from_path(path)?;      // or load_from_bytes(Vec<u8>)
let c = set.counts();                               // aatl / eutl / adbe / other / total
for a in set.filter(SourceFilter::Aatl) {           // AATL-only, or Eutl/Adbe/All
    // a.subject, a.issuer, a.serial_hex, a.not_before/after,
    // a.sources (["AATL"], ["EUTL"], …), a.trust_bits (RAW), a.policy_oids, a.der
}
```

**Contract points a consumer must respect (rule 4):**
- **`trust_bits` is RAW and its meaning is provisional.** Adobe does not publish
  the `/Trust` bit constants; surface the integer + `a.sources`, do NOT act on a
  specific bit to grant certify / JavaScript / system-operation trust.
- **`/Source` is the authoritative provenance**, and `SourceFilter` narrows to
  exactly AATL (the operator's "reconstruct-AATL" concern — AATL is a superset
  of Windows-roots ∪ EUTL by construction, so reading Acrobat's own store is the
  only 1:1 route; decision 133).
- **Freshness is Acrobat's, not pdfcer's** — the anchor set is only as current
  as Acrobat's last refresh; disclose the store file's mtime.
- **Locating the file is the SHELL's job** (decision 133): `trust_store` takes a
  path/bytes; the CLI's `trust-store-list` auto-locates
  `%APPDATA%\Adobe\Acrobat\<track>\Security\addressbook.acrodata` and is
  OFF by default (an explicit invocation). `set.undecodable` counts entries whose
  cert did not decode — disclose it rather than imply the store was fully read.
- **Read-only, no network.** Nothing is written; a bad store is a named
  `TrustStoreError`, never a panic (fuzzed).

### 12.5b Evaluating signer trust against the anchors (`Pass 10.3`, `trust_chain`)

`signature::verify_all_with_trust(graph, bytes, anchors: Option<&TrustAnchorSet>)`
turns the anchor pool into a per-signature `Trust` verdict. `None` ⇒ `NotChecked`
(identical to `verify_all`). `Some` ⇒ each signer is chained, **by verifying
each link's signature**, to a trusted anchor:

- `Trust::Trusted { anchor_subject, source, validity_checked }` — the signer
  chains to an anchor, AND RFC 5280 CA/key-usage constraints held.
  `validity_checked` is `true` iff certificate validity dates were checked
  against the signing time (`false` ⇒ no signing-time clock, so expiry was not
  verified). Revocation is never checked (see below).
- `Trust::Untrusted { reason }` — a parsed signer that does NOT chain (incomplete
  chain, untrusted self-signed root, a link whose signature failed, a non-CA
  intermediate, or a certificate outside its validity window at the signing
  time). A valid signature with an untrusted signer is `Integrity::Verified` +
  `Trust::Untrusted` ("valid but untrusted") — the two axes are independent.
- `Trust::SignerUnknown` — the signer certificate could not be parsed.

**★ What `Trusted` DOES and does NOT mean (`Pass 10.5`).** `trust_chain::evaluate(
signer_der, intermediates, anchors, now: Option<&str>)` returns
`ChainVerdict::Trusted { anchor_subject, source, checks: PathChecks }`, where
`PathChecks { validity_checked, constraints_checked, revocation_checked }`
records exactly which checks ran. It verifies: **signature linkage** (every
issuer→subject link), **CA/key-usage constraints** on intermediates
(`basicConstraints` cA TRUE + `keyUsage` not clearing `keyCertSign`;
`constraints_checked` always `true`), and — when `now` is supplied — **validity
dates** (`notBefore ≤ now ≤ notAfter` for every cert, RFC 5280 §4.1.2.5). It
does **not** check **revocation** (CRL/OCSP): that needs the network
`pdfcer-core` never touches, so `revocation_checked` is always `false` this
build (a shell that fetches, or embedded DSS/LTV data, is the later increment).
Cert signatures verify for RSA PKCS#1 v1.5, RSASSA-PSS (params from the cert's
`signatureAlgorithm`) and ECDSA; any other scheme is declined (safe direction).
The verdict carries
a note stating precisely what ran; a shell MUST surface it. Every uncertainty
resolves to `Untrusted`, never a false `Trusted`.

**Opt-in and at the operator's risk (decision 133).** Supplying the Acrobat
store is the shell's choice, off by default. The CLI's `verify-signatures
--trust-from-acrobat` loads it and prints the at-own-risk disclosure (reading
Adobe's own downloaded file is a local read; whether relying on it fits the
Adobe Reader licence is the operator's call, resolved by an explicit opt-in, not
a pdfcer legal determination). A persistent opt-in setting exists: `settings::AcrobatTrustStore { Off, AtOwnRisk }` (`Pass 10.4`, default `Off`); the CLI reads it as the default for `--trust-from-acrobat`, and the GUI binds it for its security tab.

### 12.6 ★ Document metadata — the honest gap

**There is no `Document`-level `/Info` accessor, and no XMP reader at all.**

The only public `/Info` reader is on `EditSession`:

```rust
use pdfcer_core::edit::{EditSession, InfoField};
let session = EditSession::new(doc);                       // edit.rs:3368 — takes ownership
let title = session.info_text(InfoField::Title);            // edit.rs:3807 -> Option<InfoText>
let raw   = session.info_bytes(InfoField::Title);           // edit.rs:3788
```

`InfoField` — `edit.rs:197`, `#[non_exhaustive]`: **only** `Title`,
`Author`, `Subject`, `Keywords`. `InfoText` — `edit.rs:3277`:
`{text: String, exact: bool}`.

Three consequences a GUI must plan for:

- **`/Producer` is deliberately excluded** (`edit.rs:179-194`, rule R41):
  producer identity is governed by the writer's `ProducerPolicy` and is
  *"the one field whose no-fingerprint rule must not be reachable through a
  general-purpose metadata editor."* To *display* it, read the `/Info`
  dict's `Producer` key by hand through `ObjectGraph`.
- **`/CreationDate` and `/ModDate` are not modelled either** — same place.
- **XMP (`/Metadata`) has no public reader.** Grep finds only a private
  byte-scan in `font_unembed.rs` (for `pdfaid:part`, PDF/A detection) and a
  scrubber in `redact.rs`. Neither exposes structured metadata.

For a read-only properties dialog, the lowest-friction route is raw:

```rust
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::textstring::decode_text_string;

let producer = doc.trailer_entry(b"Info")
    .map(|o| doc.resolve(o))
    .and_then(Object::as_dict)
    .and_then(|d| d.get(b"Producer"))
    .map(|o| doc.resolve(o))
    .and_then(|o| match o { Object::String(s) => Some(decode_text_string(s).text), _ => None });
```

**UNVERIFIED — whether a `Document`-level metadata accessor is planned.**
Check `docs/ROADMAP.md` before building a large properties panel on the raw
route; if one is coming, its shape will differ from the above.

**Also absent: a page-label decoder.** `/PageLabels` is tracked only as
present/stale (`pageops::references::DanglingReport::page_labels_stale`,
`references.rs:309`). Nothing turns the number tree into `"iii"` / `"A-1"`
strings. If your page rail shows document page labels rather than ordinals,
you are writing that yourself.

### 12.7 Traps — navigation and metadata

- **★ T-12.1 No `/Dest` or `/A` on `Annotation`** — §12.4. Hyperlinks need
  the `DestinationResolver` route.
- **★ T-12.2 `DestView::FitR` is NOT a `/Rect` and must not be
  normalised.** `outline.rs:600-607`: *"Reusing a normalising rectangle
  parser here would silently reorder a destination the producer wrote
  deliberately, so do not assume `left < right` or `bottom < top`."*
- **T-12.3 Bold/italic bit order is reversed from intuition** — italic is
  bit 1, bold is bit 2 (`outline.rs:328-333`). Use `is_bold()`/`is_italic()`,
  never raw bit math.
- **★ T-12.4 Attachment encryption is silently invisible.**
  `attachments.rs:700-721`: since PDF 1.5 an embedded file *"can be
  encrypted in an otherwise unencrypted document"*, and the intuitive guard
  is *"wrong silently: the `/Filter` chain runs, produces bytes, and those
  bytes are garbage that looks like a successful extraction."* Check
  `AttachmentNotes::may_be_encrypted`. pdfcer-core does not decrypt on this
  path.
- **T-12.5 `extract_attachment`'s `view` must come from the same document
  the listing came from.** `attachments.rs:1442-1452`: a mismatch usually
  errors but *could* silently return another document's bytes at a colliding
  id — *"pdfcer cannot detect the confusion … the obligation is the
  caller's."*
- **T-12.6 `Attachment::kind::PageAnnotation{page_index}` is a snapshot;
  `page_id` is the stable key.** `attachments.rs:294-299`. Use `page_id`
  for identity, `page_index` only for display order at read time.
- **T-12.7 `/RF` related files are silently unmodelled** — a known gap, not
  a bug (`attachments.rs:162-166`).
- **★ T-12.8 `apply_view_usage` must NEVER be reachable from a print or
  export path.** `annot.rs:1206-1224` quotes §8.11.4.5: printing
  applications *"shall not apply the changes based on usage application
  dictionaries."* `optional_content_default_off` is the complete and correct
  answer for printing. Calling `apply_view_usage` there *"would violate the
  standard rather than merely differ from it."* It is view-only and must be
  re-run on magnification change.
- **★ T-12.9 `pdfcer_render::LayerVisibility` REPLACES the document's
  default configuration, it does not merge with it.** (`ARCHITECTURE.md`
  §16229-16267.) You compute the **complete** hidden set — start from
  `optional_content_default_off`, apply operator toggles — and hand it in.
  `None` (obey the document) is a **distinct state** from `Some(empty set)`
  (show everything); collapsing them silently reveals document-hidden
  layers. Note also that the operator's layer toggle is **session-only
  state, held nowhere the save path can see it**, and is lost on reopen.
- **T-12.10 `/AllOff` with every member off is VISIBLE** (`annot.rs:2132`) —
  a counter-intuitive OCMD rule, marked `★` in source.
- **T-12.11 An empty configuration `/Intent` array means EVERYTHING is
  visible** (`annot.rs:2297`, `:751-757`) — fewer intents means *more*
  visible, not "no filter".
- **T-12.12 `Zoom` usage category is half-open `[min, max)`** — `max` is
  exclusive (`annot.rs:2520`, `:1159`).
- **T-12.13 Usage-application conjunction is global and order-independent
  (OFF dominates), which is the OPPOSITE algebra from `/D /ON`//`/OFF`
  arrays, where order IS load-bearing** (`annot.rs:2630`, `:1255-1266`,
  decision 038). Do not carry logic across that boundary.
- **T-12.14 `Annotation::mod_date` is a raw unparsed string** —
  §12.5.2 requires accepting any format. Parse defensively or display
  verbatim.
- **T-12.15 `Layer::locked` is a UI hint and is not enforced anywhere**
  (`layers.rs:551-575`).

### 12.8 Stability

`outline.rs` and `attachments.rs` shipped together (`1862b1f`, `fbddda5`)
and have not been revised since. **`layers.rs` and `annot.rs` are the most
actively-corrected files in this slice** — decisions 037/038, the
2026-08-10 `Design`-intent fix (a real shipped defect where a `Design`-only
group blanked a `View` render), and the `/AS` usage work. Treat the
optional-content visibility semantics as the **least stable part of the
read surface**, and re-verify against `ARCHITECTURE.md`'s dated entries
before building a feature that depends on a specific visibility edge case.
`pageops/references.rs` is moderately active. `settings/mod.rs` has only its
initial commit.

---

## 13. Settings

`settings/mod.rs` is **application settings persisted to disk**, not
document settings. A GUI shell owns the store and should reuse this rather
than inventing its own.

```rust
use pdfcer_core::settings::{self, Settings, StoreKind};

let store = settings::resolve_store();              // settings/mod.rs:1677
let (cfg, report) = Settings::load(store.clone());   // settings/mod.rs:1125
// … mutate cfg …
cfg.save(&store)?;                                   // settings/mod.rs:1622
```

`resolve_store() -> StoreLocation` `:1677` · `store_in(&Path)` `:1703` ·
`StoreLocation` `:177` · `StoreKind` `:161` · `Settings` `:840` ·
`Settings::load` `:1125` · `::parse` `:1160` · `::write_to_string` `:1369` ·
`::save` `:1622` · `LoadReport` `:261` · `SettingNote` `:201` ·
`SaveError` `:1642`.

**Store location** (`settings/mod.rs:1677-1707`, verified): portable first —
`<directory of the running executable>/userdata/settings.txt`, used when
that directory is writable (`StoreKind::Portable`); otherwise the platform
config dir (`StoreKind::PlatformFallback`); otherwise
`StoreKind::None` with `path: None`. `store_in(&Path)` exists for tests and
a future `--user-data-dir` override. This ordering is what makes the
single-folder portable packaging (`ARCHITECTURE.md` §6) actually portable —
do not reverse it.

The format is a deliberately non-`serde` flat `key = value` grammar, and
parsing is **fail-soft per key, not per document** (`settings/mod.rs:65-118`):
an unrecognised or malformed line becomes a `SettingNote` in the
`LoadReport` and the rest of the file still loads. Surface the notes; do not
discard them.

Several settings exist specifically because the standard is ambiguous and
the operator's standing directive (2026-08-08, R169) is that ambiguity
becomes a user choice rather than a hard-coded default. The full list, with
line numbers re-measured 2026-08-25:

| setting | line | the silence it fills |
|---|---|---|
| `CmykIntent` | `:313` | §8.6.4.4 defines no CMYK-to-screen conversion at all |
| `PageBlendSpaceSource` | `:408` | `PGB-A1` — where a page's blending space comes from when its group declares none |
| `MeshPatchPadding` | `:494` | `MSH-A1` — what a type 6/7 mesh-shading PATCH record pads to, when the clause states the rule for a vertex |
| `MaskResample` | `:540` | `SM-A1` — which filter resamples a size-mismatched `/SMask` |
| `MinifyFilter` | `:600` | `IM-A1` — how an image drawn smaller than its pixel grid is sampled |
| `CmykJpegPolarity` | `:650` | `DCT-A1` — how a CMYK JPEG with no `/Decode` is read |
| `UnmappableCode` | `:699` | what stands in for a character no `/ToUnicode` covers |
| `ActualTextPrecedence` | `:783` | whether `/ActualText` overrides the glyphs beneath it |
| `MissingAppearanceState` | `:842` | `AS-A1` — which appearance a widget with no `/AS` shows |
| `QuadPointOrder` | `:887` | the two orderings real producers write for `/QuadPoints` |
| `XrefEntryEol` | `:951` | the two-byte end-of-line a classic xref entry uses |
| `TrailingEol` | `:1022` | whether a saved file ends with a newline |

★ **Two of these were missing from this list before 2026-08-25**, and the
shape of the omission is worth more than the correction: the list was written
as prose with inline line numbers, so adding a setting meant editing a
sentence, and `PageBlendSpaceSource` (Pass 122.5) and `MeshPatchPadding`
(Pass 125.0) each landed without one. A table is not merely tidier — a
missing ROW is visible in a way a missing clause in a run-on sentence is not.

Two of these (`UnmappableCode`, `ActualTextPrecedence`) are consumed
directly by `ExtractOptions` (§8.2), and `CmykJpegPolarity` by
`image_codec::decode_image_view_with` (§11.2). The four RENDER-radius ones
(`PageBlendSpaceSource`, `MeshPatchPadding`, `MaskResample`, `MinifyFilter`)
reach the pixels through `pdfcer_render::RenderOptions`' `with_*` builders and
its `policy()` projection, never through a global.

**★ Do not confuse `settings::Settings` with document metadata.** This is
operator configuration — theme, ambiguity-resolution defaults — and has
nothing to do with a PDF's `/Info` dict or XMP. For those, see §12.6.

**UNVERIFIED — the exact `Settings` field list** (the struct spans
`settings/mod.rs:840-1620`). Read it before wiring a preferences dialog; the
enums above are the interesting part, but there are plain scalar fields too.

---

## 14. The traps that cost the most

Ranked by how expensive they are to find from the outside.

1. **★ Tolerance is page space, everywhere, unchecked (T-10.1).** Hit-test
   and snap radii in screen pixels compile, run, and feel *almost* right —
   they just drift with zoom. Nothing in core catches it.
2. **★ `find_text` treats `#` and `?` as wildcards; `find_text_with` +
   `TextSearchOptions::default()` does not (§8.5).** This already shipped a
   real defect in pdfcer's own Find bar. Use `find_text_with`.
3. **★ Base view vs session view (§5.2).** `&doc.view()` and
   `&session.view()` are both valid arguments to the same function and one
   of them silently shows the pre-edit document. *"Not a crash … the
   content parses fine and shows the wrong document."*
4. **`contents_unresolved > 0` and `pages_unreadable > 0` are silent data
   loss unless you surface them** (§6.2, §8.1).
5. **`Dict::get` collapses null, `Dict::len` does not** (§4.2).
6. **`ExtractedGlyph::text_len` is not 1** (T-8.1) and **artifact runs are
   always present in `runs`** (T-8.3).
7. **`FsType::permission() == None` is not "permissive"** (T-9.1), and
   **`EmbeddingPermission` is a value, not a bitmask** (T-9.2).
8. **A recovered document cannot be saved incrementally — check
   `loaded_via_recovery()` before enabling the control** (§3.6).
9. **`None` is not the empty password** (T-3.1), and
   **`PasswordRequiresNormalisation` is not "wrong password"** (T-3.2).
10. **Permissions are advisory; enforcing one silently breaks project
    rule 4** (§3.5).
11. **`Operation::operator_name` returns `None` for inline images**
    (T-7.1) — a very easy `.unwrap()` panic.
12. **Hit-test text per run, not per object bbox** (T-10.4) — the CAD-sheet
    regression.
13. **★ Never apply an "Adobe CMYK inversion" to a JPEG (T-11.5, rule R29),
    and get CCITT `BlackIs1` the right way round (T-11.2)** — both produce
    images that look *deliberate* rather than broken, so neither shows up as
    a bug report.
14. **`apply_view_usage` on a print path violates §8.11.4.5 (T-12.8)**, and
    **`LayerVisibility` replaces rather than merges (T-12.9)** — where
    `None` and `Some(empty)` mean different things.
15. **Attachment encryption is silently invisible (T-12.4)** — the decode
    "succeeds" and returns garbage. Check `may_be_encrypted`.
16. **Annotations carry no `/Dest`/`/A` (T-12.1)**, and **there is no XMP,
    `/Producer` or page-label reader (§12.6)**. Discover these before you
    scope a viewer, not during it.

---

## 15. Stability summary

| Subsystem | Verdict | Basis |
|---|---|---|
| `object`, `span` | **Settled** | initial commit + one additive fix |
| `graph`, `view` | **Settled** | one deliberate change each (`Send + Sync`; decision 018) |
| `page_tree` | **Settled** | two commits |
| `lexer`, `objstm`, `linearization`, `textstring`, `fontdata` | **Frozen** | initial commit only |
| `parser`, `recover` | Settled | two robustness fixes |
| `xref` | Settled | encryption + writer-fidelity work |
| `content` | Settled post-migration | decision 018 changed the signature |
| `crypto` | **Active** | `/R` 5 + AES-256 landed 2026-08-12; `/R` 6 unsupported |
| `text_extract` (`mod`, `font`, `page`) | **Active, additive** | `#[non_exhaustive]` + builder pattern is explicitly there to absorb growth |
| `fontinfo` | **Active** | touched 2026-08-11 |
| `vector::{geometry, centerline}` | Settled | ≤3 commits |
| `vector::snap` | Young but untouched | one commit, whole design |
| `vector::hit` | **Evolving** | tracks the text sub-model |
| `vector::decompose` | **Highest churn** | Pass 30/32 text additions ongoing |
| `vector::linepick` | **Newest** | 2026-08-12; shape may still move |
| `filters` | **Frozen** | initial import only |
| `function` | Young, untouched | landed as one unit (`9e70247`), not iterated |
| `color` | Recent | calibrated CMYK table is a rewrite (`edf7c02`), not legacy |
| `image_codec` | **Active** | most-touched of the four (`fbcb946`, `6d63d81`) |
| `outline`, `attachments` | Settled | shipped together, unrevised since |
| `layers`, `annot` | **Least stable read surface** | decisions 037/038, the `Design`-intent fix, `/AS` usage work — re-verify visibility edge cases |
| `pageops::references` | Moderately active | |
| `settings` | **Frozen** | initial commit only |

Two structural protections make this less alarming than it reads: nearly
every public enum and options struct is `#[non_exhaustive]`, and the crate
follows a builder pattern for options — so the expected direction of change
is **additive**, and a wildcard match arm plus `..Default::default()` will
carry you across most of it.

**Where I do not know:** this crate has never been released and has no
downstream consumers outside this repository (`CLAUDE.md` rule 8), so
"stability" here means *observed churn*, not *a compatibility promise*. No
semver guarantee exists. Pin a commit.

---

## 16. What this document does not cover

- **Mutation of any kind** — `edit`, `EditSession`, `pageops` writes,
  `vector::edit`, `text_edit`, `annot_author`, `forms_author`,
  `font_embed*`, `image_import`, `redact`. → **`02-editing-and-saving.md`**
- **Saving** — `writer`, `Document::save_incremental` (`document.rs:1104`),
  `Document::save_full` (`document.rs:1135`), the round-trip and
  forced-full-rewrite rules (`ARCHITECTURE.md` §5). → **part 2**
- **ce dimensions** (`dimension/`, `dimension::style`,
  `dimension::tolerance`) — the dimension objects **pdfcer authors**, as
  distinct from **pdf dimensions**, which are CAD-exported page content
  pdfcer reads and must not silently alter (`CLAUDE.md` rule 15).
  → **part 2**
- **Forms, OCR, printing, signing, export** (`forms`, `fdf`, `formcsv`,
  `form_script`, `ocr`, `pdfcer-print`, `signature` verification beyond the
  census, `export::dxf`). → **`03-capabilities.md`**
- **Rasterization** — `pdfcer-render` is a separate crate.
  `pdfcer_core` emits a draw-op stream and *"never pixels"* (`lib.rs:7-9`).
  Its `RenderOptions`, `LayerVisibility`, `RenderCancel`, `Diagnostics` and
  the bundled Base-14 substitute faces are part 3.
