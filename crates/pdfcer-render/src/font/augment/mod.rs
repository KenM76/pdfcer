//! Appending glyphs from an installed face to an embedded TrueType subset
//! (decision 173 §4–§5, rule R260). The result is a new program in which
//! every existing glyph keeps its `glyf` record, `hmtx` entry and cmap
//! mappings; the input bytes are never modified.

mod cmap;
mod glyf;
mod identity;
mod installed;
mod metrics;
mod post;
mod verify;

use std::collections::HashMap;

use crate::font::sfnt::{Directory, assemble, read_u16};

use self::metrics::{GlyphLimits, HMetric};
use self::post::PostName;

pub use self::installed::InstalledFaceAugmenter;

/// Why a subset could not be augmented (decision 173 §4–§5).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AugmentError {
    /// The subset or the face is not a well-formed TrueType program.
    #[error("the font program is malformed: {detail}")]
    MalformedFace {
        /// What was wrong.
        detail: String,
    },
    /// The subset carries a table whose meaning appending would break.
    #[error("the embedded font carries a {tag} table, which cannot be extended safely")]
    UnsupportedTable {
        /// The table tag.
        tag: String,
    },
    /// The subset's `post` is a version other than 2.0 or 3.0.
    #[error("the embedded font's post table is a version that cannot gain glyph names")]
    UnsupportedPostFormat,
    /// A Unicode `cmap` subtable is in a format other than 0, 4, 6 or 12.
    #[error("the embedded font's cmap has a format-{format} subtable, which cannot be extended")]
    UnsupportedCmapFormat {
        /// The subtable format.
        format: u16,
    },
    /// The subset has no `(3,1)` Unicode cmap.
    #[error("the embedded font has no Windows Unicode cmap")]
    MissingUnicodeCmap,
    /// The face does not map the character in `(3,1)` or `(3,10)` (I7).
    #[error("the installed font has no glyph for U+{:04X}", u32::from(*ch))]
    FaceLacksCharacter {
        /// The character.
        ch: char,
    },
    /// `fpgm`, `prep` or `cvt ` differ and the policy is to refuse (§5).
    #[error("the installed font's hinting programs differ from the embedded font's")]
    HintingDiffers,
    /// The result would exceed 65,535 glyphs.
    #[error("the extended font would exceed 65,535 glyphs")]
    ProgramTooLarge,
    /// The face is not a static TrueType-outline font (I2).
    #[error("the installed font is not a static TrueType-outline font")]
    FaceNotTrueTypeOutlines,
    /// The face's or the subset's `fsType` forbids the embed (I3, R109).
    #[error("the font's licence does not permit this: {reason}")]
    EmbeddingNotPermitted {
        /// The R109 refusal.
        reason: String,
    },
    /// `head.unitsPerEm` differs (I4).
    #[error("the installed font has a different design grid from the embedded font")]
    UnitsPerEmMismatch,
    /// A shared glyph's outline differs (I5).
    #[error("the installed font draws U+{:04X} differently from the embedded font (glyph {gid})", u32::from(*ch))]
    OutlineMismatch {
        /// The character compared.
        ch: char,
        /// The subset's glyph id.
        gid: u16,
    },
    /// A shared glyph's advance differs (I5).
    #[error("the installed font spaces U+{:04X} differently from the embedded font (glyph {gid})", u32::from(*ch))]
    AdvanceMismatch {
        /// The character compared.
        ch: char,
        /// The subset's glyph id.
        gid: u16,
    },
    /// No non-empty outline could be compared (I6).
    #[error("no glyph the two fonts share could be compared, so they cannot be shown to match")]
    IdentityUnproven,
    /// The re-parsed result failed R260; a defect in pdfcer, never skipped.
    #[error("the extended font failed verification: {detail}")]
    VerificationFailed {
        /// The failed check.
        detail: String,
    },
}

/// What to do when the hinting programs differ (decision 173 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Hinting {
    /// Append the glyphs without their instructions.
    #[default]
    Strip,
    /// Refuse with [`AugmentError::HintingDiffers`].
    Refuse,
}

/// One glyph appended for a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddedGlyph {
    pub(crate) ch: char,
    pub(crate) gid: u16,
    pub(crate) advance: u16,
}

/// An augmented program and what core needs to describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Augmented {
    pub(crate) program: Vec<u8>,
    /// One entry per requested character the subset did not already draw.
    pub(crate) added: Vec<AddedGlyph>,
    /// Instructions were removed from the appended glyphs (§5 `Strip`).
    pub(crate) instructions_stripped: bool,
    pub(crate) units_per_em: u16,
    /// `head`'s bounding box after the union, font units.
    pub(crate) bbox: [i16; 4],
}

const COPIED: [&[u8; 4]; 12] = [
    b"OS/2", b"name", b"cvt ", b"fpgm", b"prep", b"gasp", b"kern", b"GDEF", b"GSUB", b"GPOS",
    b"VDMX", b"PCLT",
];
const REWRITTEN: [&[u8; 4]; 8] = [
    b"glyf", b"loca", b"hmtx", b"hhea", b"maxp", b"cmap", b"post", b"head",
];
const DROPPED: [&[u8; 4]; 5] = [b"hdmx", b"LTSH", b"vhea", b"vmtx", b"DSIG"];

