//! Print finishing: staple, hole punch, fold and output bin, and how each
//! maps onto an IPP `finishings` enum (PWG 5100.1; values from the IANA IPP
//! registry) and onto a Windows Print Schema option (`print_schema`). Pure,
//! so it compiles and is tested on every target.
//!
//! What a device offers is the device's to say: every request is checked
//! against the supported list it reports, and one it does not offer is
//! refused by name rather than dropped.

/// Where the staples go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Staple {
    /// One staple, top-left corner.
    TopLeft,
    /// One staple, top-right corner.
    TopRight,
    /// One staple, bottom-left corner.
    BottomLeft,
    /// One staple, bottom-right corner.
    BottomRight,
    /// Two staples along the left edge.
    DualLeft,
    /// Two staples along the right edge.
    DualRight,
    /// Two staples along the top edge.
    DualTop,
    /// Two staples along the bottom edge.
    DualBottom,
    /// Saddle stitch: staples through the fold of a booklet.
    Saddle,
}

/// A sheet edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Edge {
    /// The left edge.
    Left,
    /// The top edge.
    Top,
    /// The right edge.
    Right,
    /// The bottom edge.
    Bottom,
}

/// A fold (PWG 5100.1 `fold-*`). IPP only: the Windows Print Schema has no
/// fold types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Fold {
    /// In half.
    Half,
    /// Letter (tri-fold, C-fold).
    Letter,
    /// Z-fold.
    Z,
    /// Half, then Z.
    HalfZ,
    /// Gate fold.
    Gate,
    /// Double gate fold.
    DoubleGate,
    /// Left gate fold.
    LeftGate,
    /// Right gate fold.
    RightGate,
    /// Accordion fold.
    Accordion,
    /// Parallel fold.
    Parallel,
    /// Poster fold.
    Poster,
    /// Engineering Z-fold, for large drawings.
    EngineeringZ,
}

/// The finishing one job asks for. `Default` asks for none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Finishing {
    /// Staple position.
    pub staple: Option<Staple>,
    /// Hole-punch edge.
    pub punch: Option<Edge>,
    /// Hole count, 2 to 4 (IPP only); `None` takes what the device offers.
    pub holes: Option<u8>,
    /// Fold (IPP only).
    pub fold: Option<Fold>,
    /// Output bin, by the name the device lists.
    pub output_bin: Option<String>,
}

impl Finishing {
    /// Whether nothing is asked for.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl Staple {
    /// Every position, in declaration order.
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::DualLeft,
        Self::DualRight,
        Self::DualTop,
        Self::DualBottom,
        Self::Saddle,
    ];

    /// The IPP `finishings` value (IANA IPP registry, PWG 5100.1).
    pub const fn ipp(self) -> i32 {
        match self {
            Self::TopLeft => 20,
            Self::BottomLeft => 21,
            Self::TopRight => 22,
            Self::BottomRight => 23,
            Self::DualLeft => 28,
            Self::DualTop => 29,
            Self::DualRight => 30,
            Self::DualBottom => 31,
            Self::Saddle => 8,
        }
    }
}

impl Edge {
    /// Every edge, in declaration order.
    pub const ALL: [Self; 4] = [Self::Left, Self::Top, Self::Right, Self::Bottom];

    /// The edge's offset in each four-value `punch-*-{left,top,right,bottom}`
    /// run of the registry.
    pub(crate) const fn ipp_offset(self) -> i32 {
        match self {
            Self::Left => 0,
            Self::Top => 1,
            Self::Right => 2,
            Self::Bottom => 3,
        }
    }
}

impl Fold {
    /// Every fold, in declaration order.
    pub const ALL: [Self; 12] = [
        Self::Half,
        Self::Letter,
        Self::Z,
        Self::HalfZ,
        Self::Gate,
        Self::DoubleGate,
        Self::LeftGate,
        Self::RightGate,
        Self::Accordion,
        Self::Parallel,
        Self::Poster,
        Self::EngineeringZ,
    ];

    /// The IPP `finishings` value.
    pub const fn ipp(self) -> i32 {
        match self {
            Self::Accordion => 90,
            Self::DoubleGate => 91,
            Self::Gate => 92,
            Self::Half => 93,
            Self::HalfZ => 94,
            Self::LeftGate => 95,
            Self::Letter => 96,
            Self::Parallel => 97,
            Self::Poster => 98,
            Self::RightGate => 99,
            Self::Z => 100,
            Self::EngineeringZ => 101,
        }
    }
}

/// The first value of the `punch-dual-*`, `punch-triple-*`, `punch-quad-*`
/// and `punch-multiple-*` runs, each `left, top, right, bottom`.
pub(crate) const PUNCH_RUNS: [(u8, i32); 4] = [(2, 74), (3, 78), (4, 82), (0, 86)];

