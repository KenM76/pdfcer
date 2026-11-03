//! View filtering by the template `relevant` attribute (XFA 3.3 pp.69-70,
//! p.607, p.919; spec RAG `adobe_ext__xfa_relevant_views.md`).
//!
//! An element excluded from the active views is removed from processing —
//! it neither binds, lays out nor draws — but stays in the file (p.69). It
//! is replaced in the [`Template`] by an inert [`XfaNode::EXCLUDED_TAG`]
//! node, so every other element keeps its template path.

use super::packets::{XdpDocument, XfaError, parse_xml};
use super::template::{Template, XfaNode};

/// How several un-prefixed view names on one element combine (gap G-RV1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ViewMatch {
    /// Every named view must be active: `relevant="print manager"` needs
    /// both, as the p.69 steps read.
    #[default]
    All,
    /// Any one named view being active is enough, as p.607's "include in
    /// that view" per name reads.
    Any,
}

/// The views a form is processed for, and how an element's `relevant`
/// list is matched against them.
///
/// With no active view every element is included, whatever its `relevant`
/// says (p.69 step 1). `print` is the system view for printing (p.308);
/// pdfcer never activates it unasked (gap G-RV4).
///
/// # Example
///
/// ```
/// use pdfcer_core::xfa::{XdpDocument, Views, read_template};
///
/// let xdp = XdpDocument::parse(
///     r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">
///     <template><subform name="f"><draw name="a" relevant="-print"/></subform></template>
///     </xdp:xdp>"#
///         .to_owned(),
/// )?;
/// let mut template = read_template(&xdp)?.expect("a template");
/// let views = Views::from_config(&xdp)?.with_view("print");
/// let excluded = template.exclude_irrelevant(&views);
/// assert_eq!(excluded, [vec![0, 0]]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Views {
    names: Vec<String>,
    matching: ViewMatch,
}

impl Views {
    /// No active view: everything is included.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The views the XDP's `config` packet activates: the text of
    /// `config/<agent>/common/template/relevant`, read from the `acrobat`
    /// agent, else `present`, else the first other agent with one (gap
    /// G-RV8); none when absent.
    ///
    /// # Errors
    ///
    /// [`XfaError::Xml`] when the XDP no longer parses, [`XfaError::TooDeep`]
    /// past [`super::MAX_XML_DEPTH`].
    pub fn from_config(xdp: &XdpDocument) -> Result<Self, XfaError> {
        let doc = parse_xml(xdp.text())?;
        let root = doc.root_element();
        let config = std::iter::once(root)
            .chain(root.children())
            .find(|n| n.is_element() && n.tag_name().name() == "config");
        let text = config.and_then(|c| {
            let agents: Vec<_> = c.children().filter(roxmltree::Node::is_element).collect();
            let named = |name: &str| agents.iter().find(|a| a.tag_name().name() == name);
            named("acrobat")
                .and_then(|a| agent_views(*a))
                .or_else(|| named("present").and_then(|a| agent_views(*a)))
                .or_else(|| agents.iter().find_map(|a| agent_views(*a)))
        });
        let mut views = Self::new();
        for name in text.as_deref().unwrap_or("").split_whitespace() {
            views = views.with_view(name.trim_start_matches(['+', '-']));
        }
        Ok(views)
    }

    /// With `name` active too.
    #[must_use]
    pub fn with_view(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if !name.is_empty() && !self.names.contains(&name) {
            self.names.push(name);
        }
        self
    }

    /// With `matching` for elements naming several views.
    #[must_use]
    pub const fn with_matching(mut self, matching: ViewMatch) -> Self {
        self.matching = matching;
        self
    }

    /// The active view names, in the order added.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// How several view names on one element combine.
    #[must_use]
    pub const fn matching(&self) -> ViewMatch {
        self.matching
    }

    /// Whether an element whose `relevant` is `attr` is included (p.69
    /// steps 1-3; a `+` prefix is the same as none, p.607).
    #[must_use]
    pub fn includes(&self, attr: &str) -> bool {
        let active = |n: &str| self.names.iter().any(|v| v == n);
        if self.names.is_empty() {
            return true;
        }
        let mut positive = attr.split_whitespace().filter(|t| !t.starts_with('-'));
        let mut negative = attr.split_whitespace().filter_map(|t| t.strip_prefix('-'));
        if negative.any(active) {
            return false;
        }
        let strip = |t: &str| t.strip_prefix('+').unwrap_or(t).to_owned();
        let names: Vec<String> = positive.by_ref().map(strip).collect();
        names.is_empty()
            || match self.matching {
                ViewMatch::All => names.iter().all(|n| active(n)),
                ViewMatch::Any => names.iter().any(|n| active(n)),
            }
    }
}

