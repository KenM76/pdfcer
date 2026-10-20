//! The user-data discard half of optimize (Pass 616.3), through
//! [`EditSession::remove_metadata`].

use super::EditSession;
use super::optimize_cleanup::Fold;
use crate::doc_metadata::{MetadataItem, MetadataKind, MetadataRemoveOptions};
use crate::optimize::{DiscardOptions, OptimizeError};

impl EditSession {
    /// Every inventory item of a kind `options` discards, with the
    /// disclosures the removal will need.
    pub(super) fn plan_discard(
        &self,
        options: DiscardOptions,
        notes: &mut Vec<String>,
    ) -> Vec<MetadataItem> {
        if options.is_none() {
            return Vec::new();
        }
        if crate::signature::census(self).forbids_structural_change() {
            notes.push(
                "a certification signature forbids deletions; no user data was discarded".into(),
            );
            return Vec::new();
        }
        let inventory = self.metadata_inventory();
        if inventory.truncated {
            notes.push(
                "the metadata inventory was cut short; only the items listed are discarded".into(),
            );
        }
        let items: Vec<MetadataItem> = inventory
            .items
            .into_iter()
            .filter(|i| options.contains(i.kind))
            .collect();
        let has = |kind| items.iter().any(|i| i.kind == kind);
        if has(MetadataKind::FormData) {
            notes.push(
                "form fields go back to their default values; signature and read-only fields are kept"
                    .into(),
            );
        }
        if has(MetadataKind::DocumentId) {
            notes.push("a fresh file identifier is written in place of the old one".into());
        }
        items
    }

    /// Remove `items` inside the run `fold` is collecting. On any failure
    /// the whole run is undone.
    pub(super) fn apply_discard(
        &mut self,
        items: &[MetadataItem],
        fold: &mut Fold,
    ) -> Result<usize, OptimizeError> {
        let Some(first) = items.first() else {
            return Ok(0);
        };
        let ids: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        let result = self.remove_metadata(&ids, &MetadataRemoveOptions::default());
        fold.absorb(self);
        let (item, reason) = match result {
            Err(e) => (first.id.clone(), e.to_string()),
            Ok(report) => match report.not_removed.into_iter().next() {
                Some(n) => (n.id, n.reason),
                None => return Ok(report.removed.len()),
            },
        };
        fold.roll_back(self);
        Err(OptimizeError::DiscardFailed { item, reason })
    }
}
