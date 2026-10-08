//! Embedding a 3D model ([`EditSession::add_3d_annotation`]) and replacing
//! its poster ([`EditSession::set_3d_poster`]).

use super::{AnnotKind, Command, CommandKind, EditError, EditSession, MarkupOptions, ObjectWrite};
use crate::annot_author::{self, AuthoredTextAnnot, Color};
use crate::image_import::ImportedImage;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::threed::{
    ThreeDEmbedOutcome, ThreeDFormat, ThreeDPoster, ThreeDPosterOutcome, ThreeDSpec,
};

impl EditSession {
    /// Embed a U3D or PRC model as a **3D annotation** (ISO 32000-1
    /// §13.6.2 Table 298) on page `page_index`. One undo entry.
    ///
    /// - The model goes into a 3D stream (§13.6.3 Table 300: `/Type /3D`,
    ///   `/Subtype /U3D` or `/PRC`), Flate-compressed, referenced directly
    ///   from `/3DD` so the annotation gets its own instance. No `/VA` or
    ///   `/3DV` is written: the reader opens on the artwork's own default
    ///   view.
    /// - `/3DA` carries only `/A` from `spec.activation`; the other Table 299
    ///   entries keep their defaults.
    /// - The `/AP /N` poster §13.6.2 requires — what prints and what a
    ///   reader without 3D support shows — is `spec.poster` fitted inside the
    ///   rectangle; with none, a rendering of the model from pdfcer's default
    ///   view when it is a PRC model that meshes and `spec.render_poster` is
    ///   set (needs the `3d` feature); otherwise pdfcer's placeholder drawing.
    ///   [`ThreeDEmbedOutcome::poster`] says which and why; a rendered poster
    ///   is an inference the caller discloses.
    /// - `options.note` supplies `/Contents`, `/T` and `/M`.
    /// - The header version is not raised; the outcome reports when it is
    ///   below the format's ([`ThreeDEmbedOutcome::below_required_version`]).
    ///
    /// # Errors
    ///
    /// [`EditError::ThreeD`] when the spec fails [`ThreeDSpec::validate`];
    /// otherwise as [`Self::add_file_attachment_annotation`]. A model that
    /// cannot be rendered is not an error: it gets the placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{document::Document, edit::{EditSession, MarkupOptions}};
    /// # use pdfcer_core::page_tree::Rect;
    /// # use pdfcer_core::threed::{ThreeDPoster, ThreeDSpec};
    /// # fn demo(doc: Document, prc: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    /// let mut session = EditSession::new(doc);
    /// let rect = Rect { llx: 72.0, lly: 400.0, urx: 372.0, ury: 700.0 };
    /// let outcome =
    ///     session.add_3d_annotation(0, &ThreeDSpec::new(rect, prc)?, &MarkupOptions::default())?;
    /// if let ThreeDPoster::Placeholder(why) = &outcome.poster {
    ///     println!("placeholder poster: {why}");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_3d_annotation(
        &mut self,
        page_index: usize,
        spec: &ThreeDSpec,
        options: &MarkupOptions,
    ) -> Result<ThreeDEmbedOutcome, EditError> {
        options.validate()?;
        spec.validate()?;
        let document_version = self.base.version();
        self.on_layer_if(page_index, options.layer, |s| {
            let (slots, page_id) = s.annotation_author_target(page_index)?;
            let mut objects = Vec::new();
            let mut authored = annot_author::three_d_placeholder(spec.rect, spec.color);
            let (poster_image_id, poster) =
                s.stage_embed_poster(spec, &mut authored, &mut objects)?;
            let (annot_id, mut annot, ap_write) =
                s.stage_authored_icon(authored, page_id, options)?;
            let (stream_id, stream_write) = s.stage_three_d_stream(spec)?;
            annot.insert(Name::from(b"3DD"), Object::Reference(stream_id));
            let mut activation = Dict::new();
            activation.insert(
                Name::from(b"A"),
                Object::Name(Name::from(spec.activation.name())),
            );
            annot.insert(Name::from(b"3DA"), Object::Dict(activation));
            objects.push(ap_write);
            objects.push(ObjectWrite {
                id: annot_id,
                before: None,
                after: Some(Object::Dict(annot)),
            });
            objects.push(stream_write);
            objects.append(&mut s.annots_append(page_id, &[annot_id], &slots)?);
            s.commit(Command {
                kind: CommandKind::AddAnnotation {
                    kind: AnnotKind::ThreeD,
                },
                objects,
                removals: Vec::new(),
                trailer: None,
            });
            Ok(ThreeDEmbedOutcome {
                annot_id,
                stream_id,
                poster_image_id,
                poster,
                required_version: spec.required_version(),
                document_version,
            })
        })
    }

    /// Replace a `/3D` annotation's poster (its `/AP /N`, ISO 32000-1
    /// §13.6.2 Table 298) with `image`, fitted inside the annotation's
    /// `/Rect` preserving its aspect ratio and EXIF orientation, exactly as
    /// [`Self::add_3d_annotation`] fits a supplied poster. One undo entry.
    ///
    /// Writes a new image XObject and a new appearance stream, and rewrites
    /// only `/N` of the annotation's `/AP` (other appearance entries stay).
    /// The 3D stream, its views, `/3DA` and `/C` are untouched, so
    /// [`crate::threed::extract_3d`] returns the same model bytes. The old
    /// appearance stream is left in place, unreferenced by this annotation.
    ///
    /// # Errors
    ///
    /// In order: [`EditError::DocumentEncrypted`], the certification gate,
    /// [`EditError::PageOutOfRange`],
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    /// [`EditError::AnnotationNotFound`] when page `page_index`'s `/Annots`
    /// does not list `annot_id`, [`EditError::ThreeD`] with
    /// [`ThreeDEmbedError::NotA3dAnnotation`](crate::threed::ThreeDEmbedError::NotA3dAnnotation) for another subtype,
    /// [`EditError::AnnotationLocked`] (Table 165 bit 8),
    /// [`EditError::AnnotationRectMissing`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{edit::EditSession, image_import, object::ObjId};
    /// # fn demo(session: &mut EditSession, annot: ObjId, png: &[u8])
    /// #     -> Result<(), Box<dyn std::error::Error>> {
    /// let image = image_import::import(png)?;
    /// let outcome = session.set_3d_poster(0, annot, &image)?;
    /// assert_eq!(outcome.annot_id, annot);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_3d_poster(
        &mut self,
        page_index: usize,
        annot_id: ObjId,
        image: &ImportedImage,
    ) -> Result<ThreeDPosterOutcome, EditError> {
        let (target, dict) = self.three_d_annot(page_index, annot_id)?;
        let rect = target
            .rect
            .ok_or_else(|| EditError::AnnotationRectMissing {
                subtype: "3D".to_owned(),
            })?;
        let mut objects = Vec::new();
        let mut authored = annot_author::three_d_placeholder(rect, Color::Gray(0.0));
        let poster_image_id = self.attach_fitted_image(&mut authored, image, &mut objects)?;
        let appearance_id = ObjId::new(self.alloc_number()?, 0);
        let mut ap_dict = authored.ap_dict;
        ap_dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(authored.ap_content.len()).unwrap_or(i64::MAX)),
        );
        let data_span = self.stage_bytes(&authored.ap_content);
        objects.push(ObjectWrite {
            id: appearance_id,
            before: None,
            after: Some(Object::Stream(Stream {
                dict: ap_dict,
                data_span,
            })),
        });
        let mut ap = match dict.get(b"AP").map(|o| self.resolve_value(o)) {
            Some(Object::Dict(existing)) => existing.clone(),
            _ => Dict::new(),
        };
        ap.insert(Name::from(b"N"), Object::Reference(appearance_id));
        let mut annot = dict;
        annot.insert(Name::from(b"AP"), Object::Dict(ap));
        objects.push(ObjectWrite {
            id: annot_id,
            before: self.state.get(&annot_id).cloned(),
            after: Some(Object::Dict(annot)),
        });
        self.commit(Command {
            kind: CommandKind::SetThreeDPoster,
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(ThreeDPosterOutcome {
            annot_id,
            appearance_id,
            poster_image_id,
        })
    }

    /// Draw `spec`'s poster into `authored`: the supplied image, else the
    /// rendered default view, else the placeholder (whose colour goes in
    /// `/C` for `three_d_poster_rebuild` to redraw it on resize).
    fn stage_embed_poster(
        &mut self,
        spec: &ThreeDSpec,
        authored: &mut AuthoredTextAnnot,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(Option<ObjId>, ThreeDPoster), EditError> {
        if let Some(img) = &spec.poster {
            let id = self.attach_fitted_image(authored, img, objects)?;
            return Ok((Some(id), ThreeDPoster::Supplied));
        }
        match crate::threed::default_poster(spec) {
            Ok((img, rendered)) => {
                let id = self.attach_fitted_image(authored, &img, objects)?;
                Ok((Some(id), ThreeDPoster::Rendered(rendered)))
            }
            Err(reason) => {
                authored
                    .annot
                    .insert(Name::from(b"C"), spec.color.to_array());
                Ok((None, ThreeDPoster::Placeholder(reason)))
            }
        }
    }

    /// Stage `img` as an image XObject and make `authored`'s appearance draw
    /// it as `/Poster`, fitted inside `authored.rect`. Returns the image id.
    pub(super) fn attach_fitted_image(
        &mut self,
        authored: &mut AuthoredTextAnnot,
        img: &ImportedImage,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<ObjId, EditError> {
        let (image_id, _) = self.stage_image_xobject(img, objects)?;
        authored.ap_content =
            Self::poster_content((img.width, img.height), img.orientation, authored.rect);
        let mut xobjects = Dict::new();
        xobjects.insert(Name::from(b"Poster"), Object::Reference(image_id));
        let mut resources = Dict::new();
        resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        authored
            .ap_dict
            .insert(Name::from(b"Resources"), Object::Dict(resources));
        Ok(image_id)
    }

    /// Allocate and stage `spec`'s 3D stream (§13.6.3 Table 300).
    fn stage_three_d_stream(
        &mut self,
        spec: &ThreeDSpec,
    ) -> Result<(ObjId, ObjectWrite), EditError> {
        let stream_id = ObjId::new(self.alloc_number()?, 0);
        let encoded = crate::filters::flate::encode(&spec.data);
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"3D")));
        let subtype: &[u8] = match spec.format {
            ThreeDFormat::Prc => b"PRC",
            _ => b"U3D",
        };
        dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(subtype)));
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(encoded.len()).unwrap_or(i64::MAX)),
        );
        let data_span = self.stage_bytes(&encoded);
        Ok((
            stream_id,
            ObjectWrite {
                id: stream_id,
                before: None,
                after: Some(Object::Stream(Stream { dict, data_span })),
            },
        ))
    }
}
