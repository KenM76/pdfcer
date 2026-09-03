//! Emit a subsetted donor face as a NEW PDF font resource (FF-C, Pass 21.0).
//!
//! # What this module is, and the one thing it must never do
//!
//! `pdfcer-render` parses a donor font file and subsets it, producing a
//! [`FontEmbedPlan`] — plain data, no font types, no crate coupling. This
//! module turns that plan into PDF objects: a `/Type0` font dictionary, its
//! `/CIDFontType2` descendant, a `/FontDescriptor`, the `FontFile2` stream
//! and a `/ToUnicode` CMap.
//!
//! **It only ever ALLOCATES objects. It never rewrites an existing one.**
//! That is standing rule R107, and it is what keeps the whole FF-C family
//! free of a round-trip exception (`ARCHITECTURE.md` §5): every object the
//! feature touches is either brand new or the page dictionary that
//! `addtext.rs` was already going to rewrite anyway.
//!
//! # Why the crate split runs this way (decision 021 §3.2)
//!
//! Subsetting is a write concern, so `pdfcer-core` looks like its natural
//! home. It is not. Producing a subset first requires *parsing* the donor —
//! coverage from `cmap`, advances from `hmtx`, descriptor metrics and the
//! embedding-permission bits — and that parser already exists in
//! `pdfcer-render`. Putting a subsetter here would add a second font parser
//! to a crate that has none (`fontdata/` is compiled metrics only), purely
//! to avoid a plain-data seam. So the seam is the design: render produces,
//! core emits, and `pdfcer-core` gains no new dependency.
//!
//! # Why the emitted font is always `/Type0` + `Identity-H`
//!
//! Not a preference — two independent forcings agree:
//!
//! * **The subsetter removes `cmap`.** Typst's `subsetter` states it
//!   outright: *"You must write your fonts as a CID font. This is because we
//!   remove the `cmap` table from the font, so you must provide your own
//!   cmap table in the PDF."*
//! * **The specification requires it.** ISO 32000-1 §9.9 says that under a
//!   CIDFont dictionary *"the `cmap` table is not needed and shall not be
//!   present"*, and puts a `shall` on conforming writers to use `/Type0`
//!   with `Identity-H` for OpenType `glyf` programs.
//!
//! The happy consequence is that the CID **is** the subsetter's remapped
//! GID, so `/CIDToGIDMap` is `/Identity` and no mapping stream is needed.
//!
//! # Scope of the P0 floor
//!
//! TrueType-outline (`glyf`) donors only. CFF donors are refused by name —
//! `subsetter` wraps CFF output in an `OTTO` sfnt, and ISO 32000-1 §9.9
//! Table 126 requires a `cmap` for CFF-outline OpenType programs (which the
//! subsetter has just removed), while `/CIDFontType0C` wants a *bare* CFF
//! program rather than a container. Emitting either key would be
//! non-conformant, so the refusal is honest rather than a guess. See
//! decision 021 §10 (C-3).

use crate::object::{Dict, Name, ObjId, Object};

/// Which outline flavour a donor's subsetted program carries.
///
/// Only [`Self::TrueType`] can be emitted at the P0 floor. The CFF arm
/// exists so the *refusal* can name what it refused rather than reporting a
/// generic failure — a caller that cannot tell "this font is unsupported"
/// from "this font is broken" gives the operator nothing to act on (R27).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutlineKind {
    /// `glyf`/`loca` outlines. Emitted as `/CIDFontType2` + `/FontFile2`.
    TrueType,
    /// CFF outlines, arriving inside an `OTTO` wrapper. Refused at P0.
    Cff,
}