/// The tables of one program that surgery reads.
struct Parts<'a> {
    dir: Directory<'a>,
    glyf: &'a [u8],
    loca: Vec<usize>,
    long_loca: bool,
    metrics: Vec<HMetric>,
    table: HashMap<[u8; 4], &'a [u8]>,
}

impl<'a> Parts<'a> {
    fn read(data: &'a [u8], index: u32, who: &str) -> Result<Self, AugmentError> {
        let bad = |what: &str| AugmentError::MalformedFace {
            detail: format!("the {who}'s {what}"),
        };
        let dir = Directory::parse_face(data, index)
            .ok_or_else(|| bad("table directory is unreadable"))?;
        if dir.flavor != 0x0001_0000 && dir.flavor != 0x7472_7565 {
            return Err(bad("outlines are not TrueType"));
        }
        let mut table = HashMap::new();
        for tag in REWRITTEN {
            let t = dir
                .table(*tag)
                .ok_or_else(|| bad(&format!("{} table is missing", tag.escape_ascii())))?;
            table.insert(*tag, t);
        }
        let t = |tag: &[u8; 4]| table.get(tag).copied().unwrap_or(&[]);
        let n = usize::from(read_u16(t(b"maxp"), 4).ok_or_else(|| bad("maxp is truncated"))?);
        let long_loca = read_u16(t(b"head"), 50).ok_or_else(|| bad("head is truncated"))? == 1;
        let loca =
            glyf::read_loca(t(b"loca"), long_loca, n).ok_or_else(|| bad("loca is truncated"))?;
        let long_count =
            usize::from(read_u16(t(b"hhea"), 34).ok_or_else(|| bad("hhea is truncated"))?);
        let metrics = metrics::read_hmtx(t(b"hmtx"), long_count.min(n), n)
            .ok_or_else(|| bad("hmtx is truncated"))?;
        Ok(Self {
            glyf: t(b"glyf"),
            dir,
            loca,
            long_loca,
            metrics,
            table,
        })
    }

    /// A rewritten-set table; `read` refused any program lacking one.
    fn get(&self, tag: &[u8; 4]) -> &'a [u8] {
        self.table.get(tag).copied().unwrap_or(&[])
    }

    /// Glyph `gid`'s metric; `closure` has bounded every gid by `loca`.
    fn metric(&self, gid: u16) -> HMetric {
        self.metrics
            .get(usize::from(gid))
            .copied()
            .unwrap_or((0, 0))
    }

    fn drawn(&self, gid: u16) -> bool {
        glyf::record(self.glyf, &self.loca, usize::from(gid)).is_some_and(|r| !r.is_empty())
    }
}

/// Append to `subset` the face glyphs for each of `chars` it does not
/// already draw. A character the subset maps to an empty slot is remapped to
/// the appended glyph. `face_index` selects a collection member.
pub(crate) fn append_glyphs(
    subset: &[u8],
    face: &[u8],
    face_index: u32,
    chars: &[char],
    hinting: Hinting,
) -> Result<Augmented, AugmentError> {
    let s = Parts::read(subset, 0, "embedded font")?;
    if let Some((tag, _)) = s
        .dir
        .tables
        .iter()
        .find(|(t, _)| ![&COPIED[..], &REWRITTEN, &DROPPED].concat().contains(&t))
    {
        return Err(AugmentError::UnsupportedTable {
            tag: tag.escape_ascii().to_string(),
        });
    }
    let f = Parts::read(face, face_index, "installed font")?;
    let mut roots: Vec<(char, u16)> = Vec::new();
    for &ch in chars {
        let held = cmap::unicode_glyph(s.get(b"cmap"), ch).is_some_and(|g| s.drawn(g));
        if held || roots.iter().any(|r| r.0 == ch) {
            continue;
        }
        let g = cmap::unicode_glyph(f.get(b"cmap"), ch)
            .ok_or(AugmentError::FaceLacksCharacter { ch })?;
        roots.push((ch, g));
    }
    let strip = hinting_differs(&s, &f) && {
        if hinting == Hinting::Refuse {
            return Err(AugmentError::HintingDiffers);
        }
        true
    };
    let root_gids: Vec<u16> = roots.iter().map(|r| r.1).collect();
    let order = glyf::closure(f.glyf, &f.loca, &root_gids)?;
    let old_n = s.metrics.len();
    if old_n + order.len() > usize::from(u16::MAX) {
        return Err(AugmentError::ProgramTooLarge);
    }
    let remap: HashMap<u16, u16> = order
        .iter()
        .enumerate()
        .map(|(i, &g)| (g, u16::try_from(old_n + i).unwrap_or(u16::MAX)))
        .collect();
    let added: Vec<AddedGlyph> = roots
        .iter()
        .map(|&(ch, g)| AddedGlyph {
            ch,
            gid: remap.get(&g).copied().unwrap_or(0),
            advance: f.metric(g).0,
        })
        .collect();
    let program = build(&s, &f, &order, &remap, &added, strip)?;
    verify::check(
        subset,
        &program,
        face,
        face_index,
        &added,
        old_n + order.len(),
    )?;
    let out = Directory::parse(&program)
        .and_then(|d| d.table(*b"head"))
        .unwrap_or(&[]);
    Ok(Augmented {
        units_per_em: read_u16(out, 18).unwrap_or(1000),
        bbox: [36, 38, 40, 42].map(|at| crate::font::sfnt::read_i16(out, at)),
        program,
        added,
        instructions_stripped: strip
            && order
                .iter()
                .any(|&g| glyf::instruction_len(record(&f, g)) > 0),
    })
}

