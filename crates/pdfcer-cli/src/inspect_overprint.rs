//! `inspect --overprint`: whether each page uses overprint, as Acrobat's
//! Output Preview reports it (`pdfcer_render::page_overprint`).

use super::*;

/// One line per selected page, `page=N overprint=set|none effect=0|1`,
/// then a totals line.
///
/// `overprint=set` is Acrobat's "Page has overprint: Yes": a graphics state
/// turning `/OP` or `/op` on was selected while the page was drawn.
/// `effect=1` is stricter: some paint under overprint left a colorant to
/// what was below it, so the page prints differently with overprint
/// ignored. A page can be `set` with `effect=0` (`DeviceCMYK` at `/OPM 0`
/// over process ink only). Honours `--pages` and the saved settings.
pub(crate) fn cmd_inspect_overprint(input: &Path, pages_spec: &str) -> u8 {
    let (doc, page_list, indices, options) = match open_selected(input, pages_spec) {
        Ok(opened) => opened,
        Err(code) => return code,
    };
    let view = doc.view();
    let (mut set, mut effect, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    println!("inspect --overprint {}", input.display());
    for &index in &indices {
        let Some(page) = page_list.get(index) else {
            continue;
        };
        match pdfcer_render::page_overprint(&view, page, &options) {
            Ok(found) => {
                if found.set {
                    set.push(index + 1);
                }
                if found.takes_effect {
                    effect.push(index + 1);
                }
                println!(
                    "page={} overprint={} effect={}",
                    index + 1,
                    if found.set { "set" } else { "none" },
                    u8::from(found.takes_effect)
                );
            }
            Err(err) => {
                failed.push(index + 1);
                println!("page={} overprint=unknown effect=unknown", index + 1);
                eprintln!("pdfcer: {}: page {}: {err}", input.display(), index + 1);
            }
        }
    }
    println!(
        "pages={} overprint_set={} overprint_effect={} unknown={}",
        indices.len(),
        set.len(),
        effect.len(),
        failed.len()
    );
    if failed.is_empty() {
        exit::SUCCESS
    } else {
        exit::RUNTIME_ERROR
    }
}