/// Descriptor metrics read from the donor, in 1000-unit glyph space.
///
/// These are the `/FontDescriptor` numbers ISO 32000-1 §9.8.1 Table 122
/// requires. They are carried as plain integers rather than being recomputed
/// here because they come from the donor's own tables, and `pdfcer-core` has
/// no font parser to recompute them with — which is the entire reason for
/// the crate split described in the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptorMetrics {
    /// `/FontBBox`, already scaled to 1000 units/em.
    pub bbox: [i32; 4],
    /// `/ItalicAngle` — degrees, negative leans right.
    pub italic_angle: i32,
    /// `/Ascent`.
    pub ascent: i32,
    /// `/Descent` — negative.
    pub descent: i32,
    /// `/CapHeight`.
    pub cap_height: i32,
    /// `/StemV`. No table carries it directly; the producer estimates.
    pub stem_v: i32,
    /// `/Flags` (Table 123). Bit 3 (`Nonsymbolic`, value 32) or bit 3
    /// (`Symbolic`, value 4) — mutually exclusive per the table.
    pub flags: i32,
}

/// One glyph in the subset: its CID, its advance, and what it means.
///
/// `cid` is the subsetter's remapped GID (see the module docs on why those
/// are the same number). `unicode` is what the original character was, kept
/// so the `/ToUnicode` CMap can be authored — without it the embedded text
/// would be uncopyable and unsearchable, which is a silent accessibility
/// regression rather than a visible bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsetGlyph {
    /// CID == remapped GID.
    pub cid: u16,
    /// Advance width in 1000-unit glyph space.
    pub width: i32,
    /// The character this glyph was selected for.
    pub unicode: char,
}

/// Everything `pdfcer-core` needs to emit an embedded subset, and nothing
/// about how it was produced.
///
/// Deliberately free of any font-crate type. The seam is plain data so that
/// `pdfcer-core` never acquires a font parser (R21 / decision 021 §3.2), and
/// so this module is testable without one — every test below constructs a
/// plan by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEmbedPlan {
    /// The subsetted sfnt program, exactly as it will be embedded.
    pub program: Vec<u8>,
    /// The donor's PostScript name, WITHOUT a subset tag.
    pub base_name: String,
    /// The six-uppercase-letter subset tag, WITHOUT the `+`.
    ///
    /// ISO 32000-1 §9.6.4. Held separate from `base_name` so the two can
    /// never be concatenated in the wrong order or double-tagged; the `+`
    /// is inserted in exactly one place, [`Self::tagged_name`].
    pub subset_tag: String,
    /// Outline flavour, for the descriptor key and the P0 refusal.
    pub outline_kind: OutlineKind,
    /// The glyphs, ascending by CID.
    pub glyphs: Vec<SubsetGlyph>,
    /// Descriptor metrics from the donor.
    pub metrics: DescriptorMetrics,
}

impl FontEmbedPlan {
    /// `SUBSET+FamilyName`, the `/BaseFont` and `/FontName` value.
    ///
    /// The single place the `+` is inserted. ISO 32000-1 §9.6.4 requires
    /// exactly six uppercase ASCII letters before it; [`Self::validate`]
    /// checks that rather than trusting the producer, because a malformed
    /// tag would make the font look non-subset to every consumer including
    /// pdfcer's own `is_subset_tag`.
    #[must_use]
    pub fn tagged_name(&self) -> String {
        format!("{}+{}", self.subset_tag, self.base_name)
    }

    /// Reject a plan that could only produce a non-conformant font.
    ///
    /// # Errors
    ///
    /// Returns [`FontEmbedError`] for a CFF-outline donor (unsupported at
    /// the P0 floor), a malformed subset tag, an empty glyph set, or an
    /// empty program.
    pub fn validate(&self) -> Result<(), FontEmbedError> {
        if self.outline_kind != OutlineKind::TrueType {
            return Err(FontEmbedError::OutlineKindUnsupported {
                kind: self.outline_kind,
            });
        }
        let tag_ok =
            self.subset_tag.len() == 6 && self.subset_tag.bytes().all(|b| b.is_ascii_uppercase());
        if !tag_ok {
            return Err(FontEmbedError::MalformedSubsetTag {
                tag: self.subset_tag.clone(),
            });
        }
        if self.base_name.is_empty() {
            return Err(FontEmbedError::MalformedSubsetTag {
                tag: self.subset_tag.clone(),
            });
        }
        if self.glyphs.is_empty() {
            return Err(FontEmbedError::EmptySubset);
        }
        if self.program.is_empty() {
            return Err(FontEmbedError::EmptyProgram);
        }
        Ok(())
    }
}

