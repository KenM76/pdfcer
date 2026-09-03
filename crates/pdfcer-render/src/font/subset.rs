//! Turn an operator-supplied donor face into a [`FontEmbedPlan`] (FF-C, 21.0).
//!
//! # Where this sits
//!
//! `pdfcer-core` defines [`FontEmbedPlan`] and emits PDF objects from it; this
//! module fills one in. The split is deliberate (decision 021 §3.2):
//! producing a subset requires *parsing* the donor — coverage, advances,
//! descriptor metrics, embedding-permission bits — and that parser already
//! lives in this crate. Putting a subsetter in `pdfcer-core` would give a
//! crate with no font parser two of them, purely to avoid a plain-data seam.
//!
//! # The donor is untrusted even though the operator chose it
//!
//! "The operator picked this file" is not a trust argument. Font files are a
//! long-standing exploit vector, and this is a new parser surface eating
//! bytes pdfcer did not produce. Everything here is bounded and fallible:
//! a size ceiling before parsing, checked lookups throughout, and per-cause
//! errors so a refusal tells the operator which thing went wrong (R27).
//!
//! # What is deliberately NOT guarded here
//!
//! Composite-glyph cycles. A glyph that references itself, or two that
//! reference each other, are the obvious unbounded-recursion risk in any
//! `glyf` walk. `subsetter`'s `closure()` is an **iterative worklist** that
//! only enqueues a component when `glyph_remapper.get(component).is_none()`,
//! so the remapped set grows monotonically and is bounded by `numGlyphs` —
//! it terminates structurally, upstream. Adding a depth cap here would be a
//! guard placed after a filter its guarded case cannot pass: R96's dead code
//! that looks live. What is owed instead is a *fixture* proving the property
//! holds, which can fail if upstream ever rewrites that walk recursively.

use pdfcer_core::font_embed::{DescriptorMetrics, FontEmbedPlan, OutlineKind, SubsetGlyph};
use skrifa::raw::TableProvider as _;
use skrifa::raw::types::GlyphId16;
use skrifa::{FontRef, MetadataProvider};
use subsetter::GlyphRemapper;

/// Largest donor font file pdfcer will parse, in bytes.
///
/// # Why this number is ARGUED and not measured, stated so nobody mistakes it
///
/// `ARCHITECTURE.md` §10.1 wants a ceiling on anything consuming
/// attacker-influenced bytes, and decision 021 §3.5 says to corpus-measure it
/// rather than guess — this project has three recorded cases of a guard set
/// by intuition being wrong (`MAX_TOKEN_LEN` 8 KiB, `MAX_XOBJECT_DEPTH` 16,
/// `jpx::MAX_TILES`).
///
/// So the census was run (`tools/fontfile-census`, 4,023 files): embedded
/// font programs top out at **1,195,688 bytes**, p50 11 KB, p90 62 KB. A
/// 2 MiB ceiling would refuse none of them.
///
/// **And those numbers do not apply here.** ISO 32000-1 §9.9 forbids using a
/// program extracted from a PDF as the source for newly authored text — it
/// requires *"a licensed copy of the font program, not a copy extracted from
/// the PDF file"* — so FF-C's donor is always a file from the operator's
/// font folder, and no corpus contains those. The census measured the other
/// half of the world. Quoting its 2 MiB as though it justified this constant
/// would be laundering a guess through a measurement, so: it does not, and
/// this is a judgement.
///
/// The judgement: 64 MiB. A single large CJK face is several megabytes
/// (Noto Sans JP) to a few tens (Source Han Sans, Arial Unicode), so 64 MiB
/// clears every realistic single donor with an order of magnitude to spare,
/// while still refusing a file whose only purpose could be to exhaust
/// memory. It deliberately does NOT clear a large `.ttc` collection, which
/// can exceed 100 MB; that refusal is honest — pdfcer takes one face index
/// out of a collection and has no reason to hold a hundred megabytes to do
/// it, and the operator can point at the face they mean.
///
/// Revisit if a real operator hits it. The refusal names the size and the
/// ceiling precisely so that report is actionable rather than "it didn't
/// work".
pub const MAX_DONOR_BYTES: usize = 64 * 1024 * 1024;

