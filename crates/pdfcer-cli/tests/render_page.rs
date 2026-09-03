//! # `pdfcer render-page` integration tests
//!
//! Black-box tests: they spawn the **real binary** (via Cargo's
//! `CARGO_BIN_EXE_<name>` env var, set for integration tests of any crate
//! with a `[[bin]]`) and assert on its process contract — exit code,
//! stdout bytes, stderr bytes, and the file it wrote. That is deliberate.
//! The unit tests in `main.rs` cover the pure mapping functions; what
//! *this* file exists to protect is the part a script depends on and a
//! refactor cannot see: the exit-code table (docs/ARCHITECTURE.md §7) and
//! the stable stdout result line (see `main.rs`'s module header, "stdout
//! result-line format").
//!
//! ## Why the fixtures are built inline
//!
//! Every PDF used here is assembled byte-by-byte by [`build_pdf`] below,
//! in-process, and written to a temp file. Two reasons, both binding:
//!
//! - **docs/LEGAL.md §5**: test-corpus PDFs are synthetic or clearly
//!   rights-cleared, never a downloaded real-world file of unknown
//!   provenance. Generating the bytes here makes provenance a
//!   non-question.
//! - **Legibility**: the exact structure under test (how many pages, what
//!   the content stream draws, what the MediaBox is) is visible at the
//!   call site instead of hidden in an opaque binary blob a future reader
//!   would have to hex-dump to understand.
//!
//! The builder emits a classic §7.5.4 cross-reference **table** rather
//! than a §7.5.8 xref stream. Both are supported by `pdfcer-core`; the
//! classic form is used here purely because it is readable in the test
//! source (whole-file coverage for the PDF 1.5 forms lives in
//! `crates/pdfcer-core/tests/pdf15_streams.rs`).
//!
//! ## Why no `tempfile` dependency
//!
//! [`TempDir`] below is ~30 lines and adds zero packages to the
//! dependency graph (docs/LEGAL.md §6: every dependency is a license
//! classification and an attribution entry). Uniqueness comes from
//! process id + a monotonic counter + nanosecond clock, which is
//! sufficient for a test harness that owns the paths it creates, and the
//! `Drop` impl cleans up even when an assertion panics.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Path to the freshly built `pdfcer` binary. Cargo sets this for
/// integration tests, so the test always exercises the binary produced by
/// the same build — never a stale one on `PATH`.
const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// Assemble a syntactically complete single-generation PDF from a list of
/// `(object number, body)` pairs, appending a classic cross-reference
/// table and a trailer that names `1 0 R` as the catalog.
///
/// The layout follows §7.5: header, body, `xref` section with one
/// subsection covering objects `0..=n`, `trailer`, `startxref`, `%%EOF`.
/// Offsets are recorded as each object is emitted, so the table is
/// correct by construction rather than by hand-counting — which matters,
/// because `pdfcer-core` is strict: a wrong offset is a load failure, not
/// a warning.
///
/// Free entry `0` is emitted as the spec's mandatory
/// `0000000000 65535 f` head-of-free-list. Entries are exactly 20 bytes
/// each including the `\r\n` terminator, as §7.5.4 requires.
fn build_pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let size = objects.len() + 1; // +1 for the free object 0
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for num in 1..=objects.len() as u32 {
        let (_, off) = offsets
            .iter()
            .find(|(n, _)| *n == num)
            .expect("object numbers must be 1..=n and contiguous");
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A document with `contents.len()` pages, page *i* drawing
/// `contents[i]`, all sharing a 200x100 MediaBox.
///
/// The non-square box is on purpose: it makes the `WxH` half of the
/// stdout line assert something real. A square page would pass even if
/// width and height were transposed somewhere in the geometry chain.
fn multipage_pdf(contents: &[&str]) -> Vec<u8> {
    // Object numbering: 1 = catalog, 2 = page-tree root,
    // then per page i: page dict at 3+2i, content stream at 4+2i.
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", 3 + 2 * i))
        .collect();
    let mut objects: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 200 100] \
                 /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 \
                 /BaseFont /Helvetica >> >> >> >>",
                kids.join(" "),
                contents.len()
            ),
        ),
    ];
    for (i, content) in contents.iter().enumerate() {
        let page_num = 3 + 2 * i as u32;
        let stream_num = page_num + 1;
        objects.push((
            page_num,
            format!("<< /Type /Page /Parent 2 0 R /Contents {stream_num} 0 R >>"),
        ));
        objects.push((
            stream_num,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ));
    }
    build_pdf(&objects)
}

// ---------------------------------------------------------------------------
// Temp-directory scaffolding
// ---------------------------------------------------------------------------

/// A uniquely named directory under the system temp dir, removed on drop.
///
/// Uniqueness is process id + nanosecond clock + a per-process counter:
/// the pid separates concurrent `cargo test` invocations, the counter
/// separates tests within one process (Rust runs them on parallel
/// threads, so two could otherwise read the same clock tick), and the
/// clock separates sequential runs that reuse a pid.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "pdfcer-test-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("could not create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Write `bytes` to `name` inside this directory and return the path.
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.join(name);
        std::fs::write(&p, bytes).expect("could not write fixture");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best-effort: a failure here must not mask the test's own
        // failure, so the result is deliberately discarded.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Process helpers
// ---------------------------------------------------------------------------

/// Run `pdfcer` with `args` and capture the whole process outcome.
fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("could not spawn pdfcer")
}

/// Exit code as `u8`, matching the [`exit`] table's own type. A process
/// killed by a signal (no code) fails the test loudly rather than
/// silently comparing against a default.
fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("pdfcer terminated without an exit code (signal?)")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout must be valid UTF-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr must be valid UTF-8")
}