/// Why a plan could not be emitted.
///
/// Every variant names a specific cause. A generic "embedding failed" would
/// leave the operator with no action to take, which is the failure mode R27
/// exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FontEmbedError {
    /// The donor's outlines are not `glyf`. See the module docs on why CFF
    /// cannot be emitted conformantly at the P0 floor.
    #[error(
        "this font's outlines are {kind:?}, and pdfcer can currently embed only TrueType (glyf) \
         outlines; a CFF-outline face cannot yet be written out in a form the PDF specification \
         permits. Choose a TrueType face, or keep this edit to characters already on the page."
    )]
    OutlineKindUnsupported { kind: OutlineKind },
    /// The subset tag is not six uppercase ASCII letters (§9.6.4).
    #[error(
        "internal: subset tag {tag:?} is not six uppercase ASCII letters, which ISO 32000-1 \
         §9.6.4 requires; the font would not be recognisable as a subset"
    )]
    MalformedSubsetTag { tag: String },
    /// No glyphs were selected.
    #[error("internal: the subset selected no glyphs, so there is nothing to embed")]
    EmptySubset,
    /// The subsetted program is empty.
    #[error("internal: the subsetted font program is empty")]
    EmptyProgram,
    /// The document has no object numbers left.
    #[error("this document has no free object numbers left, so no font could be added")]
    ObjectNumbersExhausted,
}

/// The objects a [`FontEmbedPlan`] becomes, all freshly allocated.
///
/// `font_dict_id` is the id the page's `/Font` sub-dictionary should
/// reference. The caller merges that one entry into the page resources; this
/// module deliberately does not, because the page dictionary is the one
/// object in the operation that is *not* new, and keeping its mutation in
/// the caller makes R107's "only new objects" property a local, checkable
/// fact about this function rather than a whole-pipeline claim.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedFontObjects {
    /// The `/Type0` dictionary the page resource points at.
    pub font_dict_id: ObjId,
    /// Every object to write, in allocation order.
    pub objects: Vec<(ObjId, Object)>,
}

impl EmbeddedFontObjects {
    /// The ids this emission writes to.
    ///
    /// Exists so R107 can be asserted over data rather than enforced by a
    /// runtime guard. A guard inside an emitter that can only allocate fresh
    /// numbers would sit behind a filter its guarded case cannot pass —
    /// unreachable code that reads as protection (R96). A test that compares
    /// this set against the document's pre-existing font objects can
    /// actually fail if someone later "optimises" the emitter into reusing
    /// one.
    #[must_use]
    pub fn written_ids(&self) -> Vec<ObjId> {
        self.objects.iter().map(|(id, _)| *id).collect()
    }
}

