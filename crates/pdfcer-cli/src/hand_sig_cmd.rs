//! `list-hand-signatures`: the hand-signature marks on a document's pages
//! (`pdfcer_core::hand_sig`).

use super::*;

/// `list-hand-signatures` — one `page= field= bounds=` line per mark still
/// painting, then `total=`.
///
/// Exit codes: [`exit::EDIT_REFUSED`] for a `--page` outside the document,
/// [`exit::RUNTIME_ERROR`] for a page tree or content stream that cannot be
/// read, the open-failure codes of [`exit_code_for_doc`] otherwise.
pub(crate) fn cmd_list_hand_signatures(input: &Path, page: Option<usize>) -> u8 {
    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let range = match page {
        None => 0..pages.len(),
        Some(n) if (1..=pages.len()).contains(&n) => n - 1..n,
        Some(n) => {
            eprintln!(
                "pdfcer: {}: --page {n} is out of range (the document has {} page(s))",
                input.display(),
                pages.len()
            );
            return exit::EDIT_REFUSED;
        }
    };
    let view = doc.view();
    let mut total = 0_usize;
    for index in range {
        let marks = match pdfcer_core::hand_sig::hand_signatures(&view, &pages[index]) {
            Ok(marks) => marks,
            Err(err) => {
                eprintln!("pdfcer: {}: page {}: {err}", input.display(), index + 1);
                return exit::RUNTIME_ERROR;
            }
        };
        for m in &marks {
            let b = &m.bounds;
            println!(
                "page={} field={:?} bounds={:.3},{:.3},{:.3},{:.3}",
                index + 1,
                m.field,
                b.llx,
                b.lly,
                b.urx,
                b.ury
            );
        }
        total += marks.len();
    }
    println!("total={total}");
    exit::SUCCESS
}
