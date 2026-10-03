//! The occurrence walk that turns the trees into placements.

use super::*;

/// Where an occurrence sits: its depth, its parent node and whether its
/// ancestors are drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct At {
    pub(crate) depth: usize,
    pub(crate) parent: Option<usize>,
    pub(crate) drawn: bool,
}

impl At {
    /// A root occurrence.
    pub(crate) const ROOT: At = At {
        depth: 0,
        parent: None,
        drawn: true,
    };
}

/// An occurrence's part and sons as `(file structure, value)`, after the
/// prototype fallback.
struct Resolved<'t> {
    part: (usize, u32),
    sons: (usize, &'t [u32]),
    proto_name: Option<&'t str>,
}

/// The occurrence walk over every file structure's tree [WD 7.3.10.1,
/// 7.6.3.2; `prc__8137__model_tree_asm.md` §9].
pub(crate) struct Walk<'t> {
    /// Per file structure: its id, tree and globals.
    pub(crate) trees: Vec<(UniqueId, &'t Tree, &'t Globals)>,
    pub(crate) out: Vec<Placement>,
    pub(crate) nodes: Vec<ModelNode>,
    visits: usize,
    palettes: Palettes,
}

impl<'t> Walk<'t> {
    /// A walk over `trees`, with no placements yet, colouring by `rule`.
    pub(crate) fn new(trees: Vec<(UniqueId, &'t Tree, &'t Globals)>, rule: StyleAlpha) -> Self {
        let palettes = trees
            .iter()
            .map(|(_, _, g)| {
                (0..=g.styles.len() as u32)
                    .map(|b| g.style_colour(b, rule))
                    .collect()
            })
            .collect();
        Walk {
            trees,
            out: Vec::new(),
            nodes: Vec::new(),
            visits: 0,
            palettes,
        }
    }

    fn fs(&self, id: UniqueId) -> Result<usize, PrcError> {
        self.trees
            .iter()
            .position(|t| t.0 == id)
            .ok_or_else(|| malformed("reference to an unknown file structure".into()))
    }

    fn product(&self, fs: usize, index: usize) -> Result<&'t Product, PrcError> {
        self.trees
            .get(fs)
            .and_then(|t| t.1.products.get(index))
            .ok_or_else(|| malformed(format!("product occurrence {index} does not exist")))
    }

    /// Draws occurrence `index` of structure `fs` under `father`, whose
    /// occurrences' graphics are `graphics`, root first, and lists it as a
    /// node. A hidden or suppressed subtree is listed but not drawn.
    pub(crate) fn occurrence(
        &mut self,
        fs: usize,
        index: usize,
        father: &Matrix,
        graphics: &[Graphics],
        at: At,
    ) -> Result<(), PrcError> {
        self.visits += 1;
        if at.depth > MAX_DEPTH || self.visits > MAX_VISITS {
            return Err(malformed(
                "the occurrence tree is too deep or too large".into(),
            ));
        }
        let p = self.product(fs, index)?;
        let m = match &p.location {
            Some(l) => multiply(father, l),
            None => *father,
        };
        let mut chain = graphics.to_vec();
        chain.push(Graphics { fs, ..p.graphics });
        let r = self.resolve(fs, p)?;
        let drawn = at.drawn && !p.hidden && !p.suppressed;
        let node = self.nodes.len();
        let (name, name_from) = self.name_of(p, &r);
        let first = self.out.len();
        self.nodes.push(ModelNode {
            name,
            name_from,
            parent: at.parent,
            depth: at.depth,
            file_structure: fs,
            occurrence: index,
            hidden: p.hidden,
            suppressed: p.suppressed,
            drawn,
            has_part: r.part.1 != 0,
            placements: first..first,
        });
        if drawn && r.part.1 != 0 {
            self.part(r.part.0, r.part.1 as usize - 1, &m, &chain)?;
        }
        let child = At {
            depth: at.depth + 1,
            parent: Some(node),
            drawn,
        };
        for &s in r.sons.1 {
            self.occurrence(r.sons.0, s as usize, &m, &chain, child)?;
        }
        if p.external != 0 {
            let efs = match p.external_fs {
                Some(id) => self.fs(id)?,
                None => fs,
            };
            self.occurrence(efs, p.external as usize - 1, &m, &chain, child)?;
        }
        let end = self.out.len();
        if let Some(n) = self.nodes.get_mut(node) {
            n.placements = first..end;
        }
        Ok(())
    }

    /// The part and sons of `p`, falling back to its prototype chain's for
    /// whichever it leaves empty [WD 7.3.10.1], and the first name on that
    /// chain.
    fn resolve(&self, fs: usize, p: &'t Product) -> Result<Resolved<'t>, PrcError> {
        let (mut part, mut sons) = ((fs, p.part), (fs, p.sons.as_slice()));
        let mut proto_name = None;
        let mut proto = (fs, p.prototype, p.prototype_fs);
        let mut hops = 0;
        while proto.1 != 0 && (part.1 == 0 || sons.1.is_empty() || proto_name.is_none()) {
            hops += 1;
            if hops > MAX_DEPTH {
                return Err(malformed("prototype chain too long".into()));
            }
            let pfs = match proto.2 {
                Some(id) => self.fs(id)?,
                None => proto.0,
            };
            let q = self.product(pfs, proto.1 as usize - 1)?;
            if part.1 == 0 {
                part = (pfs, q.part);
            }
            if sons.1.is_empty() {
                sons = (pfs, q.sons.as_slice());
            }
            if proto_name.is_none() {
                proto_name = q.name.as_deref();
            }
            proto = (pfs, q.prototype, q.prototype_fs);
        }
        Ok(Resolved {
            part,
            sons,
            proto_name,
        })
    }

    /// The display name: the occurrence's, its prototype chain's, then its
    /// part's.
    fn name_of(&self, p: &Product, r: &Resolved<'_>) -> (Option<String>, NameSource) {
        if let Some(n) = &p.name {
            return (Some(n.clone()), NameSource::Occurrence);
        }
        if let Some(n) = r.proto_name {
            return (Some(n.to_owned()), NameSource::Prototype);
        }
        let part = (r.part.1 != 0)
            .then(|| self.trees.get(r.part.0))
            .flatten()
            .and_then(|t| t.1.part_names.get(r.part.1 as usize - 1))
            .and_then(Option::as_ref);
        match part {
            Some(n) => (Some(n.clone()), NameSource::Part),
            None => (None, NameSource::Unnamed),
        }
    }

    fn part(
        &mut self,
        fs: usize,
        index: usize,
        m: &Matrix,
        graphics: &[Graphics],
    ) -> Result<(), PrcError> {
        let (_, tree, globals) = self
            .trees
            .get(fs)
            .copied()
            .ok_or_else(|| malformed("file structure out of range".into()))?;
        let items = tree
            .parts
            .get(index)
            .ok_or_else(|| malformed(format!("part definition {index} does not exist")))?;
        for item in items {
            let mut placed = *m;
            for &cs in &item.local {
                let l = globals
                    .systems
                    .get(cs as usize - 1)
                    .ok_or_else(|| malformed(format!("coordinate system {cs} does not exist")))?;
                placed = multiply(&placed, l);
            }
            let own = item.graphics.iter().map(|&g| Graphics { fs, ..g });
            let chain: Vec<Graphics> = graphics.iter().copied().chain(own).collect();
            self.out.push(Placement {
                file_structure: fs,
                tessellation: item.tessellation as usize - 1,
                matrix: placed,
                colour: chain_colour(&self.palettes, &chain),
                palettes: self.palettes.clone(),
                chain,
            });
        }
        Ok(())
    }
}
