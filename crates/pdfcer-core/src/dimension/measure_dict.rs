//! # Portable `/Measure` + optional-content authoring (ISO 32000-1 §12.9, §8.11)
//!
//! Pure PDF-object builders for the two **reader-visible** halves of the
//! hybrid storage (decision 011 §2.4): the §12.9 `/Measure` dict that mirrors
//! a group's scale into any §12.9-honouring reader, and the §8.11 `/OCG` +
//! `/OCProperties` optional-content layer. Everything here returns
//! [`Object`]s; [`crate::edit`] allocates object numbers and wires them in.
//!
//! ## The scale formula (spec-grounded, `iso32000__s__12.9.md`)
//!
//! The first `/C` of the `/X` NumberFormat array **is** the group scale:
//! `C = real_length_in_top_unit ÷ drawn_length_in_points`. `/R` ("1:100") is
//! DISPLAY-ONLY (parse-nothing); pdfcer sets `/R` and `/C` consistently. All
//! measurement is in **default user space** (any non-default `/UserUnit` is
//! folded into `/C` by the caller — the beta measures in points).
//!
//! ## Feet-inches (§12.9 Table 263, verbatim from the RAG)
//!
//! `/X [ <</U(ft)/C s>> <</U(in)/C 12 /F /F /D 8 /FD true>> ]` — two elements,
//! feet then inches, inch part a nearest-1/8 fraction with the denominator
//! kept (`/FD true`, don't reduce). Decimal units use a single element with
//! `/F /D` and `/D 10^places`.
//!
//! ## Optional content (§8.11, `iso32000__s__8.11.md`)
//!
//! One `/OCG` per group; the catalog `/OCProperties` **`/D` default-config
//! registration is MANDATORY** — a missing `/OCProperties` makes readers
//! ignore ALL optional content. Default-hidden groups go in `/D /OFF`
//! (`BaseState` in `/D` shall be `ON`, so hiding is via `/OFF`, never
//! `BaseState /OFF`). The annotation carries `/OC` → its OCG directly (NOTE 3:
//! a single-group OCMD is wasteful — reference the OCG directly).

use crate::object::{Dict, Name, ObjId, Object};

use super::units::{DecimalMarker, FractionMode, NumberFormat, Unit};

/// Build a `/Measure` dict (`/Type /Measure /Subtype /RL`) for a group whose
/// scale is `scale` real-display-units-per-point in `format.unit`
/// (§12.9 Tables 261/262/263). The first `/X` element's `/C` is `scale`; `/D`
/// (distance) stays in `/X`'s unit (`C = 1`); `/A` (area) is in `unit²` with
/// `C = 1`, because Table 262 applies `/A`'s factor to "the units of the first
/// element of X, squared" — so a reader's area is `pt² × scale² × 1`, the same
/// number an area ce dimension bakes into its label. `/R` is the display-only
/// ratio label.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{build_measure_dict, NumberFormat, Unit};
/// use pdfcer_core::object::Object;
///
/// let m = build_measure_dict(0.05, NumberFormat::decimal(Unit::Meter, 3));
/// let Object::Dict(d) = m else { panic!() };
/// assert_eq!(d.get(b"Subtype").unwrap().as_name().unwrap().as_bytes(), b"RL");
/// assert!(d.get(b"X").is_some() && d.get(b"D").is_some() && d.get(b"A").is_some());
/// ```
#[must_use]
pub fn build_measure_dict(scale: f64, format: NumberFormat) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Measure")));
    d.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"RL")));
    d.insert(
        Name::from(b"R"),
        Object::String(ratio_label(scale, format.unit)),
    );
    // /X carries the scale as its first /C.
    d.insert(Name::from(b"X"), number_format_array(format, scale));
    // /D (distance) stays in /X's unit → first /C = 1.
    d.insert(Name::from(b"D"), number_format_array(format, 1.0));
    // /A (area) is Required by Table 262; it is in /X's unit squared, /C = 1.
    d.insert(Name::from(b"A"), area_array(format));
    Object::Dict(d)
}

/// A DISPLAY-ONLY `/R` ratio label consistent with `/C` (§12.9 Table 262 `/R`
/// is free text a reader shows but never parses). pdfcer writes a readable
/// `1 pt = <s> <unit>` form.
fn ratio_label(scale: f64, unit: Unit) -> Vec<u8> {
    if scale.is_finite() && scale > 0.0 {
        format!("1 pt = {scale} {}", unit.abbrev()).into_bytes()
    } else {
        b"unscaled".to_vec()
    }
}

