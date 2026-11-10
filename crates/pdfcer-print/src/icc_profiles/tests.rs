#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::icc_encode::{D50, curv_tag, matrix_trc_rgb, mluc_tag, para_tag, profile, xyz_tag};

fn rgb(desc: &str) -> Vec<u8> {
    let rows = [
        [0.4361, 0.3851, 0.1431],
        [0.2225, 0.7169, 0.0606],
        [0.0139, 0.0971, 0.7141],
    ];
    matrix_trc_rgb(b"prtr", desc, rows, &para_tag(0, &[2.2]))
}

fn grey(desc: &str) -> Vec<u8> {
    profile(
        b"mntr",
        b"GRAY",
        &[
            (b"desc", mluc_tag(desc)),
            (b"wtpt", xyz_tag(D50)),
            (b"kTRC", curv_tag(&[0, 65535])),
        ],
    )
}

/// A fresh, empty scratch folder per test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-x064-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_destination_listing_has_only_rgb_and_grey_sorted() {
    let dir = scratch("list");
    std::fs::write(dir.join("zz-rgb.ICC"), rgb("Beta RGB Printer")).unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("g.icm"), grey("alpha grey")).unwrap();
    std::fs::write(
        dir.join("cmyk.icc"),
        include_bytes!("../../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc"),
    )
    .unwrap();
    std::fs::write(dir.join("junk.icc"), b"not a profile").unwrap();
    std::fs::write(dir.join("notes.txt"), rgb("text file")).unwrap();

    let all = installed_profiles_in(std::slice::from_ref(&dir));
    assert_eq!(all.len(), 3, "{all:?}");
    let dest = print_destinations(&all);
    let names: Vec<(&str, ProfileSpace)> = dest
        .iter()
        .map(|p| (p.name.as_str(), p.color_space))
        .collect();
    assert_eq!(
        names,
        [
            ("alpha grey", ProfileSpace::Gray),
            ("Beta RGB Printer", ProfileSpace::Rgb)
        ]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_two_levels_down_is_not_read() {
    let dir = scratch("deep");
    let deep = dir.join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("deep.icc"), rgb("deep")).unwrap();
    std::fs::write(dir.join("a").join("one.icc"), rgb("one level")).unwrap();
    let all = installed_profiles_in(std::slice::from_ref(&dir));
    let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["one level"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lookup_by_description_file_name_and_stem() {
    let dir = scratch("find");
    std::fs::write(dir.join("Printer-A.icc"), rgb("Glossy Photo Paper")).unwrap();
    std::fs::write(dir.join("other.icm"), grey("Plain")).unwrap();
    let all = installed_profiles_in(std::slice::from_ref(&dir));
    let path_of = |q: &str| find_installed(&all, q).map(|p| p.path.file_name().unwrap().to_owned());
    assert_eq!(path_of("glossy photo PAPER").unwrap(), "Printer-A.icc");
    assert_eq!(path_of("printer-a.ICC").unwrap(), "Printer-A.icc");
    assert_eq!(path_of("PRINTER-A").unwrap(), "Printer-A.icc");
    assert_eq!(path_of("plain").unwrap(), "other.icm");
    assert!(path_of("missing").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_profile_without_a_description_is_named_by_its_stem() {
    let dir = scratch("stem");
    let bare = profile(
        b"mntr",
        b"GRAY",
        &[(b"wtpt", xyz_tag(D50)), (b"kTRC", curv_tag(&[0, 65535]))],
    );
    std::fs::write(dir.join("Bare Grey.icc"), bare).unwrap();
    let all = installed_profiles_in(std::slice::from_ref(&dir));
    assert_eq!(all[0].name, "Bare Grey");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn profile_folders_returns_only_existing_folders() {
    assert!(profile_folders().iter().all(|p| p.is_dir()));
    assert!(!candidate_folders().is_empty());
}
