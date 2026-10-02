//! Test stand-in for an OCR program: speaks the Tesseract protocol and
//! answers with words naming what it was run as. Word 1 is its own file
//! stem, word 2 the `--tessdata-dir` folder name, word 3 the `-l` value,
//! word 4 `pgm` when stdin held a binary PGM.

use std::io::Read as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value_after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_default()
    };
    let tessdata = value_after("--tessdata-dir");
    let data_name = std::path::Path::new(&tessdata)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let langs = value_after("-l");
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    let stem = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let image = if input.starts_with(b"P5\n") {
        "pgm"
    } else {
        "other"
    };
    println!(
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext"
    );
    for (i, word) in [stem, data_name, langs, image.to_owned()]
        .iter()
        .enumerate()
    {
        println!("5\t1\t1\t1\t1\t{}\t{}\t0\t8\t8\t90\t{word}", i + 1, i * 10);
    }
}