/// A NumberFormat array (§12.9 Table 263) for `format`, whose FIRST element's
/// `/C` is `first_c` (the scale for `/X`, `1.0` for `/D`).
fn number_format_array(format: NumberFormat, first_c: f64) -> Object {
    match (format.unit, format.fraction) {
        // Feet-inches: two elements, ft then in (§12.9 RAG verbatim).
        (
            Unit::FeetInches,
            FractionMode::Fraction {
                denominator,
                reduce,
            },
        ) => Object::Array(vec![
            nf_dict(b"ft", first_c, None, format.decimal_marker),
            nf_dict(
                b"in",
                12.0,
                Some(FracKeys {
                    fraction: true,
                    d: i64::from(denominator.max(1)),
                    fd: !reduce,
                }),
                format.decimal_marker,
            ),
        ]),
        (Unit::FeetInches, FractionMode::Decimal { places }) => Object::Array(vec![nf_dict(
            b"ft",
            first_c,
            Some(FracKeys {
                fraction: false,
                d: pow10(places),
                fd: false,
            }),
            format.decimal_marker,
        )]),
        // Single-element decimal units.
        (unit, FractionMode::Decimal { places }) => Object::Array(vec![nf_dict(
            unit.abbrev().as_bytes(),
            first_c,
            Some(FracKeys {
                fraction: false,
                d: pow10(places),
                fd: false,
            }),
            format.decimal_marker,
        )]),
        // Single-element fractional unit (inch fraction).
        (
            unit,
            FractionMode::Fraction {
                denominator,
                reduce,
            },
        ) => Object::Array(vec![nf_dict(
            unit.abbrev().as_bytes(),
            first_c,
            Some(FracKeys {
                fraction: true,
                d: i64::from(denominator.max(1)),
                fd: !reduce,
            }),
            format.decimal_marker,
        )]),
    }
}

/// The last-element display keys `/F`, `/D`, `/FD` (§12.9 Table 263). Only
/// meaningful on the LAST array element, so non-last elements pass `None`.
struct FracKeys {
    /// `true` ⇒ `/F /F` (fraction), `false` ⇒ `/F /D` (decimal).
    fraction: bool,
    /// `/D` — the denominator (fraction) or `10^places` (decimal precision).
    d: i64,
    /// `/FD` — keep the denominator / don't truncate low-order zeros.
    fd: bool,
}

/// One NumberFormat dict (§12.9 Table 263): `/Type /NumberFormat /U /C`, plus
/// `/F /D /FD` on the last element.
fn nf_dict(u: &[u8], c: f64, frac: Option<FracKeys>, marker: DecimalMarker) -> Object {
    let mut d = Dict::new();
    d.insert(
        Name::from(b"Type"),
        Object::Name(Name::from(b"NumberFormat")),
    );
    d.insert(Name::from(b"U"), Object::String(u.to_vec()));
    d.insert(Name::from(b"C"), Object::Real(c));
    if let Some(f) = frac {
        d.insert(
            Name::from(b"F"),
            Object::Name(Name::from(if f.fraction { b"F" } else { b"D" })),
        );
        d.insert(Name::from(b"D"), Object::Integer(f.d));
        if f.fd {
            d.insert(Name::from(b"FD"), Object::Boolean(true));
        }
    }
    // §12.9 Table 263 `/RD` — the decimal separator (Pass 27.2). Written only
    // for a comma, since the spec default is already a point.
    //
    // `/RT` MUST be written alongside it. Table 263 gives `/RT` (the thousands
    // separator) a spec default of COMMA, so setting `/RD` to a comma without
    // pinning `/RT` yields `1,234,56` in a conforming reader — a number that
    // is wrong in a way pdfcer's own label would not show, because the label is
    // baked into the `/AP` and the dict is what everyone ELSE computes from.
    if matches!(marker, DecimalMarker::Comma) {
        d.insert(Name::from(b"RD"), Object::String(b",".to_vec()));
        d.insert(Name::from(b"RT"), Object::String(b" ".to_vec()));
    }
    Object::Dict(d)
}

