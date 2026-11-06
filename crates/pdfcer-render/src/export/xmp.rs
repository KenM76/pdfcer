//! Photo-sphere metadata: the Google Photo Sphere (GPano) XMP packet that
//! makes a photo viewer open an equirectangular picture as a panorama to
//! look around in, and its placement in a JPEG.
//!
//! Sources: the GPano schema,
//! <https://developers.google.com/streetview/spherical-metadata>; the XMP
//! Specification Part 3 §1.1.3 for the APP1 segment (`FF E1`, a two-byte length counting
//! itself, the 29-byte namespace `http://ns.adobe.com/xap/1.0/` and its NUL,
//! then the packet, at most 65 502 bytes) and the packet wrapper.
//!
//! GPano names only the Adobe XMP standard as its carrier, and every
//! example it gives is a JPEG. XMP can ride in a PNG `iTXt` chunk, but the
//! GPano page does not say photo-sphere viewers read it there, so this
//! module offers no PNG route.

use super::ExportError;
use std::fmt::Write as _;

/// The APP1 namespace that marks an XMP segment, with its NUL.
const XMP_SIGNATURE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// The APP1 namespace that marks an Exif segment.
const EXIF_SIGNATURE: &[u8] = b"Exif\0";
/// The largest packet one APP1 segment carries, XMP Part 3 §1.1.3.1.
pub const MAX_JPEG_XMP_PACKET: usize = 65_502;

/// The GPano XMP packet for a full-sphere equirectangular picture
/// `width` × `height` pixels: projection `equirectangular`, the cropped
/// area the whole picture (left 0, top 0, cropped size = full size = the
/// picture's size, which is what viewers check against the image), and
/// `UsePanoramaViewer` `True`. Pose and initial-view fields are left out:
/// the picture is not tied to a compass, and a viewer opens on the
/// picture's centre without them.
///
/// The picture should be twice as wide as it is tall; the packet states
/// the size it is given either way.
///
/// # Example
///
/// ```
/// use pdfcer_render::export::photo_sphere_xmp;
/// let xmp = photo_sphere_xmp(2048, 1024);
/// assert!(xmp.contains("<GPano:ProjectionType>equirectangular</GPano:ProjectionType>"));
/// assert!(xmp.contains("<GPano:FullPanoWidthPixels>2048</GPano:FullPanoWidthPixels>"));
/// ```
pub fn photo_sphere_xmp(width: u32, height: u32) -> String {
    let fields = [
        ("UsePanoramaViewer", "True".to_owned()),
        ("ProjectionType", "equirectangular".to_owned()),
        ("CroppedAreaImageWidthPixels", width.to_string()),
        ("CroppedAreaImageHeightPixels", height.to_string()),
        ("FullPanoWidthPixels", width.to_string()),
        ("FullPanoHeightPixels", height.to_string()),
        ("CroppedAreaLeftPixels", "0".to_owned()),
        ("CroppedAreaTopPixels", "0".to_owned()),
        ("StitchingSoftware", "pdfcer".to_owned()),
    ];
    let mut body = String::new();
    for (name, value) in fields {
        // Writing to a String cannot fail.
        let _ = writeln!(body, "   <GPano:{name}>{value}</GPano:{name}>");
    }
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
         <rdf:Description rdf:about=\"\" \
         xmlns:GPano=\"http://ns.google.com/photos/1.0/panorama/\">\n\
         {body}  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

/// `jpeg` with `xmp` added as an XMP APP1 segment, after the SOI marker,
/// any APP0 (JFIF) segments and an Exif APP1 segment, so it comes before
/// every frame and table segment as XMP Part 3 requires. The image data is
/// copied unchanged.
///
/// # Errors
///
/// [`ExportError::Metadata`] when `jpeg` does not start with SOI, a
/// segment before the insertion point is truncated, `jpeg` already carries
/// an XMP segment there, or `xmp` is longer than [`MAX_JPEG_XMP_PACKET`]
/// bytes (one segment's capacity; the extended-XMP split is not written).
///
/// # Example
///
/// ```
/// use pdfcer_render::export::{encode_jpeg, insert_jpeg_xmp, photo_sphere_xmp, JpegOptions};
/// use pdfcer_render::tiny_skia::Pixmap;
///
/// let pixmap = Pixmap::new(16, 8).unwrap();
/// let jpeg = encode_jpeg(&pixmap, &JpegOptions::default())?;
/// let tagged = insert_jpeg_xmp(&jpeg, &photo_sphere_xmp(16, 8))?;
/// assert_eq!(&tagged[..2], &[0xFF, 0xD8]);
/// assert_eq!(tagged.len() - jpeg.len(), 4 + 29 + photo_sphere_xmp(16, 8).len());
/// # Ok::<(), pdfcer_render::export::ExportError>(())
/// ```
pub fn insert_jpeg_xmp(jpeg: &[u8], xmp: &str) -> Result<Vec<u8>, ExportError> {
    let refused = |detail| Err(ExportError::Metadata { detail });
    if xmp.len() > MAX_JPEG_XMP_PACKET {
        return refused("the XMP packet is longer than one JPEG APP1 segment holds");
    }
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return refused("the data does not start with a JPEG SOI marker");
    }
    let at = insertion_point(jpeg)?;
    let length =
        u16::try_from(2 + XMP_SIGNATURE.len() + xmp.len()).map_err(|_| ExportError::Metadata {
            detail: "the XMP packet is longer than one JPEG APP1 segment holds",
        })?;
    let mut out = Vec::with_capacity(jpeg.len() + 4 + XMP_SIGNATURE.len() + xmp.len());
    out.extend_from_slice(jpeg.get(..at).unwrap_or_default());
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(XMP_SIGNATURE);
    out.extend_from_slice(xmp.as_bytes());
    out.extend_from_slice(jpeg.get(at..).unwrap_or_default());
    Ok(out)
}

