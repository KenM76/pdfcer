//! A rich-text field's runs as styled spans for its appearance
//! (ISO 32000-1 §12.7.3.4; RT-M6, RT-M9, RT-M12).
//!
//! A run's own style always wins. Where it is silent and `/DS` and `/DA`
//! both set font, size or colour, which supplies it is undefined (RT-A6)
//! and is the [`RichStylePrecedence`] setting.
//!
//! A `font-family` resolves against the form's `/DR` fonts by family
//! (subset tag and style suffix removed, case-insensitive) in the run's
//! weight and slant, then the standard 14.

use super::{Align, Run, Style};
use crate::edit::{EditSession, decode_text_string};
use crate::field_display::DisplayNotes;
use crate::fontdata::{Std14, basefont_to_std14, std14_base_font_name, std14_styled};
use crate::forms::{Field, FieldFlags};
use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::text_edit::face_ladder::{family_key, strip_subset_tag};
use crate::vartext::{FontResource, Quadding, RichSpan, TextColor, parse_default_appearance};

/// Which of `/DS` and `/DA` supplies the font, size or colour of a
/// rich-text run that does not set it itself, when both could (RT-A6:
/// ISO 32000-1 states no rule). Weight, slant, decoration and alignment
/// have no `/DA` form and always come from `/DS`.
///
/// Settings key `rich_style_precedence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RichStylePrecedence {
    /// `/DS` first; `/DA` fills only what `/DS` leaves unset.
    #[default]
    RichStyleFirst,
    /// `/DA` first; `/DS` fills only what `/DA` leaves unset. A `/DA` size
    /// of 0 (auto-size) counts as unset.
    DefaultAppearanceFirst,
}

/// What [`crate::vartext::build_rich_text`] needs for one field.
#[derive(Debug, Clone)]
pub(crate) struct RichPlan {
    pub spans: Vec<RichSpan>,
    pub aligns: Vec<Quadding>,
    /// `fonts` plus any standard-14 face a run needed that `/DR` lacks.
    pub fonts: Vec<FontResource>,
}

/// The document-level inputs to [`plan`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct RichEnv<'a> {
    /// The `/DR` font resources the appearance may name.
    pub fonts: &'a [FontResource],
    /// The drawable `/DR` faces, from [`dr_faces`].
    pub faces: &'a [DrFace],
    pub precedence: RichStylePrecedence,
    /// Why the field's stored `/RV` stream would not decode, from
    /// [`crate::forms::rich_value_stream_error`]; noted when drawing from
    /// the stored value finds none.
    pub stored_rv_error: Option<&'a str>,
}

/// One drawable `/DR /Font` entry, keyed for family matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DrFace {
    name: Vec<u8>,
    family: String,
    /// The `/BaseFont` without its subset tag, for notes.
    base: String,
    bold: bool,
    italic: bool,
}

/// The rich value a redraw should use, when the caller is about to write
/// one: `Some(None)` = the field is becoming plain.
pub(crate) type RichOverride<'a> = Option<Option<&'a [u8]>>;

/// The form's `/DR` fonts a run may be drawn with: those whose `/BaseFont`
/// is standard-14 (drawn through the standard face) or that the appearance
/// binds in place (simple and bindable, or composite Identity-H). Any other
/// would draw a Helvetica stand-in under the font's name.
pub(crate) fn dr_faces(graph: &impl ObjectGraph, fonts: &[FontResource]) -> Vec<DrFace> {
    let Some(dr) = EditSession::dr_font_dict(graph) else {
        return Vec::new();
    };
    dr.iter()
        .filter_map(|(name, val)| {
            let fd = graph.resolve(val).as_dict()?;
            let raw = fd.get(b"BaseFont").and_then(Object::as_name)?.as_bytes();
            let res = fonts.iter().find(|f| f.name == name.as_bytes())?;
            let drawable = basefont_to_std14(raw).is_some()
                || res.cid.is_some()
                || EditSession::dr_font_is_bindable(graph, fd);
            drawable.then(|| DrFace::new(name.as_bytes().to_vec(), &String::from_utf8_lossy(raw)))
        })
        .collect()
}

impl DrFace {
    fn new(name: Vec<u8>, base_font: &str) -> Self {
        let base = strip_subset_tag(base_font).to_owned();
        let (family, style) = split_style(&base);
        let style = style.to_ascii_lowercase();
        Self {
            name,
            family: family_key(family),
            bold: style.contains("bold"),
            italic: style.contains("italic") || style.contains("oblique"),
            base,
        }
    }
}

