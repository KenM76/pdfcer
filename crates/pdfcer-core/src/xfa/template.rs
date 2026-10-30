//! The XFA `template` packet as a read-only tree (XFA 3.3 Ch.2, Ch.7, Ch.17).
//!
//! Every element is kept, modelled or not, as an [`XfaNode`] carrying its
//! local name, attributes, direct text and children, so later layout passes
//! read what the file says rather than what this module chose to keep. Typed
//! accessors give the layout-relevant attributes with the Ch.17 defaults
//! (`adobe_ext__xfa_template_layout_dom.md` in the spec RAG); an omitted size
//! stays `None` and is never read as zero (gap G-T2).

use super::packets::{MAX_XML_DEPTH, XdpDocument, XfaError, parse_xml};

/// The template packet: the `template` element and everything under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    root: XfaNode,
}

impl Template {
    /// The `template` element itself.
    #[must_use]
    pub fn element(&self) -> &XfaNode {
        &self.root
    }

    /// The root subform: the template's first `subform` child, where the form
    /// and its `pageSet` begin (Ch.7 p.250).
    #[must_use]
    pub fn root_subform(&self) -> Option<&XfaNode> {
        self.root.child("subform")
    }

    /// The `template` element, for a merge to set field values in.
    pub(super) fn element_mut(&mut self) -> &mut XfaNode {
        &mut self.root
    }
}

/// One template element.
#[derive(Debug, Clone, PartialEq)]
pub struct XfaNode {
    /// Local name (`subform`, `field`, `margin`, ...).
    pub tag: String,
    /// Attributes in document order, by local name.
    pub attributes: Vec<(String, String)>,
    /// The element's own text, its direct text children joined.
    pub text: String,
    /// Child elements in document order (Z-order, p.70).
    pub children: Vec<XfaNode>,
}

/// What a template element is to layout (Ch.7 p.251).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContainerKind {
    /// `subform`: lays out its children by its `layout` strategy.
    Subform,
    /// `subformSet`: groups subforms for merge; not geographic.
    SubformSet,
    /// `area`: grows to hold children positioned relative to it.
    Area,
    /// `exclGroup`: a radio or check-box group.
    ExclGroup,
    /// `field`: an interactive leaf bound to a data value.
    Field,
    /// `draw`: static boilerplate.
    Draw,
    /// `pageSet`: an ordered set of display surfaces.
    PageSet,
    /// `pageArea`: one display surface.
    PageArea,
    /// `contentArea`: a fixed region of a `pageArea` that content flows into.
    ContentArea,
}

/// A container's `layout` strategy (Ch.17 p.841). Default `Position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LayoutStrategy {
    /// Children placed by their own `x`/`y`.
    #[default]
    Position,
    /// Top to bottom.
    Tb,
    /// Left to right, wrapping top to bottom.
    LrTb,
    /// Right to left, wrapping top to bottom.
    RlTb,
    /// A table row, left to right (`row`; gap G-T4 reads it as LTR).
    Row,
    /// A table row, right to left.
    RlRow,
    /// A table of `row` subforms.
    Table,
}

/// Which point of a container its `x`/`y` name (Ch.17). Default `TopLeft`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
#[allow(missing_docs)] // the nine names are their own description
pub enum AnchorType {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// `presence` (Ch.2 pp.67-68). Default `Visible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Presence {
    /// Laid out and drawn.
    #[default]
    Visible,
    /// Takes layout space, not drawn.
    Invisible,
    /// Absent from layout.
    Hidden,
    /// Binding only; calculations and events suppressed.
    Inactive,
}

/// An XFA measurement (Ch.2 pp.36-37).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Measurement {
    /// An absolute length, converted to points (1/72 in). No unit means
    /// inches.
    Points(f64),
    /// `em`, relative to the font size (XFA 2.8).
    Em(f64),
    /// `%`, relative to whatever the property names (XFA 2.8).
    Percent(f64),
}

impl Measurement {
    /// Parse `[sign] number [unit]`; `None` for an unknown unit or no number.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let split = s
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let v: f64 = num.parse().ok().filter(|v: &f64| v.is_finite())?;
        Some(match unit.trim() {
            "" | "in" => Self::Points(v * 72.0),
            "pt" => Self::Points(v),
            "cm" => Self::Points(v * 72.0 / 2.54),
            "mm" => Self::Points(v * 72.0 / 25.4),
            "em" => Self::Em(v),
            "%" => Self::Percent(v),
            _ => return None,
        })
    }

    /// The length in points, when absolute.
    #[must_use]
    pub const fn points(self) -> Option<f64> {
        match self {
            Self::Points(v) => Some(v),
            _ => None,
        }
    }
}

/// `occur` bounds (Ch.17 p.786; gap G-T3 resolved to the min-derived rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Occur {
    /// Fewest instances.
    pub min: u32,
    /// Most instances; `None` is unbounded (`-1`).
    pub max: Option<u32>,
    /// Instances in an empty merge.
    pub initial: u32,
}

impl XfaNode {
    /// The attribute named `name`, if present.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first child element named `tag`.
    #[must_use]
    pub fn child(&self, tag: &str) -> Option<&XfaNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// The child elements named `tag`.
    pub fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a XfaNode> {
        self.children.iter().filter(move |c| c.tag == tag)
    }

