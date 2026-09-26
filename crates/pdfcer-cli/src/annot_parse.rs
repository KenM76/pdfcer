use super::*;

/// Build a [`MarkupSpec`](pdfcer_core::annot_author::MarkupSpec) from the
/// parsed `annotate` flags, or a human-readable error naming the missing
/// or malformed geometry.
pub(crate) fn build_markup_spec(
    args: &AnnotateArgs<'_>,
) -> Result<pdfcer_core::annot_author::MarkupSpec, String> {
    use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec, Quad, TextMarkupKind};

    // Per-subtype default mark colour (pdfcer's own defaults; the Acrobat
    // RAG marks most of these a GAP — highlight yellow is the sourced one).
    let default_color = match args.kind {
        AnnotKindArg::Highlight => Color::Rgb(1.0, 1.0, 0.0),
        _ => Color::Rgb(1.0, 0.0, 0.0),
    };
    let color = match args.color {
        Some(h) => parse_color(h)?,
        None => default_color,
    };
    let interior = match args.fill {
        Some(h) => Some(parse_color(h)?),
        None => None,
    };

    match args.kind {
        AnnotKindArg::Square | AnnotKindArg::Circle => {
            let r = rect_from(args.rect.ok_or("this subtype needs --rect x0,y0,x1,y1")?)?;
            if matches!(args.kind, AnnotKindArg::Square) {
                Ok(MarkupSpec::Square {
                    rect: r,
                    border: Some(color),
                    interior,
                    border_width: args.width,
                    border_effect: args.cloud,
                })
            } else {
                // `/BE` is legal on `/Circle` per Table 180, but pdfcer does
                // not author it: an ellipse has no straight edges to
                // scallop, and the standard describes no curve-following
                // cloud. Refused by name rather than silently dropped —
                // a caller who typed --cloud is entitled to know it did
                // nothing.
                if args.cloud.is_some() {
                    return Err("--cloud is not supported on circle: a cloudy border is defined for straight-edged shapes, so use --type square or --type polygon"
                        .into());
                }
                Ok(MarkupSpec::Circle {
                    rect: r,
                    border: Some(color),
                    interior,
                    border_width: args.width,
                })
            }
        }
        AnnotKindArg::Line => {
            let f = parse_floats(args.line.ok_or("line needs --line x0,y0,x1,y1")?)?;
            let [x0, y0, x1, y1] = <[f64; 4]>::try_from(f)
                .map_err(|_| "--line needs exactly four numbers".to_owned())?;
            Ok(MarkupSpec::Line {
                start: (x0, y0),
                end: (x1, y1),
                color,
                width: args.width,
                endings: (LineEnding::OpenArrow, LineEnding::OpenArrow),
            })
        }
        AnnotKindArg::Ink => {
            let strokes = parse_strokes(args.strokes.ok_or("ink needs --strokes")?)?;
            Ok(MarkupSpec::Ink {
                strokes,
                color,
                width: args.width,
            })
        }
        AnnotKindArg::Polygon | AnnotKindArg::Polyline => {
            let verts = parse_points(args.points.ok_or("this subtype needs --points")?)?;
            if matches!(args.kind, AnnotKindArg::Polygon) {
                // `--cloud` turns the polygon into a revision cloud. Same
                // `/Subtype /Polygon` either way — there is no `/Cloud`
                // subtype in ISO 32000 — so this is one flag, not a
                // seventh `--type`.
                if let Some(intensity) = args.cloud {
                    Ok(MarkupSpec::Cloud {
                        vertices: verts,
                        border: Some(color),
                        interior,
                        width: args.width,
                        intensity,
                    })
                } else {
                    Ok(MarkupSpec::Polygon {
                        vertices: verts,
                        border: Some(color),
                        interior,
                        width: args.width,
                    })
                }
            } else {
                // Table 181 declares `/BE` on Polygon/PolyLine qualified
                // "meaningful only for polygon annotations" — verbatim in
                // both editions. An open polyline cannot carry one.
                if args.cloud.is_some() {
                    return Err("--cloud is not supported on polyline: ISO 32000 declares the border effect \"meaningful only for polygon annotations\", so a cloud needs a closed shape (--type polygon or --type square)"
                        .into());
                }
                Ok(MarkupSpec::PolyLine {
                    vertices: verts,
                    color,
                    width: args.width,
                })
            }
        }
        AnnotKindArg::Highlight
        | AnnotKindArg::Underline
        | AnnotKindArg::Strikeout
        | AnnotKindArg::Squiggly => {
            let quads = match (args.quads, args.rect) {
                (Some(q), _) => parse_quads(q)?,
                (None, Some(r)) => vec![Quad::from_rect(rect_from(r)?)],
                (None, None) => {
                    return Err("text markup needs --quads or --rect".to_owned());
                }
            };
            let kind = match args.kind {
                AnnotKindArg::Highlight => TextMarkupKind::Highlight,
                AnnotKindArg::Underline => TextMarkupKind::Underline,
                AnnotKindArg::Strikeout => TextMarkupKind::StrikeOut,
                _ => TextMarkupKind::Squiggly,
            };
            Ok(MarkupSpec::TextMarkup { kind, quads, color })
        }
        // The Pass-6.2 text-bearing subtypes take the variable-text path
        // (build_text_annot_spec), never this geometric one.
        AnnotKindArg::Freetext | AnnotKindArg::Text | AnnotKindArg::Stamp => {
            Err("internal: text subtype routed to the geometric path".to_owned())
        }
    }
}