/// The single-element `/A` (area) NumberFormat array (§12.9 Tables 262/263):
/// `/U` is the unit squared as a text string (`m²` — `²` is PDFDocEncoding
/// 0xB2, as in the spec's own `(mm\262)` example), `/C 1`, and decimal
/// precision matching the area label ([`super::area_places`]).
fn area_array(format: NumberFormat) -> Object {
    let label = crate::textstring::encode_text_string(&super::area_unit_label(format));
    Object::Array(vec![nf_dict(
        &label,
        1.0,
        Some(FracKeys {
            fraction: false,
            d: pow10(super::area_places(format)),
            fd: false,
        }),
        format.decimal_marker,
    )])
}

/// `10^places` as an `i64`, clamped so a huge precision cannot overflow.
fn pow10(places: u32) -> i64 {
    10i64.checked_pow(places.min(9)).unwrap_or(1_000_000_000)
}

/// Build an `/OCG` dict for a group layer (§8.11 Table 98: `/Type /OCG
/// /Name`). Intent defaults to `View` (omitted) so the layer participates in
/// normal viewing.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::build_ocg;
/// use pdfcer_core::object::Object;
///
/// let Object::Dict(d) = build_ocg("Floor Plan") else { panic!() };
/// assert_eq!(d.get(b"Type").unwrap().as_name().unwrap().as_bytes(), b"OCG");
/// ```
#[must_use]
pub fn build_ocg(name: &str) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"OCG")));
    d.insert(
        Name::from(b"Name"),
        Object::String(name.as_bytes().to_vec()),
    );
    Object::Dict(d)
}

/// Build the catalog `/OCProperties` dict (§8.11 Table 100) from the full set
/// of pdfcer-authored group OCGs, each with its default visibility.
///
/// **MANDATORY registration** (§8.11.4.2): every OCG appears in `/OCGs`, and
/// `/D` is present with an `/Order` listing them (so they show in a reader's
/// layers panel). A group whose `visible` is `false` is added to `/D /OFF`
/// (hidden by default; `BaseState` in `/D` shall be `ON`, so hiding is via
/// `/OFF`). `foreign_ocgs` are any pre-existing catalog OCG refs to preserve
/// (unioned into `/OCGs` and `/Order` so a foreign layer is not dropped).
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::build_ocproperties;
/// use pdfcer_core::object::{ObjId, Object};
///
/// let a = ObjId::new(10, 0);
/// let b = ObjId::new(11, 0);
/// let ocp = build_ocproperties(&[(a, true), (b, false)], &[]);
/// let Object::Dict(d) = ocp else { panic!() };
/// // Both OCGs registered; the hidden one is in /D /OFF.
/// assert_eq!(d.get(b"OCGs").unwrap().as_array().unwrap().len(), 2);
/// let dd = d.get(b"D").unwrap().as_dict().unwrap();
/// assert_eq!(dd.get(b"OFF").unwrap().as_array().unwrap().len(), 1);
/// ```
#[must_use]
pub fn build_ocproperties(group_ocgs: &[(ObjId, bool)], foreign_ocgs: &[ObjId]) -> Object {
    let mut all: Vec<Object> = Vec::new();
    let mut order: Vec<Object> = Vec::new();
    let mut off: Vec<Object> = Vec::new();
    // Foreign OCGs first (preserved), then pdfcer's own.
    for id in foreign_ocgs {
        all.push(Object::Reference(*id));
        order.push(Object::Reference(*id));
    }
    for &(id, visible) in group_ocgs {
        all.push(Object::Reference(id));
        order.push(Object::Reference(id));
        if !visible {
            off.push(Object::Reference(id));
        }
    }

    let mut config = Dict::new();
    config.insert(
        Name::from(b"Name"),
        Object::String(b"pdfcer dimensions".to_vec()),
    );
    config.insert(Name::from(b"Order"), Object::Array(order));
    if !off.is_empty() {
        config.insert(Name::from(b"OFF"), Object::Array(off));
    }

    let mut d = Dict::new();
    d.insert(Name::from(b"OCGs"), Object::Array(all));
    d.insert(Name::from(b"D"), Object::Dict(config));
    Object::Dict(d)
}

