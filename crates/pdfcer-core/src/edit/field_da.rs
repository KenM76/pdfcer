//! The `/DA` a field verb writes (ISO 32000-2 §12.7.4.3), shared by
//! `edit_field` and the `add_*_field` verbs so a created field and an edited
//! one resolve a [`FieldAppearance`] the same way.

use super::{EditError, EditSession, FieldAppearance, FieldFont};
use crate::fontdata::Std14;
use crate::vartext::{FontResource, TextColor};

/// What a creation verb draws and writes its variable text with.
pub(super) struct CreationDa {
    /// The `/DA` string.
    pub(super) da: Vec<u8>,
    /// The fonts the appearance builder resolves the `/DA` name against.
    pub(super) resources: Vec<FontResource>,
    /// A standard-14 face `/AcroForm /DR /Font` must gain for `da` to resolve.
    pub(super) needs: Option<Std14>,
    /// Whether the built stream's fonts are pointed at `/DR`, as the
    /// regenerator does.
    pub(super) bind: bool,
}

impl EditSession {
    /// The `/DR /Font` key `app`'s `/DA` names; refused with
    /// [`EditError::FieldFontNotInResources`] when a
    /// [`FieldFont::Resource`] is not in `/DR /Font`. A standard face cannot
    /// fail: the caller adds its resource.
    pub(super) fn field_da_key(&self, app: &FieldAppearance) -> Result<Vec<u8>, EditError> {
        match &app.font {
            FieldFont::Standard(f) => Ok(Self::std14_resource_key(*f).to_vec()),
            FieldFont::Resource(name) => {
                let present: Vec<String> = self
                    .acroform_dr_font_keys()
                    .iter()
                    .map(|k| String::from_utf8_lossy(k).into_owned())
                    .collect();
                if present.iter().any(|k| k.as_bytes() == name.as_slice()) {
                    return Ok(name.clone());
                }
                Err(EditError::FieldFontNotInResources {
                    name: String::from_utf8_lossy(name).into_owned(),
                    available: if present.is_empty() {
                        "none -- the AcroForm has no /DR /Font dictionary".to_owned()
                    } else {
                        present.join(", ")
                    },
                })
            }
        }
    }

    /// The `/DA` for a new text or choice field: `None` is the creation
    /// default, Helvetica at auto size in black, byte-identical to what the
    /// verbs wrote before they took an appearance.
    pub(super) fn creation_da(
        &self,
        app: Option<&FieldAppearance>,
    ) -> Result<CreationDa, EditError> {
        let Some(app) = app else {
            return Ok(CreationDa {
                da: crate::vartext::default_appearance_string(b"Helv", 0.0, TextColor::Gray(0.0)),
                resources: vec![FontResource {
                    name: b"Helv".to_vec(),
                    font: Std14::Helvetica,
                    widths: None,
                    ascent: None,
                    codes: None,
                    cid: None,
                }],
                needs: None,
                bind: false,
            });
        };
        let key = self.field_da_key(app)?;
        let mut resources = self.resolve_dr_fonts();
        let needs = match app.font {
            FieldFont::Standard(f) => {
                // The face this command adds to `/DR`, so the builder lays
                // out with it rather than falling back to Helvetica.
                if !resources.iter().any(|r| r.name == key) {
                    resources.push(FontResource {
                        name: key.clone(),
                        font: f,
                        widths: None,
                        ascent: None,
                        codes: None,
                        cid: None,
                    });
                }
                Some(f)
            }
            FieldFont::Resource(_) => None,
        };
        Ok(CreationDa {
            da: crate::vartext::default_appearance_string(&key, app.size, app.color),
            resources,
            needs,
            bind: true,
        })
    }
}
