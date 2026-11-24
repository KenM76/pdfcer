#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)] // tests

use super::*;
use crate::color::RenderingIntent;
use crate::preflight::{Profile, ProfileError, ProfileRule, Severity};
use crate::prepress::{IntentChoice, PdfxOutputIntent};

/// Every variant, each with every written field off its default.
fn every_fixup() -> Vec<Fixup> {
    let flatten = FlattenOptions {
        keep_text: false,
        keep_strokes: false,
        raster_ppi: 450.0,
        ..FlattenOptions::default()
    };
    let mut colours = ColourConversion::new(ColourDestination::Profile(Vec::new()));
    colours.intent = IntentChoice::Fixed(RenderingIntent::Saturation);
    colours.device_rgb = false;
    colours.lab = true;
    colours.keep_pure_black = false;
    colours.black_point_compensation = true;
    colours.embed_output_intent = Some("FOGRA39".into());
    let mut intent = PdfxOutputIntent::new(Vec::new());
    intent.condition = Some("FOGRA39".into());
    intent.registry_name = Some("http://www.color.org".into());
    intent.info = Some("Coated".into());
    let mut pdfx = PdfxConversion::new(intent);
    pdfx.title = Some("Job 7".into());
    pdfx.trapped = Some(false);
    pdfx.spot_to_process = Some(crate::prepress::InkSelection::Only(vec!["Gold".into()]));
    pdfx.flatten = flatten.clone();
    pdfx.rendering_intent = IntentChoice::ObjectsOwn;
    pdfx.infer_trim_boxes = !pdfx.infer_trim_boxes;
    pdfx.require_conforms = !pdfx.require_conforms;
    pdfx.keep_partial = !pdfx.keep_partial;
    vec![
        Fixup::FlattenTransparency(flatten),
        Fixup::FixHairlines { min_width_pt: 0.3 },
        Fixup::AliasInk {
            from: "PANTONE 185 C".into(),
            to: "Red \"spot\"".into(),
        },
        Fixup::SpotToProcess {
            keep: vec!["Varnish".into()],
        },
        Fixup::ConvertColours(colours),
        Fixup::ConvertToPdfx {
            level: PdfxLevel::X4,
            conversion: Box::new(pdfx),
        },
        Fixup::FixPdfua {
            part: PdfuaPart::Ua2,
            options: FixPdfuaOptions::default()
                .with_lang("fr-CA")
                .with_title("Rapport"),
            step: Some(PdfuaFixup::TabOrder),
        },
        Fixup::DownsampleImages {
            above_ppi: 450,
            to_ppi: 300,
            method: ResampleMethod::Average,
        },
    ]
}

#[test]
fn every_fixup_round_trips_through_profile_json() {
    let fixups = every_fixup();
    let names: Vec<&str> = fixups.iter().map(Fixup::name).collect();
    assert_eq!(names, Fixup::NAMES);
    let mut rule = ProfileRule::new("r", Check::TransparencyUsed, Severity::Error);
    rule.fixups = fixups;
    let profile = Profile {
        name: "p".into(),
        description: String::new(),
        rules: vec![rule],
    };
    let json = profile.to_json();
    let back = Profile::from_json(json.as_bytes()).unwrap();
    assert_eq!(back, profile, "{json}");
    assert_eq!(back.to_json(), json);
}

#[test]
fn icc_bytes_are_never_written() {
    let rule = ProfileRule::new("r", Check::TransparencyUsed, Severity::Error).with_fixup(
        Fixup::ConvertColours(ColourConversion::new(ColourDestination::Profile(vec![
            0xAB;
            64
        ]))),
    );
    let profile = Profile {
        name: "p".into(),
        description: String::new(),
        rules: vec![rule],
    };
    let back = Profile::from_json(profile.to_json().as_bytes()).unwrap();
    let Fixup::ConvertColours(c) = &back.rules[0].fixups[0] else {
        panic!("not a colour conversion");
    };
    assert_eq!(c.destination, ColourDestination::Profile(Vec::new()));
}

fn refused(fixup: &str) -> String {
    let json = format!(
        "{{\"name\": \"p\", \"rules\": [{{\"id\": \"r\", \"check\": \"transparency-used\", \
         \"severity\": \"error\", \"fixups\": [{fixup}]}}]}}"
    );
    match Profile::from_json(json.as_bytes()) {
        Err(ProfileError::BadValue { rule, why }) => {
            assert_eq!(rule, "r");
            why
        }
        other => panic!("{fixup}: {other:?}"),
    }
}

#[test]
fn a_bad_fixup_is_refused_with_its_name() {
    assert!(
        refused(r#"{"fixup": "shrink-everything"}"#)
            .contains("unknown; known: flatten-transparency")
    );
    assert!(refused(r#"{"fixup": "fix-hairlines", "min_width_pt": 0}"#).contains("min_width_pt"));
    assert!(
        refused(r#"{"fixup": "downsample-images", "above_ppi": 200, "to_ppi": 300}"#)
            .starts_with("fixup `downsample-images`: ")
    );
    assert!(refused(r#"{"name": "no-fixup-member"}"#).contains("`fixup` name"));
}

#[test]
fn a_downsample_defaults_to_its_ceiling_and_bicubic() {
    let json = br#"{"name": "p", "rules": [{"id": "r", "check": "image-above-ppi",
        "max_ppi": 300, "severity": "warning",
        "fixups": [{"fixup": "downsample-images", "above_ppi": 300}]}]}"#;
    let p = Profile::from_json(json).unwrap();
    assert_eq!(p.rules[0].check, Check::ImageAbovePpi { max_ppi: 300.0 });
    assert_eq!(
        p.rules[0].fixups,
        vec![Fixup::DownsampleImages {
            above_ppi: 300,
            to_ppi: 300,
            method: ResampleMethod::Bicubic,
        }]
    );
    assert_eq!(
        Fixup::suggested(&p.rules[0].check),
        Some(p.rules[0].fixups[0].clone())
    );
}

#[test]
fn rule_matches_a_rule_and_its_sub_rules_only() {
    assert!(rule_matches("pdfx", "pdfx"));
    assert!(rule_matches("pdfx/PXQ-AN-1", "pdfx"));
    assert!(!rule_matches("pdfx-4", "pdfx"));
    assert!(!rule_matches("pdf", "pdfx"));
}