/// `Courier-Bold`, `Arial,BoldItalic` and `ArialBold` as family and style.
fn split_style(base: &str) -> (&str, &str) {
    if let Some(i) = base.find(['-', ',']) {
        return base.split_at(i);
    }
    let lower = base.to_ascii_lowercase();
    ["bolditalic", "boldoblique", "bold", "italic", "oblique"]
        .iter()
        .find(|s| lower.len() > s.len() && lower.ends_with(*s))
        .map_or((base, ""), |s| base.split_at(base.len() - s.len()))
}

/// The styled spans for `field`, or `None` to draw it as plain text: not a
/// rich-text field, a password or comb field, no `/RV`, or an `/RV` that
/// will not parse or decode (recorded in `notes.rich_unreadable`).
pub(crate) fn plan(
    field: &Field,
    rv: RichOverride<'_>,
    da: &[u8],
    env: &RichEnv<'_>,
    notes: &mut DisplayNotes,
) -> Option<RichPlan> {
    let multiline = field.flags.has(FieldFlags::MULTILINE);
    if !field.is_rich_text()
        || field.flags.has(FieldFlags::PASSWORD)
        || crate::widget_draw::comb_cells(field, multiline).is_some()
    {
        return None;
    }
    let Some(rv) = rv.unwrap_or(field.rich_value.as_deref()) else {
        if rv.is_none() {
            notes.rich_unreadable = env.stored_rv_error.map(str::to_owned);
        }
        return None;
    };
    let rv = decode_text_string(rv).text;
    let ds = field
        .default_style
        .as_deref()
        .map(|d| decode_text_string(d).text);
    let parsed = match super::parse_noting(&rv, ds.as_deref()) {
        Ok(parsed) => parsed,
        Err(e) => {
            notes.rich_unreadable = Some(e.to_string());
            return None;
        }
    };
    // Each run's style without `/DS`, so `/DA` can be put in front of it.
    let da_first = env.precedence == RichStylePrecedence::DefaultAppearanceFirst;
    let own = (da_first && ds.is_some())
        .then(|| super::parse_noting(&rv, None).ok())
        .flatten()
        .map(|p| p.runs)
        .filter(|runs| runs.len() == parsed.runs.len());
    notes.rich_text = true;
    notes.rich_ignored = parsed.ignored;
    let ds_style = ds.as_deref().map(super::parse_style).unwrap_or_default();
    let da = parse_default_appearance(da).ok();
    let mut ctx = Resolve {
        fonts: env.fonts.to_vec(),
        faces: env.faces,
        da_font: da.as_ref().map(|d| d.font_name.clone()),
        ds_family: ds_style.family.clone(),
        da_first,
        notes,
    };
    let fill = Fill::new(da.as_ref(), &ds_style, da_first);
    let mut spans = Vec::with_capacity(parsed.runs.len());
    for (i, run) in parsed.runs.iter().enumerate() {
        let own = own.as_ref().and_then(|o| o.get(i)).map(|r| &r.style);
        spans.push(ctx.span(run, own, &fill));
    }
    Some(RichPlan {
        aligns: paragraph_aligns(&parsed.runs, field.quadding),
        spans,
        fonts: ctx.fonts,
    })
}

/// Size and colour for a run that sets neither.
struct Fill {
    /// Under `DefaultAppearanceFirst`, `/DA`'s explicit size then `/DS`'s.
    size: Option<f64>,
    colour: Option<TextColor>,
    /// `/DA`'s size, 0 meaning auto-size; used when nothing else sets one.
    da_size: f64,
}

impl Fill {
    fn new(da: Option<&crate::vartext::DefaultAppearance>, ds: &Style, da_first: bool) -> Self {
        let da_size = da.map_or(0.0, |d| d.font_size);
        let da_colour = da.and_then(|d| d.color);
        let ds_colour = ds.color.map(|[r, g, b]| TextColor::Rgb(r, g, b));
        let (size, colour) = if da_first {
            let explicit = Some(da_size).filter(|s| *s != 0.0);
            (explicit.or(ds.size_pt), da_colour.or(ds_colour))
        } else {
            (None, da_colour)
        };
        Self {
            size,
            colour,
            da_size,
        }
    }
}

