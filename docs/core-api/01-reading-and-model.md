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
(`lib.rs`): `clippy::unwrap_used`, `expect_used`, `panic`, and
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
| Check a file is a PDF without parsing it | `pdfcer_core::probe_file(&Path)` / `probe_header(&[u8])` — `lib.rs`, `lib.rs` | §3.1 |
| Load a PDF from disk | `Document::load(&Path)` — `document.rs` | §3.2 |
| Load a PDF from memory | `Document::from_bytes(Vec<u8>)` — `document.rs` | §3.2 |
| Open a password-protected PDF | `Document::load_with_password(&Path, Option<&[u8]>)` — `document.rs` | §3.5 |
| Know whether the open document was encrypted, and how | `Document::encryption() -> Option<&DocumentEncryption>` — `document.rs` | §3.5 |
| Read the author's declared permission bits | `enc.config.permissions()` — `crypto/standard.rs`, then `Permissions::granted(bit)` — `standard.rs` | §3.5 |
| Detect that the file was structurally damaged and rebuilt | `Document::loaded_via_recovery() -> bool` — `document.rs`; detail via `Document::recovery()` — `document.rs` | §3.6 |
| **Open a file that contradicts itself, and see what pdfcer decided** | `Document::from_bytes` (tolerant by default) + `Document::load_anomalies() -> &[LoadAnomaly]`; take the other value with `Document::from_bytes_with_options` or `Document::load_with_options` + `LoadOptions` | §3.6b |
| Warn that saving will destroy Fast Web View | `Document::linearization()` — `document.rs`, then `Linearization::save_invalidates_fast_web_view()` — `linearization.rs` | §3.7 |
| Detect an ISO 32000-2 §7.6.7 encrypted-payload wrapper | `wrapper::detect(&graph) -> WrapperInfo` — `wrapper.rs`; message via `WrapperInfo::message()` — `wrapper.rs` | §3.8 |
| Get the effective PDF version (header + catalog `/Version`) | `Document::version()` — `document.rs` | §3.2 |
| Fetch one indirect object | `Document::get(ObjId) -> Option<&IndirectObject>` — `document.rs` | §4 |
| Follow a reference to a real value | `ObjectGraph::resolve(&Object)` / `::resolved(ObjId)` — `graph.rs`, `graph.rs` | §5 |
| Get the document catalog | `ObjectGraph::catalog_dict() -> Option<&Dict>` — `graph.rs` (or `Document::catalog() -> Result` — `document.rs`) | §5 |
| Iterate every object in the file | `Document::objects()` — `document.rs`; count via `object_count()` — `document.rs` | §4 |
| Read a dictionary key (null-collapsing, per spec) | `Dict::get(&[u8]) -> Option<&Object>` — `object.rs` | §4.2 |
| Get a page list with inheritance resolved | `page_tree::pages(&Document) -> Result<Vec<Page>, _>` — `page_tree.rs` | §6 |
| Do the same over an edit session or any graph | `page_tree::pages_in::<G: ObjectGraph>(&G)` — `page_tree.rs` | §6 |
| Get a page's MediaBox / CropBox / rotation | `Page::media_box`, `::crop_box`, `::rotate` — `page_tree.rs` | §6 |
| Build a read view to pass to render/vector/content | `Document::view() -> DocumentView<'_>` — `document.rs` | §5.2 |
| Decode + tokenize a page's content streams | `ContentStream::from_page(&DocumentView, &Page)` — `content.rs` | §7 |
| Walk content-stream operators semantically | `ContentStream::operations()` — `content.rs`; name via `Operation::operator_name(buf)` — `content.rs` | §7 |
| Extract all text from one page | `text_extract::extract_page(&Document, &Page, idx, &ExtractOptions)` — `text_extract/mod.rs` | §8 |
| Extract all text from the whole document | `text_extract::extract_document(&Document, &ExtractOptions)` — `mod.rs` | §8 |
| Extract text reflecting unsaved edits | `text_extract::extract_page_view` / `extract_document_view` — `mod.rs`, `mod.rs` | §8 |
| Get text as one string | `ExtractedText::plain_text()` — `mod.rs`; file-sourced only: `sourced_text()` — `mod.rs` | §8.3 |
| Get per-glyph positions for a selection highlight | `PageText::runs[].glyphs[]` → `ExtractedGlyph{x,y,advance,size}` — `mod.rs` | §8.4 |
| Get the byte-exact origin of a glyph (for editing) | `ExtractOptions::default().with_provenance(true)` then `ExtractedGlyph::provenance` — `mod.rs`, `mod.rs` | §8.4 |
| Search for text across the document | `EditSession::find_text_with(&needle, &TextSearchOptions)` — `edit.rs` **(read-only in effect, but needs a session)** | §8.5 |
| Search for text **and learn what was unreadable** | `EditSession::search_text(&needle, &TextSearchOptions)` — `edit.rs` → `TextSearch { matches, diagnostics }` | §8.5 |
| Render-setting preset for a subset standard (PDF/X, PDF/A, PDF/UA) | `pdfcer_core::settings::presets::RenderPreset::for_standard(RenderStandard)` | §8.5a |
| Decode a PDF text string (`/Title`, `/Author`, bookmark labels) | `textstring::decode_text_string(&[u8]) -> DecodedText` — `textstring.rs` | §8.6 |
| Inventory every font the document uses | `fontinfo::inventory(&DocumentView) -> FontInventory` — `fontinfo.rs` | §9.1 |
| Know if a font is embedded / subsetted / removable | `FontRecord::program`, `::removability` — `fontinfo.rs`; `split_subset_tag` — `fontinfo.rs` | §9.1 |
| Read a font's embedding permission (`OS/2 fsType`) | `fontinfo::read_fs_type(&[u8])` — `fontinfo.rs` | §9.1 |
| Resolve one font resource for text decoding | `ExtractFont::resolve(&DocumentView, &Dict)` — `text_extract/font.rs` | §9.2 |
| Map a character code to Unicode via `/ToUnicode` | `ToUnicodeCMap::parse(&[u8])` → `::lookup(u32)` — `cmap.rs`, `cmap.rs` | §9.3 |
| Get Base-14 metrics without any font file | `fontdata::std14_width`, `std14_descriptor` — `fontdata/mod.rs` | §9.4 |
| Turn a page into selectable vector/text/image objects | `vector::decompose_page(&DocumentView, &Page, Matrix)` — `vector/decompose.rs` | §10.1 |
| **Find what the user clicked** | **`vector::hit_test_point_deep(&PageObjects, Point, tolerance)` — `vector/hit.rs`** | §10.3 |
| Find what the user clicked, **page stream only** | `vector::hit_test_point(&PageObjects, Point, tolerance)` — `vector/hit.rs` | §10.3 |
| Cycle through overlapping objects under the cursor | `vector::hit_test_point_all` — `vector/hit.rs` | §10.3 |
| Reach objects drawn **inside** a form XObject | `PageObjects::leaves` → `vector::decompose::FormLeaf` — `vector/decompose.rs` | §10.3 |
| **Marquee-select a region** | **`vector::hit_test_rect_deep(&PageObjects, Bounds, MarqueeMode, FormMarquee)`** — `vector/hit.rs`. `hit_test_rect` still exists and is **shallow**: it cannot see inside a form, so a rubber band and a click disagree about what is selectable | §10.3 |
| Drill into which text run / subpath was clicked | `vector::hit_test_text_runs` — `hit.rs`; `vector::hit_test_subpaths` — `hit.rs` | §10.3 |
| Snap a point to geometry | `vector::snap_candidates(Point, &SnapConfig, &PageObjects)` — `vector/snap.rs` | §10.4 |
| Pick a straight edge (CAD-style measuring) | `vector::linepick::pick_line_in_page` — `vector/linepick.rs` | §10.5 |
| Classify two picked edges as parallel/angled | `vector::linepick::classify_two_lines` — `vector/linepick.rs` | §10.5 |
| Decode a stream through its `/Filter` chain | `filters::decode_stream(&Dict, &[u8])` — `filters/mod.rs` | §11.1 |
| Know which image codec a stream ends in, without decoding | `image_codec::terminal_codec(&Dict)` — `pdfcer-image-codec/src/lib.rs` | §11.2 |
| Decode an image XObject to samples | `image_codec::decode_image(&Document, &Dict, &[u8], inline)` — `pdfcer-image-codec/src/lib.rs` | §11.2 |
| Convert a device colour to sRGB | `color::{gray_to_srgb, rgb_to_srgb, cmyk_to_srgb}` — `color/mod.rs` | §11.3 |
| Resolve a full `/ColorSpace` object (Separation, ICCBased, Indexed…) | **Not in `pdfcer-core`** — `pdfcer_render::ColorSpace`, `pdfcer-render/src/color.rs` | §11.3 |
| Evaluate a PDF function (type 0/2/3/4) | `function::PdfFunction::load(&DocumentView, &Object)` then `::eval` / `::eval_into` — `function.rs` | §11.4 |
| Enumerate bookmarks as a tree, pages already resolved | `outline::read_outline(&graph)` — `outline.rs`; flat list `Outline::flatten()` — `outline.rs` | §12.1 |
| List embedded attachments | `attachments::list_attachments_with_notes(&graph)` — `attachments.rs` | §12.2 |
| Extract an attachment's bytes | `attachments::extract_attachment(&DocumentView, &Attachment)` — `attachments.rs` | §12.2 |
| Page labels each page shows (i, ii, 1, A-1) | `page_labels::page_labels(&graph) -> Result<Vec<String>, PageTreeError>` — `page_labels.rs`. Decimal from 1 before the first range or with no tree. Roman spelled with repeated `M` above 3999 and letters to 100 repeats; decimal past either. | §12.4.2 |
| Page-label ranges as stored | `page_labels::label_ranges(&graph) -> Vec<LabelRange>` (`first_page`, `format: LabelFormat { style: LabelStyle, prefix, start: NonZeroU32 }`). Empty with no tree. A `/St` below 1 reads as 1; an unknown `/S` as `LabelStyle::PrefixOnly`. `LabelFormat::label(offset)` formats one page. Set with `EditSession::set_page_labels`. | §12.4.2 |
| List embedded 3D models (U3D/PRC/STEP) | `threed::list_3d_with_notes(&graph)` — `threed.rs` | §12.2 |
| Extract a 3D model's bytes | `threed::extract_3d(&DocumentView, &ThreeDArtwork)` — `threed.rs` | §12.2 |
| Read the view a 3D model opens on (camera, projection) | `threed::default_3d_view(&graph, &ThreeDArtwork)` — `threed/view.rs` | §12.2 |
| Enumerate optional-content layers + default visibility | `layers::read_layers(&graph)` — `layers.rs` | §12.3 |
| Compute hidden layers, correctly for print/export | `annot::optional_content_default_off(&graph)` — `annot.rs` | §12.3 |
| Refine layer visibility for on-screen view only | `annot::apply_view_usage(&graph, …)` — `annot.rs` **(never on a print path — T-12.8)** | §12.3 |
| List annotations on a page with their rects | `annot::page_annotations(&graph, page.id)` — `annot.rs` | §12.4 |
| Make hyperlinks clickable | `annot::page_link_destinations(&graph, page.id, &reader)` — `annot.rs`, with `outline::DestinationReader::new(&graph)` — `outline.rs` built ONCE per document. Returns rect + fully resolved `Destination` per `/Link`. **`Pass 222.0` — this row previously said "no direct API"; that is obsolete.** | §12.4, §12.6.4 |
| Resolve where ONE annotation goes (incl. a `/Widget` pushbutton) | `Annotation::destination(&graph, &reader)` — `annot.rs`. Needs `Annotation::id`; use `page_link_destinations` when completeness matters. | §12.5.6.5 |
| Report dangling cross-references (document health) | `pageops::references::census_dangling` — `pageops/references.rs` ⚠️ **Counts REFERENCES only.** `/ResetForm`, `/SubmitForm` and `/Hide` name their targets by fully-qualified **name string**, and a name is not a reference — so deleting such a field leaves this report at zero while the buttons stop working. `is_empty() == true` is therefore **not** a clean bill of health on its own; pair it with `delete_field`'s `action_targets_orphaned` and `rename_field`'s `action_targets_retargeted` (`Pass 184.0`). | §12.4 |
| Census digital signatures, their byte coverage, and (`Pass 10.1`) their integrity | `signature::census(&graph)` — `signature.rs`; `signature::byte_range_coverage` — `signature.rs`; `signature::verify_all(&graph, bytes)` / `verify(&graph, bytes, index)` — `signature_verify.rs` | §12.5 |
| Read `/Info` title / author / subject / keywords | `EditSession::info_text(InfoField)` — `edit.rs` **(needs a session; only those 4 fields)** | §12.6 |
| Read `/Producer`, `/CreationDate`, XMP, or page labels | **No public reader** — read the raw `/Info` dict via `ObjectGraph` | §12.6 |
| Load / persist user settings | `settings::resolve_store()` — `settings/mod.rs`; `Settings` — `settings/mod.rs` | §13 |

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

- `Rect` (`page_tree.rs`) is **normalised**: `llx ≤ urx`, `lly ≤ ury`
  always, because ISO 32000-1 §7.9.5 permits the two corners in either
  order. Build one with `Rect::from_corners` (`page_tree.rs`), never by
  assigning the raw array positionally.
- `Quad` (`annot_author.rs`) has `ul`/`ur`/`ll`/`lr`. Because y is UP,
  **`ul.1` is the LARGER y** — see `Quad::from_rect` at
  `annot_author.rs`, which sets `ul: (rect.llx, rect.ury)`. A quad is a
  general quadrilateral (`/QuadPoints`, §12.5.6.10), so take bounds over
  all four corners, never from `ll`/`ur` alone.

### 2.2 Page rotation is NOT applied to geometry

`Page::rotate` (`page_tree.rs`) is a display instruction — 0/90/180/270
clockwise. Every geometry value core hands you (`media_box`, `TextRun::bbox`,
`Bounds`, `Point`, snap candidates, hit-test input) is in **unrotated** page
space. Your canvas applies the rotation. Passing a rotation-adjusted point
back into `hit_test_point` will miss.

### 2.3 Page indices

