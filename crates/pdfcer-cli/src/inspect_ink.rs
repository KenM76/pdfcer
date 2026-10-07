//! `inspect --ink`: whether each page composites in ink, read from its own
//! dictionary without rendering (`pdfcer_render::page_composites_in_ink`).

use super::*;

/// One line per selected page — `page=N ink=0|1 source=TOKEN` — then a
/// totals line. The answer honours the saved `page_blend_space_source`
/// setting, so it is the one `render-page` would reach; a page whose space
/// pdfcer took from the output intent gets a note, because that is an
/// inference (rule 4), not something the page declared.
pub(crate) fn cmd_inspect_ink(input: &Path, pages_spec: &str) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let page_list = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let indices = match parse_pages(pages_spec, page_list.len()) {
        Ok(indices) => indices,
        Err(message) => {
            eprintln!("pdfcer: {}: --pages {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (settings, settings_report) =
        pdfcer_core::settings::Settings::load(pdfcer_core::settings::resolve_store());
    report_settings(&settings_report);
    let options = pdfcer_render::RenderOptions::default()
        .with_page_blend_space_source(settings.page_blend_space_source);

    let view = doc.view();
    let (mut in_ink, mut inferred) = (0usize, Vec::new());
    println!("inspect --ink {}", input.display());
    for &index in &indices {
        let Some(page) = page_list.get(index) else {
            continue;
        };
        let ink = pdfcer_render::page_composites_in_ink(&view, page, &options);
        in_ink += usize::from(ink.composites_in_ink);
        if ink.source == pdfcer_render::interpret::BlendSpaceFrom::OutputIntent {
            inferred.push(index + 1);
        }
        println!(
            "page={} ink={} source={}",
            index + 1,
            u8::from(ink.composites_in_ink),
            ink.source.token()
        );
    }
    println!(
        "pages={} in_ink={in_ink} inferred_from_output_intent={}",
        indices.len(),
        inferred.len()
    );
    if !inferred.is_empty() {
        eprintln!(
            "pdfcer: note: page(s) {} declare no blending space; pdfcer took it from the \
document's output intent (ISO 32000-2 Annex P, informative). Set \
`page_blend_space_source` to `device_native` for ISO 32000-1's answer.",
            join_pages(&inferred)
        );
    }
    exit::SUCCESS
}

fn join_pages(pages: &[usize]) -> String {
    pages
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
