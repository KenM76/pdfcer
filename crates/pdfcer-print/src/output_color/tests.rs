use super::*;

#[test]
fn color_leaves_pixels_alone() {
    let mut px = vec![200, 10, 30, 255];
    assert_eq!(apply_output_color(&mut px, OutputColor::Color), 0);
    assert_eq!(px, [200, 10, 30, 255]);
}

#[test]
fn grayscale_uses_bt709_luma() {
    let mut px = vec![255, 0, 0, 255, 0, 255, 0, 255, 255, 255, 255, 255];
    apply_output_color(&mut px, OutputColor::Grayscale);
    assert_eq!(
        px,
        [54, 54, 54, 255, 182, 182, 182, 255, 255, 255, 255, 255]
    );
}

#[test]
fn grayscale_stays_premultiplied() {
    let mut px = vec![128, 128, 128, 128];
    apply_output_color(&mut px, OutputColor::Grayscale);
    assert_eq!(px, [128, 128, 128, 128]);
}

#[test]
fn black_only_blackens_colour_and_keeps_white() {
    let mut px = vec![
        255, 255, 0, 255, 255, 255, 255, 255, 252, 252, 252, 255, 0, 0, 0, 0,
    ];
    let n = apply_output_color(&mut px, OutputColor::BlackOnly);
    assert_eq!(n, 1);
    // Near-white (252) counts as white; fully transparent stays.
    assert_eq!(
        px,
        [
            0, 0, 0, 255, 255, 255, 255, 255, 252, 252, 252, 255, 0, 0, 0, 0
        ]
    );
}

#[test]
fn black_only_keeps_coverage() {
    let mut px = vec![60, 20, 20, 100];
    apply_output_color(&mut px, OutputColor::BlackOnly);
    assert_eq!(px, [0, 0, 0, 100]);
}
