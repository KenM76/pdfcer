//! Fixture builders shared by the cases.

use super::*;

pub(super) fn occurrence(w: &mut W, o: &Occ<'_>, extra: bool) {
    w.uint(PRODUCT_OCCURRENCE);
    graphics_named(w, o.name, o.behaviour, extra);
    w.uint(o.part);
    match o.prototype {
        None => w.uint(0),
        Some((index, None)) => w.uint(index).bit(true),
        Some((index, Some(fs))) => {
            w.uint(index).bit(false);
            fs.0.iter().fold(&mut *w, |w, &c| w.uint(c))
        }
    };
    w.uint(0).uint(o.sons.len() as u32);
    for &s in o.sons {
        w.uint(s);
    }
    w.put(u64::from(o.suppressed), 8);
    w.bit(false).double(1.0).put(0, 8).int(0);
    w.bit(o.location.is_some());
    if let Some(t) = o.location {
        translation(w, t, o.mirror);
    }
    w.uint(o.refs.len() as u32);
    for er in o.refs {
        entity_reference(w, er);
    }
    if o.markups {
        markup::tests::markup_data(w);
        markup::tests::views(w, 8137);
    } else {
        no_markups_or_views(w);
    }
    w.bit(false).uint(0);
    if o.camera {
        w.uint(1).uint(SCENE_DISPLAY_PARAMETERS);
        base(w);
        w.uint(0).uint(0).uint(9).bit(false).uint(0).bit(true);
        w.uint(CAMERA);
        base(w);
        w.uint(0).uint(0).uint(10).bit(true);
        for i in 0..15 {
            w.double(f64::from(i) + 0.5);
        }
        w.bit(false).uint(0).uint(19).uint(0).uint(0).bit(false);
    } else {
        w.uint(0);
    }
    w.uint(0);
}

/// Schema, then globals with a picture, a scaled texture, red and blue,
/// two alpha-0 materials and a texture over the blue one, and styles 1
/// (red, transparency 255) and 2 (the texture, 128); no styles when
/// `plain`.
pub(super) fn coloured_globals(plain: bool) -> W {
    let mut w = W::default();
    w.uint(0).uint(FILE_STRUCTURE_GLOBALS);
    base(&mut w);
    w.uint(0)
        .double(2000.0)
        .double(40.0)
        .string(Some(""))
        .uint(0);
    if plain {
        for _ in 0..9 {
            w.uint(0);
        }
        return w;
    }
    w.uint(2);
    for c in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        w.double(c);
    }
    w.uint(1).uint(PICTURE);
    base(&mut w);
    w.uint(0).uint(0).uint(1).uint(1);
    w.uint(1).uint(TEXTURE_DEFINITION);
    ref_base(&mut w, 1);
    w.uint(1).put(2, 8).int(3).int(0).bit(false).uint(0);
    w.uint(1).double(1.0).uint(1).put(0, 8);
    w.int(3).double(1.0).double(1.0).double(1.0).double(1.0);
    w.int(1).int(1).int(0);
    w.put(u64::from(TEXTURE_ALPHA_TEST), 8).int(0).double(0.5);
    w.int(0).int(0).bit(true);
    w.uint(TEXTURE_TRANSFORMATION)
        .bit(false)
        .bit(false)
        .bit(true);
    w.put(0x08, 8).double(2.0);
    w.uint(3);
    for (id, diffuse) in [(2, 1), (3, 4)] {
        w.uint(MATERIAL);
        ref_base(&mut w, id);
        w.uint(0).uint(diffuse).uint(0).uint(0);
        for alpha in [0.5, 1.0, 0.0, 1.0, 1.0] {
            w.double(alpha);
        }
    }
    w.uint(TEXTURE_APPLICATION);
    ref_base(&mut w, 4);
    w.uint(2).uint(1).uint(0).uint(0);
    w.uint(0).uint(2);
    for (id, material, transparency) in [(5, 1, 255), (6, 3, 128)] {
        w.uint(STYLE);
        ref_base(&mut w, id);
        w.double(1.0).bit(false).uint(0).bit(true).uint(material);
        w.bit(true)
            .put(transparency, 8)
            .bit(false)
            .bit(false)
            .bit(false);
    }
    w.uint(0).uint(0).uint(0);
    w
}