/// The IPP `finishings` values for `f`, checked against `supported` (the
/// device's `finishings-supported`). `None` means the device's list is not
/// known: exact values are sent and nothing is checked. The output bin is
/// a separate attribute and is not included.
///
/// # Errors
///
/// One line per request the device does not offer, naming it.
pub fn ipp_finishings(f: &Finishing, supported: Option<&[i32]>) -> Result<Vec<i32>, Vec<String>> {
    let offered = |n: i32| supported.is_none_or(|s| s.contains(&n));
    let mut out = Vec::new();
    let mut refused = Vec::new();
    let mut want = |n: i32, what: String| {
        if offered(n) {
            out.push(n);
        } else {
            refused.push(format!("{what} ({})", finishing_name(n).unwrap_or("?")));
        }
    };
    if let Some(s) = f.staple {
        want(s.ipp(), format!("staple {s:?}"));
    }
    if let Some(edge) = f.punch {
        let n = punch_value(edge, f.holes, supported);
        want(n, format!("punch {edge:?}"));
    }
    if let Some(fold) = f.fold {
        want(fold.ipp(), format!("fold {fold:?}"));
    }
    if refused.is_empty() {
        Ok(out)
    } else {
        Err(refused)
    }
}

/// The `punch-*` value for `edge`: the run with `holes` holes, or with no
/// count given the first run the device offers on that edge (the dual run
/// when its list is unknown).
fn punch_value(edge: Edge, holes: Option<u8>, supported: Option<&[i32]>) -> i32 {
    let at = |start: i32| start + edge.ipp_offset();
    match holes {
        Some(h) => PUNCH_RUNS
            .iter()
            .find(|(n, _)| *n == h)
            .map_or(at(86), |(_, s)| at(*s)),
        None => PUNCH_RUNS
            .iter()
            .map(|(_, s)| at(*s))
            .find(|n| supported.is_some_and(|list| list.contains(n)))
            .unwrap_or(at(74)),
    }
}

/// The registry keyword for a `finishings` value, for reports.
pub fn finishing_name(n: i32) -> Option<&'static str> {
    let names: &[(&str, i32)] = &[
        ("none", 3),
        ("staple", 4),
        ("punch", 5),
        ("cover", 6),
        ("bind", 7),
        ("saddle-stitch", 8),
        ("edge-stitch", 9),
        ("fold", 10),
        ("trim", 11),
        ("bale", 12),
        ("booklet-maker", 13),
        ("jog-offset", 14),
        ("coat", 15),
        ("laminate", 16),
        ("staple-top-left", 20),
        ("staple-bottom-left", 21),
        ("staple-top-right", 22),
        ("staple-bottom-right", 23),
        ("punch-top-left", 70),
        ("punch-bottom-left", 71),
        ("punch-top-right", 72),
        ("punch-bottom-right", 73),
        ("fold-accordion", 90),
        ("fold-double-gate", 91),
        ("fold-gate", 92),
        ("fold-half", 93),
        ("fold-half-z", 94),
        ("fold-left-gate", 95),
        ("fold-letter", 96),
        ("fold-parallel", 97),
        ("fold-poster", 98),
        ("fold-right-gate", 99),
        ("fold-z", 100),
        ("fold-engineering-z", 101),
    ];
    if let Some((name, _)) = names.iter().find(|(_, v)| *v == n) {
        return Some(name);
    }
    let runs: &[(i32, &[&str; 4])] = &[
        (24, &EDGE_STITCH),
        (28, &STAPLE_DUAL),
        (32, &STAPLE_TRIPLE),
        (50, &BIND),
        (74, &PUNCH_DUAL),
        (78, &PUNCH_TRIPLE),
        (82, &PUNCH_QUAD),
        (86, &PUNCH_MULTIPLE),
    ];
    runs.iter()
        .find(|(start, _)| (*start..*start + 4).contains(&n))
        .and_then(|(start, names)| names.get(usize::try_from(n - start).ok()?).copied())
}

const EDGE_STITCH: [&str; 4] = [
    "edge-stitch-left",
    "edge-stitch-top",
    "edge-stitch-right",
    "edge-stitch-bottom",
];
const STAPLE_DUAL: [&str; 4] = [
    "staple-dual-left",
    "staple-dual-top",
    "staple-dual-right",
    "staple-dual-bottom",
];
const STAPLE_TRIPLE: [&str; 4] = [
    "staple-triple-left",
    "staple-triple-top",
    "staple-triple-right",
    "staple-triple-bottom",
];
const BIND: [&str; 4] = ["bind-left", "bind-top", "bind-right", "bind-bottom"];
const PUNCH_DUAL: [&str; 4] = [
    "punch-dual-left",
    "punch-dual-top",
    "punch-dual-right",
    "punch-dual-bottom",
];
const PUNCH_TRIPLE: [&str; 4] = [
    "punch-triple-left",
    "punch-triple-top",
    "punch-triple-right",
    "punch-triple-bottom",
];
const PUNCH_QUAD: [&str; 4] = [
    "punch-quad-left",
    "punch-quad-top",
    "punch-quad-right",
    "punch-quad-bottom",
];
const PUNCH_MULTIPLE: [&str; 4] = [
    "punch-multiple-left",
    "punch-multiple-top",
    "punch-multiple-right",
    "punch-multiple-bottom",
];

mod driver;
/// The Windows Print Schema side: reading a printer's capabilities and
/// writing the delta ticket.
pub mod print_schema;

pub use driver::{apply_finishing, printer_finishing};

#[cfg(test)]
mod tests;
