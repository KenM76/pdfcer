//! Recording a soft mask's group beside the export recording, for a
//! writer that rebuilds the mask as geometry (`crate::mask_source`).

use std::sync::Arc;

use tiny_skia::Mask;

use super::Canvas;
use crate::display_list::{Op, Recorder};
use crate::mask_source::MaskSource;

impl Canvas<'_> {
    /// Whether this is an export recording, the one recorder that keeps a
    /// mask's source.
    pub(crate) fn records_export(&self) -> bool {
        matches!(self, Self::Record(r) if r.export.is_some())
    }

    /// Run `f` into a fresh frame of an export recording and return what
    /// it recorded, leaving the page's ops, tally, site log and poison as
    /// they were. Clips it makes stay in the page's clip table, which is
    /// what the returned ops refer to. `None` on any other canvas.
    pub(crate) fn record_aside(&mut self, f: impl FnOnce(&mut Canvas<'_>)) -> Option<Vec<Op>> {
        let Self::Record(r) = self else {
            return None;
        };
        r.export.as_ref()?;
        r.harvest();
        let export = r.export.as_mut()?;
        let tally = export.tally;
        let sites = std::mem::take(&mut export.sites);
        let poison = r.poison;
        r.frames.push(Vec::new());
        f(&mut Canvas::Record(r));
        r.harvest();
        let ops = r.frames.pop().unwrap_or_default();
        if let Some(e) = r.export.as_mut() {
            e.tally = tally;
            e.sites = sites;
        }
        r.poison = poison;
        Some(ops)
    }

    /// Record that `mask` was built from `source`. A no-op on any canvas
    /// but an export recording.
    pub(crate) fn register_mask(&mut self, mask: &Arc<Mask>, source: &Arc<MaskSource>) {
        if let Self::Record(r) = self
            && let Some(e) = r.export.as_mut()
        {
            e.masks.push((Arc::clone(mask), Arc::clone(source)));
        }
    }
}

impl Recorder {
    /// The registered `Arc` holding `mask` itself (same address), so a
    /// layer carries the allocation its source is keyed on.
    pub(crate) fn registered_mask(&self, mask: &Mask) -> Option<Arc<Mask>> {
        let e = self.export.as_ref()?;
        e.masks
            .iter()
            .find(|(m, _)| std::ptr::eq(Arc::as_ptr(m), mask))
            .map(|(m, _)| Arc::clone(m))
    }
}
