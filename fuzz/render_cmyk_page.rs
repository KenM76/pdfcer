//! The fixed page the `render_cmyk_page` fuzz target renders: its content
//! stream is the fuzz input, everything else is constant.
//!
//! The page declares a `/DeviceCMYK` transparency group (ISO 32000-1
//! §11.6.6, Table 147), which routes it through the renderer's subtractive
//! colorant buffer. Its resources offer every construct that buffer
//! composites, so a content stream only has to NAME them:
//!
//! | name | construct |
//! |---|---|
//! | `/Gi` `/Gn` `/Gk` | isolated, non-isolated and knockout group XObjects |
//! | `/OP` `/M` `/L` `/SM` | overprint (`/OPM 1`), Multiply + alpha, Luminosity, a luminosity soft mask |
//! | `/Sp` `/DN` `/Ix` | a Separation, a two-colorant DeviceN and an Indexed-over-CMYK colour space |
//! | `/Sh` | an axial shading in the DeviceN space |
//! | `/Im` `/Ii` | a DeviceCMYK image and an Indexed-over-CMYK image |
//!
//! Shared by the fuzz target and by
//! `crates/pdfcer-render/tests/fuzz_cmyk_page_reaches_the_buffer.rs`, which
//! keeps the target from silently falling off the colorant buffer.

/// A 48x48 pt one-page PDF whose content stream is `content`, verbatim.
pub fn page_pdf(content: &[u8]) -> Vec<u8> {
    let group_body = b"/OP gs 0.8 0 0 0 k 4 4 24 24 re f 0 0.7 0 0 k 12 12 24 24 re f \
/M gs /Sp cs 0.9 scn 20 20 20 20 re f"
        .as_slice();
    let group = |attrs: &str| {
        stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 48 48] /Resources 4 0 R /Group << /S /Transparency {attrs} >>"
            ),
            group_body,
        )
    };
    let image_cmyk: Vec<u8> = [255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255].to_vec();
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 48 48] /Resources 4 0 R /Contents 5 0 R \
/Group << /S /Transparency /CS /DeviceCMYK /I true >> >>"
            .to_vec(),
        b"<< /ExtGState << /OP << /OP true /op true /OPM 1 >> /M << /BM /Multiply /ca 0.6 /CA 0.6 >> \
/L << /BM /Luminosity >> /SM << /SMask << /S /Luminosity /G 9 0 R >> >> >> \
/ColorSpace << /Sp [/Separation /Spot1 /DeviceCMYK 10 0 R] \
/DN [/DeviceN [/Spot1 /Spot2] /DeviceCMYK 11 0 R] /Ix [/Indexed /DeviceCMYK 1 <00FF000000FF0000>] >> \
/Shading << /Sh << /ShadingType 2 /ColorSpace [/DeviceN [/Spot1 /Spot2] /DeviceCMYK 11 0 R] \
/Coords [0 0 48 48] /Function 12 0 R /Extend [true true] >> >> \
/XObject << /Gi 6 0 R /Gn 7 0 R /Gk 8 0 R /Im 13 0 R /Ii 14 0 R >> >>"
            .to_vec(),
        stream("", content),
        group("/I true /CS /DeviceCMYK"),
        group(""),
        group("/I true /K true"),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 48 48] /Group << /S /Transparency /CS /DeviceGray >>",
            b"0.5 g 0 0 48 24 re f",
        ),
        b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0.1 0.9 0 0.1] /N 1 >>".to_vec(),
        stream(
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            b"{ 0 0 }",
        ),
        b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0] /C1 [1 0.5] /N 1 >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceCMYK /BitsPerComponent 8",
            &image_cmyk,
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
/ColorSpace [/Indexed /DeviceCMYK 1 <00FF000000FF0000>] /BitsPerComponent 8",
            &[0, 1, 1, 0],
        ),
    ];

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn stream(extra: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {extra} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}
