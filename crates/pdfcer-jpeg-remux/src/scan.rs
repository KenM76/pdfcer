//! Sequential Huffman scan decoding (T.81 F.2.2) and the one-scan
//! interleaved re-encode (T.81 F.1.2, A.2.3).

use crate::RemuxError;
use crate::bits::{BitReader, BitWriter};
use crate::frame::{Component, Parsed};
use crate::huffman::{Decoder, Encoder, annex_k};

/// A scan component: its frame index and resolved tables.
pub(crate) struct ScanComp {
    pub(crate) index: usize,
    pub(crate) dc: Decoder,
    pub(crate) ac: Decoder,
}

/// Decodes one scan's entropy-coded data into `frame`'s coefficient store.
///
/// A one-component scan codes that component's own block extent in raster
/// order (A.2.2); a multi-component scan codes whole MCUs (A.2.3). `restart`
/// is the DRI interval in MCUs, 0 for none.
pub(crate) fn decode(
    ecs: &[u8],
    comps: &[ScanComp],
    restart: usize,
    frame: &mut Parsed<'_>,
) -> Result<(), RemuxError> {
    for sc in comps {
        if frame.comps[sc.index].filled.is_some() {
            return Err(RemuxError::Malformed("a component is coded twice"));
        }
    }
    let mut r = BitReader::new(ecs);
    let mut pred = vec![0i32; comps.len()];
    let (mcus, mcux) = match comps {
        [one] => {
            let c = &frame.comps[one.index];
            (c.own_w * c.own_h, c.own_w)
        }
        _ => (frame.mcux * frame.mcuy, frame.mcux),
    };
    for n in 0..mcus {
        if restart > 0 && n > 0 && n % restart == 0 {
            r.restart()?;
            pred.iter_mut().for_each(|p| *p = 0);
        }
        let (mx, my) = (n % mcux, n / mcux);
        for (sc, p) in comps.iter().zip(&mut pred) {
            let c = &mut frame.comps[sc.index];
            if comps.len() == 1 {
                let at = my * c.grid_w + mx;
                decode_block(&mut r, sc, p, &mut c.blocks[at])?;
                continue;
            }
            for by in 0..c.v {
                for bx in 0..c.h {
                    let at = (my * c.v + by) * c.grid_w + mx * c.h + bx;
                    decode_block(&mut r, sc, p, &mut c.blocks[at])?;
                }
            }
        }
    }
    for sc in comps {
        let c = &mut frame.comps[sc.index];
        c.filled = Some(match comps {
            [_] => (c.own_w, c.own_h),
            _ => (c.grid_w, c.grid_h),
        });
    }
    Ok(())
}

/// T.81 F.2.2.1 and F.2.2.2: one block, coefficients kept in zig-zag order.
fn decode_block(
    r: &mut BitReader<'_>,
    sc: &ScanComp,
    pred: &mut i32,
    out: &mut [i16; 64],
) -> Result<(), RemuxError> {
    let t = u32::from(sc.dc.decode(r)?);
    if t > 11 {
        return Err(RemuxError::Malformed("DC difference category over 11"));
    }
    *pred += extend(r.bits(t), t);
    out[0] = i16::try_from(*pred).map_err(|_| RemuxError::Malformed("DC value overflows"))?;
    let mut k = 1;
    while k < 64 {
        let rs = sc.ac.decode(r)?;
        let (run, s) = (usize::from(rs >> 4), u32::from(rs & 0x0F));
        if s == 0 {
            if run != 15 {
                break;
            }
            k += 16;
            continue;
        }
        k += run;
        if k > 63 {
            return Err(RemuxError::Malformed("AC run passes coefficient 63"));
        }
        // s <= 15, so the value fits i16.
        out[k] = extend(r.bits(s), s) as i16;
        k += 1;
    }
    Ok(())
}

