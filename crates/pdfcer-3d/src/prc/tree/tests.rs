//! Unit tests for `tree`: shared fixtures here, the cases in `tests/`.

use super::style::{Material, Style, Terms};
use super::*;
use crate::Schema;
use crate::prc::bits::BitReader;
use crate::prc::testw::W;

const FS: UniqueId = UniqueId([1, 2, 3, 4]);

/// `ContentPRCBase`: no attributes, same name.
fn base(w: &mut W) {
    named(w, None);
}

/// `ContentPRCBase` with `name`: `None` for same name, `Some(None)`
/// for a null one.
fn named(w: &mut W, name: Option<Option<&str>>) {
    w.uint(0);
    match name {
        None => w.bit(true),
        Some(n) => w.bit(false).string(n),
    };
}

/// `PRCBaseWithGraphics`; `g` is `(line style + 1, behaviour)`, `None`
/// reusing the current graphics. `extra` writes the UInt a schema adds
/// to type 2.
fn graphics(w: &mut W, g: Option<(u32, u16)>, extra: bool) {
    graphics_named(w, None, g, extra);
}

fn graphics_named(w: &mut W, name: Option<Option<&str>>, g: Option<(u32, u16)>, extra: bool) {
    named(w, name);
    w.uint(0).uint(0).uint(7);
    match g {
        None => {
            w.bit(true);
        }
        Some((style, b)) => {
            w.bit(false)
                .uint(0)
                .uint(style)
                .put(u64::from(b & 0xff), 8)
                .put(u64::from(b >> 8), 8);
        }
    }
    if extra {
        w.uint(42);
    }
}

/// A translation by `t`, mirrored in x first when `mirror`.
fn translation(w: &mut W, t: [f64; 3], mirror: bool) {
    w.uint(CARTESIAN_TRANSFORMATION)
        .put(if mirror { 0x11 } else { 0x01 }, 8);
    for c in t {
        w.double(c);
    }
    if mirror {
        w.double(-1.0).double(1.0).double(1.0);
    }
}

/// Four empty markup lists, then no views.
fn no_markups_or_views(w: &mut W) {
    w.uint(0).uint(0).uint(0).uint(0).uint(0);
}

/// A part with one item on tessellation `tess` (biased), local CS
/// `cs`: a poly-BRep, or with `brep` a B-rep model of that `(context,
/// body)`. Items have PRC unique id 7.
fn part(
    w: &mut W,
    (cs, tess): (u32, u32),
    extra: bool,
    name: Option<Option<&str>>,
    brep: Option<(u32, u32)>,
) {
    w.uint(PART_DEFINITION);
    graphics_named(w, name, None, extra);
    for c in [1.0, 0.0, 0.0, -1.0, 0.0, 0.0] {
        w.double(c);
    }
    match brep {
        None => {
            w.uint(1).uint(RI_POLY_BREP_MODEL);
            graphics(w, None, extra);
            w.uint(cs).uint(tess).bit(false).uint(0);
        }
        Some((context, body)) => {
            w.uint(1).uint(RI_BREP_MODEL);
            graphics(w, None, extra);
            w.uint(cs).uint(tess).bit(true).uint(context).uint(body);
            w.bit(false).uint(0);
        }
    }
    no_markups_or_views(w);
    w.uint(0);
}

#[derive(Clone, Copy)]
struct Occ<'a> {
    part: u32,
    sons: &'a [u32],
    behaviour: Option<(u32, u16)>,
    suppressed: bool,
    location: Option<[f64; 3]>,
    mirror: bool,
    camera: bool,
    /// One of each markup entity and a view, not none.
    markups: bool,
    /// `(prototype + 1, its file structure)`, `None` meaning this one.
    prototype: Option<(u32, Option<UniqueId>)>,
    /// As [`named`].
    name: Option<Option<&'a str>>,
    refs: &'a [Er<'a>],
}

