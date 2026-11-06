//! Burning an annotation's appearance into page content, shared by
//! [`EditSession::flatten_annotations`] and
//! [`EditSession::to_print_ready_bytes`].

use std::collections::BTreeSet;

use super::{
    AnnotBurn, AnnotFlattenOutcome, AnnotFlattenRefusalReason, ContentBuilder, EditError,
    EditSession, ObjectWrite, PageSlot, counter_rotation_about, fit_matrix_for, read_matrix_entry,
    read_rect_array,
};
use crate::graph::ObjectGraph as _;
use crate::object::{Dict, ObjId};

/// `/XObject` resource names and what each resolves to.
type XObjectNames = Vec<(Vec<u8>, ObjId)>;

impl EditSession {
    /// The appearance stream of `a` and its ISO 32000-1 §12.5.5 placement on
    /// a page with effective `/Rotate` `rotate`, or why it has none to burn.
    pub(super) fn annot_burn_placement(
        &self,
        a: &crate::annot::Annotation,
        rotate: u16,
    ) -> Result<(ObjId, [f64; 6]), AnnotFlattenRefusalReason> {
        use AnnotFlattenRefusalReason as R;
        let ap = match a.appearance {
            crate::annot::Appearance::Normal {
                stream_id: Some(id),
            } => id,
            crate::annot::Appearance::StateUnresolved => return Err(R::StateUnresolved),
            _ => return Err(R::NoAppearance),
        };
        let graph = self.graph();
        let Some(dict) = graph.resolved(ap).as_dict() else {
            return Err(R::NoAppearance);
        };
        let rect = a.rect.ok_or(R::DegenerateAppearance)?;
        let bbox = dict
            .get(b"BBox")
            .and_then(|o| read_rect_array(&graph, o))
            .ok_or(R::DegenerateAppearance)?;
        let matrix = read_matrix_entry(&graph, dict);
        // Render refuses the same degenerate transformed box (§12.5.5 step a).
        let [ma, mb, mc, md, _, _] = matrix;
        let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for x in [bbox[0], bbox[2]] {
            for y in [bbox[1], bbox[3]] {
                let (tx, ty) = (ma * x + mc * y, mb * x + md * y);
                minx = minx.min(tx);
                maxx = maxx.max(tx);
                miny = miny.min(ty);
                maxy = maxy.max(ty);
            }
        }
        if maxx - minx <= 1e-6 || maxy - miny <= 1e-6 || rect.width() <= 0.0 || rect.height() <= 0.0
        {
            return Err(R::DegenerateAppearance);
        }
        let placement = fit_matrix_for(bbox, matrix, rect);
        // §12.5.3 NoRotate (and §12.5.6.4's `/Text`, which behaves as if it
        // were set): what a reader shows is the appearance upright about the
        // `/Rect` upper-left corner. Page content turns with `/Rotate`, so
        // burn it pre-turned the other way about that corner.
        let upright = a.subtype == b"Text" || a.flags.no_rotate();
        if upright && !rotate.is_multiple_of(360) {
            let pivot = counter_rotation_about(rect.llx, rect.ury, rotate);
            return Ok((ap, crate::text_edit::edit::mat_mul(placement, pivot)));
        }
        Ok((ap, placement))
    }

    /// The burn of `a`, whose appearance `ap` goes on the page at `cm`.
    pub(super) fn annot_burn_of(
        &self,
        a: &crate::annot::Annotation,
        ap: ObjId,
        cm: [f64; 6],
    ) -> AnnotBurn {
        AnnotBurn {
            ap,
            cm,
            alpha: a.constant_alpha.filter(|v| *v < 1.0).map(|v| v.max(0.0)),
            oc: a.oc,
            has_resources: self
                .graph()
                .resolved(ap)
                .as_dict()
                .is_some_and(|d| d.contains_key(b"Resources")),
            rect: a.rect.map_or([0.0; 4], |r| [r.llx, r.lly, r.urx, r.ury]),
        }
    }

    /// Stage the content stream drawing `burns` in order on `page_id`.
    /// Returns its id and the `/XObject` names the page must gain.
    pub(super) fn burn_overlay(
        &mut self,
        burns: Vec<AnnotBurn>,
        page_id: ObjId,
        slots: &[PageSlot],
        objects: &mut Vec<ObjectWrite>,
        outcome: &mut AnnotFlattenOutcome,
    ) -> Result<(ObjId, XObjectNames), EditError> {
        let page_resources: Dict = self.effective_resources(page_id, slots);
        let mut invocations: Vec<(Vec<u8>, Option<[f64; 6]>)> = Vec::new();
        let mut xobjects: XObjectNames = Vec::new();
        let mut taken: BTreeSet<Vec<u8>> = BTreeSet::new();
        for burn in burns {
            let (target, placement) = if burn.alpha.is_none() && burn.oc.is_none() {
                (burn.ap, Some(burn.cm))
            } else {
                let target = self.annot_burn_wrapper(&burn, &page_resources, objects, outcome)?;
                (target, None)
            };
            let name = self.free_page_resource_name(page_id, slots, b"XObject", "pdfceAn", &taken);
            taken.insert(name.clone());
            invocations.push((name.clone(), placement));
            xobjects.push((name, target));
        }

        let mut cb = ContentBuilder::new();
        for (name, cm) in &invocations {
            cb.save_state();
            if let Some(cm) = cm {
                cb.concat_matrix(cm[0], cm[1], cm[2], cm[3], cm[4], cm[5]);
            }
            cb.invoke_xobject(name);
            cb.restore_state();
        }
        let overlay_id = self.new_stream(Dict::new(), &cb.into_bytes(), objects)?;
        Ok((overlay_id, xobjects))
    }
}
