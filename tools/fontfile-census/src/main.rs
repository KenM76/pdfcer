//! # `fontfile-census` — sizing the FF-C donor ceiling from evidence
//!
//! Pass 21.0 needs a `MAX_FONT_PROGRAM_BYTES` guard on the donor face it
//! subsets (`ARCHITECTURE.md` §10.1, decision 021 §3.5). The number has to
//! come from somewhere, and decision 021 is explicit that it must not come
//! from intuition: this project has three recorded cases of a guard chosen
//! that way being wrong once it met real files — `MAX_TOKEN_LEN` at 8 KiB,
//! `MAX_XOBJECT_DEPTH` at 16, and `jpx::MAX_TILES`.
//!
//! A ceiling set too low refuses documents people actually have. Set too
//! high it is decoration. So this walks the corpus and reports what embedded
//! font programs actually weigh.
//!
//! ## What it measures, and the one thing it deliberately does not
//!
//! Every `/FontFile`, `/FontFile2` and `/FontFile3` stream reachable from
//! any object in the file, reported by **decoded** length — because that is
//! what a subsetter would be handed, and it is the number the guard has to
//! be expressed in. Raw (still-compressed) length is reported alongside so
//! the compression ratio is visible; a guard written against raw length
//! would be trivially bypassable by anyone who could pick the filter.
//!
//! It does **not** measure supplied donor faces from an operator's font
//! folder, which is the *other* input FF-C accepts and the one that can be
//! genuinely enormous — a CJK `.ttc` collection runs to tens or hundreds of
//! megabytes. Those are not in any corpus and cannot be. This census bounds
//! one half of the question; the other half needs a separately-argued
//! headroom figure, and pretending otherwise would be the more dangerous
//! error, so it is stated here rather than left for someone to assume.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--tsv <path>] <corpus-dir> [more-dirs ...]
//! ```
//!
//! Exit codes: `0` the sweep completed (whatever the numbers were), `2`
//! usage error, `3` a corpus directory could not be walked. A census has no
//! notion of "failing" — an unopenable file is data about the corpus, not a
//! gate breach.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::filters;
use pdfcer_core::object::{Object, Stream};

/// One embedded font program.
struct Sample {
    file: String,
    obj: u32,
    /// Which `/FontFile*` key carried it.
    key: &'static str,
    raw_len: usize,
    /// `None` when the stream would not decode — itself a finding.
    decoded_len: Option<usize>,
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut tsv: Option<PathBuf> = None;
    let mut dirs: Vec<PathBuf> = Vec::new();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tsv" => match it.next() {
                Some(p) => tsv = Some(PathBuf::from(p)),
                None => {
                    eprintln!("--tsv needs a path");
                    return std::process::ExitCode::from(2);
                }
            },
            other => dirs.push(PathBuf::from(other)),
        }
    }
    if dirs.is_empty() {
        eprintln!(
            "usage: fontfile-census [--tsv <path>] <corpus-dir> [more-dirs ...]\n\
             Measures decoded /FontFile* sizes so Pass 21.0's donor ceiling is evidence-based."
        );
        return std::process::ExitCode::from(2);
    }

    let mut pdfs: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        if let Err(e) = walk(d, &mut pdfs) {
            eprintln!("cannot walk {}: {e}", d.display());
            return std::process::ExitCode::from(3);
        }
    }
    pdfs.sort();
    eprintln!("scanning {} file(s)…", pdfs.len());

    let mut samples: Vec<Sample> = Vec::new();
    let mut unreadable = 0usize;
    let mut with_embedded = 0usize;

    for (n, path) in pdfs.iter().enumerate() {
        if n % 500 == 0 && n > 0 {
            eprintln!("  {n}/{}…", pdfs.len());
        }
        let Ok(bytes) = fs::read(path) else {
            unreadable += 1;
            continue;
        };
        // A file the parser refuses is data about the corpus, not an error.
        let Ok(doc) = Document::from_bytes(bytes) else {
            unreadable += 1;
            continue;
        };
        let before = samples.len();
        collect(&doc, &path.display().to_string(), &mut samples);
        if samples.len() > before {
            with_embedded += 1;
        }
    }

    report(&samples, pdfs.len(), unreadable, with_embedded);

    if let Some(p) = tsv {
        if let Err(e) = write_tsv(&p, &samples) {
            eprintln!("could not write {}: {e}", p.display());
        } else {
            eprintln!("per-font rows -> {}", p.display());
        }
    }
    std::process::ExitCode::SUCCESS
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            // A corpus directory that cannot be descended is skipped rather
            // than fatal — permission oddities inside a scratch tree should
            // not throw away a 4,000-file sweep.
            let _ = walk(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
    Ok(())
}