impl Template {
    /// Remove every element `views` excludes, with everything under it,
    /// from processing; return their template paths in document order.
    ///
    /// Any element carrying `relevant` is tested, `border` and `value`
    /// included (gap G-RV7). Adobe's `LegacyPlusPrint` compatibility flag
    /// is not honoured (gap G-RV9). Excluding every `pageArea` leaves
    /// nothing to lay out on, which [`super::layout_template`] reports.
    pub fn exclude_irrelevant(&mut self, views: &Views) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        if !views.names().is_empty() {
            exclude(self.element_mut(), views, &mut Vec::new(), &mut out);
        }
        out
    }
}

fn exclude(node: &mut XfaNode, views: &Views, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, child) in node.children.iter_mut().enumerate() {
        path.push(i);
        if child.attr("relevant").is_some_and(|r| !views.includes(r)) {
            *child = XfaNode {
                tag: XfaNode::EXCLUDED_TAG.to_owned(),
                attributes: Vec::new(),
                text: String::new(),
                children: Vec::new(),
            };
            out.push(path.clone());
        } else {
            exclude(child, views, path, out);
        }
        path.pop();
    }
}

/// An agent's `common/template/relevant` text.
fn agent_views(agent: roxmltree::Node<'_, '_>) -> Option<String> {
    path(agent, &["common", "template", "relevant"]).map(|n| n.text().unwrap_or("").to_owned())
}

/// The element reached from `node` through child elements named `names`.
fn path<'a, 'i>(node: roxmltree::Node<'a, 'i>, names: &[&str]) -> Option<roxmltree::Node<'a, 'i>> {
    names.iter().try_fold(node, |n, name| {
        n.children()
            .find(|c| c.is_element() && c.tag_name().name() == *name)
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)] // a panic here is the test failing
mod tests {
    use super::super::{LayoutOptions, XdpDocument, layout_template, read_template};
    use super::*;

    fn xdp(config: &str, body: &str) -> XdpDocument {
        XdpDocument::parse(format!(
            r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">{config}<template><subform name="f">
            <pageSet><pageArea name="p"><contentArea w="8in" h="10in"/></pageArea></pageSet>
            {body}</subform></template></xdp:xdp>"#
        ))
        .expect("xdp")
    }

    #[test]
    fn no_active_view_includes_everything() {
        let v = Views::new();
        assert!(v.includes("print") && v.includes("-print"));
    }

    #[test]
    fn a_minus_view_is_excluded_when_active() {
        let v = Views::new().with_view("print");
        assert!(!v.includes("-print"));
        assert!(v.includes("+print") && v.includes("print") && v.includes("-manager"));
        assert!(!v.includes("manager"), "an inactive positive view excludes");
    }

    #[test]
    fn several_positive_views_combine_by_the_setting() {
        let all = Views::new().with_view("print");
        assert!(!all.includes("print manager"), "All needs both views");
        let any = all.with_matching(ViewMatch::Any);
        assert!(any.includes("print manager"), "Any needs one");
    }

    #[test]
    fn config_views_come_from_the_acrobat_agent_first() {
        let config = r"<config><present><common><template><relevant>other</relevant></template></common></present>
            <acrobat><common><template><relevant> print  +simplex </relevant></template></common></acrobat></config>";
        let views = Views::from_config(&xdp(config, "")).expect("views");
        assert_eq!(views.names(), ["print", "simplex"]);
        assert!(
            Views::from_config(&xdp("", ""))
                .expect("views")
                .names()
                .is_empty()
        );
    }

    #[test]
    fn an_excluded_draw_takes_no_layout_space_and_keeps_sibling_paths() {
        let body = r#"<subform layout="tb" name="s">
            <draw name="a" w="1in" h="1in" relevant="-print"/>
            <draw name="b" w="1in" h="1in"/></subform>"#;
        let doc = xdp("", body);
        let mut template = read_template(&doc).expect("read").expect("template");
        let excluded = template.exclude_irrelevant(&Views::new().with_view("print"));
        assert_eq!(excluded, [vec![0, 1, 0]]);
        let layout = layout_template(&template, &LayoutOptions::new()).expect("layout");
        let b = layout.pages[0]
            .items
            .iter()
            .find(|i| i.path == [0, 1, 1])
            .expect("b keeps its path");
        assert!(
            b.rect.y.abs() < 1e-9,
            "b moves up into a's space: {}",
            b.rect.y
        );
    }
}
