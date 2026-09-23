//! Byte-authored ICC profiles — no ICC file on disk, no corpus dependency.
//!
//! ## Why a measurement tool gets tests
//!
//! This is a parser of **untrusted input**: every byte comes from a PDF in a
//! corpus whose entire purpose is to contain the files that break parsers.
//! The project rule is a fixture-based test per parser branch, and *"it is
//! only a measurement tool"* is not an exemption — a census that silently
//! mis-parses is **worse** than no census, because its numbers are about to
//! be written into another project's `NUMERIC_CLAIMS.md` and cited as
//! evidence against one of their constants.
//!
//! Every profile here is built byte by byte rather than read from disk, so a
//! test cannot inherit a bug from a real file, and so the expected value of
//! each field is visible at the point its assertion is written.

use super::{HEADER_LEN, Profile, SIG_OFFSET, TransformShape, parse};

/// A minimal well-formed profile: 128-byte header, a tag table, and whatever
/// tags a test asks for. `tags` is `(signature, body)`.
fn build(
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    ver: u32,
    tags: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let table_len = 4 + tags.len() * 12;
    let mut header = vec![0u8; HEADER_LEN];
    header[8..12].copy_from_slice(&ver.to_be_bytes());
    header[12..16].copy_from_slice(class);
    header[16..20].copy_from_slice(space);
    header[20..24].copy_from_slice(pcs);
    header[SIG_OFFSET..SIG_OFFSET + 4].copy_from_slice(b"acsp");

    let mut table = Vec::new();
    let mut body = Vec::new();
    table.extend_from_slice(&u32::try_from(tags.len()).unwrap().to_be_bytes());
    for (sig, data) in tags {
        let off = HEADER_LEN + table_len + body.len();
        table.extend_from_slice(sig);
        table.extend_from_slice(&u32::try_from(off).unwrap().to_be_bytes());
        table.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        body.extend_from_slice(data);
    }
    let mut out = header;
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    out
}

/// An `mft2` lookup tag with the given input/output channel counts and grid
/// dimension — ICC.1:2010 §10.11, which puts those at bytes 8, 9 and 10.
fn mft2(inputs: u8, outputs: u8, grid: u8) -> Vec<u8> {
    let mut t = vec![0u8; 52];
    t[0..4].copy_from_slice(b"mft2");
    t[8] = inputs;
    t[9] = outputs;
    t[10] = grid;
    t
}

fn one(bytes: &[u8]) -> Profile {
    parse(bytes, None).expect("a well-formed profile parses")
}

#[test]
fn bytes_without_the_acsp_signature_are_not_a_profile() {
    // The whole census rests on this. Profiles are found BY SIGNATURE, so a
    // scan that accepted arbitrary bytes would report a population of
    // content streams and images.
    assert!(parse(&[0u8; 512], None).is_none());
    let mut nearly = build(b"mntr", b"RGB ", b"XYZ ", 0x0210_0000, &[]);
    nearly[SIG_OFFSET] = b'X';
    assert!(parse(&nearly, None).is_none(), "one wrong byte must reject");
}

#[test]
fn every_truncation_is_refused_rather_than_read_past_its_end() {
    let full = build(
        b"prtr",
        b"CMYK",
        b"Lab ",
        0x0210_0000,
        &[(*b"A2B0", mft2(4, 3, 9))],
    );
    // The corpus contains truncated files. A census that aborted on one
    // would be a census of the files that happened to be complete. Every
    // prefix must either parse or refuse, and never panic.
    for cut in 0..full.len() {
        let _ = parse(&full[..cut], None);
    }
}