/// Pull every `/FontFile*` stream out of one document.
///
/// Walks ALL objects rather than descending the page tree to font resources.
/// That is deliberate: a font program referenced only from an unreferenced
/// or damaged descriptor still tells us what real producers emit, and the
/// question here is "how big do these get", not "how big are the ones a
/// renderer would reach".
fn collect(doc: &Document, file: &str, out: &mut Vec<Sample>) {
    for io_ in doc.objects() {
        let Object::Dict(d) = &io_.value else {
            continue;
        };
        for key in ["FontFile", "FontFile2", "FontFile3"] {
            let Some(v) = d.get(key.as_bytes()) else {
                continue;
            };
            let Object::Reference(id) = v else { continue };
            let Some(target) = doc.get(*id) else { continue };
            let Object::Stream(st) = &target.value else {
                continue;
            };
            let (raw_len, decoded_len) = measure(doc, st);
            out.push(Sample {
                file: file.to_owned(),
                obj: id.num,
                key: match key {
                    "FontFile" => "FontFile",
                    "FontFile2" => "FontFile2",
                    _ => "FontFile3",
                },
                raw_len,
                decoded_len,
            });
        }
    }
}

fn measure(doc: &Document, st: &Stream) -> (usize, Option<usize>) {
    let Some(raw) = pdfcer_core::writer::serialize::stream_data(st, doc.bytes()) else {
        return (0, None);
    };
    let decoded = filters::decode_stream(&st.dict, raw).ok().map(|v| v.len());
    (raw.len(), decoded)
}

fn report(samples: &[Sample], files: usize, unreadable: usize, with_embedded: usize) {
    println!("=== fontfile-census ===");
    println!("files scanned          : {files}");
    println!("files unreadable       : {unreadable}");
    println!("files with an embedded font program: {with_embedded}");
    println!("font programs found    : {}", samples.len());

    let mut decoded: Vec<usize> = samples.iter().filter_map(|s| s.decoded_len).collect();
    let undecodable = samples.len() - decoded.len();
    println!("undecodable programs   : {undecodable}");

    if decoded.is_empty() {
        println!("\nNo decodable font programs found — no ceiling can be justified from this run.");
        return;
    }
    decoded.sort_unstable();

    let pick = |q: f64| -> usize {
        // `len - 1` is safe: `decoded` is non-empty by the guard above.
        let idx = ((decoded.len() - 1) as f64 * q).round() as usize;
        decoded.get(idx).copied().unwrap_or(0)
    };

    println!("\ndecoded size distribution (bytes):");
    println!("  min   {:>12}", decoded.first().copied().unwrap_or(0));
    println!("  p50   {:>12}", pick(0.50));
    println!("  p90   {:>12}", pick(0.90));
    println!("  p99   {:>12}", pick(0.99));
    println!("  p99.9 {:>12}", pick(0.999));
    println!("  max   {:>12}", decoded.last().copied().unwrap_or(0));

    let mut by_key: BTreeMap<&str, usize> = BTreeMap::new();
    for s in samples {
        *by_key.entry(s.key).or_default() += 1;
    }
    println!("\nby descriptor key:");
    for (k, n) in &by_key {
        println!("  {k:<10} {n}");
    }

    // What a candidate ceiling would REFUSE. The useful output is not the
    // max — it is how many real files each candidate would break, because
    // that is the cost side of the tradeoff and the part an argument from
    // intuition always omits.
    println!(
        "\nwhat a candidate ceiling would refuse (of {} programs):",
        decoded.len()
    );
    for mb in [1u64, 2, 4, 8, 16, 32, 64] {
        let limit = (mb * 1024 * 1024) as usize;
        let over = decoded.iter().filter(|d| **d > limit).count();
        let pct = over as f64 * 100.0 / decoded.len() as f64;
        println!("  {mb:>3} MiB  ->  {over:>6} refused  ({pct:.4}%)");
    }

    println!(
        "\nNOTE: this bounds only font programs ALREADY EMBEDDED in PDFs. FF-C's other input is\n\
         an operator-supplied donor from a font folder, which is not represented in any corpus\n\
         and can be far larger (a CJK .ttc runs to tens or hundreds of MB). The ceiling chosen\n\
         must argue that headroom separately — this census cannot supply it."
    );
}

fn write_tsv(path: &Path, samples: &[Sample]) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    writeln!(f, "file\tobject\tkey\traw_len\tdecoded_len")?;
    for s in samples {
        let d = s
            .decoded_len
            .map_or_else(|| "UNDECODABLE".to_owned(), |v| v.to_string());
        writeln!(f, "{}\t{}\t{}\t{}\t{}", s.file, s.obj, s.key, s.raw_len, d)?;
    }
    Ok(())
}