/// The eight-byte PNG signature (RFC 2083 §3.1 / W3C PNG §5.2). Checking
/// it proves the file is a PNG and not, say, a zero-length file left
/// behind by a failed write — without pulling in an image-decoding
/// dependency just to assert "yes, that is a PNG".
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// Assert `path` exists and begins with the PNG signature, and return its
/// declared width and height read from the IHDR chunk.
///
/// IHDR is required by the format to be the **first** chunk, so its
/// dimensions live at fixed offsets 16..20 (width) and 20..24 (height),
/// big-endian — a stable enough guarantee to check without a decoder.
fn png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).expect("output PNG was not written");
    assert!(
        bytes.starts_with(&PNG_MAGIC),
        "output file is not a PNG (first bytes: {:?})",
        &bytes[..bytes.len().min(8)]
    );
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn renders_a_single_page_to_png_with_the_stable_stdout_line() {
    let dir = TempDir::new("ok");
    let pdf = dir.write("one.pdf", &multipage_pdf(&["0 0 0 rg 10 10 50 50 re f"]));
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);

    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    // The page is 200x100 user units; the default scale is 1.0, so the
    // raster is 200x100 device pixels. Asserting BOTH the stdout line and
    // the PNG's own IHDR catches a mismatch between what the CLI reports
    // and what it actually wrote.
    assert_eq!(png_dimensions(&png), (200, 100));

    let line = stdout(&out);
    assert!(
        line.ends_with('\n') && line.matches('\n').count() == 1,
        "stdout must be exactly one LF-terminated line, got {line:?}"
    );
    assert!(
        line.starts_with("rendered "),
        "unexpected stdout prefix: {line:?}"
    );
    assert!(
        line.contains(" page 1 -> "),
        "stdout must name the 1-based page: {line:?}"
    );
    assert!(
        line.contains(" 200x100; "),
        "stdout must carry WxH: {line:?}"
    );

    // The metrics half: everything after the first "; " parses as
    // key=integer pairs in the documented order. This is the exact
    // parsing recipe the module docs promise a script can use.
    let metrics = line.trim_end().split("; ").nth(1).expect("metrics half");
    let keys: Vec<&str> = metrics
        .split(' ')
        .map(|kv| kv.split('=').next().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "substituted",
            "notdef",
            "unsupported",
            "unknown",
            "deferred",
            // Appended by the XObject/image slice. The contract permits
            // APPENDING keys; the five above never move.
            "images",
            "images_unsupported",
            "forms",
            // `forms_culled` sits BESIDE `forms` rather than at the end
            // of the line, which is a deliberate departure from the
            // append-only habit above. What this test actually guards is
            // that the first five keys never move and that any addition
            // is a decision someone made on purpose; the metrics half is
            // parsed as `key=value` pairs, so a consumer keying off
            // ordinal position is already broken. The other audience is a
            // human scanning the line, and "342 forms executed, 0 culled"
            // only reads as a pair when the two are adjacent.
            "forms_culled",
            // `Pass 74.9`: the opt-in lossy cull. Beside its exact
            // sibling on purpose -- the pair is only readable together,
            // since one changes no pixel and the other does.
            "subpixel_culled",
            // Appended by Pass 2.1's image-codec slice (decision 005
            // §6.4). Same rule again: appended at the END, and every
            // key above keeps its meaning and its position.
            "images_codec_unsupported",
            "codec_features",
            "codec_geometry_mismatch",
            "dct_cmyk",
            "lzw_anomalies",
            // Appended by decision 006's diagnostic split: the benign
            // YCCK census stayed in `dct_cmyk` (same key, now verified
            // neutral) and the R30 polarity-unverifiable shape got its
            // own key, appended at the END per the contract.
            "dct_cmyk_unverifiable",
            // Appended by Pass 2.3's JPXDecode slice: Table 89's
            // /SMaskInData 2, where the codestream's colour channels
            // arrive preblended with a backdrop. Appended at the END,
            // same contract.
            "jpx_preblended",
            // Appended by Pass 6.0's annotation-appearance slice
            // (docs/decisions/008). Eight keys, appended at the END, same
            // contract — every key above keeps its meaning and position.
            // `annots_no_ap` is a SUM of the per-subtype
            // `annotations_without_ap` map (the per-subtype breakdown is a
            // stderr note); `need_appearances` is the document-scoped
            // /AcroForm /NeedAppearances disclosure (R51).
            "annots",
            "annots_painted",
            "annots_no_ap",
            "annots_hidden",
            "annots_state_missing",
            "annots_widget",
            "annots_degenerate",
            // `Pass 74.8`: both were computed, merged and unit-tested in
            // `pdfcer-render` while being printed nowhere, so
            // `--no-annotations` withheld content and reported no number
            // saying how much. Inserted beside the annotation family for
            // the same reason `forms_culled` sits beside `forms`.
            "annots_out_of_scope",
            "page_content_suppressed",
            "need_appearances",
            // Appended by the font-diagnostics by-reason split: the
            // per-reason breakdown of `unsupported`
            // (`fonts_unsupported_by_reason`). Six keys, appended at the
            // END, same contract — their sum equals `unsupported`, and
            // every key above keeps its meaning and position.
            "unsupported_type3",
            "unsupported_noncmap",
            "unsupported_vertical",
            "unsupported_composite_not_embedded",
            "unsupported_unknown_subtype",
            "unsupported_unusable_program",
            // Appended by decision 012's supplied-fonts slice: `supplied`
            // is glyphs drawn from an operator-supplied `--font-dir` face
            // (the third trust level, R62); `supplied_registered` is the
            // count of name→file registrations the walk added. Appended at
            // the END, same contract — every key above keeps its meaning
            // and position.
            "supplied",
            "supplied_registered",
            // Appended by the /Contents-degradation slice: `/Contents`
            // entries this page named that are not present in the file, so
            // their marks are missing from the raster (ISO 32000-1 §7.3.10
            // makes such a reference the null object; Table 30 makes an
            // absent /Contents an empty page — the document opens, but the
            // page is incomplete and says so). Appended at the END, same
            // contract — every key above keeps its meaning and position.
            "contents_unresolved",
            // Appended by the image-transparency slice (§8.9.6,
            // §11.6.5.3): `/SMask`, `/Mask` (stencil and colour-key) and
            // `/Matte` compositing. Five keys, appended at the END, same
            // contract — every key above keeps its meaning and position.
            //
            // `images_masked` is CENSUS, a subset of `images`: how many
            // images had their transparency actually composited. Its
            // shortfall twin is `images_mask_unsupported` — the image IS
            // on the page but too solid, which is a different operator
            // question from `images_unsupported`'s "the image is missing".
            // The per-mechanism split (smask / stencil / colour-key /
            // jpx-embedded-alpha) and the per-reason refusal breakdown
            // both go to stderr, where a new key cannot break a parser.
            "images_masked",
            "images_mask_unsupported",
            "masks_resampled",
            "mattes_undone",
            "mattes_not_undone",
            // §8.11.3.2 hidden layers, appended 2026-08-10. Appended, never
            // inserted: this list IS the contract, and a caller that
            // splits on position breaks the moment a key moves.
            "oc_hidden",
            // §8.6 colour, appended 2026-08-17. The whole of
            // `pdfcer_render::ColorDiagnostics` — twelve counters that the
            // engine had computed, merged across nested form XObjects and
            // unit-tested since the colour-space slice shipped, and that
            // NO shell had ever read. They stopped at the crate boundary,
            // which is disclosure to nobody (project rule 4).
            //
            // Found by rendering the print-conformance suite test file:
            // it reported a clean render for pages whose gradient patches
            // paint nothing at all, because `patterns_unpainted` was the
            // only counter that could have said so and it went nowhere.
            //
            // All twelve are here, census included, because the struct is
            // merged as a unit and a partial exposure would only create a
            // second judgement call later about which half was worth
            // reporting. Appended at the END, same contract.
            "cs_unresolved",
            "colors_not_set",
            "icc_alternate",
            "icc_device_fallback",
            "tint_applied",
            "tint_not_applied",
            "sep_all_approximated",
            "sep_none_suppressed",
            "pattern_spaces",
            "patterns_unpainted",
            "indexed_clamped",
            "indexed_short",
            // §8.7.4 shadings, appended 2026-08-17 in the SAME change that
            // added the counters — which is the whole point. The colour
            // block directly above spent months computed-and-unread
            // because adding a counter and adding its shell surface were
            // treated as two separate changes. They are one.
            //
            // `shadings` beside `shadings_painted` is the pair that
            // matters while the geometry slice is outstanding: non-zero
            // left, zero right, is pdfcer saying it found the gradients,
            // understood them, and drew none. When the geometry lands the
            // right number moves and nothing else here changes — so this
            // test can watch the feature arrive rather than be told.
            "shadings",
            "shadings_via_sh",
            "shadings_paintable",
            "shadings_painted",
            "shadings_refused",
            "shadings_mesh",
            "mesh_records",
            "mesh_truncated",
            "mesh_unusable",
            "type3_glyphs",
            "type3_glyphs_missing",
            "type3_colors_ignored",
            // §8.6.6.4/.5 and §8.6.5 image colour, appended 2026-08-17.
            // Both exist because the pixel-parity harness reads THIS LINE
            // and nothing else: a `Lab` image with a perfectly good
            // stderr explanation still landed in that harness's
            // *unexplained* bucket, which is the third time in one
            // session that a disclosure reached a human and not a
            // machine.
            "img_colorant_none",
            "img_uncalibrated",
            // Clause 11 transparency (§11.3.5 `/BM`, §11.6.5 `/SMask`).
            // Neither is implemented, and before these keys existed
            // neither was COUNTED: `apply_ext_gstate` read seven keys and
            // silently dropped the rest, so a page composited by the wrong
            // rule reported nothing at all. Disclosing the gap BEFORE
            // implementing it is what made it measurable — the operator's
            // suite X-4 file turned out to carry 113 ignored blend modes
            // and 36 ignored soft masks, with the worst page being one
            // that had looked clean by every other counter.
            "blend_modes_applied",
            "blend_modes_ignored",
            "soft_masks_ignored",
            // §11.6.5 soft masks: what was applied, and the two
            // shortfalls inside that. `soft_mask_tr_ignored` is the
            // one an operator cannot see by looking — `/TR` is where
            // a mask gets inverted, so an ignored one can show the
            // content the document meant to hide.
            "soft_masks_applied",
            "soft_mask_tr_ignored",
            "soft_masks_reset_stale",
            // §11.4.7 transparency groups — the third clause-11 gap, and
            // the one that explained why the suite blend-mode panel still
            // failed after blend modes were implemented and verified.
            "groups_flattened",
            "groups_special",
            // §11.4.5 group compositing, added when groups stopped being
            // flattened. `composited` is the census; `knockout_approx` is
            // the shortfall inside it.
            "groups_composited",
            "groups_knockout_approx",
            // §8.6.7 / §11.7.4.3 overprint. The comment here used to read
            // "tracked and reported, not simulated", which stopped being
            // true when `CompatibleOverprint` shipped — Table 149 is now
            // applied per colour component rather than approximated by a
            // Normal blend.
            "overprint_requested",
            "overprint_opm1",
            // The subset of enabled overprint that is a real difference,
            // i.e. the set composited through Table 149.
            "overprint_effective",
            // What ran, what could not, and how far it moved the page.
            // `overprint_refused` is the shortfall an operator cannot see
            // by looking at the output, which is why it is on the stable
            // line rather than only in the verbose report.
            "overprint_composited",
            "overprint_refused",
            "overprint_pixels",
            // The four NON-SEPARABLE blend modes take their own code path
            // (pdfcer computes Table 137 per pixel; decision 066), so they
            // get their own counters rather than folding into
            // `blend_modes_applied`. APPENDED, never inserted -- a script
            // reading this line positionally must keep working, which is
            // what makes the order a contract rather than a convention.
            "nonseparable_composited",
            "nonseparable_pixels",
            // §11.4.4's second content walk, appended after every
            // pre-existing key because this list IS the stable contract
            // and inserting in the middle would break every consumer
            // parsing it positionally.
            "groups_backdrop_reruns",
            "soft_masks_on_group_result",
            "overprint_images_unsupported",
            "overprint_shadings_unsupported",
            "blend_space_subtractive",
            // Pass 122.5. A FLAG rather than the provenance word it started
            // as: every value on this line parses as an integer, and the
            // assertion below pins that. The word moved to the operator
            // note, where prose belongs.
            "blend_space_from_output_intent",
            "blends_in_wrong_space",
            // `Pass 97.1e`'s colorant buffer, appended for the same reason
            // the previous group was: this list IS the contract, and a
            // consumer parsing positionally must not have keys inserted
            // under it.
            "cmyk_buffer",
            "cmyk_buffer_refused",
            "cmyk_bridged_pixels",
            "cmyk_groups_approximated",
            "cmyk_unbridged_images",
            // `Pass 130.1` -- the complement of `cmyk_bridged_pixels`:
            // pixels a DeviceCMYK image contributed as authored ink,
            // with no conversion in either direction.
            "cmyk_native_image_pixels",
            // Appended, never inserted: the key ORDER is the contract a
            // script parses positionally, so a new counter goes on the end
            // (`Pass 199.0`).
            "rendering_intents_set",
            // `Pass 199.2`, same append-never-insert rule. The PAIR goes on
            // together deliberately: `icc_managed_paints` without its
            // `icc_unmanaged_paints` twin would let a reader take a zero as
            // "colour management agreed with the fallback" when it can equally
            // mean "the branch was never reached". Two keys, one fact.
            "icc_managed_paints",
            "icc_unmanaged_paints",
            // `Pass 204.0`, appended not inserted.
            "overprint_process_images_unsupported",
        ],
        "metrics key order is part of the stable contract"
    );
    for kv in metrics.split(' ') {
        let (_, v) = kv.split_once('=').expect("key=value");
        v.parse::<u64>()
            .unwrap_or_else(|_| panic!("metric value must be a non-negative integer: {kv:?}"));
    }

    // A path drawing with no text at all is a fully faithful render, so
    // every counter is zero and stderr stays silent — the property that
    // makes "stderr had output" a usable batch signal.
    assert!(
        metrics.contains("substituted=0")
            && metrics.contains("unsupported=0")
            && metrics.contains("unknown=0"),
        "clean render should report zeros: {metrics:?}"
    );
    assert_eq!(stderr(&out), "", "a clean render must not write to stderr");
}

