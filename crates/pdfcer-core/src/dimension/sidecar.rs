//! # The authoritative `/PieceInfo` sidecar (ISO 32000-1 §14.5, decision 011 §2.4)
//!
//! Serialise the [`DimensionModel`] to — and parse it back from — the PDF
//! object graph, for storage under the document catalog's
//! `/PieceInfo /pdfcer /Private` (§14.5 Table 319). This is pdfcer's
//! **authoritative** dimensioning model: groups, scale, units, number format,
//! per-dimension geometry, best-fit params + residual, and the annotation/`/AP`
//! wiring handles the scale-repropagation needs.
//!
//! ## Why `/PieceInfo` is authoritative but its cross-tool survival is not spec-guaranteed
//!
//! Per `iso32000__s__14.5.md` (NOTE 1): private `/PieceInfo` data **"may be
//! ignored by general-purpose conforming readers"** — there is **no** ISO
//! preservation guarantee. Survival across a pdfcer round-trip is guaranteed
//! only by **pdfcer's own R34 minimal-diff save** (untouched objects re-emitted
//! byte-identical), not by §14.5. That is exactly why the load-bearing scale is
//! **also** mirrored into a reader-visible `/Measure` dict
//! ([`super::measure_dict`]): if a foreign editor drops `/PieceInfo`, the
//! `/Measure`-encoded scale still survives. On load, native-vs-sidecar
//! disagreement ⇒ disclose + prefer the sidecar (decision 011 §2.4, Z8).
//!
//! ## Format
//!
//! A self-describing `Object::Dict` with `/Version`, a `/Groups` array, and a
//! `/Dimensions` array. Deserialisation is **total and lenient** — a malformed
//! or partial sidecar yields `None` (the caller then starts fresh rather than
//! panicking), and unknown keys are ignored (forward-compat, §14.5's
//! `PictureEdit`/`PictureEditExtended` pattern).

use crate::object::{Dict, Name, Object};
use crate::vector::{AxisConstraint, Point};

use super::fit::FitCircle;
use super::group::{
    DimStandard, DimensionId, DimensionKind, DimensionModel, DimensionRecord, Group, GroupId,
};
use super::style::{ArrowForm, GroupStyle, StyleOverrides};
use super::tolerance::Tolerance;
use super::units::{DecimalMarker, FractionMode, NumberFormat, ScaleState, Unit};
use crate::vector::Rgb;

/// The sidecar schema version pdfcer writes (bumped only on a breaking layout
/// change; readers ignore unknown extra keys, §14.5 forward-compat).
///
/// # The version history, because each bump had to earn itself
///
/// - **1** — the original `Pass 12.M2` schema: linear + circular kinds.
/// - **2** — `Pass 68.0` added the `angular` kind. A new KIND is not a
///   defaultable key: a build that does not know the token drops the whole
///   record, so without a bump an older pdfcer would read the file, silently
///   lose every angular ce dimension, and be free to save it back that way.
/// - **3** — `Pass 107.0` added the `perimeter` kind, for exactly the same
///   reason. Note what did NOT bump it: `/Offset` and `/TextAlong`
///   (`Pass 27.0`/`27.1`) and the whole `Pass 69.0` style-override key set are
///   optional-with-default, so an older build reading them reconstructs a
///   correct — if plainer — ce dimension rather than losing one.
///
/// # What a bump actually buys, since it is not backward compatibility
///
/// An older build still READS a version-3 sidecar and shows what it
/// understands ([`deserialize_model`]'s gate is a range, not an equality). The
/// bump is what makes that older session REFUSE TO WRITE
/// (`EditError::SidecarWrittenByNewerBuild`, via [`sidecar_version`]) — the
/// destructive half, and the only half worth blocking. This matters more than
/// usual here because the operator deliberately runs two builds side by side
/// out of two folders and WILL open a perimeter-bearing file in the older one.
/// - **4** — `Pass 175.0` added `/LabelOverride`, the operator's ce-dimension
///   text override (decision 097). Bumped for the reason `/Offset` and the
///   `Pass 69.0` style keys were NOT: an older build reading a v3-shaped
///   record drops the key, shows the measured caption instead of the
///   operator's, and on its next regeneration re-bakes the `/AP` — so the
///   number printed on a drawing CHANGES, silently, with nothing on screen
///   saying it did. Losing a colour override is visible and re-settable;
///   losing a text override alters what the document asserts. That is the
///   `angular`/`perimeter` severity class arrived at through a defaultable
///   key rather than an unknown kind token.
///
/// # ★ Version 4 is emitted PER DOCUMENT, not per build
///
/// [`serialize_model`] writes `4` only when some ce dimension actually
/// carries an override, and `3` otherwise — see `model_sidecar_version`.
/// This is the one place this constant is not the whole story, and it is
/// deliberate on two counts. It keeps R34 minimal-diff honest: a document
/// with no overrides re-serialises to the exact bytes it had before this
/// Pass, so opening and saving does not rewrite every dimensioned file in
/// existence for a feature it does not use. And it keeps the write-refusal
/// PRECISE: the older build is blocked from saving exactly those documents
/// that hold something it would destroy, instead of every document the newer
/// build has ever touched. The operator deliberately runs two builds side by
/// side, which is what makes that difference worth the extra function.
///
/// The earlier bumps are deliberately NOT retrofitted to this scheme. A
/// content-dependent `2`/`3` would be strictly more accurate, but changing
/// the version an existing file re-serialises to is a byte change to every
/// sidecar in the corpus for no capability — the retrofit would cost exactly
/// what this scheme was introduced to avoid.
pub const SIDECAR_VERSION: i64 = 4;

/// The schema version a document needs when no ce dimension uses a
/// `Pass 175.0` feature — i.e. everything up to and including the `perimeter`
/// kind.
const SIDECAR_VERSION_PRE_OVERRIDE: i64 = 3;

/// The version `model` must be written at: [`SIDECAR_VERSION`] when it uses a
/// feature only this build's schema has, otherwise the older version that
/// fully represents it.
///
/// # Why this is a function and not a constant
///
/// See [`SIDECAR_VERSION`]'s note. The short form: the version field exists
/// to answer "would an older build destroy something here on save?", and for
/// a document with no override the honest answer is no. Answering yes anyway
/// would lock the operator's other build out of files it can handle
/// perfectly, and would rewrite their bytes on the way.
///
/// Deterministic and total — same model, same answer, which is what keeps
/// [`serialize_model`]'s no-change-is-a-no-op guarantee (R34) true.
fn model_sidecar_version(model: &DimensionModel) -> i64 {
    if model
        .dimensions()
        .iter()
        .any(|d| d.label_override.is_some())
    {
        SIDECAR_VERSION
    } else {
        SIDECAR_VERSION_PRE_OVERRIDE
    }
}

/// Serialise the whole [`DimensionModel`] to the `Object` pdfcer stores as the
/// `/PieceInfo /pdfcer /Private` value (§14.5). Deterministic — the same model
/// always yields the same bytes, so a no-change save is a no-op (R34).
#[must_use]
pub fn serialize_model(model: &DimensionModel) -> Object {
    let mut d = Dict::new();
    d.insert(
        Name::from(b"Version"),
        Object::Integer(model_sidecar_version(model)),
    );
    d.insert(
        Name::from(b"Groups"),
        Object::Array(model.groups().iter().map(serialize_group).collect()),
    );
    d.insert(
        Name::from(b"Dimensions"),
        Object::Array(model.dimensions().iter().map(serialize_dimension).collect()),
    );
    Object::Dict(d)
}

