//! Data merge for explicit data references: `<bind match="dataRef"
//! ref="…"/>` (XFA 3.3 Ch.4 data binding, p.183 explicit data references;
//! SOM Ch.3; slices `adobe_ext__xfa_som_data_binding.md` and
//! `adobe_ext__xfa_data_binding_normal.md`).
//!
//! Scope of this cut: the root subform binds to the top data group (p.182),
//! a `dataRef` binding resolves its reference, and a subform it binds to a
//! group becomes `$` for everything under it. A container with no binding,
//! or `once`/`global`/`none` binding, passes its parent's scope through
//! (G-SOM1); normal name-matching binding and instance creation are later
//! cuts. Nothing is written to the file: the merge is a transient read of
//! the template for layout and drawing (G-SOM5).

mod data;
mod som;

use std::collections::HashSet;

use super::packets::{XdpDocument, XfaError, parse_xml};
use super::template::{ContainerKind, Template, XfaNode};

use data::{DataNode, node_at};

/// How an unqualified reference finds its first name (gap G-SOM3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ScopeSearch {
    /// The scope's children, then each ancestor's in turn (p.110).
    #[default]
    Upward,
    /// The scope's children only.
    Children,
}

/// What a `bind` with a `ref` but no `match="dataRef"` does (gap G-SOM8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BareRef {
    /// Ignored, as p.623 says.
    #[default]
    Ignore,
    /// Taken as a data reference.
    DataRef,
}

/// Whether data attributes load as values (gap G-SOM6, pp.99-100).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DataAttributes {
    /// Each non-namespaced attribute is a value, ahead of the elements.
    #[default]
    Load,
    /// Attributes are not data.
    Ignore,
}

/// The settings where XFA leaves a merge choice open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct MergeOptions {
    /// See [`ScopeSearch`].
    pub scope_search: ScopeSearch,
    /// See [`BareRef`].
    pub bare_ref: BareRef,
    /// See [`DataAttributes`].
    pub data_attributes: DataAttributes,
}

impl MergeOptions {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::scope_search`].
    #[must_use]
    pub const fn with_scope_search(mut self, v: ScopeSearch) -> Self {
        self.scope_search = v;
        self
    }

    /// Set [`Self::bare_ref`].
    #[must_use]
    pub const fn with_bare_ref(mut self, v: BareRef) -> Self {
        self.bare_ref = v;
        self
    }

    /// Set [`Self::data_attributes`].
    #[must_use]
    pub const fn with_data_attributes(mut self, v: DataAttributes) -> Self {
        self.data_attributes = v;
        self
    }
}

/// Why a reference bound nothing. The container keeps its template value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MergeWarningKind {
    /// No data node matches.
    Unresolved,
    /// The match is a group where a value is needed, or the reverse
    /// (G-SOM4: skipped).
    WrongType,
    /// SOM syntax this cut does not resolve (`..`, `#class`, predicates,
    /// `*` names), or forbidden relative indexes.
    Unsupported,
    /// More than 1,024 steps (a pdfcer guard).
    TooLong,
}

impl MergeWarningKind {
    /// The kebab-case name the CLI prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::WrongType => "wrong-type",
            Self::Unsupported => "unsupported",
            Self::TooLong => "too-long",
        }
    }
}

/// One reference that bound nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MergeWarning {
    /// The container, as child indices from the `template` element (the
    /// same convention as [`super::LayoutItem::path`]).
    pub path: Vec<usize>,
    /// Why.
    pub kind: MergeWarningKind,
    /// The reference as written.
    pub reference: String,
}

/// One field or exclusion group that took a data value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoundValue {
    /// The container, as child indices from the `template` element.
    pub path: Vec<usize>,
    /// The value it took.
    pub value: String,
}

/// A template with its data merged in.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MergedForm {
    /// The template, every bound field's `value` set. A field with no
    /// `value` element gains one as its last child, so every
    /// [`super::LayoutItem::path`] is the same as the unmerged template's.
    pub template: Template,
    /// Every value bound, in template order.
    pub bound: Vec<BoundValue>,
    /// Every reference that bound nothing, in template order.
    pub warnings: Vec<MergeWarning>,
    /// Whether the XDP had an `xfa:data` element at all.
    pub has_data: bool,
}

impl MergedForm {
    /// The value bound to the container at `path`, if one was.
    #[must_use]
    pub fn value_at(&self, path: &[usize]) -> Option<&str> {
        self.bound
            .iter()
            .find(|b| b.path == path)
            .map(|b| b.value.as_str())
    }
}