#[test]
fn r20_counters_disclose_a_substituted_font_on_stdout_and_stderr() {
    // Decision 004 rule R20: an operator must be able to tell, WITHOUT
    // reading the code, that these letterforms are pdfcer's bundled
    // substitute rather than the document's own. Helvetica is declared
    // with no embedded program, so every glyph is substituted — and the
    // count has to reach stdout (machine) and the font name has to reach
    // stderr (human).
    let dir = TempDir::new("r20");
    let pdf = dir.write(
        "text.pdf",
        &multipage_pdf(&["BT /F1 24 Tf 10 40 Td (Hi) Tj ET"]),
    );
    let png = dir.join("text.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);

    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    let line = stdout(&out);
    assert!(
        !line.contains("substituted=0"),
        "R20: substituted glyphs must be counted on stdout: {line:?}"
    );
    let err = stderr(&out);
    assert!(
        err.contains("Helvetica"),
        "R20: the substituted face must be NAMED on stderr: {err:?}"
    );
}

/// A subtractive page fixture: a `/Group /CS /DeviceCMYK` page whose
/// content blends, which is the only kind of page the colorant buffer —
/// and therefore the ceiling — has anything to do with.
///
/// Built here rather than reused from `multipage_pdf` because the page
/// GROUP is the whole point: without `/Group /CS /DeviceCMYK` the render
/// composites on screen at every ceiling and the test would pass while
/// measuring nothing.
fn subtractive_pdf() -> Vec<u8> {
    let content = "0 0 0 1 k 0 0 200 100 re f /GS0 gs 0 1 0 0 k 0 0 200 100 re f";
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> \
             /Resources << /ExtGState << /GS0 5 0 R >> >> >>"
                .into(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
        (5, "<< /Type /ExtGState /BM /Difference >>".into()),
    ])
}

/// `--max-cmyk-buffer-bytes` moves the boundary in BOTH directions, and
/// says which side of it this render landed on (`Pass 132.0`).
///
/// # Why both directions, on the same file, at the same scale
///
/// Only the pair is evidence. A test that merely lowered the ceiling and
/// saw a refusal would pass against a build that had hard-wired the
/// refusal; a test that merely raised it proves nothing, because the
/// default already composites this page in ink. Changing ONLY the flag and
/// watching `cmyk_buffer` flip is what says the flag is the cause.
///
/// The refusal message is asserted too, and deliberately on the phrase
/// that names the way out. Before this Pass it said only "re-render at a
/// lower resolution", which was the whole truth at the time and is now
/// half of it — and an operator-facing paragraph that has gone stale is
/// invisible, because nothing else tests one.
#[test]
fn the_cmyk_buffer_ceiling_is_settable_and_the_outcome_is_disclosed() {
    let dir = TempDir::new("cmyk-ceiling");
    let pdf = dir.write("ink.pdf", &subtractive_pdf());
    let png = dir.join("ink.png");
    let path = pdf.to_str().unwrap().to_owned();
    let out_path = png.to_str().unwrap().to_owned();

    // 200x100 pt at scale 1 is 20,000 px = 400,000 bytes of colorant
    // buffer. The two ceilings straddle that number.
    let low = run(&[
        "render-page",
        &path,
        "--max-cmyk-buffer-bytes",
        "128kib",
        "-o",
        &out_path,
    ]);
    assert_eq!(
        code(&low),
        0,
        "a refusal is not a failure: {}",
        stderr(&low)
    );
    assert!(
        stdout(&low).contains("cmyk_buffer=0 cmyk_buffer_refused=1"),
        "a ceiling below the raster must refuse and SAY so: {}",
        stdout(&low)
    );
    assert!(
        stderr(&low).contains("RAISE THE CEILING"),
        "the refusal must name the new way out, not only the old one: {}",
        stderr(&low)
    );

    let high = run(&[
        "render-page",
        &path,
        "--max-cmyk-buffer-bytes",
        "1mib",
        "-o",
        &out_path,
    ]);
    assert_eq!(code(&high), 0, "stderr: {}", stderr(&high));
    assert!(
        stdout(&high).contains("cmyk_buffer=1 cmyk_buffer_refused=0"),
        "the same raster must composite in ink once the ceiling allows it: {}",
        stdout(&high)
    );

    // An unreadable value is REPORTED and then ignored. The failure this
    // guards against is a silent `0`, which would read as "pdfcer stopped
    // compositing in ink" rather than as "you typed something wrong".
    let bad = run(&[
        "render-page",
        &path,
        "--max-cmyk-buffer-bytes",
        "plenty",
        "-o",
        &out_path,
    ]);
    assert_eq!(code(&bad), 0, "a bad flag value is a note, not a failure");
    assert!(stderr(&bad).contains("is not a size"), "{}", stderr(&bad));
    assert!(
        stdout(&bad).contains("cmyk_buffer=1"),
        "an unreadable ceiling must fall back to the SETTING, not to zero: {}",
        stdout(&bad)
    );
}

