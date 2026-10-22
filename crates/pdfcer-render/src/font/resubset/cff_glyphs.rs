//! What a CFF cut learns by evaluating charstrings: the parts of each kept
//! glyph drawn with `seac` (an `endchar` carrying a base and an accent,
//! Adobe TN5177 Appendix C), and the check that every kept glyph draws the
//! same after the cut.

use std::cell::RefCell;
use std::collections::BTreeSet;

use pdfcer_core::optimize::resubset::ResubsetRefusal;
use skrifa::raw::ps::cff::CffFontRef;
use skrifa::raw::ps::cff::index::Index;
use skrifa::raw::ps::cs::{self, CharstringContext, CharstringKind, CommandSink};
use skrifa::raw::ps::error::Error;
use skrifa::raw::types::{Fixed, GlyphId};

use super::malformed;
use crate::font::coredata::{self, BaseEncoding};
use crate::font::program::FontProgram;

/// The glyphs a cut keeps: glyph 0, `roots` below `total`, and the base and
/// accent of every kept glyph drawn with `seac`. A part cannot itself use
/// `seac` (TN5177 forbids nesting it), so one pass is the whole closure.
///
/// # Errors
///
/// `malformed-glyf` when a kept glyph does not evaluate.
pub(super) fn closure(
    cff: &[u8],
    roots: &BTreeSet<u16>,
    total: usize,
) -> Result<BTreeSet<u16>, ResubsetRefusal> {
    let font = CffFontRef::new_cff(cff, 0, None).map_err(|_| malformed())?;
    let program = FontProgram::Cff(font.clone());
    let mut keep: BTreeSet<u16> = std::iter::once(0)
        .chain(roots.iter().copied())
        .filter(|&g| usize::from(g) < total)
        .collect();
    let mut parts = Vec::new();
    for &gid in &keep {
        let probe = Probe {
            font: &font,
            program: &program,
            subrs: local_subrs(&font, gid).map_err(|_| malformed())?,
            parts: RefCell::new(Vec::new()),
        };
        let charstring = font
            .charstrings()
            .get(usize::from(gid))
            .map_err(|_| malformed())?;
        cs::evaluate(&probe, None, charstring, &mut Silent).map_err(|_| malformed())?;
        parts.extend(probe.parts.into_inner());
    }
    keep.extend(parts);
    Ok(keep)
}

/// `new`, the cut of `old`, parses with the same glyph count, glyph names,
/// built-in encoding and subfont count, and every glyph in `keep` draws the
/// same path with the same advance.
///
/// # Errors
///
/// `malformed-glyf` when any of it differs.
pub(super) fn verify(old: &[u8], new: &[u8], keep: &BTreeSet<u16>) -> Result<(), ResubsetRefusal> {
    let parse = |d| CffFontRef::new_cff(d, 0, None).map_err(|_| malformed());
    let (a, b) = (parse(old)?, parse(new)?);
    let (pa, pb) = (FontProgram::Cff(a.clone()), FontProgram::Cff(b.clone()));
    let same_shape = a.num_glyphs() == b.num_glyphs()
        && a.num_subfonts() == b.num_subfonts()
        && a.is_cid() == b.is_cid()
        && (0..=255u8).all(|c| pa.glyph_for_builtin_code(c) == pb.glyph_for_builtin_code(c))
        && names(&a) == names(&b);
    if !same_shape {
        return Err(malformed());
    }
    for &gid in keep {
        if drawn(&a, gid).ok_or_else(malformed)? != drawn(&b, gid).ok_or_else(malformed)? {
            return Err(malformed());
        }
    }
    Ok(())
}

/// Each glyph's string ID in the charset (its name, or its CID).
fn names(font: &CffFontRef<'_>) -> Vec<u16> {
    font.charset()
        .map(|c| c.iter().map(|(_, sid)| sid.to_u16()).collect())
        .unwrap_or_default()
}

/// Glyph `gid`'s path commands, as raw 16.16 values, and its advance.
fn drawn(font: &CffFontRef<'_>, gid: u16) -> Option<(Vec<i32>, Option<i32>)> {
    let gid = GlyphId::new(gid.into());
    let subfont = font.subfont(font.subfont_index(gid)?, &[]).ok()?;
    let mut sink = Recorder::default();
    let width = font
        .evaluate_charstring(&subfont, gid, &[], &mut sink)
        .ok()?;
    Some((sink.0, width.map(Fixed::to_bits)))
}

