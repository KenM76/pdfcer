//! A rich-text field's runs as styled spans for its appearance
//! (ISO 32000-1 §12.7.3.4; RT-M6, RT-M9, RT-M12).
//!
//! Precedence where `/DS` and `/DA` both set an attribute is undefined
//! (RT-A6). Here the run's own style, cascaded over `/DS`, wins; `/DA`
//! supplies only what neither sets — font, size, colour.

use super::{Align, Run, Style};
use crate::edit::decode_text_string;
use crate::field_display::DisplayNotes;
use crate::fontdata::{Std14, std14_base_font_name, std14_styled};
use crate::forms::{Field, FieldFlags};
use crate::vartext::{FontResource, Quadding, RichSpan, TextColor, parse_default_appearance};

/// What [`crate::vartext::build_rich_text`] needs for one field.
#[derive(Debug, Clone)]
pub(crate) struct RichPlan {
    pub spans: Vec<RichSpan>,
    pub aligns: Vec<Quadding>,
    /// `fonts` plus any standard-14 face a run needed that `/DR` lacks.
    pub fonts: Vec<FontResource>,
}

/// The rich value a redraw should use, when the caller is about to write
/// one: `Some(None)` = the field is becoming plain.
pub(crate) type RichOverride<'a> = Option<Option<&'a [u8]>>;

/// The styled spans for `field`, or `None` to draw it as plain text: not a
/// rich-text field, a password or comb field, no `/RV`, or an `/RV` that
/// will not parse (recorded in `notes.rich_unreadable`).
pub(crate) fn plan(
    field: &Field,
    rv: RichOverride<'_>,
    da: &[u8],
    fonts: &[FontResource],
    notes: &mut DisplayNotes,
) -> Option<RichPlan> {
    let multiline = field.flags.has(FieldFlags::MULTILINE);
    if !field.is_rich_text()
        || field.flags.has(FieldFlags::PASSWORD)
        || crate::widget_draw::comb_cells(field, multiline).is_some()
    {
        return None;
    }
    let rv = decode_text_string(rv.unwrap_or(field.rich_value.as_deref())?).text;
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
    notes.rich_text = true;
    notes.rich_ignored = parsed.ignored;
    let ds_style = ds.as_deref().map(super::parse_style).unwrap_or_default();
    let da = parse_default_appearance(da).ok();
    let mut ctx = Resolve {
        fonts: fonts.to_vec(),
        da_font: da.as_ref().map(|d| d.font_name.clone()),
        ds_family: ds_style.family,
        notes,
    };
    let size = da.as_ref().map_or(0.0, |d| d.font_size);
    let colour = da.as_ref().and_then(|d| d.color);
    let mut spans = Vec::with_capacity(parsed.runs.len());
    for run in &parsed.runs {
        ctx.note_undrawn(&run.style);
        let font = ctx.font(&run.style);
        let colour = run
            .style
            .color
            .map_or(colour, |[r, g, b]| Some(TextColor::Rgb(r, g, b)));
        let span = RichSpan::new(
            run.text.clone(),
            font,
            run.style.size_pt.unwrap_or(size),
            colour.unwrap_or(TextColor::Gray(0.0)),
            run.paragraph,
        )
        .with_decoration(
            run.style.underline == Some(true),
            run.style.strikethrough == Some(true),
        );
        spans.push(span);
    }
    Some(RichPlan {
        aligns: paragraph_aligns(&parsed.runs, field.quadding),
        spans,
        fonts: ctx.fonts,
    })
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
struct Resolve<'n> {
    fonts: Vec<FontResource>,
    da_font: Option<Vec<u8>>,
    ds_family: Vec<String>,
    notes: &'n mut DisplayNotes,
}

impl Resolve<'_> {
    /// Report the Table 225 attributes that parse but are not drawn.
    fn note_undrawn(&mut self, style: &Style) {
        if style.baseline_shift_pt.is_some_and(|s| s != 0.0) {
            super::note(&mut self.notes.rich_ignored, "vertical-align".to_owned());
        }
        if style.stretch.is_some_and(|s| s != super::Stretch::Normal) {
            super::note(&mut self.notes.rich_ignored, "font-stretch".to_owned());
        }
    }

    /// The resource name a run draws with: the first of its families that
    /// names a standard-14 family, else the first of `/DS`'s, else the
    /// `/DA` font — in the run's weight and slant.
    fn font(&mut self, style: &Style) -> Vec<u8> {
        let bold = style.weight.is_some_and(|w| w >= 600);
        let italic = style.italic == Some(true);
        let own = self.family(&style.family);
        let face = own.or_else(|| {
            let ds = std::mem::take(&mut self.ds_family);
            let face = self.family(&ds);
            self.ds_family = ds;
            face
        });
        let da = self.da_face();
        if own.is_none() && !style.family.is_empty() {
            let to = face
                .or(da.as_ref().map(|d| d.1))
                .map_or("Helvetica", std14_base_font_name);
            let from = style.family.join(", ");
            super::note(&mut self.notes.rich_fonts, format!("{from} -> {to}"));
        }
        let face = match (face, da) {
            (Some(face), _) => face,
            // The `/DA` resource itself, so `/DR`'s own font is bound.
            (None, Some((name, _))) if !bold && !italic => return name,
            (None, Some((_, face))) => face,
            (None, None) => Std14::Helvetica,
        };
        let face = std14_styled(face, bold, italic).unwrap_or(face);
        self.resource(face)
    }

    /// The first family in `list` with a standard-14 equivalent; a
    /// non-generic alias (`Arial`) is reported as a substitution.
    fn family(&mut self, list: &[String]) -> Option<Std14> {
        list.iter().find_map(|name| {
            let (face, alias) = family_face(name)?;
            if alias {
                let to = std14_base_font_name(face);
                super::note(&mut self.notes.rich_fonts, format!("{name} -> {to}"));
            }
            Some(face)
        })
    }

    /// The `/DA` font's resource name and face, when it resolves.
    fn da_face(&self) -> Option<(Vec<u8>, Std14)> {
        let name = self.da_font.as_ref()?;
        let res = self.fonts.iter().find(|f| &f.name == name)?;
        Some((name.clone(), res.font))
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
}