/// T.81 F.2.2.1 `EXTEND`.
fn extend(v: u32, t: u32) -> i32 {
    if t == 0 {
        0
    } else if v < 1 << (t - 1) {
        v as i32 - (1 << t) + 1
    } else {
        v as i32
    }
}

/// Writes the parsed stream back out as one interleaved scan.
pub(crate) fn write(p: &Parsed<'_>) -> Result<Vec<u8>, RemuxError> {
    let mut out = vec![0xFF, 0xD8];
    for seg in &p.kept {
        out.extend_from_slice(seg);
    }
    let tables = annex_k();
    let body: Vec<u8> = tables
        .iter()
        .flat_map(|(class, dest, s)| {
            std::iter::once(class << 4 | dest)
                .chain(s.bits)
                .chain(s.vals.iter().copied())
        })
        .collect();
    push_segment(&mut out, 0xC4, &body);
    let mut sos = vec![p.comps.len() as u8];
    for (i, c) in p.comps.iter().enumerate() {
        sos.extend([c.id, if i == 0 { 0x00 } else { 0x11 }]);
    }
    sos.extend([0, 63, 0]);
    push_segment(&mut out, 0xDA, &sos);
    let [dc_l, dc_c, ac_l, ac_c] = tables.map(|(_, _, s)| Encoder::new(&s));
    let mut w = BitWriter::default();
    let mut pred = vec![0i32; p.comps.len()];
    for my in 0..p.mcuy {
        for mx in 0..p.mcux {
            for (i, (c, pr)) in p.comps.iter().zip(&mut pred).enumerate() {
                let (dc, ac) = if i == 0 {
                    (&dc_l, &ac_l)
                } else {
                    (&dc_c, &ac_c)
                };
                for at in mcu_blocks(c, mx, my) {
                    encode_block(&mut w, dc, ac, pr, &c.blocks[at])?;
                }
            }
        }
    }
    out.extend(w.finish());
    out.extend([0xFF, 0xD9]);
    Ok(out)
}

pub(crate) fn push_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend([0xFF, marker]);
    out.extend(((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

/// Block indices of `c` inside MCU (`mx`, `my`), in coding order.
fn mcu_blocks(c: &Component, mx: usize, my: usize) -> impl Iterator<Item = usize> + '_ {
    (0..c.v).flat_map(move |by| (0..c.h).map(move |bx| (my * c.v + by) * c.grid_w + mx * c.h + bx))
}

/// Size category of `v` (T.81 F.1.2.1.1): the bit length of `|v|`.
fn category(v: i32) -> u32 {
    32 - v.unsigned_abs().leading_zeros()
}

/// Appends `v`'s additional bits: `v` itself if positive, `v - 1`'s low
/// bits if negative (T.81 F.1.2.1.1).
fn put_value(w: &mut BitWriter, v: i32, size: u32) {
    let bits = if v < 0 { v - 1 } else { v };
    w.put(bits as u32, size);
}

/// T.81 F.1.2.1 and F.1.2.2 for one block.
pub(crate) fn encode_block(
    w: &mut BitWriter,
    dc: &Encoder,
    ac: &Encoder,
    pred: &mut i32,
    blk: &[i16; 64],
) -> Result<(), RemuxError> {
    let diff = i32::from(blk[0]) - *pred;
    *pred = i32::from(blk[0]);
    let t = category(diff);
    if t > 11 {
        return Err(RemuxError::Malformed("DC difference too large for 8-bit"));
    }
    dc.put(w, t as u8)?;
    put_value(w, diff, t);
    let mut run = 0u8;
    for &c in &blk[1..] {
        if c == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            ac.put(w, 0xF0)?;
            run -= 16;
        }
        let s = category(i32::from(c));
        if s > 10 {
            return Err(RemuxError::Malformed("AC coefficient too large for 8-bit"));
        }
        ac.put(w, run << 4 | s as u8)?;
        put_value(w, i32::from(c), s);
        run = 0;
    }
    if run > 0 {
        ac.put(w, 0x00)?;
    }
    Ok(())
}
