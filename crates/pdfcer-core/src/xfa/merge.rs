//! Data merge for explicit data references: `<bind match="dataRef"
//! ref="…"/>` (XFA 3.3 Ch.4 data binding, p.183 explicit data references;
//! SOM Ch.3; slices `adobe_ext__xfa_som_data_binding.md` and
//! `adobe_ext__xfa_data_binding_normal.md`).
//!
//! The root subform binds to the top data group (p.182). A `dataRef`
//! binding resolves its reference; a named container with `once` (or no)
//! binding matches by name ([`normal`]); a subform either binds becomes the
//! scope for everything under it. Transparent containers pass their parent's
//! scope through (pp.193-194): nameless ones, `scope="none"` subforms,
//! `match="none"`, areas and subform sets. Exclusion-group members and page
//! sets do not bind by name. `global` binding and instance creation are later
//! cuts. Nothing is written to the file: the merge is a transient read of the
//! template for layout and drawing (G-SOM5).

mod data;
pub(super) mod fill;
mod normal;
mod som;

use std::collections::HashSet;

use super::packets::{XdpDocument, XfaError, parse_xml};
use super::template::{ContainerKind, Template, XfaNode};

use data::{DataNode, node_at};
use normal::{Found, Globals, Search};

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

/// Where a `match="global"` container that normal matching missed looks
/// for its shared value (gap G-B9, p.247).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum GlobalScope {
    /// The first same-named data value anywhere in the data, in document
    /// order. With no configured record this is the only reading under
    /// which `global` does anything.
    #[default]
    Document,
    /// Only values outside every record (the data root's own values), the
    /// strict reading of pp.245-247.
    Records,
}

/// Whether `match="global"` applies to an exclusion group (gap G-B10:
/// p.348 says yes, pp.176, 246 and 623 say fields only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum GlobalExclGroup {
    /// The exclusion group shares its value like a field.
    #[default]
    Honour,
    /// The exclusion group binds as `once`.
    AsOnce,
}

/// Whether filling an unbound field creates its data node (gaps G-B12 and
/// G-B16): XFA 3.3 creates one only by re-normalization (pp.209-210), and
/// leaves an unmatched `global` field unbound (p.247).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DataCreation {
    /// Create it, as re-normalization would: a value named as the field,
    /// appended last to the data group of its nearest bound ancestor, with
    /// the data groups of unmatched ancestor subforms inserted. A `global`
    /// field's value goes under the record, shared by every copy.
    #[default]
    Create,
    /// Refuse with [`super::XfaFillError::Unbound`].
    Refuse,
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
    /// See [`GlobalScope`].
    pub global_scope: GlobalScope,
    /// See [`GlobalExclGroup`].
    pub global_exclgroup: GlobalExclGroup,
    /// See [`DataCreation`]. Read by a fill, not by the merge.
    pub data_creation: DataCreation,
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

    /// Set [`Self::global_scope`].
    #[must_use]
    pub const fn with_global_scope(mut self, v: GlobalScope) -> Self {
        self.global_scope = v;
        self
    }

    /// Set [`Self::global_exclgroup`].
    #[must_use]
    pub const fn with_global_exclgroup(mut self, v: GlobalExclGroup) -> Self {
        self.global_exclgroup = v;
        self
    }

    /// Set [`Self::data_creation`].
    #[must_use]
    pub const fn with_data_creation(mut self, v: DataCreation) -> Self {
        self.data_creation = v;
        self
    }
}

/// A field or exclusion group that bound by name and found nothing: where
/// a fill would create its data node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unbound {
    pub(super) path: Vec<usize>,
    pub(super) name: String,
    /// The data group the new node goes under.
    pub(super) scope: Vec<usize>,
    /// Data groups to insert first, outermost first: the names of the
    /// ancestor subforms that matched nothing.
    pub(super) groups: Vec<String>,
}

/// Why a reference bound nothing, or bound with a caveat
/// ([`MergeWarningKind::MixedGlobal`]). An unbound container keeps its
/// template value.
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
    /// A `match="global"` container shares a data value that a non-global
    /// container also binds; the spec leaves the mix unsupported (p.246).
    /// The value is still shared.
    MixedGlobal,
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
            Self::MixedGlobal => "mixed-global",
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
    /// The reference as written, or the container's name when
    /// [`Self::by_name`].
    pub reference: String,
    /// The container was matching by name, not by a data reference.
    pub by_name: bool,
}