/// Build a [`TextAnnotSpec`](pdfcer_core::annot_author::TextAnnotSpec) from
/// the parsed `annotate` flags for a Pass-6.2 text-bearing subtype.
pub(crate) fn build_text_annot_spec(
    args: &AnnotateArgs<'_>,
) -> Result<pdfcer_core::annot_author::TextAnnotSpec, String> {
    use pdfcer_core::annot_author::{Color, TextAnnotSpec};
    use pdfcer_core::page_tree::Rect;
    use pdfcer_core::vartext::TextColor;

    let font = resolve_latin_std14(args.font)?;
    match args.kind {
        AnnotKindArg::Freetext => {
            let rect = rect_from(args.rect.ok_or("freetext needs --rect x0,y0,x1,y1")?)?;
            let text = args.text.ok_or("freetext needs --text \"…\"")?.to_owned();
            let color = match args.color {
                Some(h) => TextColor::from(parse_color(h)?),
                None => TextColor::Gray(0.0),
            };
            // --fill doubles as the optional FreeText border colour.
            let border = match args.fill {
                Some(h) => Some(parse_color(h)?),
                None => None,
            };
            Ok(TextAnnotSpec::FreeText {
                rect,
                text,
                font,
                font_size: args.size,
                color,
                quadding: args.quad.to_quadding(),
                multiline: args.multiline,
                border,
                border_width: args.width,
            })
        }
        AnnotKindArg::Text => {
            let rect = match args.rect {
                Some(r) => rect_from(r)?,
                None => Rect::from_corners(72.0, 72.0, 96.0, 96.0),
            };
            let color = match args.color {
                Some(h) => parse_color(h)?,
                None => Color::Rgb(1.0, 0.92, 0.30), // pdfcer's own note yellow
            };
            Ok(TextAnnotSpec::Sticky {
                rect,
                icon: args.icon.to_icon(),
                contents: args.text.unwrap_or_default().to_owned(),
                color,
                open: false,
            })
        }
        AnnotKindArg::Stamp => {
            let rect = rect_from(args.rect.ok_or("stamp needs --rect x0,y0,x1,y1")?)?;
            let color = match args.color {
                Some(h) => parse_color(h)?,
                None => Color::Rgb(0.80, 0.10, 0.10), // pdfcer's own stamp red
            };
            Ok(TextAnnotSpec::Stamp {
                rect,
                name: args.stamp_name.to_stamp_name(),
                label: args.text.map(str::to_owned),
                color,
                // `--rect` is a POSITION AND A MINIMUM under the default
                // `grow` fit, not a cage: a label too long for it widens the
                // stamp instead of being silently cut off.
                style: pdfcer_core::annot_author::StampStyle::points(
                    args.stamp_font_size
                        .unwrap_or(pdfcer_core::annot_author::DEFAULT_STAMP_FONT_SIZE),
                )
                .with_fit(args.stamp_fit.to_fit()),
            })
        }
        // The geometric subtypes never reach here (is_text_bearing gates).
        _ => Err("internal: non-text subtype routed to the text path".to_owned()),
    }
}

