//! Model lights: `--light` for a glTF conversion, and the list `3d-render`
//! prints.

use crate::arg_types::{parse_hex_rgb, parse_point3};

/// One `--light`: a light added to a model converted from glTF.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LightArg {
    pub(crate) kind: LightArgKind,
    /// sRGB, 0–1.
    pub(crate) colour: [f64; 3],
    pub(crate) intensity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LightArgKind {
    Ambient,
    Point([f64; 3]),
    Directional([f64; 3]),
    /// Position, direction, half-angle in degrees.
    Spot([f64; 3], [f64; 3], f64),
}

const LIGHT_SYNTAX: &str = "ambient, point:X,Y,Z, directional:DX,DY,DZ or \
     spot:X,Y,Z:DX,DY,DZ:DEGREES, each optionally followed by :RRGGBB and :INTENSITY";

/// Clap parser for `--light`.
pub(crate) fn parse_light(s: &str) -> Result<LightArg, String> {
    let bad = |why: String| format!("`{s}`: {why}; expected {LIGHT_SYNTAX}");
    let mut parts = s.split(':');
    let kind = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut next = |what: &str| {
        parts
            .next()
            .ok_or_else(|| bad(format!("{what} is missing")))
    };
    let kind = match kind.as_str() {
        "ambient" => LightArgKind::Ambient,
        "point" => LightArgKind::Point(parse_point3(next("the position")?).map_err(bad)?),
        "directional" => {
            LightArgKind::Directional(parse_point3(next("the direction")?).map_err(bad)?)
        }
        "spot" => {
            let at = parse_point3(next("the position")?).map_err(bad)?;
            let towards = parse_point3(next("the direction")?).map_err(bad)?;
            let angle = next("the angle")?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|a| (0.0..=90.0).contains(a))
                .ok_or_else(|| bad("the angle is not 0 to 90 degrees".to_owned()))?;
            LightArgKind::Spot(at, towards, angle)
        }
        other => return Err(bad(format!("`{other}` is not a light kind"))),
    };
    let colour = match parts.next() {
        Some(hex) => parse_hex_rgb(hex)
            .map_err(bad)?
            .map(|c| f64::from(c) / 255.0),
        None => [1.0; 3],
    };
    let intensity = match parts.next() {
        Some(i) => i
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|i| i.is_finite() && *i >= 0.0)
            .ok_or_else(|| bad("the intensity is not a number of 0 or more".to_owned()))?,
        None => 1.0,
    };
    if parts.next().is_some() {
        return Err(bad("too many parts".to_owned()));
    }
    Ok(LightArg {
        kind,
        colour,
        intensity,
    })
}

#[cfg(feature = "3d")]
impl LightArg {
    /// The light as the engine holds it: a point or spot light does not
    /// fade with distance.
    pub(crate) fn to_model(&self) -> pdfcer_3d::ModelLight {
        use pdfcer_3d::{ModelLight, ModelLightKind as K};
        const NO_FADE: [f64; 3] = [1.0, 0.0, 0.0];
        let kind = match self.kind {
            LightArgKind::Ambient => K::Ambient,
            LightArgKind::Point(position) => K::Point {
                position,
                attenuation: NO_FADE,
            },
            LightArgKind::Directional(direction) => K::Directional { direction },
            LightArgKind::Spot(position, direction, degrees) => K::Spot {
                position,
                direction,
                attenuation: NO_FADE,
                angle: degrees.to_radians(),
                exponent: 0.0,
            },
        };
        let mut light = ModelLight::new(kind, self.colour);
        light.intensity = self.intensity;
        light.name = Some("--light".into());
        light
    }
}

/// One `light` line per model light, for `3d-render`'s off-canvas list.
#[cfg(feature = "3d")]
pub(crate) fn print_model_lights(lights: &[pdfcer_3d::ModelLight]) {
    for (i, l) in lights.iter().enumerate() {
        println!("light index={i} {}", light_fields(l));
    }
}

