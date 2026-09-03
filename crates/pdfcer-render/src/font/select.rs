//! # Substitute-face selection (decision 004 §4.3; §9.6.2.2, §9.8.1)
//!
//! Maps a document font WITHOUT a usable embedded program to a
//! [`FallbackKey`]: first by `BaseFont` name (exact standard-14 names,
//! subset-tag-stripped forms, and the real-world aliases producers
//! write — see `C:\personal_rag\pdf\` std-14 name-aliasing lesson),
//! then by `FontDescriptor` classification (Table 123 `Flags` bits:
//! FixedPitch bit 1, Serif bit 2, Italic bit 7; plus `ItalicAngle`
//! and the bold-ness signals). Every substitution the interpreter
//! makes through this module is DISCLOSED via diagnostics (R20).

use super::FallbackKey;

/// Strip a subset tag (`ABCDEF+`) if present: six uppercase ASCII
/// letters + `+` (§9.6.4 subset-naming convention).
#[must_use]
pub fn strip_subset_tag(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// Select a fallback slot from a `BaseFont` name, if the name is a
/// recognized standard-14 name or a known real-world alias.
#[must_use]
pub fn by_name(base_font: &str) -> Option<FallbackKey> {
    use FallbackKey as K;
    let name = strip_subset_tag(base_font);
    // Normalize the family/style split: producers write commas or
    // hyphens ("Arial,Bold" / "Arial-BoldMT" / "Helvetica-Bold").
    let lower = name.to_ascii_lowercase();
    let l = lower.as_str();

    let bold = l.contains("bold");
    let italic = l.contains("italic") || l.contains("oblique");

    // Family detection: exact std-14 names, their aliases (Helv, Arial,
    // ArialMT, TimesNewRoman, CourierNew, ZaDb — pure convention, no
    // spec basis), and the Windows metric equivalents.
    let family = if l.starts_with("helvetica") || l.starts_with("arial") || l == "helv" {
        Some(0) // sans
    } else if l.starts_with("times") {
        Some(1) // serif
    } else if l.starts_with("courier") {
        Some(2) // fixed
    } else if l == "symbol" {
        return Some(K::Symbol);
    } else if l.starts_with("zapfdingbats") || l == "zadb" {
        return Some(K::Dingbats);
    } else {
        None
    }?;

    Some(match (family, bold, italic) {
        (0, false, false) => K::Sans,
        (0, true, false) => K::SansBold,
        (0, false, true) => K::SansItalic,
        (0, true, true) => K::SansBoldItalic,
        (1, false, false) => K::Serif,
        (1, true, false) => K::SerifBold,
        (1, false, true) => K::SerifItalic,
        (1, true, true) => K::SerifBoldItalic,
        (2, false, false) => K::Fixed,
        (2, true, false) => K::FixedBold,
        (2, false, true) => K::FixedItalic,
        (_, true, true) => K::FixedBoldItalic,
        (_, true, false) => K::FixedBold,
        (_, false, true) => K::FixedItalic,
        (_, false, false) => K::Fixed,
    })
}

/// Real face names that satisfy each standard-14 slot, most-specific
/// first — the **reverse** of [`by_name`].
///
/// # Why the reverse map is a separate table rather than an inversion
///
/// [`by_name`] answers *"which slot does this document font mean?"* by
/// pattern-matching a name a producer wrote. It is deliberately loose
/// (prefix matches, `contains("bold")`), which is right for a lookup and
/// useless as an inversion: you cannot enumerate the names that would
/// match a prefix test.
///
/// Font **embedding** needs exactly that enumeration. A document says
/// `Helvetica`; an operator's font folder holds `arial.ttf`, registered
/// under the names it advertises (`Arial`, `ArialMT`, `Arial Regular`) and
/// its filename stem. Nothing connects the two without a table saying that
/// those names are the ones a Helvetica slot accepts.
///
/// # What is in here, and what is deliberately not
///
/// The families listed are the ones designed as **metric-compatible**
/// substitutes for the standard 14: Monotype's Arial / Times New Roman /
/// Courier New (the Windows set), Red Hat's Liberation set, and URW's
/// Nimbus set. DejaVu is included as a widely-installed last resort even
/// though it is not metric-compatible — which costs nothing here, because
/// the advances come from `/Widths` either way (decision 004 §3.6) and only
/// the letterforms change.
///
/// **`Symbol` and `ZapfDingbats` have no entries beyond their own names.**
/// A symbolic font's codes mean whatever its program says they mean, so a
/// family-resemblance stand-in draws a different repertoire rather than a
/// different style. Windows' `SymbolMT` is the one plausible candidate and
/// there is no way to verify from inside pdfcer that a given `SymbolMT` is
/// Adobe-Symbol-encoded, so it is left out. `pdfcer_core::font_embed_missing`
/// refuses an inferred donor for a symbolic font independently; this table
/// simply does not offer one.
///
/// PostScript-style names come first on purpose: they are unique per face,
/// whereas a `name`-table FAMILY string is shared by every weight (both
/// `arial.ttf` and `arialbd.ttf` advertise the family `Arial`, and a
/// `FontEnvironment` registration is last-wins). Matching `Arial-BoldMT`
/// before `Arial` is what keeps `Helvetica-Bold` from resolving to whatever
/// happened to be registered last.
///
/// # Examples
///
/// ```
/// use pdfcer_render::font::{FallbackKey, select};
///
/// assert!(select::candidate_names(FallbackKey::Sans).contains(&"ArialMT"));
/// assert!(select::candidate_names(FallbackKey::SansBold).contains(&"Arial-BoldMT"));
/// // A symbolic slot offers only its own name.
/// assert_eq!(select::candidate_names(FallbackKey::Symbol), ["Symbol"]);
/// ```
#[must_use]
pub const fn candidate_names(key: FallbackKey) -> &'static [&'static str] {
    use FallbackKey as K;
    match key {
        K::Sans => &[
            "Helvetica",
            "ArialMT",
            "Arial",
            "LiberationSans",
            "LiberationSans-Regular",
            "Liberation Sans",
            "NimbusSans-Regular",
            "DejaVuSans",
            "DejaVu Sans",
        ],
        K::SansBold => &[
            "Helvetica-Bold",
            "Arial-BoldMT",
            "Arial Bold",
            "LiberationSans-Bold",
            "Liberation Sans Bold",
            "NimbusSans-Bold",
            "DejaVuSans-Bold",
            "DejaVu Sans Bold",
        ],
        K::SansItalic => &[
            "Helvetica-Oblique",
            "Arial-ItalicMT",
            "Arial Italic",
            "LiberationSans-Italic",
            "Liberation Sans Italic",
            "NimbusSans-Italic",
            "DejaVuSans-Oblique",
        ],
        K::SansBoldItalic => &[
            "Helvetica-BoldOblique",
            "Arial-BoldItalicMT",
            "Arial Bold Italic",
            "LiberationSans-BoldItalic",
            "Liberation Sans Bold Italic",
            "NimbusSans-BoldItalic",
            "DejaVuSans-BoldOblique",
        ],
        K::Serif => &[
            "Times-Roman",
            "TimesNewRomanPSMT",
            "Times New Roman",
            "LiberationSerif",
            "LiberationSerif-Regular",
            "Liberation Serif",
            "NimbusRoman-Regular",
            "DejaVuSerif",
        ],
        K::SerifBold => &[
            "Times-Bold",
            "TimesNewRomanPS-BoldMT",
            "Times New Roman Bold",
            "LiberationSerif-Bold",
            "Liberation Serif Bold",
            "NimbusRoman-Bold",
            "DejaVuSerif-Bold",
        ],
        K::SerifItalic => &[
            "Times-Italic",
            "TimesNewRomanPS-ItalicMT",
            "Times New Roman Italic",
            "LiberationSerif-Italic",
            "Liberation Serif Italic",
            "NimbusRoman-Italic",
            "DejaVuSerif-Italic",
        ],
        K::SerifBoldItalic => &[
            "Times-BoldItalic",
            "TimesNewRomanPS-BoldItalicMT",
            "Times New Roman Bold Italic",
            "LiberationSerif-BoldItalic",
            "Liberation Serif Bold Italic",
            "NimbusRoman-BoldItalic",
            "DejaVuSerif-BoldItalic",
        ],
        K::Fixed => &[
            "Courier",
            "CourierNewPSMT",
            "Courier New",
            "LiberationMono",
            "LiberationMono-Regular",
            "Liberation Mono",
            "NimbusMonoPS-Regular",
            "DejaVuSansMono",
        ],
        K::FixedBold => &[
            "Courier-Bold",
            "CourierNewPS-BoldMT",
            "Courier New Bold",
            "LiberationMono-Bold",
            "Liberation Mono Bold",
            "NimbusMonoPS-Bold",
            "DejaVuSansMono-Bold",
        ],
        K::FixedItalic => &[
            "Courier-Oblique",
            "CourierNewPS-ItalicMT",
            "Courier New Italic",
            "LiberationMono-Italic",
            "Liberation Mono Italic",
            "NimbusMonoPS-Italic",
            "DejaVuSansMono-Oblique",
        ],
        K::FixedBoldItalic => &[
            "Courier-BoldOblique",
            "CourierNewPS-BoldItalicMT",
            "Courier New Bold Italic",
            "LiberationMono-BoldItalic",
            "Liberation Mono Bold Italic",
            "NimbusMonoPS-BoldItalic",
            "DejaVuSansMono-BoldOblique",
        ],
        // See this function's docs: no stand-in is offered for either
        // symbolic face.
        K::Symbol => &["Symbol"],
        K::Dingbats => &["ZapfDingbats"],
    }
}