/// Parse a [`DimensionModel`] back from a stored sidecar `Object`. `None` if
/// the object is not a dict, is not the recognised schema, or is missing the
/// group/dimension arrays (the caller then starts a fresh model).
/// The schema version a sidecar object declares, or `None` if it is not a
/// recognisable sidecar at all.
///
/// Exists so the write side can tell "this file has no pdfcer sidecar" (fine,
/// start one) from "this file's sidecar was written by a newer pdfcer than this
/// one" (refuse to overwrite it). Those two look identical to
/// [`deserialize_model`], and treating the second as the first is how an
/// operator's calibrated scales get silently destroyed by an older build.
#[must_use]
pub fn sidecar_version(obj: &Object) -> Option<i64> {
    obj.as_dict()?.get(b"Version").and_then(Object::as_int)
}

#[must_use]
pub fn deserialize_model(obj: &Object) -> Option<DimensionModel> {
    let d = obj.as_dict()?;
    // Version gate — a RANGE, not an equality.
    //
    // This used to demand exact equality and answer `None` on any mismatch,
    // which the caller turns into a FRESH model. That is silent data loss in
    // both directions: an older sidecar would be discarded on the first
    // version bump, and a sidecar written by a NEWER pdfcer is discarded today,
    // taking every group, every calibrated scale and every membership with it
    // — while the `/Line` annotations keep rendering perfectly, so nothing
    // looks wrong until the next save makes it permanent.
    //
    // Older is readable because every key this schema has ever gained is
    // OPTIONAL with a default (see `/Offset`, `/TextAlong`), so an old
    // document is simply one that used the defaults.
    //
    // NEWER is a different problem and is NOT solved here: this returns the
    // groups and dimensions it can understand, and [`sidecar_version`] lets
    // the session refuse to WRITE over a file it cannot fully represent
    // (`EditError::SidecarWrittenByNewerBuild`). Reading is safe; writing is
    // what would destroy the parts this build does not know about.
    let version = d.get(b"Version").and_then(Object::as_int)?;
    if version > SIDECAR_VERSION {
        // Still parsed, not refused — a reader should show what it can. The
        // write-side guard is the session's.
    }
    let mut model = DimensionModel::empty();
    if let Some(groups) = d.get(b"Groups").and_then(Object::as_array) {
        for g in groups {
            if let Some(group) = deserialize_group(g) {
                model.insert_group(group);
            }
        }
    }
    if let Some(dims) = d.get(b"Dimensions").and_then(Object::as_array) {
        for dim in dims {
            if let Some(record) = deserialize_dimension(dim) {
                model.insert_dimension(record);
            }
        }
    }
    // A model with at least the default group is required to be coherent.
    model.group(super::group::DEFAULT_GROUP_ID)?;
    Some(model)
}

// ---- group (de)serialization ------------------------------------------------

fn serialize_group(g: &Group) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"Id"), Object::Integer(i64::from(g.id.0)));
    d.insert(
        Name::from(b"Name"),
        Object::String(g.name.as_bytes().to_vec()),
    );
    match g.scale {
        ScaleState::NeverSet => {
            d.insert(Name::from(b"Scale"), Object::Name(Name::from(b"never")));
        }
        ScaleState::OneToOne => {
            d.insert(Name::from(b"Scale"), Object::Name(Name::from(b"one")));
        }
        ScaleState::Calibrated { scale } => {
            d.insert(
                Name::from(b"Scale"),
                Object::Name(Name::from(b"calibrated")),
            );
            d.insert(Name::from(b"ScaleValue"), Object::Real(scale));
        }
    }
    d.insert(
        Name::from(b"Unit"),
        Object::String(g.format.unit.token().as_bytes().to_vec()),
    );
    // Written only when NOT the default, so a document that never left ANSI
    // point-decimal keeps byte-identical sidecar output.
    if g.standard != DimStandard::Ansi {
        d.insert(Name::from(b"Standard"), Object::Name(Name::from(b"iso")));
    }
    if g.format.decimal_marker != DecimalMarker::Point {
        d.insert(
            Name::from(b"DecimalMarker"),
            Object::Name(Name::from(b"comma")),
        );
    }
    match g.format.fraction {
        FractionMode::Decimal { places } => {
            d.insert(Name::from(b"Frac"), Object::Name(Name::from(b"decimal")));
            d.insert(Name::from(b"Places"), Object::Integer(i64::from(places)));
        }
        FractionMode::Fraction {
            denominator,
            reduce,
        } => {
            d.insert(Name::from(b"Frac"), Object::Name(Name::from(b"fraction")));
            d.insert(
                Name::from(b"Denom"),
                Object::Integer(i64::from(denominator)),
            );
            d.insert(Name::from(b"Reduce"), Object::Boolean(reduce));
        }
    }
    d.insert(Name::from(b"Visible"), Object::Boolean(g.visible));
    if let Some(ocg) = g.ocg {
        d.insert(Name::from(b"Ocg"), Object::Reference(ocg));
    }
    // The Pass 69.0 group-tier style. Every key is written ONLY when the group
    // actually overrides that property, so a document whose groups have never
    // been styled produces byte-identical sidecar output to what it produced
    // before this Pass existed (R34 minimal diff — and the reason no
    // `SIDECAR_VERSION` bump is owed here; see the constant's own note).
    put_style_keys(
        &mut d,
        StyleKeys {
            text_height: g.style.text_height,
            line_width: g.style.line_width,
            arrow_length: g.style.arrow_length,
            arrow_form: g.style.arrow_form,
            color: g.style.color,
            tolerance: g.style.tolerance,
            tolerance_places: g.style.tolerance_places,
        },
    );
    Object::Dict(d)
}

fn deserialize_group(obj: &Object) -> Option<Group> {
    let d = obj.as_dict()?;
    let id = GroupId(u32::try_from(d.get(b"Id").and_then(Object::as_int)?).ok()?);
    let name = string_of(d.get(b"Name")?)?;
    let unit = Unit::parse(&string_of(d.get(b"Unit")?)?)?;
    let scale = match name_of(d.get(b"Scale"))?.as_slice() {
        b"never" => ScaleState::NeverSet,
        b"one" => ScaleState::OneToOne,
        b"calibrated" => ScaleState::Calibrated {
            scale: d.get(b"ScaleValue").and_then(Object::as_number)?,
        },
        _ => return None,
    };
    let fraction = match name_of(d.get(b"Frac"))?.as_slice() {
        b"decimal" => FractionMode::Decimal {
            places: u32::try_from(d.get(b"Places").and_then(Object::as_int).unwrap_or(2)).ok()?,
        },
        b"fraction" => FractionMode::Fraction {
            denominator: u32::try_from(d.get(b"Denom").and_then(Object::as_int).unwrap_or(16))
                .ok()?,
            reduce: bool_of(d.get(b"Reduce")).unwrap_or(false),
        },
        _ => return None,
    };
    let visible = bool_of(d.get(b"Visible")).unwrap_or(true);
    let ocg = d.get(b"Ocg").and_then(Object::as_reference);
    Some(Group {
        id,
        name,
        scale,
        format: NumberFormat {
            unit,
            fraction,
            // Both OPTIONAL keys at the existing schema version, absent
            // meaning the pre-27.2 behaviour — the same additive discipline
            // `/Offset` and `/TextAlong` use, and for the same reason: a
            // version bump would trip the write-side refusal on every existing
            // dimensioned file.
            decimal_marker: match name_of(d.get(b"DecimalMarker")).as_deref() {
                Some(b"comma") => DecimalMarker::Comma,
                _ => DecimalMarker::Point,
            },
        },
        ocg,
        visible,
        standard: match name_of(d.get(b"Standard")).as_deref() {
            Some(b"iso") => DimStandard::Iso,
            _ => DimStandard::Ansi,
        },
        style: {
            let k = read_style_keys(d);
            GroupStyle {
                text_height: k.text_height,
                line_width: k.line_width,
                arrow_length: k.arrow_length,
                arrow_form: k.arrow_form,
                color: k.color,
                tolerance: k.tolerance,
                tolerance_places: k.tolerance_places,
            }
        },
    })
}

