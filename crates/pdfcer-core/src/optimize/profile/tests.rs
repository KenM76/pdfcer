#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

fn none(name: &str) -> Result<String, String> {
    Err(format!("no profile {name}"))
}

fn full() -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.streams = StreamAction::Recompress { level: 6 };
    s.allow_signed = true;
    s.structure.merge_duplicates = true;
    s.structure.object_streams = true;
    s.cleanup.invalid_bookmarks = true;
    s.discard = DiscardOptions::none()
        .with(MetadataKind::Thumbnail)
        .with(MetadataKind::DocumentXmp);
    s.rules.push(Rule {
        select: Selector {
            categories: vec![SpaceCategory::Fonts, SpaceCategory::ContentStreams],
            pages: Some(PageRanges::parse("1-3,7,10-").unwrap()),
            min_bytes: Some(1000),
            max_bytes: Some(500_000),
            filters: vec!["LZWDecode".into(), "none".into()],
            color_spaces: vec!["DeviceRGB".into()],
            min_ppi: Some(150.0),
            max_ppi: Some(600.5),
            ids: vec![ObjId::new(12, 0), ObjId::new(13, 2)],
        },
        action: StreamAction::Leave,
    });
    s.rules.push(Rule {
        select: Selector::new(),
        action: StreamAction::Recompress { level: 1 },
    });
    s.objects
        .insert(ObjId::new(14, 0), StreamAction::Recompress { level: 9 });
    s
}

#[test]
fn a_written_profile_reads_back_identically() {
    let text = to_profile(&full());
    assert_eq!(parse("p.toml", &text, &mut none).unwrap(), full());
}

#[test]
fn the_module_example_parses() {
    let text = "extends = \"standard\"\nstreams = \"recompress-9\"\nallow-signed = false\n\
                [[rule]]\ncategory = [\"fonts\"]\npages = \"1-3,7\"\nmin-bytes = 1000\n\
                max-bytes = 500000\nfilter = [\"LZWDecode\", \"none\"]\ncolor-space = [\"DeviceRGB\"]\n\
                min-ppi = 150\nmax-ppi = 600\nids = [\"12 0\"]\naction = \"leave\"\n\
                [[object]]\nid = \"14 0\"\naction = \"recompress-6\"\n";
    let s = parse("p.toml", text, &mut none).unwrap();
    assert_eq!(s.rules.len(), 1);
    assert_eq!(s.rules[0].select.min_ppi, Some(150.0));
    assert_eq!(
        s.objects[&ObjId::new(14, 0)],
        StreamAction::Recompress { level: 6 }
    );
}

#[test]
fn a_child_extends_a_parent_profile_which_extends_a_preset() {
    let parent = "extends = \"lossless\"\n[[rule]]\nids = [5]\naction = \"leave\"\n\
                  [[object]]\nid = 7\naction = \"leave\"\n";
    let child = "extends = \"base.toml\"\n[[rule]]\nids = [\"6 0\"]\naction = \"recompress-2\"\n\
                 [[object]]\nid = \"7\"\naction = \"recompress-3\"\n";
    let mut resolve = |n: &str| {
        if n == "base.toml" {
            Ok(parent.to_owned())
        } else {
            none(n)
        }
    };
    let s = parse("child.toml", child, &mut resolve).unwrap();
    assert_eq!(s.streams, Preset::Lossless.settings().streams);
    let ids: Vec<u32> = s.rules.iter().map(|r| r.select.ids[0].num).collect();
    assert_eq!(ids, [5, 6]);
    assert_eq!(
        s.objects[&ObjId::new(7, 0)],
        StreamAction::Recompress { level: 3 }
    );
}

#[test]
fn a_cycle_and_an_over_deep_chain_are_errors() {
    let mut cyc = |n: &str| {
        Ok(format!(
            "extends = \"{}\"\n",
            if n == "a" { "b" } else { "a" }
        ))
    };
    let e = parse("a", "extends = \"b\"\n", &mut cyc).unwrap_err();
    assert!(e.message.contains("cycle"), "{e}");
    let mut deep = |n: &str| Ok(format!("extends = \"{n}x\"\n"));
    let e = parse("x", "extends = \"xx\"\n", &mut deep).unwrap_err();
    assert!(e.message.contains("deeper"), "{e}");
}

