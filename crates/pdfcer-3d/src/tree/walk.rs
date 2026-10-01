//! The occurrence walk that turns the trees into placements.

use super::*;

/// The occurrence walk over every file structure's tree [WD 7.3.10.1,
/// 7.6.3.2; `prc__8137__model_tree_asm.md` §9].
pub(crate) struct Walk<'t> {
    /// Per file structure: its id, tree and globals.
    pub(crate) trees: Vec<(UniqueId, &'t Tree, &'t Globals)>,
    pub(crate) out: Vec<Placement>,
    visits: usize,
    /// Per file structure, the style colours placements share.
    palettes: std::collections::HashMap<usize, std::sync::Arc<[Option<[f64; 4]>]>>,
}

impl<'t> Walk<'t> {
    /// A walk over `trees`, with no placements yet.
    pub(crate) fn new(trees: Vec<(UniqueId, &'t Tree, &'t Globals)>) -> Self {
        Walk {
            trees,
            out: Vec::new(),
            visits: 0,
            palettes: std::collections::HashMap::new(),
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
    /// occurrences' graphics are `graphics`, root first.
    pub(crate) fn occurrence(
        &mut self,
        fs: usize,
        index: usize,
        father: &Matrix,
        graphics: &[Graphics],
        depth: usize,
    ) -> Result<(), PrcError> {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS {
            return Err(malformed(
                "the occurrence tree is too deep or too large".into(),
            ));
        }
        let p = self.product(fs, index)?;
        if p.hidden {
            return Ok(());
        }
        let m = match &p.location {
            Some(l) => multiply(father, l),
            None => *father,
        };
        let mut chain = graphics.to_vec();
        chain.push(p.graphics);
        // The part and sons, falling back to the prototype chain's for
        // whichever this occurrence leaves empty [WD 7.3.10.1].
        let (mut part, mut sons) = ((fs, p.part), (fs, &p.sons));
        let mut proto = (fs, p.prototype, p.prototype_fs);
        let mut hops = 0;
        while proto.1 != 0 && (part.1 == 0 || sons.1.is_empty()) {
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
                sons = (pfs, &q.sons);
            }
            proto = (pfs, q.prototype, q.prototype_fs);
        }
        if part.1 != 0 {
            self.part(part.0, part.1 as usize - 1, &m, &chain)?;
        }
        for &s in sons.1 {
            self.occurrence(sons.0, s as usize, &m, &chain, depth + 1)?;
        }
        if p.external != 0 {
            let efs = match p.external_fs {
                Some(id) => self.fs(id)?,
                None => fs,
            };
            self.occurrence(efs, p.external as usize - 1, &m, &chain, depth + 1)?;
        }
        Ok(())
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
            let chain: Vec<Graphics> = graphics.iter().chain(&item.graphics).copied().collect();
            let palette = self.palettes.entry(fs).or_insert_with(|| {
                (0..=globals.styles.len() as u32)
                    .map(|b| globals.style_colour(b))
                    .collect()
            });
            self.out.push(Placement {
                file_structure: fs,
                tessellation: item.tessellation as usize - 1,
                matrix: placed,
                colour: globals.style_colour(resolve_style(&chain)),
                palette: palette.clone(),
                chain,
            });
        }
        Ok(())
    }
}