/// A `MISC_EntityReference` with graphics `(style + 1, behaviour)`.
#[derive(Clone, Copy)]
enum Er<'a> {
    /// `ReferenceOnPRCBase` to item `uid`; `fs` `None` for this one;
    /// `cs` the biased local coordinate system, 0 none.
    Item {
        g: (u32, u16),
        fs: Option<UniqueId>,
        uid: u32,
        cs: u32,
    },
    /// `ReferenceOnTopology` to faces of B-rep `(1, 1)`, same structure.
    Faces { g: (u32, u16), faces: &'a [u32] },
}

fn entity_reference(w: &mut W, er: &Er<'_>) {
    w.uint(ENTITY_REFERENCE);
    let g = match er {
        Er::Item { g, .. } | Er::Faces { g, .. } => *g,
    };
    graphics(w, Some(g), false);
    let cs = match er {
        Er::Item { cs, .. } => *cs,
        Er::Faces { .. } => 0,
    };
    w.uint(cs).bit(true);
    match er {
        Er::Item { fs, uid, .. } => {
            w.uint(REFERENCE_ON_PRC_BASE).uint(RI_BREP_MODEL);
            match fs {
                None => w.bit(true),
                Some(fs) => fs.0.iter().fold(w.bit(false), |w, &c| w.uint(c)),
            };
            w.uint(*uid);
        }
        Er::Faces { faces, .. } => {
            w.uint(REFERENCE_ON_TOPOLOGY).uint(149).bit(true).bit(true);
            w.uint(1).uint(1).uint(faces.len() as u32);
            for &f in *faces {
                w.uint(f);
            }
        }
    }
    w.uint(0);
}

const OCC: Occ<'static> = Occ {
    part: 0,
    sons: &[],
    behaviour: None,
    suppressed: false,
    location: None,
    mirror: false,
    camera: false,
    markups: false,
    prototype: None,
    name: None,
    refs: &[],
};