// ---- dimension (de)serialization --------------------------------------------

/// A ce-dimension group's **display settings** — number format, scale and
/// drafting standard — as a COS object, for the clipboard (`Pass 173.1`).
///
/// # Why this goes through the whole-group codec
///
/// Same reason [`serialize_kind`] goes through the whole-record one: a second
/// encoder for the same fields is how the two drift, and here the drift would
/// be silent in the worst way — a ce dimension's LABEL is derived from its
/// group's scale and format, so a field added to the document codec and
/// forgotten here produces a pasted ce dimension showing **a different
/// number** with nothing erroring.
///
/// The `/Id`, `/Name`, `/Ocg` and `/Visible` this writes are placeholders and
/// are discarded on read: a group id means nothing in another document, the
/// name travels separately on the clip, and an optional-content group is a
/// document-level object the paste re-creates.
pub(crate) fn serialize_group_settings(
    format: &NumberFormat,
    scale: ScaleState,
    standard: DimStandard,
) -> Object {
    let mut group = Group::new(GroupId(0), "", format.unit);
    group.format = *format;
    group.scale = scale;
    group.standard = standard;
    serialize_group(&group)
}

/// Read back what [`serialize_group_settings`] wrote, discarding the
/// placeholders.
pub(crate) fn deserialize_group_settings(
    obj: &Object,
) -> Option<(NumberFormat, ScaleState, DimStandard)> {
    let g = deserialize_group(obj)?;
    Some((g.format, g.scale, g.standard))
}

/// One ce dimension's own style overrides as a COS object, for the clipboard
/// (`Pass 173.1`).
///
/// The bottom tier of the style cascade — the properties this ce dimension
/// alone carries, distinct from its group's defaults. Routed through the
/// whole-record codec for the same anti-drift reason as its two siblings
/// above.
pub(crate) fn serialize_overrides(style: &StyleOverrides) -> Object {
    serialize_dimension(&DimensionRecord {
        id: DimensionId(0),
        group: GroupId(0),
        // Not a style property, and the caller reads only the style keys
        // back. The text override travels on its own clip field
        // (`ClipAnnotation::Dimension::label_override`) rather than smuggled
        // through this throwaway record, because a reader of THIS function
        // would have no reason to look for it here.
        label_override: None,
        // Any kind will do; the caller reads only the style keys back, and
        // reusing the real encoder is the point.
        kind: DimensionKind::Linear {
            a: Point::new(0.0, 0.0),
            b: Point::new(1.0, 0.0),
            constraint: crate::vector::AxisConstraint::Aligned,
            offset: 0.0,
            text_along: 0.5,
        },
        annot: None,
        ap: None,
        style: *style,
    })
}

/// Read back what [`serialize_overrides`] wrote.
///
/// Returns [`StyleOverrides::default`] — every field `None`, meaning "inherit
/// everything from the group" — when the payload cannot be read. That is the
/// safe direction: a ce dimension with no overrides looks like its group,
/// which is what an operator who never set one expects.
pub(crate) fn deserialize_overrides(obj: &Object) -> StyleOverrides {
    deserialize_dimension(obj)
        .map(|r| r.style)
        .unwrap_or_default()
}

/// A [`DimensionKind`] alone, as a COS object, for the clipboard
/// (`Pass 169.0`).
///
/// # Why this goes through the whole-record codec rather than beside it
///
/// The kind's encoding lives inside [`serialize_dimension`], keyed by
/// `/Kind`. Writing a second encoder for the same enum is how the two drift:
/// a new `DimensionKind` variant would be added to one and not the other, and
/// the failure would be a clipboard that pastes the wrong shape — a
/// *plausible* wrong shape, since every variant is a valid ce dimension.
///
/// So this builds a throwaway record around the kind and reuses the one
/// encoder. The `/Id` and `/Group` it writes are placeholders and are
/// **discarded** on read: a dimension id means nothing in another document,
/// and the group travels by NAME on the clip (see
/// [`ClipAnnotation::Dimension`](crate::vector::ClipAnnotation)) precisely
/// because a `GroupId` does not survive the trip either.
pub(crate) fn serialize_kind(kind: &DimensionKind) -> Object {
    serialize_dimension(&DimensionRecord {
        id: DimensionId(0),
        group: GroupId(0),
        kind: kind.clone(),
        annot: None,
        ap: None,
        // The GEOMETRY encoder. The text override is not geometry and rides
        // its own clip field — see `serialize_overrides` above.
        label_override: None,
        style: crate::dimension::style::StyleOverrides::default(),
    })
}

/// Read back what [`serialize_kind`] wrote, discarding the placeholder ids.
pub(crate) fn deserialize_kind(obj: &Object) -> Option<DimensionKind> {
    deserialize_dimension(obj).map(|record| record.kind)
}