/// Merge `xdp`'s `datasets` data into `template`.
///
/// Without data every reference is reported [`MergeWarningKind::Unresolved`]
/// and the template comes back unchanged. A reference that resolves to
/// nothing never fails the merge.
///
/// # Errors
///
/// [`XfaError::Xml`] when the XDP no longer parses, [`XfaError::TooDeep`]
/// past [`super::MAX_XML_DEPTH`], [`XfaError::TooLarge`] past a million
/// data nodes.
///
/// # Examples
///
/// ```
/// use pdfcer_core::xfa::{MergeOptions, XdpDocument, merge_data, read_template};
/// let xdp = XdpDocument::parse(
///     r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">
///     <template><subform name="data"><field name="a">
///       <bind match="dataRef" ref="$.A"/></field></subform></template>
///     <xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/">
///       <xfa:data><data><A>hello</A></data></xfa:data></xfa:datasets>
///     </xdp:xdp>"#
///         .to_owned(),
/// )?;
/// let template = read_template(&xdp)?.expect("a template");
/// let merged = merge_data(&template, &xdp, &MergeOptions::new())?;
/// assert_eq!(merged.bound[0].value, "hello");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn merge_data(
    template: &Template,
    xdp: &XdpDocument,
    opts: &MergeOptions,
) -> Result<MergedForm, XfaError> {
    let doc = parse_xml(xdp.text())?;
    let data = data::read_data(&doc, opts.data_attributes)?;
    let has_data = data.is_some();
    let root = data.unwrap_or_else(|| DataNode {
        name: String::new(),
        value: String::new(),
        children: Vec::new(),
        group: true,
    });
    let record: Vec<usize> = root
        .children
        .iter()
        .position(|c| c.group)
        .into_iter()
        .collect();
    let mut m = Merger {
        root: &root,
        record: &record,
        opts,
        bound_nodes: HashSet::new(),
        bound: Vec::new(),
        warnings: Vec::new(),
    };
    let mut out = template.clone();
    let element = out.element_mut();
    let mut path = Vec::new();
    for (i, child) in element.children.iter_mut().enumerate() {
        if child.tag == "subform" {
            path.push(i);
            // The root subform binds to the top data group (p.182).
            let scope = record.clone();
            m.container(child, &mut path, &scope);
            path.pop();
            break;
        }
    }
    Ok(MergedForm {
        template: out,
        bound: m.bound,
        warnings: m.warnings,
        has_data,
    })
}

struct Merger<'a> {
    root: &'a DataNode,
    record: &'a [usize],
    opts: &'a MergeOptions,
    bound_nodes: HashSet<Vec<usize>>,
    bound: Vec<BoundValue>,
    warnings: Vec<MergeWarning>,
}

impl Merger<'_> {
    /// Merge `node` (at `path`) in data scope `scope`, then its children.
    fn container(&mut self, node: &mut XfaNode, path: &mut Vec<usize>, scope: &[usize]) {
        let kind = node.container_kind();
        let mut inner = scope.to_vec();
        if let Some(reference) = self.data_ref(node) {
            let want_value = matches!(kind, Some(ContainerKind::Field | ContainerKind::ExclGroup));
            match self.lookup(&reference, scope, want_value) {
                Ok(at) => {
                    if want_value {
                        let value = node_at(self.root, &at)
                            .map(|d| d.value.clone())
                            .unwrap_or_default();
                        set_value(node, &value);
                        self.bound.push(BoundValue {
                            path: path.clone(),
                            value,
                        });
                    } else {
                        inner.clone_from(&at);
                    }
                    self.bound_nodes.insert(at);
                }
                Err(kind) => self.warnings.push(MergeWarning {
                    path: path.clone(),
                    kind,
                    reference,
                }),
            }
        }
        if matches!(kind, Some(ContainerKind::Field | ContainerKind::Draw)) {
            return;
        }
        for (i, child) in node.children.iter_mut().enumerate() {
            if child.container_kind().is_some() {
                path.push(i);
                self.container(child, path, &inner);
                path.pop();
            }
        }
    }

    /// The reference `node`'s `bind` names, when it is a data reference.
    fn data_ref(&self, node: &XfaNode) -> Option<String> {
        let bind = node.child("bind")?;
        let reference = bind.attr("ref")?;
        let taken = match bind.attr("match") {
            Some("dataRef") => true,
            None => self.opts.bare_ref == BareRef::DataRef,
            Some(_) => false,
        };
        taken.then(|| reference.to_owned())
    }

    fn lookup(
        &self,
        reference: &str,
        scope: &[usize],
        want_value: bool,
    ) -> Result<Vec<usize>, MergeWarningKind> {
        let r = som::parse(reference)?;
        let cx = som::Context {
            root: self.root,
            scope,
            record: self.record,
            bound: &self.bound_nodes,
            search: self.opts.scope_search,
        };
        let at = som::resolve(&r, &cx).ok_or(MergeWarningKind::Unresolved)?;
        let group = node_at(self.root, &at).is_some_and(|d| d.group);
        if group == want_value {
            return Err(MergeWarningKind::WrongType);
        }
        Ok(at)
    }
}

/// Set a field's value, or each member's of an exclusion group (whose
/// members are on when their value is their on-value, p.715).
fn set_value(node: &mut XfaNode, value: &str) {
    if node.tag == "exclGroup" {
        for member in node.children.iter_mut().filter(|c| c.tag == "field") {
            set_value(member, value);
        }
        return;
    }
    if !node.children.iter().any(|c| c.tag == "value") {
        node.children.push(element("value"));
    }
    let Some(holder) = node.children.iter_mut().find(|c| c.tag == "value") else {
        return;
    };
    if holder.children.is_empty() {
        holder.children.push(element("text"));
    }
    if let Some(content) = holder.children.first_mut() {
        content.text = value.to_owned();
        content.children.clear();
    }
}

fn element(tag: &str) -> XfaNode {
    XfaNode {
        tag: tag.to_owned(),
        attributes: Vec::new(),
        text: String::new(),
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
