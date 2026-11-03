//! PRC wire tessellations placed into an [`AssembledModel`] as polylines.

use crate::{AssembledModel, Placement, WireTessellation, transform_point};

/// One placed polyline of a wire tessellation [WD 7.8.7], drawn unlit, one
/// line width wide, by the renderer.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ModelLine {
    /// The points, in the space [`AssembledModel::meshes`] are placed in.
    pub points: Vec<[f64; 3]>,
    /// Each point's straight-RGBA colour from the wire's own vertex
    /// colours, parallel to [`Self::points`]; empty when it has none.
    pub colours: Vec<[u8; 4]>,
    /// The colour the model tree gives the wire's representation item;
    /// drawn when it has no vertex colours.
    pub colour: Option<[u8; 4]>,
    /// The placement it is drawn by, an index into the placements
    /// [`AssembledModel::tree`]'s ranges index; `None` when placements were
    /// not applied.
    pub placement: Option<usize>,
}

impl ModelLine {
    /// Point `i`'s colour: its vertex colour, else [`Self::colour`], else
    /// `fallback` opaque.
    #[must_use]
    pub fn colour_at(&self, i: usize, fallback: [u8; 3]) -> [u8; 4] {
        let [r, g, b] = fallback;
        self.colours
            .get(i)
            .copied()
            .or(self.colour)
            .unwrap_or([r, g, b, 255])
    }
}

/// The wires a model's tessellations hold, by (file structure,
/// tessellation) index, in index order.
pub(crate) type Wires = Vec<((usize, usize), WireTessellation)>;

/// Adds the polylines of the wire placement `k` (`p`) draws, if any.
pub(crate) fn place(p: &Placement, k: usize, wires: &Wires, model: &mut AssembledModel) {
    let key = (p.file_structure, p.tessellation);
    let Ok(at) = wires.binary_search_by_key(&key, |(i, _)| *i) else {
        return;
    };
    let Some((_, wire)) = wires.get(at) else {
        return;
    };
    let colour = p.colour.map(crate::model::to_rgba8);
    push(
        wire,
        Some(k),
        colour,
        |q| transform_point(&p.matrix, q),
        model,
    );
}

/// Adds every wire once, in its own coordinates, uncoloured by a tree.
pub(crate) fn unplaced(wires: &Wires, model: &mut AssembledModel) {
    for (_, wire) in wires {
        push(wire, None, None, |q| q, model);
    }
}

fn push(
    wire: &WireTessellation,
    placement: Option<usize>,
    colour: Option<[u8; 4]>,
    to_model: impl Fn([f64; 3]) -> [f64; 3],
    model: &mut AssembledModel,
) {
    for (i, points) in wire.polylines.iter().enumerate() {
        if points.len() < 2 {
            continue;
        }
        model.lines.push(ModelLine {
            points: points.iter().map(|&q| to_model(q)).collect(),
            colours: wire.colours.get(i).cloned().unwrap_or_default(),
            colour,
            placement,
        });
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;

    #[test]
    fn vertex_colours_win_over_the_tree_colour() {
        let mut line = ModelLine {
            points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            colours: vec![[1, 2, 3, 255], [4, 5, 6, 255]],
            colour: Some([9, 9, 9, 255]),
            placement: None,
        };
        assert_eq!(line.colour_at(1, [0; 3]), [4, 5, 6, 255]);
        line.colours.clear();
        assert_eq!(line.colour_at(1, [0; 3]), [9, 9, 9, 255]);
        line.colour = None;
        assert_eq!(line.colour_at(1, [7, 7, 7]), [7, 7, 7, 255]);
    }

    #[test]
    fn single_point_polylines_are_left_out() {
        let wire = WireTessellation {
            polylines: vec![vec![[0.0; 3]], vec![[0.0; 3], [1.0; 3]]],
            colours: Vec::new(),
        };
        let mut model = AssembledModel::default();
        unplaced(&vec![((0, 0), wire)], &mut model);
        assert_eq!(model.lines.len(), 1);
        assert_eq!(model.lines[0].points[1], [1.0; 3]);
    }
}