/// Why a donor face could not be turned into a plan.
///
/// Every variant names a distinct cause (R27). A single "font error" would
/// leave the operator unable to tell "pick a different font" from "this file
/// is damaged" from "this is too big".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubsetError {
    /// The file exceeds [`MAX_DONOR_BYTES`].
    #[error(
        "this font file is {size} bytes, above the {limit}-byte limit pdfcer will read. If this is \
         a font collection (.ttc), point pdfcer at a single face instead."
    )]
    TooLarge { size: usize, limit: usize },
    /// The bytes are not a font pdfcer recognises.
    #[error("this file is not a font pdfcer can read (it is not a TrueType or OpenType file)")]
    NotAFont,
    /// The face parsed, but its outlines are not `glyf`.
    ///
    /// Kept distinct from [`Self::NotAFont`] because the operator's next move
    /// differs completely: a CFF face is a *valid* font pdfcer cannot yet
    /// embed, and telling them it is "not a font" would be a lie that costs
    /// them time proving otherwise.
    #[error(
        "this font uses CFF (PostScript) outlines, and pdfcer can currently embed only TrueType \
         outlines. The font is fine — pdfcer's support for this kind is not. Choose a TrueType \
         (.ttf) face."
    )]
    CffNotSupported,
    /// The face is structurally damaged.
    #[error("this font file is damaged and could not be read ({detail})")]
    Malformed { detail: String },
    /// The face uses something `subsetter` has not implemented.
    #[error("this font uses a feature pdfcer's subsetter does not implement yet ({detail})")]
    Unimplemented { detail: String },
    /// A bug in the subsetter, per its own documentation.
    ///
    /// `subsetter` documents `SubsetError`/`OverflowError` as *"indicates
    /// that there is a logical bug in the subsetter"*. Surfaced as its own
    /// variant so that if it is ever seen on a real face it is recognisable
    /// as something to report upstream rather than as operator error.
    #[error(
        "pdfcer's font subsetter hit an internal error on this face ({detail}). This indicates a \
         bug in the subsetter rather than a problem with the font; please report it."
    )]
    SubsetterBug { detail: String },
    /// The donor covers none of the requested characters.
    #[error(
        "this font does not contain any of the characters you asked for, so embedding it would \
         not help. Choose a font that covers them."
    )]
    NoCoverage,
    /// The donor covers only some requested characters.
    ///
    /// A partial embed would produce a document with visible gaps and no
    /// warning — the operator must be told which characters are missing so
    /// the decision is theirs (rule 4).
    #[error(
        "this font is missing {} of the characters you asked for: {}",
        missing.len(),
        missing.iter().collect::<String>()
    )]
    IncompleteCoverage { missing: Vec<char> },
    /// The donor's `fsType` forbids embedding outright (usage value 2).
    ///
    /// OpenType `OS/2` §Comments: Restricted License embedding means the
    /// font *"must not be modified, embedded or exchanged"*. This is the
    /// font author's licence expressed in the file, and pdfcer honours it.
    #[error(
        "this font's own licence bits say it may not be embedded in a document (Restricted License embedding). pdfcer will not override the font author's setting. Choose a different face."
    )]
    EmbeddingNotPermitted,
    /// The donor permits embedding but forbids SUBSETTING (bit 8).
    ///
    /// A separate refusal from [`Self::EmbeddingNotPermitted`], and the
    /// distinction is real: a face at `0x0108` permits *editable embedding*
    /// and forbids subsetting. FF-C always subsets — `subsetter` has no
    /// pass-through mode — so pdfcer cannot honour that combination, and
    /// saying "may not be embedded" would misdescribe the font's licence.
    #[error(
        "this font allows embedding but not SUBSETTING, and pdfcer always subsets (it embeds only the glyphs your text needs). pdfcer can't honour that combination yet. Choose a different face."
    )]
    SubsettingNotPermitted,
    /// The donor permits only BITMAP embedding (bit 9).
    ///
    /// `subsetter` emits `glyf`/CFF outlines only, so bit 9 is unsatisfiable
    /// by FF-C. The specification's own word for this state, when a font has
    /// no bitmaps, is *"unembeddable"*.
    #[error(
        "this font allows only its bitmap glyphs to be embedded, and pdfcer embeds outlines. Choose a different face."
    )]
    OutlineEmbeddingNotPermitted,
    /// The face has no `head` table, so units-per-em is unknown.
    #[error("this font is missing the table that says how its coordinates are scaled")]
    NoHeadTable,
}