/// Merge pdfcer's group OCGs into an EXISTING `/OCProperties` (§8.11.4.2)
/// instead of rebuilding it, so a file's own layer setup survives a ce
/// dimension edit.
///
/// `existing` must already be resolved: its `/OCGs` array, its `/D` dict and
/// that dict's `/ON`, `/OFF` and `/Order` arrays inline. Every other entry
/// (`/Configs`, and `/D`'s `/Name`, `/BaseState`, `/Locked`, `/RBGroups`,
/// `/AS`, `/Intent`, `/ListMode`) is carried unchanged.
///
/// `/OCGs` becomes `foreign_ocgs` then `group_ocgs`. In `/D`, references to
/// an OCG no longer registered, and every reference to a group OCG, are
/// removed from `/ON`, `/OFF` and `/Order` (recursively for nested `/Order`
/// arrays); each group OCG is then appended to `/Order` and to `/OFF` when
/// hidden, or to `/ON` when visible under `/BaseState /OFF`. A missing `/D`
/// or `/Order` is created as [`build_ocproperties`] would.
#[must_use]
pub(crate) fn merge_ocproperties(
    existing: &Dict,
    group_ocgs: &[(ObjId, bool)],
    foreign_ocgs: &[ObjId],
) -> Object {
    let fresh = build_ocproperties(group_ocgs, foreign_ocgs);
    let Object::Dict(fresh) = fresh else {
        return fresh;
    };
    let registered: std::collections::BTreeSet<ObjId> = foreign_ocgs
        .iter()
        .copied()
        .chain(group_ocgs.iter().map(|(id, _)| *id))
        .collect();
    let mine: std::collections::BTreeSet<ObjId> = group_ocgs.iter().map(|(id, _)| *id).collect();
    let keep = |o: &Object| match o {
        Object::Reference(r) => registered.contains(r) && !mine.contains(r),
        _ => true,
    };

    let mut out = existing.clone();
    if let Some(ocgs) = fresh.get(b"OCGs") {
        out.insert(Name::from(b"OCGs"), ocgs.clone());
    }
    let Some(Object::Dict(existing_d)) = existing.get(b"D") else {
        if let Some(d) = fresh.get(b"D") {
            out.insert(Name::from(b"D"), d.clone());
        }
        return Object::Dict(out);
    };
    let mut d = existing_d.clone();
    let base_off = matches!(d.get(b"BaseState"), Some(Object::Name(n)) if n.as_bytes() == b"OFF");
    for key in [&b"ON"[..], &b"OFF"[..]] {
        if let Some(Object::Array(a)) = d.get(key) {
            let kept: Vec<Object> = a.iter().filter(|o| keep(o)).cloned().collect();
            d.insert(Name(key.to_vec()), Object::Array(kept));
        }
    }
    let mut order = match d.get(b"Order") {
        Some(Object::Array(a)) => filter_order(a, &keep, 0),
        // No /Order: create the flat one build_ocproperties would, so the
        // foreign layers stay listed beside pdfcer's.
        _ => foreign_ocgs
            .iter()
            .map(|id| Object::Reference(*id))
            .collect(),
    };
    let mut on: Vec<Object> = match d.get(b"ON") {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let mut off: Vec<Object> = match d.get(b"OFF") {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    for &(id, visible) in group_ocgs {
        order.push(Object::Reference(id));
        if !visible {
            off.push(Object::Reference(id));
        } else if base_off {
            on.push(Object::Reference(id));
        }
    }
    d.insert(Name::from(b"Order"), Object::Array(order));
    if !on.is_empty() || d.get(b"ON").is_some() {
        d.insert(Name::from(b"ON"), Object::Array(on));
    }
    if !off.is_empty() || d.get(b"OFF").is_some() {
        d.insert(Name::from(b"OFF"), Object::Array(off));
    }
    out.insert(Name::from(b"D"), Object::Dict(d));
    Object::Dict(out)
}

/// `/Order` with dropped references removed, nested arrays filtered in turn
/// (depth-guarded; `ARCHITECTURE.md` §10) and kept even when emptied, since a
/// nested array may carry a label string only.
fn filter_order(a: &[Object], keep: &dyn Fn(&Object) -> bool, depth: usize) -> Vec<Object> {
    const MAX_ORDER_DEPTH: usize = 64;
    a.iter()
        .filter(|o| keep(o))
        .map(|o| match o {
            Object::Array(inner) if depth < MAX_ORDER_DEPTH => {
                Object::Array(filter_order(inner, keep, depth + 1))
            }
            other => other.clone(),
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn arr<'a>(o: &'a Object, key: &[u8]) -> &'a [Object] {
        o.as_dict().unwrap().get(key).unwrap().as_array().unwrap()
    }

    #[test]
    fn decimal_measure_has_scale_as_first_x_c() {
        let m = build_measure_dict(0.05, NumberFormat::decimal(Unit::Meter, 3));
        let x = arr(&m, b"X");
        let first = x[0].as_dict().unwrap();
        assert_eq!(first.get(b"U").unwrap(), &Object::String(b"m".to_vec()));
        assert_eq!(first.get(b"C").unwrap(), &Object::Real(0.05)); // == the scale
        assert_eq!(first.get(b"F").unwrap().as_name().unwrap().as_bytes(), b"D");
        assert_eq!(first.get(b"D").unwrap(), &Object::Integer(1000)); // 10^3
        // /D distance stays in metres → first /C = 1.
        let dist = arr(&m, b"D");
        assert_eq!(
            dist[0].as_dict().unwrap().get(b"C").unwrap(),
            &Object::Real(1.0)
        );
    }

    #[test]
    fn feet_inches_measure_is_two_elements_with_eighths() {
        // /X [ <</U(ft)/C s>> <</U(in)/C 12 /F /F /D 8 /FD true>> ]
        let m = build_measure_dict(0.001_157, NumberFormat::feet_inches(8, false));
        let x = arr(&m, b"X");
        assert_eq!(x.len(), 2);
        let ft = x[0].as_dict().unwrap();
        assert_eq!(ft.get(b"U").unwrap(), &Object::String(b"ft".to_vec()));
        assert!(ft.get(b"F").is_none()); // non-last: no display keys
        let inch = x[1].as_dict().unwrap();
        assert_eq!(inch.get(b"U").unwrap(), &Object::String(b"in".to_vec()));
        assert_eq!(inch.get(b"C").unwrap(), &Object::Real(12.0)); // ft → in
        assert_eq!(inch.get(b"F").unwrap().as_name().unwrap().as_bytes(), b"F");
        assert_eq!(inch.get(b"D").unwrap(), &Object::Integer(8));
        assert_eq!(inch.get(b"FD").unwrap(), &Object::Boolean(true));
    }

    #[test]
    fn inch_fraction_measure_is_single_fraction_element() {
        let m = build_measure_dict(1.0 / 72.0, NumberFormat::inch_fraction(16));
        let x = arr(&m, b"X");
        assert_eq!(x.len(), 1);
        let e = x[0].as_dict().unwrap();
        assert_eq!(e.get(b"F").unwrap().as_name().unwrap().as_bytes(), b"F");
        assert_eq!(e.get(b"D").unwrap(), &Object::Integer(16));
    }

    #[test]
    fn ocg_and_ocproperties_register_correctly() {
        let g0 = ObjId::new(10, 0);
        let g1 = ObjId::new(11, 0);
        let ocp = build_ocproperties(&[(g0, true), (g1, false)], &[]);
        let d = ocp.as_dict().unwrap();
        // Every OCG in /OCGs (MANDATORY registration).
        let ocgs = d.get(b"OCGs").unwrap().as_array().unwrap();
        assert_eq!(ocgs.len(), 2);
        // /D present with /Order listing both.
        let config = d.get(b"D").unwrap().as_dict().unwrap();
        assert_eq!(config.get(b"Order").unwrap().as_array().unwrap().len(), 2);
        // The hidden group in /OFF; the visible one not.
        let off = config.get(b"OFF").unwrap().as_array().unwrap();
        assert_eq!(off, &[Object::Reference(g1)]);
    }

    #[test]
    fn ocproperties_preserves_foreign_ocgs() {
        let foreign = ObjId::new(5, 0);
        let mine = ObjId::new(10, 0);
        let ocp = build_ocproperties(&[(mine, true)], &[foreign]);
        let ocgs = ocp
            .as_dict()
            .unwrap()
            .get(b"OCGs")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(ocgs.contains(&Object::Reference(foreign)));
        assert!(ocgs.contains(&Object::Reference(mine)));
    }
}