fn serialize_dimension(dim: &DimensionRecord) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"Id"), Object::Integer(i64::from(dim.id.0)));
    d.insert(
        Name::from(b"Group"),
        Object::Integer(i64::from(dim.group.0)),
    );
    match dim.kind {
        DimensionKind::Linear {
            a,
            b,
            constraint,
            offset,
            text_along,
        } => {
            d.insert(Name::from(b"Kind"), Object::Name(Name::from(b"linear")));
            d.insert(Name::from(b"A"), point_array(a));
            d.insert(Name::from(b"B"), point_array(b));
            d.insert(
                Name::from(b"Constraint"),
                Object::Name(Name(constraint_token(constraint).to_vec())),
            );
            // OPTIONAL, and deliberately NOT a schema-version bump.
            //
            // NOTE, corrected 2026-08-12: this comment used to say the gate
            // was an exact equality. It is a RANGE and has been since that
            // gate was rewritten — see `deserialize_model`, which reads older
            // AND newer sidecars and leans on `sidecar_version` to refuse the
            // WRITE. The reasoning below still holds for why this key needed
            // no bump; only the description of the gate was stale.
            //
            // The original argument, still valid: bumping the version for a
            // key that is optional-with-default buys nothing and costs
            // compatibility.
            //
            // An absent key reads back as the 0.0 default, which draws exactly
            // what the pre-27.0 build drew. Written only when non-zero, so a
            // file that never used a standoff keeps byte-identical sidecar
            // output.
            if offset != 0.0 {
                d.insert(Name::from(b"Offset"), Object::Real(offset));
            }
            // Same optional-key discipline as /Offset: absent means centred,
            // which is where every pre-27.1 label sits.
            if text_along != 0.0 {
                d.insert(Name::from(b"TextAlong"), Object::Real(text_along));
            }
        }
        DimensionKind::Angular {
            apex,
            dir_a,
            dir_b,
            radius,
            text_along,
        } => {
            // ★ THIS key is why SIDECAR_VERSION went to 2, unlike /Offset and
            // /TextAlong which were optional-with-default and needed no bump.
            //
            // A new KIND is not a defaultable key. A build that does not know
            // the token `angular` hits `_ => return None` below and drops the
            // record entirely — so without a version bump an older pdfcer would
            // read the file, silently lose every angular ce dimension, and be
            // free to save it back that way. Permanent loss, invisible until
            // afterwards.
            //
            // With the bump, an older build still reads what it understands
            // but `sidecar_version` makes the session REFUSE to write
            // (`EditError::SidecarWrittenByNewerBuild`). Reading stays safe;
            // the destructive half is the one that is blocked.
            d.insert(Name::from(b"Kind"), Object::Name(Name::from(b"angular")));
            d.insert(Name::from(b"Apex"), point_array(apex));
            d.insert(Name::from(b"DirA"), point_array(dir_a));
            d.insert(Name::from(b"DirB"), point_array(dir_b));
            d.insert(Name::from(b"ArcRadius"), Object::Real(radius));
            if text_along != 0.0 {
                d.insert(Name::from(b"TextAlong"), Object::Real(text_along));
            }
        }
        DimensionKind::Perimeter {
            ref points,
            closed,
            offset,
            text_along,
        } => {
            // ★ THIS key is why SIDECAR_VERSION went to 3 — see the constant's
            // own history note. Same argument as `angular`: an unknown kind
            // token drops the record, and a dropped record is permanent loss
            // the moment the older build saves.
            d.insert(Name::from(b"Kind"), Object::Name(Name::from(b"perimeter")));
            // FLAT `[x1 y1 x2 y2 ...]`, not an array of `[x y]` pairs, even
            // though `point_array` exists and every other key here uses it.
            //
            // The reason is the ANNOTATION: `/Vertices` (ISO 32000-1
            // §12.5.6.9 Table 178) is flat, and the sidecar's copy of the same
            // geometry is the thing a reader diffs against it when the two
            // disagree (decision 011 §2.4: disagreement is disclosed, and the
            // sidecar wins). Two layouts for one geometry would make that
            // comparison a transposition, and a transposition is a place to
            // get an index wrong.
            let mut flat = Vec::with_capacity(points.len() * 2);
            for p in points {
                flat.push(Object::Real(p.x));
                flat.push(Object::Real(p.y));
            }
            d.insert(Name::from(b"Points"), Object::Array(flat));
            // Written ALWAYS, not optional-when-false, and that breaks this
            // file's own optional-key discipline on purpose: open and closed
            // are two shapes the operator picks deliberately between, so an
            // absent key would have to silently mean one of them. There is no
            // legacy to be compatible with — the kind is new at this version —
            // so nothing is bought by defaulting it and a real fact is lost.
            d.insert(Name::from(b"Closed"), Object::Boolean(closed));
            // Same optional-key discipline as the linear arm: a label that
            // was never dragged adds no keys.
            if offset != 0.0 {
                d.insert(Name::from(b"Offset"), Object::Real(offset));
            }
            if text_along != 0.0 {
                d.insert(Name::from(b"TextAlong"), Object::Real(text_along));
            }
        }
        DimensionKind::Circular { fit, show_diameter } => {
            d.insert(Name::from(b"Kind"), Object::Name(Name::from(b"circular")));
            d.insert(Name::from(b"Center"), point_array(fit.center));
            d.insert(Name::from(b"Radius"), Object::Real(fit.radius));
            d.insert(Name::from(b"Residual"), Object::Real(fit.residual));
            d.insert(Name::from(b"Diameter"), Object::Boolean(show_diameter));
        }
    }
    if let Some(annot) = dim.annot {
        d.insert(Name::from(b"Annot"), Object::Reference(annot));
    }
    if let Some(ap) = dim.ap {
        d.insert(Name::from(b"Ap"), Object::Reference(ap));
    }
    // The Pass 69.0 per-ce-dimension overrides. Same optional-key discipline
    // as `/Offset`: written only where the operator actually ticked the
    // override, so a ce dimension that inherits everything adds no keys and the
    // sidecar bytes are unchanged from before this Pass.
    //
    // The measurement-side overrides use DIFFERENT key names from the group's
    // (`/OvUnit` rather than `/Unit`) even though the values are identical in
    // shape. Reusing `/Unit` inside a dimension dict would be a key that means
    // "the value" in one dict and "the override, absence meaning inherit" in
    // another - the same word for two different contracts, one grep apart.
    // The `Pass 175.0` text override (decision 097). Optional-key discipline
    // as everywhere else in this dict — a ce dimension that prints its
    // measurement adds no key, so every sidecar written before this Pass
    // re-serialises byte-identically (R34).
    //
    // A PDF TEXT STRING (§7.9.2.2) via `encode_text_string`, not raw UTF-8
    // bytes: this value is operator-typed, so it is the first thing in this
    // dict that can contain a character outside ASCII, and a raw-UTF-8 write
    // would be read back by `decode_text_string` as PDFDocEncoding — one byte
    // per byte — turning an e-acute into two mojibake characters on the round
    // trip. The encoder picks PDFDocEncoding when the whole string fits and
    // UTF-16BE otherwise, and the decoder is driven by the BOM, so the pair
    // is self-describing.
    //
    // Note this is a WIDER repertoire than the caption can actually DRAW
    // (`WinAnsiEncoding`, enforced at `EditSession::set_dimension_label`).
    // That asymmetry is intentional: storage must be able to round-trip
    // whatever a future font capability lets the baker draw, and a storage
    // format that could hold less than the verb accepts would turn a
    // widening of the verb into a silent truncation of old files.
    if let Some(text) = dim.label_override.as_deref() {
        d.insert(
            Name::from(b"LabelOverride"),
            Object::String(crate::textstring::encode_text_string(text)),
        );
    }
    put_override_keys(&mut d, &dim.style);
    put_style_keys(
        &mut d,
        StyleKeys {
            text_height: dim.style.text_height,
            line_width: dim.style.line_width,
            arrow_length: dim.style.arrow_length,
            arrow_form: dim.style.arrow_form,
            color: dim.style.color,
            tolerance: dim.style.tolerance,
            tolerance_places: dim.style.tolerance_places,
        },
    );
    Object::Dict(d)
}