Core is **0-based** everywhere (`PageText::page_index`, `TextMatch::page_index`,
the index into `page_tree::pages()`'s `Vec`). Humans are 1-based, and
`pdfcer` converts at the print boundary (`cmd_find_text` in `crates/pdfcer-cli/src/fields.rs`:
*"1-based page, matching every other page-addressing surface in this CLI.
The extraction is 0-based and the operator is not."*). Do the same, once, at
your presentation layer.

### 2.4 Error style

Every error type is `thiserror`-derived and **`#[non_exhaustive]`**. Your
`match` arms **must** carry a wildcard, and new variants will arrive. This is
deliberate (`lib.rs`) — treat a wildcard arm as "an unexpected
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
(`document.rs`). A GUI calls `Document::load*`, matches on `DocError`,
and then reads four accessors: `encryption()`, `recovery()`,
`linearization()`, `version()`.

### 3.1 Cheap probe (no parse)

```rust
use pdfcer_core::{probe_file, probe_header, PdfVersion, HEADER_SCAN_WINDOW};

let v: PdfVersion = probe_file(std::path::Path::new("in.pdf"))?;  // lib.rs
println!("declares {v}");                                          // Display -> "1.7"
```

Reads **at most `HEADER_SCAN_WINDOW` = 1024 bytes** (`lib.rs`), so it is
safe on a hostile multi-gigabyte file. The 1024-byte tolerance for a
leading BOM/whitespace is **empirical practice, not spec** — `lib.rs`
says so explicitly and records that an earlier revision miscited it.

Use this for a file-picker filter or a drag-and-drop hover check. It says
only *"looks like a PDF, declares M.N"* — nothing about whether it opens.

### 3.2 The real load

```rust
use pdfcer_core::document::{Document, DocError};

let doc = Document::load(std::path::Path::new("in.pdf"))?;   // document.rs
let version = doc.version();                                  // document.rs
let n_objects = doc.object_count();                           // document.rs
```

`Document::load` = `from_bytes(std::fs::read(path)?)` (`document.rs`).
`Document` **owns the complete source bytes for its lifetime**
(`document.rs`) — this is the provenance substrate that makes
minimal-diff saving possible, and it means a loaded `Document`'s memory
cost is at least the file size. Budget for it; do not hold twenty open.

`version()` reconciles the header against the catalog's `/Version` and
returns the **max** of the two (§7.5.5) — an incremental update can raise
the version without touching byte 0.

### 3.3 The load pipeline (what actually happens, in order)

Useful for mapping an error back to a stage and for a progress UI.

1. **Header probe** — `probe_header` — called at `document.rs`.
2. **Strict xref chain load** — `xref::load_xref_chain` — `document.rs`,
   defined `xref.rs`. Walks `/Prev`, cycle-guarded.
3. **`Document::assemble`** — `document.rs`, defined `document.rs`:
   1. **Phase 1** — eagerly parse every in-use file-level object
      (`document.rs`).
   2. **Phase 1.5** — decrypt in place (`document.rs`, defined
      `document.rs`).
   3. **Phase 2** — inflate object streams and parse compressed objects
      (`document.rs`, defined `document.rs`).
   4. **Linearization detect** — `linearization::detect` — `document.rs`.
4. **On xref failure of a recoverable kind** → rebuild-by-scan recovery
   (`recover::recover`, `document.rs`) then the same assemble.
5. **On header-probe failure** → recovery is still attempted
   (`document.rs`), succeeding only if the scan finds objects **and** a
   `/Catalog`; otherwise the original "not a PDF" error stands.

The load is **eager and strict**: every in-use object is parsed before
`load` returns. There is no lazy-object mode. For a large file this is
your dominant open cost — put it on a worker thread (see §5.3, the
`Send + Sync` note).

### 3.4 `DocError` — the complete table

`document.rs`, `#[non_exhaustive]`.

| Variant | Meaning | Your action |
|---|---|---|
| `Io(io::Error)` | file unreadable | report; not recoverable |
| `Encryption(EncryptionUnsupported)` | encrypted with a config pdfcer refuses (e.g. `/R` 6) | **no password will help** — show the specific reason |
| `PasswordRequired` | decryptable in principle; empty password already tried silently and failed | **prompt and retry** |
| `PasswordRequiresNormalisation` | `/R` 5 + non-ASCII password; pdfcer does not implement SASLprep so a *correct* password can be rejected | **do NOT say "wrong password"** — say pdfcer cannot verify this one |
| `Header(PdfError)` | not a PDF, and recovery also found nothing | report "not a PDF" |
| `Xref(XrefError)` | unrecoverable xref failure, or the encrypted-and-damaged case | not recoverable |
| `BadObject{id,offset,source}` | an xref-declared object failed to parse | not recoverable |
| `ObjectIdMismatch{expected,found,offset}` | table and body disagree | not recoverable |
| `ObjectStreamMissing{container,num}` | type-2 entry names an absent container | not recoverable |
| `ObjectStream{container,source}` | container present but undecodable | not recoverable |
| `ObjectStreamIdMismatch{…}` | pair table contradicts the xref | not recoverable |
| `NoCatalog` | trailer `/Root` missing or not a dict | not recoverable |
| `Recovery(RecoverError)` | recovery was attempted and failed cleanly | not recoverable — never a partial document |

Nested enums you may want to surface verbatim: `xref::XrefErrorKind`
(`xref.rs`, 12 variants), `parser::ParseErrorKind` (`parser.rs`, 11
variants), `lexer::LexErrorKind` (`lexer.rs`, 9), `objstm::ObjStmError`
(`objstm.rs`, 12), `recover::RecoverError` (`recover.rs`, 4),
`crypto::EncryptionUnsupported` (`crypto/standard.rs`, 7). All are
`#[non_exhaustive]`. Their `Display` strings are written to be shown to an
operator — prefer printing them over re-wording them.

### 3.5 Encryption: driving a password prompt

```rust
use pdfcer_core::document::{Document, DocError};
use pdfcer_core::crypto::AuthKind;

// ★ `None` is NOT the empty password. It means "no password known".
//   §7.6.3.1 requires trying the empty user password first and silently
//   in either case, so `None` still opens a permissions-only document
//   with no prompt (document.rs).
let doc = match Document::load_with_password(path, None) {          // document.rs
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
if let Some(enc) = doc.encryption() {                    // document.rs
    match enc.auth {                                      // document.rs
        AuthKind::EmptyUser => { /* no prompt was needed */ }
        AuthKind::User      => { /* user password: /P-limited access */ }
        AuthKind::Owner     => { /* owner password: full access, /P advisory */ }
    }
    let perms = enc.config.permissions();                 // crypto/standard.rs
    let can_print = perms.granted(pdfcer_core::crypto::PermissionBit::Print); // standard.rs
    // `granted` returns Option<bool>: None == "not applicable at this /R".
    let _ = enc.perms; // PermsCheck — Algorithm 3.13 verdict; see below.
}
```

Three things a shell gets wrong here:

- **Permissions are a disclosure, not a gate.** §7.6.3.1 states plainly
  that *"there is nothing inherent in PDF encryption that enforces the
  document permissions"* — quoted at `document.rs`. `permissions()`
  returns the dictionary's declared `/P`, and pdfcer never substitutes the
  decrypted copy. If you choose to grey out a button because of a
  permission bit, project rule 4 requires you to **say that you did**.
- **`PermsCheck::NotApplicable` is the ordinary answer for every `/R` ≤ 4
  document**, not a failed check. `document.rs` says so and warns
  a front end must not render it as one.
- **`AuthKind::EmptyUser` vs `Some(b"")`.** `None` means no password known;
  `Some(b"")` means the operator explicitly submitted an empty box. Same
  key, different `AuthKind`, and shells use it to know whether a prompt was
  ever shown (`crypto/standard.rs`).

### 3.6 ★ Recovery detection — check this before offering "Save"

```rust
if doc.loaded_via_recovery() {                       // document.rs
    let r = doc.recovery().expect("just checked");    // document.rs -> &RecoveryReport
    // recover.rs — reason, file_level_objects, objstm_objects,
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
the writer refuses, `document.rs`). Both accessors are `const fn`
— free to call, so gate your save UI on them rather than on catching the
refusal.

`RecoveryReport` is a *counted* disclosure by design (project rule 4,
"fuzzy, never sneaky"): show the counts, do not summarise them to "the file
was repaired".


### 3.6b A file that contradicts itself OPENS, and says what pdfcer decided (`Pass 283.0`)

**The operator ruling this implements**, verbatim:

> *"We should be making pdfcer so that it opens pdfs that have errors, and have
> a way that it manages those errors such that they aren't fatal, and if the
> user can intervene in a decision that should always be an option along with
> them not having to intervene."*

Three obligations, and the API carries all three:

```rust
let doc = Document::from_bytes(bytes)?;              // 1. NOT FATAL
for a in doc.load_anomalies() {                      // 2. what was decided
    // LoadAnomaly::DuplicateDictKey { object, key, kept, discarded }
    // LoadAnomaly::StreamLengthRecovered { object }
    // LoadAnomaly::MissingEndobjRecovered { object }
}
// 3. the operator takes the other value — re-load, do not patch
let other = Document::from_bytes_with_options(
    bytes,
    None,
    LoadOptions::new().with_duplicate_keys(DuplicateKeyPolicy::KeepFirst),
)?;
```

**What changed.** Three malformations that used to cost the whole document are
now decided and recorded: a dictionary naming one key twice (§7.3.7), a stream
whose `/Length` is absent or unusable (§7.3.8.2), and an object with no
`endobj` (§7.3.10). Each was already recoverable — the last two by policies
that existed and were reachable only from the cross-reference-recovery path.

**Why a policy and not a fix.** §7.3.7 *is* a genuine `shall not` — *"Multiple
entries in the same dictionary shall not have the same key"*, identical in both
editions — but it binds the **file**. §2.1/§2.3 make conformance a property of
files and writers; clause 1 excludes validation methods. ISO 32000 therefore
neither obliges pdfcer to render such a file nor obliges it to refuse, and
`pdf-issues` #199 (open since 2022) says so directly: *"beyond the scope of
ISO 32000."* Which value wins is pdfcer's to choose **and to disclose**.

**Keep-last, on evidence.** qpdf, pdf.js and pdfium all keep the last
occurrence; none refuses. qpdf even warns in the same terms pdfcer now does.
★ This is **not** argued from §7.5.6's incremental-update ordering — that
ordering the standard makes meaningful, where §7.3.7's preceding sentence says
the ordering of a dictionary's entries *"shall be ignored"*. ISO's one resolved
duplicate-key erratum (#3, inline images) picked its winner by **content** and
rejected positional logic by name.

**Refusing the whole document was the strict reading plus two unstated
escalations** — dictionary → object → document — neither of which §7.3.7
authorises.

| you want | call |
|---|---|
| open a damaged file (the default) | `Document::from_bytes` |
| see what pdfcer decided | `Document::load_anomalies() -> &[LoadAnomaly]` |
| take the other value | `Document::from_bytes_with_options` + `LoadOptions::with_duplicate_keys` |
| …from a **path**, which is how shells open files | `Document::load_with_options(path, password, options)` |
| refuse malformed files instead | `LoadOptions::strict()` |

★ **Both entry points carry the options, and the second one is why.** `Pass
283.0` shipped the alternative reading on the **bytes** form only, and every
shell opens a **path** — the GUI's open-file action, `pdfcer`'s path argument.
Reaching the intervention therefore meant re-implementing `Document::load`'s
`std::fs::read` at each call site. That is **R245**'s shape — a facility
present on one route and absent on its twin — applied to an *affordance*
rather than a guard: the route the intended caller actually uses did not have
it. An intervention only reachable by rewriting the route beside it is
present, not offered.

★ **`LoadAnomaly::DuplicateDictKey` carries BOTH values, not a count.** A count
says pdfcer chose; only the pair lets a shell show the operator what it chose
between. Without that the intervention is theoretical.

★ **The alternative is taken by RE-LOADING, not by patching.** A decision made
during parsing is not a value that can be edited afterwards — the discarded one
was never built into the document. Carrying both would make every dictionary
lookup ambiguous for the life of the session to serve a case that is one
re-read away.

★ **`LoadOptions::default()` is NOT the derived default.**
`DuplicateKeyPolicy::default()` is `Refuse` — right for a parser, wrong for a
loader. The parser stays strict for every caller that constructs one directly
(fuzz targets, the recovery confirmation pass); the **loader** opts in.

CLI: `pdfcer --on-malformed keep-last|keep-first|refuse`, and `inspect` prints
every decision.

### 3.7 Linearization

```rust
use pdfcer_core::linearization::Linearization;
match doc.linearization() {                                    // document.rs
    Linearization::None => {}
    l => if l.save_invalidates_fast_web_view() {               // linearization.rs
        warn("saving will remove Fast Web View");
    }
}
```

`Linearization` (`linearization.rs`) is `None | Live{declared_length} |
Stale{declared_length, actual_length}`. pdfcer **never repairs it and never
strips a stale `/Linearized` dictionary** (`document.rs`).
`linearization::detect(&[u8])` (`linearization.rs`) is infallible.

### 3.8 Encrypted-payload wrapper (§7.6.7)

```rust
use pdfcer_core::wrapper;
let info = wrapper::detect(&doc);                    // wrapper.rs, takes any &G: ObjectGraph
if let Some(msg) = info.message() {                   // wrapper.rs
    show_banner(&msg);   // "the visible page is a cover sheet"
}
```

Cheap enough to run on **every** open (`wrapper.rs`: *"a detector an
operator has to remember to run is a detector that does not fire on the day
it matters"*). `WrapperInfo{is_wrapper, payload_name, payload_count}` —
`wrapper.rs`.

### 3.9 Resource limits (guards you must not remove)

All are pdfcer policy per `ARCHITECTURE.md` §10.1 unless noted.

| Constant | Value | Guards |
|---|---|---|
| `HEADER_SCAN_WINDOW` | 1024 B | header scan |
| `document::MAX_RESOLVE_DEPTH` | 32 | reference cycles → `Object::Null`, not an error |
| `lexer::MAX_TOKEN_LEN` | 1 MiB | unbounded token |
| `lexer::MAX_STRING_LEN` | 16 MiB | unbounded string |
| `parser::MAX_NESTING_DEPTH` | 256 | `[[[[…` stack bomb |
| `xref::STARTXREF_SCAN_WINDOW` | 4096 B | trailing scan |
| `xref::MAX_XREF_SECTIONS` | 1024 | `/Prev` cycle |
| `xref::MAX_XREF_ENTRIES` | 10,000,000 | table size |
| `xref::MAX_W_FIELD_WIDTH` | 8 | xref-stream `/W` |
| `xref::MAX_XREF_STREAM_ROW` | 32 | xref-stream row |
| `objstm::MAX_OBJSTM_OBJECTS` | 1,000,000 | `/N` before allocation |
| `filters::MAX_DECODED_LEN` | 256 MiB | decompression bomb, enforced **incrementally** |
| `page_tree::MAX_TREE_DEPTH` | 64 | page-tree nesting |
| `page_tree::MAX_PAGES` | 1,000,000 | page count |
| `crypto::r5::MAX_PASSWORD_LEN` | 127 B | `/R` 5 truncation (spec) |
| `linearization::LINEARIZATION_SCAN_WINDOW` | 1024 B | **spec-mandated** (Annex F.3.3), not policy |

### 3.10 Traps — loading

- **T-3.1 `None` ≠ empty password.** `document.rs`. Passing
  `Some(b"")` when you meant "user hasn't typed anything" changes the
  reported `AuthKind` and therefore your UI's story.
- **T-3.2 `PasswordRequiresNormalisation` must not be shown as "wrong
  password".** `document.rs`: doing so *"would send the operator to
  re-check a password that was correct."*
- **T-3.3 A damaged **and** encrypted file reports as
  `DocError::Xref(XrefErrorKind::EncryptionUnsupported)`, not as a recovery
  error** (`document.rs`). Do not route it into your "file damaged"
  branch.
- **T-3.4 Object-level failures never trigger recovery.** `recover.rs`:
  recovery is scoped strictly to the xref-parse stage, so `BadObject`,
  `ObjectIdMismatch`, `ObjectStream*` after a clean xref load are terminal.
  Documented limitation, not an oversight — do not build a "try harder"
  retry on top.
- **T-3.5 `Document` retains the whole file buffer.** `document.rs`.
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
(`EncryptionUnsupported::UnsourcedRevision`, `crypto/standard.rs`).
Expect new `DocError`/`EncryptionUnsupported` variants; the
`#[non_exhaustive]` attributes already protect your match arms.

---

## 4. The object model

**Module set:** `object`, `span`.

### 4.1 `Object` — the COS value

`object.rs`, `#[non_exhaustive]`:

```
Null | Boolean(bool) | Integer(i64) | Real(f64) | String(Vec<u8>)
| Name(Name) | Array(Vec<Object>) | Dict(Dict) | Stream(Stream)
| Reference(ObjId)
```

Accessors, all `Option`-returning (`object.rs`): `as_int`,
`as_number` (widens `Integer` to `f64` per §7.3.3 NOTE 2 — **use this, not
`as_int`, for anything a producer may write either way**), `as_name`,
`as_dict`, `as_array`, `as_reference`.

`Integer` and `Real` are deliberately distinct. `String(Vec<u8>)` is **raw
bytes**, escapes already applied — interpreting it as *text* is
`textstring::decode_text_string`'s job (§8.6), never `String::from_utf8`.

### 4.2 `Dict` — and its one spec-driven surprise

`object.rs`: `pub struct Dict(pub Vec<(Name, Object)>)` — an ordered
`Vec`, not a hash map, so parsed entry order is preserved for minimal-diff
re-emission.

**★ `Dict::get` collapses null.** `object.rs`: an entry whose value is
`Object::Null` returns `None`, because §7.3.7/§7.3.9 make a null-valued
entry identical to an absent one. This is implemented once so no call site
needs a second null check. Consequently `Dict::len()` (`object.rs`) is
the **physical** count including explicit nulls and may exceed the number of
keys `get` will answer for. Use `len` only for serialisation; use `get` /
`contains_key` for semantics.

`Name` (`object.rs`) stores the **decoded** bytes with `#`-escapes
expanded, so `/Type` and `/Ty#70e` hash and compare equal. Names are raw
bytes, not guaranteed UTF-8. Look keys up with byte literals:
`dict.get(b"MediaBox")`.

### 4.3 `IndirectObject`, `ObjId`, `Provenance`

- `ObjId{num: u32, generation: u16}` — `object.rs`. `Display` renders
  `"num gen"`.
- `IndirectObject{id, value, provenance}` — `object.rs`.
- `Provenance` — `object.rs`, `#[non_exhaustive]`:
  `File(ByteSpan)` | `RecoveredFile(ByteSpan)` | `ObjectStream{container, index}`.

**★ `Provenance::file_span()` and `is_verbatim_safe()` answer different
questions.** `object.rs` returns `Some` for both `File` and
`RecoveredFile` — the bytes genuinely exist and a UI showing "where is this
object defined" is right to ask. `object.rs` (`is_verbatim_safe`) is
`true` only for `File`. The doc comment states the reason: testing
`file_span().is_some()` *"would silently start copying self-contradictory
bytes the day the third variant appeared."* Read-only shells only need
`file_span`; do not repurpose it as a save-safety test.

### 4.4 `ByteSpan`

`span.rs`: `{start: usize, len: usize}`, `Copy`, ordered, hashable.
Methods: `new`, `from_range`, `end`, `range`,
`slice(&[u8]) -> Option<&[u8]>`.

`slice` returning `None` *"always indicates a logic error (a span applied to
a buffer it wasn't produced from)"* — surfaced as `Option` rather than a
panic per the crate policy. If you see `None`, you mixed up buffers (very
likely base vs. session — see §5.2). `from_range` on an inverted range
degrades to a zero-length span rather than panicking (`span.rs`).

### 4.5 Worked sequence — read an arbitrary catalog key

```rust
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;

let catalog = doc.catalog_dict().ok_or("no catalog")?;      // graph.rs
// /PageLayout is a name; /OpenAction may be an array or a dict.
let layout = catalog.get(b"PageLayout")                      // object.rs
    .map(|o| doc.resolve(o))                                 // graph.rs
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

`graph.rs`:

```rust
pub trait ObjectGraph: Send + Sync {
    fn value(&self, id: ObjId) -> Option<&Object>;              //  REQUIRED
    fn trailer_entry(&self, key: &[u8]) -> Option<&Object>;     //  REQUIRED
    fn resolve<'a>(&'a self, obj: &'a Object) -> &'a Object;    //  provided
    fn resolved(&self, id: ObjId) -> &Object;                   //  provided
    fn catalog_dict(&self) -> Option<&Dict>;                    //  provided
    fn catalog_id(&self) -> Option<ObjId>;                      //  provided
}
```

Implementors: `Document` (`graph.rs`), `DocumentView<'_>`
(`view.rs`), and `EditSession`'s overlay views. The trait exists so
there is exactly **one** page-tree walk, one outline walk, one copier —
correct for both the file-as-loaded and the file-as-edited
(`graph.rs`). The §7.3.10 resolution rules (dangling → null, cycle
depth-guarded) live in the provided methods so no view can get them subtly
different (`graph.rs`).

**Which do you pass?** If a function takes `&G: ObjectGraph`, pass
`&doc` for the base file or `&session` for edited state. If it takes
`&DocumentView`, see §5.2 — that choice is load-bearing.

`Document` also has inherent `resolve` (`document.rs`) and `catalog`
(`document.rs`, `Result`-returning) with identical semantics; inherent
methods win method resolution, so both spellings work.

### 5.2 ★ `DocumentView` — and the base-vs-session trap

`view.rs`. Built by `Document::view()` (`document.rs`) or
`EditSession::view()` (`edit.rs`). It bundles three things: a
`&dyn ObjectGraph`, a **byte source**, and the version.

The byte source is the point. `StreamSource` (`view.rs`) is
`Contiguous(&[u8])` | `Split{base, staged}`. A `Document` has one buffer;
an `EditSession` has two, because content rewritten this session lives in a
staging buffer. So:

```rust
let v = doc.view();          // base revision — the file as it is on disk
let v = session.view();      // edited state — what the operator is looking at
```

`content.rs` states the consequence bluntly: *"Getting this wrong is
not a crash, it is the Pass 17.0 defect: the content parses fine and shows
the wrong document."* Every function in §7–§10 that takes a
`&DocumentView` inherits this choice.

Accessors: `graph()`, `source()`, `slice(ByteSpan)`,
`bytes() -> Option<&[u8]>`, `version()`.

**★ Use `view.slice(span)`, never `span.slice(doc.bytes())`.**
`DocumentView::bytes()` returns `Option` and is `None` for a split
(session) view (`view.rs`) — deliberately, because *"any answer
other than 'there isn't one' would be the X5 mis-slice hazard wearing a
plausible face."* If you find yourself unwrapping `bytes()`, you are about
to read a session's authored appearance streams off the end of the base
buffer.

`Document::view()` is cheap — *"two borrows plus a version probe. Building
one per call is the intended usage; there is nothing to cache"*
(`document.rs`).

### 5.3 Threading

`ObjectGraph: Send + Sync` (added 2026-08-07, `e4256f2`) exists specifically
so **a page can be rasterized off the UI thread** (`graph.rs`). The
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

let pages: Vec<Page> = page_tree::pages(&doc)?;        // page_tree.rs
// Generic over any graph — use for an EditSession:
let pages = page_tree::pages_in(&session)?;            // page_tree.rs
```

**★ `pages(&doc)` is the base revision, not the edited state.**
`page_tree.rs` flags this with a warning marker: anything that must
see unsaved structural edits calls `EditSession::pages()` (`edit.rs`),
which walks the overlay through the same code. After a page delete, a base
walk still returns the deleted page.

Also available: `page_slots(&G) -> Vec<PageSlot>` (`page_tree.rs`) with
`PageSlot` and `InheritedRaw` — the unresolved view, for
writers that need to know where an attribute physically lives. A read-only
GUI wants `pages`/`pages_in`.

### 6.2 `Page` — `page_tree.rs`

| Field | Type | Notes |
|---|---|---|
| `id` | `ObjId` | always known (pages are reached via indirect `Kids`) |
| `resources` | `Dict` | resolved: own, inherited, explicit empty, or **defaulted** — see the next row |
| `resources_defaulted` | `bool` | **see below** (`Pass 290.0`) |
| `media_box` | `Rect` | normalised, user space, points |
| `crop_box` | `Rect` | the **effective** crop box: the written one ∩ `media_box` (ISO 32000-2 §14.11.2.1), defaulting to `media_box`; **this is what you clip display to** (Table 30). Do not intersect it again |
| `crop_box_resolution` | `BoxResolution` | `Defaulted` / `AsWritten` / `Clipped` / `Unusable` (no overlap with the media box → `media_box` used; ambiguity PB-A1). Report `Clipped`/`Unusable` off-canvas |
| `bleed_box`, `trim_box`, `art_box` | `Rect` | the page's **own** entry (not inheritable) ∩ `media_box`, defaulting to the effective `crop_box`. Not clipped to the crop box: the spec gives the crop box "no defined relationship" with them |
| `bleed_box_resolution`, `trim_box_resolution`, `art_box_resolution` | `BoxResolution` | as above; `Unusable` also covers a malformed array, which (unlike a malformed `/CropBox`) does not fail the page |
| `rotate` | `u16` | 0/90/180/270 clockwise, display only — see §2.2 |
| `contents` | `Vec<ObjId>` | in order; concatenate. Empty = empty page, **not** an error |
| `contents_unresolved` | `usize` | **see below** |
| `contents_flattened` | `usize` | nested `/Contents` arrays flattened on the way in (damage a pre-`Pass 111.0` pdfcer wrote) |

**Building one (fixtures): `Page::with_boxes(id, media_box, crop_box, rotate) -> Page`.**
The page `pages` resolves for a page that writes no bleed/trim/art box: those
three equal `crop_box`, every `*_resolution` is `Defaulted`, `resources` empty,
`contents` empty, counters zero. `rotate` is normalised as the walk does
(`630` → `270`; not a multiple of 90 → `0`). `crop_box` is taken as given — pass
the effective (already clipped) box. Override other fields with struct-update
syntax, `Page { contents, ..Page::with_boxes(..) }`; a field added later gets a
default here, so such fixtures keep compiling. `Page` is deliberately **not**
`#[non_exhaustive]`: that would forbid struct-update syntax outside the crate.

**★ `contents_unresolved` is a count you must surface.** `page_tree.rs`:
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

**★★ `resources_defaulted` is the same obligation one attribute along
(`Pass 290.0`).** `true` means `/Resources` was on neither the page nor any
ancestor, so `resources` is an empty dictionary **pdfcer supplied**. Until
`Pass 290.0` that case returned `MissingRequired("Resources")` — and because
the walk returns one `Result` for the whole tree, one such page cost the
caller EVERY page of the document. Acrobat writes such pages (a blank spacer
in a user-created stamp collection), and so did `fixtures/synthetic/minimal.pdf`.

What to do with it: report it off-canvas, never as a mark on the page (rule
4). When the flag is `true` and the page has content, expect a page-wide
scatter of resource-name failures — `fonts_unsupported`, `cs_unresolved`,
unpainted form XObjects — all with this one cause. Do **not** tell the
operator a contentless page is automatically harmless: §7.8.3 lets a form
XObject, including an annotation `/AP` stream (per ISO 32000-2's erratum),
inherit the page's resource dictionary.

The file is genuinely non-conforming — ISO considered conditioning
`/Resources` on `/Contents` and decided against it (`pdf-issues` #81, ISO
approved) — so say so if you are validating. Just do not charge the operator
the document for it: §2.2 scopes a reader's rendering duty to *conforming*
files and §1 puts conformance validation outside the standard's scope, so
refusal was a choice, never an obligation.

Same boundary as `/Contents`: a `/Resources` that is present and is not a
dictionary is a hard `PageTreeError::BadResources`. Absent, or a reference
that dangles (§7.3.10 + §7.3.9), degrades.

### 6.3 `Rect` — `page_tree.rs`

`{llx, lly, urx, ury}: f64`, **always normalised** (min,min)→(max,max)
because §7.9.5 allows the corners in either order. Construct via
`Rect::from_corners(x1,y1,x2,y2)`. `width()` and `height()`
 are non-negative by construction.

### 6.4 `PageTreeError` — `page_tree.rs`, `#[non_exhaustive]`

`NoPageTreeRoot` · `BadKid(ObjId)` · `Cycle(ObjId)` ·
`TooDeep` · `TooManyPages` · `MissingRequired(&'static str)`
 · `BadResources` · `BadRectangle(&'static str)` ·
`BadRotate(i64)` · `BadContents`. **Ten variants.**

★ `MissingRequired` means `MediaBox` and nothing else since `Pass 290.0`.
An absent `/Resources` no longer fails the page — see `resources_defaulted`
below — because Table 30 itself names the empty dictionary as the value for
*"the page requires no resources"*, while no clause anywhere names a default
media box. `BadResources` is the narrow survivor: `/Resources` present and
not a dictionary.

A well-formed empty tree (`/Count 0`, empty `Kids`) returns an **empty
vec, not an error** (`page_tree.rs`).

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
  edited state (`page_tree.rs`).
- **T-6.2 Clip to `crop_box`, size from `crop_box`, not `media_box`.**
  Table 30; `page_tree.rs`.
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
let cs = ContentStream::from_page(&doc.view(), &page)?;   // content.rs
// Or tokenize bytes you already have:
let cs = ContentStream::parse(decoded_bytes)?;            // content.rs

for op in cs.operations() {                                // content.rs
    match op.operator_name(&cs.buf) {                      // content.rs
        Some(b"Tj") | Some(b"TJ") => { /* operands in op.operands */ }
        Some(name) => { /* other operator */ }
        None => { /* ★ an inline image (BI…EI) — NOT an operator */ }
    }
}
```

`ContentStream{buf: Vec<u8>, tokens: Vec<ContentToken>}` — `content.rs`.
`buf` is the **decoded, concatenated** content; every `ContentToken::span`
indexes into it (not into the file). `ContentTokenKind` (`content.rs`) is
`Operand(Object)` | `Operator` | `InlineImage{params, data}`.

Multiple `/Contents` streams are joined with a single LF (`content.rs`),
because §7.7.3.3 guarantees the split falls on a token boundary but not that
the boundary carries whitespace.

`ContentError` — `content.rs`, `#[non_exhaustive]`: `Lex`, `BadOperand`,
`TooDeep`, `BadInlineParams`, `UnterminatedInlineImage`, `Decode`,
`NotAStream`.

### 7.2 Traps — content

- **T-7.1 `operator_name` returns `None` for an inline image**, not
  `b"BI"` (`content.rs`). `.unwrap()` here panics on any page with
  an inline image.
- **T-7.2 `operations()` silently drops trailing operands with no
  operator** (`content.rs`) — the tolerance every real viewer
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
`text_extract` and `text_state` live in `crates/pdfcer-text/src/`, re-exported
at their `pdfcer_core::` paths; line references in this section are into that crate.

**★ Structural fact:** `text_extract/mod.rs` declares
`mod layout;` and `mod page;` — **private**. Everything in `page.rs` and
`layout.rs` is internal. `pub mod cmap;` and `pub mod font;`
 are public. Do not attempt to build against
`text_extract::page::*` or `::layout::*`; they do not exist outside the
crate.

### 8.1 The six entry points

```rust
// text_extract/mod.rs
pub fn extract_page(doc: &Document, page: &Page, page_index: usize,
                    options: &ExtractOptions) -> Result<PageText, ExtractError>;      //
pub fn extract_page_view(doc: &DocumentView<'_>, page: &Page, page_index: usize,
                    options: &ExtractOptions) -> Result<PageText, ExtractError>;      //
pub fn extract_document(doc: &Document,
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; //
pub fn extract_document_view(doc: &DocumentView<'_>,
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; //
pub fn extract_pages(doc: &Document, indices: &[usize],
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; //
pub fn extract_pages_view(doc: &DocumentView<'_>, indices: &[usize],
                    options: &ExtractOptions) -> Result<ExtractedText, ExtractError>; //
```

The `_view` variants exist for the base-vs-session choice (§5.2). Use them
for anything reflecting unsaved edits.

`ExtractError` — `mod.rs`: `PageTree(PageTreeError)`,
`NoSuchPage{index, count}`, `Content(ContentError)`.

**★ The plural and singular forms have different failure semantics.**
`extract_document*` / `extract_pages*` **swallow** a per-page content
failure: the page becomes an empty-`runs` `PageText` and the failure is
counted in `TextDiagnostics::pages_unreadable` (`mod.rs`).
`extract_page` / `extract_page_view` **propagate** `ExtractError::Content`
for the one page requested (`mod.rs`). A whole-document extract
that "worked" may therefore have lost pages — check the diagnostic.

### 8.2 `ExtractOptions` — `mod.rs`, `#[non_exhaustive]`, builder-style

| Field | Default | Note |
|---|---|---|
| `include_artifacts` | `false` | policy, not conformance — §14.8.2.2 requires nothing |
| `word_gap_ratio` | `0.20` | derived word space |
| `line_gap_ratio` | `0.30` | derived line break |
| `backward_jump_ratio` | `0.50` | two-column detection |
| `max_form_depth` | `64` | corpus-corrected; a conformant PDF/A file has a 32-deep chain |
| `capture_provenance` | `false` | **must opt in** for per-glyph provenance |
| `unmappable_code` | `ReplacementChar` | the sentinel for an unmappable code |
| `actual_text` | `Always` | whether `/ActualText` replaces glyph-derived characters |

**★ The three gap ratios have zero spec basis** (`mod.rs`, negative
results S3/S4). If you expose them as settings, label them as heuristics,
not conformance knobs. The last two are `settings::UnmappableCode` /
`settings::ActualTextPrecedence` — spec ambiguities deliberately made
settings per the operator's standing directive (R169), not hard-coded.

### 8.3 Getting a string out

```rust
use pdfcer_core::text_extract::{self, ExtractOptions};

let opts = ExtractOptions::default();
let all = text_extract::extract_document(&doc, &opts)?;   // mod.rs
let s = all.plain_text();      // mod.rs — sourced chars + derived whitespace
let s2 = all.sourced_text();   // mod.rs — ONLY characters the file actually contains
```

**★ Pages are joined by U+000C (form feed), never `\n`** (`mod.rs`).
Splitting on `\n` will not separate pages and may merge one page's last line
with the next page's first.

**★ `sourced_text()` is not readable prose.** Line breaks are *always*
derived, even in Tagged PDF (`mod.rs`, negative result S5), so
`sourced_text` for a two-line file is `"HelloworldSecond line"` with no
separator. Use `plain_text()` for anything a human reads;
`sourced_text()` only when you must prove a character came from the file.

### 8.4 Positioned runs and glyphs — what a highlight needs

```rust
use pdfcer_core::text_extract::{self, ExtractOptions, TextOrigin};

let opts = ExtractOptions::default().with_provenance(true);   // mod.rs
let all = text_extract::extract_document(&doc, &opts)?;

for page in &all.pages {                       // Vec<PageText>            mod.rs
    for run in &page.runs {                    // Vec<TextRun>, content order  mod.rs
        if run.artifact.is_some() { continue; }         // ★ see T-8.3
        let bbox = run.bbox;                            // Option<Rect>, USER SPACE, f64
        if run.origin == TextOrigin::Glyphs {
            for g in &run.glyphs {                       // ExtractedGlyph     mod.rs
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

`TextRun`: `text`, `origin`, `glyphs`, `artifact`, `mcid`, `mcid_stream`
(which content stream's MCID namespace `mcid` belongs to),
`artifact_subtype: Option<ArtifactSubtype>` (`Header` | `Footer` |
`Watermark` | `Other(name)`, Table 363 — how to tell a running head from
a running foot),
`bbox: Option<Rect>`; method `direction() -> (f32, f32)`.
★ `text` is **not** one `char` per glyph and the run is **not** one show
operator — see §8.4.0 before building anything that locates an edit from a
run.
`TextOrigin`: `Glyphs` | `ActualText` | `DerivedWordSpace` |
`DerivedLineBreak`. Only `Glyphs` runs have glyphs.
`ExtractedGlyph`: `code`, `rung`, `text_start`, `text_len`,
`x`, `y`, `advance`, `size`, **`direction: (f32, f32)`**, `invisible`,
`weight`, `provenance`; methods `up()`, `advance_end()`, `cell()`.
`FontWeight { value: u16, source: WeightSource }`; `is_bold()` is
`value >= 600`; `FontWeight::UNKNOWN` is 400/`Default`. The source is
chosen in this order:
1. `Declared`: the descriptor's `/FontWeight` (Table 122), clamped to
   100–900. A Type0 font's descendant descriptor is used too.
2. `FontName`: a style word in `/BaseFont`, with the subset tag stripped
   and the longest word tried first. For example, `SemiBold` gives 600,
   not 700.
3. `Synthetic`: text drawn with `Tr` 2 or 6 (fill+stroke) over a font
   lighter than 700 reads as 700.

`/ForceBold` and `/StemV` are ignored, per Table 332.
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
**raw operand**. `mod.rs` contrasts them explicitly. Use `size` to
draw; use `tf_size` only to reason about the source operator.

### 8.4.2 The structure tree — reading order and element types (`Pass 372.0`)

```rust
use pdfcer_core::structure_tree::{self, StructKid, StructTreatment};

let tree = structure_tree::read_structure_tree(&doc, &opts)?;   // untagged: Ok, empty
for (i, e) in tree.elements.iter().enumerate() {   // pre-order = logical reading order
    // e.resolved_type: role-mapped standard name ("H1", "TD"); e.raw_type: /S as written
    // e.standard: false when the chain never reached a standard type
    // e.treatment: Normal | NonStruct | Private | Artifact
    let text = tree.element_text(i);               // ActualText replaces the subtree
    let boxes = tree.element_bbox(i);              // Vec<(page_index, Rect)>, user space
}
```

- `StructElement`: `raw_type`, `resolved_type`, `namespace`, `standard`,
  `treatment`, `parent`, `depth`, `object`, `id`, `title`, `alt`,
  `actual_text`, `expansion`, `lang`, `effective_lang` (inherited),
  `page_index`, `kids`, and for standard `TH`/`TD` `row_span`/`col_span`
  (default 1), `scope`, `headers`; `list_numbering` (inherited). Attributes
  resolve `/A` then `/C` via ClassMap, later wins (ISO 32000-2 §14.7.6).
- `StructKid`: `Element(index)` (always a later index), `MarkedContent
  { page_index, stream, mcid, runs, declared }` (`runs` index into the
  `runs` of the `tree.text.pages` entry with that `page_index`), `Object { page_index, object,
  subtype, rect }` (an annotation or XObject by OBJR).
- Role mapping always applies once, even to a standard name (ISO 32000-2
  §14.7.3 NOTE 3), then follows the chain; `/NS` elements use that
  namespace's `RoleMapNS`. PDF 2.0 namespace accepts `Hn`.
- `tree.diagnostics` counts every disagreement between the tree and the
  content (`named_not_declared`, `declared_unclaimed`, `claimed_twice`,
  `role_map_cycles`, `elements_revisited`, `page_unresolved`, …). Nothing
  is guessed silently; a broken tree still returns what it could read.
- `PageText::marked_content_ids` lists every `(ContentStreamRef, mcid)` a
  `BDC` declared on the page, including sequences with no text — the join
  needs it to tell "declared, no text" from "absent".
- `structure_tree::has_structure_tree(&view) -> bool` reads the catalog
  only: whether `/StructTreeRoot` is a dictionary. Use it before paying for
  any extraction (G067).
- `structure_tree::read_structure_tree_in_pages(&view, &indices, &opts)`
  extracts only those pages (view page list, in the order given;
  `NoSuchPage` past the end). The whole tree is still walked, so
  `elements` and depths match a full read. Marked content on other pages
  stays unjoined (`runs` empty, `declared: false`) and is counted in
  `diagnostics.content_on_other_pages`, not in `named_not_declared`,
  `claimed_twice` or `declared_unclaimed`. `tree.text.pages` follows
  `indices`.
- CLI: `pdfcer extract-tags in.pdf [--json] [-o out]`.

### 8.4.3 Untagged block layout — headings, paragraphs, lists, running text (`Pass 373.0`)

For a file with no structure tree. Everything is inferred from geometry and
type, so every kind decision is counted in `diagnostics` and the shell must
say so (CLAUDE.md rule 4).

```rust
use pdfcer_core::block_layout::{self, BlockKind, BlockSource, LayoutOptions};

let layout = block_layout::analyze_layout(&doc, &opts, &LayoutOptions::default())?;
for page in &layout.pages {
    // page.columns: Vec<Rect> (user space), empty or one entry when single-column
    for block in &page.blocks {                 // reading order
        // block.kind: Heading{level} | Paragraph | ListItem{marker} | Caption
        //           | RunningHeader | RunningFooter | PageNumber
        // block.source: Inferred, Tagged (the file's /Artifact /Subtype),
        //               or Structure (a structure element, §8.4.5)
        // block.lines: indices into page.lines; each LayoutLine.runs indexes
        //              layout.text.pages[i].runs
        let text = block.text(page);
    }
}
let n = layout.diagnostics.inferred();          // untagged blocks, paragraphs included
```

- Analysis runs in display space (`/Rotate` applied); every box returned is
  in user space. `layout_text(text, &[PageGeometry], &opts)` runs the same
  analysis on an extraction you already have.
- Running text is found by **repetition**: text in the top or bottom
  `margin_band` (default 15% of the page) repeating within 4 pt on at least
  `max(2, ceil(pages × running_min_fraction))` pages (default 0.4). Digits
  and lone roman numerals compare equal. A one-page file therefore has no
  running text unless tagged. A heading-sized group whose text differs page
  to page (`Chapter 1`, `Chapter 2`) stays headings.
- A tagged `/Artifact /Subtype /Header|/Footer` beats the heuristic;
  `/Background` and `/Watermark` artifacts are left out
  (`runs_watermark_skipped`), as is non-horizontal text
  (`runs_not_horizontal`).
- Headings: at most 3 lines, size ≥ 1.15 × body, or bold (≤ 2 lines) when
  body text is not. Levels rank (size, bold) across the whole document,
  capped at 6 — level 1 is the largest style in the file, not on the page.
- Heading weight comes from `ExtractedGlyph::weight` (§8.4): `/FontWeight`,
  then the font name, then synthetic bold from `Tr` 2/6.
- Columns need a gutter ≥ 0.5 em with lines on both sides at the same
  height; a line crossing a gutter spans the page and splits reading order
  into bands (`spanning_lines`).
- `alignment` is measured from line extents against the column (running
  text: against the page); `first_line_indent` is the first line's x
  minus the rest's, in points.
- CLI: `pdfcer extract-layout in.pdf [--json] [-o out]`.

### 8.4.4 Tables — ruled and aligned cell grids (`Pass 374.0`)

A page's tables as rows, columns and cells, from drawn rules or from
whitespace alignment. Every table,
merged cell and header guess is an inference and is counted in
`diagnostics`; the shell must say so (CLAUDE.md rule 4). A tagged file's own
`/Table` elements are §8.4.5 (`source` `Tagged`), not this.

```rust
use pdfcer_core::table_detect::{self, HeaderEvidence, TableOptions};

let found = table_detect::detect_tables(&doc, &opts, &TableOptions::default())?;
for t in &found.tables {                        // by page, then top to bottom
    // t.page_index, t.bbox (user space), t.source: Ruled | Aligned | Tagged (§8.4.5)
    // t.rows / t.columns: Vec<Rect> bands, user space, top-to-bottom /
    //                     left-to-right as displayed
    // t.header_rows: 0 or 1 (Tagged: any); t.header_evidence: Bold | Filled | HeavyRule
    //                  | RuleBelow | Tagged
    for c in &t.cells {                         // row-major by top-left
        // c.row, c.col, c.row_span, c.col_span (>= 1), c.bbox
        // c.glyphs: Vec<GlyphRef{run, glyph}> into found.text.pages[t.page_index]
        // c.text: a space at a word gap, '\n' between lines
    }
    let cell = t.cell(0, 0);                    // by top-left position
}
let n = found.diagnostics.inferred();           // ruled + aligned tables, merged cells, headers
```

- Rules are stroked axis-aligned segments (per subpath segment, so a CAD
  view drawn as one path still yields its lines) and filled rectangles at
  most `thin_fill` (2 pt) thick, from the page and its form XObjects. White
  ink and curves are ignored. Rules within `snap_tolerance` (3 pt) merge;
  collinear pieces within `join_tolerance` (3 pt) join; rules under
  `min_rule_length` (3 pt) are dropped.
- A cell is the smallest rectangle closed by four connected rule crossings.
  Cells sharing a corner form one table; a lone box is a frame, not a table
  (`single_cell_frames`). Spans come from the table's distinct cell edges.
- A glyph belongs to the smallest cell containing its centre.
- Header row: row 0's glyphs ≥ 60% bold and the rest < 30%; else a
  non-white fill covering ≥ 80% of row 0 and not of row 1; else the rule
  under row 0 ≥ 1.5 × the median of the table's other horizontal rules.
- Over 50,000 rules or 20,000 crossings on a page, that page is skipped and
  counted (`pages_over_limit`) rather than searched.
- Analysis runs as displayed (`/Rotate` applied); boxes are user space.
- Aligned tables come from the glyphs no ruled table took (horizontal as
  displayed, not whitespace). Lines split into chunks at gaps
  ≥ `min_gutter_em` (1.0) × font size; `min_aligned_rows` (3) or more
  consecutive rows of ≥ 2 chunks, each ≤ 2.5 × size below the last, form a
  candidate. Columns are the union of chunk extents. The candidate is
  rejected (`aligned_blocks_rejected`) with fewer than 2 columns, a column
  only one row uses, or a mean above `max_mean_cell_chars` (30) characters
  per non-empty cell (prose in columns). The grid is full: an empty cell is
  a cell with empty text, and no aligned cell spans. Column edges sit
  mid-gutter; row edges midway between rows, 0.6 × size outside the first
  and last.
- `RuleBelow` (aligned only, checked before `HeavyRule`): a horizontal rule
  spanning ≥ 80% of the table lies between rows 0 and 1, and no other
  interior row gap has one (booktabs style; rules above the first and below
  the last row are allowed).
- **A page subset:** `detect_tables_in_pages(&doc, &[2, 0], &opts, &TableOptions::default())`
  reads only those page-tree indices, in the order given, and every count
  (`found.text.pages.len()`, `diagnostics`) covers only them. An index past
  the end is `TableError::Extract(ExtractError::NoSuchPage { index, count })`.
  `t.page_index` stays the page-tree index, so `found.text.pages[t.page_index]`
  is only right for a full-document detect; otherwise find the `PageText`
  whose `page_index` equals it.
- CLI: `pdfcer extract-tables in.pdf [--json] [-o out] [--pages 1-3]`; `export-xlsx`
  and `export-docx` take the same `--pages` (1-based, order honoured).

### 8.4.5 Tagged layout — blocks and tables from the structure tree (`Pass 395.0`)

For a tagged file: the same `DocumentLayout` §8.4.3 returns, and the
tables, taken from the file's own structure elements instead of inferred.
Use it to drive an export by the author's structure.

```rust
use pdfcer_core::tagged_layout::{self, LayoutSourceUsed, TaggedLayoutOptions, StructureUse};
use pdfcer_core::table_detect::tables_from_structure;

let tree = structure_tree::read_structure_tree(&doc, &opts)?;
// or, for a page selection: read_structure_tree_in_pages(&doc, &pages, &opts)
// geometry[i] belongs to tree.text.pages[i], as for layout_text
let tagged = tagged_layout::layout_from_structure(
    &tree, &geometry, &LayoutOptions::default(),
    &TaggedLayoutOptions::default(),             // Auto, min_coverage 0.5
);
match tagged.report.source {
    LayoutSourceUsed::StructureTree => {
        let tables = tables_from_structure(&tagged.tables, &tagged.layout.text);
        // write_docx(&tagged.layout, &geometry, &tables, ..) / write_xlsx(&tables, ..)
    }
    _ => { /* tagged.report.fallback says why; tagged.layout is layout_text's */ }
}
```

- `StructureUse`: `Auto` (default) uses the tree when it exists and owns at
  least `min_coverage` of the laid-out non-artifact characters; `Always`
  uses any tree; `Never` returns the inferred layout. `report.fallback`:
  `Disabled | NoStructureTree | NoTextClaimed | LowCoverage`;
  `report.coverage` is the fraction owned (0 when the tree is not read).
  Coverage is judged over the pages the tree was read for, so a
  page-scoped read decides on the selection, not the document; that is
  what the CLI's `--pages` does.
- Blocks carry `BlockSource::Structure`. `H1`–`H6`/`Hn` → `Heading
  { level }` (capped at 6); `H` → level = enclosing `Sect` count; `Title`
  → level 1; `P`, `TOCI`, `BibEntry`, `FENote` → `Paragraph`; `LI` →
  `ListItem`, marker = its first `Lbl`'s text; `Caption` → `Caption`.
  Inline elements are part of their block; a grouping element inside a
  block starts fresh blocks. A block element inside a `P`, `H`, `Hn` or
  `Title` is its own block (producers nest body `P`s in heading `P`s);
  inside an `LI`, `Caption` or `TOCI` it stays part of that block.
- Content under no block-level element becomes a `Paragraph`, counted in
  `report.non_standard_as_paragraph` (its type never reached a standard
  name) or `report.untyped_as_paragraph`.
- Content the tree does not own (untagged text, artifacts) keeps its
  inferred block (`BlockSource::Inferred`), after the structure block that
  precedes it; `report.inferred_blocks_kept`. Table cell text is not in
  any block — it is in `tagged.tables`.
- A line whose runs belong to two elements is split; alignment, indent and
  column are borrowed from the inferred block holding the element's first
  line.
- `TaggedTable` (one per `Table` element per page): `element`,
  `page_index`, `bbox`, `rows`/`columns` bands (user space, as displayed),
  `header_rows` (leading rows in `THead` or all-`TH`), `cells:
  Vec<TaggedCell{element, row, col, row_span, col_span, bbox, runs, text,
  header}>`. `RowSpan`/`ColSpan` place cells on the grid (ISO 32000-1
  §14.8.5.7). A table in a cell is flattened into the cell text
  (`nested_tables_flattened`); table content outside any cell is
  `stray_table_content`; `broken_references` is the tree's
  `named_not_declared`.
- `tables_from_structure(&[TaggedTable], &ExtractedText) -> Vec<Table>`
  converts for the spreadsheet and Word writers: `source` =
  `BoundarySource::Tagged`, `header_evidence` = `HeaderEvidence::Tagged`,
  glyphs = every glyph of the cell's runs.
- `tagged.retain_pages(&[page_index, ..])` keeps those pages and their
  tables and recounts the report; it does not reorder.
- CLI: `export-docx`, `export-xlsx`, `export-ods` take `--structure
  auto|tree|layout` (default `auto`) and end their result line with
  `structure=tree|layout structure_fallback=none|disabled|no-tree|no-text-claimed|low-coverage
  structure_coverage= structure_blocks= non_standard_as_paragraph=
  untyped_as_paragraph= nested_tables_flattened= stray_table_content=
  broken_references=` (`export-docx` adds `inferred_blocks_kept=`). With
  the tree used, tables are the tree's only; none are detected.

### 8.5 ★ Search — it lives on `EditSession`

There is no read-only search entry point. Text search is:

```rust
use pdfcer_core::edit::{EditSession, TextSearchOptions};

let mut session = EditSession::new(doc);                    // edit.rs (takes ownership)
let opts = TextSearchOptions::default()                     // edit.rs
    .with_case_insensitive(true);
let hits = session.find_text_with("total", &opts);          // edit.rs -> Vec<TextMatch>
for h in &hits {
    let _page = h.page_index;      // 0-based, SESSION page space
    let _quad = h.quad;            // annot_author::Quad, unrotated page space, y-UP
    let _text = &h.text;           // what was actually matched, not the needle
}
```

`TextMatch` — `edit.rs`: `page_index`, `quad`, `text`. It needs
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
let found = session.search_text("total", &opts);            // edit.rs
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
`EditSession::find_text(needle, case_insensitive)` (`edit.rs`) passes
`with_wildcards(true)`: **`#` matches any ASCII digit and `?` matches any
single character.** `TextSearchOptions::default()` has `wildcards: false`.

The doc comment records what happened (`edit.rs`): pdfcer's own
Find bar ran through `find_text`, so *"typing `?` into it matched every
character on the page and nothing said why."* It was fixed in the **front
end**, not the function — `find_text`'s pattern behaviour is its documented
contract. The sibling verb `mark_redactions_by_search` matches **literally**,
so a Find bar on `find_text` highlights hits that a "redact every hit"
control then declines to mark.

**For a new GUI: use `find_text_with` with an explicit `TextSearchOptions`,
and expose wildcards as a visible toggle.** Never wire a search box to
`find_text`.

`TextSearchOptions` (`edit.rs`) also carries `whole_word` and
`word_boundary` — the latter because ISO 32000-1 §14.8.2.5 NOTE 1 declines
to define "word" at all, so pdfcer exposes NOTE 4's own menu of strategies
as a setting rather than picking one (R169).

Case-insensitive matching is **ASCII-only and byte-offset preserving** by
design (`edit.rs`): lower-casing would shift byte offsets for
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

let d: DecodedText = decode_text_string(bytes);   // textstring.rs, infallible
// d.text: String, d.form: TextStringForm (PdfDocEncoding | Utf16Be), plus flags
```

Also: `decode_utf16be_bytes`, `encode_text_string`,
`pdf_doc_char(u8) -> Option<char>`.

**Never `String::from_utf8` a PDF string.** §7.9.2 strings are
PDFDocEncoding by default and UTF-16BE when they carry a BOM. This function
is the only correct decoder.

**Naming trap:** there is a *second* `decode_text_string` at `edit.rs`
returning an `InfoText` — a different type for the `/Info`-dictionary path.
Import explicitly and check which you have.

### 8.7 `text_state` — ambient text-state tracking

`text_state.rs`. `TextStateParam`, `TextStateParams`,
`AmbientTextState`, `AmbientValue`, `AmbientOrigin`,
`AmbientRestoreError`.

You need this only if you are building text *editing* on top of extraction
(part 2's territory). The read-side relevance is one trap:

**★ `AmbientValue::value` for `HorizScale` is the raw `Tz` percentage
(e.g. `90.0`); `TextStateParams::h_scale` for the same parameter is the
ratio (`0.9`).** Both live in `text_state.rs`. Mixing them scales advances
by 100×.

`AmbientOrigin::Unobservable` means the value is known but a byte-faithful
restore must be **refused, never guessed** — that refusal is
`AmbientRestoreError`, not a silent default (`text_state.rs`).

### 8.8 Traps — text extraction

- **T-8.1 `ExtractedGlyph::text_len` is not 1.** `mod.rs`: *"**Not
  one.** One code may produce many code points — §9.10.3's own example
  decomposes `ffl` from a single code."* Slice with
  `[text_start .. text_start+text_len]`.
- **T-8.2 `ActualText` runs have NO glyphs, by design.** `mod.rs`:
  §14.9.4 N4 records no length relationship between replacement and replaced
  content, so character-level mapping back to glyph positions is
  *"**impossible**, not merely unimplemented."* Highlight such a run at
  `bbox` granularity or not at all.
- **T-8.3 Artifact runs are ALWAYS in `PageText::runs`.** `mod.rs`:
  `include_artifacts` filters only the `plain_text()`/`sourced_text()`
  *accessors*. Iterate `runs` directly and you will leak watermarks and
  running heads into your UI. Check `run.artifact`.
- **T-8.4 `origin.is_sourced() == true` ≠ every character is trustworthy.**
  `mod.rs`: a `Glyphs` run may still contain U+FFFD from
  `LadderRung::Failed`. Per-character confidence is `ExtractedGlyph::rung`.
- **T-8.5 Page separator is U+000C.** `mod.rs`.
- **T-8.6 `capture_provenance` defaults to `false`.** `mod.rs`.
  `provenance.unwrap()` panics without it.
- **T-8.7 `include_artifacts` is captured at extraction time** and is
  private on both `PageText` and `ExtractedText` (`mod.rs`,
). Changing the policy means re-extracting.
- **T-8.8 Plural extract swallows per-page failures; singular does not.**
  §8.1. Check `TextDiagnostics::pages_unreadable`.
- **T-8.9 `unmappable_code` changes the sentinel, never the count.**
  `mod.rs`: `TextDiagnostics::ladder_failures` counts every failure
  regardless. Do not infer "no failures" from the absence of U+FFFD.
- **T-8.10 `Tw` (word spacing) is spec-void on composite 2-byte runs**
  (§9.3.3). `GlyphProvenance::composite` tells you per-run
  (`mod.rs`).
- **T-8.11 `TextDiagnostics::via_cid_collection` is always zero this Pass**
  (`mod.rs`). Do not build a feature that depends on it firing.

`TextDiagnostics` (`mod.rs`) carries ~30 honesty counters plus
`notes: Vec<String>`. It is the read-side embodiment of project rule 4 —
if you show extracted text, show the diagnostics too, or at least a pip
when they are non-zero.

### 8.9 Stability

`textstring.rs` is frozen (initial commit only). `text_extract/layout.rs`
likewise. `mod.rs`, `font.rs`, `page.rs` and `fontinfo.rs` are the
**highest-churn** files in the crate's read side (most recent: `6d63d81`,
2026-08-08). Expect *additive* change — `mod.rs` explains the
`#[non_exhaustive]`-plus-builder pattern exists precisely so new fields do
not break callers. `text_state.rs` is young (introduced Pass 19.0, two
commits).

---

## 9. Fonts

**Module set:** `fontinfo`, `fontdata`, `text_extract::font`,
`text_extract::cmap`.
`fontinfo`, `fontdata` and `textstring` live in `crates/pdfcer-fonts/src/`,
re-exported at their `pdfcer_core::` paths; line references below are into
that crate.

Two different jobs live here. `fontinfo` answers *"what fonts does this
document use, and what may I do with them?"* — a document-level inventory
for a Fonts panel. `text_extract::font` + `cmap` answer *"how do I turn
this font's character codes into text?"* — per-resource decoding.

### 9.1 Document font inventory

```rust
use pdfcer_core::fontinfo::{self, Removability};

let inv = fontinfo::inventory(&doc.view());     // fontinfo.rs — INFALLIBLE, no Result
for f in &inv.fonts {                            // Vec<FontRecord>, first-discovery order
    // FontRecord: fontinfo.rs
    let embedded = matches!(f.program, fontinfo::Program::Embedded(_));
    let pages = fontinfo::format_page_ranges(&f.pages);    // fontinfo.rs -> "1-3, 7"
}
println!("{} embedded, {} bytes", inv.embedded_count(), inv.embedded_bytes()); //,
println!("not walked: {:?}", inv.coverage.not_walked());                        //
```

`FontInventory{fonts, coverage, diagnostics}` — `fontinfo.rs`.
`Program`: `NotEmbedded` | `Unreadable{key, why}` | `Embedded(EmbeddedProgram)`.
`Removability`, `RemovabilityUnknown`.
`SurfaceCoverage` with `includes`, `walked`,
`not_walked`.

Embedding permission from an embedded program's `OS/2` table:

```rust
let bits = fontinfo::read_fs_type(program_bytes)?;   // fontinfo.rs -> FsTypeBits
```

`FsType`, `FsTypeBits`, `EmbeddingPermission`,
`FsTypeError`.

Subset tags: `split_subset_tag`; standard-14 test `is_standard_14`
.

Guards: `MAX_RESOURCE_NODES`, `MAX_FONTS`,
`MAX_RESOURCE_NAMES_PER_FONT`, `MAX_SFNT_TABLES`.

### 9.2 Per-resource decoding font

```rust
use pdfcer_core::text_extract::ExtractFont;

let font = ExtractFont::resolve(&doc.view(), &font_dict);  // font.rs — INFALLIBLE
let composite = !font.is_simple();                          // font.rs
if let Some(cmap) = font.to_unicode_cmap() { /* … */ }
let text = font.unicode_for_code(code);                     // Option<String>: None when the ladder FAILS (no sentinel)
```

Only `base_font: String` and `notes: Vec<FontNote>` are public fields
(`font.rs`). `LadderRung` (`font.rs`) is the §9.10.2 decoding
ladder: `ToUnicode` | `EncodingAgl` | `CidCollection` | `GlyphNameExtension`
| `Failed`. `Rung3Gap`, `FontNote`. All four re-exported at
`text_extract::` (`mod.rs`).

### 9.3 `/ToUnicode` CMaps

```rust
use pdfcer_core::text_extract::cmap::ToUnicodeCMap;

let cmap = ToUnicodeCMap::parse(bytes);              // cmap.rs — INFALLIBLE
let s: Option<String> = cmap.lookup(code);            // cmap.rs
let stats = cmap.stats();                             // cmap.rs -> CMapStats
```

Guards: `MAX_BF_ENTRIES` 500_000, `MAX_BF_RANGES` 100_000,
`MAX_DST_BYTES` 512 (**spec-stated**), `MAX_CMAP_TOKENS` 10_000_000
.

### 9.4 Base-14 metrics without a font file

`fontdata` is compiled-in metrics only — `pdfcer-core` contains **no font
program parser** (rule R21; that lives in `pdfcer-render`).

`Std14` with `Std14::ALL` · `std14_by_base_font` ·
`std14_base_font_name` · `std14_width` ·
`Std14Descriptor` · `std14_descriptor` ·
`BaseEncoding` · `encoding_glyph_name` ·
`glyph_name_to_unicode` · `glyph_name_to_unicode_string` ·
`is_standard_latin_or_symbol_name` · `std14_builtin_encoding`.

`fontdata::tables` is **private**; its contents are `pub(crate)`.

**Units:** `std14_width` returns **glyph space, 1/1000 em** (`u16`), and
`Std14Descriptor`'s `font_bbox`/`ascender`/`descender` are the same
(`fontdata/mod.rs`). Multiply by `font_size / 1000.0` to get text
space.

### 9.5 Traps — fonts

- **T-9.1 `FsType::permission()` returning `None` is NOT "permissive".**
  `fontinfo.rs`: an absent `OS/2` table, a `ttcf`
  collection, or a decode failure all give `None`, and the spec defines
  **no default** for the absent case. Treating `None` as unrestricted is
  exactly the bug this API is shaped to prevent.
- **T-9.2 `EmbeddingPermission` is a value, not a bitmask.**
  `fontinfo.rs`: `0` is the *most* permissive (Installable). Never
  test `fsType != 0` for "restricted".
- **T-9.3 A subset tag is EXACTLY six uppercase letters.**
  `fontinfo.rs`: `"ABCDE+Arial"` (five) and `"AbCdEf+Arial"`
  (mixed case) are not tagged — the whole string is the family name.
- **T-9.4 `FontRecord::pages` empty ≠ unused.** `fontinfo.rs`: a
  font reached only through the AcroForm `/DR` has no page list but is a
  live form-default font.
- **T-9.5 `glyph_name_to_unicode` (char) silently drops ligatures.**
  `fontdata/mod.rs`: it returns `None` for `f_i` and
  multi-group `uni` names. **For extraction use
  `glyph_name_to_unicode_string`**; the `char` form is the rendering-side
  convenience and will lose text if misused.
- **T-9.6 `ToUnicodeCMap::lookup` returning `None` means "this CMap does
  not cover this code", not "no character".** `cmap.rs`: the
  fallthrough to rung 2 happens one level up in `ExtractFont`. Using
  `ToUnicodeCMap` directly means implementing the ladder yourself.
- **T-9.7 `ToUnicodeCMap::injective_inverse()` is O(entries) and can
  refuse.** `cmap.rs`: it materialises up to `MAX_BF_ENTRIES` and
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
let model: PageObjects = decompose_page(&doc.view(), page, Matrix::IDENTITY)?; // decompose.rs
for obj in &model.objects {                    // paint order, back to front
    let bbox = obj.page_bbox();                 // decompose.rs — page space
}
let _ = model.diagnostics;                      // DecomposeDiagnostics, decompose.rs
```

Lower-level forms if you already have a `ContentStream`:
`decompose(&cs, initial, &dyn XObjectResolver)` — `decompose.rs`
(geometry only, `NoFonts`) and `decompose_with_fonts(&cs, initial,
&dyn XObjectResolver, &dyn FontResolver)` — `decompose.rs` (the true
entry point). Resolvers: `NoXObjects` / `DocumentXObjects`;
`NoFonts` / `DocumentFonts::new`.

`VectorObject` — `decompose.rs`: `Path(PathObject)` | `Text(TextObject)`
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

**`PathObject`** — `decompose.rs`.
`subpaths: Vec<Subpath>` is **user space**; `page_subpaths()`
maps them through `ctm` to **page space**. `style: PaintStyle`,
`line_width: f64` (**user space**), `fill_color`/`stroke_color: Rgb`
, `tokens: TokenRange`, `bytes: ByteSpan`,
`page_bbox: Bounds` (page space, control-point hull).

`Subpath`: `{start, segments, closed, tokens, starts_implicitly}`;
`anchors()` yields on-curve points only.
`Segment`: `Line{to}` | `Cubic{c1, c2, to}` — control points
**already resolved** (see T-10.2).

**`TextObject`** — `decompose.rs`. `page_bbox` (approximate),
`runs: Vec<TextRun>` (per-show-op boxes), `approximate: bool`
(**always `true`**), `bounds_basis: TextBoundsBasis`, `preview`,
`font: Option<TextFont>`.

`TextBoundsBasis`: `FontMetrics` | `MetricAdvancesNominalHeight` |
`EstimatedAdvances` | `EmBox`. Four bases, not two, deliberately — a Type 3
or descriptor-less CIDFont has real advances but a guessed height, and
collapsing that into `FontMetrics` would misrepresent confidence
(`ARCHITECTURE.md` §4, Pass 18.6). **Show the basis if you show the box.**

**`ImageObject`** — `decompose.rs`: `{ctm, page_bbox, source, pixel_size,
tokens, bytes}`. `ImageSource`: `Inline` | `XObject` | `Form`.

**`Bounds`** — `geometry.rs`: `{min, max: Point}`, with `EMPTY`,
`union_point`, `union`, `inflate`, `contains`,
`contained_by`, `intersects`.
**`Point`** — `geometry.rs`: `{x, y: f64}`.
**`Matrix`** — `geometry.rs`: PDF row-vector affine `{a,b,c,d,e,f}`, with
`IDENTITY`, `map_point`, `map_vector`, `post_concat`
, `inverse -> Option<Matrix>`, `determinant`.

### 10.3 Hit-testing

```rust
use pdfcer_core::vector::{hit_test_point, hit_test_point_all, hit_test_rect,
                         hit_test_subpaths, hit_test_text_runs,
                         subpath_bounds, MarqueeMode, Point, Bounds};

// ★ tolerance is PAGE space. Convert your screen pixels first.
let tol = screen_px_tolerance / zoom;
let at = Point::new(page_x, page_y);

let top: Option<usize>  = hit_test_point(&model, at, tol);        // hit.rs
let all: Vec<usize>     = hit_test_point_all(&model, at, tol);    // hit.rs  (topmost first)
let marquee: Vec<usize> = hit_test_rect(&model, rect, MarqueeMode::Enclosed); // hit.rs

// Drill down inside one object:
let runs: Vec<usize>     = hit_test_text_runs(&model, obj_idx, at, tol);  // hit.rs
let subpaths: Vec<usize> = hit_test_subpaths(&model, obj_idx, at, tol);   // hit.rs
let b: Option<Bounds>    = subpath_bounds(&model, obj_idx, subpath_idx);  // hit.rs
```

`hit_test_point` is defined as the head of `hit_test_point_all` — one
private iterator underneath both, so they cannot disagree (`hit.rs`,
`ARCHITECTURE.md` §4 continuation-60). Use `hit_test_point_all` for alt-click
cycling; never reimplement either.

#### ★★★ For a click, use `hit_test_point_deep`. The others cannot see inside a form.

```rust
use pdfcer_core::vector::{hit_test_point_deep, HitTarget};

match hit_test_point_deep(&model, at, tol).first() {              // hit.rs
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

`MarqueeMode` — `hit.rs`: `Enclosed` | `Touched`.
`FLATTEN_STEPS` = 16 — `hit.rs` (Bézier flattening for hit-testing).

All of the above are re-exported flat at `pdfcer_core::vector::*`
(`vector/mod.rs`) — verified directly.

### 10.4 Snapping

```rust
use pdfcer_core::vector::{snap_candidates, SnapConfig, SnapKind, SnapCandidate,
                         AxisConstraint, constrained_second_point, measured_length};

let cfg = SnapConfig::new(tol_in_page_units)      // snap.rs
    .with_intersections(true)                      // snap.rs — default FALSE, costs perf
    .with_grid(grid)                               // snap.rs
    .with_axes(true);                              // snap.rs
let cands: Vec<SnapCandidate> = snap_candidates(query_point, &cfg, &model); // snap.rs
// SnapCandidate: snap.rs — {point (page space), kind, source_object}
// SnapKind: snap.rs variants; priority() (0 = highest);
//           is_derived() — TRUE only for DerivedCenterline.

// Axis constraint for a second pick (Shift-drag):
let p2 = constrained_second_point(first, raw_second, AxisConstraint::Horizontal); // snap.rs
let len = measured_length(first, p2, AxisConstraint::Horizontal);                  // snap.rs
```

Guards: `SNAP_FLATTEN_STEPS` 16, `MAX_NEIGHBOURHOOD_SEGMENTS` 256
, `MAX_CANDIDATES` 4096.

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

Related: `centerline::page_candidates(&model)` (`centerline.rs`) and
`derive_from_path(index, &path)`, with
`CENTERLINE_ASPECT_THRESHOLD` = 8.0 and `CenterlineCandidate`
.

### 10.5 Line picking (CAD measurement)

```rust
use pdfcer_core::vector::linepick::{pick_line_in_page, pick_line, classify_two_lines,
                                   measured_angle_degrees, ParallelPolicy,
                                   PickedLine, TwoLineRelation};

let a: Option<PickedLine> = pick_line_in_page(&model, at, tol);   // linepick.rs
let b = pick_line_in_page(&model, at2, tol);
match classify_two_lines(&a?, &b?, ParallelPolicy::default()) {    // linepick.rs
    Some(TwoLineRelation::Parallel { distance }) => {}
    Some(TwoLineRelation::Collinear) => {}
    Some(TwoLineRelation::Angled { degrees, apex, apex_is_real }) => {}
    None => {}
}
```

**★ `linepick` is NOT re-exported at `pdfcer_core::vector::*`.** Verified
against `vector/mod.rs`: there is no `pub use linepick::{…}` block,
unlike `centerline`, `decompose`, `edit`, `geometry`, `hit` and `snap`.
Reach it as `pdfcer_core::vector::linepick::…`. (`pub mod linepick;` is at
`vector/mod.rs`.) This is consistent with it being the newest module
(2026-08-12) and is the kind of thing that may change — do not assume the
flat path will keep failing, and do not assume it works.

`PickedLine` — `linepick.rs`: `{target, subpath, segment, start, end,
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
`ParallelPolicy`: `{epsilon_degrees, force_parallel}`, with
`default`, `from_setting`, `forcing_parallel`.
`measured_angle_degrees` returns the raw angle folded to `[0, 90]`.

### 10.6 ★ Coordinate space table — `vector` read side

| Function / field | Input space & units | Output space & units | Evidence |
|---|---|---|---|
| `decompose*`'s `initial: Matrix` | caller's starting CTM (`IDENTITY` ⇒ page space) | — | `decompose.rs` |
| `PathObject::subpaths` | — | **user space**, `f64` | `decompose.rs` |
| `PathObject::page_subpaths()` | user space via `ctm` | **page space**, `f64` | `decompose.rs` |
| `*::page_bbox`, `TextRun::bounds` | — | **page space** `Bounds`, `f64` | `decompose.rs` |
| `PathObject::line_width` | **user space** points | — (scaled by `√\|det(ctm)\|` at hit time) | `decompose.rs`; `hit.rs` |
| `TextFont::size` | **text space** — raw `Tf` operand, unscaled | — | `decompose.rs` |
| `ImageObject::pixel_size` | — | **sample count**, not a page size | `decompose.rs` |
| `hit_test_point/_all/_rect` point/rect | **page space**, `f64` | index(es) | `hit.rs` |
| every `tolerance` argument | **page-space distance** | — | `hit.rs` |
| `hit_test_text_runs`/`_subpaths` | page space / page distance | `Vec<usize>` nearest-first | `hit.rs` |
| `subpath_bounds` | — | **page space** | `hit.rs` |
| `snap_candidates` query & `SnapCandidate::point` | **page space** | **page space** | `snap.rs` |
| `SnapConfig::tolerance` | **page-space** catch radius | — | `snap.rs` |
| `constrained_second_point`, `measured_length` | page space | page space / page-space length | `snap.rs` |
| `CenterlineCandidate::{start,end}` | — | **page space** | `centerline.rs` |
| `PickedLine::{start,end,pick}` | — | **page space** | `linepick.rs`; built from `page_subpaths()` at `linepick.rs` |
| `TwoLineRelation::Angled{apex}` | — | **page space** | `linepick.rs` |

Everything is `f64` except `Rgb` (`f32`, `geometry.rs`).
`geometry.rs`: *"Values are `f64` … narrowing to `f32` only at the
render/GUI boundary."*

### 10.7 Traps — vector

- **★ T-10.1 (THE tolerance trap) Every `tolerance` / `SnapConfig::tolerance`
  is PAGE space, and nothing in core checks it.** `hit.rs`:
  *"`tolerance` is a page-space slack (the GUI converts a few screen pixels
  into page units and passes it here)."* Pass raw screen pixels and your
  hit-testing silently gets more forgiving as the user zooms out and
  unusably tight as they zoom in. The existing shell converts at the call
  site (`pdfce@cce414e:crates/pdfce-gui/src/canvas.rs`'s `screen_tolerance_to_page`); a new shell
  must implement the same conversion itself.
- **T-10.2 `v` and `y` operators have implicit control points.**
  `geometry.rs`: `cubic_from_v`'s *"first control point is the
  current point — the classic 'v/y trap' that silently mis-renders if
  forgotten"*; `cubic_from_y`'s *"second control point is the endpoint."*
  You avoid this entirely by reading `Segment::Cubic{c1,c2,to}`, which is
  already resolved. Only re-deriving from raw operands re-opens it.
- **T-10.3 Use `Matrix::map_vector` for deltas, `map_point` for
  positions.** `geometry.rs`: `map_point` on a delta folds in the
  CTM's translation and *"would shove the object across the page."*
- **T-10.4 Hit-test text per RUN, not per object bbox.** `hit.rs`,
  commit `627c807`: a CAD sheet can have one text object holding 237
  dimension labels, whose bbox *"at one point over a real line beat 57
  genuine objects underneath it."* Use `hit_test_text_runs` / `TextObject::runs`.
- **T-10.5 The drill-down queries return EMPTY on a bad index; they do not
  fall back.** `hit.rs`. The top-level point query
  *does* fall back to `page_bbox` when `runs` is empty. Do not assume
  matching behaviour.
- **T-10.6 `pick_line*` skips curves entirely — it never chords them.**
  `linepick.rs`: *"A Bézier is deliberately NOT approximated by its
  chord: dimensioning 'the line' of a curve would measure something the
  drawing does not contain."* A click near a curve returns `None`.
- **T-10.7 `PickedLine::pick` is load-bearing, not a diagnostic.**
  `linepick.rs`: two crossing lines bound four angles, and
  `classify_two_lines` picks which one is meant from where the operator
  clicked. Store `pick`; discarding it makes the angle unreconstructible.
- **T-10.8 `ParallelPolicy::force_parallel` is checked BEFORE
  `epsilon_degrees`, unconditionally.** `linepick.rs`.
- **T-10.9 `TextObject::approximate` is always `true`** (`decompose.rs`)
  and `TextFont::size` is the raw `Tf` operand — `/F1 1 Tf` then
  `12 0 0 12 x y Tm` renders 12 pt and reports `1` (`decompose.rs`).
- **T-10.10 `ImageObject::pixel_size` is a sample count.**
  `decompose.rs` quotes §8.9.5: printed size comes from the CTM and
  *"has no fixed relationship to these numbers."* Use `page_bbox`.
- **T-10.11 `SnapKind::Midpoint` never appears on curved segments** —
  cubics contribute only a `SegmentCenterline` projection
  (`snap.rs`).
- **T-10.12 `SnapKind::Node` vs `Endpoint` depends on `Subpath::closed`.**
  `snap.rs`: **every** anchor of a closed subpath is `Node`;
  `Endpoint` requires an open subpath's free terminus.
- **T-10.13 `SnapConfig::intersections` defaults `false`** and is
  neighbourhood-bounded (`snap.rs`). Enabling it on a dense page is a
  documented perf trade, not free.
- **T-10.14 `ARCHITECTURE.md`'s line numbers for `hit.rs` are drifted.**
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

let bytes: Vec<u8> = decode_stream(&stream.dict, raw)?;                 // filters/mod.rs
let (bytes, notes) = decode_stream_with_notes(&stream.dict, raw)?;      // filters/mod.rs
```

Runs the **full `/Filter` chain** with `/DecodeParms`, including PNG/TIFF
predictors. `FilterError` — `filters/mod.rs`, `#[non_exhaustive]`.
`FilterNotes`, `#[non_exhaustive]`, currently
`lzw_framing_anomalies: usize`. Use the `_with_notes` form anywhere the
notes have somewhere to go; every other caller (xref/object/content
streams) uses the plain form.

Individual filters are also public if you need one directly:
`ascii::decode_hex` / `decode_85`, `flate::decode`,
`lzw::decode`, `runlength::decode`, `predictor::Params`
 / `::from_dict` / `unpredict`.

**★ `decode_stream` deliberately refuses image codecs.** `filters/mod.rs`:
hitting `DCTDecode`/`CCITTFaxDecode`/`JBIG2Decode`/`JPXDecode` returns
`FilterError::ImageCodec` — a **distinct** variant from `UnsupportedFilter`,
meaning *"you called the wrong entry point"*. Route images through
`image_codec::decode_image*` (§11.2).

### 11.2 Image decoding

Source: `crates/pdfcer-image-codec/src/lib.rs`, re-exported as
`pdfcer_core::image_codec`.

```rust
use pdfcer_core::image_codec::{decode_image, decode_image_view, terminal_codec,
                              CodedImage, CodecColorModel, Codec};

let which: Option<Codec> = terminal_codec(&image_dict)?;   // lib.rs — no decode
let img: CodedImage = decode_image(&doc, &image_dict, raw, /*inline=*/false)?; // lib.rs
// session-aware form:
let img = decode_image_view(&doc.view(), &image_dict, raw, false)?;            // lib.rs
// explicit CMYK-JPEG polarity (R169 setting):
// decode_image_view_with(view, dict, raw, inline, CmykJpegPolarity::…)        // lib.rs
```

`Codec` — `lib.rs`: `Dct | Ccitt | Jbig2 | Jpx`; `Codec::name`,
`Codec::allowed_inline` (§8.9.7 — `Jbig2`/`Jpx` are `false`).
`CodedImage` — `lib.rs`, `#[non_exhaustive]`.
`CodecColorModel` — `lib.rs`: `Gray | Rgb | Untransformed3 | Cmyk |
Bilevel | Unspecified | Unknown{components}`.
`CodecNotes` — `lib.rs`: `geometry_mismatch`, `cmyk_image`,
`cmyk_polarity_unverifiable`, `jpx_smask_in_data_preblended`,
`lzw_framing_anomalies`.
`ImageCodecError` — `lib.rs`: `Filter | Unsupported | FeatureUnsupported
| Corrupt | TooLarge | NotAllowedInline | CodecNotTerminal`.

The per-codec modules (`image_codec::{dct, ccitt, jbig2, jpx}`,
`lib.rs`) have an **empty public surface** — every `decode` is
`pub(super)`. `decode_image*` is the only door.

#### ★ Image output format — exact

Evidence: `mod.rs`, `bilevel.rs`.

| Property | Value |
|---|---|
| Row order | **top-down**, row 0 at the top |
| Layout | row-major, interleaved, packed to `bits_per_component`, **each row padded to a byte boundary** (§8.9.3) |
| Bit depth | `CodedImage::bits_per_component` — **codestream-declared**. DCT always 8; CCITT/JBIG2 always 1; JPX codestream-authoritative (`/BitsPerComponent` is ignored, Table 89) |
| Channel count | `CodedImage::components` — codestream-declared; `0` = not declared by any codec |
| Channel order | **RGB, not BGR** (`dct.rs` via `zune_core::colorspace::ColorSpace::RGB`); no BGR path exists |
| CMYK order | C,M,Y,K, **raw** — no `/Decode`, no inversion applied here |
| Alpha | **not premultiplied**, and normally absent. `CodedImage::embedded_alpha` is populated only by JPX with `/SMaskInData == 1`. The opacity channel is **always stripped out of `samples`** — leaving it interleaved *"would shift every colour one position to the right"* (`mod.rs`) |
| Bilevel polarity | normalised by both adapters to **`0 = black`** regardless of codec-native polarity (`bilevel.rs`) |
| `width`/`height` units | **pixels/samples**, as the codestream declares — may disagree with `/Width`/`/Height`; see `CodecNotes::geometry_mismatch` |
| `samples` / `embedded_alpha` units | **bytes** (`Vec<u8>`), per the packed+padded layout — not a sample count |

**★ `/Decode` arrays and any polarity flip are `pdfcer-render`'s job, never
this crate's** (rule R26, `mod.rs`). If your shell rasterizes itself
rather than calling `pdfcer-render`, you must apply `/Decode` and the
colour-space mapping yourself; `decode_image` hands you the codec's raw
samples plus an honest statement of what they are.

### 11.3 Colour

**★ There is no `/ColorSpace` object parser in `pdfcer-core`.** `color/mod.rs`
has exactly three device converters plus an intent variant (source:
`crates/pdfcer-color/src/color/`, re-exported by `pdfcer-core` at the same path):

```rust
use pdfcer_core::color::{gray_to_srgb, rgb_to_srgb, cmyk_to_srgb, cmyk_to_srgb_with};
let rgb = cmyk_to_srgb(0.0, 0.0, 0.0, 1.0);   // color/mod.rs
```

`gray_to_srgb`, `rgb_to_srgb`, `cmyk_to_srgb`,
`cmyk_to_srgb_with(CmykIntent, …)`. All take/return `f32` components
in **0.0–1.0**, returning `[f32; 3]` sRGB.

Full `/ColorSpace` resolution (`Separation`, `DeviceN`, `ICCBased`,
`Indexed`, …) lives in **`pdfcer-render`** (`pdfcer-render/src/color.rs`,
`pub enum ColorSpace`). This split is deliberate per rule R26 — *"the codec
layer never decides colour"* — not a gap. A `Separation`/`DeviceN` colour is
a two-step composition: `PdfFunction::eval` (tint → alternate-space
components), then the matching `*_to_srgb`.

**★ `DeviceGray` 0.0 = black; `DeviceCMYK` 0.0 = white.** `color/mod.rs`
exists to keep that polarity trap visible: *"The two device spaces run
opposite ways."*

**★ `cmyk_to_srgb` is a calibrated house choice, never "colorimetrically
correct".** `color/mod.rs`: *"There is no 'correct' answer
to be spec-compliant about … it should never be described as
'colorimetrically correct'."* It uses a calibrated 6⁴ node grid — so
`cmyk_to_srgb(0,0,0,1)` is a rich near-black, **not** `[0,0,0]`. Do not
describe it to users as exact, and do not swap in a naive
`255*(1-c)*(1-k)` formula expecting a match.

### 11.4 PDF functions

```rust
use pdfcer_core::function::{PdfFunction, FunctionType, FunctionError};

let f = PdfFunction::load(&doc.view(), &function_obj)?;   // function.rs — validates structure
let outs: Vec<f64> = f.eval(&inputs)?;                     // function.rs
// Per-pixel path — reuse the buffer:
let mut buf = Vec::new();
f.eval_into(&inputs, &mut buf)?;                           // function.rs
```

`FunctionType`: `Sampled | Exponential | Stitching | PostScript`
(0/2/3/4). Accessors: `function_type`, `inputs`, `outputs`
, `domain`, `range`, `cubic_downgraded`.
`FunctionError`, `#[non_exhaustive]`, ~28 variants.

### 11.5 Resource limits

| Guard | Constant / value | Error |
|---|---|---|
| Decoded byte-stream ceiling (incremental) | `filters::MAX_DECODED_LEN` = 256 MiB | `FilterError::OutputTooLarge` |
| Image pixel count | `MAX_IMAGE_PIXELS` = 32 Mpx | `ImageCodecError::TooLarge` |
| Image dimension | `MAX_IMAGE_DIMENSION` = 65,535 | `TooLarge` |
| Decoded sample bytes | `MAX_IMAGE_SAMPLE_BYTES` = 128 MiB | `TooLarge` |
| DCT progressive scans | `dct::MAX_PROGRESSIVE_SCANS` = 100 *(private)* | `Corrupt` |
| JPX working memory | `jpx::MAX_WORKING_BYTES` *(private)* | `TooLarge` |
| JPX tile count | `jpx::MAX_TILES` = 4096 *(private)* | `TooLarge` |
| JPX component bit depth | `jpx::MAX_COMPONENT_BIT_DEPTH` = 31 *(private)* | `FeatureUnsupported` |
| CCITT/JBIG2 sink budget | `BilevelSink::budget` (latched, since vendor sinks are infallible) | `TooLarge` |
| Type-4 PS stack | `PS_STACK_LIMIT` = 100 — **spec floor+ceiling**, not policy | `StackOverflow{limit}` |
| Type-4 PS steps | `MAX_PS_STEPS` = 1,000,000 | `StepLimit{limit}` |
| Type-4 brace nesting | `MAX_PS_NESTING` = 32 | `PostScriptNestingTooDeep{limit}` |
| Type-0 input dimensions | `MAX_SAMPLED_INPUTS` = 8 | `TooManyInputs{got,limit}` |
| Type-3 recursion (also catches `/Functions` cycles) | `MAX_FUNCTION_DEPTH` = 8 | `NestingTooDeep{limit}` |

**There is no runtime API to raise any of these.** The private ones you
cannot even observe. Do not attempt to bypass them; the only deliberate
configurability in this area is `CmykJpegPolarity` (a polarity choice under
R169, not a limit override).

### 11.6 Traps — decoding and colour

- **T-11.1 `decode_stream` on an image filter is an error by design**
  (`filters/mod.rs`) — `FilterError::ImageCodec`, not
  `UnsupportedFilter`.
- **★ T-11.2 CCITT `BlackIs1` polarity — *"the single most likely
  correctness bug"*.** `image_codec/ccitt.rs`: the mapping is the
  **direct** assignment `invert_black = BlackIs1`, not the negation.
  *"Getting this backwards renders every fax image as its own negative,
  which looks deliberate rather than broken."*
- **T-11.3 JBIG2 polarity is unconditional; there is no `/BlackIs1`
  equivalent.** `image_codec/jbig2.rs`.
- **T-11.4 For JPEG, an APP14 marker outranks `/DecodeParms`
  unconditionally**, and the fallback default is component-count dependent
  — *"a 4-component JPEG with neither defaults to `0`, i.e. no transform,
  not to `1`"* (`image_codec/dct.rs`).
- **★ T-11.5 pdfcer NEVER applies an "Adobe CMYK inversion" (rule R29).**
  `image_codec/dct.rs`: *"not on APP14 presence, not on transform-byte
  value, not on component count."* `CmykJpegPolarity::NeverInvert` is the
  default; only the explicit R169 setting changes it. If your shell
  "corrects" CMYK JPEGs by inverting them, you are reintroducing the bug
  four reference engines agree is not there. The residual ambiguity is
  **reported, never repaired**, via `CodecNotes::cmyk_polarity_unverifiable`
  (`mod.rs`).
- **T-11.6 The YCCK→CMYK step *is* performed** — because zune-jpeg has no
  YCCK arm — and is *"not a polarity guess"* (`dct.rs`). Do not
  conflate it with T-11.5.
- **T-11.7 For JPX, a present `/ColorSpace` WINS over the codestream.**
  `image_codec/jpx.rs`: *"the trap is to read 'the codestream is
  authoritative for JPX' as unconditional"* — it wins only when the
  dictionary is silent.
- **T-11.8 `/SMaskInData == 2` is recognise-and-defer.** `mod.rs`:
  `embedded_alpha` stays `None` and a note is set, because the colour
  samples are already composited over an unknown backdrop and
  un-premultiplying needs a `Matte` this crate does not have.
- **T-11.9 RunLengthDecode literal-run off-by-one.** `filters/runlength.rs`:
  writing `L <= 128` for the literal branch *"consumes the EOD marker as
  data."*
- **T-11.10 PNG Average predictor is not modulo-256.** `filters/predictor.rs`:
  `left + prior` reaches 510 and must be computed wide before the
  floor-divide. And **Paeth's tie-break order (a, then b, then c) is
  normative** — a different order is wrong on only *some* inputs.
- **T-11.11 LZW: `BitOrder::Msb` is mandatory** (GIF's LSB packing is a
  different codec) and `/EarlyChange` changes the code-width switch points
  (`filters/lzw.rs`).
- **T-11.12 Function `/C0`/`/C1` default to the SCALARS `[0.0]`/`[1.0]`,
  not "n zeros".** `function.rs`: *"A type 2 with neither entry
  present is therefore a 1-output function, and a 4-output tint transform
  must carry explicit four-element arrays."*
- **T-11.13 `PdfFunction::range()` returning `None` means NO clipping and
  must not be defaulted.** `function.rs` quoting Table 38: *"If this
  entry is absent, no clipping shall be done."*
- **T-11.14 NaN inputs are refused, never clamped** (`FunctionError::NonFiniteInput`)
  — *"a NaN tint clamped to /Domain would silently become the domain's lower
  bound — a fabricated value wearing the shape of a real one."*
- **T-11.15 There is no `/FunctionType` 1.** `function.rs`:
  `UnknownFunctionType` reports `1` exactly as it reports `7`.
- **T-11.16 `/Order 3` may be silently downgraded to linear.** pdfcer always
  evaluates multilinearly and exposes whether a downgrade happened via
  `cubic_downgraded()` (`function.rs`) — surface it if you show
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

let outline = read_outline(&doc);          // outline.rs — generic over ObjectGraph
for item in outline.flatten() {             // outline.rs — document order, flat
    let label = &item.title;                // already text-decoded
    let (bold, italic) = (item.is_bold(), item.is_italic());   //,
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

`Outline` — `outline.rs`: `{items: Vec<OutlineItem>, diagnostics}`,
`#[non_exhaustive]`. It is a **real tree** (`OutlineItem` has
children); `flatten()` gives you the document-order flat list a rail wants.
`visible_item_count()` implements Table 152's root `/Count`.
`parse_outline` is `read_outline(g).items` with diagnostics
discarded — prefer `read_outline`.

`Destination`, `#[non_exhaustive]`, 6 variants.
`DestView`, `#[non_exhaustive]`: `Xyz | Fit | FitH | FitV | FitR |
FitB | FitBH | FitBV | Unknown | Absent`, with `rect()` and
`zoom_is_retain()`.
`RemoteTarget`, `page_index()`.
`OutlineDiagnostics`, ~25 counters.
`MAX_OUTLINE_DEPTH` = 32.

**Coordinates:** `outline.rs` — *"Coordinates are in the target
page's **user space**, unmodified. pdfcer does not apply `/CropBox`,
`/Rotate` or any viewer-side clamping here."* Your scroll-to code applies
those.

### 12.2 Attachments (read half)

```rust
use pdfcer_core::attachments::{list_attachments, list_attachments_with_notes,
                              attachment_bytes, extract_attachment};

let view = doc.view();                                  // required for extraction
let (found, notes) = list_attachments_with_notes(&doc);  // attachments.rs
for att in &found {
    let display = &att.name;
    let file    = att.safe_name();                       // attachments.rs — sanitized
    match &att.kind { /* DocumentLevel{..} | PageAnnotation{..} */ }  //
    if notes.may_be_encrypted { warn_user(); }           // ★ see T-12.4
    let data = attachment_bytes(&view, att);             // -> Option<Vec<u8>>
    // or extract_attachment(&view, att) -> Result<ExtractedAttachment, _> 
}
```

`Attachment`, `#[non_exhaustive]`: `name`, `name_bytes`, `kind`,
`declared_size`, `mime`, `stream_id`, `filespec_id`.
`ExtractedAttachment`: `{data, declared_size, size_check}`.
`DeclaredSizeCheck`. `AttachmentNotes`.
`AttachmentError`. `NameHazard`, `SafeName`,
`sanitize_attachment_name`.
Caps: `MAX_ATTACHMENTS`, `MAX_SAFE_NAME_CHARS`,
`FALLBACK_SAFE_NAME`.

**★ Extracted bytes are UNTRUSTED** (`attachments.rs`). Never
auto-open, never execute. The declared `mime` and the name extension are
producer claims — `attachments.rs` says the caller *"must not treat
it as a safety signal"*. The checksum is **reported, never verified**.

#### 3D models (read-only)

```rust
use pdfcer_core::threed::{list_3d_with_notes, extract_3d, sniff_3d_format,
                          ThreeDSource, ThreeDFormat};

let (found, notes) = list_3d_with_notes(&doc);   // page order, then /Annots order
for art in &found {
    let _ = (art.page_index, art.annot_id, art.view_count, art.has_poster);
    match &art.source {
        ThreeDSource::Stream { shared } => {}       // /3D annot; shared = via /3DRef
        ThreeDSource::RichMediaAsset { name, filespec_id } => {} // name UNTRUSTED
        _ => {}
    }
    let got = extract_3d(&doc.view(), art)?;     // Result<Extracted3D, ThreeDError>
    if got.contradicts(art.declared.as_ref()) { disclose(); }  // declared != magic
}
```

Carriers: `/Subtype /3D` annotations (ISO 32000-1 §13.6, `/3DD` a 3D stream
or a `/3DRef`) and `/3D` instances in RichMedia annotations (ISO 32000-2
§13.7; each asset stream once per annotation). `ThreeDArtwork`,
`#[non_exhaustive]`: `page_index`, `annot_id`, `source`, `stream_id`,
`declared: Option<ThreeDFormat>` (stream `/Subtype`, else asset MIME, else
file extension), `view_count` (`/VA` length; 0 for RichMedia), `has_poster`
(`/AP /N`). `ThreeDFormat`: `U3d | Prc | Step | Other(Vec<u8>)`, with
`extension()` and `label()`. `Extracted3D { data, sniffed }` — `data` is
the filter-decoded model, untrusted and uninterpreted. `ThreeDNotes`:
`truncated`, `page_tree_unwalkable`, `annotations_without_stream`.
`ThreeDError`: `NoStream`, `StreamUnresolvable`, `SpanUnservable`, `Decode`.
Caps: `MAX_3D_ARTWORKS`, `MAX_RICH_MEDIA_ENTRIES`. `pdfcer-core` decodes no
model. CLI: `3d-list`, `3d-extract --index N -o FILE`.

**The opening view.** `threed::default_3d_view(&graph, &art) ->
Option<ThreeDSavedView>` — the view a reader opens on: the annotation's
`/3DV` (view dict, `/VA` index, `/IN`-or-`/XN` name, `/F` `/L` `/D`), else
the stream's `/DV`, else `/VA[0]` (§13.6.2 Table 298, Table 300). `None`
when nothing names one (the artwork's own camera applies), for a RichMedia
asset, or when `/3DV` is malformed. `ThreeDSavedView`, `#[non_exhaustive]`:
`name` (`/XN`), `camera_to_world: Option<[f64; 12]>` (`/C2W` when `/MS
/M`; columns 0-2, 3-5, 6-8 are the camera's x, y, z axes in world space,
9-11 its position; it looks along +z, and **its y column is the image's up**
— measured against an operator-confirmed CAD export, §13.6.5 does not say),
`orbit_distance` (`/CO`), `orthographic` (`/P /Subtype /O`),
`ortho_scale` (`/P /OS`, default 1), `ortho_binding: OrthoBinding`
(`/P /OB`: `Absolute` default, `Width`, `Height`, `Min`, `Max`;
`#[non_exhaustive]`), `view_box: Option<[f64; 2]>` (width and height of the
annotation's `/3DB`, else `/Rect`, in user space units; Table 305 scales
onto a target system centred on it). The standard gives `OS` no unit;
pdfcer reads a binding as the bound side spanning `1/OS` camera units, and
`Absolute` as one camera unit per `OS` user space units — an
interpretation, disclosed. CLI: `3d-render` with no camera option uses the
view's direction and projection, and for an orthographic view also its
centre (the camera axis) and that scale; perspective views are fitted to
the model. It prints `note: camera:` saying which view and scale were used.

#### Decoding and drawing a PRC model (`pdfcer-3d`, feature `3d`)

A separate crate, no GUI or network dependency, wasm-clean. Feed it
`Extracted3D::data` when `sniffed` is PRC.

```rust
use pdfcer_3d::{PrcFile, Tessellation, Camera, RenderOptions, render_coloured};

let prc = PrcFile::parse(&got.data)?;                 // Result<_, PrcError>
let tess = prc.file_structures[0].tessellations()?;   // Vec<Tessellation>, per file structure
let placements = prc.placements()?;                   // Vec<Placement { file_structure, tessellation, matrix, colour }>
// meshes = each placement's Tessellation::Mesh (or rebuilt Compressed { mesh: Some(..) })
//          .transformed(&placement.matrix)
let camera = Camera::fit_meshes(&meshes, view_dir, up, /*perspective*/ true, w as f64 / h as f64)?;
// colours[i]: placement.colour as straight RGBA bytes, None = RenderOptions::colour
let image = render_coloured(&meshes, &colours, &camera, &RenderOptions { width: w, height: h, ..Default::default() })?;
// image.rgba: w*h*4 straight RGBA, row-major from the top
```

- `render_coloured` is a CPU z-buffer: flat, double-sided shading lit from
  the eye. Opaque meshes first; translucent ones blend over them (hidden by
  nearer opaque surfaces, not by each other); alpha 0 is not drawn.
  `render(meshes, camera, options)` draws everything in the one
  `RenderOptions::colour`.
- `Placement::colour: Option<[f64; 4]>` is the part's colour from its tree
  (style inheritance and father/son heritage resolved, material diffuse and
  transparency applied), straight RGBA 0–1. `None`: no style reaches it or it
  names a textured material. Textures and lights are **not read** (the
  opening view IS read — see above). Say so in the shell.
- `Placement::triangle_colours(&mesh) -> Option<Vec<Option<[f64; 4]>>>`: per
  triangle of that placement's (untransformed) mesh, the colour when faces
  carry their own style (`TESS_Face` line attributes, taking part in the same
  inheritance); `None` = every triangle is `colour`. Draw by splitting the
  mesh into one mesh per distinct colour, as `3d-render` does. Not read yet
  for compressed (`Compressed { mesh: Some(..) }`) meshes.
- `Camera { eye, target, up, projection }`; `Projection::Perspective { fov_y }`
  (degrees) or `Orthographic { height }` (model units). `Camera::fit_meshes`
  frames every vertex as seen along `direction` (about 5% clear on each side
  of the limiting axis), aimed at the middle of the projected model; it errs
  `Camera` when there is no vertex. `Camera::fit(&Bounds, ..)` frames the
  box's eight corners instead (looser in oblique views; cheap for a huge
  model). Move `eye` afterwards to orbit.
- `RenderError::Size` past `MAX_RENDER_PIXELS` (64M) or a zero side;
  `RenderError::Camera` for a degenerate or non-finite camera. Non-finite
  vertices and out-of-range indices are skipped, not errors.
- Inferred content a shell must disclose: a `Compressed { mesh: Some(..) }`
  was rebuilt by pdfcer's reconstruction of an undocumented encoding, and
  `placements()` failing means meshes are drawn unplaced. `pdfcer 3d-render`
  and `3d-mesh` print both notes; copy their wording.
- A `Compressed { mesh: None, not_rebuilt: Some(why), .. }` is left out;
  `why` is a sentence fit to show. The CLI prints one
  `note: N compressed mesh(es) left out: <why>` per distinct reason.
- CLI: `3d-mesh -o FILE.stl|.obj`, `3d-render -o FILE.png [--view iso|front|..]
  [--up x|y|z] [--eye X,Y,Z] [--target X,Y,Z] [--ortho] [--fov DEG]`.

### 12.3 Optional-content layers

```rust
use pdfcer_core::layers::{read_layers, read_layers_with, list_layers, LayerScan};

let layers = read_layers(&doc);                 // layers.rs (full CatalogAndPages scan)
for l in &layers.layers {                        // Layer — layers.rs
    let name = &l.name;
    let on   = l.visible_by_default;             // — INITIAL /D state only
    let locked = l.locked;                        // — a UI hint, NOT enforced
    let _ = (l.radio_group, l.in_default_config, l.in_order);  //,,
    // Seed a properties window with the values set_layer_properties takes:
    // Option<LayerOutputState> / Option<LayerIntent>, None = a value pdfcer cannot name.
    let _ = (l.print, l.export, l.intent_kind);
}
let _ = (&layers.order, &layers.radio_groups, &layers.config_name, &layers.diagnostics);
```

`Layers`, `#[non_exhaustive]`. `OrderNode` (the `/D /Order`
tree, for a nested layers panel). `LayerDiagnostics` with
`is_faithful()`. `LayerScan`, `LayerSource`.
`list_layers` is the convenience form. Caps: `MAX_LAYERS`,
`MAX_ORDER_DEPTH`, `MAX_ORDER_NODES`, `MAX_RESOURCE_NODES`.

The visibility algebra lives in `annot`:
`optional_content_default_off(&graph)` — `annot.rs` — is the
**print/export-correct** OFF set. `oc_is_hidden(&graph, ocg, &off_set)` —
`annot.rs`. `apply_view_usage(&graph, …)` — `annot.rs` — refines
that for **on-screen View only**. `MAX_VE_DEPTH` — `annot.rs`.

### 12.4 Annotations

```rust
use pdfcer_core::annot::{page_annotations, page_annotations_with, Annotation, Appearance};
use pdfcer_core::page_tree::pages_in;

for page in &pages_in(&doc)? {
    for a in page_annotations(&doc, page.id) {       // annot.rs
        if let Some(rect) = a.rect {                  // PDF USER SPACE, y-UP, points
            draw_marker(rect);
        }
        let _ = (a.subtype_label(), a.is_widget(), a.contents.as_deref(), a.title.as_deref());
    }
}
```

`Annotation` — `annot.rs`: `id`, `subtype`, `rect: Option<Rect>`,
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
absent → `None`, never an empty list.
`border_dash: Option<annot_author::BorderDash>` (`/BS`, §12.5.4 Table 166;
read with `BorderDash::pattern()`) is the dashed-border pattern restyle,
resize, reshape and paste preserve: `/S /D` alone reads as `[3]`, a `/D`
array without `/S` reads as that pattern, any other style is `None`.
`rich_contents: Option<RichText>` is `/RC` (Table 170, §12.7.3.4), the
comment's XHTML rich-text twin of `contents`: `RichText::Inline(String)` for
a text string, `RichText::Stream(ObjId)` for a text stream (§7.9.3). Resolve
either form with `annot::rich_text_in(graph, source, &rich) -> Option<String>`
(decodes the stream's filters, then the text-string encoding).
`default_style: Option<String>` is `/DS`, the CSS default style. Both are
read for every subtype and can disagree with `contents`; they are reported as
the file carries them. `AnnotFlags::locked_contents()` (bit 10,
value **512**) joined `locked()` (bit 8, 128) — two gates, see part 2 §1.15.
**`Pass 155.2` (`pdfcer-gui` request 2026-09-07) added the ORIENTATION**, which
nothing on this struct carried before — so no shell could show an annotation's
angle, type into it, or draw a selection outline that followed the object:

* `appearance_matrix: Option<[f64; 6]>` — the `/Matrix` of the **selected**
  appearance stream (Table 95), **raw**. Raw for `color`'s reason, given back
  to pdfcer by the requester: a shell handed six numbers can tell a rotation
  from a skew and disclose the difference; one handed `Option<f64>` cannot
  separate *"not rotated"* from *"rotated in a way we declined to describe"*.
  §12.5.2 requires `/Rect` upright, so **this is the only place an
  annotation's orientation exists**. `None` when there is no selected normal
  appearance, or the stream's `/Matrix` is absent or malformed — the last two
  collapse deliberately, because Table 95 gives both the identity and that is
  what `pdfcer-render` paints with.
* `appearance_rotation_degrees() -> Option<f64>` — the **effective** angle,
  anticlockwise in `(−180, 180]` — **SIGNED, and pdfcer's other rotation
  reader is not: `WidgetRotation::was`/`::now` are `[0, 360)`.** The two
  differ for a real reason (`/MK /R` is a stored declaration pdfcer
  normalises; this decomposes a matrix through `atan2`, and forcing it
  positive would make a 1° clockwise nudge read as `359`), but a consumer who
  learns one and generalises to the other is wrong about **every clockwise
  angle** — `pdfcer-gui` did exactly that within an hour of this shipping and
  every turned markup reported itself upright, with 3,860 of their in-process
  tests green. Want `[0, 360)`? `θ.rem_euclid(360.0)`, at the boundary where
  the convention changes. pdfcer deliberately ships no second accessor: two
  functions answering *"what angle is this"* is the `R243` shape.
  `Some(0.0)` where the field is `None` but
  there IS an appearance (Table 95's default), so an ordinary unrotated
  annotation reads as `0°` rather than as a blank. `None` for a shear, a
  mirror or a non-uniform scale: **those are not angles**, and a confident
  wrong number here would be seeded into a properties field the operator is
  about to commit. Both this and `EditSession::set_annotation_rotation` go
  through `annot::rotation_degrees([f64; 6])` — **one function, because the
  reader seeds the field the writer commits, and a tolerance's worth of
  disagreement would move the object on an unedited Enter.**

★ **The placement is a `pdfcer-render` function, not a field**:
`pdfcer_render::annot::appearance_placement(&DocumentView, &Annotation) ->
Option<[(f64, f64); 4]>` returns the `/BBox` corners after §12.5.5's full
algorithm, in default user space, in `/BBox` corner order (LL, LR, UR, UL
*of the artwork*, not of the screen). That is where a **selection outline**, a
**rotate grip** and a **hit test** belong; all three are otherwise computed
from `/Rect`, which §12.5.2 forces upright and is therefore wrong on every
turned annotation. `None` for no `/Rect`, no reachable appearance stream, no
readable `/BBox`, or a degenerate transformed box (step (b) singular).
The quad ignores `NoRotate` / `NoZoom` (and a `/Text` annotation's implied
both, §12.5.6.4): they act on the page-to-device transform, which this
function never sees. The renderer keeps such an appearance upright and at
100% size, pivoted on the `/Rect` upper-left corner (§12.5.3); a shell
drawing an outline on a turned page, or at a zoom with `NoZoom`, applies
that same pivot to the quad.

**`/F` IS NOW WRITABLE (2026-09-08).** `AnnotFlags` carried eight read
accessors and no writer — `EditSession::set_annotation_flags` is the other
half. Until it existed an operator could see a markup was hidden and not
un-hide it, and **could not lock anything**, so pdfcer's own Locked gate was
unreachable from pdfcer. ★ Four transform verbs (`move_annotation`,
`resize_annotation`, `rotate_annotation`, `set_annotation_rotation`) **also
ignored the Locked flag until that date**, while `set_markup_style`,
`reshape_annotation` and the deletion guards honoured it — so a Locked markup
could not be recoloured and could be dragged anywhere, which is inverted from
Table 165 bit 8's own words (*"including position and size"*). Both halves
fixed together. `LockedContents` (bit 10) deliberately still does **not**
block a transform: it guards the text.

**Five verbs documented `EditError::DocumentEncrypted` and none enforced it**
until the same date — `rotate_annotation`, `set_annotation_rotation`,
`resize_annotation`, `move_annotation`, `set_markup_note`. They do now, and
the guard is placed **before** subtype routing, so an encrypted document is
named as such rather than being answered with *"use rotate_widget instead"*.

Methods: `is_widget()`, `is_group_subordinate()`,
`effective_reply_type()`, `subtype_label()`,
`appearance_rotation_degrees()`.
`AnnotFlags(pub u32)`, `Appearance`, `ReplyType`.
`page_annotations_with` takes a `MissingAppearanceState` policy.

**`forms::Widget` — the `/MK` colour pair, both directions (2026-09-08).**
`::background` (`/MK` `/BG`) and the NEW `::border_color` (`/MK` `/BC`), each
`Option<MkColor>` with the three-state contract: `None` = key absent,
`Some(MkColor::None)` = Table 189's empty array *stating* no colour,
DeviceCMYK never pre-converted. ★ Until this date **read and write sat on
opposite keys of one dictionary**: pdfcer WROTE `/BC` (hard-coded black, never
settable) and never read it, READ `/BG` and never wrote it, so neither
round-tripped. Write side: `WidgetEdit::{background, border_color}`, now
`Option<MkColorEdit>` so a key can be REMOVED as well as set (`Pass 308.3`),
and `MkColor::to_array()`, the exact inverse of `from_array`. CLI:
`edit-widget --background/--border-color`, taking `none` (the empty array) and
`unset` (remove the key) as different words, plus the same two flags on all
five `add-*` verbs (`Pass 308.1`); `list-fields --widgets` prints both.
★ **RETIRED 2026-09-15.** This paragraph ended:
~~"⚠️ pdfcer's own renderer does not paint `/MK` colours (R43,
named-not-painted) — the value is in the file for viewers that honour it."~~
`Pass 308.0` bakes both colours into the `/AP`, so pdfcer paints them like
anything else and a colour-only edit redraws. R43 still holds in general —
pdfcer does not reconstruct an appearance from `/MK` at DISPLAY time; what
changed is that the appearance BUILDER now takes the colour, per call site, so
a fill is untouched.

**`annot_author::CheckStyle` (2026-09-08).** Six check-box/radio glyphs —
`Check` `Cross` `Star` `Circle` `Square` `Diamond`, default `Check` — with
`mk_caption_char()` / `from_mk_caption_char()` / `parse()` / `as_str()` /
`all()`. The `/MK` `/CA` characters (`4 8 H l n u`) come from Adobe's own
`ZapfDingbats.afm` and the Adobe Glyph List. ★★ pdfcer **draws each as vector
artwork** rather than selecting a ZapfDingbats font as Acrobat does — Acrobat
and Reader have a recurring bug failing to resolve that font, which leaves the
box blank; paths need none. `/MK` `/CA` is written anyway, both for interop
and because pdfcer's own resize recovers the style from it.
`from_mk_caption_char` returns `None` for an unrecognised character rather
than defaulting, because Table 189 constrains `/CA` not at all.
`need_appearances(&graph)` checks `/AcroForm /NeedAppearances`.
`MAX_ANNOTS_PER_PAGE` = 1,000,000.

**Coordinates:** `annot.rs` — *"The `/Rect` in default user space,
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
let reader = DestinationReader::new(&doc);          // outline.rs

for page in &pages_in(&doc)? {
    let found = page_link_destinations(&doc, page.id, &reader);   // annot.rs
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

`DestinationReader` — `outline.rs`, with `new`,
`page_tree_error()`, `named_destination_count()`,
`destination(&graph, carrier_dict)`, and
`destination_with_diagnostics` when you want the
`OutlineDiagnostics` the read produced.

`page_link_destinations(&graph, page_id, &reader)` — `annot.rs` →
`PageLinks` (`links: Vec<LinkDestination>`,
`links_without_destination: usize`). `LinkDestination` carries
`annots_index` (the `/Annots` position — **not** its position in
`links`), `id`, `rect`, `destination`.

`Annotation::destination(&graph, &reader)` — `annot.rs` — resolves a
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
`DestinationResolver` — `pageops/references.rs`, with `new`,
`named_count`, `names_targeting`, `resolve_destination`,
`resolve_target`. Also `census_dangling` → `DanglingReport`
, useful for a document-health panel.

### 12.5 Signatures — census, coverage, and (`Pass 10.1`) integrity verification

```rust
let census = pdfcer_core::signature::census(&doc);              // signature.rs
let cov = pdfcer_core::signature::byte_range_coverage(&doc, /* … */);  // signature.rs
// Pass 10.1 — verification. `bytes` is the FILE the graph was loaded from
// (`Document::bytes()`); the digest is over those bytes, not over objects.
let verdicts = pdfcer_core::signature::verify_all(&doc.view(), doc.bytes());
let one = pdfcer_core::signature::verify(&doc.view(), doc.bytes(), 0);   // Option<SignatureVerdict>
```

`SignatureCensus`, `SignatureImpact`, `ImpactBasis`,
`SaveMode`, `ByteRangeCoverage`. This tells you what signing
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
| `trust` | `Trust` — `NotChecked` unless anchors are supplied (`Pass 10.3`), then `Trusted`/`Untrusted`/`SignerUnknown` | `verify_all_with_trust` + a trust-anchor pool; chain, constraints and validity dates at the reference clock (below); NOT revocation |
| `revocation` | `Revocation` — `NotChecked` / `Good { checked }` / `Revoked { … }` / `Undetermined { reason }` (`Pass 10.16`) | what RFC 5280 §5 CRLs and RFC 6960 OCSP responses say about the signer's chain at the reference clock (OCSP: `Pass 10.17`); see §12.5c |

Plus claims — `signer_subject`, `signer_issuer`, `cert_not_before`,
`cert_not_after`, `signing_time`, and the dictionary's `name`/`date`/
`reason`/`location` — and `notes` (a SHA-1 digest, non-zero padding, extra
`/ByteRange` gaps, an ETSI signature that does not reach EOF, extra signers).

**`revocation_sources: Vec<RevocationSources>`** — where each embedded
certificate says its revocation status can be fetched (RFC 5280 §4.2.1.13
`cRLDistributionPoints`, §4.2.2.1 `authorityInfoAccess`), signer first, then
the others in CMS order; a certificate naming nothing is omitted.
`signature::RevocationSources` (`#[non_exhaustive]`, read-only) has `subject`, `crl`,
`ocsp`, `ca_issuers` (URI lists, printable ASCII, ≤ 16 each) and
`unreadable` (entries present but not kept — a directory name, a non-ASCII
URI, one past the cap, malformed DER); `is_empty()`. **Nothing is fetched**:
core has no network — these are the URLs a shell would fetch if the operator
asks, then hand back through `SuppliedRevocation` (§12.5c). Render them as the
certificate's statement, never as a revocation result. The CLI prints one
`revocation-source:` line per entry, ending `(stated by the certificate,
NOT fetched)`. The raw per-certificate form is `cms::Certificate::revocation_uris`
(`RevocationUris`, same fields without `subject`).

Implemented: `adbe.pkcs7.detached`, `ETSI.CAdES.detached`, `adbe.pkcs7.sha1`
(the double hash — the inner SHA-1 is pinned by the subfilter), and
`ETSI.RFC3161` document time-stamps (the token's `messageImprint` must be
the byte-range digest; `signing_time` is the TSA's `genTime`); RSA PKCS#1
v1.5 and RSASSA-PSS, ECDSA P-256/P-384; SHA-1/256/384/512.
`adbe.x509.rsa_sha1`, P-521, Brainpool → `Unverifiable` by
name. All workspace code (`asn1`, `cms` in `crates/pdfcer-pkix/src/`;
`crypto::{bignum,rsa,ecdsa,sha1}` in `pdfcer-model`), no third-party dependency; verified against pyHanko-signed fixtures whose expected
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
`pdfcer_core::trust_store` (source: `crates/pdfcer-pkix/src/`, re-exported with
`trust_chain`) supplies one by reading the AATL + EU-Trusted-List
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
  against the reference clock (`false` ⇒ no clock, so expiry was not
  verified). Revocation is the separate `revocation` field (§12.5c).
- **Reference clock:** the CMS `signingTime`, else the dictionary's `/M`
  converted to UTC (ISO 32000-1 §7.9.4; a date with no UT offset gives no
  clock). PAdES forbids `signingTime` (ETSI EN 319 142-1 §6.3), so for
  `ETSI.CAdES.detached` `/M` is usually the clock; a `clock:` note says so
  whenever anchors, CRLs or OCSP responses were used. Both times are the signer's claim.
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
does **not** check **revocation**, so `revocation_checked` is always `false`;
CRL/OCSP checking is `SignatureVerdict::revocation` (§12.5c), a separate axis.
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

### 12.5c Revocation from CRLs and OCSP (`Pass 10.16`, `Pass 10.17`)

```rust
use pdfcer_core::signature::{verify_all_with_revocation, Revocation, SuppliedRevocation};
let supplied = SuppliedRevocation::new()
    .with_crl(crl_der)        // repeatable; DER CertificateList
    .with_ocsp(ocsp_der);     // repeatable; DER OCSPResponse or bare BasicOCSPResponse
let verdicts = verify_all_with_revocation(&doc.view(), &bytes, anchors, &supplied);
```

`verify_all` and `verify_all_with_trust` are this with nothing supplied (they
still read the document's `/DSS`). Each certificate from the signer up to the
root — or up to a supplied anchor — must be covered, at the reference clock
(§12.5b), by either:
- a **CRL** its issuer signed (RFC 5280 §6.3): complete, not delta or
  indirect, no unsupported critical extension; or
- an **OCSP response** (RFC 6960) answering for exactly this certificate
  (all four CertID fields), signed by the issuer or by a delegated responder
  the issuer issued with `id-kp-OCSPSigning` (§4.2.2.2). A delegate must be
  valid at `producedAt` and either carry `id-pkix-ocsp-nocheck` or be shown
  not revoked by a usable CRL. `unknown`, a non-`successful` status
  (`tryLater`…) and an expired `nextUpdate` are unusable.

Any usable answer saying revoked wins over every good one; otherwise the
newest good answer is reported. Evidence is indexed per kind: the document's
`/DSS /CRLs` and `/DSS /OCSPs` (ETSI EN 319 142-1 §5.4.2.2) first, then the
supplied ones. Issuers are found among the CMS certificates, `/DSS /Certs`,
then the anchors. **pdfcer-core fetches nothing.** Nonces and the DSS `/VRI`
dictionary are not read.

`Revocation` (`#[non_exhaustive]`):
- `NotChecked` — no CRL and no OCSP response in the document or supplied.
- `Good { checked: Vec<RevocationCheck> }` — each non-root certificate:
  `subject`, `kind: RevocationKind` (`Crl`/`Ocsp`, `as_str()` → `"CRL"`/
  `"OCSP"`), `source: RevocationSource` (`Dss`/`Supplied`, `as_str()`),
  `this_update`, `next_update` (UTC ISO; `None` = none stated).
- `Revoked { subject, date, reason, before_signing: Option<bool>, kind, source }`
  — `reason` is the RFC 5280 §5.3.1 name (`"keyCompromise"`…); `before_signing`
  places the revocation against the reference clock (`None` = no clock).
  Revoked-after-signing is still `Revoked`; the shell says which side.
- `Undetermined { reason }` — evidence exists but cannot decide: nothing
  covers a certificate, a signature does not verify, it expired before the
  clock, an issuer is not available, an OCSP responder is not authorised or
  does not know the certificate, a critical extension pdfcer does not support.

Revocation never changes `integrity`. The CLI prints a `revocation:` line
naming `CRL` or `OCSP`, and takes `verify-signatures --crl FILE` and
`--ocsp FILE` (each repeatable; unreadable → exit 3); the exit code stays
integrity's.

### 12.5d Writing revocation evidence — PAdES B-LT (`Pass 10.18`)

The write half of §12.5c: `EditSession::add_validation_material` puts
certificates, CRLs and OCSP responses into `/DSS` so the §12.5c verdict comes
back `RevocationSource::Dss` with nothing supplied. Contract, errors and the
`/P 1` override: `02-editing-and-saving.md` §1, "Embed validation material".
Types: `pdfcer_core::sign::ltv::{ValidationMaterial, DssReport, MaterialKind}`.

### 12.6 ★ Document metadata — the honest gap

**There is no `Document`-level `/Info` accessor, and no XMP reader at all.**

The only public `/Info` reader is on `EditSession`:

```rust
use pdfcer_core::edit::{EditSession, InfoField};
let session = EditSession::new(doc);                       // edit.rs — takes ownership
let title = session.info_text(InfoField::Title);            // edit.rs -> Option<InfoText>
let raw   = session.info_bytes(InfoField::Title);           // edit.rs
```

`InfoField` — `edit.rs`, `#[non_exhaustive]`: **only** `Title`,
`Author`, `Subject`, `Keywords`. `InfoText` — `edit.rs`:
`{text: String, exact: bool}`.

Three consequences a GUI must plan for:

- **`/Producer` is deliberately excluded** (`edit.rs`, rule R41):
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
`references.rs`). Nothing turns the number tree into `"iii"` / `"A-1"`
strings. If your page rail shows document page labels rather than ordinals,
you are writing that yourself.

### 12.7 Traps — navigation and metadata

- **★ T-12.1 No `/Dest` or `/A` on `Annotation`** — §12.4. Hyperlinks need
  the `DestinationResolver` route.
- **★ T-12.2 `DestView::FitR` is NOT a `/Rect` and must not be
  normalised.** `outline.rs`: *"Reusing a normalising rectangle
  parser here would silently reorder a destination the producer wrote
  deliberately, so do not assume `left < right` or `bottom < top`."*
- **T-12.3 Bold/italic bit order is reversed from intuition** — italic is
  bit 1, bold is bit 2 (`outline.rs`). Use `is_bold()`/`is_italic()`,
  never raw bit math.
- **★ T-12.4 Attachment encryption is silently invisible.**
  `attachments.rs`: since PDF 1.5 an embedded file *"can be
  encrypted in an otherwise unencrypted document"*, and the intuitive guard
  is *"wrong silently: the `/Filter` chain runs, produces bytes, and those
  bytes are garbage that looks like a successful extraction."* Check
  `AttachmentNotes::may_be_encrypted`. pdfcer-core does not decrypt on this
  path.
- **T-12.5 `extract_attachment`'s `view` must come from the same document
  the listing came from.** `attachments.rs`: a mismatch usually
  errors but *could* silently return another document's bytes at a colliding
  id — *"pdfcer cannot detect the confusion … the obligation is the
  caller's."*
- **T-12.6 `Attachment::kind::PageAnnotation{page_index}` is a snapshot;
  `page_id` is the stable key.** `attachments.rs`. Use `page_id`
  for identity, `page_index` only for display order at read time.
- **T-12.7 `/RF` related files are silently unmodelled** — a known gap, not
  a bug (`attachments.rs`).
- **★ T-12.8 `apply_view_usage` must NEVER be reachable from a print or
  export path.** `annot.rs` quotes §8.11.4.5: printing
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
- **T-12.10 `/AllOff` with every member off is VISIBLE** (`annot.rs`) —
  a counter-intuitive OCMD rule, marked `★` in source.
- **T-12.11 An empty configuration `/Intent` array means EVERYTHING is
  visible** (`annot.rs`) — fewer intents means *more*
  visible, not "no filter".
- **T-12.12 `Zoom` usage category is half-open `[min, max)`** — `max` is
  exclusive (`annot.rs`).
- **T-12.13 Usage-application conjunction is global and order-independent
  (OFF dominates), which is the OPPOSITE algebra from `/D /ON`//`/OFF`
  arrays, where order IS load-bearing** (`annot.rs`,
  decision 038). Do not carry logic across that boundary.
- **T-12.14 `Annotation::mod_date` is a raw unparsed string** —
  §12.5.2 requires accepting any format. Parse defensively or display
  verbatim.
- **T-12.15 `Layer::locked` is a UI hint and is not enforced anywhere**
  (`layers.rs`).

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

let store = settings::resolve_store();              // settings/mod.rs
let (cfg, report) = Settings::load(store.clone());   // settings/mod.rs
// … mutate cfg …
cfg.save(&store)?;                                   // settings/mod.rs
```

`resolve_store() -> StoreLocation` · `store_in(&Path)` ·
`StoreLocation` · `StoreKind` · `Settings` ·
`Settings::load` · `::parse` · `::write_to_string` ·
`::save` · `LoadReport` · `LoadReport::stated` · `::was_stated` · `SettingNote` ·
`SaveError`.

**Store location** (`settings/mod.rs`, verified): portable first —
`<directory of the running executable>/userdata/settings.txt`, used when
that directory is writable (`StoreKind::Portable`); otherwise the platform
config dir (`StoreKind::PlatformFallback`); otherwise
`StoreKind::None` with `path: None`. `store_in(&Path)` exists for tests and
a future `--user-data-dir` override. This ordering is what makes the
single-folder portable packaging (`ARCHITECTURE.md` §6) actually portable —
do not reverse it.

The format is a deliberately non-`serde` flat `key = value` grammar, and
parsing is **fail-soft per key, not per document** (`settings/mod.rs`):
an unrecognised or malformed line becomes a `SettingNote` in the
`LoadReport` and the rest of the file still loads. Surface the notes; do not
discard them.

`LoadReport::stated` / `::was_stated(key)` name the keys the file actually
set (a value that took, clamped included; not a bad value or unknown key).
Use it to tell "absent, defaulted" from "stated at the default" -- never
re-default or migrate a stated key, and never copy a pdfcer default into the
shell to reconstruct the difference. A file written by `save` states every key.

Several settings exist specifically because the standard is ambiguous and
the operator's standing directive (2026-08-08, R169) is that ambiguity
becomes a user choice rather than a hard-coded default. The full list, with
line numbers re-measured 2026-08-25:

| setting | the silence it fills |
|---|---|
| `CmykIntent` | §8.6.4.4 defines no CMYK-to-screen conversion at all |
| `PageBlendSpaceSource` | `PGB-A1` — where a page's blending space comes from when its group declares none |
| `MeshPatchPadding` | `MSH-A1` — what a type 6/7 mesh-shading PATCH record pads to, when the clause states the rule for a vertex |
| `MaskResample` | `SM-A1` — which filter resamples a size-mismatched `/SMask` |
| `MinifyFilter` | `IM-A1` — how an image drawn smaller than its pixel grid is sampled |
| `CmykJpegPolarity` | `DCT-A1` — how a CMYK JPEG with no `/Decode` is read |
| `UnmappableCode` | what stands in for a character no `/ToUnicode` covers |
| `ActualTextPrecedence` | whether `/ActualText` overrides the glyphs beneath it |
| `MissingAppearanceState` | `AS-A1` — which appearance a widget with no `/AS` shows |
| `QuadPointOrder` | the two orderings real producers write for `/QuadPoints` |
| `XrefEntryEol` | the two-byte end-of-line a classic xref entry uses |
| `TrailingEol` | whether a saved file ends with a newline |

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
`settings/mod.rs`). Read it before wiring a preferences dialog; the
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
- **Saving** — `writer`, `Document::save_incremental` (`document.rs`),
  `Document::save_full` (`document.rs`), the round-trip and
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
  `pdfcer_core` emits a draw-op stream and *"never pixels"* (`lib.rs`).
  Its `RenderOptions`, `LayerVisibility`, `RenderCancel`, `Diagnostics` and
  the bundled Base-14 substitute faces are part 3.
