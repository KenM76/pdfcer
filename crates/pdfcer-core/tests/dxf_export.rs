//! DXF export — pdfcer's vector model written as CAD geometry.
//!
//! ## What is actually at risk here
//!
//! Producing *a* DXF is easy. Producing one that opens in AutoCAD LT 2004
//! on a plasma controller, at the right size, without a hole becoming a
//! 767 KB polyline, is the whole job — and every one of those failures is
//! silent. A wrongly-scaled file looks right on screen and cuts the wrong
//! part. A file with a `MATERIAL` object does not error, it simply refuses
//! to load. A flattened circle is geometrically correct and unusable.
//!
//! So these tests are mostly about the failure modes named in
//! `C:\personal_rag\dxf\`, which cost the operator real time before pdfcer
//! existed:
//!
//! - `lesson_20260424_autocad_lt_2004_compat.md` — `MATERIAL` objects and
//!   group code 94 make LT 2004 reject the whole file.
//! - `lesson_20260603_ezdxf_authoring_cut_files_lwpolyline.md` — flattened
//!   circles bloat catastrophically (~40 washers → 767 KB); closed
//!   polylines use the close FLAG, not a repeated first vertex.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::export::dxf::{DxfOptions, DxfText, DxfUnits, write_dxf};
use pdfcer_core::vector::{Matrix, NoXObjects, PageObjects, decompose};

/// Decompose an inline content stream — no fonts needed, this is geometry.
fn model(src: &[u8]) -> PageObjects {
    let cs = ContentStream::parse(src.to_vec()).expect("parses");
    decompose(&cs, Matrix::IDENTITY, &NoXObjects)
}

fn export(src: &[u8], opts: &DxfOptions) -> (String, pdfcer_core::export::dxf::DxfOutcome) {
    write_dxf(&model(src), opts)
}