fn deserialize_dimension(obj: &Object) -> Option<DimensionRecord> {
    let d = obj.as_dict()?;
    let id = DimensionId(u32::try_from(d.get(b"Id").and_then(Object::as_int)?).ok()?);
    let group = GroupId(u32::try_from(d.get(b"Group").and_then(Object::as_int)?).ok()?);
    let kind = match name_of(d.get(b"Kind"))?.as_slice() {
        b"linear" => DimensionKind::Linear {
            a: point_of(d.get(b"A")?)?,
            b: point_of(d.get(b"B")?)?,
            constraint: parse_constraint(&name_of(d.get(b"Constraint"))?)?,
            // Absent in every sidecar written before Pass 27.0. The 0.0
            // default is what makes that migration free rather than lossy.
            offset: placement_of(d.get(b"Offset")),
            text_along: placement_of(d.get(b"TextAlong")),
        },
        b"angular" => DimensionKind::Angular {
            apex: point_of(d.get(b"Apex")?)?,
            dir_a: point_of(d.get(b"DirA")?)?,
            dir_b: point_of(d.get(b"DirB")?)?,
            radius: d.get(b"ArcRadius").and_then(Object::as_number)?,
            text_along: placement_of(d.get(b"TextAlong")),
        },
        b"perimeter" => DimensionKind::Perimeter {
            points: flat_points(d.get(b"Points")?)?,
            // Absent means OPEN. That is the safe reading rather than the
            // symmetric one: the write side always emits this key, so an
            // absent one means a hand-edited or truncated sidecar, and
            // reconstructing an open path from a possibly-closed one under-
            // reports the length by one segment instead of inventing a
            // segment that may cross the drawing.
            closed: bool_of(d.get(b"Closed")).unwrap_or(false),
            offset: placement_of(d.get(b"Offset")),
            text_along: placement_of(d.get(b"TextAlong")),
        },
        b"circular" => DimensionKind::Circular {
            fit: FitCircle {
                center: point_of(d.get(b"Center")?)?,
                radius: d.get(b"Radius").and_then(Object::as_number)?,
                residual: d
                    .get(b"Residual")
                    .and_then(Object::as_number)
                    .unwrap_or(0.0),
            },
            show_diameter: bool_of(d.get(b"Diameter")).unwrap_or(false),
        },
        _ => return None,
    };
    let appearance = read_style_keys(d);
    Some(DimensionRecord {
        id,
        group,
        kind,
        annot: d.get(b"Annot").and_then(Object::as_reference),
        ap: d.get(b"Ap").and_then(Object::as_reference),
        // Absent ⇒ this ce dimension prints its measurement, which is what
        // every sidecar written before `Pass 175.0` means and what a fresh
        // one means today. The decoder's replacement count is deliberately
        // ignored: a malformed stored string is still the closest thing to
        // what the operator typed that survives, and dropping the whole
        // override because one byte was undecodable would silently restore
        // the measured caption — the exact failure this key exists to
        // prevent, arrived at from the reader side.
        label_override: match d.get(b"LabelOverride") {
            Some(Object::String(bytes)) => Some(crate::textstring::decode_text_string(bytes).text),
            _ => None,
        },
        style: StyleOverrides {
            unit: name_of(d.get(b"OvUnit"))
                .and_then(|n| String::from_utf8(n).ok())
                .and_then(|t| Unit::parse(&t)),
            fraction: read_override_fraction(d),
            decimal_marker: match name_of(d.get(b"OvDecimalMarker")).as_deref() {
                Some(b"comma") => Some(DecimalMarker::Comma),
                Some(b"point") => Some(DecimalMarker::Point),
                _ => None,
            },
            standard: match name_of(d.get(b"OvStandard")).as_deref() {
                Some(b"iso") => Some(DimStandard::Iso),
                Some(b"ansi") => Some(DimStandard::Ansi),
                _ => None,
            },
            text_height: appearance.text_height,
            line_width: appearance.line_width,
            arrow_length: appearance.arrow_length,
            arrow_form: appearance.arrow_form,
            color: appearance.color,
            tolerance: appearance.tolerance,
            tolerance_places: appearance.tolerance_places,
        },
    })
}

// ---- style (de)serialization (Pass 69.0) ------------------------------------

/// The five APPEARANCE properties, as `Option`s - the shape both tiers of the
/// cascade store, and therefore the shape both read and write.
///
/// A private carrier type rather than ten positional arguments: the group tier
/// ([`GroupStyle`]) and the ce-dimension tier ([`StyleOverrides`]) have
/// different field sets overall but identical appearance halves, and the whole
/// point of a shared writer is that the two can never encode the same property
/// differently.
struct StyleKeys {
    text_height: Option<f64>,
    line_width: Option<f64>,
    arrow_length: Option<f64>,
    arrow_form: Option<ArrowForm>,
    color: Option<Rgb>,
    tolerance: Option<Tolerance>,
    tolerance_places: Option<u32>,
}

/// Write the appearance keys that are actually set. Absent = inherit.
fn put_style_keys(d: &mut Dict, k: StyleKeys) {
    if let Some(v) = k.text_height {
        d.insert(Name::from(b"TextHeight"), Object::Real(v));
    }
    if let Some(v) = k.line_width {
        d.insert(Name::from(b"LineWidth"), Object::Real(v));
    }
    if let Some(v) = k.arrow_length {
        d.insert(Name::from(b"ArrowLength"), Object::Real(v));
    }
    if let Some(v) = k.arrow_form {
        d.insert(
            Name::from(b"ArrowForm"),
            Object::Name(Name::from(v.token().as_bytes())),
        );
    }
    if let Some(c) = k.color {
        d.insert(
            Name::from(b"Color"),
            Object::Array(vec![
                Object::Real(f64::from(c.r)),
                Object::Real(f64::from(c.g)),
                Object::Real(f64::from(c.b)),
            ]),
        );
    }
    // The tolerance (Pass 69.1): a type name plus, for the two numeric types,
    // a pair of values under names that say which is which. A positional
    // `[a b]` array would be one transposition away from turning a `+0.2/-0.1`
    // into a `-0.1/+0.2`, and nothing downstream could tell.
    if let Some(t) = k.tolerance {
        d.insert(
            Name::from(b"Tolerance"),
            Object::Name(Name::from(t.token().as_bytes())),
        );
        match t {
            Tolerance::Symmetric { magnitude } => {
                d.insert(Name::from(b"TolMagnitude"), Object::Real(magnitude));
            }
            Tolerance::Deviation { plus, minus } => {
                d.insert(Name::from(b"TolPlus"), Object::Real(plus));
                d.insert(Name::from(b"TolMinus"), Object::Real(minus));
            }
            Tolerance::Limit { upper, lower } => {
                d.insert(Name::from(b"TolUpper"), Object::Real(upper));
                d.insert(Name::from(b"TolLower"), Object::Real(lower));
            }
            Tolerance::None | Tolerance::Basic | Tolerance::Min | Tolerance::Max => {}
        }
    }
    if let Some(places) = k.tolerance_places {
        d.insert(Name::from(b"TolPlaces"), Object::Integer(i64::from(places)));
    }
}

/// Read the appearance keys back. Anything absent, malformed, or outside a
/// usable range reads as `None` - i.e. **inherit** - never as a wrong value.
///
/// # Why an out-of-range number inherits rather than clamping
///
/// These come out of the FILE. A `/LineWidth` of `-3` or `1e12` is corruption
/// or another product's bug, and clamping it to something plausible would draw
/// a ce dimension the operator never asked for while reporting nothing.
/// Falling back to inheritance draws what the group says, which is an answer
/// the operator DID give.
fn read_style_keys(d: &Dict) -> StyleKeys {
    StyleKeys {
        text_height: positive_of(d.get(b"TextHeight")),
        line_width: positive_of(d.get(b"LineWidth")),
        arrow_length: positive_of(d.get(b"ArrowLength")),
        arrow_form: name_of(d.get(b"ArrowForm"))
            .and_then(|n| String::from_utf8(n).ok())
            .and_then(|t| ArrowForm::parse(&t)),
        color: color_of(d.get(b"Color")),
        tolerance: read_tolerance(d),
        tolerance_places: d
            .get(b"TolPlaces")
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|p| *p <= 12),
    }
}

/// Read a tolerance back, or `None` for absent/malformed/invalid.
///
/// Runs the value through [`Tolerance::validate`] rather than trusting it,
/// which is the difference between a file describing a tolerance and a file
/// describing a tolerance that could be drawn. An inverted limit pair out of a
/// corrupted file would otherwise print a drawing stating the maximum is below
/// the minimum — a manufacturing defect delivered by a parser.
fn read_tolerance(d: &Dict) -> Option<Tolerance> {
    let num = |key: &[u8]| d.get(key).and_then(Object::as_number);
    let t = match name_of(d.get(b"Tolerance"))?.as_slice() {
        b"none" => Tolerance::None,
        b"basic" => Tolerance::Basic,
        b"min" => Tolerance::Min,
        b"max" => Tolerance::Max,
        b"symmetric" => Tolerance::Symmetric {
            magnitude: num(b"TolMagnitude")?,
        },
        b"deviation" => Tolerance::Deviation {
            plus: num(b"TolPlus")?,
            minus: num(b"TolMinus")?,
        },
        b"limit" => Tolerance::Limit {
            upper: num(b"TolUpper")?,
            lower: num(b"TolLower")?,
        },
        _ => return None,
    };
    t.validate().ok()
}