/// Build the PDF objects for an embedded subset, starting at `first_number`.
///
/// Allocates five consecutive object numbers — `/Type0`, `/CIDFontType2`,
/// `/FontDescriptor`, `FontFile2`, `/ToUnicode` — so the incremental update
/// section stays compact and deterministic, matching `addtext.rs`'s
/// consecutive-pair convention.
///
/// The `program_span` is where the caller has staged the font bytes; this
/// module does not own the staging buffer, for the same reason it does not
/// own the page dictionary — the fewer things it touches, the sharper the
/// R107 claim.
///
/// # Errors
///
/// Propagates [`FontEmbedPlan::validate`], and returns
/// [`FontEmbedError::ObjectNumbersExhausted`] if five consecutive numbers do
/// not fit.
pub fn build_objects(
    plan: &FontEmbedPlan,
    first_number: u32,
    program_stream: Object,
) -> Result<EmbeddedFontObjects, FontEmbedError> {
    plan.validate()?;

    // Five consecutive numbers. `checked_add` on the LAST one is the only
    // check needed — if the highest fits, all the lower ones do.
    // Allocated by name rather than into a Vec that is then indexed:
    // `clippy::indexing_slicing` is DENIED crate-wide because pdfcer-core eats
    // untrusted input and a panic here would be a denial-of-service bug, not
    // a style complaint (see lib.rs). Naming each id also makes an off-by-one
    // in the allocation order a compile error instead of a silently swapped
    // pair of dictionaries.
    let alloc = |k: u32| -> Result<ObjId, FontEmbedError> {
        first_number
            .checked_add(k)
            .map(|n| ObjId::new(n, 0))
            .ok_or(FontEmbedError::ObjectNumbersExhausted)
    };
    let type0_id = alloc(0)?;
    let cid_id = alloc(1)?;
    let desc_id = alloc(2)?;
    let file_id = alloc(3)?;
    let tounicode_id = alloc(4)?;

    let tagged = plan.tagged_name();

    // /Type0 wrapper (§9.7.6.2 Table 121).
    let mut type0 = Dict::new();
    type0.insert(Name::from(b"Type"), Object::Name(Name::from(b"Font")));
    type0.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Type0")));
    type0.insert(
        Name::from(b"BaseFont"),
        Object::Name(Name(tagged.clone().into_bytes())),
    );
    type0.insert(
        Name::from(b"Encoding"),
        Object::Name(Name::from(b"Identity-H")),
    );
    type0.insert(
        Name::from(b"DescendantFonts"),
        Object::Array(vec![Object::Reference(cid_id)]),
    );
    type0.insert(Name::from(b"ToUnicode"), Object::Reference(tounicode_id));

    // /CIDFontType2 descendant (§9.7.4.1 Table 117).
    let mut cid = Dict::new();
    cid.insert(Name::from(b"Type"), Object::Name(Name::from(b"Font")));
    cid.insert(
        Name::from(b"Subtype"),
        Object::Name(Name::from(b"CIDFontType2")),
    );
    cid.insert(
        Name::from(b"BaseFont"),
        Object::Name(Name(tagged.clone().into_bytes())),
    );
    let mut csi = Dict::new();
    csi.insert(Name::from(b"Registry"), Object::String(b"Adobe".to_vec()));
    csi.insert(
        Name::from(b"Ordering"),
        Object::String(b"Identity".to_vec()),
    );
    csi.insert(Name::from(b"Supplement"), Object::Integer(0));
    cid.insert(Name::from(b"CIDSystemInfo"), Object::Dict(csi));
    cid.insert(Name::from(b"FontDescriptor"), Object::Reference(desc_id));
    // /CIDToGIDMap /Identity is correct precisely because the subsetter's
    // remapped GID IS the CID (§9.7.4.2). Writing a stream here would be
    // valid but redundant, and a redundant table is a table that can drift.
    cid.insert(
        Name::from(b"CIDToGIDMap"),
        Object::Name(Name::from(b"Identity")),
    );
    cid.insert(Name::from(b"DW"), Object::Integer(1000));
    cid.insert(Name::from(b"W"), Object::Array(widths_array(&plan.glyphs)));

    // /FontDescriptor (§9.8.1 Table 122).
    let m = plan.metrics;
    let mut desc = Dict::new();
    desc.insert(
        Name::from(b"Type"),
        Object::Name(Name::from(b"FontDescriptor")),
    );
    desc.insert(
        Name::from(b"FontName"),
        Object::Name(Name(tagged.into_bytes())),
    );
    desc.insert(Name::from(b"Flags"), Object::Integer(i64::from(m.flags)));
    desc.insert(
        Name::from(b"FontBBox"),
        Object::Array(
            m.bbox
                .iter()
                .map(|v| Object::Integer(i64::from(*v)))
                .collect(),
        ),
    );
    desc.insert(
        Name::from(b"ItalicAngle"),
        Object::Integer(i64::from(m.italic_angle)),
    );
    desc.insert(Name::from(b"Ascent"), Object::Integer(i64::from(m.ascent)));
    desc.insert(
        Name::from(b"Descent"),
        Object::Integer(i64::from(m.descent)),
    );
    desc.insert(
        Name::from(b"CapHeight"),
        Object::Integer(i64::from(m.cap_height)),
    );
    desc.insert(Name::from(b"StemV"), Object::Integer(i64::from(m.stem_v)));
    // FontFile2 is the TrueType key (§9.9 Table 126). `validate` has already
    // refused every other outline kind, so this is not a silent assumption.
    desc.insert(Name::from(b"FontFile2"), Object::Reference(file_id));

    let tounicode = to_unicode_cmap(&plan.glyphs);

    Ok(EmbeddedFontObjects {
        font_dict_id: type0_id,
        objects: vec![
            (type0_id, Object::Dict(type0)),
            (cid_id, Object::Dict(cid)),
            (desc_id, Object::Dict(desc)),
            (file_id, program_stream),
            (tounicode_id, tounicode),
        ],
    })
}