#[test]
fn scale_multiplies_the_raster_size() {
    let dir = TempDir::new("scale");
    let pdf = dir.write("one.pdf", &multipage_pdf(&["0 0 0 rg 0 0 10 10 re f"]));
    let png = dir.join("big.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "--scale",
        "2",
        "-o",
        png.to_str().unwrap(),
    ]);

    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(png_dimensions(&png), (400, 200));
    assert!(stdout(&out).contains(" 400x200; "));
}

#[test]
fn page_flag_selects_the_right_page_and_defaults_to_one() {
    // Page 2 is 90-degree-free but has a distinguishing content stream;
    // what is actually under test is that `--page 2` reaches the second
    // element of the flattened page vector and that omitting the flag
    // reaches the first.
    let dir = TempDir::new("pages");
    let pdf = dir.write(
        "three.pdf",
        &multipage_pdf(&[
            "0 0 0 rg 0 0 10 10 re f",
            "0 0 0 rg 0 0 20 20 re f",
            "0 0 0 rg 0 0 30 30 re f",
        ]),
    );

    for (flag, expected) in [(Some("2"), 2u32), (Some("3"), 3), (None, 1)] {
        let png = dir.join(&format!("p{expected}.png"));
        let mut args = vec!["render-page", pdf.to_str().unwrap()];
        if let Some(f) = flag {
            args.extend(["--page", f]);
        }
        args.extend(["-o", png.to_str().unwrap()]);

        let out = run(&args);
        assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
        assert!(
            stdout(&out).contains(&format!(" page {expected} -> ")),
            "stdout must echo the page actually rendered: {}",
            stdout(&out)
        );
        assert_eq!(png_dimensions(&png), (200, 100));
    }
}

#[test]
fn page_out_of_range_is_a_clear_runtime_failure_at_both_ends() {
    let dir = TempDir::new("range");
    let pdf = dir.write("one.pdf", &multipage_pdf(&["0 0 0 rg 0 0 10 10 re f"]));
    let png = dir.join("nope.png");

    // Past the end, and the 0 case — the module docs commit to both
    // producing exit 1, not clap's usage exit 2.
    for page in ["2", "0"] {
        let out = run(&[
            "render-page",
            pdf.to_str().unwrap(),
            "--page",
            page,
            "-o",
            png.to_str().unwrap(),
        ]);
        assert_eq!(code(&out), 1, "page {page}: stderr: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            err.contains("out of range") && err.contains("1 page(s)"),
            "the message must name the real page count: {err:?}"
        );
        assert_eq!(stdout(&out), "", "a failure must print no result line");
        assert!(!png.exists(), "a failure must not leave an output file");
    }
}

