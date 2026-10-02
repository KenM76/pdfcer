//! Push-button icons: `/MK /I`, `/TP` and `/IF` (ISO 32000-1 §12.5.6.19
//! Table 189, §12.7.7.3.2 Table 247; ISO 32000-2 Tables 192 and 250).

use super::{EditError, EditSession, ObjectWrite, WidgetEdit};
use crate::annot_author::{ButtonIcon, CaptionPosition, IconFit};
use crate::forms;
use crate::image_import::ImportedImage;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Rect;

/// What a [`WidgetEdit`] does to a push button's icon, `/MK /I`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ButtonIconEdit {
    /// Draw this image as the icon: a new form XObject becomes `/MK /I`.
    Set(Box<ImportedImage>),
    /// Remove `/MK /I`, so the button draws its caption alone.
    Clear,
}

impl WidgetEdit {
    /// Give a push button an icon drawn from `image` (`/MK /I`, ISO 32000-1
    /// §12.5.6.19 Table 189).
    ///
    /// The image becomes a form XObject the size of the image in points,
    /// written by the same image writer as [`EditSession::add_image`], so
    /// alpha becomes an `/SMask`. The appearance is redrawn with the icon
    /// placed by `/MK /IF`; a button without `/IF` gets Table 247's default
    /// (always scale, proportionally, centred), which pdfcer writes.
    ///
    /// Without [`Self::with_caption_position`], a button whose `/TP` shows no
    /// icon gets [`CaptionPosition::IconOnly`] when its caption is empty and
    /// [`CaptionPosition::CaptionBelow`] otherwise; a button already laid out
    /// with an icon keeps its `/TP`.
    ///
    /// A field that is not a push button is refused with
    /// [`EditError::NotAPushButton`].
    ///
    /// ```
    /// # use pdfcer_core::{edit::WidgetEdit, image_import};
    /// # fn demo(png: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    /// let edit = WidgetEdit::new().with_button_icon(&image_import::import(png)?);
    /// assert!(edit.button_icon.is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_button_icon(mut self, image: &ImportedImage) -> Self {
        self.button_icon = Some(ButtonIconEdit::Set(Box::new(image.clone())));
        self
    }

    /// Remove a push button's icon (`/MK /I`) and its `/TP`, unless
    /// [`Self::with_caption_position`] also sets one. `/IF` and the rollover
    /// and down icons are left as they are.
    ///
    /// ```
    /// use pdfcer_core::edit::{ButtonIconEdit, WidgetEdit};
    /// let edit = WidgetEdit::new().without_button_icon();
    /// assert_eq!(edit.button_icon, Some(ButtonIconEdit::Clear));
    /// ```
    #[must_use]
    pub fn without_button_icon(mut self) -> Self {
        self.button_icon = Some(ButtonIconEdit::Clear);
        self
    }

    /// Set a push button's caption position, `/MK /TP` (Table 189), and
    /// redraw it. With no icon the button draws its caption alone whatever
    /// the position says.
    ///
    /// ```
    /// use pdfcer_core::{annot_author::CaptionPosition, edit::WidgetEdit};
    /// let edit = WidgetEdit::new().with_caption_position(CaptionPosition::IconOnly);
    /// assert_eq!(edit.caption_position, Some(CaptionPosition::IconOnly));
    /// ```
    #[must_use]
    pub const fn with_caption_position(mut self, position: CaptionPosition) -> Self {
        self.caption_position = Some(position);
        self
    }

    /// Whether this edit touches the icon layout at all.
    pub(super) const fn touches_button_icon(&self) -> bool {
        self.button_icon.is_some() || self.caption_position.is_some()
    }
}