/// The offset after SOI, the leading APP0 segments and an Exif APP1.
fn insertion_point(jpeg: &[u8]) -> Result<usize, ExportError> {
    let truncated = || ExportError::Metadata {
        detail: "a JPEG segment before the first frame is truncated",
    };
    let mut at = 2;
    loop {
        let Some(&[0xFF, marker]) = jpeg.get(at..at + 2) else {
            return Ok(at);
        };
        if marker != 0xE0 && marker != 0xE1 {
            return Ok(at);
        }
        let len = jpeg
            .get(at + 2..at + 4)
            .and_then(|b| <[u8; 2]>::try_from(b).ok())
            .map(|b| usize::from(u16::from_be_bytes(b)))
            .filter(|&n| n >= 2 && at + 2 + n <= jpeg.len())
            .ok_or_else(truncated)?;
        let payload = jpeg.get(at + 4..at + 2 + len).unwrap_or_default();
        if marker == 0xE1 {
            if payload.starts_with(XMP_SIGNATURE) {
                return Err(ExportError::Metadata {
                    detail: "the JPEG already carries an XMP segment",
                });
            }
            if !payload.starts_with(EXIF_SIGNATURE) {
                return Ok(at);
            }
        }
        at += 2 + len;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::export::{JpegOptions, encode_jpeg};
    use tiny_skia::{Color, Pixmap};

    fn jpeg(width: u32, height: u32, dpi: Option<f32>) -> Vec<u8> {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        pixmap.fill(Color::from_rgba8(200, 40, 10, 255));
        let options = JpegOptions {
            dpi,
            ..JpegOptions::default()
        };
        encode_jpeg(&pixmap, &options).unwrap()
    }

    fn field<'a>(xmp: &'a str, name: &str) -> &'a str {
        let open = format!("<GPano:{name}>");
        let start = xmp.find(&open).unwrap() + open.len();
        let end = xmp[start..].find("</GPano:").unwrap();
        &xmp[start..start + end]
    }

    #[test]
    fn the_packet_states_a_whole_sphere_of_the_pictures_size() {
        let xmp = photo_sphere_xmp(4000, 2000);
        assert_eq!(field(&xmp, "ProjectionType"), "equirectangular");
        assert_eq!(field(&xmp, "UsePanoramaViewer"), "True");
        assert_eq!(field(&xmp, "CroppedAreaImageWidthPixels"), "4000");
        assert_eq!(field(&xmp, "CroppedAreaImageHeightPixels"), "2000");
        assert_eq!(field(&xmp, "FullPanoWidthPixels"), "4000");
        assert_eq!(field(&xmp, "FullPanoHeightPixels"), "2000");
        assert_eq!(field(&xmp, "CroppedAreaLeftPixels"), "0");
        assert_eq!(field(&xmp, "CroppedAreaTopPixels"), "0");
        assert!(xmp.contains("xmlns:GPano=\"http://ns.google.com/photos/1.0/panorama/\""));
        assert!(xmp.starts_with("<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>"));
        assert!(xmp.ends_with("<?xpacket end=\"w\"?>"));
    }

    /// The segment lands after SOI and the JFIF APP0, its length counts
    /// itself, the signature and the packet, and the picture still decodes
    /// to the same pixels.
    #[test]
    fn the_segment_follows_soi_and_app0_and_the_jpeg_still_decodes() {
        let plain = jpeg(32, 16, Some(96.0));
        assert_eq!(&plain[2..4], &[0xFF, 0xE0], "the encoder writes JFIF");
        let app0 = 2 + usize::from(u16::from_be_bytes([plain[4], plain[5]]));
        let xmp = photo_sphere_xmp(32, 16);
        let tagged = insert_jpeg_xmp(&plain, &xmp).unwrap();
        assert_eq!(&tagged[..2 + app0], &plain[..2 + app0]);
        let seg = &tagged[2 + app0..];
        assert_eq!(&seg[..2], &[0xFF, 0xE1]);
        let len = usize::from(u16::from_be_bytes([seg[2], seg[3]]));
        assert_eq!(len, 2 + 29 + xmp.len());
        assert_eq!(&seg[4..33], XMP_SIGNATURE);
        assert_eq!(&seg[33..2 + len], xmp.as_bytes());
        assert_eq!(&seg[2 + len..], &plain[2 + app0..]);
        let decode = |bytes: &[u8]| {
            let cursor = zune_jpeg::zune_core::bytestream::ZCursor::new(bytes);
            let mut decoder = zune_jpeg::JpegDecoder::new(cursor);
            let pixels = decoder.decode().unwrap();
            let info = decoder.info().unwrap();
            (pixels, (info.width, info.height))
        };
        assert_eq!(decode(&tagged), decode(&plain));
        assert_eq!(decode(&tagged).1, (32, 16));
    }

    #[test]
    fn the_segment_follows_an_exif_segment_and_precedes_any_other_app1() {
        let plain = jpeg(8, 8, None);
        let exif: &[u8] = &[0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0];
        let other: &[u8] = &[0xFF, 0xE1, 0x00, 0x04, b'a', b'b'];
        let mut with = plain[..2].to_vec();
        with.extend_from_slice(exif);
        with.extend_from_slice(other);
        with.extend_from_slice(&plain[2..]);
        let tagged = insert_jpeg_xmp(&with, "x").unwrap();
        let at = 2 + exif.len();
        assert_eq!(&tagged[..at], &with[..at]);
        assert_eq!(&tagged[at + 4..at + 33], XMP_SIGNATURE);
        assert_eq!(&tagged[at + 34..], &with[at..]);
    }

    #[test]
    fn a_bad_jpeg_a_second_packet_or_an_oversized_one_is_refused() {
        let plain = jpeg(8, 8, None);
        let refused =
            |r: Result<Vec<u8>, ExportError>| matches!(r, Err(ExportError::Metadata { .. }));
        assert!(refused(insert_jpeg_xmp(b"\x89PNG", "x")));
        assert!(refused(insert_jpeg_xmp(
            &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x40, 1],
            "x"
        )));
        let tagged = insert_jpeg_xmp(&plain, "x").unwrap();
        assert!(refused(insert_jpeg_xmp(&tagged, "x")));
        let big = "x".repeat(MAX_JPEG_XMP_PACKET + 1);
        assert!(refused(insert_jpeg_xmp(&plain, &big)));
        let full = "x".repeat(MAX_JPEG_XMP_PACKET);
        let at = insertion_point(&plain).unwrap();
        let tagged = insert_jpeg_xmp(&plain, &full).unwrap();
        assert_eq!(&tagged[at..at + 4], &[0xFF, 0xE1, 0xFF, 0xFD]);
    }
}