/// Resolve a `--font` value to a **Latin** standard-14 face, rejecting the
/// two symbolic fonts (they carry no `WinAnsi` encoding, so pdfcer's Latin
/// variable-text generator cannot lay text out in them) and any
/// non-standard-14 name (pdfcer authors only program-free standard-14 text
/// appearances — §9.6.2.1).
pub(crate) fn resolve_latin_std14(name: &str) -> Result<pdfcer_core::fontdata::Std14, String> {
    use pdfcer_core::fontdata::{Std14, std14_by_base_font};
    match std14_by_base_font(name) {
        Some(Std14::Symbol | Std14::ZapfDingbats) => Err(format!(
            "{name} is a symbolic font; text annotations need a Latin standard-14 font \
             (Helvetica, Times-Roman, Courier, and their Bold/Italic variants)"
        )),
        Some(f) => Ok(f),
        None => Err(format!(
            "{name:?} is not a standard-14 font name (e.g. Helvetica, Helvetica-Bold, \
             Times-Roman, Times-Italic, Courier)"
        )),
    }
}

/// Parse a whitespace/comma-separated list of decimal numbers.
pub(crate) fn parse_floats(s: &str) -> Result<Vec<f64>, String> {
    s.split([',', ' ', '\t', '\n', '\r'])
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().map_err(|_| format!("not a number: {t}")))
        .collect()
}

/// Parse `x0,y0,x1,y1` into a normalized [`Rect`](pdfcer_core::page_tree::Rect).
pub(crate) fn rect_from(s: &str) -> Result<pdfcer_core::page_tree::Rect, String> {
    let f = parse_floats(s)?;
    let [x0, y0, x1, y1] =
        <[f64; 4]>::try_from(f).map_err(|_| "a rectangle needs exactly four numbers".to_owned())?;
    Ok(pdfcer_core::page_tree::Rect::from_corners(x0, y0, x1, y1))
}

/// Parse `x,y x,y …` into a point list.
pub(crate) fn parse_points(s: &str) -> Result<Vec<(f64, f64)>, String> {
    let f = parse_floats(s)?;
    if f.len() % 2 != 0 {
        return Err("points must be an even count of numbers (x,y pairs)".to_owned());
    }
    Ok(f.chunks_exact(2).map(|c| (c[0], c[1])).collect())
}

/// Parse `x,y … | x,y …` into ink strokes (`|` separates strokes).
pub(crate) fn parse_strokes(s: &str) -> Result<Vec<Vec<(f64, f64)>>, String> {
    s.split('|')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(parse_points)
        .collect()
}

/// Parse `x1,y1,…,x8,y8 ; …` into text-markup quads (Z-order UL,UR,LL,LR).
pub(crate) fn parse_quads(s: &str) -> Result<Vec<pdfcer_core::annot_author::Quad>, String> {
    use pdfcer_core::annot_author::Quad;
    s.split(';')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| {
            let f = parse_floats(g)?;
            let a = <[f64; 8]>::try_from(f)
                .map_err(|_| "each quad needs exactly eight numbers".to_owned())?;
            Ok(Quad {
                ul: (a[0], a[1]),
                ur: (a[2], a[3]),
                ll: (a[4], a[5]),
                lr: (a[6], a[7]),
            })
        })
        .collect()
}

/// Parse an `RRGGBB` (optionally `#`-prefixed) hex colour into a device
/// RGB [`Color`](pdfcer_core::annot_author::Color).
pub(crate) fn parse_color(hex: &str) -> Result<pdfcer_core::annot_author::Color, String> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return Err(format!("colour must be RRGGBB hex, got {hex:?}"));
    }
    let component = |i: usize| -> Result<f64, String> {
        u8::from_str_radix(h.get(i..i + 2).unwrap_or(""), 16)
            .map(|v| f64::from(v) / 255.0)
            .map_err(|_| format!("colour must be RRGGBB hex, got {hex:?}"))
    };
    Ok(pdfcer_core::annot_author::Color::Rgb(
        component(0)?,
        component(2)?,
        component(4)?,
    ))
}

// ---------------------------------------------------------------------------
// Text extraction (Pass 4)
// ---------------------------------------------------------------------------