/// One field or exclusion group that took a data value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoundValue {
    /// The container, as child indices from the `template` element.
    pub path: Vec<usize>,
    /// The value it took.
    pub value: String,
    /// The data node it took it from, as child indices from `xfa:data`
    /// (attributes first when they are loaded). Bound values sharing a
    /// path share one node, so a fill through one changes them all.
    pub data: Vec<usize>,
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
    merge_planned(template, xdp, opts).map(|(form, _)| form)
}

/// [`merge_data`], plus every container a fill would create a node for.
pub(super) fn merge_planned(
    template: &Template,
    xdp: &XdpDocument,
    opts: &MergeOptions,
) -> Result<(MergedForm, Vec<Unbound>), XfaError> {
    let doc = parse_xml(xdp.text())?;
    let data = data::read_data(&doc, opts.data_attributes)?;
    let has_data = data.is_some();
    let root = data.unwrap_or_else(|| DataNode {
        name: String::new(),
        value: String::new(),
        children: Vec::new(),
        group: true,
        span: data::ValueSpan::None,
        body: data::ValueSpan::None,
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
        direct_only: false,
        passive: 0,
        page_set: 0,
        globals: Globals::default(),
        global_nodes: HashSet::new(),
        bound_nodes: HashSet::new(),
        bound: Vec::new(),
        warnings: Vec::new(),
        unbound: Vec::new(),
        record_groups: Vec::new(),
    };
    let mut out = template.clone();
    let element = out.element_mut();
    let mut path = Vec::new();
    for (i, child) in element.children.iter_mut().enumerate() {
        if child.tag == "subform" {
            m.direct_only = child.attr("mergeMode") == Some("matchTemplate");
            path.push(i);
            // The root subform binds to the top data group (p.182); with
            // no data at all, a fill creates that group.
            let scope = record.clone();
            let pending: Vec<String> = match child.name() {
                Some(n) if root.children.is_empty() => vec![n.to_owned()],
                _ => Vec::new(),
            };
            m.record_groups.clone_from(&pending);
            m.container(child, &mut path, &scope, &pending);
            path.pop();
            break;
        }
    }
    let form = MergedForm {
        template: out,
        bound: m.bound,
        warnings: m.warnings,
        has_data,
    };
    Ok((form, m.unbound))
}

struct Merger<'a> {
    root: &'a DataNode,
    record: &'a [usize],
    opts: &'a MergeOptions,
    /// The root subform's `mergeMode` is `matchTemplate`.
    direct_only: bool,
    /// How many exclusion groups enclose the container; inside one nothing
    /// binds by name.
    passive: usize,
    /// How many page sets enclose it; inside one only a `global` field binds.
    page_set: usize,
    globals: Globals,
    /// Data nodes bound by a `global` container, which may share them.
    global_nodes: HashSet<Vec<usize>>,
    bound_nodes: HashSet<Vec<usize>>,
    bound: Vec<BoundValue>,
    warnings: Vec<MergeWarning>,
    unbound: Vec<Unbound>,
    /// The record group a fill must create first (empty when it exists).
    record_groups: Vec<String>,
}

