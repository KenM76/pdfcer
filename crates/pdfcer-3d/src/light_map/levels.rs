//! Published lighting levels for a light map's bands: the Canada
//! Occupational Health and Safety Regulations, SOR/86-304, Part VI,
//! Schedules I–IV ([`CANADA_OHS`], [`CANADA_OHS_SOURCE`]), and a band
//! table stepping at their levels ([`LuxBands::canada_ohs_steps`]).
//!
//! Reproduced under the Reproduction of Federal Law Order, SI/97-5; this
//! is not an official version. EN 12464-1, IES and CIBSE tables are sold
//! under copyright and are not reproduced.

use super::{BandBlend, LuxBand, LuxBands};

/// Which schedule a level is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LuxSchedule {
    /// Schedule I (s. 6.4): office areas.
    Office,
    /// Schedule II (s. 6.5): industrial areas.
    Industrial,
    /// Schedule III (s. 6.6): areas common to all workplaces.
    General,
    /// Schedule IV (s. 6.7): VDT (screen) work.
    ScreenWork,
}

/// What the regulation makes of a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LevelLimit {
    /// The average, of four measurements at the task position or area
    /// (s. 6.3), shall not be less than the level (ss. 6.4–6.6).
    MinimumAverage,
    /// The average shall not be more than the level (s. 6.7(1)).
    Maximum,
}

/// One row of a schedule.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LuxLevel {
    /// The schedule.
    pub schedule: LuxSchedule,
    /// The schedule's item, as numbered there: `"1(a)"`, `"3(a)(i)"`.
    pub item: &'static str,
    /// The task position or area, in the schedule's words.
    pub area: &'static str,
    /// The level, lux.
    pub lux: f64,
    /// Whether it is a floor or a ceiling.
    pub limit: LevelLimit,
}

/// Where [`CANADA_OHS`] comes from, for the legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LevelSource {
    /// The standard's name.
    pub title: &'static str,
    /// The citation.
    pub citation: &'static str,
    /// The consolidation the rows were taken from.
    pub current_to: &'static str,
    /// Where it is published.
    pub url: &'static str,
    /// The terms it is reproduced under.
    pub reproduced: &'static str,
    /// What the levels apply to, and the rules beside them.
    pub scope: &'static str,
}

/// The source of [`CANADA_OHS`].
pub const CANADA_OHS_SOURCE: LevelSource = LevelSource {
    title: "Canada Occupational Health and Safety Regulations, Part VI (Lighting)",
    citation: "SOR/86-304, Part VI, Schedules I to IV",
    current_to: "2026-10-29",
    url: "https://laws-lois.justice.gc.ca/eng/regulations/SOR-86-304/",
    reproduced: "Reproduced under the Reproduction of Federal Law Order (SI/97-5); not an official version",
    scope: "Federally regulated workplaces. Each level is an average of four measurements (s. 6.3); \
            no single measurement may be below one third of it, one tenth for Schedule III items 8 \
            and 9 and Schedule IV item 1 (s. 6.11). Schedule IV levels are maxima.",
};

use LevelLimit::{Maximum as Max, MinimumAverage as Min};
use LuxSchedule::{General as Gen, Industrial as Ind, Office as Off, ScreenWork as Vdt};

const fn row(
    schedule: LuxSchedule,
    item: &'static str,
    area: &'static str,
    lux: f64,
    limit: LevelLimit,
) -> LuxLevel {
    LuxLevel {
        schedule,
        item,
        area,
        lux,
        limit,
    }
}