/// One `view-light` line per light of each lit PRC view; `used` is the view
/// whose lights are drawn.
#[cfg(feature = "3d")]
pub(crate) fn print_view_lights(views: &[pdfcer_3d::ViewLights], used: Option<usize>) {
    let yes = |b: bool| if b { "yes" } else { "no" };
    for (i, v) in views.iter().enumerate() {
        for (j, l) in v.lights.iter().enumerate() {
            println!(
                "view-light view={i} view-name={:?} default={} active={} used={} index={j} {}",
                v.view.as_deref().unwrap_or("-"),
                yes(v.default),
                yes(v.active),
                yes(used == Some(i)),
                light_fields(l)
            );
        }
    }
}

/// A light's `kind=… name=… [place] colour=… intensity=…` fields.
#[cfg(feature = "3d")]
fn light_fields(l: &pdfcer_3d::ModelLight) -> String {
    use pdfcer_3d::ModelLightKind as K;
    let v = |p: [f64; 3]| p.map(|c| format!("{c:.6}")).join(",");
    let place = match l.kind {
        K::Point {
            position,
            attenuation,
        } => format!(" position={} attenuation={}", v(position), v(attenuation)),
        K::Directional { direction } => format!(" direction={}", v(direction)),
        K::Spot {
            position,
            direction,
            attenuation,
            angle,
            exponent,
        } => format!(
            " position={} direction={} attenuation={} angle={:.3} exponent={exponent:.3}",
            v(position),
            v(direction),
            v(attenuation),
            angle.to_degrees()
        ),
        _ => String::new(),
    };
    let specular = l.specular.map(|s| format!(" specular={}", v(s)));
    format!(
        "kind={} name={:?}{place} colour={} intensity={:.6}{}",
        l.kind.name(),
        l.name.as_deref().unwrap_or("-"),
        v(l.colour),
        l.intensity,
        specular.unwrap_or_default()
    )
}

/// `--shininess-scale` as the engine's setting.
#[cfg(feature = "3d")]
pub(crate) fn shininess_scale(s: crate::ShininessScaleArg) -> pdfcer_3d::ShininessScale {
    use crate::ShininessScaleArg as A;
    match s {
        A::Auto => pdfcer_3d::ShininessScale::Auto,
        A::Exponent => pdfcer_3d::ShininessScale::Exponent,
        A::Fraction => pdfcer_3d::ShininessScale::Fraction,
    }
}

/// The `note: finish:` line: how many meshes were drawn with a highlight
/// or a glow, and how shininess was read. Nothing when none carries one,
/// or the scheme is unlit.
#[cfg(feature = "3d")]
pub(crate) fn print_finish_note(
    model: &pdfcer_3d::AssembledModel,
    options: &pdfcer_3d::RenderOptions,
) {
    if options.lighting == pdfcer_3d::LightingScheme::None {
        return;
    }
    let finishes = model.finishes.iter().flatten();
    let shiny = finishes.clone().filter(|f| f.is_specular()).count();
    let glowing = finishes.filter(|f| f.is_emissive()).count();
    if shiny + glowing == 0 {
        return;
    }
    println!(
        "note: finish: {shiny} mesh(es) drawn with the highlight their material stores \
         (Blinn-Phong, shininess read as `{}`; `--shininess-scale` sets it), {glowing} with \
         the light their material gives off",
        options.shininess.name()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_parses_with_optional_colour_and_intensity() {
        let l = parse_light("spot:0,0,5:0,0,-1:30:FF8000:2").unwrap();
        assert_eq!(
            l.kind,
            LightArgKind::Spot([0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 30.0)
        );
        assert_eq!(l.colour, [1.0, 128.0 / 255.0, 0.0]);
        assert_eq!(l.intensity, 2.0);
        let l = parse_light("ambient:333333").unwrap();
        assert_eq!((l.kind, l.intensity), (LightArgKind::Ambient, 1.0));
        assert_eq!(
            parse_light("Point:1,2,3").unwrap().kind,
            LightArgKind::Point([1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn a_malformed_light_says_what_is_wrong() {
        for (s, why) in [
            ("lamp:1,2,3", "not a light kind"),
            ("point", "position is missing"),
            ("spot:0,0,0:0,0,1:120", "0 to 90"),
            ("directional:0,0,1:FFFFFF:-1", "intensity"),
            ("ambient:FFFFFF:1:2", "too many"),
        ] {
            let err = parse_light(s).unwrap_err();
            assert!(err.contains(why), "{s}: {err}");
        }
    }
}
