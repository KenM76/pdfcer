//! Finishing in the Windows Print Schema: the `JobStapleAllDocuments`,
//! `JobHolePunch` and `JobOutputBin` features of a printer's
//! PrintCapabilities document, and the delta PrintTicket that asks for
//! them (Microsoft Print Schema Specification, public keywords). The
//! schema has no fold types and no hole count, so those are refused.

use super::{Edge, Finishing, Staple};

/// The Print Schema framework namespace.
pub const PSF: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework";
/// The Print Schema public keywords namespace.
pub const PSK: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords";

const STAPLE_FEATURE: &str = "JobStapleAllDocuments";
const PUNCH_FEATURE: &str = "JobHolePunch";
const BIN_FEATURE: &str = "JobOutputBin";

/// An output bin, by its qualified option name: bin names are the
/// driver's own, usually in a private namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    /// The local part of the option name, as listed to the operator.
    pub name: String,
    /// The option name's namespace URI.
    pub namespace: String,
}

/// The finishing a printer's PrintCapabilities document offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Staple positions offered.
    pub staples: Vec<Staple>,
    /// Hole-punch edges offered.
    pub punch: Vec<Edge>,
    /// Output bins offered.
    pub bins: Vec<Bin>,
}

impl Staple {
    /// The public-keyword option of `JobStapleAllDocuments`.
    pub const fn print_schema(self) -> &'static str {
        match self {
            Self::TopLeft => "StapleTopLeft",
            Self::TopRight => "StapleTopRight",
            Self::BottomLeft => "StapleBottomLeft",
            Self::BottomRight => "StapleBottomRight",
            Self::DualLeft => "StapleDualLeft",
            Self::DualRight => "StapleDualRight",
            Self::DualTop => "StapleDualTop",
            Self::DualBottom => "StapleDualBottom",
            Self::Saddle => "SaddleStitch",
        }
    }
}

impl Edge {
    /// The public-keyword option of `JobHolePunch`.
    pub const fn print_schema(self) -> &'static str {
        match self {
            Self::Left => "LeftEdge",
            Self::Top => "TopEdge",
            Self::Right => "RightEdge",
            Self::Bottom => "BottomEdge",
        }
    }
}

const STAPLES: [Staple; 9] = [
    Staple::TopLeft,
    Staple::TopRight,
    Staple::BottomLeft,
    Staple::BottomRight,
    Staple::DualLeft,
    Staple::DualRight,
    Staple::DualTop,
    Staple::DualBottom,
    Staple::Saddle,
];
const EDGES: [Edge; 4] = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom];

/// Read the finishing features out of a PrintCapabilities document.
/// Features the document does not carry come back empty.
///
/// # Errors
///
/// The XML parser's message when `xml` is not well-formed.
pub fn parse_capabilities(xml: &str) -> Result<Capabilities, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut caps = Capabilities::default();
    for feature in doc.descendants().filter(|n| is_psf(n, "Feature")) {
        let Some((ns, local)) = qname(&feature, feature.attribute("name")) else {
            continue;
        };
        if ns != PSK {
            continue;
        }
        let options = feature
            .children()
            .filter(|n| is_psf(n, "Option"))
            .filter_map(|o| qname(&o, o.attribute("name")));
        match local {
            STAPLE_FEATURE => {
                caps.staples = keyword_options(options, &STAPLES, |s| s.print_schema())
            }
            PUNCH_FEATURE => caps.punch = keyword_options(options, &EDGES, |e| e.print_schema()),
            BIN_FEATURE => {
                caps.bins = options
                    .map(|(namespace, name)| Bin {
                        name: name.to_owned(),
                        namespace: namespace.to_owned(),
                    })
                    .collect();
            }
            _ => {}
        }
    }
    Ok(caps)
}

fn is_psf(node: &roxmltree::Node<'_, '_>, local: &str) -> bool {
    node.is_element() && node.tag_name().name() == local && node.tag_name().namespace() == Some(PSF)
}

/// Resolve a `prefix:local` attribute value against the node's in-scope
/// namespaces.
fn qname<'a>(node: &roxmltree::Node<'a, '_>, value: Option<&'a str>) -> Option<(&'a str, &'a str)> {
    let (prefix, local) = value?.split_once(':')?;
    Some((node.lookup_namespace_uri(Some(prefix))?, local))
}

fn keyword_options<'a, T: Copy>(
    options: impl Iterator<Item = (&'a str, &'a str)>,
    all: &[T],
    keyword: impl Fn(T) -> &'static str,
) -> Vec<T> {
    let offered: Vec<_> = options
        .filter(|(ns, _)| *ns == PSK)
        .map(|(_, l)| l)
        .collect();
    all.iter()
        .copied()
        .filter(|v| offered.contains(&keyword(*v)))
        .collect()
}

/// The delta PrintTicket asking for `f` on a printer offering `caps`, or
/// `None` when `f` asks for nothing this schema carries.
///
/// # Errors
///
/// One line per request the printer does not offer or the schema cannot
/// express (a fold, a hole count), naming it.
pub fn delta_ticket(f: &Finishing, caps: &Capabilities) -> Result<Option<String>, Vec<String>> {
    let mut refused = Vec::new();
    let mut features = String::new();
    if let Some(s) = f.staple {
        if caps.staples.contains(&s) {
            features.push_str(&feature(STAPLE_FEATURE, "psk", s.print_schema()));
        } else {
            refused.push(format!("staple {s:?}: not offered by this printer"));
        }
    }
    if let Some(e) = f.punch {
        if caps.punch.contains(&e) {
            features.push_str(&feature(PUNCH_FEATURE, "psk", e.print_schema()));
        } else {
            refused.push(format!("punch {e:?}: not offered by this printer"));
        }
    }
    if f.holes.is_some() {
        refused.push("punch holes: Windows print settings carry no hole count".to_owned());
    }
    if let Some(fold) = f.fold {
        refused.push(format!(
            "fold {fold:?}: Windows print settings have no fold types"
        ));
    }
    let mut extra_ns = String::new();
    if let Some(want) = &f.output_bin {
        match caps.bins.iter().find(|b| b.name.eq_ignore_ascii_case(want)) {
            Some(bin) => {
                let prefix = if bin.namespace == PSK { "psk" } else { "bin" };
                if prefix == "bin" {
                    extra_ns = format!(" xmlns:bin=\"{}\"", escape(&bin.namespace));
                }
                features.push_str(&feature(BIN_FEATURE, prefix, &bin.name));
            }
            None => refused.push(format!("output bin {want:?}: not offered by this printer")),
        }
    }
    if !refused.is_empty() {
        return Err(refused);
    }
    if features.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <psf:PrintTicket xmlns:psf=\"{PSF}\" xmlns:psk=\"{PSK}\"{extra_ns} version=\"1\">\
         {features}</psf:PrintTicket>"
    )))
}

fn feature(name: &str, prefix: &str, option: &str) -> String {
    format!(
        "<psf:Feature name=\"psk:{name}\"><psf:Option name=\"{prefix}:{}\"/></psf:Feature>",
        escape(option)
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}
