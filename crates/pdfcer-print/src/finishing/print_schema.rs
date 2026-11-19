//! Finishing in the Windows Print Schema: the staple, hole-punch and
//! output-bin features of a printer's PrintCapabilities document, job-,
//! document- or page-wide, and the delta PrintTicket that asks for them
//! (Microsoft Print Schema Specification, public keywords). The schema has
//! no fold types and no hole count, so those are refused.

use super::{Edge, Finishing, Staple};

/// The Print Schema framework namespace.
pub const PSF: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework";
/// The Print Schema public keywords namespace.
pub const PSK: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords";

/// The features a choice may be listed under, searched in order; many
/// drivers list only the document- or page-wide one. A choice is asked for
/// under the first feature that offers it.
const STAPLE_FEATURES: &[&str] = &["JobStapleAllDocuments", "DocumentStaple"];
const PUNCH_FEATURES: &[&str] = &["JobHolePunch", "DocumentHolePunch"];
const BIN_FEATURES: &[&str] = &["JobOutputBin", "DocumentOutputBin", "PageOutputBin"];

/// An output bin, by its qualified option name: bin names are the
/// driver's own, usually in a private namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    /// The local part of the option name, as listed to the operator.
    pub name: String,
    /// The option name's namespace URI.
    pub namespace: String,
    feature: &'static str,
}

impl Bin {
    /// The public-keyword feature the bin is listed under, such as
    /// `JobOutputBin` or `PageOutputBin`.
    pub const fn feature(&self) -> &'static str {
        self.feature
    }
}

/// The finishing a printer's PrintCapabilities document offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Staple positions offered.
    pub staples: Vec<Staple>,
    /// Hole-punch edges offered.
    pub punch: Vec<Edge>,
    /// Output bins offered, each name once, under the first feature that
    /// lists it.
    pub bins: Vec<Bin>,
    staple_from: Vec<(Staple, &'static str)>,
    punch_from: Vec<(Edge, &'static str)>,
    listed: Vec<(&'static str, String, String)>,
}

impl Capabilities {
    /// Whether `feature` lists the option `namespace`:`local` (the local
    /// name without case). This is how a merged ticket is checked: it may
    /// carry the same option under more than one feature.
    pub fn lists(&self, feature: &str, namespace: &str, local: &str) -> bool {
        self.listed
            .iter()
            .any(|(f, ns, l)| *f == feature && ns == namespace && l.eq_ignore_ascii_case(local))
    }

    /// The feature `s` is offered under, if it is offered.
    pub fn staple_feature(&self, s: Staple) -> Option<&'static str> {
        self.staple_from
            .iter()
            .find(|(v, _)| *v == s)
            .map(|(_, f)| *f)
    }

    /// The feature `e` is offered under, if it is offered.
    pub fn punch_feature(&self, e: Edge) -> Option<&'static str> {
        self.punch_from
            .iter()
            .find(|(v, _)| *v == e)
            .map(|(_, f)| *f)
    }
}

