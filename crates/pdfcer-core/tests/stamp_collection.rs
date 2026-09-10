//! `Pass 288.0` — Acrobat-compatible stamp collection files.
//!
//! # The operator's ask
//!
//! > *"if acrobat has a way of adding custom stamps or text, we need the same
//! > feature too with the same import/export to make the stamps as Adobe has
//! > and is compatible with adobe's."*
//!
//! # The format, MEASURED in Adobe's own files rather than sourced
//!
//! A stamp collection is an ordinary PDF: one file per **category**, one page
//! per **stamp**. The category is the file's `/Info` `/Title`; each stamp's
//! names live in the catalog's `/Names` → `/Pages` name tree as one string
//! `internal=display`; a `#` prefix marks a dynamic stamp.
//!
//! The feature-parity research reached that shape from convergent community
//! sources and **flagged two gaps by name** — where the category name is
//! stored, and whether `#` was real. Both are closed here by reading Adobe's
//! shipped files directly:
//!
//! ```text
//! StandardBusiness.pdf   /Info /Title (Standard Business)
//!                        /Names [ (SBApproved=Approved) 244 0 R … ]
//! Dynamic.pdf            /Info /Title (Dynamic)
//!                        /Names [ (#DApproved=Approved) 29 0 R … ]
//! ```
//!
//! ★ **A compatibility feature built on secondary sourcing is how a shipped
//! feature silently fails to interoperate.** These tests therefore assert the
//! *structure Adobe writes*, not merely that pdfcer can re-read itself — a
//! reader that only ever sees its own writer's output proves nothing.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, InfoField};
use pdfcer_core::stamp_file;
use pdfcer_core::writer::SaveOptions;

