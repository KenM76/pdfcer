//! A minimal ICC profile encoder: header, tag table and the handful of tag
//! types a matrix/TRC or grey profile needs (ICC.1:2022 §7, §10).
//!
//! It exists for one production reason: icccer builds sRGB only as a
//! *destination*, and a print raster needs sRGB as the *source*. Encoding
//! icccer's own constructed sRGB ([`srgb_source`]) and parsing it back gives
//! icccer a real source profile — correct source white, version and black —
//! without pdfcer doing any colour arithmetic (decision 064). The tests reuse
//! it to author synthetic profiles.

/// ICC `s15Fixed16Number` (§4.6).
fn s15(v: f64) -> [u8; 4] {
    // The values encoded are colorant XYZs and curve parameters, all well
    // inside s15Fixed16's ±32768 range, so the conversion cannot saturate.
    #[allow(clippy::cast_possible_truncation)]
    let fixed = (v * 65536.0).round() as i32;
    fixed.to_be_bytes()
}

/// An `XYZType` tag holding one value (§10.31).
pub(crate) fn xyz_tag(xyz: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for c in xyz {
        t.extend_from_slice(&s15(c));
    }
    t
}

/// A `parametricCurveType` tag (§10.18).
pub(crate) fn para_tag(func_type: u16, params: &[f64]) -> Vec<u8> {
    let mut t = b"para\0\0\0\0".to_vec();
    t.extend_from_slice(&func_type.to_be_bytes());
    t.extend_from_slice(&[0, 0]);
    for p in params {
        t.extend_from_slice(&s15(*p));
    }
    t
}

/// A sampled `curveType` tag (§10.6).
#[cfg(test)]
pub(crate) fn curv_tag(samples: &[u16]) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    let n = u32::try_from(samples.len()).unwrap_or(0);
    t.extend_from_slice(&n.to_be_bytes());
    for s in samples {
        t.extend_from_slice(&s.to_be_bytes());
    }
    t
}

/// A one-record `multiLocalizedUnicodeType` tag, `enUS` (§10.15).
pub(crate) fn mluc_tag(text: &str) -> Vec<u8> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend_from_slice(&1_u32.to_be_bytes());
    t.extend_from_slice(&12_u32.to_be_bytes());
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&u32::try_from(utf16.len()).unwrap_or(0).to_be_bytes());
    t.extend_from_slice(&28_u32.to_be_bytes());
    t.extend_from_slice(&utf16);
    t
}

/// The D50 PCS illuminant (§7.2.16).
pub(crate) const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// Assemble a v4.3 profile: 128-byte header, tag table, 4-aligned tag data.
pub(crate) fn profile(class: &[u8; 4], space: &[u8; 4], tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_len = 4 + 12 * tags.len();
    let mut entries = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    for (sig, data) in tags {
        let off = u32::try_from(128 + table_len + payload.len()).unwrap_or(0);
        entries.extend_from_slice(*sig);
        entries.extend_from_slice(&off.to_be_bytes());
        entries.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        payload.extend_from_slice(data);
        payload.resize(payload.len().next_multiple_of(4), 0);
    }
    let size = u32::try_from(128 + table_len + payload.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(128 + table_len + payload.len());
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"none");
    out.extend_from_slice(&[4, 0x30, 0, 0]);
    out.extend_from_slice(class);
    out.extend_from_slice(space);
    out.extend_from_slice(b"XYZ ");
    out.resize(36, 0);
    out.extend_from_slice(b"acsp");
    out.resize(68, 0);
    for c in D50 {
        out.extend_from_slice(&s15(c));
    }
    out.extend_from_slice(b"pdfc");
    out.resize(128, 0);
    out.extend_from_slice(&u32::try_from(tags.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&entries);
    out.extend_from_slice(&payload);
    out
}

/// A matrix/TRC RGB profile from a device→XYZ matrix (rows = X, Y, Z) and
/// one curve tag shared by the three channels.
pub(crate) fn matrix_trc_rgb(
    class: &[u8; 4],
    desc: &str,
    rows: [[f64; 3]; 3],
    trc: &[u8],
) -> Vec<u8> {
    let [x, y, z] = rows;
    let column = |i: usize| -> [f64; 3] {
        let pick = |row: [f64; 3]| row.get(i).copied().unwrap_or(0.0);
        [pick(x), pick(y), pick(z)]
    };
    profile(
        class,
        b"RGB ",
        &[
            (b"desc", mluc_tag(desc)),
            (b"wtpt", xyz_tag(D50)),
            (b"rXYZ", xyz_tag(column(0))),
            (b"gXYZ", xyz_tag(column(1))),
            (b"bXYZ", xyz_tag(column(2))),
            (b"rTRC", trc.to_vec()),
            (b"gTRC", trc.to_vec()),
            (b"bTRC", trc.to_vec()),
        ],
    )
}

/// icccer's constructed sRGB (`icccer_cmm::builtin::srgb`, its default
/// transfer-function reading), encoded as a display-class profile.
pub(crate) fn srgb_source() -> Vec<u8> {
    let matrix = icccer_cmm::builtin::srgb().matrix();
    let params =
        icccer_cmm::builtin::srgb_trc_params(icccer_cmm::builtin::SrgbTrc::ValueContinuous);
    matrix_trc_rgb(
        b"mntr",
        "sRGB (icccer built-in)",
        matrix.rows,
        &para_tag(3, &params),
    )
}