/// The local Subrs INDEX of `gid`'s subfont; empty when it has none.
fn local_subrs<'a>(font: &CffFontRef<'a>, gid: u16) -> Result<Index<'a>, Error> {
    let gid = GlyphId::new(gid.into());
    let subfont = font.subfont(font.subfont_index(gid).ok_or(Error::MissingFdArray)?, &[])?;
    match usize::try_from(subfont.subrs_offset()) {
        Ok(0) => Ok(Index::Empty),
        Ok(at) => Index::new(
            font.data().get(at..).ok_or(Error::MissingSubroutines)?,
            false,
        ),
        Err(_) => Err(Error::MissingSubroutines),
    }
}

/// An evaluation context recording each `seac` base and accent.
struct Probe<'a, 'p> {
    font: &'p CffFontRef<'a>,
    program: &'p FontProgram<'a>,
    subrs: Index<'a>,
    parts: RefCell<Vec<u16>>,
}

impl Probe<'_, '_> {
    /// The glyph StandardEncoding `code` names (ISO 32000-2 Annex D.2), by
    /// that name in the charset, as the renderer resolves `seac`.
    fn part(&self, code: i32) -> Result<u16, Error> {
        let bad = || Error::InvalidSeacCode(code);
        let code8 = u8::try_from(code).map_err(|_| bad())?;
        let name = coredata::encoding_glyph_name(BaseEncoding::Standard, code8).ok_or_else(bad)?;
        let gid = self.program.glyph_for_name(name).ok_or_else(bad)?;
        u16::try_from(gid).map_err(|_| bad())
    }
}

impl CharstringContext for Probe<'_, '_> {
    fn kind(&self) -> CharstringKind {
        CharstringKind::Type2
    }

    fn seac_components(&self, base: i32, accent: i32) -> Result<[&[u8]; 2], Error> {
        let (b, a) = (self.part(base)?, self.part(accent)?);
        self.parts.borrow_mut().extend([b, a]);
        let charstrings = self.font.charstrings();
        Ok([
            charstrings.get(usize::from(b))?,
            charstrings.get(usize::from(a))?,
        ])
    }

    fn global_subr(&self, index: i32) -> Result<&[u8], Error> {
        let subrs = self.font.global_subrs();
        subrs.get(biased(index, subrs)?)
    }

    fn subr(&self, index: i32) -> Result<&[u8], Error> {
        self.subrs.get(biased(index, &self.subrs)?)
    }
}

/// A subroutine number as encoded in a charstring, plus the INDEX's bias
/// (TN5177 §4.7).
fn biased(index: i32, subrs: &Index<'_>) -> Result<usize, Error> {
    index
        .checked_add(subrs.subr_bias())
        .and_then(|i| usize::try_from(i).ok())
        .ok_or(Error::MissingSubroutines)
}

/// A sink that ignores the path.
struct Silent;

impl CommandSink for Silent {
    fn move_to(&mut self, _: Fixed, _: Fixed) {}
    fn line_to(&mut self, _: Fixed, _: Fixed) {}
    fn curve_to(&mut self, _: Fixed, _: Fixed, _: Fixed, _: Fixed, _: Fixed, _: Fixed) {}
    fn close(&mut self) {}
}

/// A sink keeping each command as a tag and its raw 16.16 values.
#[derive(Default)]
struct Recorder(Vec<i32>);

impl CommandSink for Recorder {
    fn move_to(&mut self, x: Fixed, y: Fixed) {
        self.0.extend([0, x.to_bits(), y.to_bits()]);
    }
    fn line_to(&mut self, x: Fixed, y: Fixed) {
        self.0.extend([1, x.to_bits(), y.to_bits()]);
    }
    fn curve_to(&mut self, cx0: Fixed, cy0: Fixed, cx1: Fixed, cy1: Fixed, x: Fixed, y: Fixed) {
        self.0.push(2);
        self.0
            .extend([cx0, cy0, cx1, cy1, x, y].map(Fixed::to_bits));
    }
    fn close(&mut self) {
        self.0.push(3);
    }
}