/// Build the `/W` array (§9.7.4.3) in the `c [w1 w2 …]` run form.
///
/// The run form is chosen over the `c_first c_last w` form because the
/// subsetter's CIDs are dense and ascending from 1, so runs compress far
/// better — and because a single run is easier to read in a byte diff, which
/// matters for a feature whose whole invariant story is about diffs.
///
/// Glyphs whose advance equals the `/DW` default of 1000 are still written.
/// Omitting them would be smaller and would also mean the array silently
/// stopped describing the font if `/DW` ever changed; explicitness is worth
/// more than the bytes here.
fn widths_array(glyphs: &[SubsetGlyph]) -> Vec<Object> {
    // Iterator-driven rather than index-driven. `clippy::indexing_slicing` is
    // DENIED crate-wide (lib.rs) because this crate parses untrusted input,
    // and while `glyphs` here is pdfcer's own construction, carving out an
    // exception per call site is how a deny-by-default rule quietly becomes
    // advisory. The iterator form also cannot express the off-by-one the
    // index form invited: `glyphs[j - 1]` was reachable only because `j`
    // started at `i + 1`, which is exactly the kind of reasoning a reader
    // has to redo every time.
    let mut out: Vec<Object> = Vec::new();
    let mut run: Vec<Object> = Vec::new();
    let mut run_start: Option<u16> = None;
    let mut prev_cid: Option<u16> = None;

    for g in glyphs {
        let consecutive = prev_cid.is_some_and(|p| g.cid == p.wrapping_add(1));
        if !consecutive {
            // Close the open run, if any. A CID gap MUST start a new run, or
            // every width after the gap is attributed to the wrong glyph.
            if let Some(start) = run_start.take() {
                out.push(Object::Integer(i64::from(start)));
                out.push(Object::Array(std::mem::take(&mut run)));
            }
            run_start = Some(g.cid);
        }
        run.push(Object::Integer(i64::from(g.width)));
        prev_cid = Some(g.cid);
    }
    if let Some(start) = run_start {
        out.push(Object::Integer(i64::from(start)));
        out.push(Object::Array(run));
    }
    out
}

