//! `bindItems` (XFA 3.3 p.212, p.624, p.726; spec RAG
//! `adobe_ext__xfa_binditems.md`): a `choiceList` or `checkButton` field's
//! items taken from the data. `ref` names the data nodes, evaluated from the
//! field's own bound data node; each node makes one item, its value from
//! `valueRef` and its label from `labelRef` (the value when absent). The
//! bound items replace the template's `items`. Evaluated once, at merge
//! (the live binding renders the same with no script). A `connection`
//! source is not read.

use super::data::{DataNode, node_at};
use super::som::{self, Context};
use super::{MergeWarning, MergeWarningKind, Merger, ScopeSearch};
use crate::xfa::template::XfaNode;

/// Which of several `bindItems` on one field applies (gap G-BI1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BindItemsRepeat {
    /// The last: each replaces the list the one before made.
    #[default]
    Last,
    /// The first; the rest are ignored.
    First,
}

/// An item whose `valueRef` or `labelRef` finds no data node (gap G-BI4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BindItemsMissing {
    /// Kept, with an empty string for what was not found.
    #[default]
    Empty,
    /// Left out of the list.
    Skip,
}

/// Where a `bindItems` `ref` is evaluated when its field bound no data
/// node (gap G-BI5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BindItemsUnbound {
    /// The data scope the field merged in.
    #[default]
    Scope,
    /// Nowhere: the template's items stay.
    Ignore,
}

/// A checkButton takes at most its on, off and neutral values (p.715).
const CHECK_STATES: usize = 3;

impl Merger<'_> {
    /// Apply `node`'s `bindItems` (at `path`, merged in data scope `scope`).
    pub(super) fn bind_items(&mut self, node: &mut XfaNode, path: &[usize], scope: &[usize]) {
        let ui = node
            .child("ui")
            .and_then(|u| u.children.iter().find(|c| c.tag != "extras"))
            .map(|w| w.tag.clone());
        let check = match ui.as_deref() {
            Some("choiceList") => false,
            Some("checkButton") => true,
            _ => return,
        };
        let binds: Vec<XfaNode> = node.children_named("bindItems").cloned().collect();
        let chosen = match self.opts.bind_items_repeat {
            BindItemsRepeat::Last => binds.last(),
            BindItemsRepeat::First => binds.first(),
        };
        let Some(bind) = chosen else {
            return;
        };
        let own = self
            .bound
            .iter()
            .rev()
            .find(|b| b.path == path)
            .map(|b| b.data.clone());
        let context = match (own, self.opts.bind_items_unbound) {
            (Some(at), _) => at,
            (None, BindItemsUnbound::Scope) => scope.to_vec(),
            (None, BindItemsUnbound::Ignore) => return,
        };
        let Some(mut pairs) = self.items_from(bind, path, &context) else {
            return;
        };
        if check {
            pairs.truncate(CHECK_STATES);
            pairs.iter_mut().for_each(|p| p.1.clone_from(&p.0));
        }
        replace_items(node, &pairs);
    }

    /// The (value, label) pairs `bind` yields from `context`; `None` when
    /// it is not evaluated (a `connection`, or a `ref` that is missing or
    /// cannot be parsed), which leaves the template's items.
    fn items_from(
        &mut self,
        bind: &XfaNode,
        path: &[usize],
        context: &[usize],
    ) -> Option<Vec<(String, String)>> {
        let reference = bind.attr("ref").unwrap_or("").trim();
        let parsed = if bind.attr("connection").is_some_and(|c| !c.is_empty()) {
            Err(MergeWarningKind::Unsupported)
        } else if reference.is_empty() {
            Err(MergeWarningKind::Unresolved)
        } else {
            som::parse(reference)
        };
        let r = match parsed {
            Ok(r) => r,
            Err(kind) => {
                self.item_warning(path, kind, reference);
                return None;
            }
        };
        let nodes = som::resolve_all(&r, &self.context(context, self.opts.scope_search));
        if nodes.is_empty() {
            self.item_warning(path, MergeWarningKind::Unresolved, reference);
        }
        let value_ref = bind.attr("valueRef").filter(|v| !v.trim().is_empty());
        let label_ref = bind.attr("labelRef").filter(|v| !v.trim().is_empty());
        let mut out = Vec::new();
        for at in nodes {
            let value = self.sub_value(&at, value_ref);
            let label = match label_ref {
                Some(_) => self.sub_value(&at, label_ref),
                None => value.clone(),
            };
            match (value, label) {
                (Some(v), Some(l)) => out.push((v, l)),
                _ if self.opts.bind_items_missing == BindItemsMissing::Skip => {}
                (v, l) => out.push((v.unwrap_or_default(), l.unwrap_or_default())),
            }
        }
        Some(out)
    }

    /// The value `sub` names from the item node at `at`, or the node's own
    /// value when `sub` is absent (gap G-BI3).
    fn sub_value(&self, at: &[usize], sub: Option<&str>) -> Option<String> {
        let target = match sub {
            None => at.to_vec(),
            Some(text) => {
                let r = som::parse(text).ok()?;
                som::resolve(&r, &self.context(at, self.opts.bind_items_search))?
            }
        };
        node_at(self.root, &target).map(|d: &DataNode| d.value.clone())
    }

    fn context<'c>(&'c self, scope: &'c [usize], search: ScopeSearch) -> Context<'c> {
        Context {
            root: self.root,
            scope,
            record: self.record,
            bound: &self.bound_nodes,
            search,
        }
    }

    fn item_warning(&mut self, path: &[usize], kind: MergeWarningKind, reference: &str) {
        self.warnings.push(MergeWarning {
            path: path.to_vec(),
            kind,
            reference: reference.to_owned(),
            by_name: false,
        });
    }
}

/// Replace `node`'s `items` with `pairs`: one list when every label is its
/// value, else labels in a display list and values in a `save="1"` one,
/// paired by position (p.485; gap G-BI2).
fn replace_items(node: &mut XfaNode, pairs: &[(String, String)]) {
    let list = |texts: Vec<&String>, save: bool| XfaNode {
        tag: "items".to_owned(),
        attributes: if save {
            vec![("save".to_owned(), "1".to_owned())]
        } else {
            Vec::new()
        },
        text: String::new(),
        children: texts
            .into_iter()
            .map(|t| XfaNode {
                tag: "text".to_owned(),
                attributes: Vec::new(),
                text: t.clone(),
                children: Vec::new(),
            })
            .collect(),
    };
    let at = node.children.iter().position(|c| c.tag == "items");
    node.children.retain(|c| c.tag != "items");
    let at = at.unwrap_or(node.children.len()).min(node.children.len());
    let mut lists = vec![list(pairs.iter().map(|p| &p.1).collect(), false)];
    if pairs.iter().any(|(v, l)| v != l) {
        lists.push(list(pairs.iter().map(|p| &p.0).collect(), true));
    }
    node.children.splice(at..at, lists);
}
