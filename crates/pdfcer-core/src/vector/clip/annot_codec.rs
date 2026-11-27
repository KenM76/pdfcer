//! The annotation records of the clip byte format: one tag byte, then the
//! fields of one [`ClipAnnotation`].
//!
//! Tags 0 and 1 are retired forms, refused whole: neither length can be
//! skipped without decoding it, so reading on would mis-parse every
//! annotation after it.

use std::collections::BTreeMap;

use super::{
    ClipAnnotation, ClipError, ClipObject, MAX_CLIP_ANNOTATIONS, RawAnnotation, Reader, put_bounds,
    put_bytes, put_cos, put_u32, rebuild_stream,
};
use crate::dimension::sidecar::{
    deserialize_group_settings, deserialize_kind, deserialize_overrides, serialize_group_settings,
    serialize_kind, serialize_overrides,
};
use crate::object::Object;

/// The modelled-markup form; markups travel raw now.
const TAG_RETIRED_MARKUP: u8 = 0;
/// The model-only ce-dimension form of formats `2`–`6`, which carried no
/// dictionary, so a pasted copy lost every key the model does not describe.
const TAG_RETIRED_DIMENSION: u8 = 1;
const TAG_UNSUPPORTED: u8 = 2;
const TAG_RAW: u8 = 3;
/// A ce dimension: its model AND its dictionary with closure (format `7`).
const TAG_DIMENSION: u8 = 4;

/// Append one annotation record.
pub(super) fn put_annotation(out: &mut Vec<u8>, annotation: &ClipAnnotation) {
    match annotation {
        ClipAnnotation::Dimension {
            group_name,
            format,
            scale,
            standard,
            group_style,
            style,
            kind,
            label_override,
            source,
            anchors,
            raw,
        } => {
            out.push(TAG_DIMENSION);
            put_bytes(out, group_name.as_bytes());
            // The model travels as the COS objects the sidecar codec already
            // writes, so a new model field cannot reach the document format
            // and be forgotten here.
            put_cos(
                out,
                &serialize_group_settings(format, *scale, *standard, group_style),
            );
            put_cos(out, &serialize_overrides(style));
            put_cos(out, &serialize_kind(kind));
            match label_override {
                Some(text) => {
                    out.push(1);
                    put_bytes(out, text.as_bytes());
                }
                None => out.push(0),
            }
            put_cos(
                out,
                &crate::dimension::encode_clip_anchors(*source, anchors),
            );
            put_raw(out, raw);
        }
        ClipAnnotation::Raw(raw) => {
            out.push(TAG_RAW);
            put_raw(out, raw);
        }
        ClipAnnotation::Unsupported { subtype } => {
            out.push(TAG_UNSUPPORTED);
            put_bytes(out, subtype.as_bytes());
        }
    }
}

/// Read one record written by [`put_annotation`].
pub(super) fn read_annotation(r: &mut Reader<'_>) -> Result<ClipAnnotation, ClipError> {
    Ok(match flag(r)? {
        TAG_RETIRED_MARKUP => {
            return Err(ClipError::Content(
                "this clip holds a markup in a form older pdfcer builds wrote; copy it again with this build".to_owned(),
            ));
        }
        TAG_RETIRED_DIMENSION => {
            return Err(ClipError::Content(
                "this clip holds a ce dimension in the form older pdfcer builds wrote, which carried its measurement but not its annotation's other keys; copy it again with this build".to_owned(),
            ));
        }
        TAG_RAW => ClipAnnotation::Raw(Box::new(read_raw(r)?)),
        TAG_DIMENSION => read_dimension(r)?,
        _ => ClipAnnotation::Unsupported {
            subtype: String::from_utf8_lossy(&r.bytes()?).into_owned(),
        },
    })
}

fn read_dimension(r: &mut Reader<'_>) -> Result<ClipAnnotation, ClipError> {
    let content =
        |what: &str| ClipError::Content(format!("a ce dimension's {what} could not be read back"));
    let group_name = String::from_utf8_lossy(&r.bytes()?).into_owned();
    let (format, scale, standard, group_style) =
        deserialize_group_settings(&r.cos()?).ok_or_else(|| content("group settings"))?;
    let style = deserialize_overrides(&r.cos()?);
    let kind = deserialize_kind(&r.cos()?).ok_or_else(|| content("geometry"))?;
    let label_override = if flag(r)? == 1 {
        Some(String::from_utf8_lossy(&r.bytes()?).into_owned())
    } else {
        None
    };
    let (source, anchors) = crate::dimension::decode_clip_anchors(&r.cos()?);
    let raw = Box::new(read_raw(r)?);
    Ok(ClipAnnotation::Dimension {
        group_name,
        format,
        scale,
        standard,
        group_style,
        style,
        kind: Box::new(kind),
        label_override,
        source,
        anchors,
        raw,
    })
}

fn flag(r: &mut Reader<'_>) -> Result<u8, ClipError> {
    Ok(r.take(1)?.first().copied().unwrap_or(0))
}

fn put_raw(out: &mut Vec<u8>, raw: &RawAnnotation) {
    put_bytes(out, raw.subtype.as_bytes());
    put_cos(out, &Object::Dict(raw.dict.clone()));
    match raw.rect {
        Some(b) => {
            out.push(1);
            put_bounds(out, b);
        }
        None => out.push(0),
    }
    put_u32(out, u32::try_from(raw.objects.len()).unwrap_or(u32::MAX));
    for (&id, object) in &raw.objects {
        put_u32(out, id);
        let (value, payload): (Object, Option<&Vec<u8>>) = match &object.value {
            Object::Stream(stream) => (Object::Dict(stream.dict.clone()), object.payload.as_ref()),
            other => (other.clone(), None),
        };
        put_cos(out, &value);
        match payload {
            Some(bytes) => {
                out.push(1);
                put_bytes(out, bytes);
            }
            None => out.push(0),
        }
    }
}

fn read_raw(r: &mut Reader<'_>) -> Result<RawAnnotation, ClipError> {
    let subtype = String::from_utf8_lossy(&r.bytes()?).into_owned();
    let dict =
        r.cos()?.as_dict().cloned().ok_or_else(|| {
            ClipError::Content("a carried annotation is not a dictionary".to_owned())
        })?;
    let rect = if flag(r)? == 1 {
        Some(r.bounds()?)
    } else {
        None
    };
    let object_count = r.u32()? as usize;
    if object_count > MAX_CLIP_ANNOTATIONS {
        return Err(ClipError::ClipTooLarge {
            found: object_count,
            limit: MAX_CLIP_ANNOTATIONS,
        });
    }
    let mut objects = BTreeMap::new();
    for _ in 0..object_count {
        let id = r.u32()?;
        let value = r.cos()?;
        let payload = if flag(r)? == 1 {
            Some(r.bytes()?)
        } else {
            None
        };
        let value = rebuild_stream(value, payload.as_deref())?;
        objects.insert(id, ClipObject { value, payload });
    }
    Ok(RawAnnotation {
        subtype,
        dict,
        objects,
        rect,
    })
}