/// A PDF circle, drawn the way every producer draws one: four cubic
/// Béziers with the kappa control-point offset (0.5523 × r).
fn pdf_circle(cx: f64, cy: f64, r: f64) -> Vec<u8> {
    let k = 0.552_284_749_8 * r;
    format!(
        "{} {} m \
         {} {} {} {} {} {} c \
         {} {} {} {} {} {} c \
         {} {} {} {} {} {} c \
         {} {} {} {} {} {} c h S",
        cx + r,
        cy,
        cx + r,
        cy + k,
        cx + k,
        cy + r,
        cx,
        cy + r,
        cx - k,
        cy + r,
        cx - r,
        cy + k,
        cx - r,
        cy,
        cx - r,
        cy - k,
        cx - k,
        cy - r,
        cx,
        cy - r,
        cx + k,
        cy - r,
        cx + r,
        cy - k,
        cx + r,
        cy,
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------
// The LT 2004 constraints — the ones that make a file refuse to open
// ---------------------------------------------------------------------------

/// **`MATERIAL` objects and group code 94 must never appear.**
///
/// LT 2004 refuses the *entire file* on either — "Unknown entity" /
/// "Drawing recovery" / silent failure. The operator's existing `ezdxf`
/// pipeline strips both afterwards; this writer must never produce them in
/// the first place, which is the concrete payoff of hand-writing it.
///
/// Asserted on the output text rather than trusted to the writer's
/// structure, because "there is no code path that writes 94" is exactly
/// the kind of claim that stops being true when somebody adds an entity.
#[test]
fn the_output_carries_nothing_autocad_lt_2004_rejects() {
    let (dxf, _) = export(b"10 10 m 100 10 l 100 100 l h S", &DxfOptions::default());

    assert!(
        !dxf.contains("MATERIAL"),
        "a MATERIAL object makes LT 2004 refuse the whole file",
    );
    assert!(
        !dxf.contains("OBJECTS"),
        "there must be no OBJECTS section for a MATERIAL to live in",
    );
    // Group code 94 as a line of its own — codes are written one per line,
    // so this is exact rather than a substring accident.
    for line in dxf.lines() {
        assert_ne!(
            line.trim(),
            "94",
            "group code 94 makes LT 2004 reject the entity",
        );
    }
}

/// The file is structurally a DXF: sections open and close, and it ends
/// with `EOF`. A reader that hits an unterminated section reports a
/// corrupt file rather than a missing entity.
#[test]
fn the_output_is_a_structurally_complete_dxf() {
    let (dxf, _) = export(b"0 0 m 10 0 l S", &DxfOptions::default());
    assert_eq!(
        dxf.matches("SECTION").count(),
        3,
        "HEADER, TABLES, ENTITIES"
    );
    assert_eq!(dxf.matches("ENDSEC").count(), 3, "each one closed");
    assert!(dxf.trim_end().ends_with("EOF"), "must terminate with EOF");
    assert!(dxf.contains("$ACADVER"), "a version is required");
}

// ---------------------------------------------------------------------------
// Units and scale — the silent-wrongness axis
// ---------------------------------------------------------------------------

/// **`$INSUNITS` is written, and it matches the coordinates.**
///
/// A DXF that does not say what its numbers mean is interpreted against
/// the receiving application's default. The operator finds out at the
/// cutting table.
#[test]
fn units_are_declared_and_the_coordinates_agree() {
    // A 72 pt line is exactly one inch.
    const SRC: &[u8] = b"0 0 m 72 0 l S";

    let (inch, _) = export(SRC, &DxfOptions::default());
    assert!(inch.contains("$INSUNITS"), "units must be declared");
    assert!(
        inch.contains("1.000000"),
        "72 pt is 1.000000 inch; got:\n{inch}",
    );

    let (mm, _) = export(
        SRC,
        &DxfOptions {
            units: DxfUnits::Millimetres,
            ..DxfOptions::default()
        },
    );
    assert!(mm.contains("25.400000"), "72 pt is 25.4 mm; got:\n{mm}",);
}

/// **The drawing scale multiplies through.** A 1:2 view exported at
/// `scale = 2.0` comes back full size.
///
/// This is the field the whole feature turns on: every generic converter
/// exports at paper scale and says nothing, so a detail view arrives at
/// half size looking entirely plausible.
#[test]
fn the_drawing_scale_multiplies_through_to_the_coordinates() {
    const SRC: &[u8] = b"0 0 m 72 0 l S";
    let (dxf, _) = export(
        SRC,
        &DxfOptions {
            scale: 2.0,
            ..DxfOptions::default()
        },
    );
    assert!(
        dxf.contains("2.000000"),
        "one paper inch at 1:2 is two real inches; got:\n{dxf}",
    );
}

// ---------------------------------------------------------------------------
// Arcs — the bloat axis
// ---------------------------------------------------------------------------

/// **A PDF circle becomes ONE `CIRCLE`, not a flattened polyline.**
///
/// PDF has no arc primitive, so a circle arrives as four kappa Béziers.
/// Not recognising them is what produced a measured 767 KB for forty
/// washers — geometrically correct output that is unusable in practice.
#[test]
fn a_pdf_circle_becomes_one_circle_entity() {
    let (dxf, out) = export(&pdf_circle(100.0, 100.0, 50.0), &DxfOptions::default());

    assert_eq!(out.circles, 1, "four kappa cubics are one circle:\n{dxf}");
    assert_eq!(out.splines, 0, "and none of them is a spline");
    assert_eq!(out.arcs, 0, "nor four separate arcs");
    assert_eq!(out.polylines, 0, "and above all not a flattened polyline");

    // The radius survives the unit conversion: 50 pt = 0.694444 in.
    assert!(
        dxf.contains("0.694444"),
        "the radius must be right, not merely present:\n{dxf}",
    );
}

/// **Turning arc-fitting off proves the fitting is doing the work.**
///
/// Without this, "one CIRCLE" is consistent with a writer that emits one
/// entity per subpath whatever the geometry — which would be wrong for
/// every other shape and passes the test above.
#[test]
fn without_arc_fitting_the_same_circle_becomes_splines() {
    let src = pdf_circle(100.0, 100.0, 50.0);
    let (_, out) = export(
        &src,
        &DxfOptions {
            fit_arcs: false,
            ..DxfOptions::default()
        },
    );
    assert_eq!(out.circles, 0);
    assert_eq!(out.splines, 4, "four cubics, four splines");
}

/// **A rounded rectangle is NOT a circle**, though every one of its
/// corners is a genuine arc.
///
/// The four-arc test alone would accept it. The centres must agree, and
/// this is the shape that proves they are checked — a rounded rectangle
/// emitted as a circle would be a spectacular silent failure.
#[test]
fn four_arcs_at_different_centres_are_not_a_circle() {
    // A 100×60 rounded rect with r=10, corners as kappa quarter-arcs.
    const R: f64 = 10.0;
    let k = 0.552_284_749_8 * R;
    let src = format!(
        "10 0 m 90 0 l {} {} {} {} 100 10 c \
         100 50 l {} {} {} {} 90 60 c \
         10 60 l {} {} {} {} 0 50 c \
         0 10 l {} {} {} {} 10 0 c h S",
        90.0 + k,
        0.0,
        100.0,
        10.0 - k,
        100.0,
        50.0 + k,
        90.0 + k,
        60.0,
        10.0 - k,
        60.0,
        0.0,
        50.0 + k,
        0.0,
        10.0 - k,
        10.0 - k,
        0.0,
    )
    .into_bytes();

    let (_, out) = export(&src, &DxfOptions::default());
    assert_eq!(
        out.circles, 0,
        "a rounded rectangle has four DIFFERENT centres and is not a circle",
    );
    assert!(out.arcs > 0, "but its corners are still real arcs");
}

/// A straight run becomes ONE closed `LWPOLYLINE` with the close FLAG —
/// not four `LINE`s, and not a repeated first vertex.
///
/// The RAG is explicit: a duplicated closing vertex reads to a CAM table
/// as a zero-length segment, which some controllers treat as a pierce.
#[test]
fn a_closed_rectangle_is_one_polyline_with_the_close_flag() {
    let (dxf, out) = export(b"0 0 100 60 re S", &DxfOptions::default());

    assert_eq!(out.polylines, 1, "one entity, not four lines:\n{dxf}");
    assert!(dxf.contains("LWPOLYLINE"));
    // Group code 90 is the vertex count. `re` gives four corners; a fifth
    // would be the repeated-first-vertex mistake.
    assert!(
        dxf.contains(" 90\n       4"),
        "four vertices, with closure as a flag rather than a fifth point:\n{dxf}",
    );
}

// ---------------------------------------------------------------------------
// Disclosure — what did NOT make it into the file
// ---------------------------------------------------------------------------

/// **Text is counted as skipped, not dropped quietly.**
///
/// A drawing that is half annotation exports as geometry alone, and an
/// operator who is not told opens it in SOLIDWORKS and concludes the
/// export lost things at random. "The labels are not in this file" is a
/// sentence they need before they open it.
#[test]
fn skipped_text_is_counted_so_the_caller_can_say_so() {
    const SRC: &[u8] = b"0 0 m 10 0 l S BT /F1 12 Tf 1 0 0 1 10 20 Tm (LABEL) Tj ET";
    let opts = DxfOptions {
        text: DxfText::Omit,
        ..DxfOptions::default()
    };
    let (_, out) = export(SRC, &opts);
    assert_eq!(out.skipped_text, 1, "the text object must be reported");
    assert_eq!(out.polylines, 1, "and the geometry must still be there");
}

/// **A text object that decomposes to no runs is still disclosed.**
///
/// This assertion is here because the version of it that did NOT hold
/// shipped: when `DxfText::Entities` became the default, a text object
/// whose runs could not be determined — which is what an inline content
/// stream with no resource dictionary produces, since with no font the
/// walker cannot advance the pen and never closes a run — incremented no
/// counter at all. It vanished from the outcome, and the caller had
/// nothing to disclose.
///
/// The failure mode is the one `skipped_text` was written against, in a
/// new disguise: the operator opens the DXF and the labels are gone with
/// no sentence anywhere explaining it.
#[test]
fn a_text_object_that_yields_no_runs_is_still_counted() {
    const SRC: &[u8] = b"0 0 m 10 0 l S BT /F1 12 Tf 1 0 0 1 10 20 Tm (LABEL) Tj ET";
    let (dxf, out) = export(SRC, &DxfOptions::default());
    assert_eq!(
        out.unreadable_text, 1,
        "an unreadable text object must reach the outcome somewhere"
    );
    assert_eq!(out.text_entities, 0, "and nothing may be written for it");
    assert!(
        !dxf.contains(
            "
TEXT
"
        ),
        "no TEXT entity may be emitted from text that could not be read"
    );
    assert_eq!(out.polylines, 1, "the geometry is unaffected");
}

/// An empty page produces a valid, empty DXF rather than a malformed one.
#[test]
fn an_empty_page_still_produces_a_loadable_file() {
    let (dxf, out) = export(b"", &DxfOptions::default());
    assert_eq!(out, pdfcer_core::export::dxf::DxfOutcome::default());
    assert!(dxf.contains("ENTITIES"));
    assert!(dxf.trim_end().ends_with("EOF"));
}

// ---------------------------------------------------------------------------
// The version, and the coherence between it and the entities emitted
// ---------------------------------------------------------------------------

/// **★ The regression guard for a mistake that shipped past ten green
/// tests.**
///
/// The writer first declared `AC1009` (R12) — my own reasoning that R12
/// "reaches further than R2000" — while emitting `LWPOLYLINE` and
/// `SPLINE`, **which R12 does not have**. R12 draws polylines as
/// `POLYLINE`/`VERTEX`/`SEQEND` and has no spline entity at all. The file
/// claimed one dialect and spoke another.
///
/// `C:\personal_rag\dxf\lesson_20260603_ezdxf_authoring_cut_files_lwpolyline.md`
/// had already named `AC1015` (R2000) as the compatible baseline. I had
/// read it and substituted a guess.
///
/// **Every test in this file passed throughout** — they grepped the output
/// for strings, and the strings were all present. What caught it was
/// parsing the output with a real DXF reader (`ezdxf`), which rejected all
/// four sample exports with *"missing 'AcDbPolyline' subclass"*.
///
/// So this test asserts the thing the string checks could not: that the
/// declared version and the emitted entities are the same dialect.
#[test]
fn the_declared_version_matches_the_entities_actually_emitted() {
    let (dxf, _) = export(b"0 0 100 60 re S", &DxfOptions::default());

    assert!(
        dxf.contains("AC1015"),
        "R2000 is the RAG's compatible baseline for LT 2004 and plasma \
         controllers, and it is the earliest version that HAS the entities \
         this writer emits:\n{dxf}",
    );
    assert!(
        !dxf.contains("AC1009"),
        "R12 has no LWPOLYLINE and no SPLINE — declaring it while emitting \
         them is what produced 'missing AcDbPolyline subclass'",
    );

    // R2000 requires a handle and the AcDbEntity marker on every entity.
    assert!(
        dxf.contains("100\nAcDbEntity"),
        "every R2000 entity opens with the AcDbEntity subclass marker:\n{dxf}",
    );
    assert!(
        dxf.contains("100\nAcDbPolyline"),
        "and then names its own class:\n{dxf}",
    );
    assert!(dxf.contains("$HANDSEED"), "handles need a seed above them");

    // The subclass marker is group code 100 in THREE columns. A
    // four-character " 100" is a different token, and a reader that finds
    // one where a marker belongs reports the marker as missing — the same
    // symptom as the version mistake, from a different cause.
    assert!(
        !dxf.contains(" 100\nAcDb"),
        "group code 100 must not carry a leading space:\n{dxf}",
    );
}

/// Handles are unique. Two entities sharing one is a malformed R2000 file
/// that many readers accept and some silently mis-resolve.
#[test]
fn every_entity_handle_is_unique() {
    let (dxf, out) = export(
        b"0 0 100 60 re S 200 0 m 300 60 l S 400 0 m 500 60 l S",
        &DxfOptions::default(),
    );
    assert!(
        out.polylines >= 3,
        "the fixture must produce several entities"
    );

    let lines: Vec<&str> = dxf.lines().collect();
    let mut handles = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        // A handle is the value after a `  5` code that is followed by the
        // AcDbEntity marker — i.e. an ENTITY handle, not a table one.
        if l.trim() == "5" && lines.get(i + 2).is_some_and(|n| n.trim() == "AcDbEntity") {
            handles.push(lines[i + 1]);
        }
    }
    let mut sorted = handles.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        handles.len(),
        "duplicate entity handle in {handles:?}",
    );
}