#[test]
fn missing_input_is_exit_3_and_a_non_pdf_is_exit_4() {
    let dir = TempDir::new("errs");
    let png = dir.join("never.png");

    let out = run(&[
        "render-page",
        dir.join("absent.pdf").to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 3, "missing input is an I/O failure");
    assert_eq!(stdout(&out), "");

    let junk = dir.write("junk.bin", b"GIF89a this is not a PDF at all\n");
    let out = run(&[
        "render-page",
        junk.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 4, "a non-PDF is exit 4, not a generic failure");
    assert!(stderr(&out).contains("not a PDF"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn capability_gap_refusal_is_honest_and_distinguishable_from_corruption() {
    // pdfcer-core deliberately REFUSES structures it cannot yet handle
    // correctly, rather than misparsing them. The CLI must pass that
    // refusal through verbatim so the operator learns "pdfcer can't open
    // this *yet*", not "your file is broken" — the same honesty the GUI
    // owes on its error surface.
    //
    // ★ The case pinned here has CHANGED, and the change is the interesting
    // part. This test used to pin *any* encrypted document (§7.6), because
    // pdfcer had no security handler at all and refused the lot. It now has
    // one: RC4 documents open, so "encrypted" is no longer the gap.
    //
    // The gap is now a NAMED CIPHER, which is a strictly better refusal — it
    // tells an operator that the machinery works and one algorithm is
    // missing, rather than implying pdfcer cannot read protected files. This
    // test moves to that case rather than being deleted, because the property
    // it guards is unchanged: a refusal must read as a pdfcer limitation, not
    // as a broken file.
    //
    // ★ AND IT HAS MOVED AGAIN, for exactly the same reason: increment 2
    // implemented AES-128, so `enc-aes-128.pdf` now RENDERS, and pinning it
    // here would assert a refusal that no longer happens. The slot moved to
    // AES-256 at `/R` 5.
    //
    // ★ AND AGAIN. Increment 3 implemented `/R` 5, so the slot moves to
    // `/R` 6 — and the nature of the refusal changes with it, which is the
    // part worth reading. Every earlier occupant of this slot was "pdfcer has
    // not written this yet". `/R` 6 is not that: its Algorithm 2.B is not in
    // the project's spec corpus past step (a), so writing it would mean
    // deriving a normative algorithm from another implementation and then
    // testing against that implementation — a test that could not fail.
    //
    // That distinction is what this test now pins. The message must still
    // read as a pdfcer-side limitation rather than as a broken file, and it
    // must still name the cipher, but it must ALSO say the algorithm is
    // unavailable rather than merely unwritten, because those have different
    // next actions: one needs engineering time, the other needs a document
    // nobody has found.
    //
    // (Cross-reference streams used to be pinned here before encryption was;
    // they now load — see `a_pdf_15_file_with_xref_and_object_streams_renders`
    // below. This slot has become a rolling record of what pdfcer cannot do
    // yet, which is exactly what it should be.)
    let dir = TempDir::new("encrypted");
    let enc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/encryption");
    let pdf = enc.join("enc-aes-256-r6.pdf");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        dir.join("never.png").to_str().unwrap(),
    ]);

    assert_eq!(code(&out), 1, "an unsupported structure is a runtime error");
    let err = stderr(&out);
    assert!(
        err.contains("AES-256"),
        "the refusal must name WHICH cipher is involved — 'encrypted files are \
         unsupported' is now false, and a vague message would teach the \
         operator something untrue: {err:?}"
    );
    assert!(
        err.contains("/R 6") && err.contains("not available"),
        "the refusal must say the ALGORITHM is unavailable, not that pdfcer has \
         not got round to it — those resolve differently: {err:?}"
    );
    assert_eq!(stdout(&out), "");

    // ★ And the contrast, in the same test: the /R 5 file, same cipher, same
    // dictionary shape, differing only in the hash function, RENDERS. Without
    // this line the refusal above would be consistent with pdfcer having no
    // AES-256 support at all, which is what the assertion is trying not to
    // say.
    let ok = run(&[
        "--open-password",
        "userpw",
        "render-page",
        enc.join("enc-aes-256-r5.pdf").to_str().unwrap(),
        "-o",
        dir.join("r5.png").to_str().unwrap(),
    ]);
    assert_eq!(
        code(&ok),
        0,
        "/R 5 must render, or the /R 6 refusal above proves nothing: {}",
        stderr(&ok)
    );
}

/// ★ Decryption produces the RIGHT plaintext, not merely parseable bytes.
///
/// Every other test in this increment proves a document *loads*. None of them
/// proves it loads **correctly** — and a wrong-but-self-consistent decryption
/// is entirely possible: RC4 with a wrong key yields bytes that a lenient
/// parser can still walk, and the failure would show up as subtly wrong
/// content rather than as an error.
///
/// So: render the plaintext source and all three RC4 encryptions of it to
/// PNG, and require the four files to be **byte-identical**. That exercises
/// both halves of decryption at once — content streams (decrypted in the
/// retained buffer) all the way to pixels, and the resources they reference
/// (reached through dictionaries whose strings are decrypted in the parsed
/// objects).
///
/// The seven encrypted files span `/R` 2 at 40 bits, `/R` 3 at 128, the
/// empty-password case that needs no password at all, both of those again
/// under **AES-128**, and **AES-256 at `/R` 5 with each of its two passwords**
/// — seven key/cipher combinations, one image.
///
/// ★ The AES rows carry a second job the RC4 rows cannot. RC4 preserves
/// length, so increment 1 could write plaintext back over ciphertext in the
/// retained buffer and every `ByteSpan` stayed true. AES output is IV +
/// padding, so the plaintext is **strictly shorter** and `data_span.len` has
/// to shrink to match. That was verified by breaking it deliberately: with the
/// old `plain.len() == span.len` guard reinstated, every AES stream silently
/// stays ciphertext, the CLI **still exits 0**, and it still writes a
/// plausible PNG — just a different one. Nothing else in the suite goes red.
/// This byte-comparison is the only thing standing between that bug and a
/// release.
///
/// ★ The two `/R` 5 rows carry a third job, and it is the reason both
/// passwords appear here rather than only one. At `/R` 5 the file encryption
/// key is **wrapped twice** — once under a key derived from the user password
/// (`/UE`), once under a key derived from the owner password plus the whole
/// 48-byte `/U` (`/OE`, **T26**) — and both unwraps must yield the *same* 32
/// bytes. Rendering both to the identical PNG is what proves they do. An owner
/// path that authenticated correctly (Algorithm 3.12) and then unwrapped with
/// the wrong salt would still open the document and would still exit 0; only
/// the pixels differ.
#[test]
fn decrypting_reproduces_the_plaintext_document_exactly() {
    let dir = TempDir::new("decrypt-fidelity");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic");

    let render = |args: &[&str], out: &Path| {
        let mut argv: Vec<&str> = args.to_vec();
        argv.push("-o");
        let out_s = out.to_str().unwrap();
        argv.push(out_s);
        let r = run(&argv);
        assert_eq!(code(&r), 0, "render failed: {}", stderr(&r));
        std::fs::read(out).expect("the PNG must exist")
    };

    let plain_path = fixtures.join("forms/demo-form.pdf");
    let plain = render(
        &["render-page", plain_path.to_str().unwrap()],
        &dir.join("plain.png"),
    );

    let enc = fixtures.join("encryption");
    let cases: [(&str, Vec<String>); 7] = [
        (
            "rc4-40 via the owner password",
            vec![
                "--open-password".into(),
                "ownerpw".into(),
                "render-page".into(),
                enc.join("enc-rc4-40.pdf").to_string_lossy().into_owned(),
            ],
        ),
        (
            "rc4-128 via the user password",
            vec![
                "--open-password".into(),
                "userpw".into(),
                "render-page".into(),
                enc.join("enc-rc4-128.pdf").to_string_lossy().into_owned(),
            ],
        ),
        (
            "rc4-128 with an empty user password, no flag at all",
            vec![
                "render-page".into(),
                enc.join("enc-emptyuser-rc4-128.pdf")
                    .to_string_lossy()
                    .into_owned(),
            ],
        ),
        (
            "aes-128 via the user password",
            vec![
                "--open-password".into(),
                "userpw".into(),
                "render-page".into(),
                enc.join("enc-aes-128.pdf").to_string_lossy().into_owned(),
            ],
        ),
        (
            "aes-128 with an empty user password, no flag at all",
            vec![
                "render-page".into(),
                enc.join("enc-emptyuser.pdf").to_string_lossy().into_owned(),
            ],
        ),
        (
            "aes-256 /R 5 via the user password (the /UE unwrap)",
            vec![
                "--open-password".into(),
                "userpw".into(),
                "render-page".into(),
                enc.join("enc-aes-256-r5.pdf")
                    .to_string_lossy()
                    .into_owned(),
            ],
        ),
        (
            "aes-256 /R 5 via the owner password (the /OE unwrap, T26)",
            vec![
                "--open-password".into(),
                "ownerpw".into(),
                "render-page".into(),
                enc.join("enc-aes-256-r5.pdf")
                    .to_string_lossy()
                    .into_owned(),
            ],
        ),
    ];

    for (label, argv) in cases {
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let got = render(&borrowed, &dir.join("enc.png"));
        assert_eq!(
            got, plain,
            "{label}: the decrypted document must render byte-identically to the plaintext it was made from. A difference here means the key derivation produced something the parser could still walk, which is the failure mode no load test can see."
        );
    }
}

#[test]
fn password_reaches_every_load_path_and_bad_input_fails_by_name() {
    // ★ The affordance half of the capability. `pdfcer-core` can decrypt RC4
    // documents; a core capability no shell can reach is not a feature yet.
    //
    // This test exists because the sweep that wired `--open-password` through
    // the CLI's load sites MISSED ONE — `inspect` passed
    // `Document::from_bytes` to `.and_then()` as a function REFERENCE, not a
    // call, so a search for call sites walked straight past it. The symptom
    // was `--password` being accepted, reported as successful, and silently
    // ignored by the one subcommand most likely to be tried first.
    let dir = TempDir::new("cli-password");
    let rc4 = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption/enc-rc4-128.pdf");
    let rc4_40 = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption/enc-rc4-40.pdf");

    // Without a password: refused, and refused as a PASSWORD problem.
    let out = run(&["inspect", rc4.to_str().unwrap()]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("password-protected"),
        "the refusal must name the actual problem: {:?}",
        stderr(&out)
    );

    // With the user password: opens.
    let out = run(&[
        "--open-password",
        "userpw",
        "inspect",
        rc4.to_str().unwrap(),
    ]);
    assert_eq!(
        code(&out),
        0,
        "--open-password must reach `inspect`'s load path: {}",
        stderr(&out)
    );

    // With the OWNER password: also opens (§7.6.3.1 — either password).
    let out = run(&[
        "--open-password",
        "ownerpw",
        "inspect",
        rc4_40.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // From a file, with the trailing newline every editor adds. Stripping it
    // matters more than it looks: a newline silently included in the password
    // fails in a way indistinguishable from a wrong password.
    let pw_file = dir.write("pw.txt", b"userpw\n");
    let out = run(&[
        "--open-password-file",
        pw_file.to_str().unwrap(),
        "inspect",
        rc4.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // An --open-password-file that cannot be read fails immediately and by name,
    // rather than proceeding password-less and surfacing later as "this
    // document is password-protected" — which would send the operator
    // hunting for the wrong problem entirely.
    let out = run(&[
        "--open-password-file",
        dir.join("does-not-exist.txt").to_str().unwrap(),
        "inspect",
        rc4.to_str().unwrap(),
    ]);
    assert_eq!(
        code(&out),
        3,
        "an unreadable password file is a usage error"
    );
    assert!(
        stderr(&out).contains("password file"),
        "the error must name the password file, not the PDF: {:?}",
        stderr(&out)
    );
}

#[test]
fn an_rc4_encrypted_document_renders_once_its_password_is_supplied() {
    // The complement of the refusal above, and the thing that makes it
    // meaningful: what pdfcer refuses by cipher name, it opens where the
    // cipher is implemented. Without this, the test above would also pass
    // against a build that had quietly stopped opening encrypted files at all.
    let dir = TempDir::new("rc4-render");
    let pdf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/encryption/enc-emptyuser-rc4-128.pdf");
    let png = dir.join("page.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);

    assert_eq!(
        code(&out),
        0,
        "a permissions-only RC4 document must render with no password: {}",
        stderr(&out)
    );
    assert!(png.exists(), "the page must actually be written");
}

#[test]
fn a_pdf_15_file_with_xref_and_object_streams_renders() {
    // The complement of the test above: what used to be refused now
    // goes all the way through load -> page tree -> render. The catalog,
    // page tree and page object all live inside an object stream
    // (§7.5.7) and are reached by type-2 cross-reference entries
    // (§7.5.8.3), so this exercises the whole PDF 1.5 structural path
    // end to end through the shipped binary.
    let dir = TempDir::new("pdf15");
    let pdf = dir.write("pdf15.pdf", &build_pdf15_objstm_pdf());
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(png.is_file(), "a PNG must have been written");
}

/// Build a PDF 1.5 file whose cross-reference section is a stream and
/// whose document structure lives inside an object stream.
///
/// Layout: objects 1 (catalog), 2 (page tree) and 3 (the page) are
/// compressed into object stream 4; object 5 is the page's content
/// stream (a *stream*, so §7.5.7 forbids compressing it); object 6 is
/// the cross-reference stream, which `startxref` points at directly
/// (§7.5.8.1).
fn build_pdf15_objstm_pdf() -> Vec<u8> {
    let compressed: [(u32, &str); 3] = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 5 0 R >>",
        ),
    ];
    // §7.5.7 decoded layout: `N` `objnum offset` pairs (offsets
    // relative to `/First`), then the bare object values at `/First` —
    // no `obj`/`endobj` framing.
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in compressed {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let objstm_data = format!("{header}{body}");

    let content = "0 0 1 rg 20 20 160 60 re f\n";

    let mut buf = b"%PDF-1.5\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();

    offsets.push((4, buf.len()));
    buf.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /ObjStm /N {} /First {first} /Length {} >>\nstream\n{objstm_data}\nendstream\nendobj\n",
            compressed.len(),
            objstm_data.len(),
        )
        .as_bytes(),
    );

    offsets.push((5, buf.len()));
    buf.extend_from_slice(
        format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
            content.len(),
        )
        .as_bytes(),
    );

    // The cross-reference stream itself (object 6). `/W [1 4 2]`:
    // 1-byte type, 4-byte field 2, 2-byte field 3, all big-endian
    // (§7.5.8.3).
    let xref_at = buf.len();
    offsets.push((6, xref_at));
    let mut rows: Vec<u8> = Vec::new();
    let push = |rows: &mut Vec<u8>, ty: u8, f2: u32, f3: u16| {
        rows.push(ty);
        rows.extend(f2.to_be_bytes());
        rows.extend(f3.to_be_bytes());
    };
    // Object 0 is permanently the free-list head (§7.5.4).
    push(&mut rows, 0, 0, 65535);
    for num in 1..=6u32 {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => push(&mut rows, 1, u32::try_from(*off).unwrap(), 0),
            // Objects 1-3 are compressed in container 4, at indices
            // 0/1/2 respectively — type-2 entries (Table 18).
            None => push(&mut rows, 2, 4, u16::try_from(num - 1).unwrap()),
        }
    }
    buf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /XRef /Size 7 /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            rows.len(),
        )
        .as_bytes(),
    );
    buf.extend_from_slice(&rows);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    buf
}

// ---------------------------------------------------------------------------
// §8.6 colour disclosure — the ColorDiagnostics channel reaching a shell
// ---------------------------------------------------------------------------

/// A page whose only visible mark is a **shading-pattern fill** — the
/// shape a gradient takes in every real-world file — and the assertion
/// that pdfcer both leaves it blank AND says so.
///
/// # Why this fixture is built by hand rather than via `multipage_pdf`
///
/// It needs a `/Pattern` entry in the page `/Resources` and a
/// `PatternType 2` object carrying a `ShadingType 2` dictionary. That is
/// more structure than the shared builder's fixed resource dict provides,
/// and inlining it keeps the exact thing under test readable at the call
/// site (this file's module header, "Why the fixtures are built inline").
///
/// The content stream is the canonical four operators: select the pattern
/// colour space (`/Pattern cs`), name the pattern (`/P0 scn`), build a
/// rectangle covering most of the page, fill it.
fn shading_pattern_pdf() -> Vec<u8> {
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /Pattern << /P0 5 0 R >> >> >>"
                .into(),
        ),
        (4, {
            let content = "/Pattern cs /P0 scn 10 10 180 80 re f";
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
        }),
        // PatternType 2 = shading pattern (§8.7.4.1, Table 76), wrapping a
        // ShadingType 2 axial shading (§8.7.4.5.3, Table 79) that runs a
        // type-2 exponential function from red to blue across the page.
        //
        // This comment used to end "the point of the test is that a
        // CONFORMANT gradient paints nothing today", and the test asserted
        // exactly that. It is now painted, so both the sentence and the
        // assertions below were falsified by the Pass that implemented the
        // feature — the R180 shape, caught here by the suite rather than by
        // a reader, because the counter it pinned moved.
        (
            5,
            "<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceRGB \
             /Coords [10 0 190 0] /Extend [true true] /Function \
             << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >>"
                .into(),
        ),
    ])
}