/// Write the four MEASUREMENT-side overrides (unit, fraction, marker,
/// standard), under `Ov`-prefixed keys. See the call site for why the prefix.
fn put_override_keys(d: &mut Dict, o: &StyleOverrides) {
    if let Some(u) = o.unit {
        d.insert(
            Name::from(b"OvUnit"),
            Object::Name(Name::from(u.token().as_bytes())),
        );
    }
    match o.fraction {
        Some(FractionMode::Decimal { places }) => {
            d.insert(Name::from(b"OvFrac"), Object::Name(Name::from(b"decimal")));
            d.insert(Name::from(b"OvPlaces"), Object::Integer(i64::from(places)));
        }
        Some(FractionMode::Fraction {
            denominator,
            reduce,
        }) => {
            d.insert(Name::from(b"OvFrac"), Object::Name(Name::from(b"fraction")));
            d.insert(
                Name::from(b"OvDenom"),
                Object::Integer(i64::from(denominator)),
            );
            d.insert(Name::from(b"OvReduce"), Object::Boolean(reduce));
        }
        None => {}
    }
    if let Some(m) = o.decimal_marker {
        d.insert(
            Name::from(b"OvDecimalMarker"),
            Object::Name(Name::from(match m {
                DecimalMarker::Comma => b"comma".as_slice(),
                DecimalMarker::Point => b"point".as_slice(),
            })),
        );
    }
    if let Some(std) = o.standard {
        d.insert(
            Name::from(b"OvStandard"),
            Object::Name(Name::from(match std {
                DimStandard::Iso => b"iso".as_slice(),
                DimStandard::Ansi => b"ansi".as_slice(),
            })),
        );
    }
}

/// The per-ce-dimension fraction/precision override, or `None` to inherit.
///
/// Unlike the group's reader, a missing `/OvPlaces` does NOT fall back to a
/// default of 2: an `/OvFrac /decimal` with no digit count is a malformed
/// override, and inventing a precision the file does not state is exactly the
/// silent substitution rule 4 forbids. It inherits instead.
fn read_override_fraction(d: &Dict) -> Option<FractionMode> {
    match name_of(d.get(b"OvFrac"))?.as_slice() {
        b"decimal" => {
            let places = u32::try_from(d.get(b"OvPlaces").and_then(Object::as_int)?).ok()?;
            // A fixed-decimal format with more than a dozen places is not a
            // format, it is a corrupted integer; the formatter would emit a
            // label nobody can read.
            (places <= 12).then_some(FractionMode::Decimal { places })
        }
        b"fraction" => {
            let denominator = u32::try_from(d.get(b"OvDenom").and_then(Object::as_int)?).ok()?;
            (denominator > 0 && denominator <= 4096).then_some(FractionMode::Fraction {
                denominator,
                reduce: bool_of(d.get(b"OvReduce")).unwrap_or(false),
            })
        }
        _ => None,
    }
}

/// A strictly-positive, finite, sanely-bounded number from the file, or `None`.
///
/// The bound is [`MAX_PAGE_VALUE`] for the same reason the geometry guard uses
/// it: a text height of 1e9 points is not a preference, and letting it reach
/// the writer produces a `/Rect` that breaks every reader downstream.
fn positive_of(obj: Option<&Object>) -> Option<f64> {
    let v = obj.and_then(Object::as_number)?;
    (v.is_finite() && v > 0.0 && v <= MAX_PAGE_VALUE).then_some(v)
}

/// An `[r g b]` colour array with every component in 0.0-1.0, or `None`.
///
/// Out-of-range components inherit rather than clamp - same argument as
/// [`read_style_keys`]: a clamped colour is a colour the operator never chose.
fn color_of(obj: Option<&Object>) -> Option<Rgb> {
    let a = obj.and_then(Object::as_array)?;
    if a.len() != 3 {
        return None;
    }
    let mut c = [0.0f32; 3];
    for (slot, o) in c.iter_mut().zip(a) {
        let v = o.as_number()?;
        if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        // A 0.0-1.0 colour component into f32 is lossless enough for a paint
        // value; `Rgb` is f32 because `pdfcer-render`'s pipeline is.
        {
            *slot = v as f32;
        }
    }
    Some(Rgb {
        r: c[0],
        g: c[1],
        b: c[2],
    })
}

// ---- small object helpers ---------------------------------------------------

fn point_array(p: Point) -> Object {
    Object::Array(vec![Object::Real(p.x), Object::Real(p.y)])
}

/// The largest page-space magnitude a sidecar value may claim (Pass 27.3).
///
/// PDF's own architectural limit for a page dimension is 14,400 units (200
/// inches, Annex C.1), so a coordinate or standoff three orders past that is
/// not geometry — it is corruption, a hand edit, or another product's bug.
/// The ceiling is deliberately generous rather than tight: the job here is to
/// stop absurdity reaching the writer, not to second-guess an unusual drawing.
pub const MAX_PAGE_VALUE: f64 = 1.0e7;

/// Whether a page-space number is usable.
///
/// **Public since `Pass 107.0`** and no longer only about file-supplied
/// values: [`crate::edit::EditSession`]'s vertex verbs hold caller-supplied
/// coordinates to the identical test before they reach the writer. One
/// function rather than one rule written twice — two definitions of "a usable
/// page coordinate" would drift, and the drift would show up as a NaN that the
/// reader rejects but the writer accepted (R92).
///
/// # Why this guard exists
///
/// These values come out of the FILE, and everything downstream of them is
/// geometry that ends up in `/Rect` and `/L`. Measured on 2026-08-05, with no
/// guard:
///
/// - `/Offset 1e308` wrote a **300-digit decimal** into `/Rect`, far past
///   PDF's ~3.4e38 architectural limit for a real;
/// - `/Offset inf` made the dimension **silently vanish** — `/Rect [-2 -2 3
///   3]`, `/L [0 0 0 0]` — while `/Contents` still read "200.00 pt". A
///   measurement that disappears while still claiming a value is the worst of
///   the available outcomes, because nothing on screen says anything is wrong.
///
/// The bounds accumulator already drops non-finite points, which is what makes
/// the failure quiet rather than loud. This stops it upstream instead.
#[must_use]
pub fn usable_page_value(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_PAGE_VALUE
}

/// A file-supplied placement scalar, or the 0.0 default if it is unusable.
///
/// Defaulting rather than dropping the record: a standoff is a presentation
/// detail with a meaningful zero, so a corrupt one costs the operator the
/// dimension's POSITION, not the dimension. The measured points are held to a
/// stricter standard below, because a dimension whose geometry is corrupt has
/// no meaning to preserve.
fn placement_of(obj: Option<&Object>) -> f64 {
    obj.and_then(Object::as_number)
        .filter(|v| usable_page_value(*v))
        .unwrap_or(0.0)
}

/// A flat `[x1 y1 x2 y2 ...]` array read back as points (`Pass 107.0`).
///
/// `None` — which drops the whole ce-dimension record — for anything that is
/// not an even-length array of at least two coordinates, or that carries a
/// non-finite or out-of-page value. A perimeter reconstructed from half a
/// vertex list would draw a shape the operator never picked AND print a length
/// nobody measured, which is worse than the record being reported missing:
/// the first is a wrong measurement that looks right.
fn flat_points(obj: &Object) -> Option<Vec<Point>> {
    let arr = obj.as_array()?;
    if arr.len() < 2 || arr.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len() / 2);
    // A pairwise walk over the iterator rather than `chunks_exact(2)` plus
    // indexing: the even-length check above already guarantees the pairs, and
    // this form has no index to be out of bounds (`clippy::indexing_slicing`,
    // ARCHITECTURE.md §10).
    let mut it = arr.iter();
    while let (Some(xo), Some(yo)) = (it.next(), it.next()) {
        let x = xo.as_number()?;
        let y = yo.as_number()?;
        if !usable_page_value(x) || !usable_page_value(y) {
            return None;
        }
        out.push(Point::new(x, y));
    }
    Some(out)
}