#[test]
fn an_absurd_tag_count_does_not_allocate() {
    // A malformed count is a hostile allocation, not a parse error. The cap
    // is a guard, not a claim that 4,096 tags is legal.
    let mut p = build(b"mntr", b"RGB ", b"XYZ ", 0x0210_0000, &[]);
    p[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(parse(&p, None).is_none());
}

#[test]
fn the_version_nibbles_are_read_as_bcd_not_as_a_whole_byte() {
    // §7.2.4 packs minor and bug-fix into ONE byte as two BCD nibbles.
    // Reading that byte whole reports v2.16 for v2.1 — a version
    // distribution that looks plausible and is wrong, which is the worst
    // kind of wrong for a number somebody is about to cite.
    assert_eq!(
        one(&build(b"mntr", b"RGB ", b"XYZ ", 0x0210_0000, &[])).version,
        (2, 1)
    );
    assert_eq!(
        one(&build(b"mntr", b"RGB ", b"XYZ ", 0x0430_0000, &[])).version,
        (4, 3)
    );
}

#[test]
fn a_lut_tag_beats_a_matrix_when_both_are_present() {
    // §8.3 makes A2B0/B2A0 the general path and the matrix/TRC form a
    // special case available only to three-component XYZ-PCS profiles. A
    // profile carrying both is USING the tables, so reporting it as
    // matrix+curv would misattribute every CMYK profile that also happens to
    // carry a matrix.
    let p = build(
        b"prtr",
        b"CMYK",
        b"Lab ",
        0x0210_0000,
        &[
            (*b"A2B0", mft2(4, 3, 9)),
            (*b"rXYZ", vec![0; 20]),
            (*b"gXYZ", vec![0; 20]),
            (*b"bXYZ", vec![0; 20]),
        ],
    );
    let got = one(&p);
    assert_eq!(got.transform, TransformShape::Mft2);
    assert_eq!(got.clut_grid, Some(9));
}

#[test]
fn a_monochrome_profile_reports_ktrc_only_and_no_grid() {
    // The shape the requester called out by name: "structurally degenerate
    // […] a shape a CMM will get wrong if it has only ever seen CLUT
    // profiles". It must not fall into `Other`, or the census would report
    // the very population they asked about as unclassified.
    let p = build(
        b"mntr",
        b"GRAY",
        b"XYZ ",
        0x0210_0000,
        &[(*b"kTRC", vec![0; 12])],
    );
    let got = one(&p);
    assert_eq!(got.transform, TransformShape::KTrcOnly);
    assert_eq!(got.clut_grid, None);
    assert_eq!(got.header_channels, Some(1));
}

/// The requester's third ask — **all three outcomes**, not just the
/// interesting one.
///
/// Asserting only the disagreement would leave "100 % agree" as a result
/// equally consistent with the check never having run. That ambiguity is not
/// hypothetical: the first corpus run reported exactly that, and
/// `channel_comparable` exists because the number could not be trusted
/// without it.
#[test]
fn a_channel_disagreement_is_detected_and_agreement_is_distinguishable_from_silence() {
    let bad = build(
        b"prtr",
        b"CMYK",
        b"Lab ",
        0x0210_0000,
        &[(*b"A2B0", mft2(3, 3, 9))],
    );
    let got = one(&bad);
    assert_eq!(got.channel_disagreement, Some((4, 3)));
    assert!(got.channel_comparable);

    let good = build(
        b"prtr",
        b"CMYK",
        b"Lab ",
        0x0210_0000,
        &[(*b"A2B0", mft2(4, 3, 9))],
    );
    let got = one(&good);
    assert_eq!(got.channel_disagreement, None);
    assert!(
        got.channel_comparable,
        "agreement must be distinguishable from having nothing to compare"
    );

    // The third state, which is 94.5 % of the real corpus.
    let none = build(
        b"mntr",
        b"GRAY",
        b"XYZ ",
        0x0210_0000,
        &[(*b"kTRC", vec![0; 12])],
    );
    let got = one(&none);
    assert_eq!(got.channel_disagreement, None);
    assert!(!got.channel_comparable);
}

#[test]
fn b2a0_is_checked_on_its_output_channels_not_its_input() {
    // B2A0 runs PCS → device, so it is the OUTPUT count that must equal the
    // data space's. Checking byte 8 for both directions would report every
    // conformant CMYK B2A0 as a 3-vs-4 disagreement — a false positive on
    // the exact axis being measured, and one that would have produced a
    // confident wrong answer to the request.
    let p = build(
        b"prtr",
        b"CMYK",
        b"Lab ",
        0x0210_0000,
        &[(*b"B2A0", mft2(3, 4, 9))],
    );
    let got = one(&p);
    assert_eq!(
        got.channel_disagreement, None,
        "3 in, 4 out is CORRECT for B2A0"
    );
    assert!(got.channel_comparable);
}

#[test]
fn a_v2_desc_string_is_read() {
    let mut desc = b"desc".to_vec();
    desc.extend_from_slice(&[0; 4]);
    desc.extend_from_slice(&12u32.to_be_bytes());
    desc.extend_from_slice(b"Adobe RGB\0\0\0");
    let p = build(b"mntr", b"RGB ", b"XYZ ", 0x0210_0000, &[(*b"desc", desc)]);
    assert_eq!(one(&p).desc.as_deref(), Some("Adobe RGB"));
}

#[test]
fn an_unknown_data_space_reports_no_channel_count_rather_than_guessing() {
    // The real corpus contains `LAB ` and `YYY `, neither of which is a
    // conformant signature. Guessing 3 for them would fold junk into the
    // 3-channel bucket — which is the bucket the requester's constant is
    // about.
    assert_eq!(
        one(&build(b"mntr", b"YYY ", b"XYZ ", 0x0210_0000, &[])).header_channels,
        None
    );
}

#[test]
fn the_pdf_n_is_carried_through_and_never_derived_from_the_header() {
    // Axis 1 is read from the PROFILE; the PDF's `/N` is carried separately.
    // If either were derived from the other, the `/N`-versus-header
    // comparison would be a tautology that could never report a
    // disagreement — and it reports two.
    let p = build(b"prtr", b"CMYK", b"Lab ", 0x0210_0000, &[]);
    let got = parse(&p, Some(3)).expect("parses");
    assert_eq!(got.header_channels, Some(4));
    assert_eq!(got.pdf_n, Some(3));
}
