//! Which glyphs a cut keeps: glyph 0 and every glyph a shown code could
//! resolve to.

use std::collections::{BTreeSet, HashMap};

use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Dict;
use pdfcer_core::optimize::resubset::{FontUse, ResubsetRefusal, ResubsetRequest};

use crate::font::FontEnvironment;
use crate::font::coredata::{self, BaseEncoding};
use crate::font::program::FontProgram;
use crate::text::{LoadedFont, load_with_program, simple_code_names};

/// The glyph IDs to keep before the composite closure: glyph 0 and, for
/// every code each user shows, every candidate below `numGlyphs`.
///
/// # Errors
///
/// `unusable-font` when a user's dictionary will not load over the
/// program; `unmapped-code` when a simple font shows a code no rung
/// resolves, since another reader's fallback could then pick any glyph.
pub(super) fn keep_set(
    request: &ResubsetRequest<'_, '_>,
    program: &FontProgram<'_>,
) -> Result<BTreeSet<u16>, ResubsetRefusal> {
    let total = program.num_glyphs();
    let mut keep = BTreeSet::from([0u16]);
    let env = FontEnvironment {
        fallbacks: HashMap::new(),
        named: HashMap::new(),
    };
    for user in request.users {
        let dict = request
            .view
            .value(user.font)
            .and_then(|o| o.as_dict())
            .ok_or_else(unusable)?;
        let loaded = load_with_program(request.view, dict, &env, request.program.to_vec(), None)
            .map_err(|_| unusable())?;
        let found = if is_type0(request, dict) {
            composite(&loaded, user)
        } else {
            simple(request, dict, &loaded, program, user)?
        };
        keep.extend(
            found
                .into_iter()
                .filter(|&g| g < total)
                .filter_map(|g| u16::try_from(g).ok()),
        );
    }
    Ok(keep)
}

fn is_type0(request: &ResubsetRequest<'_, '_>, dict: &Dict) -> bool {
    dict.get(b"Subtype")
        .map(|o| request.view.resolve(o))
        .and_then(|o| o.as_name())
        .is_some_and(|n| n.as_bytes() == b"Type0")
}

/// A composite font's CIDs through `/CIDToGIDMap`; a CID past the map's
/// end draws glyph 0, which is always kept.
fn composite(loaded: &LoadedFont, user: &FontUse<'_>) -> Vec<u32> {
    user.codes
        .iter()
        .filter_map(|&c| loaded.gid(c, None))
        .collect()
}

/// A simple font's candidates for each code: the renderer's own resolution
/// plus each §9.6.6.4 rung taken on its own.
fn simple(
    request: &ResubsetRequest<'_, '_>,
    dict: &Dict,
    loaded: &LoadedFont,
    program: &FontProgram<'_>,
    user: &FontUse<'_>,
) -> Result<Vec<u32>, ResubsetRefusal> {
    let names = simple_code_names(request.view, dict);
    let mac: HashMap<&'static str, u8> = (0..=255u8)
        .filter_map(|c| coredata::encoding_glyph_name(BaseEncoding::MacRoman, c).map(|n| (n, c)))
        .collect();
    let mut out = Vec::new();
    for &code in user.codes {
        let code8 = u8::try_from(code).map_err(|_| unmapped(code))?;
        let name = names.get(usize::from(code8)).and_then(Option::as_deref);
        let mut found: Vec<u32> = [
            loaded.gid(code, None),
            program.glyph_for_builtin_code(code8),
            program.glyph_for_mac_code(code8),
            program.glyph_for_char(char::from(code8)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let Some(n) = name {
            found.extend(
                coredata::glyph_name_to_unicode(n).and_then(|ch| program.glyph_for_char(ch)),
            );
            found.extend(mac.get(n).and_then(|&mc| program.glyph_for_mac_code(mc)));
            found.extend(program.glyph_for_name(n));
        }
        found.retain(|&g| g != 0);
        if found.is_empty() {
            return Err(unmapped(code));
        }
        out.extend(found);
    }
    Ok(out)
}

fn unusable() -> ResubsetRefusal {
    ResubsetRefusal::new(
        "unusable-font",
        "the font dictionary could not be read over its program",
    )
}

fn unmapped(code: u32) -> ResubsetRefusal {
    ResubsetRefusal::new(
        "unmapped-code",
        format!(
            "code {code} resolves to no glyph, so which glyph a reader draws for it is unknown"
        ),
    )
}
