//! Ordered-occurrence page selection over a `pageSet` tree (XFA 3.3 Ch.8
//! pp.298-302).

use crate::xfa::XfaNode;

/// A `pageSet` being walked.
struct Frame<'t> {
    node: &'t XfaNode,
    path: Vec<usize>,
    next: usize,
    passes: u32,
    yielded: bool,
}

/// The `pageArea` the next page is an instance of.
#[derive(Clone)]
pub(super) struct Area<'t> {
    pub(super) node: &'t XfaNode,
    pub(super) path: Vec<usize>,
}

/// Walks page areas in document order, consuming each one's `occur@max`
/// instances, then ascending; a `pageSet` repeats while its own `occur@max`
/// allows and its last pass produced a page. Never leaves the root
/// `pageSet`.
pub(super) struct Pager<'t> {
    stack: Vec<Frame<'t>>,
    current: Option<Area<'t>>,
    used: u32,
}

impl<'t> Pager<'t> {
    /// A pager over the root `pageSet` `root` at `path`.
    pub(super) fn new(root: &'t XfaNode, path: Vec<usize>) -> Self {
        Self {
            stack: vec![Frame {
                node: root,
                path,
                next: 0,
                passes: 0,
                yielded: false,
            }],
            current: None,
            used: 0,
        }
    }

    /// Instances of the current area so far.
    pub(super) const fn used(&self) -> u32 {
        self.used
    }

    /// The next page instance; `None` once the tree is exhausted.
    /// `max_of` is the effective `occur@max` of an area (`None` unbounded).
    pub(super) fn advance(
        &mut self,
        max_of: &mut dyn FnMut(&Area<'t>) -> Option<u32>,
    ) -> Option<Area<'t>> {
        if let Some(a) = &self.current
            && max_of(a).is_none_or(|m| self.used < m)
        {
            self.used += 1;
            return Some(a.clone());
        }
        let a = self.next_area(max_of)?;
        self.current = Some(a.clone());
        self.used = 1;
        Some(a)
    }

    /// Make `node` at `path` (a `pageArea` in this tree) the current area as
    /// a fresh instance, continuing in document order after it.
    pub(super) fn jump(&mut self, root: &'t XfaNode, path: &[usize]) {
        let base = self.stack.first().map(|f| f.path.len()).unwrap_or(0);
        let rel = path.get(base..).unwrap_or_default();
        let mut node = root;
        let mut frames = Vec::new();
        for (depth, &i) in rel.iter().enumerate() {
            let p = path.get(..base + depth).unwrap_or_default().to_vec();
            let passes = self
                .stack
                .iter()
                .find(|f| f.path == p)
                .map_or(0, |f| f.passes);
            frames.push(Frame {
                node,
                path: p,
                next: i + 1,
                passes,
                yielded: true,
            });
            let Some(c) = node.children.get(i) else {
                return;
            };
            node = c;
        }
        self.stack = frames;
        self.current = Some(Area {
            node,
            path: path.to_vec(),
        });
        self.used = 1;
    }

    /// Page areas still ahead in the current pass of every open `pageSet`,
    /// without starting another pass: the candidates for `occur@min`
    /// padding when content ends.
    pub(super) fn rest_of_pass(&self) -> Vec<Area<'t>> {
        let mut out = Vec::new();
        for f in self.stack.iter().rev() {
            for (i, c) in f.node.children.iter().enumerate().skip(f.next) {
                if c.tag == "pageArea" {
                    let mut path = f.path.clone();
                    path.push(i);
                    out.push(Area { node: c, path });
                }
            }
        }
        out
    }

    fn next_area(&mut self, max_of: &mut dyn FnMut(&Area<'t>) -> Option<u32>) -> Option<Area<'t>> {
        loop {
            let top = self.stack.last_mut()?;
            let found = top
                .node
                .children
                .iter()
                .enumerate()
                .skip(top.next)
                .find(|(_, c)| matches!(c.tag.as_str(), "pageArea" | "pageSet"));
            if let Some((i, c)) = found {
                top.next = i + 1;
                let mut path = top.path.clone();
                path.push(i);
                if c.tag == "pageArea" {
                    let a = Area { node: c, path };
                    if max_of(&a) != Some(0) {
                        top.yielded = true;
                        return Some(a);
                    }
                    continue;
                }
                if c.occur().and_then(|o| o.max) != Some(0) {
                    self.stack.push(Frame {
                        node: c,
                        path,
                        next: 0,
                        passes: 0,
                        yielded: false,
                    });
                }
                continue;
            }
            top.passes += 1;
            let max = top.node.occur().and_then(|o| o.max);
            if top.yielded && max.is_none_or(|m| top.passes < m) {
                top.next = 0;
                top.yielded = false;
                continue;
            }
            let done = self.stack.pop();
            if let (Some(done), Some(parent)) = (done, self.stack.last_mut()) {
                parent.yielded |= done.yielded;
            }
        }
    }
}