#[test]
fn errors_name_the_line_and_the_problem() {
    let cases = [
        (
            "streams = \"leave\"\nstream = \"leave\"\n",
            2,
            "unknown key",
        ),
        ("[[rule]]\npages = \"1\"\n", 1, "action"),
        ("[[rule]]\naction = \"shrink\"\n", 2, "shrink"),
        ("[[object]]\nid = 0\naction = \"leave\"\n", 2, ""),
        ("[[table]]\n", 1, "unknown table"),
        ("a = 1\na = 2\n", 2, "duplicate"),
        ("streams = \"leave\" junk\n", 1, "unexpected"),
        ("streams = \"leave\n", 1, "unterminated"),
        ("streams = [[\"x\"]]\n", 1, "nested"),
        ("allow-signed = 1\n", 1, ""),
        (
            "[[rule]]\ncategory = [\"nope\"]\naction = \"leave\"\n",
            2,
            "nope",
        ),
        ("[[rule]]\npages = \"3-1\"\naction = \"leave\"\n", 2, ""),
        ("extends = \"missing\"\n", 1, "no profile missing"),
    ];
    for (text, line, needle) in cases {
        let e = parse("p", text, &mut none).unwrap_err();
        assert_eq!((e.line, e.profile.as_str()), (line, "p"), "{text:?}: {e}");
        assert!(e.message.contains(needle), "{text:?}: {e}");
    }
}

#[test]
fn strings_unescape_and_quote_round_trips() {
    let tricky = "a\"b\\c\nd\te\u{1}f'g";
    let text = format!(
        "[[rule]]\ncolor-space = [{}, 'lit\\eral']\naction = \"leave\"\n",
        syntax::quote(tricky)
    );
    let s = parse("p", &text, &mut none).unwrap();
    assert_eq!(s.rules[0].select.color_spaces, [tricky, "lit\\eral"]);
}

#[test]
fn an_oversized_profile_is_refused() {
    let text = "#".repeat(MAX_PROFILE_BYTES + 1);
    assert_eq!(parse("p", &text, &mut none).unwrap_err().line, 0);
}

#[test]
fn one_line_rules_and_object_overrides_parse() {
    let r = parse_rule(
        "category=images,fonts pages=2- min-bytes=4096 max-ppi=300 ids=4,5 action=leave",
    )
    .unwrap();
    assert_eq!(
        r.select.categories,
        [SpaceCategory::Images, SpaceCategory::Fonts]
    );
    assert_eq!(r.select.max_ppi, Some(300.0));
    assert_eq!(r.select.ids, [ObjId::new(4, 0), ObjId::new(5, 0)]);
    for bad in [
        "action=leave junk",
        "min-bytes=x action=leave",
        "pages=1",
        "colour=x action=leave",
        "action=leave action=leave",
    ] {
        assert!(parse_rule(bad).is_err(), "{bad}");
    }
    assert_eq!(
        parse_object("12 3=recompress-4").unwrap(),
        (ObjId::new(12, 3), StreamAction::Recompress { level: 4 })
    );
    assert_eq!(
        parse_object("12=leave").unwrap(),
        (ObjId::new(12, 0), StreamAction::Leave)
    );
    for bad in ["12", "0=leave", "x=leave", "12=squash"] {
        assert!(parse_object(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_structure_key_overrides_the_preset() {
    let text = "extends = \"standard\"
object-streams = false
";
    let s = parse("p.toml", text, &mut none).unwrap();
    assert!(s.structure.remove_unreferenced && s.structure.merge_duplicates);
    assert!(!s.structure.object_streams);
}

#[test]
fn a_cleanup_key_overrides_the_preset() {
    let text = "extends = \"screen\"
remove-invalid-links = false
";
    let s = parse("p.toml", text, &mut none).unwrap();
    assert!(!s.cleanup.invalid_links);
    assert!(s.cleanup.invalid_bookmarks && s.cleanup.unreferenced_destinations);
    assert_eq!(parse("p.toml", &to_profile(&s), &mut none).unwrap(), s);
}

#[test]
fn a_discard_list_replaces_the_presets() {
    let s = parse(
        "p.toml",
        "extends = \"screen\"
discard = [\"info\"]
",
        &mut none,
    )
    .unwrap();
    assert_eq!(
        s.discard.kinds().collect::<Vec<_>>(),
        [MetadataKind::InfoEntry]
    );
    let all = parse(
        "p.toml",
        "discard = \"all\"
",
        &mut none,
    )
    .unwrap();
    assert_eq!(all.discard, DiscardOptions::all());
    let empty = parse(
        "p.toml",
        "extends = \"screen\"
discard = []
",
        &mut none,
    )
    .unwrap();
    assert!(empty.discard.is_none());
    assert_eq!(
        parse("p.toml", &to_profile(&empty), &mut none).unwrap(),
        empty
    );
    let bad = parse(
        "p.toml",
        "discard = [\"thumbs\"]
",
        &mut none,
    )
    .unwrap_err();
    assert!(bad.message.contains("unknown discard kind"), "{bad}");
}
