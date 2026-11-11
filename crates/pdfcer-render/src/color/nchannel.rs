//! The two `DeviceN` attributes that change what a component IS: `/Process`
//! (ISO 32000-2 §8.6.6.5, Table 72) and `/Colorants` (Table 71).
//!
//! Both are read at resolve time, once per space. A space without them, or
//! with entries this module cannot use, resolves exactly as plain `DeviceN`.

use std::sync::Arc;

use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::view::DocumentView;

use super::{ColorDiagnostics, ColorSpace, Colorant, resolve_object};

/// The standard CMYK component names, in process-space order.
const CMYK: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];

/// The attributes dictionary, `args[3]`.
fn attributes<'a>(doc: &'a DocumentView<'_>, args: &'a [Object]) -> Option<&'a Dict> {
    args.get(3)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
}

/// Rename the components `/Process` `/Components` maps onto a CMYK process
/// space to `Cyan`/`Magenta`/`Yellow`/`Black`, so every consumer that asks
/// "is this a process colorant" by name gets Table 72's answer.
///
/// Table 72: `Components` "correspond, in order, to the components of the
/// process colour space" and "may be arbitrary". Only a four-component
/// process space (`DeviceCMYK`, or `ICCBased` with `/N 4`) maps onto the
/// CMYK planes; any other process space is left as authored. A rename that
/// would give two components one name is skipped, because colorant names
/// key the spot planes.
pub(super) fn map_process_names(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    names: &mut [Colorant],
    diag: &mut ColorDiagnostics,
) {
    let Some(process) = attributes(doc, args)
        .and_then(|a| a.get(b"Process"))
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    let space = process
        .get(b"ColorSpace")
        .and_then(|o| resolve_object(doc, doc.resolve(o), resources, depth + 1, diag));
    let cmyk = space.is_some_and(|s| {
        matches!(
            *s,
            ColorSpace::DeviceCmyk | ColorSpace::IccBased { n: 4, .. }
        )
    });
    if !cmyk {
        return;
    }
    let Some(components) = process
        .get(b"Components")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    else {
        return;
    };
    for (authored, standard) in components.iter().zip(CMYK) {
        let Some(authored) = doc.resolve(authored).as_name().map(|n| n.as_bytes()) else {
            continue;
        };
        if authored == standard || names.iter().any(|c| named(c, standard)) {
            continue;
        }
        for name in names.iter_mut().filter(|c| named(c, authored)) {
            *name = Colorant::Named(standard.into());
        }
    }
}

/// Whether `c` is the named colorant `name`.
fn named(c: &Colorant, name: &[u8]) -> bool {
    matches!(c, Colorant::Named(n) if &**n == name)
}

/// One `Separation` space per component, from `/Colorants`, describing
/// "the appearance of that colorant alone" (Table 71). Empty when no
/// component has a usable entry.
///
/// An entry is used only for a spot component (bullet 3 ignores a process
/// colorant's entry) whose value resolves to a `Separation` naming the same
/// colorant ("The key shall match the colorant name given in that colour
/// space").
pub(super) fn colorant_spaces(
    doc: &DocumentView<'_>,
    args: &[Object],
    resources: &Dict,
    depth: usize,
    names: &[Colorant],
    diag: &mut ColorDiagnostics,
) -> Arc<[Option<Arc<ColorSpace>>]> {
    let Some(colorants) = attributes(doc, args)
        .and_then(|a| a.get(b"Colorants"))
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    else {
        return Arc::from([]);
    };
    let spaces: Vec<Option<Arc<ColorSpace>>> = names
        .iter()
        .map(|c| {
            let Colorant::Named(name) = c else {
                return None;
            };
            if CMYK.contains(&&**name) {
                return None;
            }
            let space = colorants
                .get(name)
                .and_then(|o| resolve_object(doc, doc.resolve(o), resources, depth + 1, diag))?;
            matches!(&*space, ColorSpace::Separation { colorant, .. } if colorant == c)
                .then_some(space)
        })
        .collect();
    if spaces.iter().all(Option::is_none) {
        Arc::from([])
    } else {
        Arc::from(spaces.into_boxed_slice())
    }
}
