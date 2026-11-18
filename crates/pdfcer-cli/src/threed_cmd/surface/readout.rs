//! How measured lengths are read out: `--measure-units`,
//! `--measure-dual-units` and `--measure-decimals`.

use super::target::Unit;
use pdfcer_3d::LengthUnit;

/// The engine unit a flag value names.
pub(super) fn length_unit(u: Unit) -> LengthUnit {
    match u {
        Unit::Um => LengthUnit::Micrometre,
        Unit::Mm => LengthUnit::Millimetre,
        Unit::Cm => LengthUnit::Centimetre,
        Unit::M => LengthUnit::Metre,
        Unit::In => LengthUnit::Inch,
        Unit::Ft => LengthUnit::Foot,
    }
}

/// The unit-converted figures that follow a model-unit figure. Empty when
/// the model's unit is not known.
#[derive(Clone, Copy, Debug)]
pub(super) struct Readout {
    mm_per_model_unit: Option<f64>,
    units: [Option<LengthUnit>; 2],
    decimals: usize,
}

impl Readout {
    /// Figures in `unit`, then `dual`, at `decimals` places, for a model
    /// whose unit is `mm_per_model_unit` millimetres.
    pub(super) fn new(
        mm_per_model_unit: Option<f64>,
        unit: LengthUnit,
        dual: Option<LengthUnit>,
        decimals: usize,
    ) -> Self {
        Readout {
            mm_per_model_unit,
            units: [Some(unit), dual],
            decimals,
        }
    }

    /// The units in use, primary first, when the model's unit is known.
    fn scales(&self, power: i32) -> impl Iterator<Item = (f64, LengthUnit)> + '_ {
        self.units.into_iter().flatten().filter_map(move |u| {
            let to = self.mm_per_model_unit? / u.mm_per_unit();
            Some((to.powi(power), u))
        })
    }

    fn joined(&self, v: f64, power: i32) -> String {
        let suffix = if power == 2 { "2" } else { "" };
        let parts: Vec<String> = self
            .scales(power)
            .map(|(k, u)| format!("{:.*} {u}{suffix}", self.decimals, v * k))
            .collect();
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(" / "))
        }
    }

    /// ` (L unit / L dual)` after a length.
    pub(super) fn length(&self, v: f64) -> String {
        self.joined(v, 1)
    }

    /// ` (A unit2 / A dual2)` after an area.
    pub(super) fn area(&self, v: f64) -> String {
        self.joined(v, 2)
    }

    /// ` point_UNIT=X,Y,Z` for each unit in use.
    pub(super) fn point(&self, p: [f64; 3]) -> String {
        self.scales(1)
            .map(|(k, u)| {
                let xyz = p.map(|c| format!("{:.*}", self.decimals, c * k)).join(",");
                format!(" point_{u}={xyz}")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_converts_into_each_unit_at_the_chosen_decimals() {
        let inch_model = Some(25.4);
        let r = Readout::new(
            inch_model,
            LengthUnit::Millimetre,
            Some(LengthUnit::Inch),
            2,
        );
        assert_eq!(r.length(2.0), " (50.80 mm / 2.00 in)");
        assert_eq!(r.area(1.0), " (645.16 mm2 / 1.00 in2)");
        assert_eq!(
            r.point([1.0, 0.0, -0.5]),
            " point_mm=25.40,0.00,-12.70 point_in=1.00,0.00,-0.50"
        );
        use clap::ValueEnum;
        for u in Unit::value_variants() {
            let name = u.to_possible_value().map(|v| v.get_name().to_owned());
            assert_eq!(name.as_deref(), Some(length_unit(*u).symbol()));
        }
        assert_eq!(Unit::value_variants().len(), LengthUnit::ALL.len());
        let unknown = Readout::new(None, LengthUnit::Foot, None, 6);
        assert_eq!(
            (unknown.length(1.0), unknown.point([0.0; 3])),
            (String::new(), String::new())
        );
    }
}