fn record<'a>(p: &Parts<'a>, gid: u16) -> &'a [u8] {
    glyf::record(p.glyf, &p.loca, usize::from(gid)).unwrap_or(&[])
}

/// §5: instructions travel only when `fpgm`, `prep` and `cvt ` are each
/// byte-equal (or each absent from both).
fn hinting_differs(s: &Parts<'_>, f: &Parts<'_>) -> bool {
    [b"fpgm", b"prep", b"cvt "]
        .iter()
        .any(|t| s.dir.table(**t) != f.dir.table(**t))
}

/// The augmented program's bytes.
fn build(
    s: &Parts<'_>,
    f: &Parts<'_>,
    order: &[u16],
    remap: &HashMap<u16, u16>,
    added: &[AddedGlyph],
    strip: bool,
) -> Result<Vec<u8>, AugmentError> {
    let records = order
        .iter()
        .map(|&g| glyf::rewrite(record(f, g), remap, strip))
        .collect::<Result<Vec<_>, _>>()?;
    let (glyf_bytes, offsets) = glyf::append(s.glyf, &s.loca, &records);
    let (loca, long) = glyf::write_loca(&offsets, s.long_loca);
    let mut hm = s.metrics.clone();
    hm.extend(order.iter().map(|&g| f.metric(g)));
    let boxes: Vec<_> = (0..hm.len())
        .map(|g| glyf::record(&glyf_bytes, &offsets, g).and_then(metrics::bbox))
        .collect();
    let limits = order
        .iter()
        .map(|&g| metrics::limits(f.glyf, &f.loca, g))
        .collect::<Result<Vec<GlyphLimits>, _>>()?;
    let num = u16::try_from(hm.len()).map_err(|_| AugmentError::ProgramTooLarge)?;
    let face_maxp = (!strip).then(|| f.get(b"maxp"));
    let names = post_names(s, f, order, added);
    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"glyf", glyf_bytes),
        (*b"loca", loca),
        (*b"hmtx", metrics::write_hmtx(&hm)),
        (*b"hhea", metrics::write_hhea(s.get(b"hhea"), &hm, &boxes)),
        (
            *b"maxp",
            metrics::write_maxp(s.get(b"maxp"), num, &limits, face_maxp),
        ),
        (
            *b"cmap",
            cmap::add_entries(
                s.get(b"cmap"),
                &added.iter().map(|a| (a.ch, a.gid)).collect::<Vec<_>>(),
            )?,
        ),
        (*b"post", post::append_names(s.get(b"post"), &names)?),
        (
            *b"head",
            metrics::write_head(
                s.get(b"head"),
                boxes.get(s.metrics.len()..).unwrap_or(&[]),
                long,
            ),
        ),
    ];
    tables.extend(
        s.dir
            .tables
            .iter()
            .filter(|(t, _)| COPIED.contains(&t))
            .map(|(t, d)| (*t, d.to_vec())),
    );
    Ok(assemble(s.dir.flavor, tables))
}

/// Each appended glyph's `post` name: the face's own when its `post` is 2.0
/// and the name is not already taken in the subset (an empty slot keeps
/// its name), else `uniXXXX` for a character's glyph and `glyphN` for a
/// component, so a name lookup never lands on two glyphs.
fn post_names(s: &Parts<'_>, f: &Parts<'_>, order: &[u16], added: &[AddedGlyph]) -> Vec<PostName> {
    let old_n = s.metrics.len();
    let mut taken: Vec<PostName> = (0..old_n)
        .filter_map(|g| post::face_name(s.get(b"post"), u16::try_from(g).ok()?))
        .collect();
    let mut out = Vec::with_capacity(order.len());
    for (&g, new) in order.iter().zip(old_n..) {
        let fallback = match added
            .iter()
            .find(|a| usize::from(a.gid) == new)
            .map(|a| u32::from(a.ch))
        {
            Some(c) if c > 0xFFFF => format!("u{c:X}"),
            Some(c) => format!("uni{c:04X}"),
            None => format!("glyph{new}"),
        };
        let name = match post::face_name(f.get(b"post"), g) {
            Some(n) if !taken.contains(&n) => n,
            _ if taken.contains(&PostName::Custom(fallback.clone())) => {
                PostName::Custom(format!("{fallback}.{new}"))
            }
            _ => PostName::Custom(fallback),
        };
        taken.push(name.clone());
        out.push(name);
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
