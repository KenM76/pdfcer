//! Checks on the raw XML before `usvg` converts it: a nesting bound that
//! counts references (usvg's converter recurses through `use`, clip, mask,
//! pattern and filter links), and the elements usvg drops without a trace.

use std::collections::HashMap;

use usvg::roxmltree::{Document, Node};

use super::{MAX_ELEMENT_DEPTH, SvgFeature, SvgImportError, Tally};

/// Count unsupported elements into `tally` and refuse a document deeper
/// than [`MAX_ELEMENT_DEPTH`] once references are followed, before usvg
/// (which recurses) sees it.
pub(super) fn check(doc: &Document<'_>, tally: &Tally) -> Result<(), SvgImportError> {
    let elements: Vec<Node<'_, '_>> = doc.descendants().filter(Node::is_element).collect();
    let mut index = HashMap::with_capacity(elements.len());
    let mut ids = HashMap::new();
    let mut css_refs = Vec::new();
    for (i, n) in elements.iter().enumerate() {
        index.insert(n.id(), i);
        if let Some(id) = n.attribute("id") {
            ids.entry(id).or_insert(i);
        }
        match n.tag_name().name() {
            "text" => tally.add(SvgFeature::Text, 1),
            "foreignObject" => tally.add(SvgFeature::ForeignObject, 1),
            "style" => {
                for t in n.children().filter_map(|c| c.text()) {
                    css_refs.extend(url_refs(t));
                }
            }
            _ => {}
        }
    }

    let edges: Vec<Vec<usize>> = elements
        .iter()
        .map(|n| {
            let mut e: Vec<usize> = n
                .children()
                .filter_map(|c| index.get(&c.id()).copied())
                .collect();
            for a in n.attributes() {
                let v = a.value();
                if a.name() == "href"
                    && let Some(t) = v.strip_prefix('#').and_then(|id| ids.get(id))
                {
                    e.push(*t);
                }
                e.extend(url_refs(v).filter_map(|id| ids.get(id).copied()));
            }
            e
        })
        .collect();

    let heights = heights(&edges);
    let root = heights.first().copied().unwrap_or(0);
    // A stylesheet rule can attach its references to any element; each
    // target can appear at most once on one conversion path (usvg breaks
    // recursive links), so its height adds once.
    let mut css: Vec<usize> = css_refs
        .iter()
        .filter_map(|id| ids.get(id).copied())
        .collect();
    css.sort_unstable();
    css.dedup();
    let depth = css.iter().fold(root, |acc, &t| {
        acc.saturating_add(heights.get(t).copied().unwrap_or(0) + 1)
    });
    if depth > MAX_ELEMENT_DEPTH {
        return Err(SvgImportError::TooDeep {
            depth,
            limit: MAX_ELEMENT_DEPTH,
        });
    }
    Ok(())
}

/// Every `id` in `url(#id)` within `s`.
fn url_refs(s: &str) -> impl Iterator<Item = &str> {
    s.split("url(").skip(1).filter_map(|rest| {
        let rest = rest.trim_start().trim_start_matches(['"', '\'']);
        let rest = rest.strip_prefix('#')?;
        let end = rest.find([')', '"', '\'', ' ']).unwrap_or(rest.len());
        rest.get(..end)
    })
}

/// Longest edge path from each node, iteratively; an edge back into the
/// path being explored counts zero (usvg drops recursive links).
// Every edge target is a position in the node list `edges` was built from,
// so each index is below `edges.len()`.
#[allow(clippy::indexing_slicing)]
fn heights(edges: &[Vec<usize>]) -> Vec<usize> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; edges.len()];
    let mut height = vec![1usize; edges.len()];
    for start in 0..edges.len() {
        if state[start] != NEW {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        state[start] = OPEN;
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            if let Some(&child) = edges[node].get(*next) {
                *next += 1;
                match state[child] {
                    NEW => {
                        state[child] = OPEN;
                        stack.push((child, 0));
                    }
                    DONE => height[node] = height[node].max(height[child].saturating_add(1)),
                    _ => {}
                }
            } else {
                state[node] = DONE;
                stack.pop();
                if let Some(&(parent, _)) = stack.last() {
                    height[parent] = height[parent].max(height[node].saturating_add(1));
                }
            }
        }
    }
    height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_count_references_and_tolerate_cycles() {
        // 0 -> 1 -> 2, 2 -> 0 (cycle), 3 isolated.
        let h = heights(&[vec![1], vec![2], vec![0], vec![]]);
        assert_eq!(h, vec![3, 2, 1, 1]);
    }

    #[test]
    fn url_refs_parse_quoted_and_bare() {
        let v: Vec<&str> = url_refs("fill:url(#a);clip-path: url('#b')").collect();
        assert_eq!(v, ["a", "b"]);
    }
}
