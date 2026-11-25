//! Adding, changing and removing the centre mark of a placed ce dimension
//! (pdfcer-gui request G330).

use super::{Command, CommandKind, EditError, EditSession, check_centre_kind};
use crate::crypto::PermissionBit;
use crate::dimension::{CentreMarkSpec, DimensionId, DimensionKind};

/// Which ce dimensions carry a centre mark, and the one each carries now:
/// the answer to [`EditSession::centre_mark_host`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CentreMarkHost {
    /// A radius/diameter ce dimension ([`DimensionKind::Circular`]); its mark
    /// is optional, so [`EditSession::set_centre_mark`] accepts `None`.
    Circular {
        /// The mark it carries now, if any.
        mark: Option<CentreMarkSpec>,
    },
    /// A standalone centre mark ([`DimensionKind::CentreMark`]); the mark is
    /// the whole object, so it can be replaced but not cleared (delete the
    /// ce dimension instead).
    Standalone {
        /// The mark it carries.
        mark: CentreMarkSpec,
    },
}

impl CentreMarkHost {
    /// The mark carried now, if any.
    #[must_use]
    pub fn mark(self) -> Option<CentreMarkSpec> {
        match self {
            Self::Circular { mark } => mark,
            Self::Standalone { mark } => Some(mark),
        }
    }

    /// Whether `set_centre_mark(id, None)` is accepted: true only for a
    /// radius/diameter ce dimension.
    #[must_use]
    pub fn can_clear(self) -> bool {
        matches!(self, Self::Circular { .. })
    }
}

/// What [`EditSession::set_centre_mark`] changed beyond the mark itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CentreMarkOutcome {
    /// The value label moved. A radius/diameter ce dimension with no stored
    /// text distance puts its label outside the rim while it carries a mark,
    /// so adding or removing one moves the label; a shell discloses that off
    /// the page ("label moved outside the rim to clear the centre mark").
    /// Always `false` for a standalone mark, which has no label.
    pub label_moved: bool,
}

impl EditSession {
    /// The centre-mark host behind `dimension`, or `None` when the id is
    /// unknown or its kind carries no centre mark. A preflight: it reads
    /// the model and changes nothing, so a properties panel can decide
    /// whether to draw its centre-mark section without calling
    /// [`Self::set_centre_mark`] to find out.
    #[must_use]
    pub fn centre_mark_host(&self, dimension: DimensionId) -> Option<CentreMarkHost> {
        match self.read_dimension_model().dimension(dimension)?.kind {
            DimensionKind::Circular { centre_mark, .. } => {
                Some(CentreMarkHost::Circular { mark: centre_mark })
            }
            DimensionKind::CentreMark { mark, .. } => Some(CentreMarkHost::Standalone { mark }),
            _ => None,
        }
    }

    /// **Set, replace or clear the centre mark of a placed ce dimension**, as
    /// one undoable command that regenerates its `/AP` (and the `/InkList` of
    /// a standalone mark) and the sidecar.
    ///
    /// | target | `Some(spec)` | `None` |
    /// |---|---|---|
    /// | [`DimensionKind::Circular`] | sets `centre_mark` | clears it |
    /// | [`DimensionKind::CentreMark`] | replaces `mark` | [`EditError::CentreMarkRequired`] |
    /// | any other kind | [`EditError::NotACentreMarkHost`] | same |
    ///
    /// The id, group, fit, style and placement are kept. The mark's size is
    /// a style property ([`crate::dimension::StyleOverrides`]'s
    /// `centre_mark_size`), not part of the spec. Undo restores the previous
    /// spec, and with it the previous label position, which is derived from
    /// the spec rather than stored.
    ///
    /// Setting the current value still commits, as
    /// [`Self::set_dimension_display`] does, so a control press always makes
    /// one undo entry: compare against [`Self::centre_mark_host`] first to
    /// suppress a no-op.
    ///
    /// # Errors
    ///
    /// All refusals happen before anything is written:
    /// [`EditError::DimensionNotFound`] for an unknown id;
    /// [`EditError::NotACentreMarkHost`] and
    /// [`EditError::CentreMarkRequired`] as tabled;
    /// [`EditError::CentreGeometryDegenerate`] for a spec whose angle is not
    /// finite, or a circle with no positive finite radius; plus the
    /// encryption, enforced-certification and newer-sidecar guards.
    pub fn set_centre_mark(
        &mut self,
        dimension: DimensionId,
        mark: Option<CentreMarkSpec>,
    ) -> Result<CentreMarkOutcome, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.check_dimension_sidecar()?;

        let id = dimension.0;
        let mut model = self.read_dimension_model();
        let Some(record) = model.dimension_mut(dimension) else {
            return Err(EditError::DimensionNotFound { id });
        };
        let before = record.kind.circular_text_distance();
        let mut kind = record.kind.clone();
        match (&mut kind, mark) {
            (DimensionKind::Circular { centre_mark, .. }, m) => *centre_mark = m,
            (DimensionKind::CentreMark { mark: slot, .. }, Some(m)) => *slot = m,
            (DimensionKind::CentreMark { .. }, None) => {
                return Err(EditError::CentreMarkRequired { id });
            }
            _ => return Err(EditError::NotACentreMarkHost { id }),
        }
        check_centre_kind(&kind)?;
        let label_moved = kind.circular_text_distance() != before;
        record.kind = kind;

        let mut objects = self.regenerate_dimension_writes(&model, &[dimension])?;
        objects.push(self.catalog_dimension_write(&model)?);
        self.commit(Command {
            kind: CommandKind::SetCentreMark {
                present: mark.is_some(),
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(CentreMarkOutcome { label_moved })
    }
}