fn point_of(obj: &Object) -> Option<Point> {
    let a = obj.as_array()?;
    let x = a.first()?.as_number()?;
    let y = a.get(1)?.as_number()?;
    // `None` drops the whole dimension record — the sidecar's existing
    // malformed-entry posture. A measured point that is infinite or absurd
    // does not describe anything, and keeping the record would mean drawing a
    // dimension between coordinates nobody chose.
    (usable_page_value(x) && usable_page_value(y)).then(|| Point::new(x, y))
}

const fn constraint_token(c: AxisConstraint) -> &'static [u8] {
    match c {
        AxisConstraint::Aligned => b"aligned",
        AxisConstraint::Horizontal => b"horizontal",
        AxisConstraint::Vertical => b"vertical",
    }
}

fn parse_constraint(bytes: &[u8]) -> Option<AxisConstraint> {
    match bytes {
        b"aligned" => Some(AxisConstraint::Aligned),
        b"horizontal" => Some(AxisConstraint::Horizontal),
        b"vertical" => Some(AxisConstraint::Vertical),
        _ => None,
    }
}

fn string_of(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

fn name_of(obj: Option<&Object>) -> Option<Vec<u8>> {
    obj?.as_name().map(|n| n.as_bytes().to_vec())
}

fn bool_of(obj: Option<&Object>) -> Option<bool> {
    match obj? {
        Object::Boolean(b) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::dimension::group::DEFAULT_GROUP_ID;
    use crate::object::ObjId;

    fn sample_model() -> DimensionModel {
        let mut m = DimensionModel::new();
        // Calibrate the default group and add a couple of groups + dims.
        m.set_group_scale(
            DEFAULT_GROUP_ID,
            ScaleState::Calibrated { scale: 0.05 },
            NumberFormat::decimal(Unit::Meter, 3),
        );
        let fp = m.add_group("Floor Plan", Unit::FeetInches);
        m.set_group_scale(
            fp,
            ScaleState::OneToOne,
            NumberFormat::feet_inches(8, false),
        );
        m.set_group_visible(fp, false);
        let d1 = m.add_dimension(
            DEFAULT_GROUP_ID,
            DimensionKind::Linear {
                a: Point::new(1.0, 2.0),
                b: Point::new(3.0, 4.0),
                constraint: AxisConstraint::Horizontal,
                offset: 0.0,
                text_along: 0.0,
            },
        );
        // Wire fake object handles to prove they round-trip.
        m.dimension_mut(d1).unwrap().annot = Some(ObjId::new(20, 0));
        m.dimension_mut(d1).unwrap().ap = Some(ObjId::new(21, 0));
        m.add_dimension(
            fp,
            DimensionKind::Circular {
                fit: FitCircle {
                    center: Point::new(50.0, 60.0),
                    radius: 12.5,
                    residual: 0.3,
                },
                show_diameter: true,
            },
        );
        m.group_mut(fp).unwrap().ocg = Some(ObjId::new(30, 0));
        m
    }

    #[test]
    fn model_round_trips_through_the_sidecar() {
        let m = sample_model();
        let obj = serialize_model(&m);
        let back = deserialize_model(&obj).expect("valid sidecar");
        assert_eq!(back, m, "sidecar round-trip must be lossless");
    }

    #[test]
    fn a_malformed_sidecar_yields_none_not_panic() {
        assert!(deserialize_model(&Object::Null).is_none());
        assert!(deserialize_model(&Object::Integer(3)).is_none());
        // Wrong version.
        let mut d = Dict::new();
        d.insert(Name::from(b"Version"), Object::Integer(999));
        assert!(deserialize_model(&Object::Dict(d)).is_none());
        // Right version but no default group → incoherent → None.
        let mut d2 = Dict::new();
        d2.insert(Name::from(b"Version"), Object::Integer(SIDECAR_VERSION));
        d2.insert(Name::from(b"Groups"), Object::Array(vec![]));
        assert!(deserialize_model(&Object::Dict(d2)).is_none());
    }

    #[test]
    fn wiring_handles_and_ocg_survive_the_round_trip() {
        let m = sample_model();
        let back = deserialize_model(&serialize_model(&m)).unwrap();
        let d1 = back.dimensions()[0].clone();
        assert_eq!(d1.annot, Some(ObjId::new(20, 0)));
        assert_eq!(d1.ap, Some(ObjId::new(21, 0)));
        let fp = back
            .groups()
            .iter()
            .find(|g| g.name == "Floor Plan")
            .unwrap();
        assert_eq!(fp.ocg, Some(ObjId::new(30, 0)));
        assert!(!fp.visible);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod angular_sidecar_tests {
    use super::*;
    use crate::dimension::group::{DEFAULT_GROUP_ID, DimensionKind, DimensionModel};
    use crate::vector::Point;

    fn wedge() -> DimensionKind {
        let r = 42.5f64.to_radians();
        DimensionKind::Angular {
            apex: Point::new(120.0, 90.0),
            dir_a: Point::new(1.0, 0.0),
            dir_b: Point::new(r.cos(), r.sin()),
            radius: 55.0,
            text_along: 7.5,
        }
    }

    /// An angular ce dimension survives serialise → deserialise unchanged.
    #[test]
    fn an_angular_dimension_round_trips() {
        let mut model = DimensionModel::new();
        let id = model.add_dimension(DEFAULT_GROUP_ID, wedge());
        let back = deserialize_model(&serialize_model(&model)).expect("must deserialise");
        let d = back.dimension(id).expect("the record must survive");
        assert_eq!(d.kind, wedge(), "the geometry must round-trip exactly");
    }

    /// ★ The version bump is REAL, and this is why it had to happen.
    ///
    /// A new kind is not a defaultable key. An older build hits the
    /// `_ => return None` arm for the unknown token and drops the record —
    /// so without the bump it would read the file, silently lose every
    /// angular ce dimension, and be free to save it back that way.
    ///
    /// Asserting the number directly rather than the mechanism, because the
    /// mechanism (`sidecar_version` gating the write) already has its own
    /// test; what this pins is that somebody did not later "tidy" the version
    /// back down while the `angular` token stayed.
    #[test]
    fn writing_an_angular_dimension_declares_a_version_old_builds_will_refuse() {
        let mut model = DimensionModel::new();
        model.add_dimension(DEFAULT_GROUP_ID, wedge());
        let obj = serialize_model(&model);
        let v = sidecar_version(&obj).expect("a version must be written");
        assert!(
            v >= 2,
            "a sidecar containing an `angular` kind must declare version 2 or \
             later, or an older pdfcer will drop those dimensions and then be \
             allowed to overwrite the file without them; got {v}"
        );
    }

    /// A version-1 sidecar (no angular dimensions) still loads.
    ///
    /// The other half of the migration: bumping the version must not orphan
    /// every file written before it.
    #[test]
    fn a_version_one_sidecar_still_loads() {
        let mut model = DimensionModel::new();
        model.add_dimension(
            DEFAULT_GROUP_ID,
            DimensionKind::Circular {
                fit: crate::dimension::fit::FitCircle {
                    center: Point::new(10.0, 10.0),
                    radius: 5.0,
                    residual: 0.0,
                },
                show_diameter: false,
            },
        );
        let Object::Dict(mut d) = serialize_model(&model) else {
            panic!("the sidecar is a dictionary");
        };
        d.insert(Name::from(b"Version"), Object::Integer(1));
        let back = deserialize_model(&Object::Dict(d)).expect("a v1 sidecar must still load");
        assert_eq!(
            back.dimensions().len(),
            1,
            "and must keep its dimensions rather than coming back empty"
        );
    }
}

/// Pass 69.0 - the style cascade's sidecar half.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod style_sidecar_tests {
    use super::*;
    use crate::dimension::group::DEFAULT_GROUP_ID;
    use crate::vector::Point;

    /// A model with one calibrated group and one linear ce dimension, and NO
    /// style anywhere - the pre-Pass-69.0 shape, which is what the
    /// compatibility assertions below need.
    fn sample_model() -> DimensionModel {
        let mut m = DimensionModel::new();
        m.set_group_scale(
            DEFAULT_GROUP_ID,
            ScaleState::Calibrated { scale: 0.05 },
            NumberFormat::decimal(Unit::Meter, 3),
        );
        m.add_dimension(
            DEFAULT_GROUP_ID,
            DimensionKind::Linear {
                a: Point::new(1.0, 2.0),
                b: Point::new(3.0, 4.0),
                constraint: crate::vector::AxisConstraint::Horizontal,
                offset: 0.0,
                text_along: 0.0,
            },
        );
        m
    }

    /// ★ The compatibility claim, asserted on BYTES rather than on behaviour.
    ///
    /// A model whose groups and ce dimensions carry no style must serialise to
    /// exactly what it serialised to before the style keys existed. Asserting
    /// "it round-trips" would pass even if every dict gained five default keys
    /// — and that is precisely the failure mode: a sidecar that grows keys on
    /// every save makes each save dirty an object R34 says is untouched.
    #[test]
    fn an_unstyled_model_writes_no_style_keys_at_all() {
        let model = sample_model();
        let Object::Dict(d) = serialize_model(&model) else {
            panic!("the sidecar is a dictionary");
        };
        let groups = d.get(b"Groups").and_then(Object::as_array).unwrap();
        let dims = d.get(b"Dimensions").and_then(Object::as_array).unwrap();
        for entry in groups.iter().chain(dims.iter()) {
            let e = entry.as_dict().unwrap();
            for key in [
                b"TextHeight".as_slice(),
                b"LineWidth",
                b"ArrowLength",
                b"ArrowForm",
                b"Color",
                b"OvUnit",
                b"OvFrac",
                b"OvStandard",
                b"OvDecimalMarker",
                b"Tolerance",
                b"TolPlaces",
            ] {
                assert!(
                    e.get(key).is_none(),
                    "an unstyled entry must not gain the key {}",
                    String::from_utf8_lossy(key)
                );
            }
        }
    }

    #[test]
    fn group_and_dimension_style_round_trip() {
        let mut model = sample_model();
        model.group_mut(DEFAULT_GROUP_ID).unwrap().style = GroupStyle {
            text_height: Some(14.0),
            line_width: Some(1.5),
            arrow_length: None,
            arrow_form: Some(ArrowForm::Slash),
            color: Some(Rgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            }),
            tolerance: Some(Tolerance::Symmetric { magnitude: 0.25 }),
            tolerance_places: Some(3),
        };
        let first = model.dimensions()[0].id;
        model.dimension_mut(first).unwrap().style = StyleOverrides {
            unit: Some(Unit::Inch),
            fraction: Some(FractionMode::Fraction {
                denominator: 16,
                reduce: true,
            }),
            decimal_marker: Some(DecimalMarker::Comma),
            standard: Some(DimStandard::Iso),
            text_height: Some(9.0),
            line_width: None,
            arrow_length: Some(5.0),
            arrow_form: Some(ArrowForm::Dot),
            color: None,
            tolerance: Some(Tolerance::Deviation {
                plus: 0.2,
                minus: -0.1,
            }),
            tolerance_places: None,
        };

        let back = deserialize_model(&serialize_model(&model)).expect("round trip");
        assert_eq!(
            back.group(DEFAULT_GROUP_ID).unwrap().style,
            model.group(DEFAULT_GROUP_ID).unwrap().style
        );
        assert_eq!(
            back.dimension(first).unwrap().style,
            model.dimension(first).unwrap().style
        );
        // The un-set halves must come back as `None` (inherit), not as some
        // materialised default — that distinction IS the feature.
        assert!(
            back.group(DEFAULT_GROUP_ID)
                .unwrap()
                .style
                .arrow_length
                .is_none()
        );
        assert!(back.dimension(first).unwrap().style.line_width.is_none());
    }

    /// A file-supplied style value that is absurd or malformed must read as
    /// **inherit**, never as a clamped substitute the operator never chose.
    #[test]
    fn corrupt_style_values_fall_back_to_inheritance() {
        let mut model = sample_model();
        let Object::Dict(mut d) = serialize_model(&model) else {
            panic!("dict");
        };
        let mut groups = d
            .get(b"Groups")
            .and_then(Object::as_array)
            .unwrap()
            .to_vec();
        let Object::Dict(ref mut g0) = groups[0] else {
            panic!("dict");
        };
        g0.insert(Name::from(b"TextHeight"), Object::Real(-4.0));
        g0.insert(Name::from(b"LineWidth"), Object::Real(1.0e12));
        g0.insert(
            Name::from(b"ArrowForm"),
            Object::Name(Name::from(b"lightning-bolt")),
        );
        g0.insert(
            Name::from(b"Color"),
            Object::Array(vec![
                Object::Real(2.0),
                Object::Real(0.0),
                Object::Real(0.0),
            ]),
        );
        d.insert(Name::from(b"Groups"), Object::Array(groups));

        let back = deserialize_model(&Object::Dict(d)).expect("still a valid sidecar");
        let style = back.group(DEFAULT_GROUP_ID).unwrap().style;
        assert!(style.text_height.is_none(), "a negative height inherits");
        assert!(style.line_width.is_none(), "an absurd width inherits");
        assert!(style.arrow_form.is_none(), "an unknown form inherits");
        assert!(style.color.is_none(), "an out-of-range colour inherits");
        // And the group is otherwise intact — one bad key must not cost the
        // scale the operator calibrated.
        model.set_group_visible(DEFAULT_GROUP_ID, true);
        assert_eq!(
            back.group(DEFAULT_GROUP_ID).unwrap().scale,
            model.group(DEFAULT_GROUP_ID).unwrap().scale
        );
    }

    /// An `/OvFrac /decimal` with no `/OvPlaces` is malformed. It must inherit
    /// rather than invent a precision — the group's reader defaults to 2
    /// places because a group MUST have a format; an override must not.
    #[test]
    fn a_malformed_precision_override_inherits_rather_than_inventing_one() {
        let mut d = Dict::new();
        d.insert(Name::from(b"OvFrac"), Object::Name(Name::from(b"decimal")));
        assert!(read_override_fraction(&d).is_none());
        d.insert(Name::from(b"OvPlaces"), Object::Integer(3));
        assert_eq!(
            read_override_fraction(&d),
            Some(FractionMode::Decimal { places: 3 })
        );
        // Absurd precision is corruption, not a preference.
        d.insert(Name::from(b"OvPlaces"), Object::Integer(9999));
        assert!(read_override_fraction(&d).is_none());
    }
}