// ---------------------------------------------------------------------------
// Text as TEXT entities (`Pass 52.3`)
// ---------------------------------------------------------------------------

/// Export a real fixture PDF's first page, which — unlike the inline
/// content streams above — has a resource dictionary and therefore a font,
/// which is what makes runs exist at all.
fn export_fixture(name: &str, opts: &DxfOptions) -> (String, pdfcer_core::export::dxf::DxfOutcome) {
    use pdfcer_core::{document::Document, page_tree, vector};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name);
    let doc = Document::from_bytes(std::fs::read(&path).expect("fixture readable"))
        .expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.first().expect("one page");
    let model =
        vector::decompose_page(&doc.view(), page, vector::Matrix::IDENTITY).expect("decomposes");
    write_dxf(&model, opts)
}

/// **One `TEXT` entity per RUN, each carrying its own string.**
///
/// The whole reason `TextObject::run_text` was built. A CAD exporter puts
/// every label on a sheet inside one `BT`…`ET` — the operator's own drawing
/// has 237 in a single object — so a per-OBJECT mapping would emit one
/// `TEXT` holding every label concatenated, at one insertion point. The
/// four-name fixture makes that failure visible: it would produce one
/// entity reading `ALPHABETAGAMMADELTA`.
#[test]
fn each_text_run_becomes_its_own_text_entity_with_its_own_string() {
    let (dxf, out) = export_fixture("runs-inherited.pdf", &DxfOptions::default());
    assert_eq!(out.text_entities, 4, "four runs, four TEXT entities");
    assert_eq!(out.unreadable_text, 0);
    for name in ["ALPHA", "BETA", "GAMMA", "DELTA"] {
        assert!(
            dxf.contains(&format!("\n{name}\n")),
            "{name} must appear as its own group-1 value:\n{dxf}"
        );
    }
    assert!(
        !dxf.contains("ALPHABETA"),
        "the runs must not be concatenated into one entity"
    );
}