#[test]
fn a_shading_pattern_gradient_is_painted_and_counted_on_stdout() {
    // WAS `a_gradient_that_paints_nothing_is_disclosed_on_stdout_and_stderr`.
    //
    // It asserted `patterns_unpainted=1` and a stderr sentence containing
    // "NOTHING was painted" — both correct when written, both false the
    // moment `PatternType 2` fills shipped. Kept as a test of the SAME
    // fixture with inverted expectations rather than deleted, because the
    // fixture is the interesting part (a fully conformant axial gradient
    // reached through a pattern) and because the pair of counters is
    // exactly what a regression would move back.
    let dir = TempDir::new("pattern");
    let pdf = dir.write("gradient.pdf", &shading_pattern_pdf());
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let line = stdout(&out);

    // `pattern_spaces` still counts the `cs` selection — selecting the
    // space and painting the pattern are different events and the first
    // one did not stop happening.
    assert!(
        line.contains(" pattern_spaces=1 "),
        "the /Pattern cs selection must be counted: {line:?}"
    );
    // `patterns_unpainted` now counts only a SHORTFALL. A painted shading
    // pattern is not one.
    assert!(
        line.contains(" patterns_unpainted=0 "),
        "a painted shading pattern is not an unpainted pattern: {line:?}"
    );
    // The shading counters are how a shell can tell WHICH route painted.
    // `shadings_via_sh=0` is the load-bearing half: it says this gradient
    // arrived as a pattern, not through the `sh` operator, and the two
    // anchor to different coordinate spaces (§8.7.2 NOTE 1 vs Table 77).
    assert!(
        line.contains(" shadings=1 "),
        "the shading must be counted: {line:?}"
    );
    assert!(
        line.contains(" shadings_via_sh=0 "),
        "it arrived via a pattern, not via `sh`: {line:?}"
    );
    assert!(
        line.contains(" shadings_painted=1 "),
        "the gradient must be painted: {line:?}"
    );

    // And the fact the counters are ABOUT: pixels really changed. Without
    // this the test would pass on a stale counter over a blank page, which
    // is the failure mode a counter-only assertion cannot see — the same
    // reasoning the original test used, pointed the other way.
    let png_bytes = std::fs::read(&png).unwrap();
    assert!(!png_bytes.is_empty(), "a PNG is written");
    assert_eq!(png_dimensions(&png), (200, 100));
}