/// SOR/86-304 Schedules I–IV, row by row, in the schedules' order.
pub const CANADA_OHS: &[LuxLevel] = &[
    row(
        Off,
        "1(a)",
        "Desk work: task positions at which cartography, designing, drafting, plan-reading or other very difficult visual tasks are performed",
        1000.0,
        Min,
    ),
    row(
        Off,
        "1(b)",
        "Desk work: task positions at which business machines are operated or stenography, accounting, typing, filing, clerking, billing, continuous reading or writing or other difficult visual tasks are performed",
        500.0,
        Min,
    ),
    row(
        Off,
        "2",
        "Other office work: conference and interview rooms, file storage areas, switchboard or reception areas or other areas where ordinary visual tasks are performed",
        300.0,
        Min,
    ),
    row(
        Off,
        "3(a)(i)",
        "Service areas: stairways and corridors that are used frequently",
        100.0,
        Min,
    ),
    row(
        Off,
        "3(a)(ii)",
        "Service areas: stairways and corridors that are used infrequently",
        50.0,
        Min,
    ),
    row(
        Off,
        "3(b)",
        "Service areas: stairways that are used only in emergencies",
        30.0,
        Min,
    ),
    row(
        Ind,
        "1(a)",
        "Garages: main repair and maintenance areas, other than those referred to in paragraph (b)",
        300.0,
        Min,
    ),
    row(
        Ind,
        "1(b)",
        "Garages: main repair and maintenance areas used for repairing and maintaining cranes, bulldozers and other major equipment",
        150.0,
        Min,
    ),
    row(
        Ind,
        "1(c)",
        "Garages: general work areas adjacent to a main repair and maintenance area referred to in paragraph (b)",
        50.0,
        Min,
    ),
    row(Ind, "1(d)", "Garages: fuelling areas", 150.0, Min),
    row(Ind, "1(e)", "Garages: battery rooms", 100.0, Min),
    row(
        Ind,
        "1(f)(i)",
        "Garages: other areas in which there is a high or moderate level of activity",
        100.0,
        Min,
    ),
    row(
        Ind,
        "1(f)(ii)",
        "Garages: other areas in which there is a low level of activity",
        50.0,
        Min,
    ),
    row(
        Ind,
        "2(a)",
        "Laboratories: areas in which instruments are read and where errors in such reading may be hazardous to the health or safety of an employee",
        750.0,
        Min,
    ),
    row(
        Ind,
        "2(b)",
        "Laboratories: areas in which a hazardous substance is handled",
        500.0,
        Min,
    ),
    row(
        Ind,
        "2(c)",
        "Laboratories: areas in which laboratory work requiring close and prolonged attention is performed",
        500.0,
        Min,
    ),
    row(
        Ind,
        "2(d)",
        "Laboratories: areas in which other laboratory work is performed",
        300.0,
        Min,
    ),
    row(
        Ind,
        "3(a)",
        "Loading platforms, storage rooms and warehouses: areas in which packages are frequently checked or sorted",
        250.0,
        Min,
    ),
    row(
        Ind,
        "3(b)",
        "Loading platforms, storage rooms and warehouses: areas in which packages are infrequently checked or sorted",
        75.0,
        Min,
    ),
    row(
        Ind,
        "3(c)",
        "Loading platforms, storage rooms and warehouses: docks (indoor and outdoor), piers and other locations where packages or containers are loaded or unloaded",
        150.0,
        Min,
    ),
    row(
        Ind,
        "3(d)",
        "Loading platforms, storage rooms and warehouses: areas in which grain or granular material is loaded or unloaded in bulk",
        30.0,
        Min,
    ),
    row(
        Ind,
        "3(e)",
        "Loading platforms, storage rooms and warehouses: areas in which goods are stored in bulk or where goods in storage are all of one kind",
        30.0,
        Min,
    ),
    row(
        Ind,
        "3(f)",
        "Loading platforms, storage rooms and warehouses: areas where goods in storage are of different kinds",
        75.0,
        Min,
    ),
    row(
        Ind,
        "3(g)",
        "Loading platforms, storage rooms and warehouses: any other area",
        10.0,
        Min,
    ),
    row(
        Ind,
        "4(a)",
        "Machine and woodworking shops: areas in which medium or fine bench or machine work is performed",
        500.0,
        Min,
    ),
    row(
        Ind,
        "4(b)",
        "Machine and woodworking shops: areas in which rough bench or machine work is performed",
        300.0,
        Min,
    ),
    row(
        Ind,
        "4(c)",
        "Machine and woodworking shops: any other area",
        200.0,
        Min,
    ),
    row(
        Ind,
        "5(a)",
        "Manufacturing and processing areas: major control rooms or rooms with dial displays",
        500.0,
        Min,
    ),
    row(
        Ind,
        "5(b)(i)",
        "Manufacturing and processing areas: areas in which a hazardous substance is processed, manufactured or used, in main work areas",
        500.0,
        Min,
    ),
    row(
        Ind,
        "5(b)(ii)",
        "Manufacturing and processing areas: areas in which a hazardous substance is processed, manufactured or used, in surrounding areas",
        200.0,
        Min,
    ),
    row(
        Ind,
        "5(c)(i)",
        "Manufacturing and processing areas: areas in which substances that are not hazardous substances are processed, manufactured or used or where automatically controlled equipment operates, in main work areas",
        100.0,
        Min,
    ),
    row(
        Ind,
        "5(c)(ii)",
        "Manufacturing and processing areas: areas in which substances that are not hazardous substances are processed, manufactured or used or where automatically controlled equipment operates, in surrounding areas",
        50.0,
        Min,
    ),
    row(
        Ind,
        "6(a)(i)",
        "Service areas: stairways and elevating devices that are used frequently",
        100.0,
        Min,
    ),
    row(
        Ind,
        "6(a)(ii)",
        "Service areas: stairways and elevating devices that are used infrequently",
        50.0,
        Min,
    ),
    row(
        Ind,
        "6(b)",
        "Service areas: stairways that are used only in emergencies",
        30.0,
        Min,
    ),
    row(
        Ind,
        "6(c)(i)",
        "Service areas: corridors and aisles that are used by persons and mobile equipment, at main intersections",
        100.0,
        Min,
    ),
    row(
        Ind,
        "6(c)(ii)",
        "Service areas: corridors and aisles that are used by persons and mobile equipment, at other locations",
        50.0,
        Min,
    ),
    row(
        Ind,
        "6(d)",
        "Service areas: corridors and aisles that are used by mobile equipment only",
        50.0,
        Min,
    ),
    row(
        Ind,
        "6(e)(i)",
        "Service areas: corridors and aisles that are used by persons only and are used frequently by employees",
        50.0,
        Min,
    ),
    row(
        Ind,
        "6(e)(ii)",
        "Service areas: corridors and aisles that are used by persons only and are used infrequently by employees",
        30.0,
        Min,
    ),
    row(
        Gen,
        "1(a)(i)",
        "Building exteriors: entrances and exits that are used frequently",
        100.0,
        Min,
    ),
    row(
        Gen,
        "1(a)(ii)",
        "Building exteriors: entrances and exits that are used infrequently",
        50.0,
        Min,
    ),
    row(
        Gen,
        "1(b)(i)",
        "Building exteriors: passageways used by persons at vehicular intersections",
        30.0,
        Min,
    ),
    row(
        Gen,
        "1(b)(ii)",
        "Building exteriors: passageways used by persons at other locations",
        10.0,
        Min,
    ),
    row(
        Gen,
        "1(c)(i)",
        "Building exteriors: areas used by persons and mobile equipment in which there is a high or moderate level of activity",
        20.0,
        Min,
    ),
    row(
        Gen,
        "1(c)(ii)",
        "Building exteriors: areas used by persons and mobile equipment in which there is a low level of activity",
        10.0,
        Min,
    ),
    row(
        Gen,
        "1(d)(i)",
        "Building exteriors: storage areas in which there is a high or moderate level of activity",
        30.0,
        Min,
    ),
    row(
        Gen,
        "1(d)(ii)",
        "Building exteriors: storage areas in which there is a low level of activity",
        10.0,
        Min,
    ),
    row(
        Gen,
        "2(a)",
        "First aid rooms: in treatment and examination area",
        1000.0,
        Min,
    ),
    row(Gen, "2(b)", "First aid rooms: in other areas", 500.0, Min),
    row(Gen, "3", "Food preparation areas", 500.0, Min),
    row(Gen, "4", "Personal service rooms", 200.0, Min),
    row(Gen, "5", "Boiler rooms", 200.0, Min),
    row(
        Gen,
        "6",
        "Rooms in which principal heating, ventilation or air conditioning equipment is installed",
        50.0,
        Min,
    ),
    row(
        Gen,
        "7",
        "Emergency shower facilities and emergency equipment locations",
        50.0,
        Min,
    ),
    row(Gen, "8(a)", "Parking areas: covered", 50.0, Min),
    row(Gen, "8(b)", "Parking areas: open", 10.0, Min),
    row(Gen, "9", "Lobbies and atria", 100.0, Min),
    row(
        Vdt,
        "1(a)",
        "VDT work: task positions at which data entry and retrieval work are performed intermittently",
        500.0,
        Max,
    ),
    row(
        Vdt,
        "1(b)",
        "VDT work: task positions at which data entry work is performed exclusively",
        750.0,
        Max,
    ),
    row(
        Vdt,
        "1(d)",
        "VDT work: telephone operator areas",
        300.0,
        Max,
    ),
];