/// Author the `/ToUnicode` CMap (§9.10.3).
///
/// Without this the embedded text is invisible to copy, search, and every
/// screen reader — a regression that produces a perfect-looking page and an
/// unusable document, which is exactly the class of failure that is hardest
/// to notice from a screenshot.
///
/// Emitted as `beginbfchar` entries rather than ranges: the CID set is small
/// by construction (it is a subset), and one entry per glyph is trivially
/// verifiable as injective, which standing rule R110 will require when
/// composite runs become editable.
fn to_unicode_cmap(glyphs: &[SubsetGlyph]) -> Object {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\n\
         begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n\
         /CMapType 2 def\n\
         1 begincodespacerange\n\
         <0000> <FFFF>\n\
         endcodespacerange\n",
    );
    // `beginbfchar` admits at most 100 entries per block (§9.10.3).
    for chunk in glyphs.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for g in chunk {
            let mut buf = [0u16; 2];
            let units = g.unicode.encode_utf16(&mut buf);
            let hex: String = units.iter().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{:04X}> <{}>\n", g.cid, hex));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    Object::String(s.into_bytes())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn plan() -> FontEmbedPlan {
        FontEmbedPlan {
            program: vec![0x00, 0x01, 0x00, 0x00, 0xAB, 0xCD],
            base_name: "NotoSansJP".to_owned(),
            subset_tag: "ABCDEF".to_owned(),
            outline_kind: OutlineKind::TrueType,
            glyphs: vec![
                SubsetGlyph {
                    cid: 1,
                    width: 600,
                    unicode: 'A',
                },
                SubsetGlyph {
                    cid: 2,
                    width: 620,
                    unicode: 'B',
                },
            ],
            metrics: DescriptorMetrics {
                bbox: [0, -200, 1000, 800],
                italic_angle: 0,
                ascent: 800,
                descent: -200,
                cap_height: 700,
                stem_v: 80,
                flags: 32,
            },
        }
    }

    /// **R107, the load-bearing test of this whole family.**
    ///
    /// Decision 021 requires it be written in 21.0 — while the emitter is
    /// trivially correct — rather than in 21.2, when `set-font` creates the
    /// temptation to "just widen the existing font" instead of adding a
    /// second resource. It is deliberately a test and not a runtime guard: a
    /// guard inside a function that can only allocate fresh numbers is
    /// unreachable by construction, which is R96's dead code that looks
    /// live.
    #[test]
    fn emission_touches_only_freshly_allocated_object_ids() {
        // Stand in for a document whose objects 1..=9 already exist, some of
        // them fonts. The emitter is told the first FREE number is 10.
        let pre_existing: Vec<ObjId> = (1u32..=9).map(|n| ObjId::new(n, 0)).collect();
        let first_free = 10u32;

        let out = build_objects(&plan(), first_free, Object::Null).expect("plan is valid");
        let written = out.written_ids();

        for id in &written {
            assert!(
                !pre_existing.contains(id),
                "R107 VIOLATION: font embedding wrote to {id:?}, which already exists in the \
                 document. FF-C must only ever ADD font resources — rewriting an existing font \
                 object breaks the round-trip invariant (ARCHITECTURE.md §5) for a file pdfcer \
                 did not author, and would put this feature in the forced-full-rewrite family it \
                 was designed to stay out of."
            );
        }

        // And the positive half: it really did allocate from `first_free`,
        // rather than (say) returning an empty set, which would satisfy the
        // loop above vacuously.
        assert_eq!(written.len(), 5, "expected exactly five new objects");
        assert_eq!(
            written,
            (10u32..15).map(|n| ObjId::new(n, 0)).collect::<Vec<_>>(),
            "objects must be consecutive from the first free number"
        );
        assert_eq!(out.font_dict_id, ObjId::new(10, 0));
    }

    #[test]
    fn cff_donors_are_refused_by_name_not_silently_mis_emitted() {
        let mut p = plan();
        p.outline_kind = OutlineKind::Cff;
        let err = build_objects(&p, 10, Object::Null).unwrap_err();
        assert_eq!(
            err,
            FontEmbedError::OutlineKindUnsupported {
                kind: OutlineKind::Cff
            }
        );
        // The message has to tell the operator what to do instead — a
        // refusal that only says "no" is a dead end (R27).
        let text = err.to_string();
        assert!(
            text.contains("TrueType"),
            "refusal must name what IS supported: {text}"
        );
    }

    /// The refusal must be REACHABLE (R96). A guard placed behind a filter
    /// its case cannot pass is dead code that looks live, so this asserts
    /// the gate actually fires rather than that the happy path works.
    #[test]
    fn malformed_subset_tag_is_refused() {
        for bad in ["abcdef", "ABCDE", "ABCDEFG", "ABC1EF", ""] {
            let mut p = plan();
            p.subset_tag = bad.to_owned();
            assert!(
                build_objects(&p, 10, Object::Null).is_err(),
                "subset tag {bad:?} should have been refused — §9.6.4 requires exactly six \
                 uppercase ASCII letters, and a malformed tag makes the font unrecognisable as \
                 a subset to every consumer, including pdfcer's own is_subset_tag"
            );
        }
        // Positive control: the valid tag must still pass, or the test above
        // proves only that the function rejects everything.
        assert!(build_objects(&plan(), 10, Object::Null).is_ok());
    }

    #[test]
    fn empty_subset_and_empty_program_are_refused() {
        let mut p = plan();
        p.glyphs.clear();
        assert_eq!(
            build_objects(&p, 10, Object::Null).unwrap_err(),
            FontEmbedError::EmptySubset
        );

        let mut p = plan();
        p.program.clear();
        assert_eq!(
            build_objects(&p, 10, Object::Null).unwrap_err(),
            FontEmbedError::EmptyProgram
        );
    }

    #[test]
    fn object_number_exhaustion_is_refused_rather_than_wrapping() {
        let err = build_objects(&plan(), u32::MAX - 2, Object::Null).unwrap_err();
        assert_eq!(err, FontEmbedError::ObjectNumbersExhausted);
    }

    #[test]
    fn tagged_name_inserts_exactly_one_plus() {
        assert_eq!(plan().tagged_name(), "ABCDEF+NotoSansJP");
    }

    /// Consecutive CIDs share one run; a gap must start a new one, or every
    /// width after the gap is attributed to the wrong glyph.
    #[test]
    fn widths_array_breaks_runs_at_cid_gaps() {
        let glyphs = vec![
            SubsetGlyph {
                cid: 1,
                width: 100,
                unicode: 'a',
            },
            SubsetGlyph {
                cid: 2,
                width: 200,
                unicode: 'b',
            },
            SubsetGlyph {
                cid: 7,
                width: 300,
                unicode: 'c',
            },
        ];
        let w = widths_array(&glyphs);
        assert_eq!(
            w,
            vec![
                Object::Integer(1),
                Object::Array(vec![Object::Integer(100), Object::Integer(200)]),
                Object::Integer(7),
                Object::Array(vec![Object::Integer(300)]),
            ]
        );
    }

    /// A CMap that maps two CIDs to one scalar is not invertible, which is
    /// what standing rule R110 turns on. pdfcer-authored CMaps are injective
    /// by construction — and are checked anyway, because authorship is a
    /// claim and the check is evidence (R93).
    #[test]
    fn authored_to_unicode_is_injective() {
        let Object::String(bytes) = to_unicode_cmap(&plan().glyphs) else {
            panic!("expected a string object");
        };
        let text = String::from_utf8(bytes).expect("CMap is ASCII");
        assert!(text.contains("<0001> <0041>"), "CID 1 -> 'A':\n{text}");
        assert!(text.contains("<0002> <0042>"), "CID 2 -> 'B':\n{text}");
        assert!(
            text.contains("2 beginbfchar"),
            "entry count must match:\n{text}"
        );

        let targets: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with('<') && l.contains("> <"))
            .filter(|l| !l.contains("FFFF"))
            .map(|l| l.split("> <").nth(1).unwrap_or(""))
            .collect();
        let mut uniq = targets.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(
            targets.len(),
            uniq.len(),
            "two CIDs map to the same scalar — the CMap is not injective, so it cannot be \
             inverted and R110 would refuse to edit this run"
        );
    }

    /// Supplementary-plane characters need a surrogate PAIR in the CMap
    /// (§9.10.3). Emitting a single unit would silently truncate every emoji
    /// and every rarer CJK ideograph to a lone surrogate.
    #[test]
    fn supplementary_plane_characters_emit_a_surrogate_pair() {
        let glyphs = vec![SubsetGlyph {
            cid: 1,
            width: 1000,
            unicode: '\u{20B9F}',
        }];
        let Object::String(bytes) = to_unicode_cmap(&glyphs) else {
            panic!("expected a string object");
        };
        let text = String::from_utf8(bytes).expect("CMap is ASCII");
        assert!(
            text.contains("<0001> <D842DF9F>"),
            "U+20B9F must appear as the surrogate pair D842 DF9F:\n{text}"
        );
    }
}