impl Staple {
    /// The public-keyword option of a staple feature.
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
    /// The public-keyword option of a hole-punch feature.
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

/// Each feature in `names`, with its options as (namespace, local name).
type Listed = Vec<(&'static str, Vec<(String, String)>)>;

/// Read the finishing features out of a PrintCapabilities document, or out
/// of a PrintTicket (one option per feature). Features the document does
/// not carry come back empty.
///
/// # Errors
///
/// The XML parser's message when `xml` is not well-formed.
pub fn parse_capabilities(xml: &str) -> Result<Capabilities, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let listed = |names: &[&'static str]| -> Listed {
        names
            .iter()
            .map(|&name| (name, options_of(&doc, name)))
            .collect()
    };
    let (staples, punches, outputs) = (
        listed(STAPLE_FEATURES),
        listed(PUNCH_FEATURES),
        listed(BIN_FEATURES),
    );
    let staple_from = keyword_options(&staples, &STAPLES, Staple::print_schema);
    let punch_from = keyword_options(&punches, &EDGES, Edge::print_schema);
    let all = staples.iter().chain(&punches).chain(&outputs);
    let listed = all
        .flat_map(|(f, opts)| opts.iter().map(|(ns, l)| (*f, ns.clone(), l.clone())))
        .collect();
    let mut bins: Vec<Bin> = Vec::new();
    for (feature, options) in outputs {
        for (namespace, name) in options {
            if !bins.iter().any(|b| b.name.eq_ignore_ascii_case(&name)) {
                bins.push(Bin {
                    name,
                    namespace,
                    feature,
                });
            }
        }
    }
    Ok(Capabilities {
        staples: staple_from.iter().map(|(s, _)| *s).collect(),
        punch: punch_from.iter().map(|(e, _)| *e).collect(),
        bins,
        staple_from,
        punch_from,
        listed,
    })
}

/// The options of every public-keyword feature `name` in `doc`.
fn options_of(doc: &roxmltree::Document<'_>, name: &str) -> Vec<(String, String)> {
    doc.descendants()
        .filter(|n| is_psf(n, "Feature"))
        .filter(|f| qname(f, f.attribute("name")) == Some((PSK, name)))
        .flat_map(|f| f.children().filter(|n| is_psf(n, "Option")))
        .filter_map(|o| qname(&o, o.attribute("name")))
        .map(|(ns, local)| (ns.to_owned(), local.to_owned()))
        .collect()
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

/// Each of `all` some feature offers as a public keyword, with the first
/// such feature in `listed` order.
fn keyword_options<T: Copy>(
    listed: &Listed,
    all: &[T],
    keyword: impl Fn(T) -> &'static str,
) -> Vec<(T, &'static str)> {
    all.iter()
        .filter_map(|&v| {
            listed
                .iter()
                .find(|(_, opts)| opts.iter().any(|(ns, l)| ns == PSK && l == keyword(v)))
                .map(|(feature, _)| (v, *feature))
        })
        .collect()
}

/// The delta PrintTicket asking for `f` on a printer offering `caps`, or
/// `None` when `f` asks for nothing this schema carries. Each choice is
/// asked for under the feature `caps` offers it in.
///
/// # Errors
///
/// One line per request the printer does not offer or the schema cannot
/// express (a fold, a hole count), naming it.
pub fn delta_ticket(f: &Finishing, caps: &Capabilities) -> Result<Option<String>, Vec<String>> {
    let mut refused = Vec::new();
    let mut features = String::new();
    if let Some(s) = f.staple {
        match caps.staple_feature(s) {
            Some(name) => features.push_str(&feature(name, "psk", s.print_schema())),
            None => refused.push(format!("staple {s:?}: not offered by this printer")),
        }
    }
    if let Some(e) = f.punch {
        match caps.punch_feature(e) {
            Some(name) => features.push_str(&feature(name, "psk", e.print_schema())),
            None => refused.push(format!("punch {e:?}: not offered by this printer")),
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
                features.push_str(&feature(bin.feature, prefix, &bin.name));
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

/// The requests the merged ticket no longer carries under the feature
/// the delta asked in: a driver resolves a conflict by substituting, and
/// reports success when it does. `caps` is what the delta was built
/// from, `kept` the merged ticket read back.
pub fn dropped(f: &Finishing, caps: &Capabilities, kept: &Capabilities) -> Vec<String> {
    let mut out = Vec::new();
    let kept_under = |feature: Option<&str>, option: &str| {
        feature.is_some_and(|name| kept.lists(name, PSK, option))
    };
    if let Some(s) = f
        .staple
        .filter(|&s| !kept_under(caps.staple_feature(s), s.print_schema()))
    {
        out.push(format!(
            "staple {s:?}: the driver substituted another setting"
        ));
    }
    if let Some(e) = f
        .punch
        .filter(|&e| !kept_under(caps.punch_feature(e), e.print_schema()))
    {
        out.push(format!(
            "punch {e:?}: the driver substituted another setting"
        ));
    }
    let asked = |b: &str| caps.bins.iter().find(|c| c.name.eq_ignore_ascii_case(b));
    if let Some(b) = &f.output_bin
        && !asked(b).is_some_and(|c| kept.lists(c.feature(), &c.namespace, &c.name))
    {
        out.push(format!(
            "output bin {b:?}: the driver substituted another bin"
        ));
    }
    out
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