impl Merger<'_> {
    /// Merge `node` (at `path`) in data scope `scope`, then its children.
    /// `pending` names the unmatched subforms between `scope` and `node`.
    fn container(
        &mut self,
        node: &mut XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        let kind = node.container_kind();
        let mut inner = scope.to_vec();
        let mut inner_pending = pending.to_vec();
        // The root subform is bound to the record already (p.182).
        let name = if path.len() > 1 {
            self.bind_name(node, kind)
        } else {
            None
        };
        if let Some((name, global)) = name {
            let at = self.by_name(node, path, scope, (&name, global), &mut inner);
            if node.tag == "subform" {
                if at.is_some() {
                    inner_pending.clear();
                } else {
                    inner_pending.push(name);
                }
            } else if at.is_none() {
                self.unbound.push(Unbound {
                    path: path.clone(),
                    scope: if global {
                        self.record.to_vec()
                    } else {
                        scope.to_vec()
                    },
                    groups: if global {
                        self.record_groups.clone()
                    } else {
                        pending.to_vec()
                    },
                    name,
                });
            }
        } else if let Some(reference) = self.data_ref(node) {
            let want_value = matches!(kind, Some(ContainerKind::Field | ContainerKind::ExclGroup));
            match self.lookup(&reference, scope, want_value) {
                Ok(at) => {
                    if want_value {
                        self.take_value(node, path, &at);
                    } else {
                        inner.clone_from(&at);
                        inner_pending.clear();
                    }
                    self.bound_nodes.insert(at);
                }
                Err(kind) => self.warnings.push(MergeWarning {
                    path: path.clone(),
                    kind,
                    reference,
                    by_name: false,
                }),
            }
        }
        if matches!(kind, Some(ContainerKind::Field | ContainerKind::Draw)) {
            return;
        }
        let passive = kind == Some(ContainerKind::ExclGroup);
        let page_set = kind == Some(ContainerKind::PageSet);
        self.passive += usize::from(passive);
        self.page_set += usize::from(page_set);
        for (i, child) in node.children.iter_mut().enumerate() {
            if child.container_kind().is_some() {
                path.push(i);
                self.container(child, path, &inner, &inner_pending);
                path.pop();
            }
        }
        self.passive -= usize::from(passive);
        self.page_set -= usize::from(page_set);
    }

    /// The name `node` binds by, and whether it is `global`, when it binds
    /// by name: a named subform, field or exclusion group whose `bind` is
    /// absent, `once` or `global` (which tries normal matching first,
    /// p.176), outside exclusion groups. In a page set only a `global` field
    /// binds. A `dataRef` or `none` binding, a nameless container and a
    /// `scope="none"` subform do not (pp.193-194). A `global` subform binds
    /// as `once`, and so does a `global` exclusion group under
    /// [`GlobalExclGroup::AsOnce`].
    fn bind_name(&self, node: &XfaNode, kind: Option<ContainerKind>) -> Option<(String, bool)> {
        if self.passive > 0 {
            return None;
        }
        let bind = node.child("bind");
        let (by_name, global) = match bind.and_then(|b| b.attr("match")) {
            None => (
                bind.and_then(|b| b.attr("ref")).is_none() || self.opts.bare_ref == BareRef::Ignore,
                false,
            ),
            Some("once") => (true, false),
            Some("global") => (true, true),
            Some(_) => (false, false),
        };
        let global = global
            && match kind {
                Some(ContainerKind::Field) => true,
                Some(ContainerKind::ExclGroup) => {
                    self.opts.global_exclgroup == GlobalExclGroup::Honour
                }
                _ => false,
            };
        let eligible = match kind {
            Some(ContainerKind::Field | ContainerKind::ExclGroup) => true,
            Some(ContainerKind::Subform) => node.attr("scope") != Some("none"),
            _ => false,
        };
        if self.page_set > 0 && !(global && kind == Some(ContainerKind::Field)) {
            return None;
        }
        let name = node.name().filter(|n| !n.is_empty())?;
        (by_name && eligible).then(|| (name.to_owned(), global))
    }

    /// Bind `node` to the data node named `name`: normal matching, then for
    /// a `global` container the shared value (pp.245-247). A page set's
    /// global field takes the shared value only. Returns the node bound.
    fn by_name(
        &mut self,
        node: &mut XfaNode,
        path: &[usize],
        scope: &[usize],
        (name, global): (&str, bool),
        inner: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        let want_value = node.tag != "subform";
        let found = if self.page_set > 0 {
            Found::Miss
        } else {
            Search {
                root: self.root,
                record: self.record,
                bound: &self.bound_nodes,
                direct_only: self.direct_only,
            }
            .find(scope, name, want_value)
        };
        let at = match found {
            Found::Hit(at) => Some(at),
            miss if global => {
                let records_only = self.opts.global_scope == GlobalScope::Records;
                let shared = self.globals.find(self.root, name, records_only);
                if shared.is_none() && matches!(miss, Found::WrongType) {
                    self.warn(path, MergeWarningKind::WrongType, name);
                }
                shared
            }
            Found::WrongType => {
                self.warn(path, MergeWarningKind::WrongType, name);
                None
            }
            Found::Miss => None,
        };
        let at = at?;
        if global {
            if self.bound_nodes.contains(&at) && !self.global_nodes.contains(&at) {
                self.warn(path, MergeWarningKind::MixedGlobal, name);
            }
            self.global_nodes.insert(at.clone());
        }
        if want_value {
            self.take_value(node, path, &at);
        } else {
            inner.clone_from(&at);
        }
        self.bound_nodes.insert(at.clone());
        Some(at)
    }

    /// Give `node` (at `path`) the value of the data node at `at`.
    fn take_value(&mut self, node: &mut XfaNode, path: &[usize], at: &[usize]) {
        let value = node_at(self.root, at)
            .map(|d| d.value.clone())
            .unwrap_or_default();
        set_value(node, &value);
        self.bound.push(BoundValue {
            path: path.to_vec(),
            value,
            data: at.to_vec(),
        });
    }

    fn warn(&mut self, path: &[usize], kind: MergeWarningKind, name: &str) {
        self.warnings.push(MergeWarning {
            path: path.to_vec(),
            kind,
            reference: name.to_owned(),
            by_name: true,
        });
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