/// Build a [`FontEmbedPlan`] covering `chars` from `donor` bytes.
///
/// `face_index` selects a face inside a collection; pass `0` for a plain
/// font file.
///
/// # Errors
///
/// See [`SubsetError`] — every failure is named by cause, and coverage gaps
/// are reported with the specific characters rather than as a count, so the
/// operator can decide rather than guess.
pub fn plan_subset(
    donor: &[u8],
    face_index: u32,
    chars: &[char],
    base_name: &str,
    subset_tag: &str,
) -> Result<FontEmbedPlan, SubsetError> {
    // Bound BEFORE parsing. The ceiling exists to stop pdfcer reading a file
    // whose only purpose is to be enormous, so checking it after handing the
    // bytes to a parser would be checking the wrong side of the door.
    if donor.len() > MAX_DONOR_BYTES {
        return Err(SubsetError::TooLarge {
            size: donor.len(),
            limit: MAX_DONOR_BYTES,
        });
    }

    let font = FontRef::from_index(donor, face_index).map_err(|_| SubsetError::NotAFont)?;

    // Outline kind decides the descriptor key, and CFF cannot be emitted
    // conformantly at the P0 floor — `subsetter` returns an OTTO-wrapped
    // sfnt for CFF donors, while §9.9 Table 126 requires a `cmap` for
    // CFF-outline OpenType programs (which the subsetter removes) and
    // `/CIDFontType0C` wants a bare CFF program rather than a container.
    // Refusing here, before any work, keeps the error close to the cause.
    let outline_kind = if font.glyf().is_ok() {
        OutlineKind::TrueType
    } else {
        return Err(SubsetError::CffNotSupported);
    };

    // R109: read the font author's embedding permission BEFORE subsetting.
    //
    // It has to be before, not after, for a mechanical reason as well as a
    // moral one: `subsetter` strips `OS/2` from its output, so once the
    // subset exists the permission bits are gone and there is nothing left
    // to check.
    check_embedding_permission(&font)?;

    let head = font.head().map_err(|_| SubsetError::NoHeadTable)?;
    let upem = f64::from(head.units_per_em());
    if upem <= 0.0 {
        return Err(SubsetError::Malformed {
            detail: "units-per-em is zero".to_owned(),
        });
    }
    // Everything the PDF sees is in 1000-unit glyph space (§9.7.4.3), which
    // is NOT the font's own space. Scaling once here means no downstream
    // arithmetic has to remember to.
    let to_pdf = |v: f64| -> i32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "glyph-space values are bounded well inside i32 after scaling to 1000/em"
        )]
        {
            (v * 1000.0 / upem).round() as i32
        }
    };

    // Coverage first, so a font that cannot help is refused before any
    // subsetting work happens — and so the refusal can name the characters.
    let charmap = font.charmap();
    let mut wanted: Vec<(char, GlyphId16)> = Vec::new();
    let mut missing: Vec<char> = Vec::new();
    for &c in chars {
        match charmap.map(c) {
            // `.notdef` is GID 0 and means "this font has no glyph for it".
            // Treating it as coverage would embed a face that draws boxes.
            Some(gid) if gid.to_u32() != 0 => {
                let raw = u16::try_from(gid.to_u32()).map_err(|_| SubsetError::Malformed {
                    detail: "glyph id exceeds 16 bits".to_owned(),
                })?;
                wanted.push((c, GlyphId16::new(raw)));
            }
            _ => missing.push(c),
        }
    }
    if wanted.is_empty() {
        return Err(SubsetError::NoCoverage);
    }
    if !missing.is_empty() {
        missing.dedup();
        return Err(SubsetError::IncompleteCoverage { missing });
    }

    // Remap: the subsetter assigns each kept glyph a new, dense GID, and
    // that new GID IS the CID in the emitted CIDFont (§9.7.4.2 with
    // /CIDToGIDMap /Identity). `.notdef` is always member 0.
    let mut mapper = GlyphRemapper::new();
    let mut glyphs: Vec<SubsetGlyph> = Vec::new();
    let metrics = font.glyph_metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    for (c, gid) in &wanted {
        let new_gid = mapper.remap(gid.to_u16());
        let advance = metrics
            .advance_width(skrifa::GlyphId::from(gid.to_u16()))
            .unwrap_or(0.0);
        glyphs.push(SubsetGlyph {
            cid: new_gid,
            width: to_pdf(f64::from(advance)),
            unicode: *c,
        });
    }
    // Ascending by CID: `/W` runs are built by walking this in order, and an
    // unsorted list would emit runs that describe the wrong glyphs.
    glyphs.sort_by_key(|g| g.cid);

    let program = subsetter::subset(donor, face_index, &mapper).map_err(map_subsetter_error)?;

    let bbox = [
        to_pdf(f64::from(head.x_min())),
        to_pdf(f64::from(head.y_min())),
        to_pdf(f64::from(head.x_max())),
        to_pdf(f64::from(head.y_max())),
    ];

    // Descriptor numbers. `post` and `OS/2` are optional in the format, so
    // each is defaulted rather than treated as required — a face without
    // `OS/2` is unusual but not damaged, and refusing it would be a stricter
    // rule than the format's.
    let italic_angle = font
        .post()
        .map(|p| p.italic_angle().to_f32() as f64)
        .unwrap_or(0.0);
    let (ascent, descent, cap_height) = font.os2().map_or_else(
        |_| {
            (
                to_pdf(f64::from(head.y_max())),
                to_pdf(f64::from(head.y_min())),
                0,
            )
        },
        |os2| {
            (
                to_pdf(f64::from(os2.s_typo_ascender())),
                to_pdf(f64::from(os2.s_typo_descender())),
                to_pdf(f64::from(os2.s_cap_height().unwrap_or(0))),
            )
        },
    );

    Ok(FontEmbedPlan {
        program,
        base_name: base_name.to_owned(),
        subset_tag: subset_tag.to_owned(),
        outline_kind,
        glyphs,
        metrics: DescriptorMetrics {
            bbox,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "italic angle is degrees; any real value is inside i32"
            )]
            italic_angle: italic_angle.round() as i32,
            ascent,
            descent,
            cap_height,
            // No table carries /StemV. Every producer estimates it, and the
            // value only affects a viewer's synthetic-substitute rendering —
            // which cannot happen here, because the program is embedded. A
            // fixed, documented estimate beats a fabricated computation that
            // implies precision it does not have.
            stem_v: 80,
            // Nonsymbolic (bit 6, value 32). The donor is being embedded to
            // carry ordinary text the operator typed, which is by definition
            // in the standard character set. Symbolic would tell a consumer
            // to ignore /Encoding — wrong for this use, and Table 123 makes
            // the two mutually exclusive.
            flags: 32,
        },
    })
}