impl EditSession {
    /// Apply `edit`'s icon and caption position to `mk` (the widget's `/MK`,
    /// already patched by the other entries), staging a new icon form into
    /// `objects`. Returns the icon the redraw should draw.
    ///
    /// Patch-only: an edit that sets no icon leaves `/I` as it was.
    pub(super) fn apply_button_icon_edit(
        &mut self,
        field: &forms::Field,
        mk: &mut Dict,
        edit: &WidgetEdit,
        caption_after: &str,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<Option<(ButtonIcon, ObjId)>, EditError> {
        let push = field.field_type == Some(forms::FieldType::Button)
            && field.button_kind == Some(forms::ButtonKind::Push);
        if edit.touches_button_icon() && !push {
            return Err(EditError::NotAPushButton {
                name: field.fully_qualified_name.clone(),
            });
        }
        let mut staged = None;
        match &edit.button_icon {
            Some(ButtonIconEdit::Set(image)) => {
                let (id, bounds) = self.stage_button_icon_form(image, objects)?;
                mk.insert(Name::from(b"I"), Object::Reference(id));
                if mk.get(b"IF").is_none() {
                    mk.insert(
                        Name::from(b"IF"),
                        Object::Dict(IconFit::default().to_dict()),
                    );
                }
                let stored = self.tp_of(mk);
                let position = edit.caption_position.unwrap_or(match stored {
                    p if p.shows_icon() => p,
                    _ if caption_after.is_empty() => CaptionPosition::IconOnly,
                    _ => CaptionPosition::CaptionBelow,
                });
                mk.insert(Name::from(b"TP"), Object::Integer(position.to_tp()));
                staged = Some((id, bounds));
            }
            Some(ButtonIconEdit::Clear) => {
                mk.remove(b"I");
                if let Some(p) = edit.caption_position {
                    mk.insert(Name::from(b"TP"), Object::Integer(p.to_tp()));
                } else {
                    mk.remove(b"TP");
                }
            }
            None => {
                if let Some(p) = edit.caption_position {
                    mk.insert(Name::from(b"TP"), Object::Integer(p.to_tp()));
                }
            }
        }
        Ok(self.icon_from_mk(mk, staged))
    }

    /// The icon a widget's stored `/MK` lays out, if it draws one.
    pub(super) fn stored_button_icon(&self, widget: ObjId) -> Option<(ButtonIcon, ObjId)> {
        let Some(Object::Dict(dict)) = self.value(widget) else {
            return None;
        };
        let mk = self.deref_dict(dict.get(b"MK"))?;
        self.icon_from_mk(&mk, None)
    }

    /// The icon `mk` lays out: `/I` when `/TP` shows one. `staged` supplies
    /// the bounds of an icon form this command has not committed yet.
    fn icon_from_mk(
        &self,
        mk: &Dict,
        staged: Option<(ObjId, Rect)>,
    ) -> Option<(ButtonIcon, ObjId)> {
        let id = mk.get(b"I")?.as_reference()?;
        let position = self.tp_of(mk);
        if !position.shows_icon() {
            return None;
        }
        let bounds = match staged {
            Some((sid, bounds)) if sid == id => bounds,
            _ => self.form_bounds(id)?,
        };
        let fit = self
            .deref_dict(mk.get(b"IF"))
            .map(|d| IconFit::from_dict(&d))
            .unwrap_or_default();
        Some((
            ButtonIcon {
                bounds,
                position,
                fit,
            },
            id,
        ))
    }

    /// `/TP`, defaulting to caption-only when absent or out of range.
    fn tp_of(&self, mk: &Dict) -> CaptionPosition {
        self.deref_value(mk.get(b"TP"))
            .and_then(|o| o.as_int())
            .and_then(CaptionPosition::from_tp)
            .unwrap_or_default()
    }

    /// A form XObject's `/BBox` through its `/Matrix` (§8.10.1), bounded.
    fn form_bounds(&self, id: ObjId) -> Option<Rect> {
        let Some(Object::Stream(form)) = self.value(id) else {
            return None;
        };
        let nums = |key: &[u8]| -> Option<Vec<f64>> {
            match self.deref_value(form.dict.get(key))? {
                Object::Array(a) => a.iter().map(Object::as_number).collect(),
                _ => None,
            }
        };
        let [x0, y0, x1, y1]: [f64; 4] = nums(b"BBox")?.try_into().ok()?;
        let [a, b, c, d, e, f]: [f64; 6] = nums(b"Matrix")
            .and_then(|m| m.try_into().ok())
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        let fold = |f: fn(f64, f64) -> f64, init: f64, pick: fn(&(f64, f64)) -> f64| {
            corners.iter().map(pick).fold(init, f)
        };
        let r = Rect {
            llx: fold(f64::min, f64::INFINITY, |c| c.0),
            lly: fold(f64::min, f64::INFINITY, |c| c.1),
            urx: fold(f64::max, f64::NEG_INFINITY, |c| c.0),
            ury: fold(f64::max, f64::NEG_INFINITY, |c| c.1),
        };
        (r.width() > 0.0 && r.height() > 0.0).then_some(r)
    }

    /// Stage `image` as an icon form: a form XObject one point per pixel
    /// (EXIF orientation applied) that draws the image XObject.
    fn stage_button_icon_form(
        &mut self,
        image: &ImportedImage,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(ObjId, Rect), EditError> {
        let (image_id, _) = self.stage_image_xobject(image, objects)?;
        let (w, h) = (f64::from(image.width), f64::from(image.height));
        let (w, h) = if image.orientation.transposes() {
            (h, w)
        } else {
            (w, h)
        };
        let bounds = Rect {
            llx: 0.0,
            lly: 0.0,
            urx: w,
            ury: h,
        };
        let content = Self::poster_content((image.width, image.height), image.orientation, bounds);
        let mut xobjects = Dict::new();
        xobjects.insert(Name::from(b"Poster"), Object::Reference(image_id));
        let mut resources = Dict::new();
        resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
        dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
        dict.insert(
            Name::from(b"BBox"),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(w),
                Object::Real(h),
            ]),
        );
        dict.insert(Name::from(b"Resources"), Object::Dict(resources));
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(content.len()).unwrap_or(i64::MAX)),
        );
        let id = ObjId::new(self.alloc_number()?, 0);
        let span = self.stage_bytes(&content);
        objects.push(ObjectWrite {
            id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict,
                data_span: span,
            })),
        });
        Ok((id, bounds))
    }
}