/// A three-page document to name as three stamps.
fn three_pages() -> Vec<u8> {
    let mut bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_string(),
        ),
    ];
    for n in 3..=5u32 {
        bodies.push((
            n,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_string(),
        ));
    }

    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let high = bodies.len() as u32 + 1;
    buf.extend_from_slice(format!("xref\n0 {high}\n").as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for n in 1..high {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map(|(_, o)| *o)
            .expect("offset");
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {high} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Author a collection and return the saved bytes.
fn authored_collection() -> Vec<u8> {
    let doc = Document::from_bytes(three_pages()).expect("the fixture loads");
    let mut session = EditSession::new(doc);

    let stamps = vec![
        ("KMApproved".to_owned(), "Approved".to_owned()),
        ("KMForReview".to_owned(), "For Review".to_owned()),
        ("KMSuperseded".to_owned(), "Superseded".to_owned()),
    ];
    let written = stamp_file::name_stamp_pages(&mut session, &stamps).expect("names written");
    assert_eq!(written.stamps_named, 3);
    assert!(written.skipped.is_empty());

    session
        .set_info_field(InfoField::Title, Some("Stanley Engineering"))
        .expect("the category name is the /Info /Title");

    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("the collection saves");
    bytes
}

// ------------------------------------------------- 1. reading Adobe's files

/// ★★★ pdfcer READS ADOBE'S OWN STAMP FILES.
///
/// Skipped when Acrobat is not installed, because a test that silently passes
/// on a machine without the files would be worse than no test — but on a
/// machine that has them it is the only assertion here that proves
/// compatibility rather than self-consistency.
#[test]
fn adobes_own_stamp_files_are_read_correctly() {
    let path = std::path::Path::new(
        "C:/Program Files/Adobe/Acrobat DC/Acrobat/plug_ins/Annotations/Stamps/ENU/StandardBusiness.pdf",
    );
    if !path.exists() {
        eprintln!("SKIP: Acrobat's stamp files are not installed on this machine");
        return;
    }

    let doc = Document::load(path).expect("Adobe's stamp file opens");
    let c = stamp_file::read(&doc);

    assert_eq!(c.category.as_deref(), Some("Standard Business"));
    assert_eq!(c.stamps.len(), 12, "twelve stamps in Standard Business");
    let approved = c
        .stamps
        .iter()
        .find(|s| s.internal == "SBApproved")
        .expect("SBApproved is present");
    assert_eq!(approved.display, "Approved");
    assert_eq!(
        approved.page_index,
        Some(0),
        "and it names a real page of the file"
    );
    assert!(!approved.dynamic, "a Standard Business stamp is static");
}

/// ★★ The `#` prefix marks a dynamic stamp — Adobe's own convention, in
/// Adobe's own file.
#[test]
fn adobes_dynamic_stamps_are_recognised_as_dynamic() {
    let path = std::path::Path::new(
        "C:/Program Files/Adobe/Acrobat DC/Acrobat/plug_ins/Annotations/Stamps/ENU/Dynamic.pdf",
    );
    if !path.exists() {
        eprintln!("SKIP: Acrobat's stamp files are not installed on this machine");
        return;
    }

    let doc = Document::load(path).expect("opens");
    let c = stamp_file::read(&doc);

    assert_eq!(c.category.as_deref(), Some("Dynamic"));
    assert!(
        c.stamps.iter().all(|s| s.dynamic),
        "every stamp in the Dynamic category is dynamic: {:?}",
        c.stamps.iter().map(|s| &s.internal).collect::<Vec<_>>()
    );
}

/// ★ THE CONTROL: an ordinary PDF is not a stamp file.
///
/// Without it, a reader that returned a stamp for every document would satisfy
/// both assertions above and make `is_stamp_file` worthless.
#[test]
fn an_ordinary_pdf_is_not_a_stamp_collection() {
    let doc = Document::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/hello.pdf"),
    )
    .expect("load hello.pdf");
    let c = stamp_file::read(&doc);
    assert!(!c.is_stamp_file());
    assert!(c.stamps.is_empty());
}

// ------------------------------------------------- 2. writing the format

/// ★★★ A COLLECTION PDFCER WROTE HAS THE STRUCTURE ADOBE WRITES.
///
/// Asserted against the raw bytes, not by re-reading with pdfcer's own reader:
/// the point is the file's shape, and a writer and reader that agree with each
/// other while both being wrong is exactly the failure this Pass must avoid.
#[test]
fn an_authored_collection_has_adobes_structure() {
    let bytes = authored_collection();
    let text = String::from_utf8_lossy(&bytes);

    assert!(
        text.contains("/Names"),
        "the catalog must carry a /Names dictionary"
    );
    assert!(
        text.contains("(KMApproved=Approved)"),
        "each stamp is one `internal=display` name string"
    );
    assert!(
        text.contains("(Stanley Engineering)"),
        "the category is the /Info /Title"
    );
}

/// And pdfcer reads back what it wrote, through a save and reopen.
#[test]
fn an_authored_collection_round_trips() {
    let doc = Document::from_bytes(authored_collection()).expect("reopens");
    let c = stamp_file::read(&doc);

    assert_eq!(c.category.as_deref(), Some("Stanley Engineering"));
    assert_eq!(c.stamps.len(), 3);
    assert!(c.is_stamp_file());

    let names: Vec<&str> = c.stamps.iter().map(|s| s.internal.as_str()).collect();
    assert_eq!(names, ["KMApproved", "KMForReview", "KMSuperseded"]);
    assert!(
        c.stamps.iter().all(|s| s.page_index.is_some()),
        "every stamp names a real page: {:?}",
        c.stamps
    );
    assert!(
        c.stamps.iter().all(|s| !s.dynamic),
        "pdfcer authors static stamps only"
    );
}

/// ★★ THE NAME TREE IS SORTED BY NAME, which §7.9.6 requires.
///
/// Adobe's own file proves page order is NOT tree order — `SBApproved` names
/// page 0 and `SBCompleted` names page 4 — so a writer that emitted page order
/// would produce a tree a conforming reader may binary-search wrongly. The
/// fixture's names are deliberately given in an order that is NOT alphabetical
/// by page, so emitting page order would fail this.
#[test]
fn the_name_tree_is_written_in_lexicographic_order() {
    let doc = Document::from_bytes(three_pages()).expect("loads");
    let mut session = EditSession::new(doc);
    // Page 0 gets the LAST name alphabetically, page 2 the first.
    let stamps = vec![
        ("Zulu".to_owned(), "Zulu".to_owned()),
        ("Mike".to_owned(), "Mike".to_owned()),
        ("Alpha".to_owned(), "Alpha".to_owned()),
    ];
    stamp_file::name_stamp_pages(&mut session, &stamps).expect("written");
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("saves");

    let doc = Document::from_bytes(bytes).expect("reopens");
    let c = stamp_file::read(&doc);
    let order: Vec<&str> = c.stamps.iter().map(|s| s.internal.as_str()).collect();
    assert_eq!(
        order,
        ["Alpha", "Mike", "Zulu"],
        "the tree must be sorted by NAME, not by page"
    );
    // And the sort did not scramble which page each name points at.
    let alpha = &c.stamps[0];
    assert_eq!(
        alpha.page_index,
        Some(2),
        "Alpha still names the page it was given"
    );
}

/// ★ A stamp naming a page that does not exist is SKIPPED and REPORTED, not
/// written.
///
/// A name tree pointing at nothing is a stamp that appears in a picker and
/// then draws no page — the silent failure this disclosure exists to prevent.
#[test]
fn naming_more_stamps_than_pages_reports_the_overflow() {
    let doc = Document::from_bytes(three_pages()).expect("loads");
    let mut session = EditSession::new(doc);
    let stamps = vec![
        ("One".to_owned(), "One".to_owned()),
        ("Two".to_owned(), "Two".to_owned()),
        ("Three".to_owned(), "Three".to_owned()),
        ("Four".to_owned(), "Four".to_owned()),
    ];
    let written = stamp_file::name_stamp_pages(&mut session, &stamps).expect("written");

    assert_eq!(written.stamps_named, 3, "only three pages exist");
    assert_eq!(
        written.skipped,
        vec!["Four=Four".to_owned()],
        "and the fourth is NAMED in the report, not silently dropped"
    );
}

/// ★★ Writing the name tree PRESERVES a `/Names` dictionary that already
/// exists.
///
/// Acrobat's `Dynamic.pdf` carries `/Names << /JavaScript … /Pages … >>`.
/// Replacing the dictionary wholesale would silently delete the
/// document-level JavaScript that makes its dynamic stamps work — so this
/// asserts the sibling key survives.
#[test]
fn an_existing_names_dictionary_keeps_its_other_trees() {
    // The fixture's catalog already carries a `/Names` with a sibling tree —
    // built into the bytes rather than edited in, so the test exercises the
    // preservation path and nothing else.
    let bytes = String::from_utf8(three_pages())
        .expect("the fixture is ASCII")
        .replace(
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript << >> >> >>",
        )
        .into_bytes();
    let doc = Document::from_bytes(bytes).expect("loads");
    let mut session = EditSession::new(doc);

    stamp_file::name_stamp_pages(&mut session, &[("A".to_owned(), "A".to_owned())])
        .expect("written");

    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("saves");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/JavaScript"),
        "the sibling name tree must survive"
    );
    assert!(text.contains("/Pages"), "and /Pages must have been added");
}