/// Honour the donor's OpenType `fsType` embedding permissions (R109).
///
/// Bit semantics are sourced from the OpenType specification via
/// `pdfcer-spec-librarian` (`PDF_Spec/fonts/font__opentype_os2_fstype.md`),
/// not from recall — this governs a refuse-or-proceed decision about
/// redistributing a third party's font, which is exactly the class of claim
/// rule 1 forbids reconstructing from memory.
///
/// | field | meaning |
/// |---|---|
/// | `& 0x000F` | usage sub-field; valid values 0, 2, 4, 8 |
/// | `0` | Installable — most permissive |
/// | `2` | Restricted License — *"must not be modified, embedded or exchanged"* |
/// | `4` | Preview & Print — may embed, but the document *"must be opened read-only"* |
/// | `8` | Editable — may embed, and edits may be saved |
/// | bit 8 `0x0100` | **No subsetting** |
/// | bit 9 `0x0200` | **Bitmap embedding only** |
///
/// # Two deliberate non-refusals, both flagged rather than decided here
///
/// **Absent or unparseable `OS/2` proceeds.** The specification states no
/// default and no fallback permission for a missing table (RAG gap N1), so
/// this is pdfcer policy, and it is Ken's to set (decision 021 §7.1, open
/// operator question (r)). The trap to avoid is modelling "absent" as `0`:
/// `0` means *Installable*, the MOST permissive value, so defaulting to it
/// would silently grant the broadest right on the least information.
/// Proceeding without pretending the data said so is the honest middle.
///
/// **Value 4 (Preview & Print) proceeds.** It permits embedding; what it
/// additionally requires is that the *document* be opened read-only
/// thereafter — an obligation on every later reader, which pdfcer cannot
/// enforce and which no PDF field expresses. Refusing would block a large
/// share of real fonts over a constraint that is not about the embed itself.
/// Also Ken's call, and also flagged.
///
/// Bits 8 and 9 are only honoured from `OS/2` version 2 onward: the
/// specification says they **must be ignored** on versions 0 and 1, where
/// they had no assigned meaning. Reading them there would refuse fonts on
/// the strength of bytes that never meant anything.
fn check_embedding_permission(font: &FontRef<'_>) -> Result<(), SubsetError> {
    let Ok(os2) = font.os2() else {
        // No OS/2: proceed. See the doc comment — NOT treated as 0.
        return Ok(());
    };
    let fs_type = os2.fs_type();

    if fs_type & 0x000F == 2 {
        return Err(SubsetError::EmbeddingNotPermitted);
    }
    if os2.version() >= 2 {
        if fs_type & 0x0200 != 0 {
            return Err(SubsetError::OutlineEmbeddingNotPermitted);
        }
        if fs_type & 0x0100 != 0 {
            return Err(SubsetError::SubsettingNotPermitted);
        }
    }
    Ok(())
}