    /// The `name` attribute.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.attr("name")
    }

    /// The container kind, when this element is one.
    #[must_use]
    pub fn container_kind(&self) -> Option<ContainerKind> {
        Some(match self.tag.as_str() {
            "subform" => ContainerKind::Subform,
            "subformSet" => ContainerKind::SubformSet,
            "area" => ContainerKind::Area,
            "exclGroup" => ContainerKind::ExclGroup,
            "field" => ContainerKind::Field,
            "draw" => ContainerKind::Draw,
            "pageSet" => ContainerKind::PageSet,
            "pageArea" => ContainerKind::PageArea,
            "contentArea" => ContainerKind::ContentArea,
            _ => return None,
        })
    }

    /// A measurement attribute (`x`, `y`, `w`, `h`, `minW`, ...). `None` when
    /// absent or unparseable. `maxW`/`maxH` of zero read as absent (XFA 3.0).
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<Measurement> {
        let m = Measurement::parse(self.attr(name)?)?;
        let unbounded_zero = matches!(name, "maxW" | "maxH") && m.points() == Some(0.0);
        (!unbounded_zero).then_some(m)
    }

    /// `layout`; unknown or absent reads as the default.
    #[must_use]
    pub fn layout(&self) -> LayoutStrategy {
        match self.attr("layout") {
            Some("tb") => LayoutStrategy::Tb,
            Some("lr-tb") => LayoutStrategy::LrTb,
            Some("rl-tb") => LayoutStrategy::RlTb,
            Some("row") => LayoutStrategy::Row,
            Some("rl-row") => LayoutStrategy::RlRow,
            Some("table") => LayoutStrategy::Table,
            _ => LayoutStrategy::Position,
        }
    }

    /// `anchorType`; unknown or absent reads as the default.
    #[must_use]
    pub fn anchor_type(&self) -> AnchorType {
        match self.attr("anchorType") {
            Some("topCenter") => AnchorType::TopCenter,
            Some("topRight") => AnchorType::TopRight,
            Some("middleLeft") => AnchorType::MiddleLeft,
            Some("middleCenter") => AnchorType::MiddleCenter,
            Some("middleRight") => AnchorType::MiddleRight,
            Some("bottomLeft") => AnchorType::BottomLeft,
            Some("bottomCenter") => AnchorType::BottomCenter,
            Some("bottomRight") => AnchorType::BottomRight,
            _ => AnchorType::TopLeft,
        }
    }

    /// `presence`; unknown or absent reads as the default.
    #[must_use]
    pub fn presence(&self) -> Presence {
        match self.attr("presence") {
            Some("invisible") => Presence::Invisible,
            Some("hidden") => Presence::Hidden,
            Some("inactive") => Presence::Inactive,
            _ => Presence::Visible,
        }
    }

    /// `colSpan`, default 1; `-1` (span the rest of the row) reads as `None`.
    #[must_use]
    pub fn col_span(&self) -> Option<u32> {
        match self.attr("colSpan").map(str::trim) {
            None => Some(1),
            Some(v) => v.parse::<i64>().ok().and_then(|n| u32::try_from(n).ok()),
        }
    }

    /// This container's `occur` child, with the container-specific defaults:
    /// subforms default to 1/min/min, page areas and page sets to 0/unbounded
    /// (Ch.17 p.786). `None` for an element that is neither.
    #[must_use]
    pub fn occur(&self) -> Option<Occur> {
        let page = match self.container_kind()? {
            ContainerKind::PageArea | ContainerKind::PageSet => true,
            ContainerKind::Subform | ContainerKind::SubformSet => false,
            _ => return None,
        };
        let o = self.child("occur");
        let int = |k: &str| {
            o.and_then(|o| o.attr(k))
                .and_then(|v| v.trim().parse::<i64>().ok())
        };
        let min = int("min")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(u32::from(!page));
        let max = match int("max") {
            Some(-1) => None,
            Some(v) => Some(u32::try_from(v).unwrap_or(min)),
            None if page => None,
            None => Some(min),
        };
        let initial = int("initial")
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(min);
        Some(Occur { min, max, initial })
    }
}

/// Read the template packet of `xdp`. `Ok(None)` when there is none.
///
/// The packet is found by its element's local name among the children of
/// `xdp:xdp` (or as the document root), as [`XdpDocument::packets`] does.
///
/// # Errors
///
/// [`XfaError::Xml`] when the XDP no longer parses, [`XfaError::TooDeep`]
/// past [`MAX_XML_DEPTH`].
pub fn read_template(xdp: &XdpDocument) -> Result<Option<Template>, XfaError> {
    let doc = parse_xml(xdp.text())?;
    let root = doc.root_element();
    let element = if root.tag_name().name() == "template" {
        Some(root)
    } else if root.tag_name().name() == "xdp" {
        root.children()
            .find(|n| n.is_element() && n.tag_name().name() == "template")
    } else {
        None
    };
    element
        .map(|e| convert(e, 0).map(|root| Template { root }))
        .transpose()
}

fn convert(node: roxmltree::Node<'_, '_>, depth: usize) -> Result<XfaNode, XfaError> {
    if depth > MAX_XML_DEPTH {
        return Err(XfaError::TooDeep);
    }
    let mut out = XfaNode {
        tag: node.tag_name().name().to_owned(),
        attributes: node
            .attributes()
            .map(|a| (a.name().to_owned(), a.value().to_owned()))
            .collect(),
        text: String::new(),
        children: Vec::new(),
    };
    for child in node.children() {
        if child.is_element() {
            out.children.push(convert(child, depth + 1)?);
        } else if let Some(t) = child.text().filter(|_| child.is_text()) {
            out.text.push_str(t);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