/// Each paragraph's quadding: its first run's `text-align`, else `/Q`.
fn paragraph_aligns(runs: &[Run], q: Quadding) -> Vec<Quadding> {
    let count = runs.last().map_or(0, |r| r.paragraph + 1);
    let mut out = vec![None; count];
    for run in runs {
        if let Some(slot @ None) = out.get_mut(run.paragraph) {
            *slot = Some(run.style.align);
        }
    }
    out.into_iter()
        .map(|a| match a.flatten() {
            Some(Align::Left) => Quadding::Left,
            Some(Align::Center) => Quadding::Center,
            Some(Align::Right) => Quadding::Right,
            None => q,
        })
        .collect()
}

/// Font resolution state for one field.
struct Resolve<'a, 'n> {
    fonts: Vec<FontResource>,
    faces: &'a [DrFace],
    da_font: Option<Vec<u8>>,
    ds_family: Vec<String>,
    da_first: bool,
    notes: &'n mut DisplayNotes,
}

/// The weight and slant a run asks for.
#[derive(Clone, Copy)]
struct Want {
    bold: bool,
    italic: bool,
}

impl<'a> Resolve<'a, '_> {
    /// One run as a span. `own` is the run's style without `/DS`, present
    /// only under `DefaultAppearanceFirst`.
    fn span(&mut self, run: &Run, own: Option<&Style>, fill: &Fill) -> RichSpan {
        let style = &run.style;
        self.note_undrawn(style);
        let mine = own.unwrap_or(style);
        let font = self.font(style, &mine.family);
        let size = mine.size_pt.or(fill.size).unwrap_or(fill.da_size);
        let colour = mine
            .color
            .map(|[r, g, b]| TextColor::Rgb(r, g, b))
            .or(fill.colour);
        RichSpan::new(
            run.text.clone(),
            font,
            size,
            colour.unwrap_or(TextColor::Gray(0.0)),
            run.paragraph,
        )
        .with_decoration(
            style.underline == Some(true),
            style.strikethrough == Some(true),
        )
    }

    /// Report the Table 225 attributes that parse but are not drawn.
    fn note_undrawn(&mut self, style: &Style) {
        if style.baseline_shift_pt.is_some_and(|s| s != 0.0) {
            super::note(&mut self.notes.rich_ignored, "vertical-align".to_owned());
        }
        if style.stretch.is_some_and(|s| s != super::Stretch::Normal) {
            super::note(&mut self.notes.rich_ignored, "font-stretch".to_owned());
        }
    }

    /// The resource name a run draws with: the first of `requested` that
    /// resolves, else `/DS`'s families and the `/DA` font in
    /// [`RichStylePrecedence`] order, else Helvetica — in the run's weight
    /// and slant. A requested family not used is reported.
    fn font(&mut self, style: &Style, requested: &[String]) -> Vec<u8> {
        let want = Want {
            bold: style.weight.is_some_and(|w| w >= 600),
            italic: style.italic == Some(true),
        };
        if let Some((name, _)) = self.family(requested, want) {
            return name;
        }
        let ds = std::mem::take(&mut self.ds_family);
        let chosen = if self.da_first {
            self.da(want).or_else(|| self.family(&ds, want))
        } else {
            self.family(&ds, want).or_else(|| self.da(want))
        };
        self.ds_family = ds;
        let (name, to) = chosen.unwrap_or_else(|| self.standard(Std14::Helvetica, want));
        if !requested.is_empty() {
            let from = requested.join(", ");
            super::note(&mut self.notes.rich_fonts, format!("{from} -> {to}"));
        }
        name
    }

    /// The first family in `list` that resolves, as a resource name and
    /// the face's name: a `/DR` face in the wanted style, else a
    /// standard-14 family (a non-generic alias such as `Arial` reported),
    /// else a `/DR` face of the family in another style (reported).
    fn family(&mut self, list: &[String], want: Want) -> Option<(Vec<u8>, String)> {
        list.iter().find_map(|name| {
            let key = family_key(name);
            if let Some(face) = self.dr_face(&key, want, true) {
                return Some((face.name.clone(), face.base.clone()));
            }
            if let Some((face, alias)) = family_face(name) {
                let (res, to) = self.standard(face, want);
                if alias {
                    super::note(&mut self.notes.rich_fonts, format!("{name} -> {to}"));
                }
                return Some((res, to));
            }
            let face = self.dr_face(&key, want, false)?;
            let to = face.base.clone();
            super::note(&mut self.notes.rich_fonts, format!("{name} -> {to}"));
            Some((face.name.clone(), to))
        })
    }

    /// A `/DR` face of family `key`, in exactly the wanted style or any.
    fn dr_face(&self, key: &str, want: Want, exact: bool) -> Option<&'a DrFace> {
        let faces: &'a [DrFace] = self.faces;
        faces.iter().find(|f| {
            f.family == key && (!exact || (f.bold == want.bold && f.italic == want.italic))
        })
    }

    /// The `/DA` font: its own resource for a plain run, else a `/DR` face
    /// of its family in the wanted style, else its standard-14 face styled.
    fn da(&mut self, want: Want) -> Option<(Vec<u8>, String)> {
        let name = self.da_font.clone()?;
        let res = self.fonts.iter().find(|f| f.name == name)?.font;
        let face = self.faces.iter().find(|f| f.name == name);
        if !want.bold && !want.italic {
            let to = face.map_or_else(|| std14_base_font_name(res).to_owned(), |f| f.base.clone());
            return Some((name, to));
        }
        if let Some(styled) = face.and_then(|f| self.dr_face(&f.family, want, true)) {
            return Some((styled.name.clone(), styled.base.clone()));
        }
        Some(self.standard(res, want))
    }

    /// The resource for standard-14 `face` in the wanted style.
    fn standard(&mut self, face: Std14, want: Want) -> (Vec<u8>, String) {
        let face = std14_styled(face, want.bold, want.italic).unwrap_or(face);
        (self.resource(face), std14_base_font_name(face).to_owned())
    }

    /// A resource that IS `face` — not a stand-in for another font — adding
    /// one when `/DR` has none.
    fn resource(&mut self, face: Std14) -> Vec<u8> {
        let own = self
            .fonts
            .iter()
            .find(|f| f.font == face && f.widths.is_none() && f.codes.is_none() && f.cid.is_none());
        if let Some(f) = own {
            return f.name.clone();
        }
        let mut name = b"Rt".to_vec();
        name.extend(std14_base_font_name(face).bytes().filter(|&b| b != b'-'));
        while self.fonts.iter().any(|f| f.name == name) {
            name.push(b'_');
        }
        self.fonts.push(FontResource {
            name: name.clone(),
            font: face,
            widths: None,
            ascent: None,
            descent: None,
            codes: None,
            cid: None,
        });
        name
    }
}