#[test]
fn a_spot_colour_with_a_usable_tint_transform_is_reported_as_the_documents_own() {
    // The positive twin. `tint_applied` exists so a shell can say the spot
    // colours on a page ARE the document's own rather than pdfcer's
    // stand-in, and until this test there was no shell that could.
    //
    // It also guards a specific stale sentence: `device_n_to_rgb`'s
    // fallback note used to read "NOT evaluated (no 7.10 function
    // evaluator yet)" long after the evaluator had shipped, so an operator
    // chasing a grey spot colour was told to wait for a feature that
    // already existed. A file with a WORKING transform must reach neither
    // the counter nor any sentence about a missing evaluator.
    let dir = TempDir::new("spot");
    let pdf = dir.write(
        "spot.pdf",
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
                 << /ColorSpace << /CS0 [/Separation /PANTONE#20185#20C /DeviceCMYK \
                 << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.9 0.7 0] /N 1 >>] \
                 >> >> >>"
                    .into(),
            ),
            (4, {
                let content = "/CS0 cs 1 scn 10 10 180 80 re f";
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                )
            }),
        ]),
    );
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let line = stdout(&out);
    // TWO, not one, and the difference is a fact about the standard rather
    // than an off-by-one. Table 74 makes `cs` "set the current colour to
    // its initial value for that colour space" — for a `Separation` that
    // is a tint of 1.0 — so selecting the space evaluates the transform
    // once before `scn` evaluates it again. The first draft of this test
    // asserted 1 and was wrong; the counter was right.
    //
    // Asserted exactly rather than as `>= 1` on purpose: this counter's
    // job is to let a shell say how much of a page's colour is the
    // document's own, and a test that tolerates any positive number
    // cannot see it drift.
    assert!(
        line.contains(" tint_applied=2 "),
        "a usable /tintTransform must be counted as APPLIED, once for the `cs` \
initial colour and once for `scn`: {line:?}"
    );
    assert!(
        line.contains(" tint_not_applied=0 "),
        "a usable /tintTransform must not be counted as a shortfall: {line:?}"
    );

    let err = stderr(&out);
    assert!(
        !err.contains("function evaluator"),
        "no sentence may claim the §7.10 evaluator is missing — it shipped: {err:?}"
    );
    assert!(
        !err.contains("hue is not the document's"),
        "a working transform must not be reported as a hue substitution: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// §8.7.4 shadings — the gradient inventory reaching a shell
// ---------------------------------------------------------------------------

/// A page carrying three shadings reached by the `sh` operator: one axial
/// (`ShadingType 2`), one radial (`ShadingType 3`) and one tensor-product
/// mesh (`ShadingType 7`).
///
/// # Why all three types in one fixture
///
/// The counter that matters is not "how many shadings" but **the split**:
/// an operator asking "will an update fix my file?" is asking whether the
/// shadings on it are analytic or mesh, and those are answered by
/// different amounts of engineering. A fixture with only axial shadings
/// would let `shadings_mesh` be hard-wired to zero and still pass.
///
/// The mesh is a **stream**, not a dictionary — types 4-7 carry their
/// geometry in stream data — which also exercises the branch in
/// `Shading::load` that reads Table 78's entries out of a stream's
/// dictionary rather than a bare dictionary.
fn three_shadings_pdf() -> Vec<u8> {
    // The ShadingType 7 patch data is two bytes, which cannot hold one
    // complete patch record (the smallest legal one is 24 coordinate fields
    // plus four colours plus a flag). ★★ THAT USED TO BE INCIDENTAL AND IS
    // NOW THE POINT. Until `Pass 125.0` no mesh was decoded at all, so this
    // fixture asserted "classified as a mesh and declined by name" and any
    // bytes would have done. Meshes are decoded now, so the same two bytes
    // exercise a different branch: 8.7.4.5.7's "at least one complete patch
    // shall be specified", reported as `mesh_unusable`.
    //
    // Left at two bytes rather than grown into a real patch, deliberately.
    // Real mesh geometry is tested in
    // `crates/pdfcer-render/tests/mesh_shadings.rs` against twelve purpose-
    // built fixtures; what THIS file tests is the census LINE, and a census
    // needs one shading of each kind, not one correct picture.
    let mesh_body = "AB";
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
             << /Shading << /Ax 5 0 R /Rad 6 0 R /Mesh 7 0 R >> >> >>"
                .into(),
        ),
        (4, {
            // `q`/`Q` around each, the way a real producer writes it: `sh`
            // paints in CURRENT user space, so its anchoring is whatever
            // the CTM is at that moment.
            let content = "q /Ax sh Q q /Rad sh Q q /Mesh sh Q";
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
        }),
        // ShadingType 2, axial — ISO 32000-1 Table 80 (NOT 79; the family
        // is 78 common / 79 type-1 / 80 type-2 / 81 type-3, off by one
        // from the intuitive guess, and wrong in this crate's own doc
        // comments until the spec corpus was consulted).
        (
            5,
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
             /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] \
             /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"
                .into(),
        ),
        // ShadingType 3, radial — Table 81. BOTH radii non-zero, which is
        // exactly the case `tiny_skia::RadialGradient` cannot express (it
        // has no start radius) and the reason pdfcer will evaluate shadings
        // itself rather than delegate.
        (
            6,
            "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [100 50 10 100 50 40] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] /C1 [0 1 1] /N 1 >> >>"
                .into(),
        ),
        (
            7,
            format!(
                "<< /ShadingType 7 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 \
                 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 200 0 100 0 1 0 1 0 1] \
                 /Length {} >>\nstream\n{mesh_body}\nendstream",
                mesh_body.len()
            ),
        ),
    ])
}

#[test]
fn shadings_are_inventoried_by_type_and_reported_as_unpainted() {
    let dir = TempDir::new("shading");
    let pdf = dir.write("grad.pdf", &three_shadings_pdf());
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let line = stdout(&out);
    assert!(
        line.contains(" shadings=3 "),
        "all three shadings must be counted: {line:?}"
    );
    assert!(
        line.contains(" shadings_via_sh=3 "),
        "all three arrived by the `sh` operator: {line:?}"
    );
    // The split that answers "will an update fix my file?".
    assert!(
        line.contains(" shadings_paintable=2 "),
        "the axial and radial are paintable; the mesh is not, because its stream is too short for one patch: {line:?}"
    );
    // The counters that say WHY it is not paintable. Without these the
    // assertion above passes equally well against a build that stopped
    // decoding meshes entirely, which is exactly the regression this line
    // exists to notice.
    assert!(
        line.contains(" mesh_unusable=1 "),
        "the two-byte patch stream must be reported as unusable, by name: {line:?}"
    );
    assert!(
        line.contains(" mesh_records=0 "),
        "no geometry can come out of a two-byte patch stream: {line:?}"
    );
    // No trailing space: `shadings_mesh` is currently the LAST key on the
    // line, so it is followed by the newline. Asserted with a leading
    // space only, which stays correct when a future slice appends a key
    // after it — the append-never-reorder contract guarantees the leading
    // space, never the trailing one.
    assert!(
        line.contains(" shadings_mesh=1"),
        "the type 7 must be counted as a mesh: {line:?}"
    );
    assert!(
        line.contains(" shadings_refused=0 "),
        "three well-formed shadings must not be refused: {line:?}"
    );

    // ★ This assertion was `shadings_painted=0` for exactly one commit,
    // and it DID its job: when the axial and radial painters landed, this
    // is the line that went red, which is how the feature announced its
    // own arrival instead of being asserted into existence. It is updated
    // here rather than deleted, and the history is kept in this comment,
    // because the same trick works again for the mesh types.
    //
    // Two of three painted: the axial and the radial. The type 7 mesh is
    // not, and `painted` must NOT be allowed to drift up to 3 when a mesh
    // is merely resolved.
    assert!(
        line.contains(" shadings_painted=2 "),
        "the axial and radial are painted; the mesh is not: {line:?}"
    );

    let err = stderr(&out);
    // The prose half has to state the CONSEQUENCE. "3 shadings found" on
    // its own reads like a success line; what an operator needs to know is
    // that the page in front of them is missing them.
    assert!(
        err.contains("type2=1") && err.contains("type3=1") && err.contains("type7=1"),
        "stderr must break the inventory down by ShadingType: {err:?}"
    );
    // The headline reports BOTH numbers and lets the reader subtract,
    // rather than asserting a capability. Its first version read "pdfcer
    // resolves gradients but does not yet draw them" — true for one
    // commit. A sentence that encodes the roadmap's current state goes
    // stale silently; two counters cannot.
    assert!(
        err.contains("2 painted, 1 NOT"),
        "stderr must state painted AND unpainted, not a capability claim: {err:?}"
    );
    assert!(
        err.contains("MESH shadings"),
        "the mesh types deserve their own sentence — they are a different wait: {err:?}"
    );
}

#[test]
fn a_malformed_shading_is_refused_by_name_and_not_counted_as_paintable() {
    // The distinction this guards: `refused` means the DOCUMENT is broken
    // and no amount of pdfcer engineering will draw it, while `paintable=0`
    // with `refused=0` means pdfcer simply has not got there yet. Collapsing
    // the two would tell an operator with a corrupt file to wait for an
    // update that cannot help them.
    let dir = TempDir::new("shadbad");
    let pdf = dir.write(
        "bad.pdf",
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
            ),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources \
                 << /Shading << /Short 5 0 R /NoCs 6 0 R >> >> >>"
                    .into(),
            ),
            (4, {
                let content = "/Short sh /NoCs sh";
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                )
            }),
            // An axial shading with three /Coords instead of four. NOT
            // repaired: a missing fourth number is a geometry pdfcer cannot
            // know, and inventing it would paint a plausible wrong
            // gradient — the failure mode that is worst to debug.
            (
                5,
                "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> >>"
                    .into(),
            ),
            // No /ColorSpace at all, which Table 78 requires.
            (
                6,
                "<< /ShadingType 2 /Coords [0 0 200 0] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> >>"
                    .into(),
            ),
        ]),
    );
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "a malformed shading is not a fatal error");

    let line = stdout(&out);
    assert!(
        line.contains(" shadings=2 "),
        "both were REACHED, even though neither loaded — otherwise a page \
whose every shading is broken reports the same zero as a page with no \
gradients at all: {line:?}"
    );
    assert!(
        line.contains(" shadings_refused=2 "),
        "both must be refused by name: {line:?}"
    );
    assert!(
        line.contains(" shadings_paintable=0 "),
        "a refused shading is never paintable: {line:?}"
    );

    let err = stderr(&out);
    assert!(
        err.contains("REFUSED"),
        "stderr must name the refusal: {err:?}"
    );
    assert!(
        err.contains("the file is malformed"),
        "stderr must distinguish a broken file from a pending feature: {err:?}"
    );
}