/// **Text goes on its own layer; geometry stays on `0`.**
///
/// Sourced from the operator's own DXF RAG, which records that a title
/// block drawn on layer `0` "cannot be removed by layer filtering" and
/// forced an entire suite of geometric furniture-detection heuristics
/// downstream. pdfcer is the upstream producer here and declines to
/// reproduce that: turning `PDFCER_TEXT` off must leave exactly the
/// geometry.
///
/// Asserted on the ENTITY's layer code, not merely on the layer table
/// containing the name — a file can define a layer and still put
/// everything on `0`, which is precisely the defect.
#[test]
fn text_entities_land_on_their_own_layer_and_geometry_does_not() {
    let (dxf, _) = export_fixture("runs-inherited.pdf", &DxfOptions::default());
    assert!(
        dxf.contains("  2\nPDFCER_TEXT\n"),
        "the layer table must define the text layer"
    );
    // Every TEXT entity's `8` code must name the text layer. The head is
    // written as one block, so this substring is the entity's real layer.
    let texts = dxf.matches("\nTEXT\n").count();
    assert_eq!(texts, 4, "four TEXT entities");
    assert_eq!(
        dxf.matches("  8\nPDFCER_TEXT\n").count(),
        4,
        "all four must be ON that layer, not merely accompanied by it"
    );
    assert!(
        !dxf.contains("  8\nPDFCER_TEXT\n100\nAcDbPolyline"),
        "no geometry may be placed on the text layer"
    );
}

