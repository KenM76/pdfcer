#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn patch(text: &str) -> ImagePatch {
    match StreamAction::parse(text).unwrap() {
        StreamAction::Image(p) => p,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_spelling_round_trips_without_whitespace() {
    let p = patch("image( reset, method = lanczos )");
    assert_eq!((p.reset, p.method), (true, Some(ResampleMethod::Lanczos)));
    let full = "image(reset,method=subsample,ppi=150,above=200,encode=flate-6,min-saving=10)";
    let p = patch(full);
    assert_eq!(p.to_string(), full);
    assert_eq!(StreamAction::Image(p).to_string(), full);
    assert_eq!(patch("image()"), ImagePatch::new());
    assert_eq!(
        patch("image(encode=flate)").encode,
        Some(ImageEncode::Flate { level: 9 })
    );
}

#[test]
fn bad_values_are_refused_by_name() {
    for (text, needle) in [
        ("image(ppi=0)", "at least 1"),
        ("image(ppi=300,above=150)", "below ppi"),
        ("image(quality=0)", "1 to 100"),
        ("image(quality=best)", "bad quality"),
        ("image(chroma=411)", "unknown chroma"),
        ("image(progressive=yes)", "bad value"),
        ("image(colour=1)", "unknown key"),
        ("image(method=fast)", "unknown method"),
        ("image(encode=flate-10)", "unknown encode"),
        ("image(min-saving=100)", "0 to 99"),
        ("image(ppi=1,ppi=2)", "duplicate"),
        ("image(ppi=x)", "bad value"),
        ("image(ppi=150", "expected image("),
    ] {
        let e = StreamAction::parse(text).unwrap_err();
        assert!(e.contains(needle), "{text}: {e}");
    }
}

fn act(text: &str) -> StreamAction {
    StreamAction::parse(text).unwrap()
}

#[test]
fn a_patch_merges_field_by_field_and_names_each_source() {
    let r = fold([
        (act("recompress-6"), SettingSource::Streams),
        (act("image(ppi=150,method=average)"), SettingSource::Rule(0)),
        (act("image(method=off)"), SettingSource::Object),
    ]);
    assert_eq!(r.base, StreamAction::Recompress { level: 6 });
    let img = r.image.unwrap();
    assert_eq!(
        img.method,
        sourced(ResampleMethod::Off, SettingSource::Object)
    );
    assert_eq!(img.ppi, Some(sourced(150, SettingSource::Rule(0))));
    assert_eq!(
        img.encode,
        sourced(ImageEncode::Flate { level: 6 }, SettingSource::Streams)
    );
    assert!(!img.resamples());
}

#[test]
fn a_whole_state_action_discards_earlier_image_settings() {
    let r = fold([
        (act("image(ppi=150)"), SettingSource::Streams),
        (act("leave"), SettingSource::Rule(0)),
    ]);
    assert_eq!((r.base, r.image), (StreamAction::Leave, None));
    let r = fold([
        (act("image(ppi=150)"), SettingSource::Streams),
        (act("image(reset,above=400)"), SettingSource::Rule(1)),
    ]);
    let img = r.image.unwrap();
    assert_eq!((img.ppi, img.above.map(|a| a.value)), (None, Some(400)));
    assert_eq!(
        img.encode,
        sourced(ImageEncode::Keep, SettingSource::BuiltIn)
    );
}

#[test]
fn the_trigger_defaults_to_one_and_a_half_times_and_never_falls_below_ppi() {
    let img = |t: &[&str]| {
        fold(t.iter().map(|s| (act(s), SettingSource::Streams)))
            .image
            .unwrap()
    };
    assert_eq!(img(&["image(ppi=150)"]).trigger(), Some(225.0));
    assert_eq!(
        img(&["image(above=200)", "image(ppi=300)"]).trigger(),
        Some(300.0)
    );
    assert_eq!(img(&["image(above=200)"]).trigger(), None);
}

#[test]
fn an_action_that_converts_or_writes_jpeg_may_be_lossy() {
    for (text, lossy) in [
        ("image(convert=gray)", true),
        ("image(convert=keep)", false),
        ("image(encode=jpeg)", true),
        ("image(encode=flate)", false),
        ("image(ppi=150)", true),
        ("image(ppi=150,method=off)", false),
    ] {
        assert_eq!(
            StreamAction::parse(text).unwrap().is_lossy(),
            lossy,
            "{text}"
        );
    }
}

#[test]
fn the_conversion_keys_parse_round_trip_and_refuse_bad_values() {
    let full = "image(convert=bilevel,threshold=120)";
    let p = patch(full);
    assert_eq!(p.convert, Some(ImageConvert::Bilevel));
    assert_eq!(p.threshold, Some(Threshold::Fixed(120)));
    assert_eq!(p.to_string(), full);
    let p = patch("image(convert=indexed,colors=16)");
    assert_eq!(p.colors, Some(16));
    assert_eq!(
        patch("image(threshold=dither)").threshold,
        Some(Threshold::Dither)
    );
    for (text, needle) in [
        ("image(convert=sepia)", "convert"),
        ("image(threshold=300)", "bad threshold"),
        ("image(colors=1)", "2 to 256"),
        ("image(colors=257)", "2 to 256"),
        ("image(convert=bilevel,encode=jpeg)", "JPEG"),
        ("image(convert=indexed,encode=jpeg)", "JPEG"),
    ] {
        let e = StreamAction::parse(text).unwrap_err();
        assert!(e.contains(needle), "{text}: {e}");
    }
}
