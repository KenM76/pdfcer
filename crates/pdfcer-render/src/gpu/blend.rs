//! Separable blend modes as `tiny-skia` 0.11's low-precision pipeline
//! computes them: `u16` lanes, wrapping, `div255(v) = (v + 255) >> 8`.
//! A solid paint under one of these modes runs `blend` then `lerp_1_float`
//! by its coverage (`blitter.rs`; none of these modes pre-scales).
//! [`super::glsl`] repeats each formula.

use tiny_skia::BlendMode;

use super::scene::GpuRefusal;

/// The blend code a fill header carries (lane 3 of its origin texel):
/// 0 for source-over, else the mode `blend` evaluates.
///
/// # Errors
///
/// [`GpuRefusal::Transparency`] for a mode the low-precision pipeline does
/// not evaluate (colour dodge and burn, soft light: high precision) or that
/// no PDF blend mode maps to.
pub(crate) fn code(mode: BlendMode) -> Result<i32, GpuRefusal> {
    Ok(match mode {
        BlendMode::SourceOver => 0,
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Overlay => 3,
        BlendMode::Darken => 4,
        BlendMode::Lighten => 5,
        BlendMode::HardLight => 6,
        BlendMode::Difference => 7,
        BlendMode::Exclusion => 8,
        _ => return Err(GpuRefusal::Transparency),
    })
}

fn w(v: i32) -> i32 {
    v & 0xFFFF
}
fn add(a: i32, b: i32) -> i32 {
    w(a + b)
}
fn sub(a: i32, b: i32) -> i32 {
    w(a - b)
}
fn mul(a: i32, b: i32) -> i32 {
    w(a * b)
}
pub(crate) fn div255(v: i32) -> i32 {
    add(v, 255) >> 8
}
fn inv(v: i32) -> i32 {
    sub(255, v)
}

/// Blend `code`'s colour channel of source `s` over destination `d`.
fn channel(code: i32, (s, sa): (i32, i32), (d, da): (i32, i32)) -> i32 {
    let light = |c: bool| {
        let v = if c {
            mul(mul(2, s), d)
        } else {
            sub(mul(sa, da), mul(mul(2, sub(sa, s)), sub(da, d)))
        };
        div255(add(add(mul(s, inv(da)), mul(d, inv(sa))), v))
    };
    let (sda, dsa) = (mul(s, da), mul(d, sa));
    match code {
        1 => div255(add(add(mul(s, inv(da)), mul(d, inv(sa))), mul(s, d))),
        2 => sub(add(s, d), div255(mul(s, d))),
        3 => light(add(d, d) <= da),
        4 => sub(add(s, d), div255(sda.max(dsa))),
        5 => sub(add(s, d), div255(sda.min(dsa))),
        6 => light(add(s, s) <= sa),
        7 => sub(add(s, d), mul(2, div255(sda.min(dsa)))),
        _ => sub(add(s, d), mul(2, div255(mul(s, d)))),
    }
}

/// `code`'s blend of source `s` over destination `d`, then the lerp by
/// coverage `cov` (none at full coverage) and `store_8888`'s truncation.
pub(crate) fn pixel(code: i32, s: [i32; 4], d: [i32; 4], cov: i32) -> [i32; 4] {
    let (sa, da) = (s[3], d[3]);
    let mut r = [0; 4];
    for ((o, sv), dv) in r.iter_mut().zip(s).zip(d).take(3) {
        *o = channel(code, (sv, sa), (dv, da));
    }
    // Multiply and screen apply their formula to alpha; the rest source-over.
    r[3] = match code {
        1 | 2 => channel(code, (sa, sa), (da, da)),
        _ => add(sa, div255(mul(da, inv(sa)))),
    };
    if cov < 255 {
        for (o, dv) in r.iter_mut().zip(d) {
            *o = div255(add(mul(dv, inv(cov)), mul(*o, cov)));
        }
    }
    r.map(|v| v & 255)
}