/// The unit square, defined in file structure A, placed three times by
/// prototype from file structure B, whose globals hold the colours:
/// styles 1, 2 and none, at x 0, 2 and 4. The SolidWorks layout that
/// colours a part with another structure's palette. The CLI's colour
/// fixture.
pub(super) fn coloured_prc() -> Vec<u8> {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let a = UniqueId([5, 6, 7, 8]);
    let shown = |style, x| Occ {
        behaviour: Some((style, SHOW)),
        prototype: Some((1, Some(a))),
        location: Some([x, 0.0, 0.0]),
        ..OCC
    };
    let b_occs = [
        Occ {
            sons: &[1, 2, 3],
            ..OCC
        },
        shown(1, 0.0),
        shown(2, 2.0),
        shown(0, 4.0),
    ];
    let mut model = W::default();
    model.uint(0).uint(MODEL_FILE);
    base(&mut model);
    model.bit(false).double(1.0).uint(2);
    for (last, root) in [(8, 0), (9, 1)] {
        for c in [5, 6, 7, last] {
            model.uint(c);
        }
        model.uint(root).bit(true);
    }
    crate::prc::testw::prc_container_n(
        &[
            [
                &coloured_globals(true).bytes(),
                &tree(&[Occ { part: 1, ..OCC }], 0, false).bytes(),
                tess,
            ],
            [
                &coloured_globals(false).bytes(),
                &tree(&b_occs, 0, false).bytes(),
                &[],
            ],
        ],
        &model.bytes(),
    )
}

/// The unit square as B-rep `(1, 1)` in structure A, placed at x 0, 2
/// and 4 by prototype from structure B with style 1 (red), 1 and none.
/// The first occurrence recolours the item with style 2 (translucent
/// blue), the second only face 0, and the third hides it: the layout a
/// CAD export uses for its only transparency. The CLI's override
/// fixture.
pub(super) fn overridden_prc() -> Vec<u8> {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let a = UniqueId([5, 6, 7, 8]);
    let shown = |style, x, refs| Occ {
        behaviour: Some((style, SHOW)),
        prototype: Some((1, Some(a))),
        location: Some([x, 0.0, 0.0]),
        refs,
        ..OCC
    };
    let blue = [Er::Item {
        g: (2, SHOW),
        fs: Some(a),
        uid: 7,
        cs: 0,
    }];
    let face = [Er::Faces {
        g: (2, SHOW),
        faces: &[0],
    }];
    let hide = [Er::Item {
        g: (0, 0),
        fs: Some(a),
        uid: 7,
        cs: 0,
    }];
    let b_occs = [
        Occ {
            sons: &[1, 2, 3],
            ..OCC
        },
        shown(1, 0.0, &blue),
        shown(1, 2.0, &face),
        shown(0, 4.0, &hide),
    ];
    let mut model = W::default();
    model.uint(0).uint(MODEL_FILE);
    base(&mut model);
    model.bit(false).double(1.0).uint(2);
    for (last, root) in [(8, 0), (9, 1)] {
        for c in [5, 6, 7, last] {
            model.uint(c);
        }
        model.uint(root).bit(true);
    }
    let a_occs = [Occ { part: 1, ..OCC }];
    crate::prc::testw::prc_container_n(
        &[
            [
                &coloured_globals(true).bytes(),
                &tree_with(&a_occs, 0, false, None, Some((1, 1))).bytes(),
                tess,
            ],
            [
                &coloured_globals(false).bytes(),
                &tree(&b_occs, 0, false).bytes(),
                &[],
            ],
        ],
        &model.bytes(),
    )
}