// ------------------------------- 4. an unreadable page tree is not a stamp fact

/// ★★ A PAGE-TREE failure must not be reported as "every stamp points at
/// nothing" (`Pass 290.1`).
///
/// `StampEntry::page_index` is `None` for a defined reason — *the named page
/// is not one of the document's own* — and `read` used to write that meaning
/// onto a completely different event by `unwrap_or_default()`-ing the page
/// walk. On the operator's own Acrobat-written signature file both of his
/// stamps printed `page=MISSING` when neither was missing; what had actually
/// happened was a refusal three modules away.
///
/// The fixture damages the page tree by making the `/Pages` node its own kid
/// — a cycle, which is structural damage with no second reading — while
/// leaving the `/Names` tree perfect. So the names MUST still be read, and
/// the reason the indices are absent must be carried in the one place a
/// caller can tell the two cases apart.
#[test]
fn an_unwalkable_page_tree_is_not_reported_as_missing_stamps() {
    let bytes = {
        // Two objects only: a catalog whose /Names /Pages tree names two
        // stamps, and a /Pages node that lists itself in /Kids.
        let bodies: Vec<(u32, String)> = vec![
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /Names << /Pages << /Names \
                 [(KMApproved=Approved) 3 0 R (#KMDynamic=Dynamic) 4 0 R] >> >> >>"
                    .to_string(),
            ),
            (2, "<< /Type /Pages /Kids [2 0 R] /Count 1 >>".to_string()),
        ];
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in &bodies {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
        for (_, off) in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        buf
    };

    let doc =
        Document::from_bytes(bytes).expect("the file itself loads — only the page tree is damaged");
    let collection = stamp_file::read(&doc);

    // The names survive: nothing is wrong with the name tree.
    assert_eq!(collection.stamps.len(), 2, "the name tree still parses");
    assert_eq!(collection.stamps[0].display, "Approved");
    assert!(collection.stamps[1].dynamic, "the # prefix still reads");

    // And the reason the indices are absent is carried, not guessed at.
    assert!(
        collection.page_tree_error.is_some(),
        "a page-tree failure must be reported, not laundered into page_index: None"
    );
    assert!(
        collection.stamps.iter().all(|s| s.page_index.is_none()),
        "no index is knowable when the page list could not be built"
    );
    let why = collection.page_tree_error.as_deref().unwrap_or_default();
    assert!(
        why.contains("cycle"),
        "the message must name the real cause, got {why:?}"
    );
}

/// The twin: a well-formed page tree leaves `page_tree_error` `None`, so
/// `Some(_)` keeps meaning something. Without this a build that set the field
/// unconditionally would pass the test above and make every collection look
/// damaged.
#[test]
fn a_readable_page_tree_reports_no_error() {
    let doc = Document::from_bytes(authored_collection()).expect("loads");
    let collection = stamp_file::read(&doc);
    assert_eq!(collection.stamps.len(), 3);
    assert!(collection.page_tree_error.is_none());
    assert!(
        collection.stamps.iter().all(|s| s.page_index.is_some()),
        "every stamp resolves to a page in a well-formed collection"
    );
}