/// [`LuxBands::canada_ohs_steps`]' colours: below the lowest level, then
/// one per distinct level of [`CANADA_OHS`] rising, dark blue through
/// green and yellow to red.
const STEPS: [(f64, [u8; 3]); 14] = [
    (0.0, [20, 20, 60]),
    (10.0, [40, 40, 140]),
    (20.0, [30, 80, 200]),
    (30.0, [20, 130, 230]),
    (50.0, [20, 180, 210]),
    (75.0, [30, 190, 150]),
    (100.0, [60, 200, 90]),
    (150.0, [140, 210, 50]),
    (200.0, [200, 220, 40]),
    (250.0, [245, 210, 40]),
    (300.0, [250, 170, 30]),
    (500.0, [245, 120, 30]),
    (750.0, [230, 60, 30]),
    (1000.0, [190, 20, 60]),
];

impl LuxBands {
    /// A band at each distinct level of [`CANADA_OHS`] (10 lx to
    /// 1000 lx) and one below the lowest from 0, so a surface's colour says
    /// which levels its illuminance reaches. For a map in lux; for a
    /// relative map the bounds mean nothing.
    #[must_use]
    pub fn canada_ohs_steps(blend: BandBlend) -> LuxBands {
        LuxBands {
            bands: STEPS
                .iter()
                .map(|&(from, colour)| LuxBand { from, colour })
                .collect(),
            blend,
        }
    }
}
