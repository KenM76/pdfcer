#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, only, saved_text, session};
use super::*;
use crate::content::ContentStream;
use crate::edit::EditSession;
use crate::graph::ObjectGraph;
use crate::object::Object;

const STRIPS: usize = 20;

/// One page (3) drawing `STRIPS` 16x2 grey strips (objects 5 on), strip `i`
/// filled with the letter `A + i`, each 10pt tall, from the top down
/// (`up` draws them bottom first). `extra` is added to every strip.
fn sample(up: bool, extra: &str, gap: f64) -> Vec<u8> {
    build(&bodies(up, extra, gap))
}

fn bodies(up: bool, extra: &str, gap: f64) -> Vec<Vec<u8>> {
    let b = |s: &str| s.as_bytes().to_vec();
    let mut names = String::new();
    let mut content = String::new();
    let order: Vec<usize> = if up {
        (0..STRIPS).rev().collect()
    } else {
        (0..STRIPS).collect()
    };
    for i in 0..STRIPS {
        names.push_str(&format!("/S{i} {} 0 R ", i + 5));
    }
    for i in order {
        let f = 190.0 - 10.0 * i as f64 - if i == STRIPS - 1 { gap } else { 0.0 };
        content.push_str(&format!("q 40 0 0 10 0 {f} cm /S{i} Do Q\n"));
    }
    let mut bodies = vec![
        b("<< /Type /Catalog /Pages 2 0 R >>"),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /XObject << {names}>> >> >>"
        )),
        b(&format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )),
    ];
    for i in 0..STRIPS {
        let row = char::from(b'A' + i as u8).to_string().repeat(32);
        bodies.push(b(&format!(
            "<< /Type /XObject /Subtype /Image /Width 16 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 {extra}/Length 32 >>\nstream\n{row}\nendstream"
        )));
    }
    bodies
}

fn fragments() -> DiscardOptions {
    DiscardOptions::none().with_object(DiscardObject::MergeImageFragments)
}

fn content(s: &EditSession) -> String {
    let view = s.view();
    let page = crate::page_tree::pages_in(&view).unwrap().remove(0);
    String::from_utf8(ContentStream::from_page(&view, &page).unwrap().buf).unwrap()
}

/// The decoded samples of the only image the page now draws.
fn merged_samples(s: &EditSession) -> Vec<u8> {
    let view = s.view();
    let page = crate::page_tree::pages_in(&view).unwrap().remove(0);
    let xobjects = view.resolve(page.resources.get(b"XObject").unwrap());
    let text = content(s);
    let name = text.split('/').nth(1).unwrap().split(' ').next().unwrap();
    let r = xobjects.as_dict().unwrap().get(name.as_bytes()).unwrap();
    let Object::Stream(img) = view.resolve(r) else {
        panic!("not a stream");
    };
    let raw = view.slice(img.data_span).unwrap();
    crate::filters::decode_stream(&img.dict, raw).unwrap()
}

fn expected() -> Vec<u8> {
    (0..STRIPS)
        .flat_map(|i| std::iter::repeat_n(b'A' + i as u8, 32))
        .collect()
}

#[test]
fn the_plan_lists_the_page_and_measures_it() {
    let s = session(sample(false, "", 0.0));
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    let [item] = plan.discard_objects.as_slice() else {
        panic!("{:?}", plan.discard_objects);
    };
    assert_eq!(item.location, "page 1: 20 strips into 1 image (16x40)");
    assert!(item.bytes > 0);
    assert!(plan.notes.iter().any(|n| n.contains("thin strips")));
}

#[test]
fn the_strips_become_one_image_drawn_in_their_place() {
    let mut s = session(sample(false, "", 0.0));
    let before = content(&s);
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    s.apply_optimize(&plan).unwrap();
    let after = content(&s);
    assert_eq!(after.matches(" Do").count(), 1, "{after}");
    assert!(after.contains("q 40 0 0 200 0 0 cm /"), "{after}");
    assert_eq!(merged_samples(&s), expected());
    s.undo();
    assert_eq!(content(&s), before);
}

#[test]
fn strips_drawn_bottom_first_still_merge_top_row_first() {
    let mut s = session(sample(true, "", 0.0));
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(merged_samples(&s), expected());
}

#[test]
fn a_gap_splits_the_run_and_a_soft_mask_refuses_it() {
    let s = session(sample(false, "", 5.0));
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    assert_eq!(
        plan.discard_objects[0].location,
        "page 1: 19 strips into 1 image (16x38)"
    );
    let s = session(sample(false, "/SMask 4 0 R ", 0.0));
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    assert!(plan.discard_objects.is_empty());
}

#[test]
fn the_saved_file_drops_the_strips_even_with_recompression() {
    let mut s = session(sample(false, "", 0.0));
    let mut settings = only(fragments());
    settings.streams = StreamAction::Recompress { level: 9 };
    let plan = s.plan_optimize(&settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    let text = saved_text(&mut s, &plan);
    assert_eq!(text.matches("/Subtype /Image").count(), 1);
    assert!(!text.contains(&"A".repeat(32)), "a strip survived");
}

#[test]
fn inherited_or_shared_resources_refuse_the_merge() {
    let mut inherited = bodies(false, "", 0.0);
    let page = String::from_utf8(inherited[2].clone()).unwrap();
    let (head, res) = page.split_once(" /Resources").unwrap();
    inherited[1] = format!(
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources{}",
        res.trim_end_matches(" >>")
    )
    .into_bytes();
    inherited[1].extend_from_slice(b" >>");
    inherited[2] = format!("{head} >>").into_bytes();
    let s = session(build(&inherited));
    assert!(
        s.plan_optimize(&only(fragments()))
            .unwrap()
            .discard_objects
            .is_empty()
    );

    // The XObject dictionary is its own object, also held by the catalog.
    let mut shared = bodies(false, "", 0.0);
    let page = String::from_utf8(shared[2].clone()).unwrap();
    let start = page.find("<< /S0").unwrap();
    let end = page.rfind(">> >> >>").unwrap();
    shared.push(page.as_bytes()[start..end + 2].to_vec());
    let id = shared.len();
    shared[2] = format!("{}{id} 0 R >> >>", &page[..start]).into_bytes();
    shared[0] = format!("<< /Type /Catalog /Pages 2 0 R /Extra {id} 0 R >>").into_bytes();
    let s = session(build(&shared));
    assert!(
        s.plan_optimize(&only(fragments()))
            .unwrap()
            .discard_objects
            .is_empty()
    );
    shared[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let s = session(build(&shared));
    assert_eq!(
        s.plan_optimize(&only(fragments()))
            .unwrap()
            .discard_objects
            .len(),
        1
    );
}

#[test]
fn undo_restores_the_strips_and_their_names() {
    let mut s = session(sample(false, "", 0.0));
    let plan = s.plan_optimize(&only(fragments())).unwrap();
    s.apply_optimize(&plan).unwrap();
    s.undo();
    let view = s.view();
    let page = crate::page_tree::pages_in(&view).unwrap().remove(0);
    let xobjects = view.resolve(page.resources.get(b"XObject").unwrap());
    assert_eq!(xobjects.as_dict().unwrap().len(), STRIPS);
    assert!(view.value(crate::object::ObjId::new(5, 0)).is_some());
}
