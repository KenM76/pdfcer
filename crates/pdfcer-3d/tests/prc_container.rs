//! PRC container walk [WD 6.1-6.2] against synthetic streams.

use std::io::Write;

use pdfcer_3d::{PrcError, PrcFile, SectionKind, UniqueId};

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn u32s(out: &mut Vec<u8>, vs: &[u32]) {
    for v in vs {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// One file structure with the given section payloads and model file.
fn build(sections: [&[u8]; 5], model: &[u8], padding: usize) -> Vec<u8> {
    const HEADER_LEN: usize = 107;
    let mut fs_header = b"PRC".to_vec();
    u32s(&mut fs_header, &[8137, 8137, 5, 6, 7, 8, 0, 0, 0, 0, 1, 3]);
    fs_header.extend_from_slice(b"\xff\xd8\xff");

    let mut body = fs_header.clone();
    let mut offs = vec![HEADER_LEN];
    for s in sections {
        offs.push(HEADER_LEN + body.len());
        body.extend(zlib(s));
        body.extend(std::iter::repeat_n(0u8, padding));
    }
    let mf_start = HEADER_LEN + body.len();
    body.extend(zlib(model));
    let mf_end = HEADER_LEN + body.len();

    let mut out = b"PRC".to_vec();
    u32s(&mut out, &[8137, 8137, 1, 2, 3, 4, 0, 0, 0, 0, 1]);
    u32s(&mut out, &[5, 6, 7, 8, 0, 6]);
    u32s(
        &mut out,
        &offs.iter().map(|&o| o as u32).collect::<Vec<_>>(),
    );
    u32s(&mut out, &[mf_start as u32, mf_end as u32, 0]);
    assert_eq!(out.len(), HEADER_LEN);
    out.extend(body);
    out
}

const SECTIONS: [&[u8]; 5] = [b"globals", b"tree", b"tess", b"", b"extra"];

#[test]
fn the_container_walk_inflates_every_section() {
    for padding in [0, 3] {
        let f = PrcFile::parse(&build(SECTIONS, b"model file", padding)).unwrap();
        assert_eq!(f.header.min_version_for_read, 8137);
        assert_eq!(f.header.file_id, UniqueId([1, 2, 3, 4]));
        assert_eq!(f.file_structures.len(), 1);
        let fs = &f.file_structures[0];
        assert_eq!(fs.id, UniqueId([5, 6, 7, 8]));
        assert_eq!(fs.pictures, vec![b"\xff\xd8\xff".to_vec()]);
        for (kind, want) in SectionKind::ALL.into_iter().zip(SECTIONS) {
            assert_eq!(fs.section(kind), want, "{}", kind.name());
        }
        assert_eq!(f.model_file, b"model file");
    }
}

#[test]
fn damaged_containers_are_refused_by_name() {
    let good = build(SECTIONS, b"m", 0);
    assert_eq!(PrcFile::parse(b"U3D\0").unwrap_err(), PrcError::NotPrc);
    assert!(matches!(
        PrcFile::parse(&good[..40]),
        Err(PrcError::Truncated(_))
    ));

    // Reserved field (offset 63) not zero.
    let mut bad = good.clone();
    bad[63] = 1;
    assert!(matches!(PrcFile::parse(&bad), Err(PrcError::Malformed(_))));

    // File-structure id disagrees with its description (FS header id at 107+11).
    let mut bad = good.clone();
    bad[118] ^= 0xff;
    assert!(matches!(PrcFile::parse(&bad), Err(PrcError::Malformed(_))));

    // A section that is not zlib: corrupt the globals stream's header byte.
    let globals = u32::from_le_bytes(good[75..79].try_into().unwrap()) as usize;
    let mut bad = good.clone();
    bad[globals] = 0;
    assert!(matches!(
        PrcFile::parse(&bad),
        Err(PrcError::Inflate {
            section: "globals",
            ..
        })
    ));

    // Offsets out of order.
    let mut bad = good;
    bad[75..79].copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(PrcFile::parse(&bad), Err(PrcError::Malformed(_))));
}

#[test]
fn the_inflation_ceiling_is_reported_not_exceeded() {
    let f = build([&[0u8; 600], &[0u8; 600], b"", b"", b""], b"", 0);
    assert!(PrcFile::parse_with_limit(&f, 1200).is_ok());
    assert_eq!(
        PrcFile::parse_with_limit(&f, 1199).unwrap_err(),
        PrcError::TooLarge { limit: 1199 }
    );
}