/// Map `subsetter`'s errors to pdfcer's, preserving the distinction between
/// "this font is unusual" and "this is our bug".
fn map_subsetter_error(e: subsetter::Error) -> SubsetError {
    let detail = e.to_string();
    match e {
        subsetter::Error::UnknownKind => SubsetError::NotAFont,
        subsetter::Error::MalformedFont => SubsetError::Malformed { detail },
        subsetter::Error::Unimplemented => SubsetError::Unimplemented { detail },
        // Documented upstream as indicating a logical bug in the subsetter.
        subsetter::Error::SubsetError
        | subsetter::Error::OverflowError
        | subsetter::Error::CFFError => SubsetError::SubsetterBug { detail },
    }
}

/// Six uppercase ASCII letters derived from a face name, for the §9.6.4
/// subset prefix.
///
/// Deterministic rather than random. A random tag would be equally valid and
/// would make two otherwise identical runs produce different bytes, which
/// breaks byte-comparison — and byte-comparison is how this project proves its
/// round-trip invariant. Derived from the name so two different faces in one
/// document are unlikely to collide.
///
/// Not a hash with collision guarantees, and does not pretend to be: if two
/// faces ever do collide, the consequence is two subsets sharing a tag, which
/// consumers tolerate (the tag is a hint, not an identifier). Spending entropy
/// here to avoid a harmless collision would cost the determinism, which is the
/// property actually worth having.
///
/// # Why it lives HERE and not in a shell
///
/// It was `pdfcer`-private until the GUI grew its own embed path (the Pass
/// 21.0 GUI slice). Two copies of a tag derivation is two chances to change
/// one of them, after which the CLI and the GUI would write DIFFERENT bytes
/// for the same document and the same donor — and the round-trip harness
/// compares bytes. Hoisted for exactly the reason
/// [`FontEnvironment::subset_stem`](crate::FontEnvironment::subset_stem)
/// already was: one copy, in the crate both shells already depend on, keeping
/// `pdfcer-render` GUI-dependency-free.
///
/// ```
/// use pdfcer_render::font::subset::subset_tag_for;
/// let tag = subset_tag_for("NotoSans");
/// assert_eq!(tag.len(), 6);
/// assert!(tag.bytes().all(|b| b.is_ascii_uppercase()));
/// // Deterministic: the same name always yields the same tag.
/// assert_eq!(tag, subset_tag_for("NotoSans"));
/// ```
#[must_use]
pub fn subset_tag_for(name: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (0..6)
        .map(|i| {
            let k = (h >> (i * 8)) & 0xff;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the modulo keeps this inside the 26-letter alphabet"
            )]
            char::from(b'A' + (k % 26) as u8)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The ceiling must refuse BEFORE parsing, or it is not bounding the
    /// thing it claims to bound. A 65 MiB buffer of zeros is not a font, so
    /// if the size check were ordered after the parse this would surface as
    /// `NotAFont` and the ceiling would be untested.
    #[test]
    fn oversized_donor_is_refused_by_size_not_by_parse_failure() {
        let huge = vec![0u8; MAX_DONOR_BYTES + 1];
        let err = plan_subset(&huge, 0, &['A'], "X", "ABCDEF").unwrap_err();
        match err {
            SubsetError::TooLarge { size, limit } => {
                assert_eq!(size, MAX_DONOR_BYTES + 1);
                assert_eq!(limit, MAX_DONOR_BYTES);
            }
            other => panic!(
                "expected TooLarge (the ceiling must be checked before parsing, or it bounds \
                 nothing); got {other:?}"
            ),
        }
        // Positive control: a buffer one byte UNDER the ceiling must get
        // past the size gate and fail for a different reason, proving the
        // gate is a ceiling rather than a blanket refusal.
        let under = vec![0u8; 64];
        assert_eq!(
            plan_subset(&under, 0, &['A'], "X", "ABCDEF").unwrap_err(),
            SubsetError::NotAFont
        );
    }

    /// The synthetic donor from `tools/gen-subset-font-fixtures.py`.
    ///
    /// A REAL font file on disk, not a program lifted out of a PDF: ISO
    /// 32000-1 §9.9 forbids using an extracted program as the source for new
    /// text ("a licensed copy of the font program, not a copy extracted from
    /// the PDF file"), so the donor path can only be exercised with a
    /// standalone face. Without it every test here would be an error-path
    /// test and the code that actually does the work would be uncovered.
    fn donor() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/synthetic/text/subset-donor.ttf"
        );
        std::fs::read(path).unwrap_or_else(|e| {
            panic!("missing donor fixture at {path}: {e}. Run `python tools/gen-subset-font-fixtures.py`.")
        })
    }

    /// The happy path, end to end: a real face in, a well-formed plan out.
    #[test]
    fn plans_a_subset_from_a_real_donor_face() {
        let plan = plan_subset(&donor(), 0, &['A', 'C'], "pdfceSubsetDemo", "ABCDEF")
            .expect("the donor covers A and C");

        assert_eq!(plan.outline_kind, OutlineKind::TrueType);
        assert_eq!(plan.tagged_name(), "ABCDEF+pdfceSubsetDemo");
        assert!(
            plan.validate().is_ok(),
            "the produced plan must be emittable"
        );

        // The subsetted program is a real sfnt and SMALLER than the donor —
        // if it were not, nothing was actually subsetted and the whole
        // feature is a no-op wearing a costume.
        assert_eq!(&plan.program[..4], &[0x00, 0x01, 0x00, 0x00], "sfnt magic");
        assert!(
            plan.program.len() <= donor().len(),
            "subsetting must not GROW the program: {} > {}",
            plan.program.len(),
            donor().len()
        );

        // Two glyphs requested, two carried, and the /ToUnicode side knows
        // which characters they were.
        assert_eq!(plan.glyphs.len(), 2);
        let chars: Vec<char> = plan.glyphs.iter().map(|g| g.unicode).collect();
        assert!(chars.contains(&'A') && chars.contains(&'C'), "{chars:?}");

        // CIDs must be ascending — `/W` runs are built by walking this in
        // order, so an unsorted list emits widths against the wrong glyphs.
        let cids: Vec<u16> = plan.glyphs.iter().map(|g| g.cid).collect();
        let mut sorted = cids.clone();
        sorted.sort_unstable();
        assert_eq!(cids, sorted, "glyphs must be ascending by CID");

        // CID 0 is `.notdef` and is never a text glyph.
        assert!(!cids.contains(&0), "no text glyph may be CID 0: {cids:?}");

        // The donor is 1000 upem with a 600 advance, so the PDF-space width
        // is 600 with no scaling. A wrong upem conversion would show here.
        for g in &plan.glyphs {
            assert_eq!(g.width, 600, "advance for {:?}", g.unicode);
        }
    }

    /// A character the donor genuinely lacks must be named, not silently
    /// dropped — a partial embed produces visible gaps and no warning, which
    /// is the rule-4 failure this refusal exists to prevent.
    #[test]
    fn a_character_the_donor_lacks_is_refused_by_name() {
        let err = plan_subset(&donor(), 0, &['A', 'Z'], "pdfceSubsetDemo", "ABCDEF").unwrap_err();
        assert_eq!(err, SubsetError::IncompleteCoverage { missing: vec!['Z'] });
        assert!(err.to_string().contains('Z'), "{err}");
    }

    /// A request the donor covers NONE of is a different situation from a
    /// partial gap, and gets its own refusal: "choose another font" rather
    /// than "these characters are missing".
    #[test]
    fn a_donor_covering_nothing_requested_is_refused_distinctly() {
        let err = plan_subset(&donor(), 0, &['Z', 'Q'], "pdfceSubsetDemo", "ABCDEF").unwrap_err();
        assert_eq!(err, SubsetError::NoCoverage);
    }

    /// A donor whose composite glyphs form CYCLES must terminate.
    ///
    /// `subset-cycle-donor.ttf` carries two shapes a naive recursive `glyf`
    /// walk would follow forever: `gSelf` references itself, and `gPing`/
    /// `gPong` reference each other. The second matters separately — a depth
    /// counter reset per glyph would catch the first and miss it.
    ///
    /// # Why this is a test and not a guard
    ///
    /// `ARCHITECTURE.md` §10 would normally demand a depth cap here.
    /// Decision 021 §3.5 deliberately declines: `subsetter`'s `closure()` is
    /// an iterative worklist that enqueues a component only when the remapper
    /// has not seen it, so the visited set grows monotonically and is bounded
    /// by `numGlyphs`. It terminates structurally, upstream. A pdfcer-side cap
    /// would sit behind a filter its guarded case cannot pass — a guard that
    /// reads as protection and executes never (R96).
    ///
    /// So the property is asserted instead of defended, and this test can do
    /// what a redundant guard never could: **fail** if upstream ever rewrites
    /// that walk recursively.
    ///
    /// The assertion is simply that this returns. A stack overflow aborts the
    /// process rather than unwinding, so "the test finished" IS the result —
    /// there is no cleverer check available, and pretending otherwise by
    /// wrapping it in something that looks more rigorous would be theatre.
    ///
    /// Worth recording: fontTools cannot even WRITE this font. Building the
    /// cycle directly dies with a Python RecursionError, because its bounds
    /// recalculation walks components recursively. The fixture is built
    /// acyclic and the component indices patched in the compiled `glyf`
    /// bytes. That a mature font library takes the recursive route is the
    /// best evidence that the non-recursive one is worth asserting.
    #[test]
    fn composite_glyph_cycles_terminate() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/synthetic/text/subset-cycle-donor.ttf"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| {
            panic!("missing cycle fixture at {path}: {e}. Run `python tools/gen-subset-font-fixtures.py`.")
        });

        // 'S' -> gSelf (one-glyph cycle).
        let _ = plan_subset(&bytes, 0, &['S'], "pdfceCycleDemo", "ABCDEF");
        // 'P' -> gPing -> gPong -> gPing (two-glyph cycle).
        let _ = plan_subset(&bytes, 0, &['P'], "pdfceCycleDemo", "ABCDEF");
        // Both cycles reachable at once, plus a real outline, so the walk has
        // somewhere legitimate to go as well as somewhere circular.
        let _ = plan_subset(&bytes, 0, &['A', 'S', 'P', 'Q'], "pdfceCycleDemo", "ABCDEF");
    }

    fn fstype_donor(label: &str) -> Vec<u8> {
        let path = format!(
            "{}/../../fixtures/synthetic/text/subset-fstype-{label}.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|e| {
            panic!("missing fixture {path}: {e}. Run `python tools/gen-subset-font-fixtures.py`.")
        })
    }

    fn plan_fstype(label: &str) -> Result<pdfcer_core::font_embed::FontEmbedPlan, SubsetError> {
        plan_subset(&fstype_donor(label), 0, &['A'], "pdfceFsTypeDemo", "ABCDEF")
    }

    /// R109: the font author's embedding licence, honoured per value.
    ///
    /// Every arm is asserted as FIRING, not merely as "the happy path still
    /// works" (R96) — and the permissive arms are asserted too, because a
    /// check that refused everything would satisfy the refusal assertions
    /// alone and break the feature completely.
    #[test]
    fn fstype_embedding_permissions_are_honoured_per_value() {
        // Value 0 (Installable) and 8 (Editable) both permit embedding.
        assert!(plan_fstype("installable").is_ok(), "Installable must embed");
        assert!(plan_fstype("editable").is_ok(), "Editable must embed");

        // Value 2 (Restricted License) forbids embedding outright.
        assert_eq!(
            plan_fstype("restricted").unwrap_err(),
            SubsetError::EmbeddingNotPermitted
        );

        // Bit 8: permits embedding, forbids SUBSETTING — a genuinely
        // different refusal, and one that would be misdescribed as "may not
        // be embedded" if the two were merged.
        assert_eq!(
            plan_fstype("nosubset").unwrap_err(),
            SubsetError::SubsettingNotPermitted
        );

        // Bit 9: outlines forbidden; pdfcer embeds outlines only.
        assert_eq!(
            plan_fstype("bitmaponly").unwrap_err(),
            SubsetError::OutlineEmbeddingNotPermitted
        );
    }

    /// Bits 8 and 9 had no assigned meaning before `OS/2` version 2, and the
    /// specification says a consumer **must ignore** them there.
    ///
    /// The fixture pair carries IDENTICAL bits (`0x0108`) at v4 and v1. If
    /// pdfcer read them unconditionally, both would refuse and this test
    /// would fail — which is the whole point: version gating is invisible
    /// unless something asserts the same bytes mean different things.
    #[test]
    fn bits_8_and_9_are_ignored_before_os2_version_2() {
        assert_eq!(
            plan_fstype("nosubset").unwrap_err(),
            SubsetError::SubsettingNotPermitted,
            "at OS/2 v4 bit 8 must be honoured"
        );
        assert!(
            plan_fstype("nosubset-v1").is_ok(),
            "at OS/2 v1 bits 8/9 must be IGNORED — the spec says a consumer must not read them there, so refusing would reject a font on the strength of bytes that never meant anything"
        );
    }

    /// Preview & Print permits the EMBED; what it additionally requires is
    /// that the document be opened read-only afterwards — an obligation on
    /// every later reader that pdfcer cannot enforce and no PDF field
    /// expresses.
    ///
    /// pdfcer proceeds. Refusing would block a large share of real fonts over
    /// a constraint that is not about the embedding action itself. Asserted
    /// explicitly so the choice is visible as a choice rather than as an
    /// omission, and flagged as decision 021 §7.1 / open operator question
    /// (r) — Ken's to settle.
    #[test]
    fn preview_and_print_proceeds_and_that_is_a_recorded_choice() {
        assert!(plan_fstype("preview-print").is_ok());
    }

    /// A font with NO `OS/2` proceeds, and must NOT be modelled as value 0.
    ///
    /// The specification states no default for a missing table, so this is
    /// pdfcer policy (open operator question (r)). The trap is that `0` means
    /// *Installable* — the most permissive value — so defaulting to it would
    /// silently grant the broadest right on the least information. The donor
    /// fixture from the sibling tests has no `OS/2` at all, which is exactly
    /// the case.
    #[test]
    fn a_font_without_os2_proceeds_without_being_treated_as_installable() {
        assert!(
            plan_subset(&donor(), 0, &['A'], "pdfceSubsetDemo", "ABCDEF").is_ok(),
            "a font with no OS/2 must still be embeddable"
        );
    }

    #[test]
    fn garbage_is_refused_as_not_a_font() {
        let err = plan_subset(b"this is not a font at all", 0, &['A'], "X", "ABCDEF").unwrap_err();
        assert_eq!(err, SubsetError::NotAFont);
    }

    /// Refusals have to tell the operator what to do next. A message that
    /// only says "no" is a dead end (R27), and the CFF case is the one most
    /// likely to be misread as "your font is broken" when it is not.
    #[test]
    fn cff_refusal_says_the_font_is_fine_and_pdfcer_is_not() {
        let text = SubsetError::CffNotSupported.to_string();
        assert!(text.contains("The font is fine"), "{text}");
        assert!(
            text.contains("TrueType"),
            "must name what to choose instead: {text}"
        );
    }

    #[test]
    fn incomplete_coverage_names_the_missing_characters() {
        let text = SubsetError::IncompleteCoverage {
            missing: vec!['あ', 'い'],
        }
        .to_string();
        assert!(text.contains('あ'), "the operator must see WHICH: {text}");
        assert!(text.contains('い'), "{text}");
    }

    /// `subsetter` documents two of its variants as indicating a bug in
    /// itself. Those must not be reported as operator error, or someone will
    /// spend an afternoon proving their font is valid.
    #[test]
    fn subsetter_internal_errors_are_labelled_as_our_bug() {
        for e in [
            subsetter::Error::SubsetError,
            subsetter::Error::OverflowError,
            subsetter::Error::CFFError,
        ] {
            let mapped = map_subsetter_error(e);
            assert!(
                matches!(mapped, SubsetError::SubsetterBug { .. }),
                "{e:?} should map to SubsetterBug, got {mapped:?}"
            );
            assert!(
                mapped.to_string().contains("report it"),
                "an internal error must ask to be reported: {mapped}"
            );
        }
        // And the operator-facing ones must NOT be labelled our bug, or the
        // distinction is worthless.
        assert!(matches!(
            map_subsetter_error(subsetter::Error::MalformedFont),
            SubsetError::Malformed { .. }
        ));
        assert!(matches!(
            map_subsetter_error(subsetter::Error::UnknownKind),
            SubsetError::NotAFont
        ));
    }
}
