//! Synthetic markup, view, light and clipping-plane entities, written in
//! the spec RAG's field order; `pub(in crate::tree)` writers also feed the
//! tree tests' end-to-end file.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use crate::bits::BitReader;
use crate::tess::Ctx;
use crate::testw::W;
use crate::{PrcError, Schema};

const SENTINEL: u32 = 12345;

/// `ContentPRCRefBase`: no attributes, same name, CAD ids, unique id 7.
fn ref_base(w: &mut W) {
    w.uint(0).bit(true).uint(0).uint(0).uint(7);
}

/// `PRCBaseWithGraphics` reusing the current graphics.
fn pbwg(w: &mut W) {
    ref_base(w);
    w.bit(true);
}

/// `ReferenceOnPRCBase` to B-rep item 7 in the same structure.
fn r205(w: &mut W) {
    w.uint(205).uint(232).bit(true).uint(7);
}

fn linked_item(w: &mut W) {
    w.uint(204);
    pbwg(w);
    w.uint(0).bit(true);
    r205(w);
    w.bit(true);
    r205(w);
    w.bit(true).bit(false).bit(true).bit(false).uint(0);
}

fn leader(w: &mut W) {
    w.uint(503);
    pbwg(w);
    w.bit(true);
    r205(w);
    w.bit(false).uint(2).uint(0);
}

fn markup(w: &mut W) {
    w.uint(502);
    pbwg(w);
    w.uint(1).uint(2).uint(1);
    r205(w);
    w.uint(1);
    r205(w);
    w.uint(3).uint(0);
}

fn annotation_item(w: &mut W) {
    w.uint(504);
    pbwg(w);
    r205(w);
    w.uint(0);
}

fn annotation_reference(w: &mut W) {
    w.uint(506);
    pbwg(w);
    w.uint(2);
    r205(w);
    r205(w);
    w.uint(0);
}

/// An annotation set holding `inner`, `depth` sets deep.
fn annotation_set(w: &mut W, depth: usize, inner: fn(&mut W)) {
    w.uint(505);
    pbwg(w);
    w.uint(1);
    if depth > 1 {
        annotation_set(w, depth - 1, inner);
    } else {
        inner(w);
    }
    w.uint(0);
}

/// One of each kind; two markups.
pub(in crate::tree) fn markup_data(w: &mut W) {
    w.uint(1);
    linked_item(w);
    w.uint(1);
    leader(w);
    w.uint(2);
    markup(w);
    markup(w);
    w.uint(2);
    annotation_set(w, 2, annotation_reference);
    annotation_item(w);
}

/// `SURF_Plane` with base geometry, translated and rotated.
fn plane(w: &mut W) {
    w.uint(86).bit(true).uint(0).bit(true).uint(9);
    w.uint(0).put(0x03, 8);
    w.doubles([1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    w.doubles([-1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
}

fn light(w: &mut W, t: u32, version: u32) {
    w.uint(t);
    ref_base(w);
    w.uint(1).uint(2).uint(3);
    match t {
        732 => {
            w.doubles([0.0; 6]);
        }
        733 => {
            w.doubles([0.0, 0.0, 1.0]);
            if version >= 8030 {
                w.double(0.5);
            }
        }
        734 => {
            w.doubles([0.0; 11]);
        }
        _ => {}
    }
}

/// Scene parameters with every light kind and one clipping plane.
fn scene(w: &mut W, version: u32) {
    w.uint(741);
    ref_base(w);
    w.bit(true).uint(4);
    for t in [731, 732, 733, 734] {
        light(w, t, version);
    }
    w.bit(false).bit(false).uint(1);
    plane(w);
    w.uint(0).uint(0).uint(0);
    if version >= 8137 {
        w.bit(false);
    }
}

fn view(w: &mut W, version: u32) {
    w.uint(501);
    pbwg(w);
    w.uint(1);
    r205(w);
    plane(w);
    w.bit(true);
    scene(w, version);
    if version >= 7309 {
        w.bit(true);
    }
    if version >= 8016 {
        w.bit(false).bit(true).uint(1);
        r205(w);
        w.uint(0);
    }
    w.uint(0);
}

/// One view.
pub(in crate::tree) fn views(w: &mut W, version: u32) {
    w.uint(1);
    view(w, version);
}

/// Reads `write`'s output at `version` with `read`, then the sentinel.
fn reads<T>(
    version: u32,
    write: impl Fn(&mut W),
    read: impl Fn(&mut Ctx<'_, '_>) -> Result<T, PrcError>,
) -> T {
    let mut w = W::default();
    write(&mut w);
    w.uint(SENTINEL);
    let bytes = w.bytes();
    let schema = Schema::default();
    let mut c = Ctx::new(BitReader::new(&bytes), &schema, version);
    let out = read(&mut c).unwrap();
    assert_eq!(c.r.unsigned_integer().unwrap(), SENTINEL);
    out
}

#[test]
fn every_markup_entity_reads_past() {
    reads(8137, linked_item, |c| c.markup_linked_item());
    reads(8137, leader, |c| c.leader());
    reads(8137, markup, |c| c.mkp_markup());
    reads(8137, annotation_item, |c| c.annotation_entity(0));
    reads(8137, annotation_reference, |c| c.annotation_entity(0));
    reads(
        8137,
        |w| annotation_set(w, 3, annotation_item),
        |c| c.annotation_entity(0),
    );
    assert_eq!(reads(8137, markup_data, |c| c.markup_data()), 2);
}

#[test]
fn views_read_past_at_each_authoring_version() {
    for v in [7094, 7309, 8016, 8030, 8137] {
        assert_eq!(reads(v, |w| views(w, v), |c| c.views()), 1, "version {v}");
    }
}

#[test]
fn lights_and_clipping_planes_read_past() {
    for v in [8029, 8030, 8137] {
        reads(v, |w| scene(w, v), |c| c.scene_display_parameters());
    }
    reads(8137, plane, |c| c.surface_plane());
}

#[test]
fn nesting_past_the_ceiling_is_refused() {
    let mut w = W::default();
    annotation_set(&mut w, super::MAX_ANNOTATION_DEPTH + 2, annotation_item);
    let bytes = w.bytes();
    let schema = Schema::default();
    let mut c = Ctx::new(BitReader::new(&bytes), &schema, 8137);
    assert!(matches!(
        c.annotation_entity(0),
        Err(PrcError::Malformed(_))
    ));
}

#[test]
fn a_hostile_count_is_truncation_not_an_allocation() {
    let bytes = W::default().uint(0).uint(0).uint(u32::MAX).bytes();
    let schema = Schema::default();
    let mut c = Ctx::new(BitReader::new(&bytes), &schema, 8137);
    assert!(matches!(c.markup_data(), Err(PrcError::Truncated(_))));
}

#[test]
fn every_truncation_is_an_error() {
    let mut w = W::default();
    markup_data(&mut w);
    views(&mut w, 8137);
    let bytes = w.bytes();
    let schema = Schema::default();
    for len in 0..bytes.len() {
        let mut c = Ctx::new(BitReader::new(&bytes[..len]), &schema, 8137);
        let r = c.markup_data().and_then(|_| c.views());
        assert!(r.is_err(), "{len} bytes read as whole");
    }
}