#[test]
fn no_annotations_says_how_many_it_withheld() {
    // Rule 4, "fuzzy never sneaky", applied to a WITHHOLDING rather than
    // an inference. `--no-annotations` is the operator asking pdfcer not to
    // paint something the file contains; the answer is allowed, the
    // silence is not.
    //
    // Until `Pass 74.8` the count existed -- `Diagnostics::
    // annotations_out_of_scope`, computed, merged across pages and covered
    // by unit tests in `pdfcer-render` -- and `pdfcer` printed it
    // NOWHERE, neither on the line nor on stderr. So the operator saw
    // `annots=1 annots_painted=0` and had nothing to tell "withheld on
    // request" apart from "tried and failed". Those need different
    // reactions, which is the whole reason the counters are split.
    //
    // A counter that exists and is not surfaced is worse than one that
    // does not exist: it makes the gap look measured.
    let dir = TempDir::new("no-annots-discloses");
    let pdf = dir.write(
        "a.pdf",
        &build_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Annots [5 0 R] /Resources << >> >>"
                    .into(),
            ),
            (4, "<< /Length 0 >>
stream

endstream".into()),
            (
                5,
                "<< /Type /Annot /Subtype /Square /Rect [10 10 60 60] /AP << /N 6 0 R >> >>"
                    .into(),
            ),
            (
                6,
                "<< /Type /XObject /Subtype /Form /BBox [0 0 50 50] /Resources << >> /Length 23 >>
stream
0 0 1 rg 0 0 50 50 re f
endstream"
                    .into(),
            ),
        ]),
    );
    let png = dir.join("a.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    let with = stdout(&out);
    assert!(
        with.contains(" annots=1 ") && with.contains(" annots_out_of_scope=0 "),
        "nothing is out of scope when no scope was narrowed: {with:?}"
    );

    let without = stdout(&run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "--no-annotations",
        "-o",
        png.to_str().unwrap(),
    ]));
    assert!(
        without.contains(" annots=1 "),
        "the annotation is still COUNTED -- a withheld annotation that vanishes from the census is indistinguishable from a file that never had one: {without:?}"
    );
    assert!(
        without.contains(" annots_painted=0 "),
        "and it was not painted: {without:?}"
    );
    assert!(
        without.contains(" annots_out_of_scope=1 "),
        "THE POINT OF THIS TEST: the shortfall is attributed. Without this key, `annots=1 annots_painted=0` reads exactly like a failure: {without:?}"
    );
}

// ---------------------------------------------------------------------------
// `--probe-ink` — Pass 174.0
// ---------------------------------------------------------------------------

/// A page whose group declares `/DeviceCMYK`, carrying one opaque `k` fill.
///
/// Built inline rather than read from `fixtures/synthetic/ink-probe/` on
/// purpose: this file's whole convention is that the structure under test is
/// visible at the call site (see the module header), and the structure under
/// test here is *the page group*, which is one dictionary entry. The
/// committed fixtures exist for the `pdfcer-render` unit tests, where the
/// question is about the compositor rather than about the CLI's report.
fn cmyk_group_pdf(content: &str) -> Vec<u8> {
    build_pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> /Resources << >> >>"
                .into(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
    ])
}

/// ★ THE ANSWER THE SIBLING `iccce` PROJECT ASKED FOR, THROUGH THE BINARY.
///
/// A single opaque `0.75 0 1 0 k` fill on a page composited in ink reaches
/// the exit conversion with its operand intact. The composite is an identity
/// here — transparent backdrop, alpha 1, Normal blend — so anything still
/// wrong about the colour is downstream of this point, in the conversion.
///
/// Asserted through the CLI rather than only in `pdfcer-render` because the
/// unit tests exercise the library and this is the surface the other project
/// can actually run. `Pass 162.0`'s lesson: the untested path is the shipped
/// one.
#[test]
fn probe_ink_reports_the_colorants_in_the_buffer_before_the_conversion() {
    let dir = TempDir::new("probe-ink");
    let pdf = dir.write("ink.pdf", &cmyk_group_pdf("0.75 0 1 0 k 20 20 160 60 re f"));
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "--probe-ink",
        "100,50",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let all = stdout(&out);
    let lines: Vec<&str> = all.trim_end().split('\n').collect();
    assert_eq!(
        lines.len(),
        2,
        "the probe adds ONE line and does not disturb the metrics line: {all:?}"
    );
    assert!(
        lines[0].starts_with("rendered "),
        "the stable metrics line stays FIRST, so a script that reads one line \
         off this command keeps working: {all:?}"
    );
    assert_eq!(
        lines[1],
        "ink-probe: x=100 y=50 source=cmyk-buffer c=0.750 m=0.000 y=1.000 k=0.000 \
         alpha=1.000 srgb=47,181,73",
        "the operand written by the content stream must arrive at the exit \
         conversion unchanged; a difference here is a defect in the COMPOSITOR, \
         not in the conversion"
    );
}

/// The control: the same paint with no page group. There is no colorant
/// buffer, so there are no colorants — reported as `-`, not reconstructed.
///
/// Without this, an implementation that echoed the content stream's operands
/// instead of reading the buffer would pass the test above.
#[test]
fn probe_ink_on_a_page_composited_on_screen_reports_no_colorants() {
    let dir = TempDir::new("probe-screen");
    let pdf = dir.write(
        "flat.pdf",
        &multipage_pdf(&["0.75 0 1 0 k 20 20 160 60 re f"]),
    );
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "--probe-ink",
        "100,50",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));

    let all = stdout(&out);
    let probe = all.trim_end().split('\n').nth(1).expect("a probe line");
    assert!(
        probe.contains("source=screen-srgb"),
        "no page group means no ink: {probe:?}"
    );
    assert!(
        probe.contains("c=- m=- y=- k=- alpha=-"),
        "EVERY key is present with `-` for absent, so \"this page was never \
         composited in ink\" cannot be read as \"this pixel has no ink\": {probe:?}"
    );
    assert!(
        probe.contains("srgb="),
        "the raster exists either way, so its colour is always reportable: {probe:?}"
    );
}

/// A coordinate outside the raster is a report, not a refusal — and the page
/// still renders.
///
/// The raster's size is a function of `--scale`, `--region` and the page's own
/// box, none of which are resolved when the flag is parsed. Refusing here
/// would let a diagnostic destroy the output it was asked about.
#[test]
fn probe_ink_outside_the_raster_still_renders_the_page() {
    let dir = TempDir::new("probe-oob");
    let pdf = dir.write("flat.pdf", &multipage_pdf(&["0 0 0 rg 10 10 50 50 re f"]));
    let png = dir.join("out.png");

    let out = run(&[
        "render-page",
        pdf.to_str().unwrap(),
        "--probe-ink",
        "99999,7",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(png_dimensions(&png), (200, 100), "the page still rendered");
    let all = stdout(&out);
    assert!(
        all.contains("source=out-of-range"),
        "the probe says it could not answer, rather than answering wrongly: {all:?}"
    );
}

/// A malformed coordinate IS refused, because it is decidable from the string
/// alone and nothing about the document could make it valid.
///
/// ★ `-4,2` is deliberately NOT in this list, and its absence is the finding.
/// A value beginning with `-` is eaten by `clap` as a flag name before this
/// parser sees it, so it exits `2` (usage) with `clap`'s message rather than
/// `1` with ours. `--region` carries `allow_hyphen_values` to defeat exactly
/// that, because a `/MediaBox` may legitimately have a negative origin. A
/// DEVICE pixel may not — there is no raster whose top-left is left of
/// itself — so the flag is left without it and `clap`'s refusal stands. The
/// `4,-2` form below is the one that reaches this parser, and it is refused
/// here.
#[test]
fn probe_ink_rejects_a_coordinate_it_can_judge_without_the_document() {
    let dir = TempDir::new("probe-bad");
    let pdf = dir.write("flat.pdf", &multipage_pdf(&["0 0 0 rg 10 10 50 50 re f"]));
    let png = dir.join("out.png");

    for spec in ["1", "1,2,3", "4,-2", "a,b", "1.5,2"] {
        let out = run(&[
            "render-page",
            pdf.to_str().unwrap(),
            "--probe-ink",
            spec,
            "-o",
            png.to_str().unwrap(),
        ]);
        assert_eq!(
            code(&out),
            1,
            "{spec:?} should be refused: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("--probe-ink"),
            "the message must name the flag: {}",
            stderr(&out)
        );
    }
}