/// Classify from `FontDescriptor` signals when the name matched
/// nothing (Table 123: Flags bit 1 = FixedPitch, bit 2 = Serif,
/// bit 7 = Italic; `ItalicAngle` non-zero ⇒ italic; StemV ≥ 140 or a
/// ForceBold-ish weight ⇒ bold is approximated by the caller passing
/// `bold`).
#[must_use]
pub fn by_descriptor(flags: u32, italic_angle: f64, bold: bool) -> FallbackKey {
    use FallbackKey as K;
    let fixed = flags & (1 << 0) != 0;
    let serif = flags & (1 << 1) != 0;
    let italic = flags & (1 << 6) != 0 || italic_angle != 0.0;
    match (fixed, serif, bold, italic) {
        (true, _, false, false) => K::Fixed,
        (true, _, true, false) => K::FixedBold,
        (true, _, false, true) => K::FixedItalic,
        (true, _, true, true) => K::FixedBoldItalic,
        (false, true, false, false) => K::Serif,
        (false, true, true, false) => K::SerifBold,
        (false, true, false, true) => K::SerifItalic,
        (false, true, true, true) => K::SerifBoldItalic,
        (false, false, false, false) => K::Sans,
        (false, false, true, false) => K::SansBold,
        (false, false, false, true) => K::SansItalic,
        (false, false, true, true) => K::SansBoldItalic,
    }
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
    use FallbackKey as K;

    #[test]
    fn exact_std14_names() {
        assert_eq!(by_name("Helvetica"), Some(K::Sans));
        assert_eq!(by_name("Helvetica-BoldOblique"), Some(K::SansBoldItalic));
        assert_eq!(by_name("Times-Roman"), Some(K::Serif));
        assert_eq!(by_name("Times-BoldItalic"), Some(K::SerifBoldItalic));
        assert_eq!(by_name("Courier-Oblique"), Some(K::FixedItalic));
        assert_eq!(by_name("Symbol"), Some(K::Symbol));
        assert_eq!(by_name("ZapfDingbats"), Some(K::Dingbats));
    }

    #[test]
    fn real_world_aliases() {
        // The personal_rag std-14 aliasing lesson: no spec basis, pure
        // producer convention.
        assert_eq!(by_name("Helv"), Some(K::Sans));
        assert_eq!(by_name("Arial"), Some(K::Sans));
        assert_eq!(by_name("ArialMT"), Some(K::Sans));
        assert_eq!(by_name("Arial-BoldMT"), Some(K::SansBold));
        assert_eq!(by_name("TimesNewRoman"), Some(K::Serif));
        assert_eq!(by_name("TimesNewRomanPS-ItalicMT"), Some(K::SerifItalic));
        assert_eq!(by_name("CourierNew"), Some(K::Fixed));
        assert_eq!(by_name("ZaDb"), Some(K::Dingbats));
    }

    #[test]
    fn subset_tags_stripped() {
        assert_eq!(by_name("ABCDEF+Helvetica-Bold"), Some(K::SansBold));
        assert_eq!(strip_subset_tag("ABCDEF+Foo"), "Foo");
        // Not a subset tag: wrong length / case.
        assert_eq!(strip_subset_tag("ABC+Foo"), "ABC+Foo");
    }

    #[test]
    fn unknown_names_fall_to_descriptor() {
        assert_eq!(by_name("CompletelyCustomFont"), None);
        assert_eq!(by_descriptor(0b10, 0.0, false), K::Serif);
        assert_eq!(by_descriptor(0b01, 0.0, true), K::FixedBold);
        assert_eq!(by_descriptor(0, -12.0, false), K::SansItalic);
    }
}
