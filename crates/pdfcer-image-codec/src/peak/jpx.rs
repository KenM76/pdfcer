//! JPX (T.800) peak, measured on hayro-jpeg2000: an f32 coefficient plane
//! per component, an f32 interleaved buffer over all components, the
//! inverse-wavelet scratch planes, then pdfcer's 8-bit samples. Field
//! layout: `PDF_Spec/filters/filter__jpx_codestream_main_header.md`.

use super::{be, len};

/// Working bytes above `data` for a JPX decode: a raw codestream, or a JP2
/// file whose `jp2c` box holds one. `None` if no SIZ marker is found.
pub(super) fn working_bytes(data: &[u8]) -> Option<u64> {
    let siz = codestream(data).and_then(|cs| cs.strip_prefix(&[0xFF, 0x4F, 0xFF, 0x51]))?;
    let (width, height, components) = size(siz)?;
    let pixels = width.saturating_mul(height);
    let planes: u64 = components
        .iter()
        .map(|&(w, h)| w.saturating_mul(h).saturating_mul(4))
        .sum();
    let nc = len(&components);
    let interleaved = pixels.saturating_mul(nc).saturating_mul(4);
    // Two wavelet scratch planes, 4 + 1 bytes per padded sample.
    let scratch = (width + 2).saturating_mul(height + 2).saturating_mul(5);
    // Code-block and packet bookkeeping, measured at under (nc + 1) / 8
    // bytes per pixel; a quarter keeps the doubling steps inside the bound.
    let bookkeeping = pixels.saturating_mul(nc + 1) / 4;
    let output = pixels.saturating_mul(nc);
    Some(
        planes
            .saturating_add(interleaved)
            .saturating_add(scratch)
            .saturating_add(bookkeeping)
            .saturating_add(output),
    )
}

/// The codestream: `data` itself when it starts with SOC, else the
/// contents of the first top-level `jp2c` box (T.800 Annex I.4).
fn codestream(data: &[u8]) -> Option<&[u8]> {
    if data.starts_with(&[0xFF, 0x4F]) {
        return Some(data);
    }
    let mut at = 0usize;
    while at < data.len() {
        let lbox = be::<4>(data, at)?;
        let kind = data.get(at + 4..at + 8)?;
        let (header, total) = match lbox {
            0 => (8, len(data) - u64::try_from(at).ok()?),
            1 => (16, be::<8>(data, at + 8)?),
            n => (8, n),
        };
        let end = at.checked_add(usize::try_from(total).ok()?)?;
        if kind == b"jp2c" {
            return data.get(at + header..end.min(data.len()));
        }
        if total < header as u64 {
            return None;
        }
        at = end;
    }
    None
}

/// A component's `(width, height)` in samples.
type Extent = (u64, u64);

/// SIZ (T.800 A.5.1, after its marker): Lsiz, Rsiz, then Xsiz, Ysiz,
/// XOsiz, YOsiz, four tile fields, Csiz, and (Ssiz, XRsiz, YRsiz) per
/// component. Image extent is Xsiz - XOsiz; component c spans
/// ceil(Xsiz / XRsiz) - ceil(XOsiz / XRsiz) samples (B.2).
fn size(p: &[u8]) -> Option<(u64, u64, Vec<Extent>)> {
    let (x, y, x0, y0) = (
        be::<4>(p, 4)?,
        be::<4>(p, 8)?,
        be::<4>(p, 12)?,
        be::<4>(p, 16)?,
    );
    let count = usize::try_from(be::<2>(p, 36)?).ok()?;
    let specs = p.get(38..38 + 3 * count)?;
    let components = specs
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[_, xr, yr]| {
            let (xr, yr) = (u64::from(xr.max(1)), u64::from(yr.max(1)));
            (
                x.div_ceil(xr).saturating_sub(x0.div_ceil(xr)),
                y.div_ceil(yr).saturating_sub(y0.div_ceil(yr)),
            )
        })
        .collect();
    Some((x.saturating_sub(x0), y.saturating_sub(y0), components))
}