fn tree(occs: &[Occ<'_>], cs: u32, extra: bool) -> W {
    tree_with(occs, cs, extra, None, None)
}

fn tree_with(
    occs: &[Occ<'_>],
    cs: u32,
    extra: bool,
    part_name: Option<Option<&str>>,
    brep: Option<(u32, u32)>,
) -> W {
    tree_on(occs, (cs, 1), extra, part_name, brep)
}

/// [`tree_with`], its item on tessellation `tess` (biased).
fn tree_on(
    occs: &[Occ<'_>],
    (cs, tess): (u32, u32),
    extra: bool,
    part_name: Option<Option<&str>>,
    brep: Option<(u32, u32)>,
) -> W {
    let mut w = W::default();
    w.uint(FILE_STRUCTURE_TREE);
    base(&mut w);
    w.uint(1);
    part(&mut w, (cs, tess), extra, part_name, brep);
    w.uint(occs.len() as u32);
    for o in occs {
        occurrence(&mut w, o, extra);
    }
    w.uint(FILE_STRUCTURE);
    base(&mut w);
    w.uint(100).uint(1).uint(0);
    w
}

/// Globals holding one colour, material, line pattern and style, then
/// one reference coordinate system translating by `t`.
fn globals(t: [f64; 3]) -> W {
    let mut w = W::default();
    w.uint(FILE_STRUCTURE_GLOBALS);
    base(&mut w);
    w.uint(0)
        .double(2000.0)
        .double(40.0)
        .string(Some(""))
        .uint(0);
    w.uint(1).double(1.0).double(0.5).double(0.0);
    w.uint(0).uint(0);
    w.uint(1).uint(MATERIAL);
    base(&mut w);
    w.uint(0).uint(0).uint(11);
    for _ in 0..4 {
        w.uint(1);
    }
    for _ in 0..5 {
        w.double(0.25);
    }
    w.uint(1).uint(LINE_PATTERN);
    base(&mut w);
    w.uint(0).uint(0).uint(12);
    w.uint(2).double(1.0e6).double(0.0).double(0.0).bit(false);
    w.uint(1).uint(STYLE);
    base(&mut w);
    w.uint(0).uint(0).uint(13);
    w.double(1.0).bit(false).uint(1).bit(false).uint(1);
    w.bit(true).put(128, 8).bit(false).bit(false).bit(false);
    w.uint(0);
    w.uint(1).uint(RI_COORDINATE_SYSTEM);
    graphics(&mut w, None, false);
    w.uint(0).uint(0);
    translation(&mut w, t, false);
    w.uint(0).uint(0);
    w
}

fn ctx<'a, 's>(bytes: &'a [u8], schema: &'s Schema) -> Ctx<'a, 's> {
    Ctx::new(BitReader::new(bytes), schema, 8137)
}

fn place(occs: &[Occ<'_>], cs: u32, globals: &Globals) -> Result<Vec<Placement>, PrcError> {
    let bytes = tree(occs, cs, false).bytes();
    let t = ctx(&bytes, &Schema::default()).file_structure_tree()?;
    let mut walk = Walk::new(
        vec![(FS, &t, globals)],
        StyleAlpha::default(),
        EntityOverrides::default(),
    );
    walk.occurrence(0, 0, &IDENTITY, &[], At::ROOT)?;
    Ok(walk.out)
}

fn fixture_globals() -> Globals {
    let bytes = globals([0.0, 5.0, 0.0]).bytes();
    ctx(&bytes, &Schema::default()).globals().unwrap()
}

fn translate(t: [f64; 3]) -> Matrix {
    let mut m = IDENTITY;
    for (row, c) in m.iter_mut().zip(t) {
        row[3] = c;
    }
    m
}

/// The square's tessellation, from the committed fixture.
fn square_tess() -> Vec<u8> {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    square.file_structures[0]
        .section(crate::SectionKind::Tessellation)
        .to_vec()
}

fn listed(occs: &[Occ<'_>], part_name: Option<Option<&str>>) -> (usize, Vec<ModelNode>) {
    let bytes = tree_with(occs, 0, false, part_name, None).bytes();
    let t = ctx(&bytes, &Schema::default())
        .file_structure_tree()
        .unwrap();
    let globals = Globals::default();
    let mut walk = Walk::new(
        vec![(FS, &t, &globals)],
        StyleAlpha::default(),
        EntityOverrides::default(),
    );
    walk.occurrence(0, 0, &IDENTITY, &[], At::ROOT).unwrap();
    (walk.out.len(), walk.nodes)
}

/// A two-occurrence assembly of the unit square: once in place, once
/// mirrored in x and moved 5 along it. The CLI's placement fixture.
fn assembly_prc() -> Vec<u8> {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let occs = [
        Occ {
            sons: &[1, 2],
            ..OCC
        },
        Occ { part: 1, ..OCC },
        Occ {
            part: 1,
            location: Some([5.0, 0.0, 0.0]),
            mirror: true,
            ..OCC
        },
    ];
    placed(tess, &occs)
}

/// One file structure holding `tess` and the occurrence tree `occs`.
fn placed(tess: &[u8], occs: &[Occ]) -> Vec<u8> {
    placed_tree(tess, &tree(occs, 0, false).bytes())
}

/// One file structure holding `tess` and the tree section `tree`.
fn placed_tree(tess: &[u8], tree: &[u8]) -> Vec<u8> {
    let schema = W::default().uint(0).bytes();
    crate::prc::testw::prc_container(&schema, tree, tess, &placed_model())
}

/// The model file naming `prc_container`'s one structure, root 1.
fn placed_model() -> Vec<u8> {
    let mut model = W::default();
    model
        .uint(0)
        .uint(MODEL_FILE)
        .uint(0)
        .bit(true)
        .bit(false)
        .double(1.0)
        .uint(1);
    // The id `prc_container` gives its file structure.
    for id in [5, 6, 7, 8] {
        model.uint(id);
    }
    model.uint(1).bit(true);
    model.bytes()
}

/// The square under a root with a part named directly, one named by
/// its prototype, one by its part, one stored hidden and one
/// suppressed: every state a model-tree panel shows.
fn named_tree_prc() -> Vec<u8> {
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let part = |name| Occ {
        part: 1,
        name: Some(name),
        ..OCC
    };
    let occs = [
        Occ {
            sons: &[1, 2, 3, 4, 5],
            name: Some(Some("Assembly")),
            ..OCC
        },
        part(Some("Bolt")),
        Occ {
            prototype: Some((7, None)),
            name: Some(None),
            ..OCC
        },
        part(None),
        Occ {
            behaviour: Some((0, 0)),
            ..part(Some("Pin"))
        },
        Occ {
            suppressed: true,
            ..part(Some("Nut"))
        },
        part(Some("Bracket")),
    ];
    placed_tree(
        tess,
        &tree_with(&occs, 0, false, Some(Some("Plate")), None).bytes(),
    )
}

const RED: [f64; 3] = [1.0, 0.0, 0.0];
const GREEN: [f64; 3] = [0.0, 1.0, 0.0];

fn g(style: u32, bits: u16) -> Graphics {
    Graphics { style, bits, fs: 0 }
}

fn ref_base(w: &mut W, id: u32) {
    base(w);
    w.uint(0).uint(0).uint(id);
}

/// Globals with grey, a 2x2 raw-RGB picture in header file 1, a texture
/// (stored-coordinate `mapping`, Replace, repeating) and a material
/// applying it over grey, used by style 1.
fn textured_globals(mapping: i32, next: u32) -> W {
    let mut w = W::default();
    w.uint(0).uint(FILE_STRUCTURE_GLOBALS);
    base(&mut w);
    w.uint(0)
        .double(2000.0)
        .double(40.0)
        .string(Some(""))
        .uint(0);
    w.uint(1).double(0.5).double(0.5).double(0.5);
    w.uint(1).uint(PICTURE);
    base(&mut w);
    w.uint(2).uint(1).uint(2).uint(2);
    w.uint(1).uint(TEXTURE_DEFINITION);
    ref_base(&mut w, 1);
    w.uint(1).put(2, 8).int(mapping);
    if mapping == TEXTURE_MAPPING_OPERATOR {
        w.int(0).bit(false);
    }
    w.uint(0).uint(1).double(1.0).uint(1).put(0, 8);
    w.int(2).int(0).int(0).put(0, 8);
    w.int(1).int(1).bit(false);
    w.uint(2);
    w.uint(MATERIAL);
    ref_base(&mut w, 2);
    w.uint(1).uint(1).uint(0).uint(0);
    for v in [0.5, 1.0, 1.0, 1.0, 1.0] {
        w.double(v);
    }
    w.uint(TEXTURE_APPLICATION);
    ref_base(&mut w, 3);
    w.uint(1).uint(1).uint(next).uint(1);
    w.uint(0).uint(1).uint(STYLE);
    ref_base(&mut w, 4);
    w.double(1.0).bit(false).uint(0).bit(true).uint(2);
    w.bit(false).bit(false).bit(false).bit(false);
    w.uint(0).uint(0).uint(0);
    w
}

/// The unit square in grey from a material whose diffuse alpha is 0.0,
/// through a style that states no transparency: the layout CAD exports
/// write for parts meant to be seen. The CLI's unset-alpha fixture.
pub(crate) fn alpha_unset_prc() -> Vec<u8> {
    grey_square_prc([1, 1, 0, 0], [0.5, 0.0, 0.0, 0.0, 0.0])
}

/// The grey square with a grey highlight of stored shininess 0.5 and no
/// glow. The CLI's finish fixture.
pub(crate) fn shiny_prc() -> Vec<u8> {
    grey_square_prc([1, 1, 0, 1], [0.5, 1.0, 1.0, 1.0, 1.0])
}

/// The unit square through a style over one material whose ambient,
/// diffuse, emissive and specular slots are `slots` (colour index + 1
/// into a single grey, 0 for none), followed by `doubles`: shininess
/// and the four alphas.
fn grey_square_prc(slots: [u32; 4], doubles: [f64; 5]) -> Vec<u8> {
    let mut g = W::default();
    g.uint(0).uint(FILE_STRUCTURE_GLOBALS);
    base(&mut g);
    g.uint(0)
        .double(2000.0)
        .double(40.0)
        .string(Some(""))
        .uint(0);
    g.uint(1).double(0.75).double(0.75).double(0.75);
    g.uint(0).uint(0).uint(1).uint(MATERIAL);
    ref_base(&mut g, 1);
    for i in slots {
        g.uint(i);
    }
    for v in doubles {
        g.double(v);
    }
    g.uint(0).uint(1).uint(STYLE);
    ref_base(&mut g, 2);
    g.double(1.0).bit(false).uint(0).bit(true).uint(1);
    g.bit(false).bit(false).bit(false).bit(false);
    g.uint(0).uint(0).uint(0);
    let square = crate::PrcFile::parse(include_bytes!(
        "../../../../../fixtures/synthetic/prc/square.prc"
    ))
    .unwrap();
    let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
    let mut model = W::default();
    model.uint(0).uint(MODEL_FILE);
    base(&mut model);
    model.bit(false).double(1.0).uint(1);
    for c in [5, 6, 7, 8] {
        model.uint(c);
    }
    model.uint(1).bit(true);
    let occ = Occ {
        part: 1,
        behaviour: Some((1, SHOW)),
        ..OCC
    };
    crate::prc::testw::prc_container_n(
        &[[&g.bytes(), &tree(&[occ], 0, false).bytes(), tess]],
        &model.bytes(),
    )
}

/// The unit square with stored texture coordinates, styled by the
/// texture of [`textured_globals`]; its picture is red, green over
/// blue, white. The CLI's texture fixture when `mapping` is 1.
pub(crate) fn textured_prc(mapping: i32) -> Vec<u8> {
    textured_prc_with(
        &textured_globals(mapping, 0),
        &crate::prc::tess::tests::textured_square_section(),
    )
}

/// [`textured_prc`]'s layout with the given globals and tessellation.
fn textured_prc_with(globals: &W, tess: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
        .unwrap();
    let picture = e.finish().unwrap();
    let mut model = W::default();
    model.uint(0).uint(MODEL_FILE);
    base(&mut model);
    model.bit(false).double(1.0).uint(1);
    for c in [5, 6, 7, 8] {
        model.uint(c);
    }
    // Root occurrence 0, biased by one.
    model.uint(1).bit(true);
    let occ = Occ {
        part: 1,
        behaviour: Some((1, SHOW)),
        ..OCC
    };
    crate::prc::testw::prc_container_files(
        &[[&globals.bytes(), &tree(&[occ], 0, false).bytes(), tess]],
        &model.bytes(),
        &[&picture],
    )
}

/// Globals with grey, white and orange, one 702 per `(ambient, emissive,
/// specular, shininess)` over diffuse grey, and a style per material.
fn lit_globals(materials: &[(u32, u32, u32, f64)]) -> Globals {
    let mut w = W::default();
    w.uint(FILE_STRUCTURE_GLOBALS);
    base(&mut w);
    w.uint(0)
        .double(2000.0)
        .double(40.0)
        .string(Some(""))
        .uint(0);
    w.uint(3);
    for c in [0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0] {
        w.double(c);
    }
    w.uint(0).uint(0).uint(materials.len() as u32);
    for (id, &(ambient, emissive, specular, shininess)) in (2..).zip(materials) {
        w.uint(MATERIAL);
        ref_base(&mut w, id);
        w.uint(ambient).uint(1).uint(emissive).uint(specular);
        for d in [shininess, 1.0, 1.0, 1.0, 0.25] {
            w.double(d);
        }
    }
    w.uint(0).uint(materials.len() as u32);
    for (id, m) in (20..).zip(1..=materials.len() as u32) {
        w.uint(STYLE);
        ref_base(&mut w, id);
        w.double(1.0).bit(false).uint(0).bit(true).uint(m);
        w.bit(false).bit(false).bit(false).bit(false);
    }
    w.uint(0).uint(0).uint(0);
    ctx(&w.bytes(), &Schema::default()).globals().unwrap()
}

mod fixtures;
use fixtures::*;
mod overrides_and_looks;
mod placement;