/// The standard-14 family `name` denotes, and whether it is an alias for a
/// different font (reported) rather than the family itself or a CSS
/// generic family.
fn family_face(name: &str) -> Option<(Std14, bool)> {
    let key: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '\'' | '"'))
        .flat_map(char::to_lowercase)
        .collect();
    Some(match key.as_str() {
        "helvetica" | "sansserif" => (Std14::Helvetica, false),
        "times" | "timesroman" | "serif" => (Std14::TimesRoman, false),
        "courier" | "monospace" => (Std14::Courier, false),
        "symbol" => (Std14::Symbol, false),
        "zapfdingbats" => (Std14::ZapfDingbats, false),
        "arial" | "arialmt" => (Std14::Helvetica, true),
        "timesnewroman" | "timesnewromanpsmt" => (Std14::TimesRoman, true),
        "couriernew" | "couriernewpsmt" => (Std14::Courier, true),
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn families_map_to_standard_faces() {
        assert_eq!(
            family_face("Times New Roman"),
            Some((Std14::TimesRoman, true))
        );
        assert_eq!(family_face("sans-serif"), Some((Std14::Helvetica, false)));
        assert_eq!(family_face("'Courier'"), Some((Std14::Courier, false)));
        assert_eq!(family_face("Wingdings"), None);
    }

    #[test]
    fn base_fonts_split_into_family_and_style() {
        let face = |b: &str| {
            let f = DrFace::new(Vec::new(), b);
            (f.family, f.bold, f.italic)
        };
        assert_eq!(face("ABCDEF+Courier-Bold"), ("courier".into(), true, false));
        assert_eq!(face("Arial,BoldItalic"), ("arial".into(), true, true));
        assert_eq!(face("Helvetica-Oblique"), ("helvetica".into(), false, true));
        assert_eq!(face("ArialBold"), ("arial".into(), true, false));
        assert_eq!(
            face("TimesNewRomanPS-BoldMT"),
            ("timesnewroman".into(), true, false)
        );
        assert_eq!(face("Times-Roman"), ("times".into(), false, false));
    }
}
