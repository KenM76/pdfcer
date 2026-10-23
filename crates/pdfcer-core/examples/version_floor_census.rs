//! Census probe: how far below its declared version could each file go?
//!
//! Runs `optimize::version_floor` over every `.pdf` under the given
//! directories and prints, per file, the declared version, the floor, and
//! whether the floor is proven; then a summary of the declared -> floor
//! pairs and the constructs that most often set a floor.
//!
//! ```text
//! cargo run --release -p pdfcer-core --example version_floor_census -- <dir> [more...]
//! ```
//!
//! It only reads.

use pdfcer_core::document::Document;
use pdfcer_core::optimize::{FloorOptions, IccVersionGate, version_floor, version_floor_with};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for d in std::env::args().skip(1) {
        walk(Path::new(&d), &mut files);
    }
    files.sort();
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut setters: BTreeMap<String, usize> = BTreeMap::new();
    let (mut lowerable, mut unproven, mut unreadable, mut strict_lowerable) = (0, 0, 0, 0);
    for path in &files {
        let Ok(doc) = Document::load(path) else {
            unreadable += 1;
            continue;
        };
        let f = version_floor(&doc);
        let strict = version_floor_with(
            &doc,
            FloorOptions::default().with_icc(IccVersionGate::Strict),
        )
        .floor;
        let declared = doc.version();
        if f.proven() && strict < declared {
            strict_lowerable += 1;
        }
        println!(
            "{}\t{}\t{}\t{}\t{}",
            declared,
            f.floor,
            strict,
            if f.proven() { "proven" } else { "bound" },
            path.display()
        );
        *pairs
            .entry((declared.to_string(), f.floor.to_string()))
            .or_default() += 1;
        if !f.proven() {
            unproven += 1;
        } else if f.floor < declared {
            lowerable += 1;
        }
        for r in f.requirements.iter().filter(|r| r.version == f.floor) {
            *setters
                .entry(format!("{} {}", r.version, r.construct))
                .or_default() += 1;
        }
    }
    println!(
        "# files {} unreadable {unreadable} unproven {unproven} lowerable(proven, floor<declared) {lowerable} strict-icc {strict_lowerable}",
        files.len()
    );
    for ((d, f), n) in &pairs {
        println!("# declared {d} floor {f}: {n}");
    }
    let mut top: Vec<_> = setters.into_iter().collect();
    top.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (what, n) in top.iter().take(40) {
        println!("# setter {n}\t{what}");
    }
}
