//! Author and edit `/Link` annotations (§12.5.6.5, pdfcer-gui request G161):
//! create one over a page rectangle, change where it goes, change its border.

use super::{
    AnnotFlags, AnnotKind, Command, CommandKind, Dict, EditError, EditSession, MarkupSpec,
    ObjectWrite, PermissionBit, Stream, dest_array,
};
use crate::annot_author::{self, BorderDash, Color};
use crate::object::{Name, ObjId, Object};
use crate::outline::DestView;
use crate::page_tree::Rect;

/// Where a link goes when activated.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LinkTarget {
    /// A page of this document, written as an explicit `/Dest` array
    /// (§12.3.2.2 Table 151).
    Page {
        /// 0-based page index.
        page_index: usize,
        /// The view on arrival. [`DestView::Unknown`] and
        /// [`DestView::Absent`] are refused.
        view: DestView,
    },
    /// A named destination this document defines (§12.3.2.3), written as
    /// the key so the link follows the destination if pages move.
    Named(Vec<u8>),
    /// A web address, written as a `/URI` action (ISO 32000-1 §12.6.4.7,
    /// ISO 32000-2 §12.6.4.8). 7-bit ASCII only: 1.7 requires it and 2.0's
    /// UTF-8 admits it, so percent-encode anything else first.
    Uri(String),
}

/// A visible link border: a stroked rectangle inside `/Rect`.
///
/// No border at all is `None` wherever a `LinkBorder` is taken.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LinkBorder {
    /// Stroke width in points; finite and greater than zero.
    pub width: f64,
    /// Stroke colour, written as `/C`.
    pub color: Color,
    /// `/BS /D`, or `None` for a solid border.
    pub dash: Option<BorderDash>,
}

impl LinkBorder {
    /// A solid border of `width` points in `color`.
    #[must_use]
    pub fn new(width: f64, color: Color) -> Self {
        Self {
            width,
            color,
            dash: None,
        }
    }

    /// The same border, dashed.
    #[must_use]
    pub fn with_dash(mut self, dash: BorderDash) -> Self {
        self.dash = Some(dash);
        self
    }
}

/// What [`EditSession::set_link_target`] replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkTargetChange {
    /// The link, unchanged in identity.
    pub annot_id: ObjId,
    /// The `/S` type of the `/A` action that was removed, when the link
    /// had one (`GoTo`, `URI`, `JavaScript`, …). A chained `/Next` action
    /// goes with it. `None` when the link had a `/Dest` or nothing.
    pub replaced_action: Option<String>,
}

/// What [`EditSession::set_link_border`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkBorderChange {
    /// The link, unchanged in identity.
    pub annot_id: ObjId,
    /// The link carried an `/AP` that is now replaced (visible border) or
    /// removed (no border). A link drawn by another producer may have had
    /// artwork there; disclose it.
    pub appearance_replaced: bool,
}

impl EditSession {
    /// Create a `/Link` over `rect` on page `page_index` that goes to
    /// `target`, with `border` or no visible border. One undo entry,
    /// [`CommandKind::AddAnnotation`] with [`AnnotKind::Link`].
    ///
    /// `rect` is in unrotated page space, y-up. A visible border is drawn
    /// by a generated `/AP` (pdfcer's renderer never synthesises one, so
    /// a border without it would show elsewhere and not here) and also
    /// described by `/BS` and `/C`. No border is written as
    /// `/Border [0 0 0]`, since Table 164's default is a 1-point border.
    /// The link prints (`/F` Print).
    ///
    /// # Errors
    ///
    /// - [`EditError::EmptyGeometry`] — `rect` has no area or a
    ///   non-finite edge.
    /// - [`EditError::LinkUriInvalid`], [`EditError::NamedDestinationNotFound`],
    ///   [`EditError::UnsupportedDestination`], [`EditError::PageOutOfRange`]
    ///   — `target` cannot be written.
    /// - [`EditError::LinkBorderWidthInvalid`].
    /// - [`EditError::DocumentEncrypted`],
    ///   [`EditError::CertificationForbidsChange`],
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    ///   [`EditError::ObjectNumbersExhausted`], [`EditError::AnnotsNotAnArray`]
    ///   — as for [`EditSession::add_markup`].
    pub fn add_link(
        &mut self,
        page_index: usize,
        rect: Rect,
        target: &LinkTarget,
        border: Option<&LinkBorder>,
    ) -> Result<ObjId, EditError> {
        self.link_gates()?;
        let rect = normalised(rect).ok_or(EditError::EmptyGeometry)?;
        let target_keys = self.link_target_keys(target)?;
        let slots = self.page_slots()?;
        let page_id = slots
            .get(page_index)
            .ok_or(EditError::PageOutOfRange {
                index: page_index,
                count: slots.len(),
            })?
            .id;
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }

        let annot_id = ObjId::new(self.alloc_number()?, 0);
        let mut annot = Dict::new();
        annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
        annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Link")));
        annot.insert(Name::from(b"Rect"), rect_object(rect));
        annot.insert(Name::from(b"P"), Object::Reference(page_id));
        annot.insert(
            Name::from(b"F"),
            Object::Integer(i64::from(AnnotFlags::PRINT)),
        );
        let (key, value) = target_keys;
        annot.insert(Name::from(key), value);
        let mut objects = self.apply_link_border(&mut annot, border.map(|b| (rect, b)))?;
        objects.push(ObjectWrite {
            id: annot_id,
            before: None,
            after: Some(Object::Dict(annot)),
        });
        objects.append(&mut self.annots_writes(page_id, annot_id, &slots)?);
        self.commit(Command {
            kind: CommandKind::AddAnnotation {
                kind: AnnotKind::Link,
            },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(annot_id)
    }

    /// Point the link `annot_id` at `target`, removing its old `/Dest` or
    /// `/A`. Border, `/Rect`, `/H`, `/PA` and identity are kept. One undo
    /// entry, [`CommandKind::SetLinkTarget`].
    ///
    /// # Errors
    ///
    /// - [`EditError::LinkVerbOnOther`] — not a `/Link`.
    /// - [`EditError::AnnotationLocked`] — Table 165 bit 8.
    /// - `target` errors as for [`Self::add_link`].
    /// - [`EditError::AnnotationNotFound`], [`EditError::NotADictionary`],
    ///   and the encryption and certification gates.
    pub fn set_link_target(
        &mut self,
        annot_id: ObjId,
        target: &LinkTarget,
    ) -> Result<LinkTargetChange, EditError> {
        self.link_gates()?;
        let (mut annot, _) = self.editable_link(annot_id)?;
        let (key, value) = self.link_target_keys(target)?;
        annot.remove(b"Dest");
        let replaced_action = annot.remove(b"A").map(|action| self.action_type(&action));
        annot.insert(Name::from(key), value);
        self.commit_link(annot_id, annot, Vec::new(), CommandKind::SetLinkTarget);
        Ok(LinkTargetChange {
            annot_id,
            replaced_action,
        })
    }

    /// Give the link `annot_id` the visible `border`, or none. One undo
    /// entry, [`CommandKind::SetLinkBorder`]. The written keys are as for
    /// [`Self::add_link`]; `/Border`, `/BS`, `/C` and `/AP` are replaced.
    ///
    /// # Errors
    ///
    /// [`EditError::LinkBorderWidthInvalid`], [`EditError::EmptyGeometry`]
    /// (a link whose `/Rect` has no area cannot carry a visible border),
    /// and the errors of [`Self::set_link_target`] other than its target
    /// ones.
    pub fn set_link_border(
        &mut self,
        annot_id: ObjId,
        border: Option<&LinkBorder>,
    ) -> Result<LinkBorderChange, EditError> {
        self.link_gates()?;
        let (mut annot, rect) = self.editable_link(annot_id)?;
        let appearance_replaced = annot.contains_key(b"AP");
        for key in [&b"Border"[..], b"BS", b"C", b"AP"] {
            annot.remove(key);
        }
        let objects = match border {
            None => self.apply_link_border(&mut annot, None)?,
            Some(b) => {
                let rect = rect.and_then(normalised).ok_or(EditError::EmptyGeometry)?;
                self.apply_link_border(&mut annot, Some((rect, b)))?
            }
        };
        self.commit_link(annot_id, annot, objects, CommandKind::SetLinkBorder);
        Ok(LinkBorderChange {
            annot_id,
            appearance_replaced,
        })
    }

    fn link_gates(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()
    }

    /// The current dictionary and `/Rect` of the `/Link` `annot_id`, refused
    /// when it is another subtype or Locked.
    fn editable_link(&self, annot_id: ObjId) -> Result<(Dict, Option<Rect>), EditError> {
        let (target, _all) = self.locate_annotation(annot_id)?;
        let subtype = String::from_utf8_lossy(&target.subtype).into_owned();
        if subtype != "Link" {
            return Err(EditError::LinkVerbOnOther {
                id: annot_id,
                subtype,
            });
        }
        if target.flags.locked() {
            return Err(EditError::AnnotationLocked {
                id: annot_id,
                subtype,
            });
        }
        match self.value(annot_id) {
            Some(Object::Dict(d)) => Ok((d.clone(), target.rect)),
            _ => Err(EditError::NotADictionary {
                id: annot_id,
                key: "Subtype",
            }),
        }
    }

    /// `/S` of an action dictionary, or `"?"` when unreadable.
    fn action_type(&self, action: &Object) -> String {
        self.deref_dict(Some(action))
            .and_then(|d| d.get(b"S").and_then(Object::as_name).cloned())
            .map_or_else(
                || "?".to_owned(),
                |n| String::from_utf8_lossy(n.as_bytes()).into_owned(),
            )
    }

    /// The `/Dest` or `/A` entry that sends a link to `target`.
    fn link_target_keys(&self, target: &LinkTarget) -> Result<(&'static [u8], Object), EditError> {
        match target {
            LinkTarget::Page { page_index, view } => {
                let slots = self.page_slots()?;
                let slot = slots.get(*page_index).ok_or(EditError::PageOutOfRange {
                    index: *page_index,
                    count: slots.len(),
                })?;
                let array = dest_array(slot.id, view)
                    .map_err(|kind| EditError::UnsupportedDestination { kind })?;
                Ok((b"Dest", Object::Array(array)))
            }
            LinkTarget::Named(name) => Ok((b"Dest", self.named_destination_reference(name)?)),
            LinkTarget::Uri(uri) => {
                check_uri(uri)?;
                let mut action = Dict::new();
                action.insert(Name::from(b"S"), Object::Name(Name::from(b"URI")));
                action.insert(Name::from(b"URI"), Object::String(uri.as_bytes().to_vec()));
                Ok((b"A", Object::Dict(action)))
            }
        }
    }

    /// Write `border`, drawn inside its rectangle, into `annot` and return
    /// the new appearance stream's write, if any.
    fn apply_link_border(
        &mut self,
        annot: &mut Dict,
        border: Option<(Rect, &LinkBorder)>,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let Some((rect, border)) = border else {
            annot.insert(
                Name::from(b"Border"),
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(0),
                ]),
            );
            return Ok(Vec::new());
        };
        if !border.width.is_finite() || border.width <= 0.0 {
            return Err(EditError::LinkBorderWidthInvalid {
                given: border.width,
            });
        }
        let authored = annot_author::build_appearance_opts(
            &MarkupSpec::Square {
                rect,
                border: Some(border.color),
                interior: None,
                border_width: border.width,
                border_effect: None,
            },
            &annot_author::AppearanceOptions {
                quad_order: self.quad_point_order,
                dash: border.dash.clone(),
            },
        );
        for key in [&b"BS"[..], b"C"] {
            if let Some(v) = authored.annot.get(key) {
                annot.insert(Name::from(key), v.clone());
            }
        }
        let ap_id = ObjId::new(self.alloc_number()?, 0);
        let mut ap_dict = authored.ap_dict;
        ap_dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(authored.ap_content.len()).unwrap_or(i64::MAX)),
        );
        let data_span = self.stage_bytes(&authored.ap_content);
        let mut ap = Dict::new();
        ap.insert(Name::from(b"N"), Object::Reference(ap_id));
        annot.insert(Name::from(b"AP"), Object::Dict(ap));
        Ok(vec![ObjectWrite {
            id: ap_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: ap_dict,
                data_span,
            })),
        }])
    }

    fn commit_link(
        &mut self,
        annot_id: ObjId,
        annot: Dict,
        mut objects: Vec<ObjectWrite>,
        kind: CommandKind,
    ) {
        objects.push(ObjectWrite {
            id: annot_id,
            before: self.state.get(&annot_id).cloned(),
            after: Some(Object::Dict(annot)),
        });
        self.commit(Command {
            kind,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
    }
}

/// `rect` with its edges ordered, or `None` when it has no area or a
/// non-finite edge.
fn normalised(rect: Rect) -> Option<Rect> {
    let r = Rect {
        llx: rect.llx.min(rect.urx),
        lly: rect.lly.min(rect.ury),
        urx: rect.llx.max(rect.urx),
        ury: rect.lly.max(rect.ury),
    };
    let finite = [r.llx, r.lly, r.urx, r.ury].iter().all(|v| v.is_finite());
    (finite && r.urx > r.llx && r.ury > r.lly).then_some(r)
}

fn rect_object(r: Rect) -> Object {
    Object::Array(
        [r.llx, r.lly, r.urx, r.ury]
            .into_iter()
            .map(Object::Real)
            .collect(),
    )
}

/// ISO 32000-1 §12.6.4.7: a URI action's `/URI` is 7-bit ASCII (2.0's
/// UTF-8 is a superset, so ASCII satisfies both editions). Control
/// characters are refused too: no URI syntax admits them.
fn check_uri(uri: &str) -> Result<(), EditError> {
    let reason = if uri.is_empty() {
        "it is empty"
    } else if !uri.is_ascii() {
        "it is not 7-bit ASCII; percent-encode it first"
    } else if uri.bytes().any(|b| b.is_ascii_control()) {
        "it contains a control character"
    } else {
        return Ok(());
    };
    Err(EditError::LinkUriInvalid { reason })
}