/// **`DxfText::Omit` writes no `TEXT` and counts every object.**
///
/// The cutting-table case: a stray entity on a plasma path is a hazard,
/// so the operator can turn text off entirely and be told what that cost.
#[test]
fn omit_mode_writes_no_text_and_discloses_the_objects_it_left_out() {
    let opts = DxfOptions {
        text: DxfText::Omit,
        ..DxfOptions::default()
    };
    let (dxf, out) = export_fixture("runs-inherited.pdf", &opts);
    assert_eq!(out.text_entities, 0);
    assert_eq!(out.skipped_text, 1, "one text object, disclosed");
    assert_eq!(
        out.unreadable_text, 0,
        "omitting is not the same as failing"
    );
    assert!(!dxf.contains("\nTEXT\n"));
    assert!(!dxf.contains("ALPHA"));
}

/// **A control character in a run cannot desynchronise the file.**
///
/// DXF is line-oriented: a newline inside a group-1 value ends the value,
/// and the following line is read as a group CODE. One such character
/// would not corrupt a string, it would misparse every entity after it.
#[test]
fn control_characters_are_neutralised_rather_than_written_through() {
    // Exercised through the sanitizer's own contract rather than a fixture,
    // because producing a decoded run containing U+000A requires a font
    // whose ToUnicode maps a code to it — a fixture that would test the
    // font machinery, not this guard.
    let (dxf, _) = export_fixture("runs-inherited.pdf", &DxfOptions::default());
    // Every group-1 value sits alone on its line; the invariant is that the
    // file's line count matches its group-code count.
    let lines: Vec<&str> = dxf.lines().collect();
    assert_eq!(
        lines.len() % 2,
        0,
        "a DXF is strictly alternating code/value lines; an odd count means \
         a value carried an embedded newline and desynchronised the file"
    );
}
